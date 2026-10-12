package nativeui

/**
 * [INPUT]: 依赖 text_clip.go 的截断函数
 * [OUTPUT]: 验证按字节/行数截断的 Unicode 边界、截断标记与不截断的原样返回
 * [POS]: Inspector 摘要与工具输出预览共用截断工具的回归
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"strings"
	"testing"
	"unicode/utf8"
)

func TestClipNativeTextKeepsShortText(t *testing.T) {
	if got := clipNativeText("short", 5); got != "short" {
		t.Fatalf("text within the bound changed: %q", got)
	}
}

// A cut that lands inside a multi-byte rune backs off to the rune start, so
// the preview never ends in a broken character.
func TestClipNativeTextCutsOnRuneBoundary(t *testing.T) {
	got := clipNativeText("aé", 2)
	if want := "a\n… (truncated)"; got != want {
		t.Fatalf("clip = %q, want %q", got, want)
	}
	if !utf8.ValidString(got) {
		t.Fatalf("clip is not valid UTF-8: %q", got)
	}
}

func TestClipPreviewBoundsLinesThenCharacters(t *testing.T) {
	lines := "one\ntwo\nthree\nfour"
	if got := clipPreview(lines, 100, 2); got != "one\ntwo" {
		t.Fatalf("line bound = %q, want the first two lines", got)
	}
	if got := clipPreview("one\ntwo", 100, 4); got != "one\ntwo" {
		t.Fatalf("text inside both bounds = %q, want it unchanged", got)
	}
	long := strings.Repeat("x", 300)
	got := clipPreview(long, 200, 4)
	if !strings.HasPrefix(got, strings.Repeat("x", 200)) || !strings.HasSuffix(got, "(truncated)") {
		t.Fatalf("character bound = %d bytes, want 200 x's and a truncation marker", len(got))
	}
}
