package nativeui

import (
	"strings"
	"testing"

	"github.com/wh-studio/herdr-client/internal/conversation"
	"github.com/wh-studio/herdr-client/internal/herdr"
)

// The fixture is the real visible screen of a Claude Code AskUserQuestion
// prompt (captured via pane.read, 2026-10-11).
const askUserQuestionScreen = `❯ hi

  Thought for 14s, ran 1 shell command

⏺ Hi! I'm ready to help with Shardlane (herdr-client). What would you like to work on?

✻ Baked for 17s · done 1:43 PM

❯ 向我展示一下用户问答，也就是 Ask user question，给个例子

  Thought for 15s, ran 1 shell command

⏺ 好的，下面就是 AskUserQuestion 的实际效果：
──────────────────────────────────────────────────────────────────────────────────────────────────────────────
←  ☐ 演示问题  ☐ 主题演示  ✔ Submit  →

这是一个演示：你想让我接下来关注 Shardlane 的哪个部分？

❯ 1. nativeui 界面层
     MyGo Native UI 展示层：面板、标签页、交互状态等
  2. herdr adapter
     herdr 适配层：与 Herdr 运行时的 API 对接
  3. 随便看看
     只是看个效果，不需要真的回答
  4. Type something.
──────────────────────────────────────────────────────────────────────────────────────────────────────────────
  5. Chat about this

Enter to select · Tab/Arrow keys to navigate · Esc to cancel`

func TestDetectClaudeInteractionAskUserQuestion(t *testing.T) {
	kind, prompt, options, ok := detectClaudeInteraction(askUserQuestionScreen)
	if !ok {
		t.Fatal("the AskUserQuestion screen must detect an interaction")
	}
	if kind != conversation.InteractionQuestion {
		t.Fatalf("kind = %q, want question", kind)
	}
	if !strings.Contains(prompt, "哪个部分") {
		t.Fatalf("prompt = %q, want the full-width question line", prompt)
	}
	want := []string{"nativeui 界面层", "herdr adapter", "随便看看", "Type something.", "Chat about this"}
	if len(options) != len(want) {
		t.Fatalf("options = %q, want %q", options, want)
	}
	for i := range want {
		if options[i] != want[i] {
			t.Fatalf("option %d = %q, want %q", i, options[i], want[i])
		}
	}
}

func TestDetectClaudeInteractionPermissionSheet(t *testing.T) {
	screen := strings.Join([]string{
		"⏺ Reading internal/nativeui/chat_hooks.go",
		"",
		"Do you want to make this edit to chat_hooks.go?",
		"❯ 1. Yes",
		"  2. Yes, allow all edits during this session",
		"  3. No, tell Claude what to do differently (esc)",
		"",
		"Enter to select · Arrow keys to navigate · Esc to cancel",
	}, "\n")
	kind, prompt, options, ok := detectClaudeInteraction(screen)
	if !ok {
		t.Fatal("the permission sheet must detect an interaction")
	}
	if kind != conversation.InteractionPermission {
		t.Fatalf("kind = %q, want permission", kind)
	}
	if !strings.HasPrefix(prompt, "Do you want") {
		t.Fatalf("prompt = %q", prompt)
	}
	if len(options) != 3 || options[2] != "No, tell Claude what to do differently (esc)" {
		t.Fatalf("options = %q", options)
	}
}

func TestDetectClaudeInteractionIgnoresNumberedTranscript(t *testing.T) {
	// Numbered lists in plain transcript output, with no footer and no
	// question line, must not become interactions.
	screen := strings.Join([]string{
		"⏺ Here is the plan:",
		"1. Read the files",
		"2. Write the code",
		"3. Run the tests",
		"",
		"❯ done",
	}, "\n")
	if _, _, _, ok := detectClaudeInteraction(screen); ok {
		t.Fatal("plain numbered transcript must not detect")
	}
}

// TestProjectionEquivalent pins the hook-refresh dedupe: identical agent
// status surfaces skip the apply; status, revision or membership changes
// must apply.
func TestProjectionEquivalent(t *testing.T) {
	base := herdr.Projection{
		Agents: []herdr.Agent{
			{PaneID: "p1", Status: "idle", Revision: 3},
			{PaneID: "p2", Status: "working", Revision: 8},
		},
		Panes: []herdr.Pane{{ID: "p1"}, {ID: "p2"}},
		Tabs:  []herdr.Tab{{ID: "t1"}},
	}
	if !projectionEquivalent(base, base) {
		t.Fatal("identical projections must compare equivalent")
	}
	flip := base
	agents := append([]herdr.Agent(nil), base.Agents...)
	agents[1].Status = "idle"
	flip.Agents = agents
	if projectionEquivalent(base, flip) {
		t.Fatal("a status change must not compare equivalent")
	}
	bump := base
	agents = append([]herdr.Agent(nil), base.Agents...)
	agents[0].Revision = 4
	bump.Agents = agents
	if projectionEquivalent(base, bump) {
		t.Fatal("a revision bump must not compare equivalent")
	}
	grow := base
	grow.Panes = append(grow.Panes, herdr.Pane{ID: "p3"})
	if projectionEquivalent(base, grow) {
		t.Fatal("a pane membership change must not compare equivalent")
	}
}

// TestScheduleProjectionRefreshHeadlessGuard pins the guard: without a
// runtime the hook-driven refresh is a no-op and never latches the debounce.
func TestScheduleProjectionRefreshHeadlessGuard(t *testing.T) {
	shell := NewShell()
	shell.scheduleProjectionRefresh()
	shell.chatRefreshMu.Lock()
	pending := shell.chatRefreshPending
	shell.chatRefreshMu.Unlock()
	if pending {
		t.Fatal("headless shell must not latch the refresh debounce")
	}
}

// TestReconcileBlockedInteractionsDeduplicates pins the pending-guard: a
// pane with a live pending interaction is not re-detected, so repeated
// projection applies can never duplicate interaction cards.
func TestReconcileBlockedInteractionsDeduplicates(t *testing.T) {
	shell := NewShell()
	shell.projection = herdr.Projection{
		Agents: []herdr.Agent{
			{TerminalID: "term-1", PaneID: "p1", Status: "blocked"},
		},
	}
	shell.interactionBroker.Publish(conversation.PendingInteraction{
		ID:             "ia-seed",
		ConversationID: "live:claude:term-1",
		AgentPaneID:    "p1",
		Kind:           conversation.InteractionQuestion,
		Prompt:         "seeded",
		Options:        []string{"a", "b"},
	})
	for range 3 {
		shell.reconcileBlockedInteractions()
		if pending := len(shell.interactionBroker.PendingForPane("p1")); pending != 1 {
			t.Fatalf("pending interactions = %d, want 1", pending)
		}
	}
}

// TestReconcileBlockedInteractionsDispatchesSourceless pins the other side:
// a blocked pane without a pending interaction is scheduled for detection
// (headless: detect is a no-op on the nil runtime, but the reconcile must
// not touch paned that are not blocked).
func TestReconcileBlockedInteractionsDispatchesSourceless(t *testing.T) {
	shell := NewShell()
	shell.projection = herdr.Projection{
		Agents: []herdr.Agent{
			{TerminalID: "term-1", PaneID: "p1", Status: "blocked"},
			{TerminalID: "term-2", PaneID: "p2", Status: "idle"},
		},
	}
	shell.reconcileBlockedInteractions()
	if pending := len(shell.interactionBroker.PendingForPane("p1")); pending != 0 {
		t.Fatalf("detection without a runtime must not publish: %d", pending)
	}
	if pending := len(shell.interactionBroker.PendingForPane("p2")); pending != 0 {
		t.Fatalf("idle pane must never be detected: %d", pending)
	}
}
