//go:build linux && (amd64 || arm64)

package linux

import (
	"unsafe"

	"github.com/ebitengine/purego"

	"github.com/egoist/mygo/internal/platform"
)

// Files dragged over native UI arrive as a uri-list. The content answers
// whether it takes them where they are, which GTK shows the drag source;
// their paths are read only once they are dropped.

var (
	gtkDragDestSet        func(widget ptr, flags int32, targets unsafe.Pointer, n int32, actions int32)
	gtkDragDestFindTarget func(widget, context, targets ptr) ptr
	gtkDragGetData        func(widget, context, target ptr, time uint32)
	gtkDragFinish         func(context ptr, success, del bool, time uint32)
	gdkDragStatus         func(context ptr, action int32, time uint32)

	cbSurfaceDragMotion, cbSurfaceDragLeave, cbSurfaceDragDrop, cbSurfaceDragData ptr
)

const gdkActionCopy = 1 << 1

// gtkTargetEntry is GtkTargetEntry.
type gtkTargetEntry struct {
	target      *byte
	flags, info uint32
}

func loadDrops() {
	mustBind(libGTK, &gtkDragDestSet, "gtk_drag_dest_set")
	mustBind(libGTK, &gtkDragDestFindTarget, "gtk_drag_dest_find_target")
	mustBind(libGTK, &gtkDragGetData, "gtk_drag_get_data")
	mustBind(libGTK, &gtkDragFinish, "gtk_drag_finish")
	mustBind(libGDK, &gdkDragStatus, "gdk_drag_status")
	b := func() *Backend { return theBackend }
	cbSurfaceDragMotion = purego.NewCallback(func(widget, context ptr, x, y int32, time uint32, data ptr) bool {
		s := b().surfaceOf(data)
		if s == nil || gtkDragDestFindTarget(widget, context, 0) == 0 {
			return false
		}
		var action int32
		if s.fileDragOver(float64(x), float64(y)) {
			action = gdkActionCopy
		}
		gdkDragStatus(context, action, time)
		return true
	})
	// GTK leaves before it drops, as when the files leave.
	cbSurfaceDragLeave = purego.NewCallback(func(widget, context ptr, time uint32, data ptr) {
		if s := b().surfaceOf(data); s != nil {
			s.send(platform.SurfaceEvent{Kind: platform.FileDragLeave})
		}
	})
	cbSurfaceDragDrop = purego.NewCallback(func(widget, context ptr, x, y int32, time uint32, data ptr) bool {
		target := gtkDragDestFindTarget(widget, context, 0)
		if b().surfaceOf(data) == nil || target == 0 {
			return false
		}
		gtkDragGetData(widget, context, target, time)
		return true
	})
	cbSurfaceDragData = purego.NewCallback(func(widget, context ptr, x, y int32, sel ptr, info, time uint32, data ptr) {
		s := b().surfaceOf(data)
		paths := selectionPaths(sel)
		gtkDragFinish(context, s != nil && len(paths) > 0 && s.dropFiles(float64(x), float64(y), paths), false, time)
	})
}

// acceptFileDrops makes the surface a destination of dragged files.
func (s *surface) acceptFileDrops(data ptr) {
	target := gtkTargetEntry{target: cs("text/uri-list")}
	gtkDragDestSet(s.area, 0, unsafe.Pointer(&target), 1, gdkActionCopy)
	connect(s.area, "drag-motion", cbSurfaceDragMotion, data)
	connect(s.area, "drag-leave", cbSurfaceDragLeave, data)
	connect(s.area, "drag-drop", cbSurfaceDragDrop, data)
	connect(s.area, "drag-data-received", cbSurfaceDragData, data)
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

// selectionPaths returns the paths of the files of a uri-list.
func selectionPaths(sel ptr) []string {
	uris := gtkSelectionDataGetUris(sel)
	if uris == 0 {
		return nil
	}
	defer gStrfreev(uris)
	var paths []string
	for i := uintptr(0); ; i++ {
		uri := *(*ptr)(unsafe.Add(*(*unsafe.Pointer)(unsafe.Pointer(&uris)), i*unsafe.Sizeof(uris)))
		if uri == 0 {
			break
		}
		if p := takeStr(gFilenameFromURI(goStr(uri), 0, 0)); p != "" {
			paths = append(paths, p)
		}
	}
	return paths
}
