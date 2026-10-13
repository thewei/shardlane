package filesview

import (
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

// TestReadTextHeadBoundsAndTruncates: the native fallback sees the head
// with Truncated telling the truth about the rest.
func TestReadTextHeadBoundsAndTruncates(t *testing.T) {
	root := t.TempDir()
	small := filepath.Join(root, "small.txt")
	if err := os.WriteFile(small, []byte("hello"), 0o600); err != nil {
		t.Fatal(err)
	}
	preview, err := ReadTextHead(small)
	if err != nil {
		t.Fatal(err)
	}
	if preview.Content != "hello" || preview.Truncated {
		t.Fatalf("small head = %+v", preview)
	}

	big := filepath.Join(root, "big.txt")
	if err := os.WriteFile(big, []byte(strings.Repeat("a", NativeTextHeadCap+10)), 0o600); err != nil {
		t.Fatal(err)
	}
	preview, err = ReadTextHead(big)
	if err != nil {
		t.Fatal(err)
	}
	if len(preview.Content) != NativeTextHeadCap || !preview.Truncated {
		t.Fatalf("big head len = %d truncated = %v", len(preview.Content), preview.Truncated)
	}

	// An invalid UTF-8 tail is trimmed, never mojibaked.
	mixed := filepath.Join(root, "mixed.bin")
	if err := os.WriteFile(mixed, append([]byte("ok"), 0xff, 0xfe), 0o600); err != nil {
		t.Fatal(err)
	}
	preview, err = ReadTextHead(mixed)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasPrefix(preview.Content, "ok") {
		t.Fatalf("mixed head = %q", preview.Content)
	}
}
