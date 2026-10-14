package nativeui

import (
	"go/ast"
	"go/parser"
	"go/token"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"testing"
)

/**
 * [INPUT]: 依赖 nativeui 包内非测试源码里 emptyState / workspaceEmptyBanner 的字面标题、菜单项（.Item("…")）的字面标签与 openConfirm 的标题实参、全部生产字符串字面量
 * [OUTPUT]: 对外提供文案回归：空态标题句首大写（专有名词除外），菜单项 Title Case（短介词/冠词小写），确认框标题一律是问句，文案里没有 Please/successfully/Oops 这类填充词——同一类组件只有一种写法
 * [POS]: 2026-10-14 文案统一的守卫测试；新增空态/菜单项时由它拦下大小写漂移
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// emptyTitle matches the literal title a page hands to a shared empty state.
var emptyTitle = regexp.MustCompile(`(?:emptyState\(c|workspaceEmptyBanner\(c, [^,]+), "([^"]+)"`)

// TestEmptyStateTitlesAreSentenceCase pins one voice for the shared empty
// states: "Nothing to commit", never "Nothing to Commit". Dialog and menu
// titles keep Title Case; an empty state reads as a sentence.
func TestEmptyStateTitlesAreSentenceCase(t *testing.T) {
	proper := map[string]bool{"Git": true, "SSH": true}
	files, err := filepath.Glob("*.go")
	if err != nil {
		t.Fatal(err)
	}
	seen := 0
	for _, name := range files {
		if strings.HasSuffix(name, "_test.go") {
			continue
		}
		src, err := os.ReadFile(name)
		if err != nil {
			t.Fatal(err)
		}
		for _, match := range emptyTitle.FindAllStringSubmatch(string(src), -1) {
			seen++
			words := strings.Fields(match[1])
			for _, word := range words[1:] {
				if first := word[0]; first >= 'A' && first <= 'Z' && !proper[word] {
					t.Errorf("%s: empty-state title %q capitalizes %q; titles are sentence case", name, match[1], word)
				}
			}
		}
	}
	if seen < 10 {
		t.Fatalf("matched only %d empty-state titles; the pattern no longer finds the call sites", seen)
	}
}

// menuItemLabel matches the literal label of a context-menu or pull-down item.
var menuItemLabel = regexp.MustCompile(`\.Item\("([^"]+)"`)

// TestMenuItemsAreTitleCase pins one voice for menu items: "Copy Branch Name",
// never "Copy branch name". Articles and short prepositions stay lowercase
// (HIG title style); tooltips and empty states are the sentence-case voices.
func TestMenuItemsAreTitleCase(t *testing.T) {
	small := map[string]bool{
		"a": true, "an": true, "the": true, "and": true, "or": true, "in": true, "into": true,
		"to": true, "of": true, "on": true, "as": true, "from": true, "with": true, "for": true, "at": true, "by": true,
	}
	files, err := filepath.Glob("*.go")
	if err != nil {
		t.Fatal(err)
	}
	seen := 0
	for _, name := range files {
		if strings.HasSuffix(name, "_test.go") {
			continue
		}
		src, err := os.ReadFile(name)
		if err != nil {
			t.Fatal(err)
		}
		for _, match := range menuItemLabel.FindAllStringSubmatch(string(src), -1) {
			seen++
			for _, word := range strings.Fields(match[1]) {
				if first := word[0]; first >= 'a' && first <= 'z' && !small[word] {
					t.Errorf("%s: menu item %q starts %q with a lowercase letter; menu items are Title Case", name, match[1], word)
				}
			}
		}
	}
	if seen < 40 {
		t.Fatalf("matched only %d menu items; the pattern no longer finds the call sites", seen)
	}
}

// confirmTitleTail returns the literal tail of an openConfirm title argument:
// a string literal, the last operand of a "+" chain (which must be a literal),
// or a Sprintf format. A title that is a plain variable or call result is not
// decidable here.
func confirmTitleTail(e ast.Expr) (string, bool) {
	switch v := e.(type) {
	case *ast.BasicLit:
		if v.Kind != token.STRING {
			return "", false
		}
		text, err := strconv.Unquote(v.Value)
		return text, err == nil
	case *ast.BinaryExpr:
		if lit, ok := v.Y.(*ast.BasicLit); ok {
			return confirmTitleTail(lit)
		}
		// A "+" chain must end in its own literal "?", not in a computed value.
		return "(a computed value)", true
	case *ast.CallExpr:
		if sel, ok := v.Fun.(*ast.SelectorExpr); ok && sel.Sel.Name == "Sprintf" && len(v.Args) > 0 {
			return confirmTitleTail(v.Args[0])
		}
	}
	return "", false
}

// TestConfirmTitlesAreQuestions pins one voice for confirm dialogs: every
// openConfirm title asks — "Close Tab?", "Delete Forever?" — so the dialog
// title never reads like the toolbar button that opened it ("Empty Trash").
func TestConfirmTitlesAreQuestions(t *testing.T) {
	files, err := filepath.Glob("*.go")
	if err != nil {
		t.Fatal(err)
	}
	fset := token.NewFileSet()
	seen := 0
	for _, name := range files {
		if strings.HasSuffix(name, "_test.go") {
			continue
		}
		file, err := parser.ParseFile(fset, name, nil, 0)
		if err != nil {
			t.Fatal(err)
		}
		ast.Inspect(file, func(n ast.Node) bool {
			call, ok := n.(*ast.CallExpr)
			if !ok {
				return true
			}
			sel, ok := call.Fun.(*ast.SelectorExpr)
			if !ok || sel.Sel.Name != "openConfirm" || len(call.Args) < 4 {
				return true
			}
			tail, decidable := confirmTitleTail(call.Args[2])
			if !decidable {
				return true
			}
			seen++
			if !strings.HasSuffix(tail, "?") {
				t.Errorf("%s: confirm title ending %q is not a question; confirms read “Close Tab?”", fset.Position(call.Pos()), tail)
			}
			return true
		})
	}
	if seen < 18 {
		t.Fatalf("matched only %d confirm titles; the scan no longer finds the call sites", seen)
	}
}

// fillerWord matches words that pad a sentence without telling the reader
// anything: "Please reopen…", "Saved successfully", "Oops".
var fillerWord = regexp.MustCompile(`(?i)\b(please|successfully|oops)\b`)

// TestCopyHasNoFillerWords keeps the product voice plain: say what happened
// and what to do next. It scans every string literal of production code, so
// toasts, tooltips, dialogs and empty states are all covered.
func TestCopyHasNoFillerWords(t *testing.T) {
	files, err := filepath.Glob("*.go")
	if err != nil {
		t.Fatal(err)
	}
	fset := token.NewFileSet()
	seen := 0
	for _, name := range files {
		if strings.HasSuffix(name, "_test.go") {
			continue
		}
		file, err := parser.ParseFile(fset, name, nil, 0)
		if err != nil {
			t.Fatal(err)
		}
		ast.Inspect(file, func(n ast.Node) bool {
			lit, ok := n.(*ast.BasicLit)
			if !ok || lit.Kind != token.STRING {
				return true
			}
			text, err := strconv.Unquote(lit.Value)
			if err != nil {
				return true
			}
			seen++
			if word := fillerWord.FindString(text); word != "" {
				t.Errorf("%s: %q uses %q; say what happened and what to do next", fset.Position(lit.Pos()), text, word)
			}
			return true
		})
	}
	if seen < 1000 {
		t.Fatalf("scanned only %d string literals; the scan no longer finds the source", seen)
	}
}
