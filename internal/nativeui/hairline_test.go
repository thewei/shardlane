package nativeui

import (
	"go/ast"
	"go/parser"
	"go/token"
	"path/filepath"
	"strings"
	"testing"
)

/**
 * [INPUT]: 依赖 nativeui 包内非测试源码的 AST（.Background(...) / .BorderColor(...) 的实参）
 * [OUTPUT]: 对外提供守卫：分隔线与面板边缘只用 BorderSubtle 发丝线 token；控件描边色 theme.Border 只能出现在 .Border(w, c) 里，不能当分隔线的底色/边色
 * [POS]: 2026-10-14 第二轮审计：Git 工作台四处窗格分隔线曾误用比发丝线重得多的控件描边色，与侧栏边线并排时一粗一细
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// TestSeparatorsUseTheHairlineToken pins one line weight for every separator
// and pane edge: the BorderSubtle hairline. The theme's Border is the outline
// of a control (an input, a segmented track) — opaque and several times
// stronger — so painting a separator or an edge with it (.Background(t.Border),
// .BorderColor(t.Border)) puts a heavy line next to the sidebar's light one.
func TestSeparatorsUseTheHairlineToken(t *testing.T) {
	files, err := filepath.Glob("*.go")
	if err != nil {
		t.Fatal(err)
	}
	fset := token.NewFileSet()
	painted := 0
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
			if !ok || (sel.Sel.Name != "Background" && sel.Sel.Name != "BorderColor") || len(call.Args) != 1 {
				return true
			}
			painted++
			if arg, ok := call.Args[0].(*ast.SelectorExpr); ok && arg.Sel.Name == "Border" {
				t.Errorf("%s: .%s(%s.Border) paints with the control outline color; separators and edges use BorderSubtle",
					fset.Position(call.Pos()), sel.Sel.Name, receiverName(arg.X))
			}
			return true
		})
	}
	if painted < 100 {
		t.Fatalf("saw only %d Background/BorderColor calls; the scan no longer finds the call sites", painted)
	}
}

// receiverName names a selector's receiver for the failure message.
func receiverName(e ast.Expr) string {
	if id, ok := e.(*ast.Ident); ok {
		return id.Name
	}
	return "…"
}
