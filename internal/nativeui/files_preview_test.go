package nativeui

/**
 * [INPUT]: headless Files 页面、filesview 读取域、原生 editor 代码视图与 filesPreview 桥
 * [OUTPUT]: 2026-10-14 预览分级回归：文本/代码原生、Markdown 渲染⇄源码、网页图片、偏好关闭、截断/二进制/空文件/CJK、桥只放行页面拥有的文件、语法配色单一来源
 * [POS]: 只验证 nativeui 的分级接线与状态；读取/分类语义由 internal/filesview 测试覆盖
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"bytes"
	"context"
	"image"
	"image/png"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/wh-studio/herdr-client/internal/codehl"
	"github.com/wh-studio/herdr-client/internal/filesview"
)

func writePreviewFile(t *testing.T, root, name string, data []byte) string {
	t.Helper()
	path := filepath.Join(root, name)
	if err := os.WriteFile(path, data, 0o600); err != nil {
		t.Fatal(err)
	}
	return path
}

func tinyPNG(t *testing.T) []byte {
	t.Helper()
	var buf bytes.Buffer
	if err := png.Encode(&buf, image.NewRGBA(image.Rect(0, 0, 2, 2))); err != nil {
		t.Fatal(err)
	}
	return buf.Bytes()
}

// TestPreviewPlanSendsTextAndCodeToTheNativeView: text and code never go to
// the WebView page — even past the old 1 MiB web cap — and the view is a
// read-only editor holding the file with its language.
func TestPreviewPlanSendsTextAndCodeToTheNativeView(t *testing.T) {
	s, tester, root := filesOpsTestShell(t)
	goFile := writePreviewFile(t, root, "main.go", []byte("package main\n\nfunc main() {}\n"))
	big := writePreviewFile(t, root, "big.log", bytes.Repeat([]byte("line of a log\n"), 120_000)) // ~1.7 MiB

	s.openFilePreview(goFile)
	st := &s.rightPanel
	if st.nativePreviewPath != goFile || st.nativeCode == nil || s.webPreviewOwns(goFile) {
		t.Fatalf("go file: native=%q code=%v web=%v", st.nativePreviewPath, st.nativeCode != nil, s.webPreviewOwns(goFile))
	}
	if !st.nativeCode.ReadOnly || st.nativeCode.Buffer.String() != "package main\n\nfunc main() {}\n" {
		t.Fatalf("code state = readonly:%v text:%q", st.nativeCode.ReadOnly, st.nativeCode.Buffer.String())
	}
	if st.nativeCode.Buffer.Language() == nil || st.nativeCode.SoftWrap {
		t.Fatalf("code must highlight by extension and scroll sideways: lang=%v wrap=%v",
			st.nativeCode.Buffer.Language(), st.nativeCode.SoftWrap)
	}
	tester.Frame()
	if !tester.HasText("main.go") || !tester.HasText("29 B") {
		t.Fatalf("viewer bar must carry the name and size: %q", tester.Texts())
	}
	if tester.HasText("Rendered") {
		t.Fatal("only Markdown offers the Rendered/Source choice")
	}

	s.openFilePreview(big)
	if st.nativePreviewPath != big || st.nativeCode == nil || st.nativeText.Truncated {
		t.Fatalf("a file past the old web cap must show whole: native=%q code=%v", st.nativePreviewPath, st.nativeCode != nil)
	}
	if st.nativeCode.SoftWrap != (st.nativeCode.Buffer.Language() == nil) {
		t.Fatal("plain text wraps, a known language does not")
	}
}

// TestPreviewPlanTruncatesPastTheTextCapTruthfully: an 8 MiB head with the
// pill that says so, never silence.
func TestPreviewPlanTruncatesPastTheTextCapTruthfully(t *testing.T) {
	s, tester, root := filesOpsTestShell(t)
	huge := writePreviewFile(t, root, "huge.txt", bytes.Repeat([]byte("a"), filesview.TextPreviewCap+1))
	s.openFilePreview(huge)
	st := &s.rightPanel
	if st.nativeText == nil || !st.nativeText.Truncated || st.nativeCode == nil {
		t.Fatalf("huge text = %+v code=%v", st.nativeText, st.nativeCode != nil)
	}
	tester.Frame()
	if !tester.HasText("Showing the first 8.0 MB") {
		t.Fatalf("truncation pill missing: %q", tester.Texts())
	}
}

// TestPreviewShowsBinaryEmptyAndCJKFilesHonestly: binary has no code view,
// an empty file is an empty view, and CJK prose past the classification
// probe stays text (the cut-rune regression).
func TestPreviewShowsBinaryEmptyAndCJKFilesHonestly(t *testing.T) {
	s, tester, root := filesOpsTestShell(t)
	st := &s.rightPanel

	bin := writePreviewFile(t, root, "blob.dat", append([]byte("head"), make([]byte, 64)...))
	s.openFilePreview(bin)
	if st.nativeText == nil || !st.nativeText.IsBinary || st.nativeCode != nil {
		t.Fatalf("binary = %+v code=%v", st.nativeText, st.nativeCode != nil)
	}
	tester.Frame()
	if !tester.HasText("Binary file") {
		t.Fatalf("binary state missing: %q", tester.Texts())
	}

	empty := writePreviewFile(t, root, "empty.txt", nil)
	s.openFilePreview(empty)
	if st.nativeCode == nil || st.nativeCode.Buffer.Len() != 0 || st.nativeText.IsBinary {
		t.Fatalf("empty file must be an empty code view: code=%v", st.nativeCode != nil)
	}

	cjk := writePreviewFile(t, root, "notes.txt", []byte(strings.Repeat("中", 4000)))
	s.openFilePreview(cjk)
	if st.nativeText == nil || st.nativeText.IsBinary || st.nativeCode == nil || st.nativeCode.Buffer.Len() != 12000 {
		t.Fatalf("CJK text must stay text: %+v", st.nativeText)
	}
}

// TestPreviewMarkdownRendersInThePageUnlessSourceOrOversized: Markdown goes
// to the page by default, the sticky Source choice and the 1 MiB cap send
// it to the native view, and the bar's toggle round-trips.
func TestPreviewMarkdownRendersInThePageUnlessSourceOrOversized(t *testing.T) {
	s, tester, root := filesOpsTestShell(t)
	small := writePreviewFile(t, root, "small.md", []byte("# hello\n"))
	big := writePreviewFile(t, root, "big.md", bytes.Repeat([]byte("# x\n"), 1<<19)) // 2 MiB
	st := &s.rightPanel

	s.openFilePreview(small)
	if !s.webPreviewOwns(small) || st.nativePreviewPath != "" || st.nativeCode != nil {
		t.Fatalf("small markdown must belong to the page: native=%q", st.nativePreviewPath)
	}
	tester.Frame()
	if !tester.HasText("Rendered") || !tester.HasText("Source") {
		t.Fatalf("markdown must offer Rendered/Source: %q", tester.Texts())
	}

	s.setMarkdownSource(true)
	if s.webPreviewOwns(small) || st.nativePreviewPath != small || st.nativeCode == nil || !st.nativeCode.SoftWrap {
		t.Fatalf("Source must show the wrapped native view: native=%q code=%v", st.nativePreviewPath, st.nativeCode != nil)
	}
	if st.nativeCode.Buffer.String() != "# hello\n" {
		t.Fatalf("source text = %q", st.nativeCode.Buffer.String())
	}
	s.setMarkdownSource(false)
	if !s.webPreviewOwns(small) || st.nativeCode != nil {
		t.Fatal("Rendered must hand the file back to the page and free the editor")
	}

	s.openFilePreview(big)
	if st.nativePreviewPath != big || st.nativeCode == nil {
		t.Fatalf("oversized markdown must show natively: native=%q", st.nativePreviewPath)
	}
	tester.Frame()
	if tester.HasText("Rendered") {
		t.Fatal("an oversized file cannot render, so it must not offer Rendered")
	}
}

// TestPreviewSourceChoiceDrivesTheBarToggle clicks the real toggle.
func TestPreviewSourceChoiceDrivesTheBarToggle(t *testing.T) {
	s, tester, root := filesOpsTestShell(t)
	small := writePreviewFile(t, root, "small.md", []byte("# hello\n"))
	s.openFilePreview(small)
	tester.Frame()
	tester.Click("Source")
	tester.Frame()
	if !s.rightPanel.markdownSource || s.rightPanel.nativePreviewPath != small {
		t.Fatalf("clicking Source: markdownSource=%v native=%q", s.rightPanel.markdownSource, s.rightPanel.nativePreviewPath)
	}
	tester.Click("Rendered")
	tester.Frame()
	if s.rightPanel.markdownSource || !s.webPreviewOwns(small) {
		t.Fatalf("clicking Rendered: markdownSource=%v web=%v", s.rightPanel.markdownSource, s.webPreviewOwns(small))
	}
}

// TestPreviewImagesFollowThePageAndTheRichPreviewPreference: web images
// belong to the page; with the preference off decodable ones show as native
// bitmaps and the formats only the page decodes say so; oversized ones
// keep the truthful states.
func TestPreviewImagesFollowThePageAndTheRichPreviewPreference(t *testing.T) {
	s, _, root := filesOpsTestShell(t)
	st := &s.rightPanel
	dot := writePreviewFile(t, root, "dot.png", tinyPNG(t))
	icon := writePreviewFile(t, root, "icon.ico", []byte("not decoded natively"))
	bigAVIF := writePreviewFile(t, root, "big.avif", make([]byte, filesview.ImagePreviewCap+1))
	bigSVG := writePreviewFile(t, root, "big.svg", make([]byte, filesview.ImagePreviewCap+1))

	s.openFilePreview(dot)
	if !s.webPreviewOwns(dot) {
		t.Fatal("a web image belongs to the page")
	}

	s.settings.General.WebViewFilesPreview = false
	s.replanFilesPreview()
	if s.webPreviewOwns(dot) || st.nativePreviewPath != dot || st.nativeImageFailed != "" {
		t.Fatalf("preference off: a decodable image shows natively: native=%q failed=%q", st.nativePreviewPath, st.nativeImageFailed)
	}
	s.openFilePreview(icon)
	if !strings.Contains(st.nativeImageFailed, "Rich file preview") {
		t.Fatalf("an ICO with the page off must say how to enable it: %q", st.nativeImageFailed)
	}

	s.settings.General.WebViewFilesPreview = true
	s.openFilePreview(bigAVIF)
	if !strings.Contains(st.nativeImageFailed, "too large") {
		t.Fatalf("an oversized AVIF must say it is too large: %q", st.nativeImageFailed)
	}
	s.openFilePreview(bigSVG)
	if st.nativePreviewPath != bigSVG || st.nativeText == nil {
		t.Fatalf("an oversized SVG reads as XML source: native=%q", st.nativePreviewPath)
	}

	s.openFilePreview(dot)
	if !s.webPreviewOwns(dot) || st.nativeImageFailed != "" || st.nativeText != nil {
		t.Fatal("back on a web image the native tier must release the center")
	}
}

// TestPreviewRenameReplansForTheNewExtension: a rename can change the
// flavor, so the preview follows the new name through the plan.
func TestPreviewRenameReplansForTheNewExtension(t *testing.T) {
	s, _, root := filesOpsTestShell(t)
	old := writePreviewFile(t, root, "notes.txt", []byte("# hi\n"))
	s.openFilePreview(old)
	if s.rightPanel.nativeCode == nil || s.webPreviewOwns(old) {
		t.Fatal("a .txt file is native text")
	}
	renamed := filepath.Join(root, "notes.md")
	if err := os.Rename(old, renamed); err != nil {
		t.Fatal(err)
	}
	s.migrateFilesSelection(old, renamed)
	if s.rightPanel.selectedFilePath != renamed || !s.webPreviewOwns(renamed) || s.rightPanel.nativeCode != nil {
		t.Fatalf("renamed to .md: selected=%q web=%v", s.rightPanel.selectedFilePath, s.webPreviewOwns(renamed))
	}
}

// TestFilesBridgeOnlyServesFilesThePageOwns: the page hears and reads only
// what it renders — never native-owned text, never a stale selection.
func TestFilesBridgeOnlyServesFilesThePageOwns(t *testing.T) {
	s, _, root := filesOpsTestShell(t)
	goFile := writePreviewFile(t, root, "main.go", []byte("package main\n"))
	md := writePreviewFile(t, root, "doc.md", []byte("# doc\n"))
	bridge := NewFilesPreviewBridge(s)
	ctx := context.Background()

	s.openFilePreview(md)
	if target, _ := bridge.Current(ctx); target.Path != md || target.Name != "doc.md" {
		t.Fatalf("page-owned markdown target = %+v", target)
	}
	content, err := bridge.Open(ctx, md)
	if err != nil || content.Flavor != "markdown" || content.Content != "# doc\n" {
		t.Fatalf("markdown open = %+v, %v", content, err)
	}

	s.openFilePreview(goFile)
	if target, _ := bridge.Current(ctx); target.Path != "" {
		t.Fatalf("a native-owned file must clear the page target: %+v", target)
	}
	if _, err := bridge.Open(ctx, goFile); err == nil {
		t.Fatal("text must never cross the bridge")
	}
	if _, err := bridge.Open(ctx, md); err == nil {
		t.Fatal("a stale selection must be refused")
	}
}

// TestCodeEditorThemeFollowsTheReviewPalette: Files and the Git review color
// a file the same way, because the editor's tokens come from gd_palette.
func TestCodeEditorThemeFollowsTheReviewPalette(t *testing.T) {
	for _, dark := range []bool{false, true} {
		theme := ui.LightTheme()
		if dark {
			theme = ui.DarkTheme()
		}
		th := codeEditorTheme(theme)
		p := gdPaletteFor(theme)
		for kind, class := range map[editor.TokenKind]codehl.Class{
			editor.TokenKeyword: codehl.Keyword, editor.TokenString: codehl.String,
			editor.TokenComment: codehl.Comment, editor.TokenType: codehl.Type,
			editor.TokenNumber: codehl.Number, editor.TokenOperator: codehl.Plain,
		} {
			if got := th.Tokens[kind].Color; got != p.syntax[class] {
				t.Errorf("dark=%v kind %v = %v, want the review's %v", dark, kind, got, p.syntax[class])
			}
		}
		if th.Text != p.code || th.LineNumber != p.lineNumber {
			t.Errorf("dark=%v code/line-number colors diverge from the review", dark)
		}
		if th.Selection != editor.DefaultTheme(theme).Selection {
			t.Errorf("dark=%v structural colors must stay the framework's", dark)
		}
	}
}

// TestReviewAndFilesPaintACharacterTheSameColor: the review's lines and the
// Files code view run one tokenizer through one palette, so a character has
// one color on both surfaces — the promise of codehl.KindClasses, checked
// character by character instead of table against table.
func TestReviewAndFilesPaintACharacterTheSameColor(t *testing.T) {
	lines := []string{
		"package main",
		`import "fmt"`,
		"/* a block",
		"still comment */ var n int = 42",
		`func main() { fmt.Println("x\n", n) } // done`,
	}
	for _, dark := range []bool{false, true} {
		theme := ui.LightTheme()
		if dark {
			theme = ui.DarkTheme()
		}
		files, review := codeEditorTheme(theme), gdPaletteFor(theme)
		segs := codehl.Lines("main.go", lines)
		var (
			state  uint64
			tokens []editor.Token
		)
		for row, line := range lines {
			tokens, state = editor.LanguageFor("main.go").Tokenizer.Tokenize(line, state, tokens[:0])
			want := make([]ui.Color, len(line))
			for i := range want {
				want[i] = files.Text
			}
			start := 0
			for _, tok := range tokens {
				for i := start; i < min(tok.End, len(line)); i++ {
					want[i] = files.Tokens[tok.Kind].Color
				}
				start = max(start, tok.End)
			}
			var got []ui.Color
			for _, span := range gdCodeSpans(line, segs[row], nil, review) {
				for range len(span.Text) {
					got = append(got, span.Color)
				}
			}
			if len(got) != len(want) {
				t.Fatalf("dark=%v line %d: review painted %d characters, Files %d", dark, row, len(got), len(want))
			}
			for i := range want {
				if got[i] != want[i] {
					t.Errorf("dark=%v line %d col %d (%q of %q): review %v, Files %v", dark, row, i, line[i], line, got[i], want[i])
					break
				}
			}
		}
	}
}
