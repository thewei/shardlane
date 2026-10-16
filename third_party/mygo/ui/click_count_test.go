package ui

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/internal/platform"
)

// A handler gets the count of clicks with a press and its release, and
// each button counts its own.
func TestHandlerClickCount(t *testing.T) {
	var clicks []int
	tt := NewTester(func(c *Context) {
		Box(c).Size(100, 100).HandleInput(func(ev InputEvent) bool {
			if ev.Kind == InputPointerDown || ev.Kind == InputPointerUp {
				clicks = append(clicks, ev.Clicks)
			}
			return true
		})
	}, 100, 100)
	defer tt.rt.close()
	tt.Frame()
	tt.ClickAt(50, 50)
	tt.ClickAt(50, 50)
	tt.RightClickAt(50, 50)
	tt.ClickAt(50, 50)
	if want := []int{1, 1, 2, 2, 1, 1, 1, 1}; !slices.Equal(clicks, want) {
		t.Fatalf("presses and releases saw clicks %v, want %v", clicks, want)
	}
}

// A release carries its press's count, though another button went down
// meanwhile.
func TestReleaseCarriesPressCount(t *testing.T) {
	var ups []int
	tt := NewTester(func(c *Context) {
		Box(c).Size(100, 100).HandleInput(func(ev InputEvent) bool {
			if ev.Kind == InputPointerUp && ev.Button == 0 {
				ups = append(ups, ev.Clicks)
			}
			return ev.Button == 0
		})
	}, 100, 100)
	defer tt.rt.close()
	tt.Frame()
	tt.ClickAt(50, 50)
	tt.Press(50, 50)
	tt.send(platform.SurfaceEvent{Kind: platform.PointerDown, X: 50, Y: 50, Button: 1})
	tt.Release(50, 50)
	if want := []int{1, 2}; !slices.Equal(ups, want) {
		t.Fatalf("releases saw clicks %v, want %v", ups, want)
	}
}

// A click that puts another element under the pointer does not make the
// next click on it a double click, inside a handler that leaves presses.
func TestClickOnNewElementIsNotDouble(t *testing.T) {
	opened := false
	doubles := 0
	tt := NewTester(func(c *Context) {
		Box(c).Size(200, 100).HandleInput(func(InputEvent) bool { return false }).Children(func() {
			if !opened {
				opened = Button(c.Key("open"), "Open").Size(100, 40).Clicked()
			} else if Button(c.Key("row"), "Row").Size(100, 40).DoubleClicked() {
				doubles++
			}
		})
	}, 200, 100)
	defer tt.rt.close()
	tt.Frame()
	tt.ClickAt(50, 20)
	tt.Frame()
	tt.Frame()
	if !opened {
		t.Fatal("the click did not open")
	}
	tt.ClickAt(50, 20)
	tt.Frame()
	if doubles != 0 {
		t.Fatal("the first click on the new element was a double click")
	}
}

// A backend's count, as macOS's, starts over on another element than the
// last press's: a click then a click on its neighbor is no double click,
// and the release carries the count its press got.
func TestBackendCountPerElement(t *testing.T) {
	var a, b []int
	record := func(to *[]int) func(InputEvent) bool {
		return func(ev InputEvent) bool {
			if ev.Kind == InputPointerDown {
				*to = append(*to, ev.Clicks)
			} else if ev.Kind == InputPointerUp && ev.Clicks != (*to)[len(*to)-1] {
				t.Errorf("a release carried clicks %d, its press %d", ev.Clicks, (*to)[len(*to)-1])
			}
			return true
		}
	}
	tt := NewTester(func(c *Context) {
		Row(c).Children(func() {
			Box(c).Size(50, 50).HandleInput(record(&a))
			Box(c).Size(50, 50).HandleInput(record(&b))
		})
	}, 100, 50)
	defer tt.rt.close()
	tt.Frame()
	for i, x := range []float64{25, 75, 75, 75, 25} {
		tt.send(platform.SurfaceEvent{Kind: platform.PointerDown, X: x, Y: 25, Clicks: i + 1})
		tt.send(platform.SurfaceEvent{Kind: platform.PointerUp, X: x, Y: 25, Clicks: i + 1})
	}
	if !slices.Equal(a, []int{1, 1}) || !slices.Equal(b, []int{1, 2, 3}) {
		t.Fatalf("the left box saw clicks %v and the right %v, want [1 1] and [1 2 3]", a, b)
	}
}

// Two handlers in one focusable element count apart: the element pressed
// is the innermost of the target and the handler.
func TestBackendCountPerHandlerInFocusable(t *testing.T) {
	var a, b []int
	record := func(to *[]int) func(InputEvent) bool {
		return func(ev InputEvent) bool {
			if ev.Kind == InputPointerDown {
				*to = append(*to, ev.Clicks)
			}
			return true
		}
	}
	tt := NewTester(func(c *Context) {
		Row(c).Focusable().Label("Grid").Children(func() {
			Box(c).Size(50, 50).HandleInput(record(&a))
			Box(c).Size(50, 50).HandleInput(record(&b))
		})
	}, 100, 50)
	defer tt.rt.close()
	tt.Frame()
	for i, x := range []float64{25, 75, 75} {
		tt.send(platform.SurfaceEvent{Kind: platform.PointerDown, X: x, Y: 25, Clicks: i + 1})
		tt.send(platform.SurfaceEvent{Kind: platform.PointerUp, X: x, Y: 25, Clicks: i + 1})
	}
	if !slices.Equal(a, []int{1}) || !slices.Equal(b, []int{1, 2}) {
		t.Fatalf("the left box saw clicks %v and the right %v, want [1] and [1 2]", a, b)
	}
}

// A press the scroll bar takes is of no element: a click on the content,
// one on the bar beside it, and one on the content again make no double
// click.
func TestScrollbarPressStartsOver(t *testing.T) {
	doubles := 0
	tt := coreNewTester(func(c *context) {
		coreScroll(c).Size(300, 200).Children(func() {
			row := coreBox(c).Key("row").Size(300, 2000)
			row.flags |= flagClickable
			if row.DoubleClicked() {
				doubles++
			}
		})
	}, 300, 200)
	defer tt.rt.close()
	tt.Frame()
	tt.Move(150, 100)
	var st *state
	for id := tt.rt.hitChain(150, 100)[0]; id != 0; id = st.parent {
		if st = tt.rt.states[id]; st.flags&flagScrollY != 0 {
			break
		}
	}
	bars := scrollBars(Rect{st.x, st.y, st.w, st.h}, st.barInset, float32(st.contentW), float32(st.contentH), 0, float32(st.scrollY), st.flags, tt.rt.c.theme.scrollbarWidth(), false)
	content, bar, y := bars.vTrack.X-1, bars.vTrack.X+4, bars.v.Y+2
	tt.Move(content, y)
	tt.ClickAt(content, y)
	tt.ClickAt(bar, y)
	tt.Frame()
	tt.ClickAt(content, y)
	tt.Frame()
	if doubles != 0 || tt.rt.pressClicks != 1 {
		t.Fatalf("%d double clicks, the last press counted %d, want none and 1", doubles, tt.rt.pressClicks)
	}
}
