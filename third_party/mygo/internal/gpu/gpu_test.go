package gpu

import (
	"bytes"
	"testing"

	"github.com/egoist/mygo/internal/gpu/gputest"
	"github.com/egoist/mygo/internal/scene"
)

func TestAffineInstancesPreserveGeometryAndWideColors(t *testing.T) {
	m := scene.Translation(30, 10).Mul(scene.Rotation(20))
	s := &scene.Scene{Width: 80, Height: 60, Clear: scene.Color{B: 120, A: 80}, Ops: []scene.Op{{Kind: scene.OpFill, Rect: scene.Rect{W: 25, H: 15}, Color: scene.Color{R: 255, A: 150}, Transform: m, Wide: 1}}, Wide: []scene.WideColors{{Set: scene.WideColor, Color: [4]float32{1.2, -0.1, 0.2, 0.8}}}}
	var b Builder
	b.Wide = true
	defer b.Release()
	uploads := 0
	shown := b.Build(s, func(*scene.Image) uintptr { uploads++; return 1 })
	if shown != s || uploads != 0 || len(b.Instances) != 1 {
		t.Fatal("transformed fill was rasterized or uploaded as an image")
	}
	in := b.Instances[0]
	if in.Params[0] != 0 || in.Rect != [4]float32{0, 0, 25, 15} || in.Color != s.Wide[0].Color {
		t.Fatal("local geometry or wide color was lost")
	}
	if in.Transform0 != [4]float32{m.A, m.C, m.X, 1} || in.Transform1 != [4]float32{m.B, m.D, m.Y, 0} {
		t.Fatal("vertex transform does not match scene geometry")
	}
	s.Ops[0].Transform = scene.Scaling(0, 1)
	b.Build(s, func(*scene.Image) uintptr { return 1 })
	if len(b.Instances) != 0 {
		t.Fatal("singular transform emitted GPU geometry")
	}
}

func TestAffineClipMaskCacheFollowsClipInsteadOfContent(t *testing.T) {
	s := &scene.Scene{Width: 100, Height: 80, Ops: []scene.Op{
		{Kind: scene.OpPushClip, Rect: scene.Rect{X: 10, Y: 10, W: 70, H: 50}, Radii: [4]float32{8, 8, 8, 8}},
		{Kind: scene.OpFill, Rect: scene.Rect{W: 25, H: 15}, Color: scene.Color{R: 255, A: 255}, Transform: scene.Translation(20, 15)},
		{Kind: scene.OpPopClip},
	}}
	var b Builder
	defer b.Release()
	var mask *scene.Image
	build := func() { b.Build(s, func(m *scene.Image) uintptr { mask = m; return 1 }) }
	build()
	if mask == nil || b.Batches[0].ClipMask != 1 || b.Instances[0].Transform1[3] != 1 {
		t.Fatal("nested clip was not bound as a mask")
	}
	id, version := mask.ID(), mask.Version()
	pixels := append([]byte(nil), mask.Pix...)
	s.Ops[1].Transform = scene.Translation(35, 25)
	build()
	if mask.ID() != id || mask.Version() != version || !bytes.Equal(mask.Pix, pixels) {
		t.Fatal("moving content rerasterized its stationary clip")
	}
	s.Ops[0].Radii = [4]float32{12, 12, 12, 12}
	build()
	if mask.ID() != id || mask.Version() == version {
		t.Fatal("changing clip did not update the retained mask")
	}
	s.Ops[0].Rect.W = 60
	build()
	if mask.ID() != id || mask.W != 60 {
		t.Fatal("resizing clip accumulated a new cached image")
	}
	clipped := s.Ops
	s.Ops = s.Ops[1:2]
	build()
	if len(b.maskSlots) != 1 {
		t.Fatal("a removed clip did not keep its mask")
	}
	version = mask.Version()
	s.Ops = clipped
	build()
	if mask.ID() != id || mask.Version() != version {
		t.Fatal("a clip coming back rasterized its mask again")
	}
}

func TestTransformComingAndGoingKeepsMasks(t *testing.T) {
	// A badge toggling between scaled and not, as in #255, beside a
	// rounded card holding a transformed clip of its own while it is.
	card := scene.Op{Kind: scene.OpPushClip, Rect: scene.Rect{X: 10, Y: 10, W: 200, H: 150}, Radii: [4]float32{12, 12, 12, 12}}
	inner := scene.Op{Kind: scene.OpPushClip, Rect: scene.Rect{W: 80, H: 40}, Transform: scene.Translation(30, 30).Mul(scene.Rotation(5))}
	fill := scene.Op{Kind: scene.OpFill, Rect: scene.Rect{W: 80, H: 40}, Color: scene.Color{A: 255}, Transform: inner.Transform}
	other := scene.Op{Kind: scene.OpPushClip, Rect: scene.Rect{X: 220, Y: 10, W: 60, H: 60}, Radii: [4]float32{6, 6, 6, 6}}
	pop := scene.Op{Kind: scene.OpPopClip}
	shown := &scene.Scene{Width: 300, Height: 200, Ops: []scene.Op{card, inner, fill, pop, pop}}
	elsewhere := &scene.Scene{Width: 300, Height: 200, Ops: []scene.Op{other, fill, pop}}
	hidden := &scene.Scene{Width: 300, Height: 200, Ops: []scene.Op{card, pop}}
	var b Builder
	defer b.Release()
	versions := map[uint64]uint64{}
	rasterized := 0
	build := func(s *scene.Scene) {
		b.Build(s, func(m *scene.Image) uintptr {
			if v, ok := versions[m.ID()]; !ok || v != m.Version() {
				rasterized++
				versions[m.ID()] = m.Version()
			}
			return uintptr(m.ID())
		})
	}
	build(shown)
	first := rasterized
	for range 5 {
		build(hidden)
		build(elsewhere)
		build(shown)
	}
	if first == 0 || rasterized != first+1 {
		t.Fatalf("rasterized %d masks after the first frame's %d, want only the other clip's", rasterized-first, first)
	}
}

func TestIdleMasksStayWithinBudget(t *testing.T) {
	// Two masks of 24 MiB each in a frame, then none: one idle mask fits
	// the budget, two don't.
	clip := func(x float32) []scene.Op {
		return []scene.Op{
			{Kind: scene.OpPushClip, Rect: scene.Rect{X: x, W: 3000, H: 2100}, Transform: scene.Rotation(0.01)},
			{Kind: scene.OpFill, Rect: scene.Rect{X: x, W: 10, H: 10}, Color: scene.Color{A: 255}},
			{Kind: scene.OpPopClip},
		}
	}
	var b Builder
	defer b.Release()
	build := func(s *scene.Scene) { b.Build(s, func(*scene.Image) uintptr { return 1 }) }
	build(&scene.Scene{Width: 6200, Height: 2100, Ops: append(clip(0), clip(3100)...)})
	if len(b.maskSlots) != 2 {
		t.Fatalf("kept %d masks, want 2", len(b.maskSlots))
	}
	build(&scene.Scene{Width: 10, Height: 10})
	if len(b.maskSlots) != 1 {
		t.Fatalf("kept %d idle masks, want 1 within the budget", len(b.maskSlots))
	}
}

func TestTransformOutsideAClipLeavesItUnmasked(t *testing.T) {
	// A scaled badge beside a rounded card of clipped rows: nothing in the
	// card is transformed, so its clips stay in the shader.
	s := &scene.Scene{Width: 400, Height: 300, Ops: []scene.Op{
		{Kind: scene.OpFill, Rect: scene.Rect{W: 40, H: 40}, Color: scene.Color{B: 255, A: 255}, Transform: scene.Scaling(1.2, 1.2)},
		{Kind: scene.OpPushClip, Rect: scene.Rect{Y: 50, W: 400, H: 250}, Radii: [4]float32{11, 11, 11, 11}},
		{Kind: scene.OpPushClip, Rect: scene.Rect{Y: 50, W: 200, H: 32}},
		{Kind: scene.OpFill, Rect: scene.Rect{Y: 50, W: 300, H: 32}, Color: scene.Color{A: 255}},
		{Kind: scene.OpPopClip},
		{Kind: scene.OpPopClip},
	}}
	var b Builder
	defer b.Release()
	masks := 0
	b.Build(s, func(*scene.Image) uintptr { masks++; return 1 })
	if masks != 0 || len(b.maskSlots) != 0 {
		t.Fatalf("rasterized %d clip masks for clips with no transform inside", masks)
	}
	if in := b.Instances[1]; in.Transform1[3] != 0 || in.Clip != [4]float32{0, 50, 200, 32} {
		t.Fatalf("row clip %v, mask mode %v", in.Clip, in.Transform1[3])
	}
}

func TestRoundedClipMasksOnlyTheBranchWithATransform(t *testing.T) {
	// A zoomed image beside a list in the same rounded card: the image's
	// clip keeps the card's corners, the list's rows stay in the shader.
	s := &scene.Scene{Width: 400, Height: 300, Ops: []scene.Op{
		{Kind: scene.OpPushClip, Rect: scene.Rect{W: 400, H: 300}, Radii: [4]float32{11, 11, 11, 11}},
		{Kind: scene.OpPushClip, Rect: scene.Rect{W: 200, H: 300}},
		{Kind: scene.OpFill, Rect: scene.Rect{W: 200, H: 300}, Color: scene.Color{A: 255}, Transform: scene.Scaling(1.5, 1.5)},
		{Kind: scene.OpPopClip},
		{Kind: scene.OpPushClip, Rect: scene.Rect{X: 200, W: 200, H: 32}},
		{Kind: scene.OpFill, Rect: scene.Rect{X: 200, W: 200, H: 32}, Color: scene.Color{A: 255}},
		{Kind: scene.OpPopClip},
		{Kind: scene.OpPopClip},
	}}
	var b Builder
	defer b.Release()
	b.Build(s, func(*scene.Image) uintptr { return 1 })
	if b.Instances[0].Transform1[3] != 1 {
		t.Fatal("zoomed content lost its rounded ancestor's mask")
	}
	if b.Instances[1].Transform1[3] != 0 || b.Batches[len(b.Batches)-1].ClipMask != 0 {
		t.Fatal("untransformed row was masked")
	}
}

// shadowFragments is the area, in pixels, the scissor rectangles of the
// batches of the shadows (kind 1) leave to the renderer to shade.
func shadowFragments(b *Builder) (area int64, instances int) {
	for _, bt := range b.Batches {
		for _, in := range b.Instances[bt.Start : bt.Start+bt.Count] {
			if in.Params[0] == 1 {
				area += bt.Scissor.area()
				instances++
			}
		}
	}
	return
}

// TestShadowSplitShrinksTheShaded checks that a large shadow cast by a box is
// drawn around that box: the scissor rectangles of its pieces tile the window
// less the core of the box, so no fragment of the core is shaded, and a small
// shadow stays one instance.
func TestShadowSplitShrinksTheShaded(t *testing.T) {
	for i, c := range gputest.ShadowCases() {
		s := gputest.ShadowPanelScene(c)
		var split, whole Builder
		whole.NoShadowSplit = true
		split.Build(s, nil)
		whole.Build(s, nil)
		wa, wn := shadowFragments(&whole)
		sa, sn := shadowFragments(&split)
		if wn != len(c.Blur) {
			t.Fatalf("case %d: %d shadow instances without the split, want %d", i, wn, len(c.Blur))
		}
		if sn <= wn || sa >= wa {
			t.Errorf("case %d: split shades %d px in %d instances, whole %d px in %d", i, sa, sn, wa, wn)
		}
		// The pieces of one shadow are disjoint and stay out of the core.
		core := shadowCore(scene.Rect{X: 70, Y: 60, W: 340, H: 220}, [4]float32{c.Radius, c.Radius, c.Radius, c.Radius}, c.Continuous)
		var pieces []Scissor
		for _, bt := range split.Batches {
			for _, in := range split.Instances[bt.Start : bt.Start+bt.Count] {
				if in.Params[0] == 1 {
					pieces = append(pieces, bt.Scissor)
				}
			}
		}
		for x, p := range pieces {
			if p != (Scissor{0, 0, int32(s.Width), int32(s.Height)}) && p.intersect(core).area() != 0 { // whole: not split
				t.Errorf("case %d: piece %d %v reaches the core %v", i, x, p, core)
			}
		}
	}
	// Small shadows, and shadows without a caster, are one instance.
	small := &scene.Scene{Width: 200, Height: 100, Ops: []scene.Op{
		{Kind: scene.OpShadow, Rect: scene.Rect{X: 20, Y: 20, W: 40, H: 30}, Color: scene.Color{A: 80}, Blur: 8,
			Cast: scene.Rect{X: 20, Y: 20, W: 40, H: 30}},
		{Kind: scene.OpShadow, Rect: scene.Rect{X: 20, Y: 20, W: 150, H: 80}, Color: scene.Color{A: 80}, Blur: 8},
	}}
	var b Builder
	b.Build(small, nil)
	if len(b.Instances) != 2 || len(b.Batches) != 1 {
		t.Errorf("small shadows: %d instances in %d batches", len(b.Instances), len(b.Batches))
	}
}

// TestCullLeavesOutCoveredOps checks how many ops the builder leaves out of
// scenes where opaque fills cover others, and that it leaves none out when
// told not to.
func TestCullLeavesOutCoveredOps(t *testing.T) {
	for _, c := range gputest.CullCases() {
		var culled, whole Builder
		whole.NoCull = true
		tex := func(*scene.Image) uintptr { return 1 }
		culled.Build(c.Scene, tex)
		whole.Build(c.Scene, tex)
		if whole.Culled != 0 {
			t.Errorf("%s: %d ops left out with NoCull", c.Name, whole.Culled)
		}
		if culled.Culled != c.Culled {
			t.Errorf("%s: %d ops left out, want %d", c.Name, culled.Culled, c.Culled)
		}
		if c.Culled == 0 && len(culled.Instances) != len(whole.Instances) {
			t.Errorf("%s: %d instances, want %d", c.Name, len(culled.Instances), len(whole.Instances))
		}
		if c.Culled > 0 && len(culled.Instances) >= len(whole.Instances) {
			t.Errorf("%s: %d instances, not fewer than %d", c.Name, len(culled.Instances), len(whole.Instances))
		}
	}
}

// TestCullPlanIsBounded checks that a scene of many fills, none of them
// hidden by a bounded number of occluders, is planned in a linear number of
// steps (here: it only has to finish quickly and leave out what is hidden).
func TestCullPlanIsBounded(t *testing.T) {
	s := &scene.Scene{Width: 4000, Height: 4000}
	for i := range 20000 {
		x := float32(i%100) * 40
		y := float32(i/100) * 20
		s.Ops = append(s.Ops, scene.Op{Kind: scene.OpFill, Rect: scene.Rect{X: x, Y: y, W: 30, H: 15}, Color: scene.Color{R: 1, A: 255}})
	}
	// One panel over the first cells.
	s.Ops = append(s.Ops, scene.Op{Kind: scene.OpFill, Rect: scene.Rect{X: 0, Y: 0, W: 400, H: 200}, Color: scene.Color{A: 255}})
	var b Builder
	b.Build(s, nil)
	if b.Culled != 100 { // 10 columns by 10 rows of cells lie inside the panel's core
		t.Errorf("%d ops left out, want 100", b.Culled)
	}
}

// ringFragments returns the area of the scissor rectangles of the
// border-only fills with a solid color that b built, and how many there are.
func ringFragments(b *Builder) (area int64, n int) {
	for _, bt := range b.Batches {
		for _, in := range b.Instances[bt.Start : bt.Start+bt.Count] {
			if in.Params[0] == 0 && in.Params[2] == 0 && in.Color[3] == 0 && scene.HasBorder(in.UV) {
				area += bt.Scissor.area()
				n++
			}
		}
	}
	return area, n
}

// TestRingSplitShrinksTheShaded checks that a border-only fill the size of a
// panel is drawn around the core inside its border, in pieces that tile the
// frame less that core, and that other fills are not split.
func TestRingSplitShrinksTheShaded(t *testing.T) {
	for _, c := range gputest.RingCases() {
		var split, whole Builder
		whole.NoRingSplit = true
		split.NoCull, whole.NoCull = true, true
		tex := func(*scene.Image) uintptr { return 1 }
		split.Build(c.Scene, tex)
		whole.Build(c.Scene, tex)
		sa, sn := ringFragments(&split)
		wa, wn := ringFragments(&whole)
		if !c.Split {
			if len(split.Instances) != len(whole.Instances) {
				t.Errorf("%s: %d instances, want %d (not split)", c.Name, len(split.Instances), len(whole.Instances))
			}
			continue
		}
		if wn == 0 || sn <= wn || sa >= wa {
			t.Errorf("%s: split shades %d px in %d instances, whole %d px in %d", c.Name, sa, sn, wa, wn)
		}
	}
	// The pieces of the first ring of a plain case lie outside its core,
	// and with the clip's scissor rectangle tile what is left.
	c := gputest.RingCases()[2] // rounded
	var b Builder
	b.Build(c.Scene, nil)
	core := ringCore(scene.Rect{X: 20, Y: 20, W: 440, H: 320}, [4]float32{16, 16, 16, 16}, scene.Uniform(2), false)
	var covered int64
	for _, bt := range b.Batches {
		for _, in := range b.Instances[bt.Start : bt.Start+bt.Count] {
			if in.Params[0] == 0 && in.Color[3] == 0 && scene.HasBorder(in.UV) {
				if bt.Scissor.intersect(core).area() != 0 {
					t.Errorf("piece %v reaches the core %v", bt.Scissor, core)
				}
				covered += bt.Scissor.area()
			}
		}
	}
	if want := int64(c.Scene.Width*c.Scene.Height) - core.area(); covered != want {
		t.Errorf("pieces cover %d px, want %d", covered, want)
	}
}

// TestOpaqueCores checks which fills the builder draws as opaque cores with
// blended edges around them: a layer as large as the frame is one opaque
// instance without edges, a card has a core and four edges that tile the
// frame less it, and small, translucent and gradient fills stay blended.
func TestOpaqueCores(t *testing.T) {
	white := scene.Color{R: 255, G: 255, B: 255, A: 255}
	s := &scene.Scene{Width: 800, Height: 600, Ops: []scene.Op{
		{Kind: scene.OpFill, Rect: scene.Rect{W: 800, H: 600}, Color: white},
		{Kind: scene.OpFill, Rect: scene.Rect{X: 100, Y: 100, W: 600, H: 400}, Radii: [4]float32{16, 16, 16, 16}, Color: white},
		{Kind: scene.OpFill, Rect: scene.Rect{X: 10, Y: 10, W: 50, H: 50}, Color: white},
		{Kind: scene.OpFill, Rect: scene.Rect{X: 10, Y: 10, W: 500, H: 500}, Color: scene.Color{R: 1, A: 254}},
	}}
	var b Builder
	b.NoCull, b.Opaque = true, true
	b.Build(s, nil)
	if b.OpaqueCores != 2 {
		t.Fatalf("%d opaque cores, want 2", b.OpaqueCores)
	}
	var core, blended int64
	for _, bt := range b.Batches {
		if bt.Opaque {
			if bt.Count != 1 {
				t.Errorf("an opaque batch has %d instances", bt.Count)
			}
			core += bt.Scissor.area()
		} else {
			blended += bt.Scissor.area() * int64(bt.Count)
		}
	}
	card := shadowCore(scene.Rect{X: 100, Y: 100, W: 600, H: 400}, [4]float32{16, 16, 16, 16}, false)
	if want := int64(800*600) + card.area(); core != want {
		t.Errorf("opaque cores cover %d px, want %d", core, want)
	}
	// Blended: the card's four edges tile the frame less its core, and the
	// small and the translucent fill are one instance each, scissored to the frame.
	if want := int64(800*600) - card.area() + 2*800*600; blended != want {
		t.Errorf("blended scissor area %d, want %d", blended, want)
	}
	b.Opaque = false
	b.Build(s, nil)
	for _, bt := range b.Batches {
		if bt.Opaque {
			t.Error("an opaque batch without Opaque")
		}
	}
}

// TestBatchClasses checks the class of each batch: plain fills (with and
// without continuous corners, in one batch), other fills, shadows, glyphs and
// images, and that transformed instances take the general shader.
func TestBatchClasses(t *testing.T) {
	red := scene.Color{R: 255, A: 255}
	atlas := scene.NewAtlas(1, 64, 32)
	img := scene.NewImageRGBA(2, 2, make([]byte, 16))
	s := &scene.Scene{Width: 200, Height: 100, MaskAtlas: atlas}
	s.Glyphs = []scene.Glyph{{X: 10, Y: 20, W: 8, H: 9, U: 16, V: 8, UW: 8, VH: 9, Color: red}}
	r := func(x float32) scene.Rect { return scene.Rect{X: x, Y: 10, W: 20, H: 20} }
	s.Ops = []scene.Op{
		{Kind: scene.OpFill, Rect: r(0), Color: red},
		{Kind: scene.OpFill, Rect: r(30), Radii: [4]float32{4, 4, 4, 4}, Continuous: true, Color: red},
		{Kind: scene.OpFill, Rect: r(60), Radii: [4]float32{4, 4, 4, 4}, Color: red},
		{Kind: scene.OpFill, Rect: r(90), Color: red, Border: scene.Uniform(1), BorderColor: red},
		{Kind: scene.OpShadow, Rect: r(0), Color: red, Blur: 8},
		{Kind: scene.OpGlyphs, Start: 0, End: 1},
		{Kind: scene.OpImage, Rect: r(0), Image: img, Src: scene.Rect{W: 2, H: 2}},
		{Kind: scene.OpFill, Rect: r(120), Color: red, Transform: scene.Translation(1, 1)},
	}
	var b Builder
	b.Classes = true
	b.Build(s, func(*scene.Image) uintptr { return 1 })
	want := []Class{ClassPlain, ClassFill, ClassShadow, ClassGlyph, ClassImage, ClassGeneral}
	if len(b.Batches) != len(want) {
		t.Fatalf("%d batches: %+v", len(b.Batches), b.Batches)
	}
	for i, c := range want {
		if b.Batches[i].Class != c {
			t.Errorf("batch %d: class %d, want %d", i, b.Batches[i].Class, c)
		}
	}
	if b.Batches[0].Count != 3 {
		t.Errorf("the plain fills share a batch of %d", b.Batches[0].Count)
	}
	b.NoClasses = true
	b.Build(s, func(*scene.Image) uintptr { return 1 })
	for i, bt := range b.Batches {
		if bt.Class != ClassGeneral {
			t.Errorf("NoClasses: batch %d has class %d", i, bt.Class)
		}
	}
}

// TestCullKeepsTransformedShadows checks that the planner does not leave out a
// shadow under a transform whose blur reaches out of the cover over it: the blur
// grows with the scale, so a shadow of blur 20 at scale 4 reaches about 120
// device pixels from its box, and a cover of the box and 100 pixels around it
// does not hold it. A cover of the frame does.
func TestCullKeepsTransformedShadows(t *testing.T) {
	shadow := scene.Op{Kind: scene.OpShadow, Rect: scene.Rect{X: 50, Y: 50, W: 40, H: 40}, Color: scene.Color{A: 255}, Blur: 20, Transform: scene.Scaling(4, 4)}
	cover := func(r scene.Rect) scene.Op {
		return scene.Op{Kind: scene.OpFill, Rect: r, Color: scene.Color{R: 1, A: 255}}
	}
	for _, c := range []struct {
		name   string
		cover  scene.Rect
		culled int
	}{
		{"cover of the box and 100 px", scene.Rect{X: 154, Y: 154, W: 252, H: 252}, 0},
		{"cover of the frame", scene.Rect{W: 640, H: 640}, 1},
	} {
		var b Builder
		b.Build(&scene.Scene{Width: 640, Height: 640, Ops: []scene.Op{shadow, cover(c.cover)}}, nil)
		if b.Culled != c.culled {
			t.Errorf("%s: %d ops left out, want %d", c.name, b.Culled, c.culled)
		}
	}
}

// TestFillsInsideTheirClipSkipItsCoverage checks which fills the builder
// tells the shaders need no clip coverage: those not transformed whose quad
// lies well inside a clip that is a plain rectangle.
func TestFillsInsideTheirClipSkipItsCoverage(t *testing.T) {
	s := &scene.Scene{Width: 200, Height: 200}
	fill := func(r scene.Rect) {
		s.Ops = append(s.Ops, scene.Op{Kind: scene.OpFill, Rect: r, Color: scene.Color{A: 255}})
	}
	fill(scene.Rect{X: 10, Y: 10, W: 50, H: 50}) // 0: inside the root clip
	fill(scene.Rect{X: 0, Y: 0, W: 200, H: 200}) // 1: the frame
	s.Ops = append(s.Ops, scene.Op{Kind: scene.OpPushClip, Rect: scene.Rect{X: 20, Y: 20, W: 100, H: 100}})
	fill(scene.Rect{X: 40, Y: 40, W: 20, H: 20}) // 2: inside the clip
	fill(scene.Rect{X: 19, Y: 40, W: 20, H: 20}) // 3: cut by it
	fill(scene.Rect{X: 21, Y: 40, W: 20, H: 20}) // 4: a pixel from its edge, within the antialiasing's reach
	s.Ops = append(s.Ops, scene.Op{Kind: scene.OpPopClip})
	s.Ops = append(s.Ops, scene.Op{Kind: scene.OpPushClip, Rect: scene.Rect{X: 20, Y: 20, W: 100, H: 100}, Radii: [4]float32{8, 8, 8, 8}})
	fill(scene.Rect{X: 50, Y: 50, W: 10, H: 10}) // 5: under a rounded clip
	s.Ops = append(s.Ops, scene.Op{Kind: scene.OpPopClip})
	s.Ops = append(s.Ops, scene.Op{Kind: scene.OpFill, Rect: scene.Rect{X: 10, Y: 10, W: 50, H: 50}, Color: scene.Color{A: 255}, Transform: scene.Translation(3, 3)}) // 6
	want := map[int]bool{0: true, 1: true, 2: true}                                                                                                                   // 1, the frame, lies in the root clip, which is unbounded
	tex := func(*scene.Image) uintptr { return 1 }
	var b Builder
	b.NoCull = true
	b.ClipSkip = true
	b.Build(s, tex)
	if len(b.Instances) != 7 {
		t.Fatalf("%d instances", len(b.Instances))
	}
	for k, in := range b.Instances {
		if got := in.Transform1[3] == 3; got != want[k] {
			t.Errorf("fill %d: no clip coverage is %v, want %v", k, got, want[k])
		}
	}
	b.NoClipSkip = true
	b.Build(s, tex)
	for k, in := range b.Instances {
		if in.Transform1[3] == 3 {
			t.Errorf("instance %d skips the clip with NoClipSkip", k)
		}
	}
}

// Renderers that have not opted in retain the general shader contract.
func TestShaderCapabilitiesAreOptIn(t *testing.T) {
	s := &scene.Scene{Width: 100, Height: 100, Ops: []scene.Op{
		{Kind: scene.OpFill, Rect: scene.Rect{X: 10, Y: 10, W: 20, H: 20}, Color: scene.Color{A: 255}},
		{Kind: scene.OpShadow, Rect: scene.Rect{X: 50, Y: 50, W: 20, H: 20}, Color: scene.Color{A: 128}, Blur: 4},
	}}
	var b Builder
	b.Build(s, nil)
	if len(b.Batches) != 1 || b.Batches[0].Class != ClassGeneral {
		t.Fatalf("default builder split shader classes: %+v", b.Batches)
	}
	for _, in := range b.Instances {
		if in.Transform1[3] == 3 {
			t.Fatal("default builder emitted a Metal-only clip flag")
		}
	}
}

func TestCullDoesNotPlanSpriteOnlyScenes(t *testing.T) {
	var b Builder
	s := gputest.SpriteScene(1920, 1080, 1.5, 3, 1000)
	b.Build(s, func(*scene.Image) uintptr { return 1 })
	if b.Culled != 0 || len(b.Instances) != len(s.Ops) {
		t.Fatalf("sprite-only scene: %d culled, %d instances for %d ops", b.Culled, len(b.Instances), len(s.Ops))
	}
	if cap(b.plan) != 0 {
		t.Fatalf("sprite-only scene allocated %d planning records", cap(b.plan))
	}
	// A later covered frame must still plan, and returning to sprites must
	// clear old culling decisions even though its planning pass is skipped.
	covered := &scene.Scene{Width: 1920, Height: 1080, Ops: []scene.Op{
		{Kind: scene.OpFill, Rect: scene.Rect{W: 1920, H: 1080}, Color: scene.Color{A: 255}},
		{Kind: scene.OpFill, Rect: scene.Rect{W: 1920, H: 1080}, Color: scene.Color{R: 100, A: 255}},
	}}
	b.Build(covered, nil)
	if b.Culled != 1 {
		t.Fatalf("covered frame culled %d ops", b.Culled)
	}
	b.Build(s, func(*scene.Image) uintptr { return 1 })
	if b.Culled != 0 || len(b.Instances) != len(s.Ops) {
		t.Fatal("sprite frame retained the previous culling plan")
	}
}

// A shader's effect draws with its wide uniforms for a wide gamut, and
// with its own otherwise.
func TestShaderWideUniforms(t *testing.T) {
	fx := &scene.Effect{Name: "shader", Shader: &scene.ShaderLayout{Lanes: []int32{0}, Size: 16}}
	narrow, wide := []uint32{1}, []uint32{2}
	s := &scene.Scene{Width: 40, Height: 40,
		Ops:     []scene.Op{{Kind: scene.OpEffect, Rect: scene.Rect{W: 20, H: 20}}},
		Effects: []scene.EffectOp{{Effect: fx, Uniforms: narrow, WideUniforms: wide}}}
	for _, w := range []bool{false, true} {
		var b Builder
		b.Wide = w
		b.Build(s, func(*scene.Image) uintptr { return 1 })
		want := narrow
		if w {
			want = wide
		}
		if len(b.Batches) != 1 || &b.Batches[0].Uniforms[0] != &want[0] {
			t.Errorf("wide %v: the batch draws with %v, want %v", w, b.Batches[0].Uniforms, want)
		}
		b.Release()
	}
}
