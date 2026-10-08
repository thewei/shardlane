//go:build windows && (amd64 || arm64)

package windows

import (
	"sync"
	"syscall"
	"unsafe"

	"github.com/egoist/mygo/internal/platform"
)

// Files dragged over native UI come through OLE drag and drop: the
// surface's window is a drop target, which tells the source whether the
// content takes the files where they are.

var (
	procOleInitialize    = ole32.NewProc("OleInitialize")
	procRegisterDragDrop = ole32.NewProc("RegisterDragDrop")
	procRevokeDragDrop   = ole32.NewProc("RevokeDragDrop")
	procReleaseStgMedium = ole32.NewProc("ReleaseStgMedium")
	procDragQueryFileW   = shell32.NewProc("DragQueryFileW")

	dropOnce       sync.Once
	dropTargetVtbl [7]uintptr
	dropTargets    = map[uintptr]*dropTarget{}
	iidIDropTarget = guid("00000122-0000-0000-c000-000000000046")
	iidIUnknown    = guid("00000000-0000-0000-c000-000000000046")
)

const (
	cfHDrop          = 15
	dvaspectContent  = 1
	tymedHGlobal     = 1
	dropEffectNone   = 0
	dropEffectCopy   = 1
	dropEffectLink   = 4
	dataGetData      = 3 // IDataObject::GetData
	dataQueryGetData = 5 // IDataObject::QueryGetData
)

// dropTarget is the IDropTarget of a surface.
type dropTarget struct {
	vtbl  *[7]uintptr
	refs  int32
	s     *surface
	files bool // whether what is dragged over has files
}

// formatEtc is FORMATETC.
type formatEtc struct {
	Format uint16
	Device uintptr
	Aspect uint32
	Index  int32
	Tymed  uint32
}

// stgMedium is STGMEDIUM.
type stgMedium struct {
	Tymed         uint32
	Handle        uintptr
	UnkForRelease uintptr
}

var hdropFormat = formatEtc{Format: cfHDrop, Aspect: dvaspectContent, Index: -1, Tymed: tymedHGlobal}

func initDropTarget() {
	procOleInitialize.Call(0)
	target := func(this uintptr) *dropTarget { return dropTargets[this] }
	dropTargetVtbl = [7]uintptr{
		syscall.NewCallback(func(this, riid, out uintptr) uintptr {
			if iid := *(*GUID)(native(riid)); iid != iidIUnknown && iid != iidIDropTarget {
				*(*uintptr)(native(out)) = 0
				return eNoInterface
			}
			*(*uintptr)(native(out)) = this
			target(this).refs++
			return sOK
		}),
		syscall.NewCallback(func(this uintptr) uintptr {
			t := target(this)
			t.refs++
			return uintptr(t.refs)
		}),
		syscall.NewCallback(func(this uintptr) uintptr {
			t := target(this)
			if t.refs--; t.refs == 0 {
				delete(dropTargets, this)
			}
			return uintptr(t.refs)
		}),
		// DragEnter(data, keys, point, effect)
		syscall.NewCallback(func(this, data, keys, pt, effect uintptr) uintptr {
			t := target(this)
			t.files = comCall(data, dataQueryGetData, uintptr(unsafe.Pointer(&hdropFormat))) == sOK
			t.over(pt, effect)
			return sOK
		}),
		// DragOver(keys, point, effect)
		syscall.NewCallback(func(this, keys, pt, effect uintptr) uintptr {
			target(this).over(pt, effect)
			return sOK
		}),
		// DragLeave()
		syscall.NewCallback(func(this uintptr) uintptr {
			t := target(this)
			if t.files {
				t.files = false
				t.s.send(platform.SurfaceEvent{Kind: platform.FileDragLeave})
			}
			return sOK
		}),
		// Drop(data, keys, point, effect)
		syscall.NewCallback(func(this, data, keys, pt, effect uintptr) uintptr {
			t := target(this)
			t.files = false
			paths := droppedPaths(data)
			x, y := t.s.screenDIP(pt)
			allowed := *(*uint32)(native(effect))
			*(*uint32)(native(effect)) = dropEffectNone
			if len(paths) > 0 && t.s.dropFiles(x, y, paths) {
				*(*uint32)(native(effect)) = acceptEffect(allowed)
			}
			return sOK
		}),
	}
}

// acceptFileDrops makes the surface's window a drop target.
func (s *surface) acceptFileDrops() {
	dropOnce.Do(initDropTarget)
	t := &dropTarget{vtbl: &dropTargetVtbl, refs: 1, s: s}
	p := uintptr(unsafe.Pointer(t))
	dropTargets[p] = t
	procRegisterDragDrop.Call(s.hwnd, p) // takes its own reference
	s.dropTarget = p
}

func (s *surface) revokeFileDrops() {
	if s.dropTarget != 0 {
		procRevokeDragDrop.Call(s.hwnd)
		release(s.dropTarget)
		s.dropTarget = 0
	}
}

// over answers whether the content takes the files dragged to a point of
// the screen, by the effect it lets the drop have.
func (t *dropTarget) over(pt, effect uintptr) {
	allowed := *(*uint32)(native(effect))
	*(*uint32)(native(effect)) = dropEffectNone
	if !t.files {
		return
	}
	if x, y := t.s.screenDIP(pt); t.s.fileDragOver(x, y) {
		*(*uint32)(native(effect)) = acceptEffect(allowed)
	}
}

// acceptEffect returns the effect of a drop the content takes, of those
// the source allows: never a move, which would delete the files.
func acceptEffect(allowed uint32) uint32 {
	switch {
	case allowed&dropEffectCopy != 0:
		return dropEffectCopy
	case allowed&dropEffectLink != 0:
		return dropEffectLink
	}
	return dropEffectNone
}

// screenDIP converts a POINTL of the screen, passed by value, to DIPs
// relative to the surface.
func (s *surface) screenDIP(pt uintptr) (float64, float64) {
	p := point{int32(uint32(pt)), int32(uint32(pt >> 32))}
	procScreenToClient.Call(s.hwnd, uintptr(unsafe.Pointer(&p)))
	return s.toDIP(p.X), s.toDIP(p.Y)
}

// fileDragOver reports whether the content takes files dragged to (x, y).
func (s *surface) fileDragOver(x, y float64) bool {
	return s.send(platform.SurfaceEvent{Kind: platform.FileDragOver, X: x, Y: y})
}

// dropFiles drops files at (x, y), and reports whether the content took
// them.
func (s *surface) dropFiles(x, y float64, paths []string) bool {
	return s.send(platform.SurfaceEvent{Kind: platform.FileDrop, X: x, Y: y, Files: paths})
}

// droppedPaths returns the paths of the files of a data object.
func droppedPaths(data uintptr) []string {
	format := hdropFormat
	var m stgMedium
	if comCall(data, dataGetData, uintptr(unsafe.Pointer(&format)), uintptr(unsafe.Pointer(&m))) != sOK {
		return nil
	}
	defer procReleaseStgMedium.Call(uintptr(unsafe.Pointer(&m)))
	n, _, _ := procDragQueryFileW.Call(m.Handle, 0xFFFFFFFF, 0, 0)
	var paths []string
	for i := range n {
		size, _, _ := procDragQueryFileW.Call(m.Handle, i, 0, 0)
		buf := make([]uint16, size+1)
		procDragQueryFileW.Call(m.Handle, i, uintptr(unsafe.Pointer(&buf[0])), size+1)
		if p := syscall.UTF16ToString(buf); p != "" {
			paths = append(paths, p)
		}
	}
	return paths
}
