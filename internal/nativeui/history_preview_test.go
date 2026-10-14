package nativeui

/**
 * [INPUT]: 依赖真实 Codex rollout 夹具、官方 MyGo headless Tester 与 HistoryListView 假体
 * [OUTPUT]: 验证行内预览：点击展开不跳页、再点收起、Open 才进入详情、摘录只取可读消息、过期读取被代际丢弃
 * [POS]: Sessions 列表就地阅读回归；预览每次现读原始会话文件，不缓存
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"context"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/history"
)

const previewTestKey = "codex:22222222-aaaa-bbbb-cccc-000000000002"

// TestHistoryRowClickUnfoldsPreviewInPlace pins the row click: it unfolds the
// latest messages under the row, stays on the list, and folds again on a
// second click.
func TestHistoryRowClickUnfoldsPreviewInPlace(t *testing.T) {
	shell, _, _ := newHistoryTestShell(t)
	shell.router.Replace(routeHistory)
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()

	if tester.HasText("Implemented.") {
		t.Fatalf("the excerpt must not show before the row is opened; texts=%q", tester.Texts())
	}
	if err := tester.Click("Build history"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if shell.router.Path() != routeHistory {
		t.Fatalf("a row click navigated away: %q", shell.router.Path())
	}
	for _, want := range []string{"Implemented.", "You", "Assistant", "Open"} {
		if !tester.HasText(want) {
			t.Fatalf("preview missing %q; texts=%q", want, tester.Texts())
		}
	}

	if err := tester.Click("Build history"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if tester.HasText("Implemented.") || shell.hist.preview.key != "" {
		t.Fatalf("second click did not fold the preview; texts=%q", tester.Texts())
	}
}

// TestHistoryPreviewOpensDetailOnlyOnRequest pins that Open is the way into
// the conversation.
func TestHistoryPreviewOpensDetailOnlyOnRequest(t *testing.T) {
	shell, _, _ := newHistoryTestShell(t)
	shell.router.Replace(routeHistory)
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()
	if err := tester.Click("Build history"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if err := tester.Click("Open"); err != nil {
		t.Fatal(err)
	}
	if got := shell.router.Path(); got != "/history/"+previewTestKey {
		t.Fatalf("Open went to %q", got)
	}
}

// TestPreviewExcerptPicksLatestReadableMessages pins the excerpt rule: the
// last readable user/assistant texts, oldest first, no tool/meta/thinking
// rows, bodies clipped.
func TestPreviewExcerptPicksLatestReadableMessages(t *testing.T) {
	message := func(seq int64, role history.Role, kind history.MessageKind, text string) history.TranscriptMessage {
		return history.TranscriptMessage{Seq: seq, Role: role, Kind: kind, Text: text}
	}
	messages := []history.TranscriptMessage{
		message(1, history.RoleUser, history.MessageText, "first"),
		message(2, history.RoleAssistant, history.MessageText, "second"),
		message(3, history.RoleUser, history.MessageText, "third"),
		message(4, history.RoleAssistant, history.MessageText, ""),
		message(5, history.RoleAssistant, history.MessageMeta, "system noise"),
		message(6, history.RoleAssistant, history.MessageText, "fourth"),
		message(7, history.RoleUser, history.MessageText, "  fifth  "),
		message(8, history.RoleAssistant, history.MessageText, strings.Repeat("x", previewMaxChars+50)),
	}
	got := previewExcerpt(messages)
	if len(got) != previewMessages {
		t.Fatalf("excerpt length = %d, want %d: %+v", len(got), previewMessages, got)
	}
	if got[0].text != "third" || got[1].text != "fourth" || got[2].text != "fifth" {
		t.Fatalf("excerpt order/filter wrong: %+v", got)
	}
	last := []rune(got[3].text)
	if len(last) != previewMaxChars+1 || last[len(last)-1] != '…' {
		t.Fatalf("long body not clipped on a rune boundary: %d runes", len(last))
	}
	if previewExcerpt(nil) != nil {
		t.Fatal("no messages must give no excerpt")
	}
}

// blockingOpenView answers Open only after running a hook — the way to
// supersede a read while it is in flight in a headless (synchronous) test.
type blockingOpenView struct {
	*fakeHistoryView
	onOpen func()
}

func (v blockingOpenView) Open(ctx context.Context, req history.ConversationWindowRequest) (history.TranscriptWindow, error) {
	if v.onOpen != nil {
		v.onOpen()
	}
	return history.TranscriptWindow{
		Key:           req.ConversationID,
		TotalMessages: 1,
		Messages: []history.TranscriptMessage{
			{Seq: 1, Role: history.RoleUser, Kind: history.MessageText, Text: "late answer"},
		},
	}, nil
}

// TestHistoryPreviewDropsSupersededRead pins the generation guard: a read that
// finishes after the preview was closed (or moved to another row) must not
// repopulate it.
func TestHistoryPreviewDropsSupersededRead(t *testing.T) {
	shell := NewShell()
	fake := newFakeHistoryView()
	view := blockingOpenView{fakeHistoryView: fake}
	shell.hist.service = view

	view.onOpen = func() { shell.closeHistoryPreview() }
	shell.hist.service = view
	shell.openHistoryPreview("codex:a")
	if shell.hist.preview.key != "" || len(shell.hist.preview.excerpt) != 0 {
		t.Fatalf("a closed preview was refilled: %+v", shell.hist.preview)
	}

	view.onOpen = nil
	shell.hist.service = view
	shell.openHistoryPreview("codex:b")
	if shell.hist.preview.key != "codex:b" || len(shell.hist.preview.excerpt) != 1 || shell.hist.preview.excerpt[0].text != "late answer" {
		t.Fatalf("the live read did not apply: %+v", shell.hist.preview)
	}
}
