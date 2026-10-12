package ui

import (
	"testing"
	"time"
)

// built keeps the elements the view built last, by key.
var built = map[any]*node{}

// keep keeps e, built with key k, for keyed.
func keep(k any, e *node) *node {
	built[k] = e
	return e
}

// keyed returns the element of the last frame with key k.
func keyed(_ *Tester, k any) *node { return built[k] }

func TestTransitionSpringKeepsGoing(t *testing.T) {
	x := float32(0)
	a := Snappy
	tt, now := clockTester(func(c *context) {
		coreBox(c).Size(300, 100).Children(func() {
			keep("box", coreBox(c).Key("box")).Size(20, 20).Absolute().Left(x).Transition(ElementTransition{Animation: &a})
		})
	}, 300, 100)
	x = 200
	tt.Frame()
	*now = now.Add(100 * time.Millisecond)
	tt.Frame()
	r := tt.rt.trans[keyed(tt, "box").id]
	before, v := keyed(tt, "box").x, r.ch[0].velocity
	if before <= 0 || before >= 200 || v <= 0 {
		t.Fatalf("on its way, the box is at %v going %v/s", before, v)
	}
	// Sent back, it keeps going forward for a moment, then turns.
	x = 0
	tt.Frame()
	*now = now.Add(16 * time.Millisecond)
	tt.Frame()
	if e := keyed(tt, "box"); e.x <= before {
		t.Errorf("sent back, the box turned at once: %v, from %v", e.x, before)
	}
	if !keyed(tt, "box").Transitioning() {
		t.Error("the box moving is not transitioning")
	}
	*now = now.Add(2 * time.Second)
	tt.Frame()
	if e := keyed(tt, "box"); e.x != 0 || len(tt.rt.trans) != 1 || tt.rt.trans[e.id].moving != 0 {
		t.Errorf("once over, the box is at %v", e.x)
	}
	tt.Frame()
	if keyed(tt, "box").Transitioning() {
		t.Error("the box at rest is transitioning")
	}
}

func TestTransitionDelaysChanges(t *testing.T) {
	y := float32(0)
	tt, now := clockTester(func(c *context) {
		coreBox(c).Size(300, 100).Children(func() {
			for i := range 3 {
				a := Animation{Duration: 100 * time.Millisecond, Ease: Linear, Delay: time.Duration(i) * 50 * time.Millisecond}
				keep(i, coreBox(c).Key(i)).Size(20, 20).Absolute().Left(float32(i) * 30).Top(y).Transition(ElementTransition{Animation: &a})
			}
		})
	}, 300, 100)
	y = 60
	tt.Frame()
	*now = now.Add(75 * time.Millisecond)
	tt.Frame()
	for i, want := range []float32{45, 15, 0} {
		if e := keyed(tt, i); e.y != want {
			t.Errorf("staggered, box %d is at %v, not %v", i, e.y, want)
		}
	}
}

func TestTransitionEntersScaledAndTurned(t *testing.T) {
	show := false
	tt, now := clockTester(func(c *context) {
		coreBox(c).Size(200, 200).Children(func() {
			if show {
				keep("pop", coreBox(c).Key("pop")).Size(40, 40).Transition(ElementTransition{Duration: 100 * time.Millisecond, Ease: Linear,
					Enter: &Motion{Scale: 0.5, Rotate: 90, Opacity: 1},
					Exit:  &Motion{Scale: 2}})
			}
		})
	}, 200, 200)
	show = true
	tt.Frame()
	e := keyed(tt, "pop")
	if w := e.st.world; !w.Set || w.A > 0.01 || w.B < 0.49 || w.B > 0.51 {
		t.Fatalf("entering, the box shows as %+v", w)
	}
	*now = now.Add(50 * time.Millisecond)
	tt.Frame()
	b := keyed(tt, "pop").Bounds()
	if b.W <= 30 || b.W >= 60 {
		t.Errorf("halfway in, the box is %v wide", b.W)
	}
	*now = now.Add(time.Second)
	tt.Frame()
	if keyed(tt, "pop").st.world.Set {
		t.Error("in, the box is still transformed")
	}
	show = false
	tt.Frame()
	*now = now.Add(50 * time.Millisecond)
	tt.Frame()
	var ghost *node
	for g := tt.rt.c.root.first.first; g != nil; g = g.next {
		if g.leaving != 0 {
			ghost = g
		}
	}
	if ghost == nil || !ghost.world.Set || ghost.world.A < 1.4 || ghost.world.A > 1.6 {
		t.Fatalf("halfway out, the copy is %+v", ghost)
	}
}

func TestTransitionMatchesElements(t *testing.T) {
	detail := false
	type photo struct{ id int }
	tt, now := clockTester(func(c *context) {
		coreBox(c).Size(400, 300).Children(func() {
			tr := ElementTransition{Duration: 100 * time.Millisecond, Ease: Linear, Match: photo{7}}
			if !detail {
				coreBox(c).Key("grid").Absolute().Left(20).Top(30).Size(100, 100).Children(func() {
					keep("thumb", coreBox(c).Key("thumb")).Size(40, 20).Background(RGB(255, 0, 0)).Transition(tr)
				})
			} else {
				coreBox(c).Key("detail").Absolute().Left(100).Top(100).Size(200, 100).Children(func() {
					keep("hero", coreBox(c).Key("hero")).Size(200, 100).Background(RGB(0, 0, 255)).Transition(tr)
				})
			}
		})
	}, 400, 300)
	detail = true
	tt.Frame()
	if b := keyed(tt, "hero").Bounds(); !nearRect(b, Rect{20, 30, 40, 20}) {
		t.Fatalf("appearing, the hero shows at %v, not where the thumbnail was", b)
	}
	if p := tt.Image().RGBAAt(30, 35); p.R < 200 || p.B > 50 {
		t.Errorf("appearing, the hero is %v, not the thumbnail's red", p)
	}
	*now = now.Add(50 * time.Millisecond)
	tt.Frame()
	if b := keyed(tt, "hero").Bounds(); !nearRect(b, Rect{60, 65, 120, 60}) {
		t.Errorf("halfway, the hero shows at %v", b)
	}
	if keyed(tt, "hero").st.world.Set {
		t.Error("halfway, the hero is scaled rather than laid out at its size")
	}
	*now = now.Add(time.Second)
	tt.Frame()
	if b := keyed(tt, "hero").Bounds(); !nearRect(b, Rect{100, 100, 200, 100}) {
		t.Errorf("once over, the hero shows at %v", b)
	}
	if p := tt.Image().RGBAAt(150, 150); p.B < 250 {
		t.Errorf("once over, the hero is %v", p)
	}
	// And back.
	detail = false
	tt.Frame()
	if b := keyed(tt, "thumb").Bounds(); !nearRect(b, Rect{100, 100, 200, 100}) {
		t.Errorf("back, the thumbnail shows at %v", b)
	}
	// Without motion, it shows where it is.
	*now = now.Add(time.Second)
	tt.Frame()
	tt.SetPreferences(Preferences{ReduceMotion: true})
	detail = true
	tt.Frame()
	if b := keyed(tt, "hero").Bounds(); !nearRect(b, Rect{100, 100, 200, 100}) {
		t.Errorf("without motion, the hero shows at %v", b)
	}
}

func TestTransitionWithoutMotionFades(t *testing.T) {
	show := false
	bg := RGB(0, 0, 255)
	tt, now := clockTester(func(c *context) {
		coreBox(c).Size(200, 200).Children(func() {
			keep("color", coreBox(c).Key("color")).Size(50, 50).Background(bg).Transition(ElementTransition{Duration: 100 * time.Millisecond, Ease: Linear})
			if show {
				keep("fade", coreBox(c).Key("fade")).Size(20, 20).Transition(ElementTransition{Duration: 100 * time.Millisecond, Ease: Linear,
					Enter: &Motion{Y: 30}, Exit: &Motion{Y: 30}})
				keep("fold", coreBox(c).Key("fold")).Size(20, 20).Transition(ElementTransition{Duration: 100 * time.Millisecond, Ease: Linear,
					Enter: &Motion{Collapse: true}})
			}
		})
	}, 200, 200)
	tt.SetPreferences(Preferences{ReduceMotion: true})
	show = true
	bg = RGB(255, 0, 0)
	tt.Frame()
	*now = now.Add(50 * time.Millisecond)
	tt.Frame()
	if e := keyed(tt, "fade"); e.y != 50 || !e.opacitySet || e.opacity < 0.4 || e.opacity > 0.6 {
		t.Errorf("without motion, the box entering is at %v, %v opaque", e.y, e.opacity)
	}
	if e := keyed(tt, "fold"); e.h != 20 || e.opacitySet {
		t.Errorf("without motion, the box collapsing is %v high, %v opaque", e.h, e.opacity)
	}
	if p := tt.Image().RGBAAt(25, 25); p.R < 100 || p.B < 100 {
		t.Errorf("without motion, the colors jumped to %v", p)
	}
	*now = now.Add(time.Second)
	tt.Frame()
	show = false
	tt.Frame()
	*now = now.Add(50 * time.Millisecond)
	tt.Frame()
	leaving := 0
	for g := tt.rt.c.root.first.first; g != nil; g = g.next {
		if g.leaving != 0 {
			leaving++
			if g.y != 50 || g.opacity < 0.4 || g.opacity > 0.6 {
				t.Errorf("without motion, the copy leaving is at %v, %v opaque", g.y, g.opacity)
			}
		}
	}
	if leaving != 1 {
		t.Errorf("%d copies leaving", leaving)
	}
}
