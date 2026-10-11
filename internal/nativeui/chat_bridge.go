package nativeui

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"mime/multipart"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/wh-studio/herdr-client/internal/conversation"
	"github.com/wh-studio/herdr-client/internal/history"
)

// Voice transcription defaults; both are overridable in Settings -> General.
const (
	voiceDefaultEndpoint = "https://api.openai.com/v1"
	voiceDefaultModel    = "whisper-1"
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
	Live      bool             `json:"live,omitempty"`
	TsMS      *int64           `json:"tsMs,omitempty"`
	Tools     []ChatBridgeTool `json:"tools,omitempty"`
}

// ChatBridgeState is the full chat projection the page renders. It is
// presentation state only: the conversation runtime, the send path and the
// follow-up ledger stay in Go (internal/agent + internal/herdr).
type ChatBridgeState struct {
	ConversationID string `json:"conversationId"`
	Title          string `json:"title,omitempty"`
	Provider       string `json:"provider,omitempty"`
	Loading        bool   `json:"loading"`
	Disposition    string `json:"disposition"`
	Outcome        string `json:"outcome,omitempty"`
	QueuedText     string `json:"queuedText,omitempty"`
	// Theme is the app-resolved appearance ("light" or "dark"), following
	// Settings -> General -> Appearance through mygo.Theme — not the bare
	// system preference.
	Theme string `json:"theme"`
	// Model is the provider model the transcript last recorded, when known
	// (presentation pill only; never a runtime fact source).
	Model string `json:"model,omitempty"`
	// HasOlder reports an earlier history page exists (history-bound chats
	// only; the live tail is bounded by design).
	HasOlder bool `json:"hasOlder,omitempty"`
	// ContextTokens is the provider-recorded size of the current request
	// context, when the transcript states one (context ring only).
	ContextTokens *int64 `json:"contextTokens,omitempty"`
	// Interactions are the bound pane's pending structured interactions
	// (our hook layer's feed): approval/question cards above the composer.
	Interactions []ChatBridgeInteraction `json:"interactions,omitempty"`
	Turns        []ChatBridgeTurn        `json:"turns"`
}

// ChatBridgeInteraction is one pending interaction card.
type ChatBridgeInteraction struct {
	ID      string   `json:"id"`
	Kind    string   `json:"kind"`
	Prompt  string   `json:"prompt"`
	Options []string `json:"options"`
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

// Stop interrupts the agent's current turn with one Escape (the product
// decision of 2026-10-11): agent.send_keys with the named key "esc", the
// same verified named-key operation the terminal's Esc uses — an interrupt,
// never a process kill. The RPC runs off the UI lane; the projection
// refreshes after it settles.
func (b *ChatBridge) Stop(ctx context.Context) (ChatBridgeState, error) {
	var paneID string
	var ready bool
	b.shell.callOnWindow(func() {
		if b.shell.launch != nil && b.shell.activeInstance != "" && b.shell.chatPaneID != "" {
			paneID, ready = b.shell.chatPaneID, true
		}
	})
	if !ready {
		return b.shell.chatBridgeStateOnWindow(), errors.New("no agent bound to this chat")
	}
	err := b.shell.runtime.SendAgentKeys(ctx, b.shell.activeInstance, paneID, []string{"esc"})
	var state ChatBridgeState
	b.shell.callOnWindow(func() {
		b.shell.publishChatBridge()
		state = b.shell.chatBridgeState()
	})
	return state, err
}

// Older pages the history-bound transcript one window up: the load runs on
// the background lane and the refreshed projection arrives through the
// chat:updated event; the returned state is the pre-load snapshot.
func (b *ChatBridge) Older(ctx context.Context) (ChatBridgeState, error) {
	b.shell.callOnWindow(func() { b.shell.chatLoadOlder() })
	return b.shell.chatBridgeStateOnWindow(), nil
}

// PickAttachment opens the native file picker and returns the chosen path,
// empty when the user canceled. The path goes into the draft as an @mention
// the agent's own CLI understands — files are local, nothing is uploaded.
func (b *ChatBridge) PickAttachment(ctx context.Context) (string, error) {
	paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{
		Parent:  mygo.CallerWindow(ctx),
		Title:   "Attach a file",
		Message: "The file path is inserted into the draft as an @mention.",
	})
	if err != nil {
		return "", err
	}
	if len(paths) == 0 {
		return "", nil
	}
	return paths[0], nil
}

// Transcribe sends a recorded clip to the configured OpenAI-compatible
// transcription endpoint (local proxies included) and returns the text.
// The API key never reaches the page and is never logged.
func (b *ChatBridge) Transcribe(ctx context.Context, audioBase64 string, mime string) (string, error) {
	var endpoint, key, model string
	b.shell.callOnWindow(func() {
		voice := b.shell.settings.Voice
		endpoint, key, model = voice.EndpointURL, voice.APIKey, voice.Model
	})
	if key == "" {
		key = os.Getenv("OPENAI_API_KEY")
	}
	if endpoint == "" {
		endpoint = voiceDefaultEndpoint
	}
	if model == "" {
		model = voiceDefaultModel
	}
	audio, err := base64.StdEncoding.DecodeString(audioBase64)
	if err != nil || len(audio) == 0 {
		return "", errors.New("empty or malformed recording")
	}
	if mime == "" {
		mime = "audio/webm"
	}

	body := &bytes.Buffer{}
	writer := multipart.NewWriter(body)
	part, err := writer.CreateFormFile("file", "clip."+strings.SplitN(mime, "/", 2)[1])
	if err == nil {
		part.Write(audio)
		writer.WriteField("model", model)
		writer.WriteField("response_format", "json")
		err = writer.Close()
	}
	if err != nil {
		return "", err
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, strings.TrimSuffix(endpoint, "/")+"/audio/transcriptions", body)
	if err != nil {
		return "", err
	}
	req.Header.Set("Content-Type", writer.FormDataContentType())
	req.Header.Set("Authorization", "Bearer "+key)
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return "", fmt.Errorf("could not reach the transcription endpoint: %w", err)
	}
	defer resp.Body.Close()
	raw, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if err != nil {
		return "", err
	}
	if resp.StatusCode != http.StatusOK {
		detail := string(raw)
		if payload := map[string]any{}; json.Unmarshal(raw, &payload) == nil {
			if inner, ok := payload["error"].(map[string]any); ok {
				if message, ok := inner["message"].(string); ok {
					detail = message
				}
			}
		}
		if resp.StatusCode == http.StatusUnauthorized || resp.StatusCode == http.StatusForbidden {
			return "", errors.New("transcription auth failed — check the voice API key")
		}
		return "", fmt.Errorf("transcription failed (%d): %s", resp.StatusCode, detail)
	}
	var payload struct {
		Text string `json:"text"`
	}
	if err := json.Unmarshal(raw, &payload); err != nil {
		return "", err
	}
	return strings.TrimSpace(payload.Text), nil
}

// AnswerInteraction claims the interaction (CAS, first answer wins) and
// delivers the choice as the option's arrow-navigation key sequence —
// arrows, never digits, which would type into the TUI (the reference
// transport's rule). The interaction is consumed even if the delivery
// fails: the agent stays blocked and may re-ask.
func (b *ChatBridge) AnswerInteraction(ctx context.Context, interactionID, option string) (ChatBridgeState, error) {
	var paneID string
	var keys []string
	var prepareErr error
	b.shell.callOnWindow(func() {
		var chosen *conversation.PendingInteraction
		for _, interaction := range b.shell.interactionBroker.PendingForPane(b.shell.chatPaneID) {
			if interaction.ID == interactionID {
				value := interaction
				chosen = &value
				break
			}
		}
		if chosen == nil {
			prepareErr = errors.New("the interaction is no longer pending")
			return
		}
		index := -1
		for i, candidate := range chosen.Options {
			if candidate == option {
				index = i
				break
			}
		}
		if index < 0 {
			prepareErr = conversation.ErrUnknownOption
			return
		}
		if _, err := b.shell.interactionBroker.ResolveCAS(interactionID, option); err != nil {
			prepareErr = err
			return
		}
		paneID = chosen.AgentPaneID
		keys = make([]string, 0, index+1)
		for i := 0; i < index; i++ {
			keys = append(keys, "down")
		}
		keys = append(keys, "enter")
	})
	if prepareErr != nil {
		return b.shell.chatBridgeStateOnWindow(), prepareErr
	}
	err := b.shell.runtime.SendAgentKeys(ctx, b.shell.activeInstance, paneID, keys)
	var state ChatBridgeState
	b.shell.callOnWindow(func() {
		b.shell.publishChatBridge()
		state = b.shell.chatBridgeState()
	})
	return state, err
}

// ListFiles lists the chat pane's project directory entries matching the
// composer's typed @ token (shallow; a "sub/dir/" prefix descends into the
// subdirectory). Bounded, contains-match, dotfiles only when asked for.
func (b *ChatBridge) ListFiles(ctx context.Context, token string) ([]string, error) {
	var dir string
	b.shell.callOnWindow(func() {
		if a := b.shell.paneAgent(b.shell.chatPaneID); a != nil && a.CWD != "" {
			dir = a.CWD
		}
	})
	if dir == "" {
		return nil, nil
	}
	sub, name := "", strings.ToLower(strings.TrimPrefix(token, "./"))
	if at := strings.LastIndex(name, "/"); at != -1 {
		sub, name = name[:at], name[at+1:]
	}
	entries, err := os.ReadDir(filepath.Join(dir, filepath.FromSlash(sub)))
	if err != nil {
		return nil, nil // unreadable or gone: no suggestions, no error spam
	}
	found := make([]string, 0, 8)
	for _, entry := range entries {
		entryName := entry.Name()
		if strings.HasPrefix(entryName, ".") && !strings.HasPrefix(name, ".") {
			continue
		}
		if name != "" && !strings.Contains(strings.ToLower(entryName), name) {
			continue
		}
		label := entryName
		if entry.IsDir() {
			label += "/"
		}
		found = append(found, label)
		if len(found) >= 20 {
			break
		}
	}
	return found, nil
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
		Disposition:    chatBridgeDisposition(s.chatDispositionNow()),
		Outcome:        s.chatOutcome,
		QueuedText:     s.chatQueuedText,
		Theme:          s.chatBridgeTheme(),
		Turns:          make([]ChatBridgeTurn, 0, len(s.chatTurns)),
	}
	if card, ok := s.agentCardForSelectedPane(); ok {
		state.Title = card.Title
		state.Provider = card.ProviderLabel
	}
	state.Model = s.chatModel
	state.HasOlder = s.chatHasOlder
	state.ContextTokens = s.chatContextTokens
	if s.chatPaneID != "" {
		for _, interaction := range s.interactionBroker.PendingForPane(s.chatPaneID) {
			state.Interactions = append(state.Interactions, ChatBridgeInteraction{
				ID:      interaction.ID,
				Kind:    string(interaction.Kind),
				Prompt:  interaction.Prompt,
				Options: interaction.Options,
			})
		}
	}
	for _, turn := range s.chatTurns {
		bridgeTurn := ChatBridgeTurn{
			User:      turn.UserRow,
			Text:      turn.Text,
			Narration: turn.Narration,
			Thinking:  turn.Thinking,
			Failed:    turn.Failed,
			Compacted: turn.Compacted,
			Live:      turn.Live,
			TsMS:      turn.TsMS,
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

// chatBridgeTheme reports the app-resolved appearance for the page. The
// framework call blocks on the main thread, so it only runs with a window
// attached and always on the UI lane (chatBridgeState's contract);
// headless sessions (tests) report light without touching the framework.
func (s *Shell) chatBridgeTheme() string {
	if s.win != nil && mygo.Theme.IsDark() {
		return "dark"
	}
	return "light"
}

// chatBridgeStateOnWindow marshals the projection read onto the window lane.
func (s *Shell) chatBridgeStateOnWindow() ChatBridgeState {
	var state ChatBridgeState
	s.callOnWindow(func() { state = s.chatBridgeState() })
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

// chatPublishInterval is the minimum spacing between chat projection
// events while the live tail streams. File appends land well below this
// interval, so the page sees token-level updates without event flooding.
const chatPublishInterval = 120 * time.Millisecond

// publishChatBridge pushes the projection to the WebView chat page. Called
// on the UI lane after every chat state mutation; a no-op without a window.
// The guard is only on the window: an empty projection (a pane without a
// conversation) must still reach the page, or it would keep rendering the
// previously bound conversation's transcript. Publishes coalesce to one
// event per interval; the trailing state always wins.
func (s *Shell) publishChatBridge() {
	if s.win == nil {
		return
	}
	s.chatPublishMu.Lock()
	elapsed := time.Since(s.chatPublishAt)
	if elapsed >= chatPublishInterval {
		s.chatPublishAt = time.Now()
		s.chatPublishMu.Unlock()
		s.emitChatBridge()
		return
	}
	if s.chatPublishTrailer == nil {
		delay := chatPublishInterval - elapsed
		s.chatPublishTrailer = time.AfterFunc(delay, func() {
			s.applyOnWindow(func() {
				s.chatPublishMu.Lock()
				s.chatPublishTrailer = nil
				s.chatPublishAt = time.Now()
				s.chatPublishMu.Unlock()
				s.emitChatBridge()
			})
		})
	}
	s.chatPublishMu.Unlock()
}

// emitChatBridge delivers the current state immediately (UI lane).
func (s *Shell) emitChatBridge() {
	// A closed window ends delivery; the page is gone with it.
	_ = chatUpdated.Emit(s.win, s.chatBridgeState())
}

// lastContextFromMessages picks the latest provider-recorded context size.
func lastContextFromMessages(messages []history.TranscriptMessage) *int64 {
	var context *int64
	for i := len(messages) - 1; i >= 0; i-- {
		if messages[i].ContextTokens != nil {
			context = messages[i].ContextTokens
			break
		}
	}
	return context
}
