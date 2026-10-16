package parser

import (
	"strings"
	"testing"
)

// Bugs found reviewing the inline parser; each test fails on 088606d5.

// An email autolink in text after an entity or an escape gets the
// offsets of the decoded text, not of the source.
func TestReviewEmailOffsets(t *testing.T) {
	for _, src := range []string{"a &amp; b x@y.com", `a \* b x@y.com`} {
		d := Parse(src)
		for _, in := range d.Inlines(1) {
			if in.Kind == Link {
				if got := d.Source()[in.Start:in.End]; got != "x@y.com" {
					t.Errorf("%q: link [%d,%d) is %q, want %q", src, in.Start, in.End, got, "x@y.com")
				}
			}
		}
	}
}

// Quadratic time: nested footnote-like brackets, $ math with
// ExtDollarMath, and \( without \).
func TestReviewQuadratic(t *testing.T) {
	checkLinear(t, "footnote brackets", Default, func(n int) string { return strings.Repeat("[^a", n) + strings.Repeat("]", n) })
	checkLinear(t, "dollar openers", Default|ExtDollarMath, repeat("$a "))
	checkLinear(t, "\\( openers", Default, repeat(`\(`))
}

// HTML entities are only names of the table followed by ;, not a
// legacy name's prefix.
func TestReviewEntityPrefix(t *testing.T) {
	if got, want := render("&copyright; &notit;", Default), "<p>&amp;copyright; &amp;notit;</p>\n"; got != want {
		t.Errorf("got %q, want %q", got, want)
	}
}

func TestReviewHealing(t *testing.T) {
	for _, c := range []struct{ src, want string }{
		// A tilde for "about" strikes the rest of the paragraph through.
		{"about ~200 keys. See", "<p>about ~200 keys. See</p>\n"},
		// Strong next to CJK text is not healed: ** shows.
		{"这是**重要", "<p>这是<strong>重要</strong></p>\n"},
		// An open code span skips healing emphasis: ** shows.
		{"Edit **`config.ya", "<p>Edit <strong><code>config.ya</code></strong></p>\n"},
		// $$ inside a code span counts as open math: $$ shows.
		{"Use `$$` for display math", "<p>Use <code>$$</code> for display math</p>\n"},
		// A destination with parentheses: raw syntax, and the added ) shows.
		{"See [Go](https://en.wikipedia.org/wiki/Go_(lang", `<p>See <a href="https://en.wikipedia.org/wiki/Go_(lang)">Go</a></p>` + "\n"},
		// The text of a link being written shows its [.
		{"See the [Go blog", "<p>See the Go blog</p>\n"},
		// Strikethrough being closed shows its tildes.
		{"a ~~struck~", "<p>a <del>struck</del></p>\n"},
		// A < destination: the added ) shows.
		{"see [a](<htt", `<p>see <a href="htt">a</a></p>` + "\n"},
	} {
		if got := streamed(c.src); got != c.want {
			t.Errorf("%q:\ngot  %q\nwant %q", c.src, got, c.want)
		}
	}
}
