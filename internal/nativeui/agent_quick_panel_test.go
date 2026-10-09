package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
)

/**
 * [INPUT]: 依赖 quickPanelShell 的真实卡片投影和 MyGo headless Tester
 * [OUTPUT]: 钉住浮层概览统计、单 Agent 精确身份、返回列表、已消失 Agent 的安全空态
 * [POS]: agent_quick_panel UI 交互的回归测试，不访问用户实例或修改运行时
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

func TestAgentQuickPanelOverviewStats(t *testing.T) {
	shell := quickPanelShell(t)
	tester := ui.NewTester(shell.QuickPanelView, QuickPanelWidth, QuickPanelHeight)
	for _, want := range []string{"Agent Activity", "Attention", "Review", "Working"} {
		if !tester.HasText(want) {
			t.Fatalf("missing overview label %q: %q", want, tester.Texts())
		}
	}
}

func TestAgentQuickPanelFocusedDetailsAndBack(t *testing.T) {
	shell := quickPanelShell(t)
	cards := shell.workbenchCards()
	if len(cards) == 0 {
		t.Fatal("fixture has no agents")
	}
	selected := cards[0].Key
	shell.quickPanelAgent = &selected
	tester := ui.NewTester(shell.QuickPanelView, QuickPanelWidth, QuickPanelHeight)
	for _, want := range []string{"← All Agents", "Open Terminal", "Open Chat", "Inspect Agent", "Context"} {
		if !tester.HasText(want) {
			t.Fatalf("focused agent lacks %q: %q", want, tester.Texts())
		}
	}
	if tester.HasText("All") && tester.HasText("Attention") {
		t.Fatalf("focused agent must not show the overview filters: %q", tester.Texts())
	}
	if err := tester.Click("← All Agents"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if shell.quickPanelAgent != nil || !tester.HasText("Agent Activity") {
		t.Fatalf("Back must show the all-agents overview: %q", tester.Texts())
	}
}

func TestAgentQuickPanelRejectsGoneAgent(t *testing.T) {
	shell := quickPanelShell(t)
	key := agent.AgentKey{InstanceID: "gone-instance", TerminalID: "gone-terminal"}
	shell.quickPanelAgent = &key
	tester := ui.NewTester(shell.QuickPanelView, QuickPanelWidth, QuickPanelHeight)
	if !tester.HasText("Agent unavailable") {
		t.Fatalf("missing gone-agent safe state: %q", tester.Texts())
	}
	if tester.HasText("Open Terminal") || tester.HasText("Open Chat") {
		t.Fatalf("gone Agent cannot expose stale actions: %q", tester.Texts())
	}
}
