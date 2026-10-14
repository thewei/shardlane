package ui

import (
	"image/color"
	"testing"
)

func TestPainterTransform(t *testing.T) {
	red, blue := RGB(220, 20, 30), RGB(30, 60, 220)
	tt := coreNewTester(func(c *context) {
		coreBox(c).Size(200, 100).Draw(func(p *Painter, r Rect) {
			// Moved right 50: a red square at 50..70.
			p.Transform(Translation(50, 0), 0, 0, func() {
				p.Fill(Rect{X: r.X, Y: r.Y, W: 20, H: 20}, red, 0)
			})
			// A quarter turn around its center, nested in twice as large
			// around the corner (100, 0).
			p.Transform(Scaling(2, 2), r.X+100, r.Y, func() {
				p.Transform(Rotation(90), r.X+110, r.Y+10, func() {
					p.Fill(Rect{X: r.X + 100, Y: r.Y + 5, W: 20, H: 10}, blue, 0)
				})
			})
		})
	}, 200, 100)
	img := tt.Image()
	at := func(x, y int) color.RGBA { return img.RGBAAt(x, y) }
	if c := at(60, 10); c != (color.RGBA{220, 20, 30, 255}) {
		t.Errorf("translated square at 60 = %v", c)
	}
	if c := at(10, 10); c.R == 220 && c.G == 20 {
		t.Error("the square is still where it was drawn")
	}
	// The 20×10 bar turned upright around (110, 10) covers 105..115 by
	// 0..20, then twice as large around (100, 0): 110..130 by 0..40.
	if c := at(120, 30); c != (color.RGBA{30, 60, 220, 255}) {
		t.Errorf("scaled and turned bar at (120, 30) = %v", c)
	}
	if c := at(140, 10); c.B == 220 {
		t.Errorf("the bar reaches (140, 10): %v", c)
	}
}

// An image the transform moves out of view is culled; one it moves into
// view is drawn.
func TestPainterTransformCulls(t *testing.T) {
	bm := NewBitmap(stripedImage(4, 4, 1, 0))
	tt := coreNewTester(func(c *context) {
		coreBox(c).Size(100, 100).Draw(func(p *Painter, r Rect) {
			p.Transform(Translation(-1000, 0), 0, 0, func() {
				p.ImageRegion(bm, Rect{}, Rect{X: r.X, Y: r.Y, W: 10, H: 10}, ImageOptions{})
			})
			p.Transform(Translation(1000, 0), 0, 0, func() {
				p.ImageRegion(bm, Rect{}, Rect{X: r.X - 1000, Y: r.Y, W: 10, H: 10}, ImageOptions{})
			})
		})
	}, 100, 100)
	images := 0
	for _, op := range tt.h.last.Ops {
		if op.Image != nil {
			images++
		}
	}
	if images != 1 {
		t.Errorf("%d images drawn, want the one in view", images)
	}
}
