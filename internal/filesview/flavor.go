// flavor.go decides how a file previews (FILES-07/08): the WebView tier
// renders text and Markdown up to PreviewCap and images up to
// ImagePreviewCap as data URLs; larger files fall back to the Native UI
// tier (bounded text head, or a decoded native bitmap). SVG stays text
// when it outgrows the image cap — it is XML source a developer can read.

package filesview

import (
	"bytes"
	"encoding/base64"
	"errors"
	"image"
	_ "image/gif"
	_ "image/jpeg"
	_ "image/png"
	"os"
	"path/filepath"
	"strings"
)

// Flavor names the three render paths a preview can take.
type Flavor string

const (
	FlavorText     Flavor = "text"
	FlavorMarkdown Flavor = "markdown"
	FlavorImage    Flavor = "image"
)

// ImagePreviewCap is the largest image the WebView tier takes as a data
// URL (24 MiB; text keeps the 1 MiB PreviewCap). Beyond it the Native UI
// tier renders.
const ImagePreviewCap = 24 << 20

// NativeTextHeadCap bounds the Native fallback's text head: a read-only
// glimpse (first 256 KiB) of a file that outgrows the WebView tier.
const NativeTextHeadCap = 256 << 10

// markdownExts render (default) instead of highlighting.
var markdownExts = map[string]struct{}{
	".md":       {},
	".markdown": {},
}

// imageExts are the formats WKWebView's <img> renders from a data URL.
var imageExts = map[string]string{
	".png":  "image/png",
	".jpg":  "image/jpeg",
	".jpeg": "image/jpeg",
	".gif":  "image/gif",
	".webp": "image/webp",
	".bmp":  "image/bmp",
	".svg":  "image/svg+xml",
	".ico":  "image/x-icon",
	".avif": "image/avif",
}

// PreviewFlavor maps a filename to its preview flavor. Everything that is
// neither Markdown nor an image previews as text (CodeMirror highlights by
// extension; unknown extensions render plain).
func PreviewFlavor(name string) Flavor {
	ext := strings.ToLower(filepath.Ext(name))
	if _, ok := markdownExts[ext]; ok {
		return FlavorMarkdown
	}
	if _, ok := imageExts[ext]; ok {
		return FlavorImage
	}
	return FlavorText
}

// ImageMIME returns the data-URL media type for an image filename.
func ImageMIME(name string) string {
	return imageExts[strings.ToLower(filepath.Ext(name))]
}

// ErrTooLarge reports a file beyond its flavor's WebView cap; the caller
// falls back to the Native UI tier.
var ErrTooLarge = errors.New("file is too large for the web preview")

// ReadImageDataURL reads an image up to ImagePreviewCap and returns it as
// a data URL for the WebView tier's <img>.
func ReadImageDataURL(path string) (string, error) {
	info, err := os.Stat(path)
	if err != nil {
		return "", err
	}
	if info.Size() > ImagePreviewCap {
		return "", ErrTooLarge
	}
	raw, err := os.ReadFile(path)
	if err != nil {
		return "", err
	}
	mime := ImageMIME(path)
	if mime == "" {
		mime = "application/octet-stream"
	}
	return "data:" + mime + ";base64," + base64.StdEncoding.EncodeToString(raw), nil
}

// MaxNativeImagePixels bounds the Native tier's decoded bitmap: 32M pixels
// (a 128 MiB RGBA buffer). DecodeConfig rejects insanity before the full
// decode allocates.
const MaxNativeImagePixels = 32 << 20

// ErrDimensionsTooLarge reports an image whose decoded size exceeds the
// Native tier's pixel budget.
var ErrDimensionsTooLarge = errors.New("image dimensions exceed the preview budget")

// DecodeImageBounded decodes an image for the Native UI tier (the WebView
// tier already refused it as too large): the pixel budget is checked via
// DecodeConfig before the full decode allocates. Runs off the UI lane; the
// caller wraps the result in ui.NewBitmap on the UI lane.
func DecodeImageBounded(path string) (image.Image, error) {
	raw, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	cfg, _, err := image.DecodeConfig(bytes.NewReader(raw))
	if err != nil {
		return nil, err
	}
	if int64(cfg.Width)*int64(cfg.Height) > MaxNativeImagePixels {
		return nil, ErrDimensionsTooLarge
	}
	img, _, err := image.Decode(bytes.NewReader(raw))
	if err != nil {
		return nil, err
	}
	return img, nil
}

// ReadTextHead reads up to NativeTextHeadCap bytes as displayable text for
// the Native fallback tier: oversized files preview as their head, with
// Truncated carrying the story. Invalid UTF-8 tails are trimmed.
func ReadTextHead(path string) (Preview, error) {
	info, err := os.Stat(path)
	if err != nil {
		return Preview{}, err
	}
	if info.IsDir() {
		return Preview{}, errors.New("cannot preview a directory")
	}
	file, err := os.Open(path)
	if err != nil {
		return Preview{}, err
	}
	defer file.Close()

	buf := make([]byte, min(NativeTextHeadCap, max(info.Size(), 1)))
	n, err := file.Read(buf)
	if err != nil && n == 0 {
		return Preview{}, err
	}
	head := strings.ToValidUTF8(string(buf[:n]), "")
	return Preview{
		Path:      path,
		SizeBytes: info.Size(),
		Truncated: info.Size() > int64(n),
		Content:   head,
	}, nil
}
