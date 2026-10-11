package nativeui

import (
	"testing"

	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/conversation"
	"github.com/wh-studio/herdr-client/internal/herdr"
)

// TestRebindWorkspaceChatSourcelessClearsStaleTranscript pins the bug where
// toggling to Chat on a session-less Agent kept the previous pane's
// conversation on screen: the fallthrough branch must drop to the empty
// state, latch the pane so the render-path rebind exits early, and never
// keep a foreign conversation id.
func TestRebindWorkspaceChatSourcelessClearsStaleTranscript(t *testing.T) {
	shell := NewShell()
	shell.loading = false
	shell.selectedPaneID = "p-new"
	shell.chatPaneID = "p-old"
	shell.chatConversationID = "live:claude:term-old"
	shell.chatTurns = []conversation.TimelineTurn{{UserRow: true, Text: "stale"}}
	shell.projection = herdr.Projection{
		Agents: []herdr.Agent{
			{TerminalID: "term-new", PaneID: "p-new", Status: "idle"},
		},
	}

	shell.rebindWorkspaceChat()

	if shell.chatPaneID != "p-new" {
		t.Fatalf("chatPaneID = %q, want p-new", shell.chatPaneID)
	}
	if shell.chatConversationID != "" {
		t.Fatalf("stale conversation survived: %q", shell.chatConversationID)
	}
	if len(shell.chatTurns) != 0 {
		t.Fatalf("stale turns survived: %d", len(shell.chatTurns))
	}
	if shell.chatSourcelessPaneID != "p-new" {
		t.Fatalf("sourceless latch = %q, want p-new", shell.chatSourcelessPaneID)
	}
	if shell.pendingToast == "" {
		t.Fatal("the sourceless notice must be shown once")
	}

	// The render path re-enters for the same pane: the latch makes it a
	// cheap exit with no second toast churn.
	shell.pendingToast = ""
	shell.rebindWorkspaceChat()
	if shell.pendingToast != "" {
		t.Fatalf("latched rebind re-notified: %q", shell.pendingToast)
	}
	if shell.chatSourcelessPaneID != "p-new" {
		t.Fatalf("latch lost across rebind: %q", shell.chatSourcelessPaneID)
	}
}

// TestRebindWorkspaceChatUnlatchesWhenSourceAppears pins the latch
// semantics: while the pane has no chat source the latch exits early, but
// once the card carries a conversation identity (agent established a
// session), the rebind must retry — headless the binds fail and the
// cleanup re-runs, which is the observable proof the latch was left.
func TestRebindWorkspaceChatUnlatchesWhenSourceAppears(t *testing.T) {
	shell := NewShell()
	shell.loading = false
	shell.selectedPaneID = "p-new"
	shell.chatSourcelessPaneID = "p-new"
	shell.chatPaneID = "p-new"
	shell.projection = herdr.Projection{
		Agents: []herdr.Agent{
			{TerminalID: "term-new", PaneID: "p-new", Status: "idle"},
		},
	}

	// Still sourceless: the latch exits early, no notice churn.
	shell.pendingToast = ""
	shell.rebindWorkspaceChat()
	if shell.pendingToast != "" {
		t.Fatalf("latched rebind re-notified while sourceless: %q", shell.pendingToast)
	}

	// The agent establishes a session: the card gains a conversation id.
	shell.workbench.directory = agent.NewAgentDirectory()
	shell.workbench.directory.Replace([]agent.AgentCardModel{{
		Key:           agent.AgentKey{InstanceID: "inst", TerminalID: "term-new"},
		PaneID:        "p-new",
		ConversationID: "live:claude:term-new",
	}})
	shell.pendingToast = ""
	shell.rebindWorkspaceChat()
	if shell.chatSourcelessPaneID == "p-new" && shell.pendingToast == "" {
		t.Fatal("the rebind must retry once the pane has a chat source")
	}
}
