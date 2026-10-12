package nativeui

/**
 * [INPUT]: 官方 MyGo headless Tester、Settings / Sessions 页面与 Local Changes 提交表单夹具
 * [OUTPUT]: 验证内容列在宽窗口里居中且封顶（Settings 760、Sessions 1040、提交表单 760），两侧留白对称
 * [POS]: 宽屏版式回归；列宽常量只在 ds_components.go 的 contentColumn 一处生效
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

// renderedRect renders shell at width×900 and returns where the element
// showing text sits.
func renderedRect(t *testing.T, shell *Shell, width int, text string) ui.Rect {
	t.Helper()
	tester := ui.NewTester(shell.View, width, 900)
	tester.Frame()
	tester.Frame()
	rect, ok := tester.Find(text)
	if !ok {
		t.Fatalf("%q not found at width %d; texts=%q", text, width, tester.Texts())
	}
	return rect
}

// assertCentered checks that growing the window by delta moves a centered
// column's content by exactly delta/2 — the margins stay symmetric.
func assertCentered(t *testing.T, what string, narrow, wide, delta float32) {
	t.Helper()
	if moved := wide - narrow; moved < delta/2-1 || moved > delta/2+1 {
		t.Fatalf("%s moved %.1fpt when the window grew %.0fpt; want %.0f (a centered, capped column)", what, moved, delta, delta/2)
	}
}

func TestSettingsContentIsACenteredColumn(t *testing.T) {
	for _, route := range []string{"/settings/general", "/settings/git", "/settings/diagnostics"} {
		shell := NewShell()
		shell.loading = false
		shell.router.Replace(route)
		anchor := map[string]string{"/settings/general": "Appearance", "/settings/git": "Commit message rules", "/settings/diagnostics": "About"}[route]
		narrow := renderedRect(t, shell, 1200, anchor)
		wide := renderedRect(t, shell, 1600, anchor)
		assertCentered(t, route+" content", narrow.X, wide.X, 400)
	}
}

func TestSessionsListIsACenteredColumn(t *testing.T) {
	shell := newMultiAgentHistoryShell(t)
	shell.router.Replace(routeHistory)
	const row = "Tighten sidebar spacing"
	// Both widths leave the route area wider than the 1040 cap.
	narrow := renderedRect(t, shell, 1500, row)
	wide := renderedRect(t, shell, 1900, row)
	assertCentered(t, "Sessions rows", narrow.X, wide.X, 400)
}

func TestQuickCommitFormStopsAtTheReadableWidth(t *testing.T) {
	shell := gitSurfaceTestShell(t, t.TempDir())
	shell.showSurface(WorkspaceSurfaceDiff)
	subject := renderedRect(t, shell, 1900, "Commit subject draft")
	if subject.W > 761 {
		t.Fatalf("commit subject field is %.0fpt wide in a 1900pt window; want it capped at 760", subject.W)
	}
	narrow := renderedRect(t, shell, 1500, "Commit subject draft")
	wide := renderedRect(t, shell, 1900, "Commit subject draft")
	assertCentered(t, "commit form", narrow.X, wide.X, 400)
}
