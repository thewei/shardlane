package filesview

import (
	"bytes"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// TestPreviewFlavorPinsTheTypeMap: markdown renders, the web-image set is
// explicit, and everything else previews as text.
func TestPreviewFlavorPinsTheTypeMap(t *testing.T) {
	cases := []struct {
		name string
		want Flavor
	}{
		{"README.md", FlavorMarkdown},
		{"notes.markdown", FlavorMarkdown},
		{"photo.PNG", FlavorImage},
		{"icon.ico", FlavorImage},
		{"anim.gif", FlavorImage},
		{"vector.SVG", FlavorImage},
		{"pic.avif", FlavorImage},
		{"main.go", FlavorText},
		{"data.json", FlavorText},
		{"Makefile", FlavorText},
		{"archive.tgz", FlavorText},
		{"noext", FlavorText},
	}
	for _, tc := range cases {
		if got := PreviewFlavor(tc.name); got != tc.want {
			t.Errorf("PreviewFlavor(%q) = %q, want %q", tc.name, got, tc.want)
		}
	}
}

// TestImageMIMEPinsDataURLTypes: the data-URL media type follows the file.
func TestImageMIMEPinsDataURLTypes(t *testing.T) {
	if got := ImageMIME("a.png"); got != "image/png" {
		t.Errorf("png mime = %q", got)
	}
	if got := ImageMIME("b.svg"); got != "image/svg+xml" {
		t.Errorf("svg mime = %q", got)
	}
	if got := ImageMIME("c.txt"); got != "" {
		t.Errorf("non-image mime = %q", got)
	}
}

// TestReadImageDataURLCapsAndEncodes: within the cap it returns a data
// URL; beyond it, ErrTooLarge routes the caller to the Native tier.
func TestReadImageDataURLCapsAndEncodes(t *testing.T) {
	root := t.TempDir()
	png := filepath.Join(root, "dot.png")
	// One transparent pixel, minimal PNG header + IDAT + IEND.
	raw := []byte{
		0x89, 'P', 'N', 'G', '\r', '\n', 0x1a, '\n', 0, 0, 0, 13, 'I', 'H', 'D', 'R',
		0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f, 0x15, 0xc4, 0x89,
		0, 0, 0, 10, 'I', 'D', 'A', 'T', 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01,
		0x0d, 0x0a, 0x2d, 0xb4, 0, 0, 0, 0, 'I', 'E', 'N', 'D', 0xae, 0x42, 0x60, 0x82,
	}
	if err := os.WriteFile(png, raw, 0o600); err != nil {
		t.Fatal(err)
	}
	url, err := ReadImageDataURL(png)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasPrefix(url, "data:image/png;base64,") || len(url) < len(raw) {
		t.Fatalf("unexpected data URL: %q", url[:min(48, len(url))])
	}

	huge := filepath.Join(root, "huge.png")
	if err := os.WriteFile(huge, make([]byte, ImagePreviewCap+1), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := ReadImageDataURL(huge); !errors.Is(err, ErrTooLarge) {
		t.Fatalf("oversize image error = %v, want ErrTooLarge", err)
	}
}

// TestReadTextBoundsAndTruncates: the native code view sees text up to
// TextPreviewCap — past the 1 MiB web cap — with Truncated telling the
// truth about the rest, and an empty file is just empty.
func TestReadTextBoundsAndTruncates(t *testing.T) {
	root := t.TempDir()
	small := filepath.Join(root, "small.txt")
	if err := os.WriteFile(small, []byte("hello"), 0o600); err != nil {
		t.Fatal(err)
	}
	preview, err := ReadText(small, root)
	if err != nil {
		t.Fatal(err)
	}
	if preview.Content != "hello" || preview.Truncated || preview.IsBinary {
		t.Fatalf("small text = %+v", preview)
	}

	empty := filepath.Join(root, "empty.txt")
	if err := os.WriteFile(empty, nil, 0o600); err != nil {
		t.Fatal(err)
	}
	if preview, err = ReadText(empty, root); err != nil || preview.Content != "" || preview.IsBinary {
		t.Fatalf("empty text = %+v, %v", preview, err)
	}

	mid := filepath.Join(root, "mid.txt")
	if err := os.WriteFile(mid, []byte(strings.Repeat("a", PreviewCap+10)), 0o600); err != nil {
		t.Fatal(err)
	}
	if preview, err = ReadText(mid, root); err != nil || len(preview.Content) != PreviewCap+10 || preview.Truncated {
		t.Fatalf("a file past the web cap must still read whole: len=%d truncated=%v err=%v", len(preview.Content), preview.Truncated, err)
	}

	big := filepath.Join(root, "big.txt")
	if err := os.WriteFile(big, []byte(strings.Repeat("a", TextPreviewCap+10)), 0o600); err != nil {
		t.Fatal(err)
	}
	if preview, err = ReadText(big, root); err != nil {
		t.Fatal(err)
	}
	if len(preview.Content) != TextPreviewCap || !preview.Truncated {
		t.Fatalf("big text len = %d truncated = %v", len(preview.Content), preview.Truncated)
	}
}

// TestIsBinaryDoesNotMistakeACutRuneForBinary: the probe stops at 8 KiB,
// which for 3-byte CJK runes always lands inside a rune. Such files are
// text; real binary and genuinely invalid bytes stay binary.
func TestIsBinaryDoesNotMistakeACutRuneForBinary(t *testing.T) {
	for _, n := range []int{2800, 2801, 2802, 2803} {
		if IsBinary([]byte(strings.Repeat("中", n))) {
			t.Errorf("%d CJK runes read as binary", n)
		}
	}
	// 4-byte runes cut at every offset of the rune.
	for pad := 0; pad < 4; pad++ {
		text := strings.Repeat("a", pad) + strings.Repeat("😀", 3000)
		if IsBinary([]byte(text)) {
			t.Errorf("emoji text with pad %d read as binary", pad)
		}
	}
	if !IsBinary(append(bytes.Repeat([]byte("a"), 100), 0x00)) {
		t.Error("a NUL byte must read as binary")
	}
	// Invalid bytes before the cut are still binary, long or short.
	if !IsBinary([]byte("ok\xff\xfeok")) {
		t.Error("short invalid UTF-8 must read as binary")
	}
	if !IsBinary(append([]byte("\xff"), bytes.Repeat([]byte("a"), 9000)...)) {
		t.Error("invalid byte inside the probe must read as binary")
	}
}

// TestReadTextShowsBytesPastTheProbeAsReplacementRunes: a stray byte past
// the classification probe renders as U+FFFD instead of invalid text.
func TestReadTextShowsBytesPastTheProbeAsReplacementRunes(t *testing.T) {
	root := t.TempDir()
	path := filepath.Join(root, "late.txt")
	body := append(bytes.Repeat([]byte("a"), 9000), 0xff)
	body = append(body, []byte("tail")...)
	if err := os.WriteFile(path, body, 0o600); err != nil {
		t.Fatal(err)
	}
	preview, err := ReadText(path, root)
	if err != nil {
		t.Fatal(err)
	}
	if preview.IsBinary || !strings.HasSuffix(preview.Content, "\uFFFDtail") {
		t.Fatalf("late stray byte: binary=%v tail=%q", preview.IsBinary, preview.Content[len(preview.Content)-8:])
	}
}
