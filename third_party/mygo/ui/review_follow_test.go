package ui

import (
	"testing"
	"time"

	transfer "github.com/egoist/mygo/transfer"
)

func followTester(s *ListState, n *int, last *float32, below *float32) (*Tester, *time.Time) {
	now := time.Unix(1000, 0)
	tt := coreNewTester(func(c *context) {
		coreColumn(c).Grow(1).Children(func() {
			coreList(c, s, *n, func(i int) {
				h := float32(40)
				if i == *n-1 {
					h = *last
				}
				coreBox(c).Height(h).Children(func() { coreTextf(c, "Message %d", i) })
			}).Grow(1)
			coreBox(c).Height(*below)
		})
	}, 300, 400)
	tt.rt.clock = func() time.Time { return now }
	return tt, &now
}

// The list gets shorter (a composer below grows): no rows were added, yet
// the end glides.
func TestReviewFollowGlideOnShrink(t *testing.T) {
	n, last, below := 30, float32(40), float32(0)
	s := ListState{FollowEnd: true, FollowAnimation: &Smooth}
	tt, _ := followTester(&s, &n, &last, &below)
	tt.Frame()
	tt.Frame()
	if r, _ := rowBox(tt, &s, n-1); r.Y+r.H != 400 {
		t.Fatalf("at first the last row ends at %v", r.Y+r.H)
	}
	below = 100
	tt.Frame()
	if r, _ := rowBox(tt, &s, n-1); r.Y+r.H != 300 {
		t.Errorf("list 100 shorter: last row ends at %v, want 300 (it is hidden under the composer)", r.Y+r.H)
	}
}

// The last row shrinks back while it glides: the rows above it stay where
// they show, then go down to their place.
func TestReviewFollowShrinkDuringGlide(t *testing.T) {
	n, last, below := 30, float32(40), float32(0)
	s := ListState{FollowEnd: true, FollowAnimation: &Smooth}
	tt, now := followTester(&s, &n, &last, &below)
	tt.Frame()
	tt.Frame()
	last = 300
	tt.Frame()
	*now = now.Add(50 * time.Millisecond)
	tt.Frame()
	before, _ := rowBox(tt, &s, n-2)
	last = 40
	*now = now.Add(1 * time.Millisecond)
	tt.Frame()
	after, _ := rowBox(tt, &s, n-2)
	if after.Y > before.Y+1 {
		t.Errorf("the last row shrank back: the row above it jumped down from %v to %v (lag %v)", before.Y, after.Y, s.followLag.value)
	}
	*now = now.Add(2 * time.Second)
	tt.Frame()
	if r, _ := rowBox(tt, &s, n-1); r.Y+r.H != 400 {
		t.Errorf("after the glide, the last row ends at %v", r.Y+r.H)
	}
}

// The last row is removed while the list glides (a typing indicator goes).
func TestReviewFollowRemoveDuringGlide(t *testing.T) {
	n, last, below := 30, float32(40), float32(0)
	s := ListState{FollowEnd: true, FollowAnimation: &Smooth}
	tt, now := followTester(&s, &n, &last, &below)
	tt.Frame()
	tt.Frame()
	n++
	last = 200
	tt.Frame()
	*now = now.Add(50 * time.Millisecond)
	tt.Frame()
	before, _ := rowBox(tt, &s, n-2)
	n--
	last = 40
	*now = now.Add(1 * time.Millisecond)
	tt.Frame()
	after, _ := rowBox(tt, &s, n-1)
	// With the row it measured from gone, the list shows its rows in place,
	// not below it by what was left of the glide.
	if after.Y+after.H != 400 {
		t.Errorf("the last row went: the row above it moved from %v to %v, ending at %v (lag %v)", before.Y, after.Y, after.Y+after.H, s.followLag.value)
	}
}

func TestReviewFollowStaticContainers(t *testing.T) {
	for _, kind := range []string{"row", "maxh", "grid", "wrap"} {
		t.Run(kind, func(t *testing.T) {
			n := 30
			s := ListState{FollowEnd: true, FollowAnimation: &Smooth}
			now := time.Unix(1000, 0)
			calls := 0
			tt := coreNewTester(func(c *context) {
				list := func() *node {
					return coreList(c, &s, n, func(i int) {
						coreBox(c).Height(40).Children(func() { coreTextf(c, "M %d", i) })
					})
				}
				switch kind {
				case "row":
					coreRow(c).Grow(1).Children(func() { list().Grow(1) })
				case "maxh":
					coreColumn(c).Children(func() { list().MaxHeight(300) })
				case "grid":
					coreGrid(c).Grow(1).Children(func() { list().Height(300) })
				case "wrap":
					coreRow(c).Wrap().Children(func() { list().Height(300).Width(200); coreBox(c).Size(150, 50) })
				}
				calls++
			}, 300, 400)
			tt.rt.clock = func() time.Time { return now }
			for f := 0; f < 5; f++ {
				now = now.Add(16 * time.Millisecond)
				tt.Frame()
				if s.followLag.value != 0 {
					t.Errorf("frame %d: static list glides, lag %v", f, s.followLag.value)
				}
			}
		})
	}
}

func TestReviewFollowStreamInvariants(t *testing.T) {
	for _, tc := range []struct {
		name    string
		n       int
		justify Align
		header  bool
	}{{"many", 30, Start, false}, {"few-end", 2, End, false}, {"few-start", 2, Start, false}, {"headers", 30, Start, true}} {
		t.Run(tc.name, func(t *testing.T) {
			n := tc.n
			last := float32(20)
			header := func(i int) bool { return i%7 == 0 }
			s := ListState{FollowEnd: true, FollowAnimation: &Smooth}
			if tc.header {
				s.Header = header
			}
			now := time.Unix(1000, 0)
			tt := coreNewTester(func(c *context) {
				coreList(c, &s, n, func(i int) {
					h := float32(40)
					if tc.header && header(i) {
						h = 24
					}
					if i == n-1 {
						h = last
					}
					coreBox(c).Height(h).Children(func() { coreTextf(c, "M %d", i) })
				}).Grow(1).Justify(tc.justify)
			}, 300, 400)
			tt.rt.clock = func() time.Time { return now }
			tt.Frame()
			for f := 0; f < 200; f++ {
				now = now.Add(16 * time.Millisecond)
				if f%25 == 24 {
					n++
					last = 20
				} else if f < 150 {
					last += 7
				}
				tt.Frame()
				if !s.AtEnd() || !s.following {
					t.Fatalf("frame %d: AtEnd %v following %v", f, s.AtEnd(), s.following)
				}
				first, lastV := s.Visible()
				if false {
					var rs []Rect
					for i := max(0, n-4); i < n; i++ {
						r, _ := rowBox(tt, &s, i)
						rs = append(rs, r)
					}
					t.Fatalf("frame %d: visible %d..%d of %d: %v lag %v", f, first, lastV, n, rs, s.followLag.value)
				}
				if !tc.header {
					prev, _ := rowBox(tt, &s, first)
					for i := first + 1; i <= lastV; i++ {
						r, ok := rowBox(tt, &s, i)
						if d := r.Y - (prev.Y + prev.H); !ok || d > 0.01 || d < -0.01 {
							t.Fatalf("frame %d: row %d at %v after %v", f, i, r, prev)
						}
						prev = r
					}
				}
			}
			now = now.Add(3 * time.Second)
			tt.Frame()
			if r, _ := rowBox(tt, &s, n-1); r.Y+r.H != 400 && (tc.justify == End || tc.n > 8) {
				t.Errorf("rested: last row ends at %v", r.Y+r.H)
			}
			if s.followLag.moving || tt.rt.animating {
				t.Errorf("still animating after rest")
			}
		})
	}
}

func TestReviewSelectionGrowAndCopyData(t *testing.T) {
	a, b := "Alpha beta", "Gamma"
	var parts []SelectedText
	tt := coreNewTester(func(c *context) {
		coreColumn(c).Selectable().Padding(12).Gap(8).CopyData(func(p []SelectedText) transfer.Data {
			parts = p
			s := ""
			for _, x := range p {
				s += string([]rune(x.Text)[x.Start:x.End]) + "|"
			}
			return transfer.TextData(s)
		}).Children(func() {
			coreText(c, a)
			coreText(c, b)
		})
	}, 300, 150)
	selectBetween(t, tt, a, 6, b, 3)
	b = "Gamma delta"
	tt.Frame()
	tt.Key(Cmd, KeyC)
	if got := tt.Clipboard(); got != "beta|Gam|" {
		t.Errorf("after growth copied %q (%+v)", got, parts)
	}
	a = "Alpha beta and more"
	tt.Frame()
	tt.Command("copy")
	if got := tt.Clipboard(); got != "beta and more|Gam|" {
		t.Errorf("after first grew copied %q", got)
	}
	x, y := selectableAt(t, tt, b, 1)
	tt.RightClickAt(x, y)
	tt.SetClipboard("")
	if err := tt.ChooseMenuItem("Copy"); err != nil {
		t.Fatal(err)
	}
	if got := tt.Clipboard(); got != "beta and more|Gam|" {
		t.Errorf("menu copied %q", got)
	}
}

func TestReviewNestedInlineSpans(t *testing.T) {
	inner := "c"
	tt := coreNewTester(func(c *context) {
		coreRichText(c).Selectable().Children(func() {
			coreText(c, "a ")
			coreRichText(c, Span{Text: "b "}).Children(func() {
				coreText(c, inner).Bold()
			})
			coreText(c, " d")
		})
	}, 300, 100)
	for i := 0; i < 3; i++ {
		inner += "x"
		tt.Frame()
		for _, s := range tt.rt.texts {
			want := "a b " + inner + " d"
			if s.editor.source != want {
				t.Fatalf("frame %d: paragraph %q want %q", i, s.editor.source, want)
			}
			sc := s.spanCache()
			got := ""
			for _, sp := range sc.inline {
				got += sp.Text
			}
			if got != want {
				t.Fatalf("frame %d: spans %q want %q", i, got, want)
			}
		}
	}
}

func TestReviewFollowGlideOnWindowResize(t *testing.T) {
	n, last, below := 30, float32(40), float32(0)
	s := ListState{FollowEnd: true, FollowAnimation: &Smooth}
	tt, _ := followTester(&s, &n, &last, &below)
	tt.Frame()
	tt.SetSize(300, 340)
	tt.Frame()
	if r, _ := rowBox(tt, &s, n-1); r.Y+r.H != 340 {
		t.Errorf("window 60 shorter: last row ends at %v, want 340 (lag %v)", r.Y+r.H, s.followLag.value)
	}
}

// The user scrolls down (toward the end) during a glide: following stops.
func TestReviewFollowScrollDownDuringGlide(t *testing.T) {
	n, last, below := 30, float32(40), float32(0)
	s := ListState{FollowEnd: true, FollowAnimation: &Smooth}
	tt, now := followTester(&s, &n, &last, &below)
	tt.Frame()
	tt.Frame()
	n++
	last = 200
	tt.Frame()
	*now = now.Add(16 * time.Millisecond)
	tt.Move(100, 100)
	tt.Scroll(100, 100, 0, 20) // toward the end, less than the lag
	tt.Frame()
	if !s.following {
		t.Errorf("scrolling toward the end during the glide stopped following (AtEnd %v)", s.AtEnd())
	}
	*now = now.Add(2 * time.Second)
	tt.Frame()
	n++
	tt.Frame()
	*now = now.Add(2 * time.Second)
	tt.Frame()
	if r, _ := rowBox(tt, &s, n-1); r.Y+r.H != 400 {
		t.Errorf("after a row came, the last row ends at %v, want 400", r.Y+r.H)
	}
}
