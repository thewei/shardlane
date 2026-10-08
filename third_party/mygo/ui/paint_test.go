package ui

import (
	"bytes"
	"image"
	"math"
	"testing"

	"github.com/egoist/mygo/internal/raster"
)

// countBlue counts the pixels of img that are mostly blue.
func countBlue(img *image.RGBA) int {
	n := 0
	for i := 0; i < len(img.Pix); i += 4 {
		if p := img.Pix[i:]; p[2] > 180 && p[0] < 120 && p[1] < 120 {
			n++
		}
	}
	return n
}

func TestAnimatedPathShowsInEveryFrame(t *testing.T) {
	n := 0
	view := func(c *Context) {
		Column(c).Fill().Children(func() {
			Text(c, "Wave")
			Box(c).Grow(1).FillWidth().Draw(func(p *Painter, r Rect) {
				var path Path
				for i := 0; i <= 100; i++ {
					x := r.X + 10 + float32(i)*(r.W-20)/100
					y := r.Y + r.H/2 + float32(math.Sin(float64(i)/8+float64(n)/5))*(r.H/2-20)
					if i == 0 {
						path.MoveTo(x, y)
					} else {
						path.LineTo(x, y)
					}
				}
				p.StrokePath(&path, 3, RGB(20, 60, 230))
			})
		})
	}
	tt := NewTester(view, 600, 400)
	tt.SetScale(2)
	// Each frame draws another wave: the atlas must not fill up with the
	// old ones and leave a frame without its wave.
	for ; n < 80; n++ {
		tt.Frame()
		if ink := countBlue(tt.Image()); ink < 2000 {
			t.Fatalf("frame %d shows %d pixels of the wave", n, ink)
		}
		if !tt.HasText("Wave") {
			t.Fatalf("frame %d lost its text", n)
		}
	}
}

func TestFramesStayWholeWhenTheAtlasFills(t *testing.T) {
	view := func(c *Context) {
		Box(c).Fill().Draw(func(p *Painter, r Rect) {
			// 48 different discs, together larger than the atlas starts.
			for i := 0; i < 48; i++ {
				var path Path
				cx := r.X + 60 + float32(i%8)*110
				cy := r.Y + 60 + float32(i/8)*110
				path.Circle(cx, cy, 40+float32(i)/4)
				p.FillPath(&path, RGB(20, 60, 230))
			}
		})
	}
	tt := NewTester(view, 900, 700)
	tt.SetScale(2)
	want := bytes.Clone(tt.Image().Pix)
	if countBlue(tt.Image()) < 48*3000 {
		t.Fatalf("the first frame shows %d pixels of discs", countBlue(tt.Image()))
	}
	for i := 0; i < 4; i++ {
		tt.Frame()
		if !bytes.Equal(tt.Image().Pix, want) {
			t.Fatalf("frame %d differs from the first: %d pixels of discs", i+2, countBlue(tt.Image()))
		}
	}
}

func TestFramesRedrawOnlyWhatChanged(t *testing.T) {
	d := &demo{choice: "a", size: "Medium", volume: 40}
	tt := NewTester(d.view, 640, 600)
	tt.SetScale(2)
	full := raster.NewImage(1, 1)
	check := func(what string) {
		t.Helper()
		s := &tt.rt.scene
		full.Resize(s.Width, s.Height)
		raster.Render(full, s)
		if !bytes.Equal(tt.h.img.Image.Pix, full.Pix) {
			t.Fatalf("after %s, the frame differs from a whole drawing", what)
		}
	}
	check("the first frame")
	r, _ := tt.Find("Increment")
	tt.Move(r.X+5, r.Y+5)
	check("hovering a button")
	tt.Click("Increment")
	check("a click")
	tt.Click("I agree")
	check("a check box")
	in, _ := tt.Find("I agree")
	tt.ClickAt(in.X+20, in.Y+in.H/2+40)
	tt.Type("Ada")
	check("typing")
	tt.Key(0, KeyTab)
	check("moving the focus")
	row, _ := tt.Find("Row 3")
	tt.Scroll(row.X+10, row.Y+10, 0, 100)
	check("scrolling")
	tt.Click("Row 7")
	check("selecting a row")
	tt.SetDark(true)
	check("going dark")
}
