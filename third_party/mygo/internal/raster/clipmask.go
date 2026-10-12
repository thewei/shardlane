package raster

import (
	"github.com/egoist/mygo/internal/scene"
	"image"
)

// ClipMask updates a retained coverage image for nested clips in area.
// GPU renderers cache this independently of the content within a clip, so
// moving content under a stationary clip costs no CPU pixel work.
func ClipMask(dst *scene.Image, clips []scene.ClipShape, area image.Rectangle) *scene.Image {
	w, h := area.Dx(), area.Dy()
	if dst == nil {
		dst = scene.NewImageRGBA(w, h, make([]byte, 4*w*h))
	} else if dst.W != w || dst.H != h {
		dst.Resize(w, h)
	}
	prepared := make([]clip, len(clips))
	for i, c := range clips {
		prepared[i].shape = newShape(c.Rect, scene.Corners(c.Rect, c.Radii, c.Continuous))
		prepared[i].transform = c.Transform
		prepared[i].inverse, prepared[i].valid = c.Transform.Inverse()
	}
	for y := range h {
		for x := range w {
			wx, wy := float32(x+area.Min.X)+0.5, float32(y+area.Min.Y)+0.5
			cov := float32(1)
			for i := range prepared {
				c := &prepared[i]
				if !c.valid {
					cov = 0
					break
				}
				cov *= transformedCoverage(&c.shape, c.transform, c.inverse, wx, wy)
				if cov == 0 {
					break
				}
			}
			at := 4 * (y*w + x)
			dst.Pix[at], dst.Pix[at+1], dst.Pix[at+2], dst.Pix[at+3] = 0, 0, 0, to8(cov)
		}
	}
	dst.Changed()
	return dst
}
