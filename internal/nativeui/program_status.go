package nativeui

import (
	"log/slog"
	"time"

	"github.com/wh-studio/herdr-client/internal/herdr"
)

/**
 * [INPUT]: 依赖 internal/agentstatus 的 Store、log/slog、time；Shell 的投影与刷新链路
 * [OUTPUT]: 对外提供 Shell.applyProgramStatus、Shell.applyPaneStatus
 * [POS]: nativeui 的 OSC 7501 接线层：终端回调喂入 statusStore，仲裁后的状态写回投影并驱动窗格交互；hook 路径经 chat_hooks.go 共用 applyPaneStatus
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// applyProgramStatus consumes one OSC 7501 report body from a pane's
// terminal. The store arbitrates; whatever survives is applied through the
// same pane-status path the hook stream uses. Report text (msg, title)
// is never logged.
func (s *Shell) applyProgramStatus(paneID string, body []byte) {
	status, ok := s.statusStore.ApplyProgramReport(paneID, body, time.Now())
	if !ok {
		// Not a report this build understands (unknown state, bad
		// framing): ignored whole, per the protocol.
		return
	}
	slog.Debug("osc7501 status applied", "pane", paneID, "status", status)
	s.upsertProjectionAgent(paneID, func(agent *herdr.Agent) {
		agent.Status = status
	})
	s.applyPaneStatus(paneID, status)
}

// applyPaneStatus writes one pane's effective status into the in-memory
// projection and fans out the follow-ups: the blocked-interaction
// detection and the refresh/bridge cycle. The pane's Agent card status is
// the caller's concern — each event path performs exactly one
// upsertProjectionAgent, so Revision advances once per event.
func (s *Shell) applyPaneStatus(paneID, status string) {
	for i := range s.projection.Panes {
		if s.projection.Panes[i].ID == paneID {
			s.projection.Panes[i].AgentStatus = status
			break
		}
	}

	// A blocked pane opens the interaction detection; other statuses
	// cancel any pending interaction for the pane.
	if status == "blocked" {
		s.dispatch(func() { s.detectPaneInteraction(paneID) })
		return
	}
	s.interactionBroker.CancelForPane(paneID)
	s.scheduleProjectionRefresh()
	s.publishChatBridge()
}
