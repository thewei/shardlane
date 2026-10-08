package ui

import (
	"fmt"
	"hash/maphash"
	"time"

	"github.com/egoist/mygo/internal/scene"
	"github.com/egoist/mygo/internal/text"
)

// Context builds a window's user interface for one frame. The window's
// view function receives it on the main thread; it is only valid during
// that call.
type Context struct {
	rt       *engine
	parent   *Element
	root     *Element
	chunks   [][]Element
	used     int
	theme    *Theme
	now      time.Time
	w, h     float32
	titleBar TitleBar
	overlay  *Element
	// tree is the Tree being built, for its items.
	tree *treeBuild
	// buttons is how the buttons being built look, in a toolbar or a
	// group of toggles.
	buttons buttonStyle
	// form and accordion are the Form and the Accordion being built, for
	// their fields and sections.
	form      *formBuild
	accordion *accordionBuild
	// checks is the CheckboxGroup being built.
	checks *checkGroup
	// row is the row of a List or a Table being built, and sidebar the
	// Sidebar.
	row     *rowBuild
	sidebar *sidebarBuild
	// reveal lists the elements to scroll into view (ScrollIntoView).
	reveal []*Element
	// router is the Router whose page is being built, for Links; routers
	// counts the Routers built, the first of which takes the window's
	// keys for going back and forward. inert is set while a page going
	// away is built, whose shortcuts wait.
	router  *Router
	routers int
	inert   bool
	// spare are the chunks of the frame before, which the engine keeps
	// while elements of that frame may leave with an exit transition, to
	// copy them (engine.exitsBuilt).
	spare [][]Element
	// transitions are the elements given a Transition, in the order asked,
	// and dividers those drawing lines between their children.
	transitions []transitionUse
	dividers    []dividers
	// sortedUses and depthStarts are byDepth's.
	sortedUses  []transitionUse
	depthStarts []int
}

const chunkSize = 256

// alloc returns a zeroed element from the frame's arena.
func (c *Context) alloc() *Element {
	ci, ei := c.used/chunkSize, c.used%chunkSize
	if ci == len(c.chunks) {
		c.chunks = append(c.chunks, make([]Element, chunkSize))
	}
	c.used++
	e := &c.chunks[ci][ei]
	shadows, cols, rows, frags := e.shadows[:0], e.cols[:0], e.rows[:0], e.frags[:0]
	*e = Element{}
	e.shadows, e.cols, e.rows, e.frags = shadows, cols, rows, frags
	e.serial = int32(c.used)
	e.shrink = 1
	e.justify, e.align, e.self, e.alignContent = alignAuto, alignAuto, alignAuto, alignAuto
	e.justifyItems, e.justifySelf = alignAuto, alignAuto
	return e
}

func (c *Context) reset(now time.Time, w, h float32) {
	c.used = 0
	c.now = now
	c.w, c.h = w, h
	c.theme = c.rt.defaultTheme()
	c.tree = nil
	c.reveal = c.reveal[:0]
	c.transitions = c.transitions[:0]
	c.dividers = c.dividers[:0]
	c.router, c.routers, c.inert = nil, 0, false
	root := c.alloc()
	root.c = c
	root.id = 1
	root.kind = kindBox
	root.width, root.height = px(w), px(h)
	root.st = c.rt.stateFor(root.id)
	root.bg = c.theme.Background
	root.ts = textStyle{set: setColor | setSize | setFamily, color: c.theme.Text, size: c.theme.FontSize, family: c.theme.Font}
	c.root = root
	c.parent = root
	c.overlay = nil
}

// overlayID is the ID of the layer of overlays.
var overlayID = mix(1, 0x6f7665726c6179)

// overlayRoot returns the layer above the window's content that Overlay
// builds into; the frame adds it to the root once the view returns.
func (c *Context) overlayRoot() *Element {
	if c.overlay == nil {
		o := c.alloc()
		o.c = c
		o.id = overlayID
		o.kind = kindBox
		o.flags = flagAbsolute | flagPassThrough
		o.inset = [4]length{px(0), px(0), px(0), px(0)}
		o.depth = 1
		o.st = c.rt.stateFor(o.id)
		c.overlay = o
	}
	return c.overlay
}

// newElement creates an element of kind as the last child of the current
// parent.
func (c *Context) newElement(k kind) *Element {
	if c.parent == nil {
		panic("ui: element created outside of a view function")
	}
	if c.parent.kind == kindText && k != kindText {
		panic("ui: only text elements (Text, Link, RichText) go inside a text")
	}
	e := c.alloc()
	e.c = c
	e.kind = k
	p := c.parent
	e.id = mix(p.id, uint64(p.nchild)+uint64(k)<<56)
	p.add(e)
	e.st = c.rt.stateFor(e.id)
	if c.rt.insp.open && e.id == c.rt.insp.selected {
		c.rt.insp.noteSource()
	}
	return e
}

var keySeed = maphash.MakeSeed()

func (c *Context) rekey(e *Element, k any) {
	parent := uint64(0)
	if e.parent != nil {
		parent = e.parent.id
	}
	id := keyedID(parent, k)
	rt := c.rt
	st, built := rt.lookState(id)
	if built && id != e.id {
		// Another element of the pass has the key under the same parent.
		rt.duplicateKey(id, k)
	}
	e.id, e.st = id, st
	if rt.insp.open {
		rt.insp.noteKey(id, k)
		if id == rt.insp.selected {
			rt.insp.noteSource()
		}
	}
}

// keyedID returns the ID of the child of parent keyed by k.
func keyedID(parent uint64, k any) uint64 {
	var h uint64
	switch k := k.(type) {
	case string:
		h = maphash.String(keySeed, k)
	case int:
		h = uint64(k)*0x9E3779B97F4A7C15 + 1
	case int64:
		h = uint64(k)*0x9E3779B97F4A7C15 + 1
	case uint64:
		h = k*0x9E3779B97F4A7C15 + 1
	default:
		h = maphash.String(keySeed, fmt.Sprintf("%T:%v", k, k))
	}
	return mix(parent, h^0xA5A5A5A5A5A5A5A5)
}

// mix combines two hashes (the finalizer of splitmix64).
func mix(a, b uint64) uint64 {
	h := a*0x9E3779B97F4A7C15 ^ (b + 0x632BE59BD9B4E019)
	h ^= h >> 30
	h *= 0xBF58476D1CE4E5B9
	h ^= h >> 27
	h *= 0x94D049BB133111EB
	h ^= h >> 31
	return h
}

// Theme returns the theme of the frame: the light or dark theme following
// the system's appearance, unless SetTheme replaced it.
func (c *Context) Theme() *Theme { return c.theme }

// SetTheme makes the frame use t, for the window's root and the widgets
// created after the call.
func (c *Context) SetTheme(t *Theme) {
	c.theme = t
	c.root.bg = t.Background
	c.root.ts.color, c.root.ts.size, c.root.ts.family = t.Text, t.FontSize, t.Font
}

// Size returns the size of the window's content in DIPs.
func (c *Context) Size() (width, height float32) { return c.w, c.h }

// TitleBar is the room the window controls take at the top of a window
// with a hidden title bar, in DIPs: they sit in a band of Height along the
// top edge, Left wide from the left edge and Right wide from the right.
type TitleBar struct{ Height, Left, Right float32 }

// TitleBar returns the room the window controls take in a window with a
// hidden title bar (mygo.WindowOptions.TitleBarStyle), whose view draws
// the title bar under them: the traffic lights on macOS, the window
// buttons on Linux and Windows. It is zero in other windows and in full
// screen, where the controls hide. Keep clear of the controls, and let the
// title bar drag the window:
//
//	bar := c.TitleBar()
//	ui.Row(c).Height(max(bar.Height, 32)).Padding(0, bar.Right, 0, bar.Left).DragWindow()
func (c *Context) TitleBar() TitleBar { return c.titleBar }

// Now returns the time the frame started, for animations.
func (c *Context) Now() time.Time { return c.now }

// Root returns the element holding the window's content: a column the
// size of the window.
func (c *Context) Root() *Element { return c.root }

// Invalidate asks for another frame. It is safe from any goroutine, for
// state that changed outside of the window's events.
func (c *Context) Invalidate() { c.rt.host.invalidate() }

// AnimationFrame asks for another frame as soon as the display can show
// it, for something moving. Call it in every frame while it moves. A
// drawing that moves while the layout stays asks with
// Painter.AnimationFrame instead, whose frames do not build the view.
func (c *Context) AnimationFrame() { c.rt.animating = true }

// After asks for another frame after d, for something that changes with
// time, such as a clock.
func (c *Context) After(d time.Duration) { c.rt.scheduleAt(c.now.Add(d)) }

// ReadClipboard returns the text on the clipboard, and WriteClipboard
// puts text there, for widgets that copy and paste themselves. Call them
// on the main thread: in the view, or in an input handler.
func (c *Context) ReadClipboard() string   { return c.rt.host.readClipboard() }
func (c *Context) WriteClipboard(s string) { c.rt.host.writeClipboard(s) }

// Announce asks screen readers to read text out once, after what they
// are reading, for news the keyboard focus does not bring, as a search
// done or a file saved: a Router announces the title of a page it shows,
// and a toast its text. It does nothing while no assistive technology
// reads the window.
func (c *Context) Announce(text string) {
	if text != "" && !c.inert {
		c.rt.announcements = append(c.rt.announcements, text)
	}
}

// OpenURL opens a URL in the default browser, or the app registered for
// its scheme, as a Link does. It returns at once; OpenURLThen tells what
// came of it.
func (c *Context) OpenURL(url string) { c.rt.host.openURL(url, nil) }

// OpenURLThen opens a URL as OpenURL does, and done, unless nil, gets what
// came of it in a while, before a frame builds anew with what it changed:
// an error when no app could open the URL.
//
//	c.OpenURLThen(url, func(err error) {
//		if err != nil {
//			app.failed = url
//		}
//	})
func (c *Context) OpenURLThen(url string, done func(err error)) {
	rt := c.rt
	var then func(error)
	if done != nil {
		then = func(err error) {
			done(err)
			rt.requestFrame()
		}
	}
	rt.host.openURL(url, then)
}

// Shortcut reports whether the key with exactly the modifiers mods was
// pressed, wherever the keyboard focus is, unless a focused element
// handled it first: a focused button or link takes Enter and Space, and a
// check box, switch or radio button Space, so that Enter can press a
// dialog's default button.
func (c *Context) Shortcut(mods Modifiers, key Key) bool {
	if !c.insideModal() || c.inert {
		return false // behind a dialog, or in a page going away
	}
	return c.rt.shortcut(0, mods, key)
}

// Local returns state of type T that element e keeps from frame to frame,
// keyed by key among its states; init creates it the first time. Custom
// widgets keep what they need with it, on the element they create.
func Local[T any](e *Element, key any, init func() T) *T {
	st := e.st
	if st.locals == nil {
		st.locals = map[any]any{}
	}
	if v, ok := st.locals[key]; ok {
		if p, ok := v.(*T); ok {
			return p
		}
	}
	v := new(T)
	*v = init()
	st.locals[key] = v
	return v
}

// state is what the runtime keeps about an element from frame to frame.
type state struct {
	id   uint64
	seen uint64
	pass int    // the pass of the frame that built it last
	born uint64 // the frame that first built the element
	// The element's box and its visible part in the last frame.
	x, y, w, h     float32
	vx, vy, vw, vh float32
	parent         uint64
	flags          uint32
	cursor         Cursor
	// tip marks an element with a tooltip (TooltipBase).
	tip bool

	clicks, rightClicks, doubleClicks int
	// pressMods are the modifiers held as the pointer went down on the
	// element, and clickMods those of its last click.
	pressMods, clickMods Modifiers
	// typed is what was typed to choose a row of a list taking the focus
	// for them since typedAt, and typing reports more since the last
	// frame.
	typed   string
	typedAt time.Time
	typing  bool
	// press is where the pointer went down in the element; dragX and dragY
	// add up its moves since the last frame.
	pressX, pressY float32
	dragX, dragY   float32
	pressed        bool

	// scrollX and scrollY are float64, as the content a List scrolls
	// may be taller than float32 counts to a fraction of a DIP.
	scrollX, scrollY float64
	contentW         float64
	contentH         float64
	// barInset is the element's ScrollbarInsets.
	barInset [4]float32
	// track is the ScrollState of the last frame's element, which events
	// that scroll it update (scrollTo).
	track *ScrollState
	// startX and startY are the offset before frame moveFrame moved it.
	moveFrame      uint64
	startX, startY float64
	// list is the ListState that placed the rows of a List last.
	list *ListState
	// cx, cy, cw and ch are the element's content box, inside its
	// padding, in the last frame.
	cx, cy, cw, ch float32

	changed, submitted bool
	// submitMods are the modifiers held with the Enter submitting a text
	// input, as Shift going back in a find bar.
	submitMods Modifiers
	// expand is what assistive technology asked of an item of a tree: 1
	// to open it, -1 to close it.
	expand int8
	// role is the element's Role in the last frame.
	role Role
	// holding is set while a stepper's arrow is held, since holdStart, and
	// holdSteps counts the steps it took.
	holding   bool
	holdStart time.Time
	holdSteps int
	dropped   []string
	// dragValue is the value the element drags (Drag), or dragFn returns
	// it as the drag starts; accepts reports whether it takes a value
	// dragged (Drop), and droppedValue is the value dropped on it, while
	// hasDropped, at (dropX, dropY).
	dragValue    any
	dragFn       func() any
	accepts      func(any) bool
	droppedValue any
	hasDropped   bool
	dropX, dropY float32
	editor       *editor
	// spans keeps what a text made of its spans in the last frame.
	spans     *spanCache
	locals    map[any]any
	anims     map[any]*anim
	shortcuts []shortcut
	delivered []shortcut

	// input, caret and takesText are those of the last frame's element
	// (HandleInput, TextCaret).
	input     func(InputEvent) bool
	caret     Rect
	takesText bool
	// scope is the dialog the element was in, 0 for none, and anchor the
	// element it was a popover of (AttachTo, PopoverBase).
	scope, anchor uint64
	// page is the Router's page the element was in, 0 for none.
	page uint64
	// trec is what the element's Transition keeps, by the engine's
	// transitions, when it has one.
	trec *transition
}

type shortcut struct {
	mods Modifiers
	key  Key
}

func (rt *engine) stateFor(id uint64) *state {
	s, _ := rt.lookState(id)
	return s
}

// lookState returns the state of the element id as stateFor does, and
// whether the pass built an element with that ID already.
func (rt *engine) lookState(id uint64) (s *state, built bool) {
	s = rt.states[id]
	if s == nil {
		if n := len(rt.free); n > 0 {
			// A state pruned, which nothing refers to any more.
			s, rt.free = rt.free[n-1], rt.free[:n-1]
			*s = state{id: id, born: rt.frame}
		} else {
			s = &state{id: id, born: rt.frame}
		}
		rt.states[id] = s
	} else {
		built = s.seen == rt.frame && s.pass == rt.pass
	}
	s.seen, s.pass = rt.frame, rt.pass
	// What the element drags and takes, as this pass asks.
	s.dragValue, s.dragFn, s.accepts = nil, nil, nil
	return s, built
}

// textSystem is the shared text system.
func textSystem() *text.System { return text.Shared() }

// sceneColor converts a color for the scene.
func sceneColor(c Color) scene.Color { return c.scene() }
