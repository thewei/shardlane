//go:build windows

package d3d11

import (
	"runtime"
	"syscall"
	"testing"
	"time"
	"unsafe"

	"github.com/egoist/mygo/internal/gpu"
	"github.com/egoist/mygo/internal/gpu/gputest"
	"github.com/egoist/mygo/internal/scene"
)

// hiddenWindow creates a window that never shows, for a swap chain.
func hiddenWindow(t *testing.T, w, h int) uintptr {
	hwnd, err := newHiddenWindow(w, h)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { destroyWindow(hwnd) })
	return hwnd
}

// readBack copies the back buffer to memory: BGRA rows and their stride.
func (r *Renderer) readBack(t *testing.T) ([]byte, int) {
	pix, stride, err := r.read()
	if err != nil {
		t.Fatal(err)
	}
	return pix, stride
}

func TestDrawsAsTheCPURenderer(t *testing.T) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	// With the shaders compiled ahead of time, and with those compiled
	// from shader.hlsl when the renderer starts, as when they are older.
	for _, compile := range []bool{false, true} {
		compileShaders = compile
		s := gputest.Scene()
		r, err := New(hiddenWindow(t, s.Width, s.Height))
		compileShaders = false
		if err != nil {
			t.Skip("no Direct3D 11:", err)
		}
		// Twice: the second frame updates what the first uploaded.
		for frame := range 2 {
			if err := r.draw(s); err != nil {
				t.Fatal(err)
			}
			pix, stride := r.readBack(t)
			gputest.Compare(t, "d3d11", pix, stride, s)
			if err := r.present(1); err != nil {
				t.Fatalf("frame %d: %v", frame, err)
			}
		}
		r.Release()
	}
}

// TestResizeSettles checks that while the window changes size the swap
// chain shows part of larger buffers, resized only when the window
// outgrows them, and that once the settle timer asked for a frame, the
// frame gives it buffers of the window's size again.
func TestResizeSettles(t *testing.T) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	defer func(d time.Duration) { settleDelay = d }(settleDelay)
	settleDelay = 50 * time.Millisecond
	r, err := New(hiddenWindow(t, 200, 150))
	if err != nil {
		t.Skip("no Direct3D 11:", err)
	}
	defer r.Release()
	frame := func(w, h int) {
		t.Helper()
		if err := r.draw(&scene.Scene{Width: w, Height: h}); err != nil {
			t.Fatal(err)
		}
		if err := r.present(0); err != nil {
			t.Fatal(err)
		}
	}
	frame(200, 150)
	if r.swapChain2 == 0 {
		t.Skip("no IDXGISwapChain2")
	}
	if r.bw != 200 || r.bh != 150 || r.resizing {
		t.Fatalf("first frame: buffers %dx%d, resizing %v; want 200x150 and not resizing", r.bw, r.bh, r.resizing)
	}
	frame(220, 160)
	bw, bh := r.bw, r.bh
	if !r.resizing || bw <= 220 || bh <= 160 {
		t.Fatalf("resized: buffers %dx%d, resizing %v; want larger than 220x160 and resizing", bw, bh, r.resizing)
	}
	frame(230, 155)
	if r.bw != bw || r.bh != bh || r.w != 230 || r.h != 155 {
		t.Fatalf("resized within the buffers: buffers %dx%d showing %dx%d; want %dx%d showing 230x155", r.bw, r.bh, r.w, r.h, bw, bh)
	}
	// The timer fires as messages are dispatched.
	user32 := syscall.NewLazyDLL("user32.dll")
	peek, dispatch := user32.NewProc("PeekMessageW"), user32.NewProc("DispatchMessageW")
	var msg [64]byte
	for deadline := time.Now().Add(5 * time.Second); settling[r.hwnd]; time.Sleep(time.Millisecond) {
		if time.Now().After(deadline) {
			t.Fatal("the settle timer did not fire")
		}
		for {
			if ok, _, _ := peek.Call(uintptr(unsafe.Pointer(&msg)), 0, 0, 0, 1); ok == 0 {
				break
			}
			dispatch.Call(uintptr(unsafe.Pointer(&msg)))
		}
	}
	frame(230, 155)
	if r.bw != 230 || r.bh != 155 || r.resizing {
		t.Fatalf("settled: buffers %dx%d, resizing %v; want 230x155 and not resizing", r.bw, r.bh, r.resizing)
	}
}

// TestShaderBytecode checks that the shaders compiled ahead of time come
// from shader.hlsl as it is, and that the compiler renderers fall back to
// compiles it.
func TestShaderBytecode(t *testing.T) {
	if gpu.SourceSum(shaderSource) != shaderSum {
		t.Fatal("shader.hlsl changed since shaders.go was generated: run go generate ./internal/gpu/d3d11 on Windows")
	}
	for _, s := range []struct{ entry, target string }{{"vs", "vs_4_0"}, {"ps", "ps_4_0"}} {
		code, err := compileShader(s.entry, s.target)
		if err != nil {
			t.Fatal(err)
		}
		// DXBC starts with its magic.
		if len(code) < 4 || string(code[:4]) != "DXBC" {
			t.Errorf("%s: not DXBC", s.entry)
		}
	}
}
