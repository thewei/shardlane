package nativeui

import (
	"go/ast"
	"go/parser"
	"go/token"
	"path/filepath"
	"strconv"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
)

/**
 * [INPUT]: 依赖 nativeui 包内非测试源码的 AST（TextInput 链上的 Placeholder、ButtonBase 的 "Clear" 标签、ui.SearchField 调用）与 gitSurfaceTestShell 的 Git 面
 * [OUTPUT]: 对外提供守卫：列表筛选/搜索框只有官方 ui.SearchField 一种脸（放大镜 + 边框 + 清除钮 + Esc 清空），不再手搓；Git 面的筛选框收的是文字，输入时不会同时触发面上的快捷键
 * [POS]: 2026-10-14 第二轮审计：同一角色曾有三种脸（官方 SearchField / 无边框灰底手搓 / 无放大镜的 TextInput），收口为一种
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// callRoot walks a method chain back to the ui.* constructor that started it:
// ui.TextInput(c, &q).Height(28).Placeholder("x") → "TextInput". It returns ""
// for a chain that does not start at a ui.* call.
func callRoot(e ast.Expr) string {
	for {
		call, ok := e.(*ast.CallExpr)
		if !ok {
			return ""
		}
		sel, ok := call.Fun.(*ast.SelectorExpr)
		if !ok {
			return ""
		}
		if pkg, isPkg := sel.X.(*ast.Ident); isPkg {
			if pkg.Name == "ui" {
				return sel.Sel.Name
			}
			return ""
		}
		e = sel.X
	}
}

// TestListFiltersAreTheOfficialSearchField pins one face for every field that
// searches or filters a list: ui.SearchField — magnifier, border, a clear
// button, Escape clears. The app once had three (the official one, a
// borderless grey hand-rolled copy in the Git surface, and a bare TextInput
// with no magnifier); a TextInput dressed with a "Search…"/"Filter…" hint, or a
// hand-drawn "Clear" button, is that drift coming back.
func TestListFiltersAreTheOfficialSearchField(t *testing.T) {
	files, err := filepath.Glob("*.go")
	if err != nil {
		t.Fatal(err)
	}
	fset := token.NewFileSet()
	searchFields := 0
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
			if !ok {
				return true
			}
			if pkg, isPkg := sel.X.(*ast.Ident); isPkg && pkg.Name == "ui" && sel.Sel.Name == "SearchField" {
				searchFields++
				return true
			}
			if len(call.Args) != 1 {
				return true
			}
			lit, ok := call.Args[0].(*ast.BasicLit)
			if !ok || lit.Kind != token.STRING {
				return true
			}
			text, err := strconv.Unquote(lit.Value)
			if err != nil {
				return true
			}
			where := fset.Position(call.Pos())
			switch root := callRoot(sel.X); {
			case sel.Sel.Name == "Placeholder" && (root == "TextInput" || root == "TextInputBase") &&
				(strings.HasPrefix(text, "Search") || strings.HasPrefix(text, "Filter")):
				t.Errorf("%s: a ui.%s hinted %q is a hand-rolled search field; use ui.SearchField", where, root, text)
			case sel.Sel.Name == "Label" && root == "ButtonBase" && text == "Clear":
				t.Errorf("%s: a hand-drawn Clear button; ui.SearchField owns the clear button", where)
			}
			return true
		})
	}
	if searchFields < 6 {
		t.Fatalf("saw only %d ui.SearchField calls; the scan no longer finds the call sites", searchFields)
	}
}

// A filter field of the Git surface takes its keys as text: typing in it never
// also fires the surface's window shortcuts. Today the framework shields them
// (an editor handles letters and, on a Mac, Option+letter — Alt+Z types an
// Omega), so this pins the observable result for whichever layer does it.
func TestChangesFilterTakesItsKeysAsText(t *testing.T) {
	newSurface := func() (*Shell, *ui.Tester) {
		root := t.TempDir()
		shell := gitSurfaceTestShell(t, root)
		shell.git.rootOf[root] = root
		if shell.router.Path() != routeWorkspace {
			shell.router.Push(routeWorkspace)
		}
		shell.showSurface(WorkspaceSurfaceDiff)
		shell.rebuildDiffRows()
		tester := ui.NewTester(shell.View, 1200, 800)
		tester.Frame()
		return shell, tester
	}

	// Control: with nothing focused, Alt+Z toggles the diff's word wrap.
	shell, tester := newSurface()
	was := shell.git.gdWordWrap
	pressKey(tester, ui.Alt, ui.KeyZ)
	if shell.git.gdWordWrap == was {
		t.Fatal("control: Alt+Z with no field focused must toggle word wrap, or this test proves nothing")
	}

	// In the filter the same chord is text and toggles nothing.
	shell, tester = newSurface()
	was = shell.git.gdWordWrap
	if err := tester.Click("Filter files"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	tester.TypeKey(0, ui.KeyJ, "j")
	tester.TypeKey(ui.Alt, ui.KeyZ, "Ω")
	tester.Frame()
	if shell.changes.filter != "jΩ" {
		t.Fatalf("filter text = %q, want what was typed", shell.changes.filter)
	}
	if shell.git.gdWordWrap != was {
		t.Fatal("Alt+Z typed into the filter must not also toggle word wrap")
	}
}
