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
// the interaction lifecycle and republish the chat projection.
func (s *Shell) AttachHookSpool() {
	stop, err := herdrhooks.Start(herdrhooks.DefaultSpoolPath(), func(event herdrhooks.Event) {
		s.applyOnWindow(func() { s.handleHookEvent(event) })
	})
	if err != nil {
		slog.Warn("hook spool attach failed", "error", err)
		return
	}
	s.hooksStop = stop
}

// handleHookEvent is the hook layer's dispatch (UI lane).
func (s *Shell) handleHookEvent(event herdrhooks.Event) {
	if event.Name != "pane.agent_status_changed" {
		return
	}
	var payload herdrhooks.StatusPayload
	if err := json.Unmarshal(event.Payload, &payload); err != nil || payload.PaneID == "" {
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
// "2. Yes, and don't ask again"), with or without the selector mark.
var optionLine = regexp.MustCompile(`^[❯>•·\s]*(\d+)\.\s+(.+?)\s*$`)

// detectClaudeInteraction parses Claude Code's visible TUI text for its
// approval and question prompts — a minimal port of the reference parser's
// core (the full provider table lives in herdr-web-ui's prompt.ts and ports
// incrementally). It takes the last numbered-option run on screen and the
// nearest question line above it.
func detectClaudeInteraction(screen string) (conversation.InteractionKind, string, []string, bool) {
	lines := strings.Split(strings.ReplaceAll(screen, "\r\n", "\n"), "\n")
	for i := len(lines) - 1; i >= 0; i-- {
		if !optionLine.MatchString(lines[i]) {
			continue
		}
		start := i
		for start > 0 && optionLine.MatchString(lines[start-1]) {
			start--
		}
		// The prompt line sits within a few rows above the option run.
		for j := start - 1; j >= 0 && j >= start-6; j-- {
			prompt := strings.TrimSpace(lines[j])
			if prompt == "" {
				continue
			}
			if !strings.HasSuffix(prompt, "?") && !strings.HasPrefix(prompt, "Do you want") {
				break // a non-question line ends this run's candidacy
			}
			options := make([]string, 0, i-start+1)
			for k := start; k <= i; k++ {
				if match := optionLine.FindStringSubmatch(lines[k]); match != nil {
					options = append(options, match[2])
				}
			}
			if len(options) < 2 {
				break
			}
			kind := conversation.InteractionQuestion
			if strings.Contains(prompt, "Do you want") {
				kind = conversation.InteractionPermission
			}
			return kind, prompt, options, true
		}
		i = start
	}
	return "", "", nil, false
}


