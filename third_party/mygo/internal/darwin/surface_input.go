//go:build darwin

package darwin

import (
	"unicode/utf16"

	"github.com/egoist/mygo/internal/platform"
)

// Input methods see the text around the caret of the focused text input
// as their document: the content's TextInputState.Text, with the marked
// text, the composition, in place of the selection while there is one.
// NSTextInputClient counts in UTF-16 units of that document; the content
// gets rune offsets in TextInputState.Text.

// document returns the text input methods see, and where the selection,
// or the marked text, starts and ends in it, in runes.
func (s *surface) document() (doc []rune, start, end int) {
	text := []rune(s.input.Text)
	start = max(0, min(s.input.Start, len(text)))
	end = max(start, min(s.input.End, len(text)))
	if s.marked == "" {
		return text, start, end
	}
	m := []rune(s.marked)
	doc = make([]rune, 0, len(text)-(end-start)+len(m))
	doc = append(append(append(doc, text[:start]...), m...), text[end:]...)
	return doc, start, start + len(m)
}

// units returns how many UTF-16 units runes take.
func units(runes []rune) int {
	n := 0
	for _, r := range runes {
		n += utf16.RuneLen(r)
	}
	return n
}

// runeAt returns the rune of runes that UTF-16 unit u starts, or the end.
func runeAt(runes []rune, u int) int {
	for i, r := range runes {
		if u <= 0 {
			return i
		}
		u -= utf16.RuneLen(r)
	}
	return len(runes)
}

func (s *surface) markedRange() nsRange {
	if s.marked == "" {
		return nsRange{Location: nsNotFound}
	}
	doc, start, end := s.document()
	return nsRange{Location: uint(units(doc[:start])), Length: uint(units(doc[start:end]))}
}

func (s *surface) selectedRange() nsRange {
	if !s.input.Active {
		return nsRange{Location: nsNotFound}
	}
	doc, start, end := s.document()
	if s.marked != "" {
		return nsRange{Location: uint(units(doc[:start])) + s.markedSel.Location, Length: s.markedSel.Length}
	}
	return nsRange{Location: uint(units(doc[:start])), Length: uint(units(doc[start:end]))}
}

// textRange converts a range of the document, in UTF-16 units, into runes
// of TextInputState.Text: a range in the marked text stands for the
// selection it replaces.
func (s *surface) textRange(r nsRange) (from, to int) {
	doc, start, end := s.document()
	lo := runeAt(doc, int(min(r.Location, uint(1<<31))))
	hi := runeAt(doc, int(min(r.Location+r.Length, uint(1<<31))))
	if s.marked == "" {
		return lo, hi
	}
	n := len([]rune(s.input.Text))
	sel := max(0, min(s.input.End, n)) - max(0, min(s.input.Start, n))
	conv := func(i int, after bool) int {
		switch {
		case i <= start:
			return i
		case i < end: // in the marked text
			if after {
				return start + max(sel, 0)
			}
			return start
		}
		return i - (end - start) + max(sel, 0)
	}
	return conv(lo, false), conv(hi, true)
}

func (s *surface) substring(r nsRange, actual *nsRange) id {
	doc, _, _ := s.document()
	u := utf16.Encode(doc)
	lo := int(min(r.Location, uint(len(u))))
	hi := int(min(uint(lo)+r.Length, uint(len(u))))
	if actual != nil {
		*actual = nsRange{Location: uint(lo), Length: uint(hi - lo)}
	}
	str := send(send(class("NSAttributedString"), "alloc"), "initWithString:", uintptr(nsString(string(utf16.Decode(u[lo:hi])))))
	return autorelease(str)
}

func (s *surface) setMarkedText(text string, selected, replacement nsRange) {
	ev := platform.SurfaceEvent{Kind: platform.TextComposition, Text: text, Caret: runesBefore(text, int(selected.Location))}
	if replacement.Location != nsNotFound {
		ev.Replace = true
		ev.From, ev.To = s.textRange(replacement)
	}
	s.marked, s.markedSel = text, selected
	s.send(ev)
}

func (s *surface) insertText(text string, replacement nsRange) {
	ev := platform.SurfaceEvent{Kind: platform.TextInput, Text: text}
	if replacement.Location != nsNotFound {
		ev.Replace = true
		ev.From, ev.To = s.textRange(replacement)
	}
	if s.marked != "" {
		s.marked = ""
		s.send(platform.SurfaceEvent{Kind: platform.TextComposition})
	}
	if text != "" || ev.Replace {
		s.send(ev)
	}
}
