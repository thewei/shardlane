package nativeui

import (
	"context"
	"encoding/json"
	"fmt"
	"log/slog"
	"regexp"
	"strings"
	"time"

	"github.com/wh-studio/herdr-client/internal/conversation"
	"github.com/wh-studio/herdr-client/internal/herdr"
	"github.com/wh-studio/herdr-client/internal/herdrhooks"
)

/**
 * [INPUT]: 依赖 herdrhooks spool（我们的 herdr 插件转发的事件）、runtime.ReadPane（pane.read 可见文本）、InteractionBroker
 * [OUTPUT]: 提供 Shell hook 层：blocked 触发的最小 Claude 交互检测 → 结构化 PendingInteraction、非 blocked 的 pane 级取消、实时投影重推
 * [POS]: 我们自己的 hook 层（N19）：基于 herdr 插件事件钩子，app 内解析与裁决；完整解析表留在 web-ui 参照，最小实现先行
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// AttachHookSpool starts the hook spool tailer (production only: the spool
// exists when the Shardlane herdr plugin is linked). Hook events reconcile
// the interaction lifecycle and republish the chat projection. The tailer
// reads only NEW lines; agents already blocked at attach get a detection
// pass here (restart mid-prompt) — the authoritative pass runs once the
// first projection lands (applyProjection), because at attach time the
// bootstrap has not fetched any agents yet.
func (s *Shell) AttachHookSpool() {
	stop, err := herdrhooks.Start(herdrhooks.DefaultSpoolPath(), func(event herdrhooks.Event) {
		s.applyOnWindow(func() { s.handleHookEvent(event) })
	})
	if err != nil {
		slog.Warn("hook spool attach failed", "error", err)
		return
	}
	slog.Info("hook spool tailer attached", "spool", herdrhooks.DefaultSpoolPath())
	s.hooksStop = stop
	s.dispatch(func() { s.reconcileBlockedInteractions() })
}

// reconcileBlockedInteractions runs one detection pass over every agent the
// current projection reports as blocked. Idempotent: panes with a pending
// interaction are skipped, so repeated applies never duplicate cards.
func (s *Shell) reconcileBlockedInteractions() {
	for index := range s.projection.Agents {
		agent := &s.projection.Agents[index]
		if agent.Status != "blocked" || agent.PaneID == "" {
			continue
		}
		if len(s.interactionBroker.PendingForPane(agent.PaneID)) > 0 {
			continue
		}
		paneID := agent.PaneID
		s.dispatch(func() { s.detectPaneInteraction(paneID) })
	}
}

// handleHookEvent is the hook layer's dispatch (UI lane).
func (s *Shell) handleHookEvent(event herdrhooks.Event) {
	if event.Name != "pane.agent_status_changed" {
		return
	}
	var envelope herdrhooks.Envelope
	if err := json.Unmarshal(event.Payload, &envelope); err != nil || len(envelope.Data) == 0 {
		slog.Warn("hook event undecodable", "event", event.Name)
		return
	}
	var payload herdrhooks.StatusPayload
	if err := json.Unmarshal(envelope.Data, &payload); err != nil || payload.PaneID == "" {
		slog.Warn("hook status payload undecodable", "event", event.Name)
		return
	}
	slog.Debug("hook status event", "pane", payload.PaneID, "status", payload.AgentStatus)
	if payload.AgentStatus == "blocked" {
		// The agent is waiting on its TUI: read the visible screen and
		// publish whatever structured prompt it holds. Off the UI lane —
		// pane.read is a blocking RPC.
		s.dispatch(func() { s.detectPaneInteraction(payload.PaneID) })
		return
	}
	// Any other state ends the blocked window: its questions are gone.
	s.interactionBroker.CancelForPane(payload.PaneID)
	// Status edges must reach the shell projection too: the composer
	// disposition, the cards and the tray all read it, and the socket event
	// watcher is not guaranteed to have delivered this transition.
	s.scheduleProjectionRefresh()
	s.publishChatBridge()
}

// chatRefreshDebounce coalesces hook-driven projection refetches; status
// transitions arrive in tight working→idle pairs and one refresh covers both.
const chatRefreshDebounce = 150 * time.Millisecond

// scheduleProjectionRefresh refetches the session snapshot off the UI lane
// and applies it when it differs from the shell's copy. This is the hook
// layer's second job: it keeps the runtime status honest even when the
// socket event watcher missed a transition (the spool is written by Herdr
// itself, so it is the authoritative fallback feed).
func (s *Shell) scheduleProjectionRefresh() {
	if s.runtime == nil || s.activeInstance == "" {
		return
	}
	s.chatRefreshMu.Lock()
	if s.chatRefreshPending {
		s.chatRefreshMu.Unlock()
		return
	}
	s.chatRefreshPending = true
	s.chatRefreshMu.Unlock()
	time.AfterFunc(chatRefreshDebounce, func() {
		instance := s.activeInstance
		projection, err := s.runtime.Projection(instance)
		s.applyOnWindow(func() {
			s.chatRefreshMu.Lock()
			s.chatRefreshPending = false
			s.chatRefreshMu.Unlock()
			if err != nil || instance != s.activeInstance {
				return
			}
			if projectionEquivalent(projection, s.projection) {
				return
			}
			s.applyProjection(projection, false)
		})
	})
}

// projectionEquivalent compares the fields the hook layer refreshes for:
// the per-agent status surface. Structural changes always apply.
func projectionEquivalent(a, b herdr.Projection) bool {
	if len(a.Agents) != len(b.Agents) || len(a.Panes) != len(b.Panes) || len(a.Tabs) != len(b.Tabs) {
		return false
	}
	for index := range a.Agents {
		if a.Agents[index].PaneID != b.Agents[index].PaneID ||
			a.Agents[index].Status != b.Agents[index].Status ||
			a.Agents[index].Revision != b.Agents[index].Revision {
			return false
		}
	}
	return true
}

// detectPaneInteraction reads the pane's visible TUI and publishes a
// structured interaction when one of the known prompt shapes is showing.
// Idempotent per pane: a pending interaction short-circuits the read.
func (s *Shell) detectPaneInteraction(paneID string) {
	if s.runtime == nil || s.activeInstance == "" {
		return
	}
	if len(s.interactionBroker.PendingForPane(paneID)) > 0 {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 8*time.Second)
	defer cancel()
	screen, err := s.runtime.ReadPane(ctx, s.activeInstance, paneID, "visible")
	if err != nil || strings.TrimSpace(screen) == "" {
		slog.Warn("interaction detect read failed", "pane", paneID, "error", err)
		return
	}
	kind, prompt, options, ok := detectClaudeInteraction(screen)
	if !ok {
		slog.Info("interaction detect found no prompt shape", "pane", paneID)
		return
	}
	conversationID := conversation.ConversationID("pane:" + paneID)
	if s.chatPaneID == paneID && s.chatConversationID != "" {
		conversationID = conversation.ConversationID(s.chatConversationID)
	}
	fingerprint := s.liveFingerprintForPane(paneID)
	err = s.interactionBroker.Publish(conversation.PendingInteraction{
		ID:             fmt.Sprintf("ia-%s-%d", paneID, time.Now().UnixNano()),
		ConversationID: conversationID,
		AgentPaneID:    paneID,
		Fingerprint:    fingerprint,
		Kind:           kind,
		Prompt:         prompt,
		Options:        options,
	})
	if err == nil {
		slog.Info("interaction published", "pane", paneID, "kind", string(kind), "prompt", prompt)
		s.publishChatBridge()
	} else {
		slog.Warn("interaction publish rejected", "pane", paneID, "error", err)
	}
}

// optionLine matches Claude Code's numbered option rows ("❯ 1. Yes",
// "  2. herdr adapter"), with or without the selector mark.
var optionLine = regexp.MustCompile(`^[❯>•·\s]*(\d+)\.\s+(.+?)\s*$`)

// promptFooter matches the AskUserQuestion/permission footer — the
// strongest "an interactive prompt is showing" signal.
var promptFooter = regexp.MustCompile(`Enter to select`)

// dividerLine matches the full-width rule AskUserQuestion draws between
// questions and extra options.
var dividerLine = regexp.MustCompile(`^─+\s*$`)

// questionLine matches a prompt question in ASCII or full-width punctuation.
var questionSuffix = regexp.MustCompile(`[?？]\s*$`)

// detectClaudeInteraction parses Claude Code's visible TUI text for its
// interactive prompts — AskUserQuestion (tabs + numbered options with
// description rows) and the permission sheet ("Do you want …"). A minimal
// port of the reference parser's core; the full provider table lives in
// herdr-web-ui's prompt.ts and ports incrementally.
func detectClaudeInteraction(screen string) (conversation.InteractionKind, string, []string, bool) {
	lines := strings.Split(strings.ReplaceAll(screen, "\r\n", "\n"), "\n")
	// Drop trailing empties so the last row is meaningful.
	for len(lines) > 0 && strings.TrimSpace(lines[len(lines)-1]) == "" {
		lines = lines[:len(lines)-1]
	}
	// Strong signal: the footer. Collect the numbered options between the
	// footer (and its divider) and the question line above them.
	if footer := lastLineIndex(lines, promptFooter); footer != -1 {
		if kind, prompt, options, ok := parseOptionRun(lines, footer-1); ok {
			return kind, prompt, options, true
		}
	}
	// Fallback: the last numbered-option run anywhere, with a question line
	// above it.
	return parseOptionRun(lines, len(lines)-1)
}

// parseOptionRun walks up from `from`, collecting the numbered-option run —
// description rows, blanks and dividers between options are part of the
// block — and then picks the closest row below-reading-as-a-question: the
// first candidate (bottom-up) ending in ?/？ or opening "Do you want".
// Plain numbered transcript content never qualifies, so the fallback pass
// cannot misfire on ordinary lists.
func parseOptionRun(lines []string, from int) (conversation.InteractionKind, string, []string, bool) {
	options := make([]string, 0, 4)
	var candidates []string
	for i := from; i >= 0 && i > from-40; i-- {
		line := lines[i]
		trimmed := strings.TrimSpace(line)
		if dividerLine.MatchString(trimmed) {
			continue
		}
		if match := optionLine.FindStringSubmatch(line); match != nil {
			options = append([]string{strings.TrimSpace(match[2])}, options...)
			continue
		}
		if trimmed == "" {
			continue
		}
		candidates = append(candidates, trimmed)
	}
	if len(options) < 2 {
		return "", "", nil, false
	}
	prompt := ""
	for _, candidate := range candidates {
		if questionSuffix.MatchString(candidate) || strings.HasPrefix(candidate, "Do you want") {
			prompt = candidate
			break
		}
	}
	if prompt == "" {
		return "", "", nil, false
	}
	kind := conversation.InteractionQuestion
	if strings.Contains(prompt, "Do you want") {
		kind = conversation.InteractionPermission
	}
	return kind, prompt, options, true
}

// lastLineIndex returns the last line matching re, or -1.
func lastLineIndex(lines []string, re *regexp.Regexp) int {
	for i := len(lines) - 1; i >= 0; i-- {
		if re.MatchString(lines[i]) {
			return i
		}
	}
	return -1
}
