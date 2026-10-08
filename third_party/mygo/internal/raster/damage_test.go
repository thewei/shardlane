package raster

import (
	"bytes"
	"fmt"
	"image"
	"math/rand/v2"
	"testing"

	"github.com/egoist/mygo/internal/scene"
)

// sceneMaker builds random scenes and changes them a little at a time.
type sceneMaker struct {
	rnd   *rand.Rand
	atlas *scene.Atlas
	img   *scene.Image
	// masks are rectangles of the atlas holding something.
	masks [][4]uint16
	// effects has op make effects too.
	effects bool
}

func newSceneMaker(seed uint64) *sceneMaker {
	m := &sceneMaker{rnd: rand.New(rand.NewPCG(seed, 7)), atlas: scene.NewAtlas(1, 128, 128)}
	for range 6 {
		w, h := 4+m.rnd.IntN(12), 4+m.rnd.IntN(12)
		x, y, ok := m.atlas.Alloc(w, h)
		if !ok {
			break
		}
		m.atlas.Put(x, y, w, h, m.pixels(w*h), w)
		m.masks = append(m.masks, [4]uint16{uint16(x), uint16(y), uint16(w), uint16(h)})
	}
	m.img = scene.NewImageRGBA(8, 8, m.pixels(8*8*4))
	return m
}

func (m *sceneMaker) pixels(n int) []byte {
	p := make([]byte, n)
	for i := range p {
		p[i] = byte(m.rnd.IntN(256))
	}
	return p
}

func (m *sceneMaker) color() scene.Color {
	return scene.Color{R: uint8(m.rnd.IntN(256)), G: uint8(m.rnd.IntN(256)), B: uint8(m.rnd.IntN(256)), A: uint8(64 + m.rnd.IntN(192))}
}

func (m *sceneMaker) rect() scene.Rect {
	return scene.Rect{X: m.rnd.Float32()*150 - 10, Y: m.rnd.Float32()*110 - 10, W: 2 + m.rnd.Float32()*60, H: 2 + m.rnd.Float32()*50}
}

// op returns a random operation that draws.
func (m *sceneMaker) op(s *scene.Scene) scene.Op {
	if m.effects && m.rnd.IntN(4) == 0 {
		r := m.rnd.Float32() * 20
		fx := scene.EffectOp{Effect: lensEffect, Blur: float32(m.rnd.IntN(4)) * 3}
		if m.rnd.IntN(3) == 0 {
			fx.Effect = tintEffect
		}
		fx.Params[0] = [4]float32{m.rnd.Float32() * 4}
		c := m.color()
		fx.Params[1] = [4]float32{float32(c.R) / 255, float32(c.G) / 255, float32(c.B) / 255, float32(c.A) / 255}
		s.Effects = append(s.Effects, fx)
		return scene.Op{Kind: scene.OpEffect, Rect: m.rect(), Radii: [4]float32{r, r, r, r}, Start: int32(len(s.Effects) - 1)}
	}
	switch m.rnd.IntN(4) {
	case 0:
		r := m.rnd.Float32() * 12
		return scene.Op{Kind: scene.OpFill, Rect: m.rect(), Radii: [4]float32{r, r, r, r}, Color: m.color(),
			Border: scene.Uniform(float32(m.rnd.IntN(3))), BorderColor: m.color()}
	case 1:
		return scene.Op{Kind: scene.OpShadow, Rect: m.rect(), Radii: [4]float32{4, 4, 4, 4}, Color: m.color(), Blur: m.rnd.Float32() * 16}
	case 2:
		start := int32(len(s.Glyphs))
		for range 1 + m.rnd.IntN(3) {
			k := m.masks[m.rnd.IntN(len(m.masks))]
			s.Glyphs = append(s.Glyphs, scene.Glyph{X: float32(m.rnd.IntN(150)), Y: float32(m.rnd.IntN(110)),
				W: float32(k[2]), H: float32(k[3]), U: k[0], V: k[1], UW: k[2], VH: k[3], Color: m.color()})
		}
		return scene.Op{Kind: scene.OpGlyphs, Start: start, End: int32(len(s.Glyphs))}
	default:
		return scene.Op{Kind: scene.OpImage, Rect: m.rect(), Image: m.img, Src: scene.Rect{W: 8, H: 8}}
	}
}

func (m *sceneMaker) scene() *scene.Scene {
	s := &scene.Scene{Width: 160, Height: 120, Clear: scene.Color{R: 250, G: 250, B: 250, A: 255}, MaskAtlas: m.atlas}
	for range 8 {
		s.Ops = append(s.Ops, m.op(s))
	}
	s.Ops = append(s.Ops, scene.Op{Kind: scene.OpPushClip, Rect: m.rect(), Radii: [4]float32{6, 6, 6, 6}})
	for range 5 {
		s.Ops = append(s.Ops, m.op(s))
	}
	s.Ops = append(s.Ops, scene.Op{Kind: scene.OpPopClip})
	for range 4 {
		s.Ops = append(s.Ops, m.op(s))
	}
	return s
}

// change changes s a little, as a frame of an app would, and says how.
func (m *sceneMaker) change(s *scene.Scene) string {
	// Drawing operations, which can change freely, unlike clips.
	var drawing []int
	for i, op := range s.Ops {
		if op.Kind != scene.OpPushClip && op.Kind != scene.OpPopClip {
			drawing = append(drawing, i)
		}
	}
	i := drawing[m.rnd.IntN(len(drawing))]
	switch m.rnd.IntN(8) {
	case 0:
		s.Ops[i].Color = m.color()
		return "color"
	case 1:
		if s.Ops[i].Kind == scene.OpGlyphs {
			g := &s.Glyphs[s.Ops[i].Start]
			g.X += 3
			return "glyph moved"
		}
		s.Ops[i].Rect.X += m.rnd.Float32()*20 - 10
		return "moved"
	case 2:
		s.Ops = append(s.Ops[:i+1], s.Ops[i:]...)
		s.Ops[i] = m.op(s)
		return "inserted"
	case 3:
		if len(drawing) > 4 {
			s.Ops = append(s.Ops[:i], s.Ops[i+1:]...)
			return "deleted"
		}
		return "nothing"
	case 4:
		for j := range s.Ops {
			if s.Ops[j].Kind == scene.OpPushClip {
				s.Ops[j].Rect = m.rect()
			}
		}
		return "clip moved"
	case 5:
		// New pixels where glyphs are, as a transient mask of the next
		// frame takes the place of the last one's.
		k := m.masks[m.rnd.IntN(len(m.masks))]
		m.atlas.Put(int(k[0]), int(k[1]), int(k[2]), int(k[3]), m.pixels(int(k[2])*int(k[3])), int(k[2]))
		return "atlas pixels"
	case 6:
		copy(m.img.Pix, m.pixels(len(m.img.Pix)))
		m.img.Changed()
		return "image pixels"
	default:
		return "nothing"
	}
}

func TestRendererRedrawsWhatChanged(t *testing.T) {
	testRedraws(t, false)
}

// TestRendererRedrawsEffects checks that effects, and what is under those
// reading their backdrops, show however little of the window changes.
func TestRendererRedrawsEffects(t *testing.T) {
	testRedraws(t, true)
}

func testRedraws(t *testing.T, effects bool) {
	for seed := range uint64(40) {
		m := newSceneMaker(seed)
		m.effects = effects
		s := m.scene()
		var r Renderer
		full := NewImage(s.Width, s.Height)
		var partial int
		for step := range 30 {
			what := "first"
			if step > 0 {
				what = m.change(s)
			}
			damage := r.Render(s)
			Render(full, s)
			if !bytes.Equal(r.Image.Pix, full.Pix) {
				writePNG(t, fmt.Sprintf("damage-%d-%d", seed, step), &r.Image)
				writePNG(t, fmt.Sprintf("damage-%d-%d-want", seed, step), full)
				t.Fatalf("seed %d, step %d (%s): the redrawn %v differs from a whole drawing", seed, step, what, damage)
			}
			if step > 0 && (len(damage) != 1 || damage[0].Dx() != s.Width || damage[0].Dy() != s.Height) {
				partial++
			}
		}
		if partial == 0 {
			t.Errorf("seed %d: every frame was drawn whole", seed)
		}
	}
}

func TestRendererSkipsUnchangedScenes(t *testing.T) {
	m := newSceneMaker(1)
	s := m.scene()
	var r Renderer
	r.Render(s)
	if damage := r.Render(s); len(damage) != 0 {
		t.Errorf("an unchanged scene redrew %v", damage)
	}
	s.Ops[3].Color = scene.Color{R: 1, A: 255}
	damage := r.Render(s)
	if len(damage) != 1 || damage[0].Dx()*damage[0].Dy() >= s.Width*s.Height/2 {
		t.Errorf("one changed operation redrew %v", damage)
	}
}

// TestChanges checks that Changes tells how much Render then draws.
func TestChanges(t *testing.T) {
	fill := func(x float32) *scene.Scene {
		return &scene.Scene{Width: 200, Height: 100, Clear: scene.Color{R: 255, G: 255, B: 255, A: 255}, Ops: []scene.Op{
			{Kind: scene.OpFill, Rect: scene.Rect{X: 10, Y: 10, W: 50, H: 50}, Color: scene.Color{R: 255, A: 255}},
			{Kind: scene.OpFill, Rect: scene.Rect{X: x, Y: 70, W: 10, H: 10}, Color: scene.Color{B: 255, A: 255}},
		}}
	}
	var r Renderer
	if n, c := r.Changes(fill(100)); n != 200*100 || c != n {
		t.Errorf("before a first scene: %d pixels, %d changed", n, c)
	}
	r.Render(fill(100))
	if n, c := r.Changes(fill(100)); n != 0 || c != 0 {
		t.Errorf("the same scene: %d pixels, %d changed", n, c)
	}
	n, c := r.Changes(fill(120))
	if n == 0 || n > 2*12*12 || c != n {
		t.Errorf("a small square moved: %d pixels, %d changed", n, c)
	}
	area := 0
	for _, d := range r.Render(fill(120)) {
		area += d.Dx() * d.Dy()
	}
	if area != n {
		t.Errorf("Render drew %d pixels, Changes said %d", area, n)
	}
}

// TestSkip checks that scenes shown without the Renderer, as the GPU draws
// them, are compared with the next and drawn with it.
func TestSkip(t *testing.T) {
	fill := func(x, y float32) *scene.Scene {
		return &scene.Scene{Width: 200, Height: 100, Clear: scene.Color{R: 255, G: 255, B: 255, A: 255}, Ops: []scene.Op{
			{Kind: scene.OpFill, Rect: scene.Rect{X: 10, Y: 10, W: 50, H: 50}, Color: scene.Color{R: 255, A: 255}},
			{Kind: scene.OpFill, Rect: scene.Rect{X: x, Y: y, W: 10, H: 10}, Color: scene.Color{B: 255, A: 255}},
		}}
	}
	same := func(r *Renderer, s *scene.Scene) bool {
		want := NewImage(s.Width, s.Height)
		Render(want, s)
		return bytes.Equal(r.Image.Pix, want.Pix)
	}
	var r Renderer
	r.Render(fill(100, 70))
	// The GPU moves the square twice; the next frame does not move it.
	r.Skip(fill(120, 70))
	r.Skip(fill(140, 70))
	n, c := r.Changes(fill(140, 70))
	if c != 0 || n == 0 || n > 3*14*14 {
		t.Errorf("after two skipped scenes: %d pixels to draw, %d changed", n, c)
	}
	area := 0
	for _, d := range r.Render(fill(140, 70)) {
		area += d.Dx() * d.Dy()
	}
	if area != n || !same(&r, fill(140, 70)) {
		t.Errorf("Render drew %d pixels, Changes said %d; the image is right: %v", area, n, same(&r, fill(140, 70)))
	}
	if n, c := r.Changes(fill(140, 70)); n != 0 || c != 0 {
		t.Errorf("drawn again: %d pixels, %d changed", n, c)
	}
	// A skipped scene changing most of the window, as a page sliding in,
	// has the next Render draw everything, though little changes since.
	moved := fill(140, 70)
	moved.Ops[0].Rect = scene.Rect{X: 0, Y: 0, W: 200, H: 100}
	r.Skip(moved)
	next := fill(150, 70)
	next.Ops[0].Rect = moved.Ops[0].Rect
	if n, c := r.Changes(next); n != 200*100 || c == 0 || c > 2*12*12 {
		t.Errorf("after a scene changing most of the window: %d pixels to draw, %d changed", n, c)
	}
	r.Render(next)
	if !same(&r, next) {
		t.Error("the image is not the scene drawn after a whole one was skipped")
	}
	// Without its image, it still compares scenes.
	r.ReleaseImage()
	last := fill(160, 70)
	last.Ops[0].Rect = next.Ops[0].Rect
	if n, c := r.Changes(last); n != 200*100 || c == 0 || c > 2*12*12 {
		t.Errorf("without the image: %d pixels to draw, %d changed", n, c)
	}
	r.Render(last)
	if !same(&r, last) {
		t.Error("the image is not the scene drawn after the image was released")
	}
}

// TestBandsDrawAsOne checks that a large area drawn in bands on several
// cores, leaving out the operations each band misses, has the pixels of
// the area drawn at once with every operation.
func TestBandsDrawAsOne(t *testing.T) {
	m := newSceneMaker(3)
	for i := range 6 {
		s := scaled(m.scene(), 8)
		got := NewImage(s.Width, s.Height)
		Render(got, s)
		all := make([]image.Rectangle, len(s.Ops))
		for j := range all {
			all[j] = image.Rect(0, 0, s.Width, s.Height)
		}
		want := NewImage(s.Width, s.Height)
		var r renderer
		r.render(want, s, image.Rect(0, 0, s.Width, s.Height), all, 0, len(s.Ops), nil, nil)
		if !bytes.Equal(got.Pix, want.Pix) {
			t.Errorf("scene %d: drawn in bands, its pixels differ", i)
		}
	}
}

// scaled returns s k times as large, its glyphs as large as they were.
func scaled(s *scene.Scene, k float32) *scene.Scene {
	s.Width, s.Height = s.Width*int(k), s.Height*int(k)
	for i := range s.Ops {
		op := &s.Ops[i]
		op.Rect = scene.Rect{X: op.Rect.X * k, Y: op.Rect.Y * k, W: op.Rect.W * k, H: op.Rect.H * k}
		op.Blur *= k
		for j := range op.Radii {
			op.Radii[j] *= k
		}
	}
	for i := range s.Glyphs {
		s.Glyphs[i].X *= k
		s.Glyphs[i].Y *= k
	}
	return s
}

// TestSkipEffects checks that effects show what is under them when scenes
// the GPU drew left parts of the image behind.
func TestSkipEffects(t *testing.T) {
	for seed := range uint64(40) {
		m := newSceneMaker(seed)
		m.effects = true
		s := m.scene()
		var r Renderer
		r.Render(s)
		full := NewImage(s.Width, s.Height)
		for step := range 30 {
			what := m.change(s)
			if m.rnd.IntN(3) == 0 {
				r.Skip(s)
				continue
			}
			damage := r.Render(s)
			Render(full, s)
			if !bytes.Equal(r.Image.Pix, full.Pix) {
				t.Fatalf("seed %d, step %d (%s): the redrawn %v differs from a whole drawing", seed, step, what, damage)
			}
		}
	}
}
