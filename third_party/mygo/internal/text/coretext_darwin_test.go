//go:build darwin

package text

import (
	"bytes"
	"math"
	"slices"
	"testing"
)

func TestCoreTextSyntheticItalic(t *testing.T) {
	for _, tc := range []struct{ family, text string }{
		{"PingFang SC", "口中文斜体"}, {"system-ui", "口中文斜体"}, {"Helvetica, PingFang SC", "口中文斜体"}, {"PingFang SC", "gy"},
	} {
		t.Run(tc.family+tc.text, func(t *testing.T) {
			s := newSystem()
			e := s.engine().(*coreText)
			style := Style{Family: tc.family, Size: 64}
			plain := s.Layout(Params{Text: tc.text, Style: style})
			style.Italic = true
			italic := s.Layout(Params{Text: tc.text, Style: style})
			if len(plain.Lines[0].Glyphs) != len(italic.Lines[0].Glyphs) {
				t.Fatal("italic changed glyph count")
			}
			for i, g := range italic.Lines[0].Glyphs {
				p := plain.Lines[0].Glyphs[i]
				if p.Font == g.Font || ct.fontGetMatrix(g.Font.native).c != obliqueSkew {
					t.Fatalf("glyph %d did not get a separate synthetic font", i)
				}
				if tc.text == "gy" {
					id := uint16(g.ID)
					var bounds cgRect
					ct.fontGetBoundingRects(g.Font.native, ctFontOrientationDefault, &id, &bounds, 1)
					if bounds.y >= 0 {
						t.Fatal("descender case has no negative-y ink")
					}
				}
				a := e.glyph(p.Font, p.ID, 2, 0, Flat, false)
				b := e.glyph(g.Font, g.ID, 2, 0, Flat, false)
				if bytes.Equal(a.pix, b.pix) {
					t.Fatal("upright and italic pixels are identical")
				}
				for y := range b.h {
					for x := range b.w {
						if (x == 0 || x == b.w-1 || y == 0 || y == b.h-1) && b.pix[y*b.w+x] != 0 {
							t.Fatal("synthetic ink touches bitmap boundary")
						}
					}
				}
				// Exercise the atlas cache in both orders as well, small enough
				// for every glyph to fit the test system's 256×256 atlas.
				pi := s.Glyph(p.Font, p.ID, 0.5, 0, Flat, false)
				ii := s.Glyph(g.Font, g.ID, 0.5, 0, Flat, false)
				if !pi.OK || !ii.OK || (pi.X == ii.X && pi.Y == ii.Y) {
					t.Fatalf("upright and italic share an atlas entry: %+v %+v", pi, ii)
				}
				if again := s.Glyph(p.Font, p.ID, 0.5, 0, Flat, false); again != pi {
					t.Fatal("italic replaced upright cache entry")
				}
				if i == 0 && tc.text != "gy" {
					// 口 has matching vertical sides, so compare their
					// centroids near the top and bottom of the ink.
					top, bottom := inkCentroid(b, 0.2), inkCentroid(b, 0.8)
					if math.IsNaN(top) || math.IsNaN(bottom) || top-bottom < 10 {
						t.Fatalf("glyph top is not tilted right: top %v bottom %v", top, bottom)
					}
				}
			}
		})
	}
}

func inkCentroid(b bitmap, fraction float64) float64 {
	lo, hi := b.h, 0
	for y := range b.h {
		for x := range b.w {
			if b.pix[y*b.w+x] > 128 {
				lo, hi = min(lo, y), max(hi, y)
			}
		}
	}
	y := lo + int(float64(hi-lo)*fraction)
	var sum, weight float64
	for x := range b.w {
		v := float64(b.pix[y*b.w+x])
		sum += float64(x+b.left) * v
		weight += v
	}
	if weight == 0 {
		return math.NaN()
	}
	return sum / weight
}

func TestCoreTextTrueItalic(t *testing.T) {
	s := newSystem()
	l := s.Layout(Params{Text: "italic", Style: Style{Family: "system-ui", Size: 48, Italic: true}})
	identity := cgAffineTransform{a: 1, d: 1}
	for _, g := range l.Lines[0].Glyphs {
		if ct.fontGetSymbolicTraits(g.Font.native)&ctFontItalicTrait == 0 {
			t.Fatal("system Latin italic did not select a true italic face")
		}
		if m := ct.fontGetMatrix(g.Font.native); m != identity {
			t.Fatalf("true italic was transformed: %+v", m)
		}
	}
}

func TestCoreTextItalicFallbackSpans(t *testing.T) {
	s := newSystem()
	// In PingFang SC, both spans are in one font, which Core Text would
	// shape as one run.
	for _, family := range []string{"Helvetica", "PingFang SC"} {
		for _, text := range []string{"中文中文", "😀文中文"} {
			l := s.Layout(Params{Text: text, Style: Style{Family: family, Size: 32},
				Spans: EncodeSpans([]Span{{End: 2}, {End: 4, Italic: true}})})
			for _, g := range l.Lines[0].Glyphs {
				want := float64(0)
				if g.Cluster >= 2 {
					want = obliqueSkew
				}
				if got := ct.fontGetMatrix(g.Font.native).c; got != want {
					t.Fatalf("%s %q cluster %d: skew %v, want %v", family, text, g.Cluster, got, want)
				}
			}
		}
	}
}

// Core Text draws color glyphs upright whatever the font's matrix, so
// emoji in italic text keep the upright font, as in WebKit.
func TestCoreTextItalicColorGlyphs(t *testing.T) {
	s := newSystem()
	for _, family := range []string{"PingFang SC", "Helvetica"} {
		l := s.Layout(Params{Text: "中😀", Style: Style{Family: family, Size: 32, Italic: true}})
		for _, g := range l.Lines[0].Glyphs {
			want := obliqueSkew
			if g.Cluster == 1 {
				want = 0
			}
			if got := ct.fontGetMatrix(g.Font.native).c; got != want {
				t.Fatalf("%s cluster %d: skew %v, want %v", family, g.Cluster, got, want)
			}
		}
	}
}

// TestCoreTextLanguage holds the glyphs of Han characters to the language
// of the text: 直 and 骨 are drawn apart in Japanese and in Chinese, by
// the fonts Core Text falls back to and by a font's own forms for the
// language. A language that writes no Han characters leaves them as text
// without a language has them, and Latin letters keep their glyphs.
func TestCoreTextLanguage(t *testing.T) {
	s := newSystem()
	ids := func(family, lang string, spans []Span) []uint32 {
		l := s.Layout(Params{Text: "直骨 Ag", Style: Style{Family: family, Size: 32, Lang: lang}, Spans: EncodeSpans(spans)})
		var out []uint32
		for _, g := range l.Lines[0].Glyphs {
			out = append(out, g.ID)
		}
		return out
	}
	for _, family := range []string{"system-ui", "Helvetica"} {
		none, en, ja, hans := ids(family, "", nil), ids(family, "en", nil), ids(family, "ja", nil), ids(family, "zh-Hans", nil)
		if !slices.Equal(none, en) {
			t.Errorf("%s: English %v, without a language %v", family, en, none)
		}
		if slices.Equal(ja[:2], hans[:2]) {
			t.Errorf("%s: Japanese and Chinese Han glyphs are the same: %v", family, ja)
		}
		if !slices.Equal(ja[2:], hans[2:]) || !slices.Equal(ja[2:], none[2:]) {
			t.Errorf("%s: Latin glyphs changed with the language: %v %v %v", family, none, ja, hans)
		}
		// A span in Japanese in a Chinese text, and the other way round.
		if got := ids(family, "zh-Hans", []Span{{End: 2, Lang: "ja"}}); !slices.Equal(got, ja) {
			t.Errorf("%s: a Japanese span in Chinese text: %v, want %v", family, got, ja)
		}
		if got := ids(family, "ja", []Span{{End: 1}, {End: 2, Lang: "zh-Hans"}}); got[0] != ja[0] || got[1] != hans[1] {
			t.Errorf("%s: a Chinese span in Japanese text: %v", family, got)
		}
	}
	// The system's language for text without one.
	s.SetLanguage("ja")
	if got := ids("system-ui", "", nil); !slices.Equal(got, ids("system-ui", "ja", nil)) {
		t.Errorf("text without a language in a Japanese system: %v", got)
	}
}

// TestCoreTextLanguageSpacing checks that the room Core Text leaves
// between Han and Latin letters in Chinese and Japanese text counts in the
// width of a line, so that text laid out as wide as it measured stays on
// one line.
func TestCoreTextLanguageSpacing(t *testing.T) {
	s := newSystem()
	for _, lang := range []string{"", "zh-Hans", "ja"} {
		p := Params{Text: "2026年1月5日星期一", Style: Style{Size: 13, Lang: lang}}
		l := s.Layout(p)
		gs := l.Lines[0].Glyphs
		last := gs[len(gs)-1]
		if end := last.X + last.Advance - gs[0].X; end > l.Width+1e-3 {
			t.Errorf("%q: the pen ends at %v, past the line's width %v", lang, end, l.Width)
		}
		p.Width = l.Width
		if n := len(s.Layout(p).Lines); n != 1 {
			t.Errorf("%q: laid out as wide as it measured, the text takes %d lines", lang, n)
		}
	}
}
