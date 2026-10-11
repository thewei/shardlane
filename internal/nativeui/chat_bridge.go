package nativeui

import (
	"context"

	"github.com/egoist/mygo"
	"github.com/wh-studio/herdr-client/internal/conversation"
)

/**
 * [INPUT]: 依赖 Shell 的 0.6 Chat 状态（chatTurns/chatDisposition/chatOutcome/chatQueuedText）、launch 发送路径、mygo Bind 事件
 * [OUTPUT]: 对外提供 ChatBridge 绑定服务（Snapshot/Send/CancelQueued）与 chat:updated 事件、Shell.publishChatBridge、sendChatText 共享发送路径
 * [POS]: WebView Chat 的唯一表现层桥：投影与动作编组到窗口 UI 通道，会话运行时/Herdr 方法零改动；页面只能通过此桥读取聊天投影
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// ChatBridgeTool is one tool run rendered inside an assistant turn.
type ChatBridgeTool struct {
	Name    string `json:"name"`
	Input   string `json:"input,omitempty"`
	Output  string `json:"output,omitempty"`
	IsError bool   `json:"isError,omitempty"`
}

// ChatBridgeTurn is one derived timeline row (0.6 §13 contract): user rows
// verbatim; assistant turns carry visible narration, consolidated thinking
// and the (possibly grouped) tool activity.
type ChatBridgeTurn struct {
	User      bool             `json:"user"`
	Text      string           `json:"text,omitempty"`
	Narration string           `json:"narration,omitempty"`
	Thinking  *string          `json:"thinking,omitempty"`
	Failed    bool             `json:"failed,omitempty"`
	Compacted bool             `json:"compacted,omitempty"`
	Tools     []ChatBridgeTool `json:"tools,omitempty"`
}

// ChatBridgeState is the full chat projection the page renders. It is
// presentation state only: the conversation runtime, the send path and the
// follow-up ledger stay in Go (internal/agent + internal/herdr).
type ChatBridgeState struct {
	ConversationID string           `json:"conversationId"`
	Title          string           `json:"title,omitempty"`
	Provider       string           `json:"provider,omitempty"`
	Loading        bool             `json:"loading"`
	Disposition    string           `json:"disposition"`
	Outcome        string           `json:"outcome,omitempty"`
	QueuedText     string           `json:"queuedText,omitempty"`
	Turns          []ChatBridgeTurn `json:"turns"`
}

// chatUpdated pushes the projection after every chat state mutation. Events
// emitted before the page is ready are delivered once it is, so the page
// also starts from Snapshot and stays current through this event alone.
var chatUpdated = mygo.NewEvent[ChatBridgeState]("chat:updated")

// ChatBridge is the bound service of the WebView chat surface. Bound methods
// run on their own goroutines, so every call marshals onto the window UI
// lane before touching Shell state.
type ChatBridge struct {
	shell *Shell
}

// NewChatBridge builds the chat bound service over the shell's chat state.
func NewChatBridge(shell *Shell) *ChatBridge {
	return &ChatBridge{shell: shell}
}

// Snapshot returns the current chat projection.
func (b *ChatBridge) Snapshot(ctx context.Context) (ChatBridgeState, error) {
	var state ChatBridgeState
	b.shell.callOnWindow(func() {
		state = b.shell.chatBridgeState()
	})
	return state, nil
}

// Send routes the prompt through the same disposition decision as the
// native composer: SentNow delivers via the canonical agent.prompt
// transaction, MidTurn queues, terminal-needed agents are refused. The
// returned projection carries the outcome line; the page keeps its draft
// whenever one is set (uncertain deliveries never look like clean sends).
func (b *ChatBridge) Send(ctx context.Context, text string) (ChatBridgeState, error) {
	var state ChatBridgeState
	b.shell.callOnWindow(func() {
		state = b.shell.sendChatText(text)
	})
	return state, nil
}

// CancelQueued cancels the queued follow-up (only possible before its
// delivery commit) and returns the projection.
func (b *ChatBridge) CancelQueued(ctx context.Context) (ChatBridgeState, error) {
	var state ChatBridgeState
	b.shell.callOnWindow(func() {
		b.shell.cancelChatFollowUp()
		state = b.shell.chatBridgeState()
	})
	return state, nil
}

// callOnWindow runs fn on the window UI lane and waits for it; inline when
// no window exists (headless tests). Shell-bound methods run on their own
// goroutines, so this is the only path back to Shell state.
func (s *Shell) callOnWindow(fn func()) {
	if s.win == nil {
		fn()
		return
	}
	done := make(chan struct{})
	s.applyOnWindow(func() {
		fn()
		close(done)
	})
	<-done
}

// chatBridgeState projects the chat state (UI lane only).
func (s *Shell) chatBridgeState() ChatBridgeState {
	state := ChatBridgeState{
		ConversationID: s.chatConversationID,
		Loading:        s.chatLoading,
		Disposition:    chatBridgeDisposition(s.chatDisposition),
		Outcome:        s.chatOutcome,
		QueuedText:     s.chatQueuedText,
		Turns:          make([]ChatBridgeTurn, 0, len(s.chatTurns)),
	}
	if card, ok := s.agentCardForSelectedPane(); ok {
		state.Title = card.Title
		state.Provider = card.ProviderLabel
	}
	for _, turn := range s.chatTurns {
		bridgeTurn := ChatBridgeTurn{
			User:      turn.UserRow,
			Text:      turn.Text,
			Narration: turn.Narration,
			Thinking:  turn.Thinking,
			Failed:    turn.Failed,
			Compacted: turn.Compacted,
		}
		for _, run := range turn.ToolRuns {
			tool := ChatBridgeTool{Name: run.Name, Input: run.InputPreview, IsError: run.IsError}
			if run.Output != nil {
				tool.Output = *run.Output
			}
			bridgeTurn.Tools = append(bridgeTurn.Tools, tool)
		}
		state.Turns = append(state.Turns, bridgeTurn)
	}
	return state
}

// chatBridgeDisposition maps the composer disposition to the page's union.
func chatBridgeDisposition(disposition conversation.PromptDisposition) string {
	switch disposition {
	case conversation.DispositionSentNow:
		return "ready"
	case conversation.DispositionQueuedAfterTurn:
		return "queue"
	case conversation.DispositionNeedsTerminal:
		return "terminal"
	default:
		return "unknown"
	}
}

// publishChatBridge pushes the projection to the WebView chat page. Called
// on the UI lane after every chat state mutation; a no-op without a window.
func (s *Shell) publishChatBridge() {
	if s.win == nil || s.chatConversationID == "" {
		return
	}
	// A closed window ends delivery; the page is gone with it.
	_ = chatUpdated.Emit(s.win, s.chatBridgeState())
}
