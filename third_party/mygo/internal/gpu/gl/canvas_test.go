//go:build linux && (amd64 || arm64)

package gl

import (
	"testing"

	"github.com/egoist/mygo/internal/gpu/gputest"
)

// Canvases draw into textures as the CPU renderer draws them into pixels,
// shown the right way up although GL's rows go up, and once each.
func TestCanvases(t *testing.T) {
	for _, api := range apis {
		t.Run(api.name, func(t *testing.T) {
			s := gputest.CanvasScene()
			read := offscreen(t, api.es, s.Width, s.Height)
			r, err := New()
			if err != nil {
				t.Fatal(err)
			}
			defer r.Release()
			for frame := range 2 {
				if err := r.Render(s); err != nil {
					t.Fatal(err)
				}
				gputest.Compare(t, "gl-canvas", read(), s.Width*4, s)
				if frame == 0 {
					s.Canvases = nil
				}
			}
			if len(r.canvases) != 2 {
				t.Errorf("%d canvas textures, want 2", len(r.canvases))
			}
		})
	}
}
