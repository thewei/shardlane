//go:build darwin

package metal

import (
	"testing"

	"github.com/egoist/mygo/internal/gpu/gputest"
)

// Canvases draw into textures as the CPU renderer draws them into pixels,
// once each: the second frame draws none anew, and shows them the same.
func TestCanvases(t *testing.T) {
	r, err := newRenderer()
	if err != nil {
		t.Skip("no Metal:", err)
	}
	defer r.Release()
	s := gputest.CanvasScene()
	for frame := range 2 {
		pix, err := r.renderOffscreen(s)
		if err != nil {
			t.Fatal(err)
		}
		gputest.Compare(t, "metal-canvas", pix, s.Width*4, s)
		if frame == 0 {
			s.Canvases = nil // drawn: only shown from now on
		}
	}
	if len(r.canvases) != 2 {
		t.Errorf("%d canvas textures, want 2", len(r.canvases))
	}
}
