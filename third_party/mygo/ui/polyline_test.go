package ui

import (
	"bytes"
	"math"
	"testing"
)

func polylineTester(points [][2]float32, fast bool, color Color, scale float32) *Tester {
	tt := coreNewTester(func(c *context) {
		coreBox(c).Fill().Draw(func(p *Painter, r Rect) {
			p.Clip(Rect{X: 5, Y: 5, W: 190, H: 90}, 0, func() {
				if fast {
					host := p.rt.host
					p.rt.host = &windowHost{onGPU: true}
					defer func() { p.rt.host = host }()
				}
				p.StrokePolyline(points, 3, color)
			})
		})
	}, 200, 100)
	tt.SetScale(scale)
	return tt
}

func TestStrokePolylineFallback(t *testing.T) {
	nan := float32(math.NaN())
	points := [][2]float32{{10, 20}, {30, 70}, {60, 20}, {60, 20}, {80, 60}, {nan, nan}, {110, 25}, {170, 60}}
	for _, color := range []Color{RGB(20, 60, 230), RGBA(20, 60, 230, .4)} {
		got := polylineTester(points, false, color, 2)
		want := coreNewTester(func(c *context) {
			coreBox(c).Fill().Draw(func(p *Painter, r Rect) {
				var path Path
				move := true
				for _, pt := range points {
					if math.IsNaN(float64(pt[0])) {
						move = true
						continue
					}
					if move {
						path.MoveTo(pt[0], pt[1])
						move = false
					} else {
						path.LineTo(pt[0], pt[1])
					}
				}
				p.Clip(Rect{X: 5, Y: 5, W: 190, H: 90}, 0, func() { p.StrokePath(&path, 3, color) })
			})
		}, 200, 100)
		want.SetScale(2)
		if !bytes.Equal(got.Image().Pix, want.Image().Pix) {
			t.Fatal("CPU fallback differs", color)
		}
		if color.A < 255 {
			fast := polylineTester(points, true, color, 2)
			if !bytes.Equal(fast.Image().Pix, want.Image().Pix) {
				t.Fatal("translucent GPU line accumulates alpha")
			}
		}
	}
}

func TestStrokePolylineCapsules(t *testing.T) {
	points := [][2]float32{{-10, 20}, {30, 70}, {60, 20}, {80, 60}, {float32(math.Inf(1)), 0}, {110, 25}, {170, 60}, {210, 20}}
	for _, scale := range []float32{1, 2} {
		tt := polylineTester(points, true, RGB(20, 60, 230), scale)
		if n := countBlue(tt.Image()); n < 500*int(scale*scale) {
			t.Fatal("missing line", n)
		}
		img := tt.Image()
		for y := 0; y < img.Rect.Dy(); y++ {
			for x := 0; x < img.Rect.Dx(); x++ {
				c := img.RGBAAt(x, y)
				if (x < int(5*scale) || x >= int(195*scale) || y < int(5*scale) || y >= int(95*scale)) && c.B > 180 && c.R < 120 {
					t.Fatal("line escapes clip", x, y)
				}
			}
		}
	}
}

func TestStrokePolylineInvalid(t *testing.T) {
	for _, points := range [][][2]float32{nil, {{20, 20}}, {{float32(math.NaN()), 0}}, {{10, 10}, {float32(math.Inf(1)), 20}, {30, 30}}} {
		if n := countBlue(polylineTester(points, true, RGB(20, 60, 230), 1).Image()); n != 0 {
			t.Fatal("isolated point draws", n)
		}
	}
}

func TestStrokePolylineOutline(t *testing.T) {
	points := make([][2]float32, 100)
	for i := range points {
		points[i] = [2]float32{10 + float32(i)*1.8, 50 + 25*float32(math.Sin(float64(i)/3))}
	}
	for _, scale := range []float32{1, 2} {
		outline := polylineTester(points, false, RGB(20, 60, 230), scale).Image()
		capsules := polylineTester(points, true, RGB(20, 60, 230), scale).Image()
		diff := 0
		for i, v := range outline.Pix {
			diff += int(math.Abs(float64(int(v) - int(capsules.Pix[i]))))
		}
		// Capsule edges blend independently; interiors and geometry agree.
		if float64(diff)/float64(len(outline.Pix)) > 3 {
			t.Fatal("capsules change the outline", diff)
		}
	}
}
