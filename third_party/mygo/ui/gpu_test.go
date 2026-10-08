package ui

import (
	"errors"
	"image"
	"sync"
	"testing"
	"time"

	"github.com/egoist/mygo/internal/platform"
	"github.com/egoist/mygo/internal/scene"
	"github.com/egoist/mygo/internal/surface"
)

// testSurface is a surface that counts the frames drawn in memory.
type testSurface struct{ pixels int }

func (s *testSurface) Native() platform.SurfaceNative           { return platform.SurfaceNative{HWND: 1} }
func (s *testSurface) Size() (float64, float64, float64)        { return 200, 100, 1 }
func (s *testSurface) RequestFrame()                            {}
func (s *testSurface) RefreshRate() float64                     { return 60 }
func (s *testSurface) PresentPixels([]byte, int, int, int)      { s.pixels++ }
func (s *testSurface) SetCursor(platform.Cursor)                {}
func (s *testSurface) SetTextInput(platform.TextInputState)     {}
func (s *testSurface) UpdateAccessibility(*platform.AccessTree) {}

// testGPU is a GPU renderer whose device goes away when fail is set.
type testGPU struct {
	fail             bool
	frames, released int
}

func (g *testGPU) Render(*scene.Scene) error {
	if g.fail {
		return errors.New("the device was removed")
	}
	g.frames++
	return nil
}

func (g *testGPU) Release() { g.released++ }

// gpuHost returns a window host on a test surface whose GPU renderers
// newGPU makes, and a function drawing a frame.
func gpuHost(t *testing.T, make func() (gpuRenderer, error)) (*windowHost, *testSurface, func()) {
	t.Setenv("MYGO_GPU", "")
	newGPU = func(platform.SurfaceNative) (gpuRenderer, error) { return make() }
	t.Cleanup(func() { newGPU = newGPURenderer })
	s := &testSurface{}
	h := &windowHost{conn: &surface.Conn{Surface: s}}
	h.rt = newRuntime(func(c *Context) { Text(c, "Hello") }, h)
	return h, s, func() { h.event(platform.SurfaceEvent{Kind: platform.SurfaceFrame}) }
}

// softGPU is a renderer drawing on the CPU, as Direct3D's WARP.
type softGPU struct{ testGPU }

func (g *softGPU) Software() bool { return true }

func TestGPUComesBack(t *testing.T) {
	var made []*testGPU
	gone := false
	h, s, frame := gpuHost(t, func() (gpuRenderer, error) {
		if gone {
			return nil, errors.New("no device")
		}
		g := &testGPU{}
		made = append(made, g)
		return g, nil
	})
	frame()
	if len(made) != 1 || made[0].frames != 1 || s.pixels != 0 {
		t.Fatalf("first frame: %d renderers, %d frames in memory", len(made), s.pixels)
	}

	// A renderer that fails gives way to another at once.
	made[0].fail = true
	frame()
	if made[0].released != 1 || len(made) != 2 || made[1].frames != 1 || s.pixels != 0 {
		t.Fatalf("after a failure: released %d, %d renderers, %d frames in memory", made[0].released, len(made), s.pixels)
	}

	// Without a device, frames are drawn in memory until the wait is over.
	gone = true
	made[1].fail = true
	frame()
	frame()
	if len(made) != 2 || s.pixels != 2 || h.backoff != time.Second {
		t.Fatalf("without a device: %d renderers, %d frames in memory, wait %v", len(made), s.pixels, h.backoff)
	}
	// None either when the wait is over: the next wait is longer.
	h.retryAt = time.Now()
	frame()
	if s.pixels != 3 || h.backoff != 2*time.Second || h.retryAt.IsZero() {
		t.Fatalf("still without a device: %d frames in memory, wait %v", s.pixels, h.backoff)
	}
	// It is back.
	gone = false
	h.retryAt = time.Now()
	frame()
	if len(made) != 3 || made[2].frames != 1 || s.pixels != 3 || !h.retryAt.IsZero() {
		t.Fatalf("once back: %d renderers, %d frames in memory", len(made), s.pixels)
	}
}

func TestSoftwareUntilTheGPUIsBack(t *testing.T) {
	onGPU := true
	var gpus []*testGPU
	var soft *softGPU
	h, s, frame := gpuHost(t, func() (gpuRenderer, error) {
		if onGPU {
			g := &testGPU{}
			gpus = append(gpus, g)
			return g, nil
		}
		soft = &softGPU{}
		return soft, nil
	})
	frame()
	// The GPU goes away: a renderer drawing in software takes its place,
	// and frames still show.
	onGPU = false
	gpus[0].fail = true
	frame()
	if soft == nil || soft.frames != 1 || s.pixels != 0 || !h.degraded || h.retryAt.IsZero() {
		t.Fatalf("after the loss: software %v, %d frames in memory, degraded %v", soft != nil, s.pixels, h.degraded)
	}
	// When the wait is over, a frame tries for the GPU, still away.
	first := soft
	h.retryAt = time.Now()
	frame()
	if first.released != 1 || soft == first || soft.frames != 1 || h.backoff != 2*time.Second {
		t.Fatalf("trying again: released %d, wait %v", first.released, h.backoff)
	}
	// It is back.
	onGPU = true
	h.retryAt = time.Now()
	frame()
	if soft.released != 1 || len(gpus) != 2 || gpus[1].frames != 1 || h.degraded || !h.retryAt.IsZero() {
		t.Errorf("once back: software released %d, %d GPU renderers, degraded %v", soft.released, len(gpus), h.degraded)
	}
}

func TestNoGPUFromTheStart(t *testing.T) {
	tries := 0
	h, s, frame := gpuHost(t, func() (gpuRenderer, error) {
		tries++
		return nil, errors.New("no device")
	})
	frame()
	frame()
	if tries != 1 || s.pixels != 2 || !h.retryAt.IsZero() {
		t.Errorf("%d tries, %d frames in memory, retry at %v", tries, s.pixels, h.retryAt)
	}

	// A machine whose renderer draws in software from the start keeps it.
	tries = 0
	h, s, frame = gpuHost(t, func() (gpuRenderer, error) {
		tries++
		return &softGPU{}, nil
	})
	frame()
	frame()
	if tries != 1 || s.pixels != 0 || h.degraded || !h.retryAt.IsZero() {
		t.Errorf("software from the start: %d tries, degraded %v, retry at %v", tries, h.degraded, h.retryAt)
	}
}

// pixelGPU is a GPU renderer that also presents frames drawn in memory, as
// Metal's does.
type pixelGPU struct {
	testGPU
	pixels int
	damage []image.Rectangle
}

func (g *pixelGPU) PresentPixels(pix []byte, stride, width, height int, scale float64, damage []image.Rectangle) error {
	g.pixels++
	g.damage = append(g.damage[:0], damage...)
	return nil
}

// TestSmallChangesDrawOnCPU checks which frames the CPU draws and which
// the GPU does.
func TestSmallChangesDrawOnCPU(t *testing.T) {
	g := &pixelGPU{}
	h, s, frame := gpuHost(t, func() (gpuRenderer, error) { return g, nil })
	x, back := float32(10), RGB(200, 200, 200)
	h.rt = newRuntime(func(c *Context) {
		Box(c).Fill().Background(back).Children(func() {
			Box(c).Size(10, 10).Background(RGB(0, 0, 255)).Absolute().Left(x).Top(10)
		})
	}, h)
	pause := func() { h.lastFrame = time.Now().Add(-time.Second) }
	check := func(what string, pixels, frames int) {
		t.Helper()
		if g.pixels != pixels || g.frames != frames || s.pixels != 0 {
			t.Fatalf("%s: %d frames drawn on the CPU, %d on the GPU, %d in memory without the GPU", what, g.pixels, g.frames, s.pixels)
		}
	}
	frame()
	check("the first frame", 1, 0)
	if len(g.damage) != 1 || g.damage[0] != image.Rect(0, 0, 200, 100) {
		t.Errorf("the first frame changed %v", g.damage)
	}
	// A small change after a pause, and in a burst.
	pause()
	x = 30
	frame()
	check("a small change after a pause", 2, 0)
	if len(g.damage) == 0 || g.damage[0].Dx() > 40 || g.damage[0].Dy() > 20 {
		t.Errorf("moving the square changed %v", g.damage)
	}
	x = 40
	frame()
	check("a small change in a burst", 3, 0)
	// Much of the window in a burst goes to the GPU, and back to the CPU
	// after a pause.
	back = RGB(100, 100, 100)
	frame()
	check("a large change in a burst", 3, 1)
	pause()
	x = 50
	frame()
	check("a small change after a pause, the CPU's frame being old", 4, 1)
	if len(g.damage) != 1 || g.damage[0] != image.Rect(0, 0, 200, 100) {
		t.Errorf("catching up changed %v", g.damage)
	}
	// The CPU's frame goes once the GPU has drawn alone for a second.
	back = RGB(50, 50, 50)
	frame()
	check("a large change", 4, 2)
	if h.soft.Image.Pix == nil {
		t.Fatal("the CPU's frame went at once")
	}
	h.gpuSinceCPU = time.Now().Add(-2 * time.Second)
	back = RGB(60, 60, 60)
	frame()
	check("a large change, a second later", 4, 3)
	if h.soft.Image.Pix != nil {
		t.Error("the CPU's frame stays while the GPU draws alone")
	}
	pause()
	frame()
	check("the next frame after a pause", 5, 3)
}

// TestBackToCPUInABurst checks that frames changing little draw on the
// CPU again while frames still follow each other, after the GPU drew one
// changing much, and that a frame like the one shown presents nothing.
func TestBackToCPUInABurst(t *testing.T) {
	g := &pixelGPU{}
	h, _, frame := gpuHost(t, func() (gpuRenderer, error) { return g, nil })
	x, back := float32(10), RGB(200, 200, 200)
	h.rt = newRuntime(func(c *Context) {
		Box(c).Fill().Background(back).Children(func() {
			Box(c).Size(10, 10).Background(RGB(0, 0, 255)).Absolute().Left(x).Top(10)
		})
	}, h)
	check := func(what string, pixels, frames int) {
		t.Helper()
		if g.pixels != pixels || g.frames != frames {
			t.Fatalf("%s: %d frames drawn on the CPU, %d on the GPU", what, g.pixels, g.frames)
		}
	}
	frame()
	x = 20
	frame()
	check("two small changes", 2, 0)
	// The same frame again shows already.
	frame()
	check("the same frame", 2, 0)
	if h.path != "unchanged" {
		t.Errorf("the same frame was %s", h.path)
	}
	// Much of the window changes, as a page slides in, then only the
	// square moves, frame after frame.
	back = RGB(100, 100, 100)
	frame()
	check("a large change in a burst", 2, 1)
	x = 30
	frame()
	check("a small change after it", 3, 1)
	if len(g.damage) != 1 || g.damage[0] != image.Rect(0, 0, 200, 100) {
		t.Errorf("catching up on the large change changed %v", g.damage)
	}
	x = 40
	frame()
	check("the next small change", 4, 1)
	if len(g.damage) == 0 || g.damage[0].Dx() > 40 || g.damage[0].Dy() > 20 {
		t.Errorf("moving the square changed %v", g.damage)
	}
	// A small change after frames the GPU drew alone for a second, which
	// freed the CPU's frame, draws all of it.
	back = RGB(50, 50, 50)
	frame()
	h.gpuSinceCPU = time.Now().Add(-2 * time.Second)
	back = RGB(60, 60, 60)
	frame()
	check("large changes", 4, 3)
	if h.soft.Image.Pix != nil {
		t.Fatal("the CPU's frame stays while the GPU draws alone")
	}
	x = 50
	frame()
	check("a small change after them", 5, 3)
	if len(g.damage) != 1 || g.damage[0] != image.Rect(0, 0, 200, 100) {
		t.Errorf("drawing the frame freed changed %v", g.damage)
	}
}

// TestHeavyCPUBurstsDrawOnGPU checks that frames changing little draw on
// the GPU once the CPU's frames of their burst cost too much, as an
// animation over translucent layers may, and on the CPU after a pause.
func TestHeavyCPUBurstsDrawOnGPU(t *testing.T) {
	g := &pixelGPU{}
	h, _, frame := gpuHost(t, func() (gpuRenderer, error) { return g, nil })
	x := float32(10)
	h.rt = newRuntime(func(c *Context) {
		Box(c).Fill().Background(RGB(200, 200, 200)).Children(func() {
			Box(c).Size(10, 10).Background(RGB(0, 0, 255)).Absolute().Left(x).Top(10)
		})
	}, h)
	check := func(what string, pixels, frames int) {
		t.Helper()
		if g.pixels != pixels || g.frames != frames {
			t.Fatalf("%s: %d frames drawn on the CPU, %d on the GPU", what, g.pixels, g.frames)
		}
	}
	move := func() {
		x++
		frame()
	}
	frame()
	move()
	check("small changes", 2, 0)
	// The CPU's frames of the burst so far took 8 ms each, at 60 Hz.
	start := time.Now().Add(-300 * time.Millisecond)
	h.cpuBurst = cpuLoad{}
	for at := time.Duration(0); at < 300*time.Millisecond; at += 16 * time.Millisecond {
		h.noteCPUFrame(start.Add(at), 8*time.Millisecond, 8*time.Millisecond)
	}
	if !h.cpuHeavy {
		t.Fatal("frames of 8 ms at 60 Hz are not heavy")
	}
	move()
	move()
	check("small changes in a heavy burst", 2, 2)
	// After a pause, the CPU draws again, catching up on where the square
	// went while the GPU drew.
	h.lastFrame = time.Now().Add(-time.Second)
	h.frameEnd = h.lastFrame
	move()
	check("a small change after a pause", 3, 2)
	if len(g.damage) == 0 || g.damage[0].Dx() > 40 || !g.damage[0].Overlaps(image.Rect(12, 10, 23, 20)) {
		t.Errorf("catching up changed %v", g.damage)
	}
	// Light frames do not tip a burst over.
	for range 30 {
		move()
	}
	if h.cpuHeavy || g.frames != 2 {
		t.Errorf("light frames: heavy %v, %d frames on the GPU", h.cpuHeavy, g.frames)
	}
}

func TestCPULoad(t *testing.T) {
	var l cpuLoad
	t0 := time.Now()
	frame := func(at, took time.Duration) (time.Duration, bool) { return l.add(t0.Add(at), took, took) }
	const refresh = 16 * time.Millisecond
	// Frames of 2 ms at 60 Hz take an eighth of the time.
	for i := range 60 {
		if _, heavy := frame(time.Duration(i)*refresh, 2*time.Millisecond); heavy {
			t.Fatalf("frames of 2 ms are heavy at the %dth", i)
		}
	}
	// After a pause, frames of 6 ms are a burst of their own, heavy once
	// it lasted gpuBurst, and not slow.
	var first time.Duration
	for i := 0; first == 0 && i < 60; i++ {
		if lasted, heavy := frame(2*time.Second+time.Duration(i)*refresh, 6*time.Millisecond); heavy {
			first = lasted
		}
	}
	if first < gpuBurst || first > gpuBurst+refresh {
		t.Errorf("frames of 6 ms were heavy after %v", first)
	}
	if l.slow(refresh) {
		t.Error("frames of 6 ms are slow at 60 Hz")
	}
	// Frames slower than the display, each soon after the last, are one
	// burst, however far apart they begin.
	var lasted time.Duration
	var heavy bool
	for i := range 4 {
		lasted, heavy = frame(4*time.Second+time.Duration(i)*85*time.Millisecond, 80*time.Millisecond)
	}
	if !heavy || lasted < 300*time.Millisecond || !l.slow(refresh) {
		t.Errorf("frames of 80 ms: lasted %v, heavy %v, slow %v", lasted, heavy, l.slow(refresh))
	}
	// Frames of 2 ms drawn on 8 cores take as much CPU as frames of 16 ms:
	// heavy, though not slow.
	first = 0
	for i := 0; first == 0 && i < 60; i++ {
		if lasted, heavy := l.add(t0.Add(6*time.Second+time.Duration(i)*refresh), 2*time.Millisecond, 16*time.Millisecond); heavy {
			first = lasted
		}
	}
	if first < gpuBurst || first > gpuBurst+refresh {
		t.Errorf("frames of 2 ms on 8 cores were heavy after %v", first)
	}
	if l.slow(refresh) {
		t.Error("frames of 2 ms on 8 cores are slow at 60 Hz")
	}
}

// lazySurface is a test surface that gives the GPU on demand, as Linux's
// does, when it has one.
type lazySurface struct {
	testSurface
	hasGPU, given bool
	asked, idled  int
}

func (s *lazySurface) Native() platform.SurfaceNative {
	if s.given {
		return platform.SurfaceNative{HWND: 1}
	}
	return platform.SurfaceNative{Widget: 1}
}

func (s *lazySurface) UseGPU() bool {
	s.asked++
	s.given = s.hasGPU
	return s.given
}

func (s *lazySurface) Idle() { s.idled++ }

// lazyHost returns a window host on a lazySurface, a function drawing a
// frame, and one running what the host posted to the main thread.
func lazyHost(t *testing.T, hasGPU bool) (*windowHost, *lazySurface, func(), func()) {
	t.Setenv("MYGO_GPU", "")
	newGPU = func(n platform.SurfaceNative) (gpuRenderer, error) {
		if n.HWND == 0 {
			return nil, nil
		}
		return &testGPU{}, nil
	}
	t.Cleanup(func() { newGPU = newGPURenderer })
	s := &lazySurface{hasGPU: hasGPU}
	var mu sync.Mutex
	var posted []func()
	h := &windowHost{conn: &surface.Conn{Surface: s, Post: func(fn func()) {
		mu.Lock()
		posted = append(posted, fn)
		mu.Unlock()
	}}}
	h.rt = newRuntime(func(c *Context) { Text(c, "Hello") }, h)
	t.Cleanup(func() { h.idleTimer.Stop() })
	run := func() {
		mu.Lock()
		fns := posted
		posted = nil
		mu.Unlock()
		for _, fn := range fns {
			fn()
		}
	}
	return h, s, func() { h.event(platform.SurfaceEvent{Kind: platform.SurfaceFrame}) }, run
}

// burst notes frames drawn in memory at 60 Hz for lasting, each taking
// took on one core, as the frames of a lazy surface's host would.
func burst(h *windowHost, start time.Time, lasted, took time.Duration) {
	burstOn(h, start, lasted, took, took)
}

// burstOn notes frames drawn in memory at 60 Hz for lasting, each taking
// took, and cpu of CPU time on several cores.
func burstOn(h *windowHost, start time.Time, lasted, took, cpu time.Duration) {
	for at := time.Duration(0); at < lasted; at += 16 * time.Millisecond {
		h.noteCPU(start.Add(at), took, cpu)
	}
}

func TestLazyGPUWhenIdle(t *testing.T) {
	h, s, frame, run := lazyHost(t, true)
	frame()
	if s.pixels != 1 || s.asked != 0 || h.gpu != nil || h.soft.Image.Pix == nil {
		t.Fatalf("first frame: %d in memory, asked %d times", s.pixels, s.asked)
	}
	// Idle, the host frees its frame and lets the surface give memory back.
	h.lastFrame = time.Now().Add(-frameIdle)
	h.idle()
	if h.soft.Image.Pix != nil || s.idled != 1 || s.asked != 0 {
		t.Fatalf("idle: frame kept %v, %d idles, asked %d times", h.soft.Image.Pix != nil, s.idled, s.asked)
	}
	// Light frames ask for nothing; heavy ones for the GPU, once idle.
	burst(h, time.Now(), time.Second, 2*time.Millisecond)
	burst(h, time.Now().Add(2*time.Second), 400*time.Millisecond, 8*time.Millisecond)
	run()
	if !h.wantGPU || s.asked != 0 {
		t.Fatalf("after a heavy burst: wanted %v, asked %d times", h.wantGPU, s.asked)
	}
	h.lastFrame = time.Now().Add(-gpuIdle / 2)
	h.idle()
	if s.asked != 0 {
		t.Fatal("asked for the GPU before the window was idle")
	}
	h.lastFrame = time.Now().Add(-gpuIdle)
	h.idle()
	if s.asked != 1 || h.wantGPU {
		t.Fatalf("idle after a heavy burst: asked %d times", s.asked)
	}
	frame()
	if h.gpu == nil || h.path != "drawn on the GPU" || s.pixels != 1 {
		t.Errorf("the frame after: %q, %d in memory", h.path, s.pixels)
	}
}

func TestLazyGPUAtOnce(t *testing.T) {
	// A burst going on asks at once.
	h, s, frame, run := lazyHost(t, true)
	frame()
	burst(h, time.Now(), gpuBurstLong+100*time.Millisecond, 6*time.Millisecond)
	run()
	if s.asked != 1 {
		t.Errorf("a long heavy burst asked %d times", s.asked)
	}
	// So do frames the CPU draws slower than the display shows them.
	h, s, frame, run = lazyHost(t, true)
	frame()
	start := time.Now()
	for at := time.Duration(0); at < 300*time.Millisecond; at += 40 * time.Millisecond {
		h.noteCPU(start.Add(at), 30*time.Millisecond, 30*time.Millisecond)
	}
	run()
	if s.asked != 1 {
		t.Errorf("slow frames asked %d times", s.asked)
	}
	frame()
	if h.gpu == nil {
		t.Errorf("the frame after: %q", h.path)
	}
	// So does a burst of frames that last little but take several cores,
	// as scrolling a large window does.
	h, s, frame, run = lazyHost(t, true)
	frame()
	burstOn(h, time.Now(), gpuBurstLong+100*time.Millisecond, 1500*time.Microsecond, 12*time.Millisecond)
	run()
	if s.asked != 1 {
		t.Errorf("a long burst of frames on several cores asked %d times", s.asked)
	}
}

func TestLazyGPUNone(t *testing.T) {
	h, s, frame, run := lazyHost(t, false)
	frame()
	burst(h, time.Now(), gpuBurstLong+100*time.Millisecond, 8*time.Millisecond)
	run()
	burst(h, time.Now().Add(2*time.Second), gpuBurstLong+100*time.Millisecond, 8*time.Millisecond)
	run()
	frame()
	if s.asked != 1 || h.gpu != nil || s.pixels != 2 {
		t.Errorf("without a GPU: asked %d times, %d frames in memory", s.asked, s.pixels)
	}
}
