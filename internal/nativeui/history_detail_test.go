package nativeui

/**
 * [INPUT]: 依赖真实 Codex rollout 夹具、官方 MyGo headless Tester 与共享 Chat 状态/桥投影
 * [OUTPUT]: 验证 Sessions 详情走共享 Chat 视图：只读绑定、最新消息优先与向上懒加载、切换重绑、离开释放、不可读来源的恢复态
 * [POS]: 详情页回归；全程读原始会话文件，无任何会话副本
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"context"
	"fmt"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/history"
)

// bigRollout is a Codex rollout of n one-line user messages "message i".
func bigRollout(n int) string {
	var b strings.Builder
	b.WriteString(`{"timestamp":"2026-08-04T09:00:00Z","type":"session_meta","payload":{"cwd":"/work/big","originator":"codex_cli_rs"}}` + "\n")
	for i := 0; i < n; i++ {
		b.WriteString(fmt.Sprintf(`{"timestamp":"2026-08-04T09:%02d:%02dZ","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"message %d"}]}}`, i/60, i%60, i) + "\n")
	}
	return b.String()
}

// TestHistoryDetailSharesTheChatViewReadOnly pins the detail: it binds the
// shared chat detached from every pane, projects it read-only through the
// bridge, and (in the native fallback) renders the timeline with no composer.
func TestHistoryDetailSharesTheChatViewReadOnly(t *testing.T) {
	shell, _, _ := newHistoryTestShell(t)
	shell.router.Replace("/history/" + previewTestKey)
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()

	if !shell.chatReadOnly || shell.chatConversationID != previewTestKey || shell.chatPaneID != "" {
		t.Fatalf("chat binding: readOnly=%v conversation=%q pane=%q", shell.chatReadOnly, shell.chatConversationID, shell.chatPaneID)
	}
	if shell.chatSession == nil || shell.chatSession.Title != "Build history" {
		t.Fatalf("chat session meta = %+v", shell.chatSession)
	}
	for _, want := range []string{"Build history", "codex-demo · Codex", "USER", "ASSISTANT", "Implemented."} {
		if !tester.HasText(want) {
			t.Fatalf("detail missing %q; texts=%q", want, tester.Texts())
		}
	}
	for _, banned := range []string{"Send", "Chat prompt", "Follow up on this conversation…"} {
		if tester.HasText(banned) {
			t.Fatalf("a read-only transcript must not offer %q; texts=%q", banned, tester.Texts())
		}
	}

	state := shell.chatBridgeState()
	if !state.ReadOnly || state.Title != "Build history" || state.Provider != "Codex" || len(state.Turns) != 2 {
		t.Fatalf("bridge state = %+v", state)
	}
	if len(state.Interactions) != 0 || state.QueuedText != "" {
		t.Fatalf("a detached transcript carries no pane interactions: %+v", state)
	}
}

// TestHistoryDetailOpensAtNewestAndPagesUp pins the lazy reading order: the
// newest window first, "earlier" pages prepend until the start is reached.
func TestHistoryDetailOpensAtNewestAndPagesUp(t *testing.T) {
	root := filepath.Join(t.TempDir(), "sessions")
	writeCodexSessionFile(t, root, "rollout-2026-08-04T09-00-00-55555555-aaaa-bbbb-cccc-000000000005.jsonl", bigRollout(150))
	shell := NewShell()
	shell.hist.service = history.NewHistoryService(history.NewCatalog(), []history.SourceRoot{{
		Agent: history.AgentCodex, Directory: root, NativeID: testRolloutNativeID,
	}})
	if _, err := shell.hist.service.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}

	shell.router.Replace("/history/codex:55555555-aaaa-bbbb-cccc-000000000005")
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()

	if shell.chatWindowStart != 90 || len(shell.chatWindowMessages) != history.WindowLimit || !shell.chatHasOlder {
		t.Fatalf("first window: start=%d messages=%d hasOlder=%v", shell.chatWindowStart, len(shell.chatWindowMessages), shell.chatHasOlder)
	}
	if !tester.HasText("message 149") || tester.HasText("message 89") {
		t.Fatalf("the detail must open at the newest messages; texts=%q", tester.Texts())
	}
	if state := shell.chatBridgeState(); !state.HasOlder || len(state.Turns) != history.WindowLimit {
		t.Fatalf("bridge state: hasOlder=%v turns=%d", state.HasOlder, len(state.Turns))
	}

	if err := tester.Click("Earlier Messages"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if shell.chatWindowStart != 30 || len(shell.chatWindowMessages) != 2*history.WindowLimit || !shell.chatHasOlder {
		t.Fatalf("second window: start=%d messages=%d hasOlder=%v", shell.chatWindowStart, len(shell.chatWindowMessages), shell.chatHasOlder)
	}
	if err := tester.Click("Earlier Messages"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if shell.chatWindowStart != 0 || len(shell.chatWindowMessages) != 150 || shell.chatHasOlder {
		t.Fatalf("third window: start=%d messages=%d hasOlder=%v", shell.chatWindowStart, len(shell.chatWindowMessages), shell.chatHasOlder)
	}
	if !tester.HasText("message 0") || tester.HasText("Earlier Messages") {
		t.Fatalf("the start of the session must show without an Earlier control; texts=%q", tester.Texts())
	}
}

// TestHistoryDetailSwitchRebindsAndLeaveReleases pins the lifecycle: another
// session rebinds (the old transcript is gone), and leaving the page frees
// the shared chat so no pane — nor the inspector — inherits it.
func TestHistoryDetailSwitchRebindsAndLeaveReleases(t *testing.T) {
	shell, _, home := newHistoryTestShell(t)
	writeCodexSessionFile(t, filepath.Join(home, "sessions"),
		"rollout-2026-08-02T10-15-00-33333333-aaaa-bbbb-cccc-000000000003.jsonl",
		strings.ReplaceAll(codexFixtureRollout, "Implemented.", "Second session answer"))
	if _, err := shell.hist.service.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}

	shell.router.Replace("/history/" + previewTestKey)
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()
	if !tester.HasText("Implemented.") {
		t.Fatalf("first session missing; texts=%q", tester.Texts())
	}

	shell.router.Replace("/history/codex:33333333-aaaa-bbbb-cccc-000000000003")
	tester.Frame()
	if shell.chatConversationID != "codex:33333333-aaaa-bbbb-cccc-000000000003" {
		t.Fatalf("chat still bound to %q", shell.chatConversationID)
	}
	if !tester.HasText("Second session answer") || tester.HasText("Implemented.") {
		t.Fatalf("the switch did not replace the transcript; texts=%q", tester.Texts())
	}

	// The outgoing page keeps building through the exit animation (it runs
	// on the real clock); the binding drops on the first frames after it.
	shell.router.Replace(routeHistory)
	tester.Frame()
	if !shell.chatReadOnly {
		t.Fatal("the transcript vanished while the page was still sliding out")
	}
	for deadline := time.Now().Add(3 * time.Second); shell.chatReadOnly && time.Now().Before(deadline); {
		time.Sleep(50 * time.Millisecond)
		tester.Frame()
	}
	if shell.chatReadOnly || shell.chatConversationID != "" || len(shell.chatTurns) != 0 || shell.chatSession != nil {
		t.Fatalf("leaving the detail left chat state behind: readOnly=%v conversation=%q turns=%d", shell.chatReadOnly, shell.chatConversationID, len(shell.chatTurns))
	}
}

// TestHistoryDetailUnreadableSessionOffersRecovery pins the failure state: a
// catalog row whose file is gone says so and leads back to the list.
func TestHistoryDetailUnreadableSessionOffersRecovery(t *testing.T) {
	catalog := history.NewCatalog()
	meta := history.SessionMeta{
		Key: "codex:ghost", ID: "ghost", Agent: history.AgentCodex, Title: "Ghost",
		ProjectPath: "/work/ghost", ProjectName: "ghost", FilePath: "/nowhere/rollout.jsonl", UpdatedAt: 1000,
	}
	catalog.Put(meta, 1000, "")
	shell := NewShell(WithHistory(history.NewHistoryService(catalog, nil)))
	shell.router.Replace("/history/codex:ghost")
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()

	for _, want := range []string{"Couldn't read this session.", "Back to Sessions"} {
		if !tester.HasText(want) {
			t.Fatalf("recovery state missing %q; texts=%q", want, tester.Texts())
		}
	}
	if err := tester.Click("Back to Sessions"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if shell.router.Path() != routeHistory {
		t.Fatalf("Back to Sessions went to %q", shell.router.Path())
	}
}
