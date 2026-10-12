package nativeui

import (
	"sync/atomic"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agenthooks"
)

/**
 * [INPUT]: 依赖 agenthooks.Manager 的诊断与安装、mygo/ui 的 Toast/状态件
 * [OUTPUT]: 对外提供 hookDoctorUIState, requestHookDoctorAudit, runHookDoctorAction, consumeHookDoctorToast, hookStateTone, hookStateText
 * [POS]: Settings→Providers 页的 Agent Hooks（Hook Doctor）展示状态机；审计只在路由进入/手动刷新/动作完成后发生，从不随渲染轮询
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// hookDoctorUIState is the presentation state of the direct-connect Agent
// Hook doctor on the Providers page. It mirrors the integration audit
// discipline: never poll from render.
type hookDoctorUIState struct {
	service *agenthooks.Manager

	loaded   bool
	loading  bool
	errText  string
	rows     []agenthooks.Diagnosis
	auditGen atomic.Uint64

	// action tracking: one provider action in flight, its row disabled
	// until the fresh diagnosis reconciles.
	actionProvider agenthooks.ProviderID
	actionRunning  bool

	// pendingToast is a one-shot transient result consumed by the page.
	pendingToast string
}

// requestHookDoctorAudit reconciles every provider's hook state off the
// presentation path.
func (s *Shell) requestHookDoctorAudit() {
	service := s.hookDoctor.service
	if service == nil {
		return
	}
	if s.hookDoctor.loading {
		return
	}
	s.hookDoctor.loading = true
	s.hookDoctor.errText = ""
	generation := s.hookDoctor.auditGen.Add(1)

	s.dispatch(func() {
		rows := service.Diagnose()
		s.applyOnWindow(func() {
			if generation != s.hookDoctor.auditGen.Load() {
				return
			}
			s.hookDoctor.loading = false
			s.hookDoctor.loaded = true
			s.hookDoctor.rows = rows
		})
	})
}

// runHookDoctorAction executes the one safe install/update action for a
// provider off the presentation path, then reconciles from a fresh audit.
func (s *Shell) runHookDoctorAction(provider agenthooks.ProviderID) {
	service := s.hookDoctor.service
	if service == nil || s.hookDoctor.actionRunning {
		return
	}
	var row *agenthooks.Diagnosis
	for index := range s.hookDoctor.rows {
		if s.hookDoctor.rows[index].Provider == provider {
			row = &s.hookDoctor.rows[index]
			break
		}
	}
	if row == nil || !row.Actionable() {
		return
	}
	s.hookDoctor.actionProvider = provider
	s.hookDoctor.actionRunning = true
	generation := s.hookDoctor.auditGen.Add(1)
	action := row.ActionLabel()

	s.dispatch(func() {
		fresh, actionErr := service.Install(provider)
		s.applyOnWindow(func() {
			if generation != s.hookDoctor.auditGen.Load() {
				return
			}
			s.hookDoctor.actionRunning = false
			s.hookDoctor.actionProvider = ""
			if actionErr != nil {
				s.hookDoctor.pendingToast = action + " failed: " + actionErr.Error()
				return
			}
			// Merge the fresh row into the audited snapshot.
			for index := range s.hookDoctor.rows {
				if s.hookDoctor.rows[index].Provider == provider {
					s.hookDoctor.rows[index] = fresh
				}
			}
			switch {
			case fresh.State == agenthooks.StateOK:
				s.hookDoctor.pendingToast = fresh.Provider.DisplayName() + " hooks are current"
			default:
				s.hookDoctor.pendingToast = action + " finished: " + hookStateText(fresh.State)
			}
		})
	})
}

// consumeHookDoctorToast returns the pending transient result once.
func (s *Shell) consumeHookDoctorToast(c *ui.Context) {
	if s.hookDoctor.pendingToast == "" {
		return
	}
	c.Toast(s.hookDoctor.pendingToast)
	s.hookDoctor.pendingToast = ""
}

// hookStateTone maps a hook diagnosis state onto the semantic palette.
func hookStateTone(state agenthooks.State) StatusTone {
	switch state {
	case agenthooks.StateOK:
		return ToneSuccess
	case agenthooks.StateOutdated:
		return ToneWarning
	case agenthooks.StateNotInstalled:
		return ToneAttention
	case agenthooks.StateError:
		return ToneError
	default:
		return ToneMuted
	}
}

// hookStateText is the shared user-facing hook state text.
func hookStateText(state agenthooks.State) string {
	switch state {
	case agenthooks.StateOK:
		return "Current"
	case agenthooks.StateNotInstalled:
		return "Not installed"
	case agenthooks.StateOutdated:
		return "Outdated"
	case agenthooks.StateUnsupported:
		return "Unsupported"
	default:
		return string(state)
	}
}
