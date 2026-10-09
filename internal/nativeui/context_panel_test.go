package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/history"
)

/**
 * [INPUT]: 依赖 context_panel 与 MyGo Tester 的无动画渲染
 * [OUTPUT]: 验证 History/Workspace/Settings 的右侧栏归属、详情身份栅栏和工具状态保留
 * [POS]: context_panel 的行为回归门禁，防止未来将 Workspace 工具泄漏到内页
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

func TestContextPanelRoutePolicy(t *testing.T) {
	cases := []struct {
		path string
		want contextPanelKind
	}{
		{routeWorkspace, contextPanelWorkspace},
		{routeHistory, contextPanelHistory},
		{"/history-projects", contextPanelHistory},
		{"/history/session-1", contextPanelHistory},
		{"/settings/general", contextPanelNone},
		{"/inspector/pane-1", contextPanelNone},
		{"/chat", contextPanelNone},
		{"/new-task", contextPanelNone},
	}
	for _, tc := range cases {
		if got := contextPanelForRoute(tc.path); got != tc.want {
			t.Errorf("contextPanelForRoute(%q) = %v, want %v", tc.path, got, tc.want)
		}
	}
}

func TestHistoryContextPanelShowsCurrentConversation(t *testing.T) {
	shell := rightPanelTestShell(t, t.TempDir())
	shell.hist.sessions = []history.SessionSummary{
		{Meta: history.SessionMeta{Key: "a", Title: "First topic", ProjectName: "alpha", FilePath: "/tmp/first.jsonl", MessageCount: 9}},
		{Meta: history.SessionMeta{Key: "b", Title: "Second topic", ProjectName: "beta", FilePath: "/tmp/second.jsonl", MessageCount: 12}},
	}
	shell.rightPanel.open = true
	shell.router.Push("/history/b")
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.SetPreferences(ui.Preferences{ReduceMotion: true})
	tester.Frame()

	for _, want := range []string{"History details", "Second topic", "beta", "second.jsonl", "Copy source path"} {
		if !tester.HasText(want) {
			t.Errorf("missing %q from current History inspector; got %q", want, tester.Texts())
		}
	}
	if tester.HasText("first.jsonl") || tester.HasText("Services") {
		t.Fatalf("stale or Workspace tool content leaked into History: %q", tester.Texts())
	}
}

func TestContextPanelHidesUnsupportedRoutesAndPreservesWorkspaceTool(t *testing.T) {
	shell := rightPanelTestShell(t, t.TempDir())
	shell.openRightPanelSurface(SurfaceServices)
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.SetPreferences(ui.Preferences{ReduceMotion: true})
	tester.Frame()

	shell.router.Push("/settings/general")
	tester.Frame()
	if tester.HasText("Listening Ports & Local Preview") || tester.HasText("History details") {
		t.Fatalf("context panel leaked into Settings: %q", tester.Texts())
	}
	shell.toggleRightPanel() // unavailable context: no mutation
	if !shell.rightPanel.open || shell.rightPanel.surface != SurfaceServices {
		t.Fatal("unsupported route must preserve the Workspace panel's prior state")
	}

	shell.router.Push(routeWorkspace)
	tester.Frame()
	if !tester.HasText("Services") || shell.rightPanel.surface != SurfaceServices {
		t.Fatalf("Workspace context must restore the prior tool: %q", tester.Texts())
	}
}

func TestHistoryContextPanelEmptyList(t *testing.T) {
	shell := rightPanelTestShell(t, t.TempDir())
	shell.rightPanel.open = true
	shell.router.Push(routeHistory)
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.SetPreferences(ui.Preferences{ReduceMotion: true})
	tester.Frame()
	if !tester.HasText("0 conversations in view") || !tester.HasText("History details") {
		t.Fatalf("History overview panel missing: %q", tester.Texts())
	}
	if tester.HasText("Listening Ports & Local Preview") {
		t.Fatal("Services should not render on History")
	}
}
