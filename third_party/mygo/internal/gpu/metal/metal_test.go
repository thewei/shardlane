//go:build darwin

package metal

import (
	"errors"
	"image"
	"math"
	"slices"
	"testing"
	"unsafe"

	"github.com/egoist/mygo/internal/gpu"
	"github.com/egoist/mygo/internal/gpu/gputest"
	"github.com/egoist/mygo/internal/scene"
)

func TestDrawsAsTheCPURenderer(t *testing.T) {
	r, err := newRenderer()
	if err != nil {
		t.Skip("no Metal:", err)
	}
	defer r.Release()
	s := gputest.Scene()
	// Twice: the second frame updates what the first uploaded.
	for range 2 {
		pix, err := r.renderOffscreen(s)
		if err != nil {
			t.Fatal(err)
		}
		gputest.Compare(t, "metal", pix, s.Width*4, s)
	}
	s = gputest.ContinuousScene()
	pix, err := r.renderOffscreen(s)
	if err != nil {
		t.Fatal(err)
	}
	gputest.Compare(t, "metal-continuous", pix, s.Width*4, s)
}

// TestShaderLibrary checks that the library compiled ahead of time comes
// from shader.metal as it is, and that Metal loads it.
func TestShaderLibrary(t *testing.T) {
	if gpu.SourceSum(shaderSource) != shaderLibrarySum {
		t.Fatal("shader.metal changed since shaderlib.go was generated: run go generate ./internal/gpu/metal on macOS")
	}
	r, err := newRenderer()
	if err != nil {
		t.Skip("no Metal:", err)
	}
	defer r.Release()
	var lib id
	pool(func() { lib = r.compiledLibrary() })
	if lib == 0 {
		t.Fatal("Metal does not load the compiled library")
	}
	pool(func() { release(&lib) })
}

// TestChangedSince checks what frames drawn in memory copy into the
// drawables, which take turns, and after the GPU drew into one.
func TestChangedSince(t *testing.T) {
	r := &Renderer{shown: map[uint32]uint64{}}
	whole := []image.Rectangle{image.Rect(0, 0, 100, 50)}
	present := func(key uint32, damage ...image.Rectangle) []image.Rectangle {
		r.pixelFrames++
		n := r.pixelFrames
		r.pixelDamage[n%pixelHistory] = append(r.pixelDamage[n%pixelHistory][:0], damage...)
		rects := r.changedSince(key, n, 100, 50)
		r.shown[key] = n
		return rects
	}
	a, b := image.Rect(0, 0, 10, 10), image.Rect(20, 0, 30, 10)
	if got := present(1, whole...); !slices.Equal(got, whole) {
		t.Errorf("a new drawable: %v", got)
	}
	if got := present(2, a); !slices.Equal(got, whole) {
		t.Errorf("the other new drawable: %v", got)
	}
	// Each drawable lags two frames behind.
	if got := present(1, b); !slices.Equal(got, []image.Rectangle{a, b}) {
		t.Errorf("the first drawable again: %v", got)
	}
	// The GPU draws into the second.
	delete(r.shown, 2)
	if got := present(2, a); !slices.Equal(got, whole) {
		t.Errorf("a drawable the GPU drew: %v", got)
	}
	// A drawable left behind for longer than the history.
	for range pixelHistory {
		present(2, a)
	}
	if got := present(1, b); !slices.Equal(got, whole) {
		t.Errorf("a drawable %d frames behind: %v", pixelHistory+1, got)
	}
}

func TestCopyRect(t *testing.T) {
	src := make([]byte, 4*4*3) // 4×3, stride 16
	for i := range src {
		src[i] = byte(i)
	}
	dst := make([]byte, 32*3) // stride 32, wider than the frame
	copyRect(dst, 32, src, 16, image.Rect(1, 1, 3, 3))
	for y := range 3 {
		for x := range 4 {
			in := y >= 1 && y < 3 && x >= 1 && x < 3
			for c := range 4 {
				got, want := dst[y*32+x*4+c], byte(0)
				if in {
					want = src[y*16+x*4+c]
				}
				if got != want {
					t.Fatalf("pixel %d,%d: %d, want %d", x, y, got, want)
				}
			}
		}
	}
}

// TestWideColors draws wide colors into a float16 target, which keeps what
// leaves 0 to 1, and an Oklab gradient between them, which does not clamp;
// and the same scene into a BGRA8 one, which draws the nearest sRGB colors.
func TestWideColors(t *testing.T) {
	r, err := newRenderer()
	if err != nil {
		t.Skip("no Metal:", err)
	}
	defer r.Release()
	if !r.SetWide(true) {
		t.Fatal(r.wideErr)
	}
	s := &scene.Scene{}
	s.Reset(32, 16, scene.Color{})
	green, red := [4]float32{-0.25, 1.1, -0.1, 1}, [4]float32{1.15, -0.2, -0.1, 1}
	s.Wide = []scene.WideColors{{Color: green, Set: scene.WideColor}, {Color: green, Color2: red, Set: scene.WideColor | scene.WideColor2}}
	srgbGreen, srgbRed := scene.Color{G: 255, A: 255}, scene.Color{R: 255, A: 255}
	s.Ops = append(s.Ops,
		scene.Op{Kind: scene.OpFill, Rect: scene.Rect{W: 32, H: 8}, Color: srgbGreen, Wide: 1},
		scene.Op{Kind: scene.OpFill, Rect: scene.Rect{Y: 8, W: 32, H: 8}, Paint: scene.PaintOklab, Gradient: [4]float32{0, 0, 32, 0},
			Color: srgbGreen, Color2: srgbRed, Wide: 2})
	var px [2][4]float32
	pool(func() {
		r.waitLast()
		r.cur = &r.formats[1]
		tex := r.newTexture(s.Width, s.Height, pixelFormatRGBA16Float, usageRenderTarget|usageShaderRead, nil, 0)
		if tex == 0 {
			err = errors.New("no target")
			return
		}
		defer release(&tex)
		var cb id
		if cb, err = r.encode(s, tex); err != nil {
			return
		}
		blit := send(cb, "blitCommandEncoder")
		send(blit, "synchronizeResource:", tex)
		send(blit, "endEncoding")
		send(cb, "commit")
		send(cb, "waitUntilCompleted")
		pix := make([]uint16, s.Width*s.Height*4)
		msgGetBytes(tex, sel("getBytes:bytesPerRow:fromRegion:mipmapLevel:"), unsafe.Pointer(&pix[0]), uint(s.Width*8),
			mtlRegion{W: uint(s.Width), H: uint(s.Height), D: 1}, 0)
		at := func(x, y int) (c [4]float32) {
			for i := range c {
				c[i] = half(pix[(y*s.Width+x)*4+i])
			}
			return c
		}
		px[0], px[1] = at(4, 2), at(16, 12)
	})
	if err != nil {
		t.Fatal(err)
	}
	// The pixel holds the straight color premultiplied by alpha 1, in the
	// order of the target's components (RGBA).
	for i, want := range green {
		if d := px[0][i] - want; d > 0.01 || d < -0.01 {
			t.Errorf("solid wide color: component %d is %v, want %v", i, px[0][i], want)
		}
	}
	// Midway between two colors, at least one component is out of 0 to 1:
	// the mix of the greens and reds of the ends, in Oklab, keeps the
	// chroma the ends have beyond sRGB.
	if m := px[1]; !(m[0] < 0 || m[0] > 1 || m[1] < 0 || m[1] > 1 || m[2] < 0 || m[2] > 1) {
		t.Errorf("gradient midpoint %v is inside sRGB", m)
	}

	// A BGRA8 target draws the sRGB colors of the ops.
	pix, err := r.renderOffscreen(s)
	if err != nil {
		t.Fatal(err)
	}
	if b, g, red, a := pix[(2*s.Width+4)*4], pix[(2*s.Width+4)*4+1], pix[(2*s.Width+4)*4+2], pix[(2*s.Width+4)*4+3]; b != 0 || g != 255 || red != 0 || a != 255 {
		t.Errorf("BGRA8 target: %v %v %v %v, want the sRGB green", b, g, red, a)
	}
}

// half converts an IEEE half-precision float.
func half(h uint16) float32 {
	sign := float32(1)
	if h&0x8000 != 0 {
		sign = -1
	}
	e, m := int(h>>10)&0x1f, float32(h&0x3ff)
	switch e {
	case 0:
		return sign * m / 1024 / 16384
	case 31:
		return sign * float32(math.Inf(1))
	}
	return sign * (1 + m/1024) * float32(math.Pow(2, float64(e-15)))
}
