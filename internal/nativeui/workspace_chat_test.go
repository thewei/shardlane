package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/conversation"
	"github.com/wh-studio/herdr-client/internal/herdr"
	"github.com/wh-studio/herdr-client/internal/history"
)

/**
 * [INPUT]: Depends on nativeui Shell, workspaceSurfaceState, and agent/conversation models
 * [OUTPUT]: Tests for WorkspaceSurfaceChat as a first-class WorkspacePrimarySurface
 * [POS]: Verifies in-workspace Chat surface switching, toggle shortcuts, and agent binding
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

func TestWorkspaceChatSurfaceSwitchAndToggle(t *testing.T) {
	shell := NewShell()
	shell.loading = false
	shell.activeInstance = "default"
	shell.projection = herdr.Projection{
		Projects: []herdr.Project{{ID: "w1", Label: "demo", CWD: "/tmp"}},
		Tabs:     []herdr.Tab{{ID: "t1", ProjectID: "w1", Label: "main"}},
		Panes:    []herdr.Pane{{ID: "p1", ProjectID: "w1", TabID: "t1", TerminalID: "term-1"}},
	}
	shell.selectedProjectID = "w1"
	shell.selectedTabID = "t1"
	shell.selectedPaneID = "p1"
	shell.surface.resetToTerminal(shell.workspaceContext())

	// Initial surface is Terminal.
	if shell.surface.current() != WorkspaceSurfaceTerminal {
		t.Fatalf("initial surface = %v, want WorkspaceSurfaceTerminal", shell.surface.current())
	}

	// Switch to Chat surface.
	shell.showSurface(WorkspaceSurfaceChat)
	if shell.surface.current() != WorkspaceSurfaceChat {
		t.Fatalf("surface after showSurface = %v, want WorkspaceSurfaceChat", shell.surface.current())
	}

	// Toggle back to Terminal.
	shell.toggleTerminalChat()
	if shell.surface.current() != WorkspaceSurfaceTerminal {
		t.Fatalf("surface after toggle = %v, want WorkspaceSurfaceTerminal", shell.surface.current())
	}

	// Toggle back to Chat.
	shell.toggleTerminalChat()
	if shell.surface.current() != WorkspaceSurfaceChat {
		t.Fatalf("surface after second toggle = %v, want WorkspaceSurfaceChat", shell.surface.current())
	}
}

func TestWorkspaceChatSurfaceRendersInWorkspace(t *testing.T) {
	shell := NewShell()
	shell.loading = false
	shell.activeInstance = "default"
	shell.projection = herdr.Projection{
		Projects: []herdr.Project{{ID: "w1", Label: "demo", CWD: "/tmp"}},
		Tabs:     []herdr.Tab{{ID: "t1", ProjectID: "w1", Label: "main"}},
		Panes:    []herdr.Pane{{ID: "p1", ProjectID: "w1", TabID: "t1", TerminalID: "term-1"}},
	}
	shell.selectedProjectID = "w1"
	shell.selectedTabID = "t1"
	shell.selectedPaneID = "p1"
	shell.router.Replace(routeWorkspace)
	shell.showSurface(WorkspaceSurfaceChat)

	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()

	// When no agent is running in the pane, shows the in-workspace empty state
	if !tester.HasText("No agent in this pane") {
		t.Fatalf("missing in-workspace chat empty state; texts=%q", tester.Texts())
	}

	// Now simulate an agent in the pane with turns
	shell.chatConversationID = "conv-1"
	shell.chatTurns = []conversation.TimelineTurn{
		{Text: "Fix the bug", UserRow: true},
		{Narration: "Investigating the issue…", UserRow: false},
	}
	tester.Frame()

	for _, want := range []string{"USER", "Fix the bug", "ASSISTANT", "Investigating the issue…"} {
		if !tester.HasText(want) {
			t.Fatalf("workspace chat missing %q; texts=%q", want, tester.Texts())
		}
	}
}

func TestWorkspaceChatBindsSelectedPaneAgent(t *testing.T) {
	shell, _, _ := newHistoryTestShell(t)
	shell.loading = false
	shell.activeInstance = "default"
	shell.projection = herdr.Projection{
		Projects: []herdr.Project{{ID: "w1", Label: "demo", CWD: "/tmp"}},
		Tabs:     []herdr.Tab{{ID: "t1", ProjectID: "w1", Label: "main"}},
		Panes:    []herdr.Pane{{ID: "p1", ProjectID: "w1", TabID: "t1", TerminalID: "term-1"}},
	}
	shell.selectedProjectID = "w1"
	shell.selectedTabID = "t1"
	shell.selectedPaneID = "p1"

	// Register an agent card for p1
	shell.workbench.directory = agent.NewAgentDirectory()
	shell.workbench.directory.Replace([]agent.AgentCardModel{
		{
			Key:            agent.AgentKey{InstanceID: "default", TerminalID: "term-1"},
			PaneID:         "p1",
			Title:          "Codex agent",
			Provider:       history.AgentCodex,
			ConversationID: "codex:conv-xyz",
		},
	})

	shell.showSurface(WorkspaceSurfaceChat)
	if shell.chatPaneID != "p1" {
		t.Fatalf("chatPaneID = %q, want p1", shell.chatPaneID)
	}
	if shell.chatConversationID != "codex:conv-xyz" {
		t.Fatalf("chatConversationID = %q, want codex:conv-xyz", shell.chatConversationID)
	}
}
