//go:build windows

package d3d11

import (
	"runtime"
	"testing"

	"github.com/egoist/mygo/internal/gpu/gputest"
)

// Canvases draw into textures as the CPU renderer draws them into pixels,
// once each: the second frame draws none anew, and shows them the same.
func TestCanvases(t *testing.T) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	s := gputest.CanvasScene()
	r, err := New(hiddenWindow(t, s.Width, s.Height))
	if err != nil {
		t.Skip("no Direct3D 11:", err)
	}
	defer r.Release()
	for frame := range 2 {
		if err := r.draw(s); err != nil {
			t.Fatal(err)
		}
		pix, stride := r.readBack(t)
		gputest.Compare(t, "d3d11-canvas", pix, stride, s)
		if err := r.present(1); err != nil {
			t.Fatal(err)
		}
		if frame == 0 {
			s.Canvases = nil
		}
	}
	if len(r.canvases) != 2 {
		t.Errorf("%d canvas textures, want 2", len(r.canvases))
	}
}
