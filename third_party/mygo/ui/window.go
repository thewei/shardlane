package ui

import (
	"image"
	"log"
	"os"
	"time"

	"github.com/egoist/mygo/internal/platform"
	"github.com/egoist/mygo/internal/raster"
	"github.com/egoist/mygo/internal/scene"
	"github.com/egoist/mygo/internal/surface"
	"github.com/egoist/mygo/internal/text"
)

// Content is a user interface for a window, the value of
// mygo.WindowOptions.Content. Create it with View.
type Content struct {
	view func(*Context)
}

// View returns the content of a window whose user interface view builds,
// for mygo.WindowOptions.Content:
//
//	mygo.NewWindow(mygo.WindowOptions{Title: "Counter", Content: ui.View(app.View)})
//
// view runs on the main thread whenever the window needs a frame: after
// input, after Context.Invalidate or Window.Invalidate, and while
// something animates. A Content can serve several windows, each with its
// own state.
func View(view func(c *Context)) *Content { return &Content{view: view} }

// RegisterFont adds a TrueType or OpenType font, or collection, that text
// can use with Font(family). An empty family keeps the font's own name.
func RegisterFont(data []byte, family string) error {
	return text.Shared().RegisterFont(data, family)
}

// AttachContent connects the content to a window; package mygo calls it.
func (v *Content) AttachContent(conn *surface.Conn) {
	h := &windowHost{conn: conn}
	rt := newRuntime(v.view, h)
	h.rt = rt
	conn.Event = h.event
	conn.ThemeChanged = func() {
		// The interface font is part of the appearance.
		h.uiFont()
		rt.themeChanged()
	}
	conn.TitleBarChanged = rt.requestFrame
	conn.Changed = rt.changed
	conn.Capture = h.capture
	conn.Detach = h.detach
	rt.insp.enabled = conn.DevTools
	conn.ToggleDevTools = func() {
		if rt.insp.enabled {
			rt.toggleInspector()
		}
	}
	h.uiFont()
	// Load the fonts while the window shows up.
	go text.Shared().Preload()
	conn.Surface.RequestFrame()
}

// windowHost presents frames on a window's surface, with a GPU renderer
// when the platform has one and in memory otherwise.
type windowHost struct {
	conn *surface.Conn
	rt   *engine
	gpu  gpuRenderer
	// gpuTried tells that the first frame tried the GPU, onGPU that its
	// renderer drew on a GPU, degraded that one drawing in software took
	// its place, and gpuSince when the renderer was made. After a failure,
	// retryAt is when a frame makes another, and backoff how long the next
	// failure waits.
	gpuTried bool
	onGPU    bool
	degraded bool
	gpuSince time.Time
	retryAt  time.Time
	backoff  time.Duration
	soft     raster.Renderer
	last     *scene.Scene
	// cpuShown tells that the frame shown is the one soft drew last.
	cpuShown bool
	// lastFrame is when the last frame was presented, frameEnd when it
	// was done, and gpuSinceCPU when the first since the CPU drew one, if
	// the GPU drew it.
	lastFrame, frameEnd, gpuSinceCPU time.Time
	// framing tells that the surface asked for the frame being drawn.
	framing bool
	// path is how the last frame was drawn, for MYGO_FRAME_STATS.
	path string
	// load measures the frames drawn in memory on a surface that gives
	// the GPU on demand (platform.LazyGPUSurface); wantGPU tells that they
	// cost enough to ask for it, and askedGPU that the host did.
	load              cpuLoad
	wantGPU, askedGPU bool
	// cpuBurst measures the frames the CPU draws for a pixelPresenter, and
	// cpuHeavy tells that those of the burst going on cost too much: the
	// rest of it draws on the GPU (see gpuLoad).
	cpuBurst cpuLoad
	cpuHeavy bool
	// wide tells that the GPU renderer draws the colors of scenes outside
	// the sRGB gamut (see wideHold), until wideUntil.
	wide      bool
	wideUntil time.Time
	// idleTimer runs idle once frames stop, at idleAt when idleArmed.
	idleTimer *time.Timer
	idleAt    time.Time
	idleArmed bool
	detached  bool
}

func (h *windowHost) framePath() string { return h.path }

// newGPU makes the GPU renderer of a surface; tests replace it.
var newGPU = newGPURenderer

// event handles an event of the surface, noting when it asks for a frame:
// only then may renderers draw (OpenGL's context is current only then).
func (h *windowHost) event(ev platform.SurfaceEvent) bool {
	if ev.Kind == platform.SurfaceFrame {
		h.framing = true
		defer func() { h.framing = false }()
	}
	return h.rt.event(ev)
}

// gpuRenderer draws scenes into a surface on the GPU.
type gpuRenderer interface {
	Render(s *scene.Scene) error
	Release()
}

// pixelPresenter is a GPU renderer that also shows frames drawn in memory,
// without the GPU, copying where they changed (Metal's).
type pixelPresenter interface {
	PresentPixels(pix []byte, stride, width, height int, scale float64, damage []image.Rectangle) error
}

// wideRenderer is a GPU renderer that can draw the colors of scenes outside
// the sRGB gamut (Metal's).
type wideRenderer interface {
	// SetWide has the frames drawn from now on show those colors, into
	// drawables of a wide gamut, or their nearest sRGB ones, and reports
	// whether they do.
	SetWide(on bool) bool
}

// A window whose screen shows more than sRGB, as a Mac's Display P3
// screen, draws the colors of ui.Oklch outside the sRGB gamut once a frame
// has some, on the GPU, whose drawables then hold float16 components, and
// goes on for wideHold after the last frame with some: a blinking caret or
// a color in transition does not switch the drawables back and forth. The
// CPU draws sRGB colors: frames meanwhile are the GPU's, and those after
// it may draw on the CPU again. A window on an sRGB screen draws the
// nearest sRGB colors, as it would show anyway, with the CPU's frames.
const wideHold = 2 * time.Second

// Frames that change little draw on the CPU, which the renderer copies
// into its drawables: a clock ticking, typing, the pointer over a button.
// The CPU draws them in less time than the GPU takes to start, and spares
// the memory Metal's driver holds for a couple of seconds after each frame
// it draws. Frames changing more than a sixteenth of the window since the
// last one while frames follow each other, as when scrolling or animating
// most of it, draw on the GPU, as do frames redrawing more than
// cpuMaxPixels. The next frame changing less draws on the CPU again, with
// what the GPU drew meanwhile: a progress bar moving on after a page slid
// in draws on the CPU, though frames never pause. Frames changing little
// may still cost much to draw, as an animation repainting translucent
// layers over gradients and shadows at the display's rate: once the CPU's
// frames of a burst cost too much, as a surface that gives the GPU on
// demand measures them (gpuLoad), the rest of the burst draws on the GPU,
// which Metal does in a millisecond or two of the CPU's time, and the
// frame after a pause on the CPU again.
const (
	burstGap     = 50 * time.Millisecond // frames closer follow each other
	cpuMaxPixels = 8 << 20
)

// A surface that gives the GPU on demand (Linux's, as OpenGL loads Mesa
// for good: some 50 MB) draws in memory until drawing on the CPU costs too
// much: CPU time of more than gpuLoad of the time of a burst of frames
// lasting gpuBurst or more, as scrolling or animating much of a large
// window may, whose frames draw on several cores at once. The host
// then asks for the GPU once the window has been idle for gpuIdle, so that
// loading the driver delays no frame, or at once when the burst goes on
// for gpuBurstLong, or when the CPU takes longer than a refresh of the
// display to draw its frames, which are late anyway.
const (
	gpuLoad      = 0.25
	gpuBurst     = 250 * time.Millisecond
	gpuBurstLong = time.Second
	gpuIdle      = 250 * time.Millisecond
)

// frameIdle is how long after the last frame the host frees the frame
// drawn in memory, as large as the window, and the surface gives back what
// its frames took (platform.IdleSurface). The next frame draws whole.
const frameIdle = 2 * time.Second

// cpuLoad measures how much of a burst of frames the CPU spent drawing
// them in memory.
type cpuLoad struct {
	// start is when the burst's first frame began drawing, end when its
	// last one was done; took is how long drawing its frames lasted, and
	// busy how much CPU time it took, more on several cores.
	start, end time.Time
	took, busy time.Duration
	frames     int
}

// add notes a frame begun at now that took d to draw, and cpu of CPU
// time, and returns how long its burst has lasted and whether drawing took
// CPU time of more than gpuLoad of it, once it lasted gpuBurst. A frame
// begun soon after the last was done is in its burst, however long they
// take.
func (l *cpuLoad) add(now time.Time, d, cpu time.Duration) (lasted time.Duration, heavy bool) {
	if now.Sub(l.end) >= burstGap {
		l.start, l.took, l.busy, l.frames = now, 0, 0, 0
	}
	l.end = now.Add(d)
	l.took += d
	l.busy += max(cpu, d)
	l.frames++
	lasted = l.end.Sub(l.start)
	return lasted, lasted >= gpuBurst && float64(l.busy) > gpuLoad*float64(lasted)
}

// slow reports whether the burst's frames took longer than interval each
// to draw, on average.
func (l *cpuLoad) slow(interval time.Duration) bool {
	return l.took > time.Duration(l.frames)*interval
}

func (h *windowHost) refreshRate() float32 { return float32(h.conn.Surface.RefreshRate()) }

func (h *windowHost) occluded() bool {
	s, ok := h.conn.Surface.(platform.OccludableSurface)
	return ok && s.Occluded()
}

func (h *windowHost) size() (float32, float32, float32) {
	w, ht, s := h.conn.Surface.Size()
	if s <= 0 {
		s = 1
	}
	return float32(w), float32(ht), float32(s)
}

func (h *windowHost) present(s *scene.Scene) {
	h.last, h.path = s, ""
	if s.Width <= 0 || s.Height <= 0 {
		return
	}
	if !h.framing {
		// A frame built outside the surface's, for a capture, is shown by
		// the surface's next.
		h.conn.Surface.RequestFrame()
		return
	}
	due := !h.retryAt.IsZero() && !time.Now().Before(h.retryAt)
	switch {
	case !h.gpuTried || h.gpu == nil && due:
		h.makeGPU()
	case due && h.degraded:
		// Drawing in software since the GPU went away: is it back?
		h.gpu.Release()
		h.gpu = nil
		h.makeGPU()
	}
	now := time.Now()
	burst := now.Sub(h.lastFrame) < burstGap
	if now.Sub(h.frameEnd) >= burstGap {
		// A pause, after the last frame was done however long it took:
		// the CPU's frames start a burst of their own.
		h.cpuHeavy, h.cpuBurst = false, cpuLoad{}
	}
	h.lastFrame = now
	defer func() { h.frameEnd = time.Now() }()
	h.armIdle(frameIdle)
	if !h.useWide(s, now) && h.drawOnCPU(s, burst) {
		h.gpuSinceCPU = time.Time{}
		if h.path == "" {
			h.path = "drawn on the CPU"
		}
		return
	}
	if h.render(s) {
		h.cpuShown = false
		h.gpuDrew(s, now)
		h.path = "drawn on the GPU"
		if h.degraded {
			h.path = "drawn by the GPU renderer in software"
		}
		return
	}
	h.path = "drawn in memory"
	drawing := time.Now()
	damage := h.soft.Render(s)
	// What other cores took to draw with this one.
	others := max(h.soft.CPU()-time.Since(drawing), 0)
	m := &h.soft.Image
	if d, ok := h.conn.Surface.(platform.DamageSurface); ok {
		d.PresentDamage(m.Pix, m.Stride, m.W, m.H, damage)
	} else {
		h.conn.Surface.PresentPixels(m.Pix, m.Stride, m.W, m.H)
	}
	took := time.Since(now)
	h.noteCPU(now, took, took+others)
}

// noteCPU notes that drawing and presenting a frame begun at now in memory
// took d, and cpu of CPU time, and, on a surface that gives the GPU on
// demand, asks for it once that costs too much (see gpuLoad).
func (h *windowHost) noteCPU(now time.Time, d, cpu time.Duration) {
	if _, ok := h.conn.Surface.(platform.LazyGPUSurface); !ok || h.askedGPU || h.conn.Post == nil {
		return
	}
	lasted, heavy := h.load.add(now, d, cpu)
	interval := time.Second / 60
	if hz := h.refreshRate(); hz > 0 {
		interval = time.Duration(float32(time.Second) / hz)
	}
	switch {
	case !heavy:
	case lasted >= gpuBurstLong || h.load.slow(interval):
		// The surface cannot change while it draws a frame.
		h.askedGPU = true
		h.conn.Post(h.useGPU)
	case !h.wantGPU:
		h.wantGPU = true
		h.armIdle(gpuIdle)
	}
}

// useGPU asks the surface for the GPU, which it gives from the next frame
// on, or has none to give: either way the host asks no more.
func (h *windowHost) useGPU() {
	h.wantGPU, h.askedGPU = false, true
	g, ok := h.conn.Surface.(platform.LazyGPUSurface)
	if h.detached || !ok || !g.UseGPU() {
		return
	}
	// The next frame makes the GPU renderer, which draws it whole.
	h.gpuTried = false
	h.soft.Release()
}

// armIdle has idle run d from now, unless it runs sooner already.
func (h *windowHost) armIdle(d time.Duration) {
	if h.conn.Post == nil {
		return
	}
	at := time.Now().Add(d)
	if h.idleArmed && !h.idleAt.After(at) {
		return
	}
	h.idleAt, h.idleArmed = at, true
	if h.idleTimer == nil {
		h.idleTimer = time.AfterFunc(d, func() { h.conn.Post(h.idle) })
	} else {
		h.idleTimer.Reset(d)
	}
}

// idle runs on the main thread after frames stop: it asks for the GPU
// that the last burst wanted once the window is idle for gpuIdle, then
// frees the frame drawn in memory, and has the surface give back memory,
// once it is for frameIdle. A frame since rearms it for the rest.
func (h *windowHost) idle() {
	h.idleArmed = false
	if h.detached {
		return
	}
	since := time.Since(h.lastFrame)
	if h.wantGPU {
		if since < gpuIdle {
			h.armIdle(gpuIdle - since)
			return
		}
		h.useGPU()
	}
	if since < frameIdle {
		h.armIdle(frameIdle - since)
		return
	}
	h.soft.Release()
	if s, ok := h.conn.Surface.(platform.IdleSurface); ok {
		s.Idle()
	}
}

// drawOnCPU draws s on the CPU and has the GPU renderer present it, when
// it changes little (see pixelPresenter) and the CPU's frames of the burst
// cost little, and reports whether it did.
func (h *windowHost) drawOnCPU(s *scene.Scene, burst bool) bool {
	p, ok := h.gpu.(pixelPresenter)
	if !ok || h.cpuHeavy {
		return false
	}
	draw, changed := h.soft.Changes(s)
	if draw > cpuMaxPixels || burst && changed > s.Width*s.Height/16 {
		return false
	}
	drawing := time.Now()
	damage := h.soft.Render(s)
	// What other cores took to draw with this one.
	others := max(h.soft.CPU()-time.Since(drawing), 0)
	defer func() {
		took := time.Since(drawing)
		h.noteCPUFrame(drawing, took, took+others)
	}()
	if len(damage) == 0 && h.cpuShown {
		// The frame shown is the same: a drawing asking for frames that
		// did not move, as a spinner between its steps.
		h.path = "unchanged"
		return true
	}
	m := &h.soft.Image
	if err := p.PresentPixels(m.Pix, m.Stride, m.W, m.H, float64(s.Scale), damage); err != nil {
		log.Printf("mygo: presenting a frame drawn in memory: %v", err)
		h.cpuShown = false
		return false
	}
	h.cpuShown = true
	return true
}

// useWide has the GPU renderer draw the colors of s outside the sRGB gamut
// when the window shows them (see wideHold), and reports whether it does.
func (h *windowHost) useWide(s *scene.Scene, now time.Time) bool {
	w, ok := h.gpu.(wideRenderer)
	if !ok {
		return false
	}
	if len(s.Wide) > 0 {
		h.wideUntil = now.Add(wideHold)
	}
	if on := now.Before(h.wideUntil) && h.screenWide(); on != h.wide {
		h.wide = w.SetWide(on)
	}
	return h.wide
}

// screenWide reports whether the window's screen shows more than sRGB.
func (h *windowHost) screenWide() bool {
	w, ok := h.conn.Surface.(platform.WideGamutSurface)
	return ok && w.WideGamut()
}

// noteCPUFrame notes that drawing and presenting a frame begun at now on
// the CPU, for the GPU renderer to present, took d, and cpu of CPU time,
// and draws the rest of the burst on the GPU once its frames cost too
// much (see gpuLoad).
func (h *windowHost) noteCPUFrame(now time.Time, d, cpu time.Duration) {
	if _, heavy := h.cpuBurst.add(now, d, cpu); heavy {
		h.cpuHeavy = true
	}
}

// gpuDrew notes that the GPU drew s, where frames drawn on the CPU show
// too: the CPU's frame compares the next one with s, so that a frame
// changing little draws on the CPU again. Once the GPU has drawn alone for
// a second, as while it animates much of the window, the CPU's frame frees
// its pixels, which the CPU then draws whole.
func (h *windowHost) gpuDrew(s *scene.Scene, now time.Time) {
	if _, ok := h.gpu.(pixelPresenter); !ok {
		return
	}
	h.soft.Skip(s)
	if h.gpuSinceCPU.IsZero() {
		h.gpuSinceCPU = now
	} else if now.Sub(h.gpuSinceCPU) > time.Second && h.soft.Image.Pix != nil {
		h.soft.ReleaseImage()
	}
}

// render draws s with the GPU renderer and reports whether it did. One
// that fails gives way to another made at once, in case the GPU is back
// already, or another, or Windows' software rasterizer, can draw; when
// that fails too, frames are drawn in memory until a later try.
func (h *windowHost) render(s *scene.Scene) bool {
	for try := 0; h.gpu != nil; try++ {
		err := h.gpu.Render(s)
		if err == nil {
			return true
		}
		log.Printf("mygo: drawing without the GPU: %v", err)
		h.gpu.Release()
		h.gpu = nil
		if try > 0 {
			h.retryLater()
			return false
		}
		if time.Since(h.gpuSince) > time.Minute {
			h.backoff = 0 // it worked for a while: the waits start over
		}
		h.makeGPU()
	}
	return false
}

// makeGPU makes the surface's GPU renderer: for the first frame, after one
// failed, and when the wait after a failure is over. Where the first frame
// cannot have one, the window draws in memory for good, and where it has a
// GPU, a renderer that draws in software in its place tries for the GPU
// again from time to time.
func (h *windowHost) makeGPU() {
	retry := h.gpuTried
	h.cpuShown = false
	h.gpuTried, h.retryAt = true, time.Time{}
	n := h.conn.Surface.Native()
	if os.Getenv("MYGO_GPU") == "0" || n == (platform.SurfaceNative{}) {
		return
	}
	r, err := newGPU(n)
	switch {
	case err != nil:
		log.Printf("mygo: drawing without the GPU: %v", err)
		if retry {
			h.retryLater()
		}
		return
	case r == nil:
		return
	case !retry:
		h.onGPU = !software(r)
	case h.onGPU && software(r):
		if !h.degraded {
			log.Print("mygo: drawing in software until the GPU is back")
		}
		h.degraded = true
		h.retryLater()
	default:
		if !software(r) {
			log.Print("mygo: drawing with the GPU again")
		}
		h.degraded = false
	}
	h.gpu, h.gpuSince = r, time.Now()
	h.wide = false // the renderer draws sRGB until told otherwise
}

// software reports whether a renderer draws on the CPU, as Direct3D's WARP.
func software(r gpuRenderer) bool {
	s, ok := r.(interface{ Software() bool })
	return ok && s.Software()
}

// retryLater has a frame make a GPU renderer again after a wait, which
// doubles with each failure up to half a minute: a driver that resets, a
// GPU unplugged or a machine waking up may take a while. A window that
// draws nothing meanwhile is not woken for it.
func (h *windowHost) retryLater() {
	h.backoff = min(max(2*h.backoff, time.Second), 30*time.Second)
	h.retryAt = time.Now().Add(h.backoff)
}

// capture renders the last frame in memory.
func (h *windowHost) capture() (int, int, []byte) {
	if h.last == nil {
		h.rt.runFrame()
	}
	s := h.last
	if s == nil {
		return 0, 0, nil
	}
	var img raster.Image
	img.Resize(s.Width, s.Height)
	raster.Render(&img, s)
	return img.W, img.H, img.RGBA()
}

func (h *windowHost) detach() {
	h.detached = true
	if h.idleTimer != nil {
		h.idleTimer.Stop()
	}
	h.rt.close()
	h.soft.Release()
	if h.gpu != nil {
		h.gpu.Release()
		h.gpu = nil
	}
}

func (h *windowHost) requestFrame() { h.conn.Surface.RequestFrame() }

func (h *windowHost) post(fn func()) {
	if h.conn.Post != nil {
		h.conn.Post(fn)
	} else {
		h.conn.Invalidate()
	}
}

// uiFont gives system-ui the desktop's interface font, and text the
// desktop's settings for rasterizing it, where the text system does not
// know them.
func (h *windowHost) uiFont() {
	if h.conn.UIFont != nil {
		text.Shared().SetUIFamily(h.conn.UIFont())
	}
	if h.conn.FontRendering != nil {
		r := h.conn.FontRendering()
		text.Shared().SetFontRendering(r.Antialias, r.Hinting, r.Subpixels)
	}
}

func (h *windowHost) setCursor(c Cursor) { h.conn.Surface.SetCursor(platform.Cursor(c)) }

func (h *windowHost) setTextInput(t platform.TextInputState) { h.conn.Surface.SetTextInput(t) }

func (h *windowHost) updateAccessibility(tree *platform.AccessTree) {
	h.conn.Surface.UpdateAccessibility(tree)
}

func (h *windowHost) readClipboard() string {
	if h.conn.Clipboard == nil {
		return ""
	}
	return h.conn.Clipboard.ReadText()
}

func (h *windowHost) writeClipboard(s string) {
	if h.conn.Clipboard != nil {
		h.conn.Clipboard.WriteText(s)
	}
}

func (h *windowHost) startDrag() {
	if h.conn.StartDrag != nil {
		h.conn.StartDrag()
	}
}

func (h *windowHost) titleBarDoubleClicked() {
	if h.conn.TitleBarDoubleClicked != nil {
		h.conn.TitleBarDoubleClicked()
	}
}

func (h *windowHost) isDark() bool { return h.conn.IsDark != nil && h.conn.IsDark() }

func (h *windowHost) preferences() platform.Preferences {
	if h.conn.Preferences == nil {
		return platform.Preferences{}
	}
	return h.conn.Preferences()
}

func (h *windowHost) titleBar() TitleBar {
	if h.conn.TitleBar == nil {
		return TitleBar{}
	}
	t := h.conn.TitleBar()
	return TitleBar{Height: float32(t.Height), Left: float32(t.Left), Right: float32(t.Right)}
}

func (h *windowHost) invalidate() { h.conn.Invalidate() }

func (h *windowHost) openURL(u string, done func(error)) {
	if h.conn.OpenURL != nil {
		h.conn.OpenURL(u, done)
	}
}

func (h *windowHost) popupMenu(m *platform.Menu, x, y float32, chosen func(int)) {
	if h.conn.PopupMenu != nil {
		h.conn.PopupMenu(m, float64(x), float64(y), chosen)
	}
}
