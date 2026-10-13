package nativeui

import (
	"context"
	"encoding/json"
	"fmt"
	"log/slog"
	"regexp"
	"strings"
	"time"

	"github.com/wh-studio/herdr-client/internal/apphook"
	"github.com/wh-studio/herdr-client/internal/conversation"
	"github.com/wh-studio/herdr-client/internal/herdr"
	"github.com/wh-studio/herdr-client/internal/herdrhooks"
)

/**
 * [INPUT]: 依赖 apphook（进程内 IPC Hook Server）、herdrhooks spool（可选兼容向后尾部）、runtime.ReadPane（pane.read 可见文本）、InteractionBroker
 * [OUTPUT]: 提供 Shell hook 层：直接 Hook 状态接收与会话更新、blocked 触发的交互检测 → 结构化 PendingInteraction、非 blocked 的 pane 级取消、实时投影重推
 * [POS]: 我们自己的原生 hook 层：进程内 Unix Domain Socket 服务就地接收并更新 Agent 状态，消除对 Herdr 插件与外部进程的硬依赖
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// AttachHookServer starts our in-app Unix domain socket server for zero-overhead
// direct hook state ingestion, and attaches the legacy spool tailer if present.
func (s *Shell) AttachHookServer() {
	if s.hookServer == nil {
		server := apphook.NewServer(apphook.DefaultSocketPath(), apphook.Handler{
			OnStatus: func(evt apphook.StatusEvent) {
				s.applyOnWindow(func() { s.handleAppHookStatus(evt) })
			},
			OnSession: func(evt apphook.SessionEvent) {
				s.applyOnWindow(func() { s.handleAppHookSession(evt) })
			},
		})
		if runtimeSock := apphook.DefaultRuntimeSocketPath(); runtimeSock != "" {
			server.SetSymlinkPath(runtimeSock)
		}
		if err := server.Start(); err != nil {
			slog.Warn("apphook server start failed", "error", err)
		} else {
			s.hookServer = server
		}
	}
	s.AttachHookSpool()
}

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

// handleAppHookStatus applies a directly reported agent status event onto
// the shell projection and interaction broker (UI lane).
// upsertProjectionAgent locates the projection agent bound to a pane, or
// creates one from the pane's row when the hook event arrived first; the
// mutation applies in place and always bumps the revision. The two app-hook
// handlers used to inline this locate-or-create twice (F171).
func (s *Shell) upsertProjectionAgent(paneID string, mutate func(*herdr.Agent)) {
	for i := range s.projection.Agents {
		agent := &s.projection.Agents[i]
		if agent.PaneID == paneID {
			mutate(agent)
			agent.Revision++
			return
		}
	}
	var termID, projID, tabID string
	for _, p := range s.projection.Panes {
		if p.ID == paneID {
			termID = p.TerminalID
			projID = p.ProjectID
			tabID = p.TabID
			break
		}
	}
	created := herdr.Agent{
		TerminalID: termID,
		ProjectID:  projID,
		TabID:      tabID,
		PaneID:     paneID,
		Revision:   1,
	}
	mutate(&created)
	s.projection.Agents = append(s.projection.Agents, created)
}

// chatSessionIdentity builds the apphook-sourced session identity carried
// by status and session events.
func chatSessionIdentity(appAgent, sessionID string) *herdr.AgentSessionIdentity {
	if sessionID == "" {
		return nil
	}
	return &herdr.AgentSessionIdentity{
		Agent:  appAgent,
		Kind:   "id",
		Source: "apphook",
		Value:  sessionID,
	}
}

func (s *Shell) handleAppHookStatus(evt apphook.StatusEvent) {
	if evt.PaneID == "" {
		return
	}
	slog.Debug("hook status event", "pane", evt.PaneID, "status", evt.Status, "agent", evt.Agent)

	// Direct update to the in-memory projection (locate-or-create in one
	// place; see upsertProjectionAgent).
	session := chatSessionIdentity(evt.Agent, evt.SessionID)
	s.upsertProjectionAgent(evt.PaneID, func(agent *herdr.Agent) {
		agent.Status = evt.Status
		if agent.Kind == "" && evt.Agent != "" {
			agent.Kind = evt.Agent
		}
		if agent.AgentSession == nil {
			agent.AgentSession = session
		}
	})

	for i := range s.projection.Panes {
		if s.projection.Panes[i].ID == evt.PaneID {
			s.projection.Panes[i].AgentStatus = evt.Status
			break
		}
	}

	// 2. Handle blocked interaction detection or cancellation
	if evt.Status == "blocked" {
		s.dispatch(func() { s.detectPaneInteraction(evt.PaneID) })
		return
	}

	s.interactionBroker.CancelForPane(evt.PaneID)
	s.scheduleProjectionRefresh()
	s.publishChatBridge()
}

// handleAppHookSession updates the agent session identity directly in the
// shell projection (UI lane).
func (s *Shell) handleAppHookSession(evt apphook.SessionEvent) {
	if evt.PaneID == "" {
		return
	}
	slog.Info("hook session event", "pane", evt.PaneID, "agent", evt.Agent, "session_id", evt.AgentSessionID)

	source := evt.Source
	if source == "" {
		source = "apphook"
	}
	val := evt.AgentSessionID
	kind := "id"
	if evt.AgentSessionPath != "" {
		val = evt.AgentSessionPath
		kind = "path"
	}

	sessionIdentity := &herdr.AgentSessionIdentity{
		Agent:  evt.Agent,
		Kind:   kind,
		Source: source,
		Value:  val,
	}

	s.upsertProjectionAgent(evt.PaneID, func(agent *herdr.Agent) {
		agent.AgentSession = sessionIdentity
		if agent.Kind == "" && evt.Agent != "" {
			agent.Kind = evt.Agent
		}
		if agent.Status == "" {
			agent.Status = "idle"
		}
	})

	s.publishChatBridge()
}

// handleHookEvent is the legacy spool tailer's dispatch (UI lane).
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
	s.handleAppHookStatus(apphook.StatusEvent{
		PaneID: payload.PaneID,
		Status: apphook.NormalizeStatus(payload.AgentStatus),
	})
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
