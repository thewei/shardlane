package nativeui

import (
	"context"
	"sync/atomic"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/history"
)

// integrationUIState is the presentation state of the Providers/Integrations
// settings page. Audits never run in render: the page only triggers them on
// route entry, manual refresh and after actions (INT-07).
type integrationUIState struct {
	service *agent.IntegrationHealthService

	loaded     bool
	loading    bool
	errText    string
	rows       []agent.ProviderIntegrationHealth
	auditGen   atomic.Uint64
	listCancel context.CancelFunc

	// action tracking: one provider action in flight, its row disabled with
	// a spinner until the fresh audit reconciles (INT-05).
	actionProvider history.AgentID
	actionRunning  bool

	// pendingToast is a one-shot transient result consumed by the page render.
	pendingToast string
}

func (s *Shell) requestIntegrationAudit() {
	service := s.integrations.service
	if service == nil {
		return
	}
	if s.integrations.listCancel != nil {
		s.integrations.listCancel()
	}
	ctx, cancel := context.WithCancel(context.Background())
	s.integrations.listCancel = cancel
	s.integrations.loading = true
	s.integrations.errText = ""
	generation := s.integrations.auditGen.Add(1)

	s.dispatch(func() {
		rows, err := service.Audit(ctx)
		s.applyOnWindow(func() {
			if generation != s.integrations.auditGen.Load() {
				return
			}
			s.integrations.loading = false
			if err != nil {
				s.integrations.errText = err.Error()
				return
			}
			s.integrations.loaded = true
			s.integrations.rows = rows
		})
	})
}

// runIntegrationAction executes the safe action for one provider off the
// presentation path, then reconciles from a fresh audit (INT-05).
func (s *Shell) runIntegrationAction(provider history.AgentID) {
	service := s.integrations.service
	if service == nil || s.integrations.actionRunning {
		return
	}
	var row *agent.ProviderIntegrationHealth
	for index := range s.integrations.rows {
		if s.integrations.rows[index].Provider == provider {
			row = &s.integrations.rows[index]
			break
		}
	}
	if row == nil || !row.Actionable() {
		return
	}
	s.integrations.actionProvider = provider
	s.integrations.actionRunning = true
	generation := s.integrations.auditGen.Add(1)
	action := row.ActionLabel()

	s.dispatch(func() {
		actionErr := service.Install(context.Background(), provider)
		fresh, auditErr := service.RefreshProvider(context.Background(), provider)
		s.applyOnWindow(func() {
			if generation != s.integrations.auditGen.Load() {
				return
			}
			s.integrations.actionRunning = false
			s.integrations.actionProvider = ""
			if auditErr != nil {
				s.integrations.errText = auditErr.Error()
				return
			}
			// Merge the refreshed row into the audited snapshot.
			for index := range s.integrations.rows {
				if s.integrations.rows[index].Provider == provider {
					s.integrations.rows[index] = fresh
				}
			}
			switch {
			case actionErr != nil:
				s.integrations.pendingToast = action + " failed: " + actionErr.Error()
			case fresh.State == agent.HealthCurrent:
				s.integrations.pendingToast = provider.DisplayName() + " integration is current"
			default:
				s.integrations.pendingToast = action + " finished: " + healthStateText(fresh.State)
			}
		})
	})
}

// consumeIntegrationToast returns the pending transient result once.
func (s *Shell) consumeIntegrationToast(c *ui.Context) {
	if s.integrations.pendingToast == "" {
		return
	}
	c.Toast(s.integrations.pendingToast)
	s.integrations.pendingToast = ""
}

// healthTone maps a health state onto the semantic status palette.
func healthTone(state agent.IntegrationHealthState) StatusTone {
	switch state {
	case agent.HealthCurrent, agent.HealthManagedBridgeReady:
		return ToneSuccess
	case agent.HealthOutdated:
		return ToneWarning
	case agent.HealthNotInstalled:
		return ToneAttention
	case agent.HealthManagedByHerdr:
		return ToneInfo
	case agent.HealthError:
		return ToneError
	case agent.HealthChecking:
		return ToneWorking
	default:
		return ToneMuted
	}
}

// healthStateText is the shared user-facing health text.
func healthStateText(state agent.IntegrationHealthState) string {
	switch state {
	case agent.HealthChecking:
		return "Checking"
	case agent.HealthCurrent:
		return "Current"
	case agent.HealthOutdated:
		return "Outdated"
	case agent.HealthNotInstalled:
		return "Not installed"
	case agent.HealthManagedByHerdr:
		return "Managed by Herdr"
	case agent.HealthManagedBridgeReady:
		return "Managed bridge ready"
	case agent.HealthDeferred:
		return "Deferred"
	case agent.HealthUnsupported:
		return "Unsupported"
	case agent.HealthError:
		return "Error"
	default:
		return string(state)
	}
}
