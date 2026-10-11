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
// pass here (restart mid-prompt).
func (s *Shell) AttachHookSpool() {
	stop, err := herdrhooks.Start(herdrhooks.DefaultSpoolPath(), func(event herdrhooks.Event) {
		s.applyOnWindow(func() { s.handleHookEvent(event) })
	})
	if err != nil {
		slog.Warn("hook spool attach failed", "error", err)
		return
	}
	s.hooksStop = stop
	s.dispatch(func() {
		for index := range s.projection.Agents {
			agent := &s.projection.Agents[index]
			if agent.Status == "blocked" && agent.PaneID != "" {
				s.detectPaneInteraction(agent.PaneID)
			}
		}
	})
}

// handleHookEvent is the hook layer's dispatch (UI lane).
func (s *Shell) handleHookEvent(event herdrhooks.Event) {
	if event.Name != "pane.agent_status_changed" {
		return
	}
	var envelope herdrhooks.Envelope
	if err := json.Unmarshal(event.Payload, &envelope); err != nil || len(envelope.Data) == 0 {
		return
	}
	var payload herdrhooks.StatusPayload
	if err := json.Unmarshal(envelope.Data, &payload); err != nil || payload.PaneID == "" {
		return
	}
	if payload.AgentStatus == "blocked" {
		// The agent is waiting on its TUI: read the visible screen and
		// publish whatever structured prompt it holds. Off the UI lane —
		// pane.read is a blocking RPC.
		s.dispatch(func() { s.detectPaneInteraction(payload.PaneID) })
		return
	}
	// Any other state ends the blocked window: its questions are gone.
	s.interactionBroker.CancelForPane(payload.PaneID)
	s.publishChatBridge()
}

// detectPaneInteraction reads the pane's visible TUI and publishes a
// structured interaction when one of the known prompt shapes is showing.
func (s *Shell) detectPaneInteraction(paneID string) {
	if s.runtime == nil || s.activeInstance == "" {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 8*time.Second)
	defer cancel()
	screen, err := s.runtime.ReadPane(ctx, s.activeInstance, paneID, "visible")
	if err != nil || strings.TrimSpace(screen) == "" {
		return
	}
	kind, prompt, options, ok := detectClaudeInteraction(screen)
	if !ok {
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
		s.publishChatBridge()
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
