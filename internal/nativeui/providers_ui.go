package nativeui

import (
	"context"
	"errors"
	"log/slog"
	"sync/atomic"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/agenthooks"
	"github.com/wh-studio/herdr-client/internal/herdr"
	"github.com/wh-studio/herdr-client/internal/history"
)

/**
 * [INPUT]: 依赖 agent.IntegrationHealthService（herdr integration status/install 审计，经 herdr.RunCLI 注入 CLI 传输）与 agenthooks.Manager（直连状态 hook 诊断/安装）、mygo/ui 状态件
 * [OUTPUT]: 对外提供 providersUIState、providerRow、providerFacet/facetIntegration/facetStatusHook、requestProvidersAudit、runProviderAction、consumeProvidersToast、integrationErrorText（审计失败的白话原因，原始错误只进日志）、actionInFlight、actionLabel、healthTone/healthStateText、hookStateTone/hookStateText、statusHookFor
 * [POS]: Settings→Agents 页的统一状态机：每个 Agent 一行（hook 主行 + Herdr integration 子行）。直连 Status hook 是主机制（状态→应用 socket，会话身份→pane.report_agent_session 中继 Herdr）；Herdr 官方 integration 对 hook 已覆盖的 Provider 降级为 fallback 面（hook 健康时仅次要展示，不提供动作；hook 不能履职时才浮出修复动作）。一次审计同时覆盖两面；审计只在路由进入/手动刷新/动作完成后发生，从不随渲染轮询。两个领域包保持分离，只在展示层合流
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// providerFacet names one health facet of a provider card: the Herdr
// runtime integration, or the Shardlane direct status hook.
type providerFacet int

const (
	facetIntegration providerFacet = iota
	facetStatusHook
)

// statusHookFor maps a direct status hook provider onto the provider
// registry identity; ok is false for hook-only extras (Aider) the
// integration registry does not track.
func statusHookFor(id agenthooks.ProviderID) (history.AgentID, bool) {
	switch id {
	case agenthooks.ProviderClaude:
		return history.AgentClaudeCode, true
	case agenthooks.ProviderAntigravity:
		return history.AgentAntigravity, true
	default:
		return "", false
	}
}

// providerRow is one provider's merged presentation card: the Herdr
// integration facet and, where one exists, the direct status hook facet.
// Facet pointers stay nil until the matching audit data arrives.
type providerRow struct {
	provider history.AgentID       // empty for hook-only extras
	hookID   agenthooks.ProviderID // non-empty when a status hook facet exists
	name     string

	integration *agent.ProviderIntegrationHealth
	hook        *agenthooks.Diagnosis
}

// key is the stable identity used to join audits and actions to rows.
func (r providerRow) key() string {
	if r.provider != "" {
		return string(r.provider)
	}
	return string(r.hookID)
}

// hookHealthy reports whether the direct status hook facet currently
// serves the provider end to end (app status feed + session-identity
// relay); the Herdr integration is then demoted to an informational
// fallback row.
func (r providerRow) hookHealthy() bool {
	return r.hook != nil && r.hook.State == agenthooks.StateOK
}

// actionLabel is the facet-scoped safe action text, sourced from the
// domain vocabularies so copy never forks. For hook-covered providers the
// Herdr integration is the fallback lane (降级使用): its repair action only
// surfaces while the hook cannot do the job.
func (r providerRow) actionLabel(facet providerFacet) string {
	switch facet {
	case facetIntegration:
		if r.integration == nil {
			return ""
		}
		if r.hookHealthy() {
			return ""
		}
		if label := r.integration.ActionLabel(); label != "" {
			return label + " integration"
		}
	case facetStatusHook:
		if r.hook != nil {
			if label := r.hook.ActionLabel(); label != "" {
				return label + " status hook"
			}
		}
	}
	return ""
}

// providersUIState is the presentation state of the unified Providers
// settings surface. One audit generation covers both facets; audits never
// run from render.
type providersUIState struct {
	integrations *agent.IntegrationHealthService
	hooks        *agenthooks.Manager

	loaded   bool
	loading  bool
	errText  string
	rows     []providerRow
	auditGen atomic.Uint64
	listCancel context.CancelFunc

	// action tracking: one facet action in flight; its control is disabled
	// with a spinner until the fresh audit reconciles.
	actionRunning bool
	actionFacet   providerFacet
	actionKey     string

	// pendingToast is a one-shot transient result consumed by the page.
	pendingToast string
}

// requestProvidersAudit reconciles both facets off the presentation path:
// one dispatch runs the Herdr integration audit and the local hook
// diagnosis, then applies them atomically as one joined card list.
func (s *Shell) requestProvidersAudit() {
	state := &s.providers
	if state.loading || state.integrations == nil || state.hooks == nil {
		return
	}
	if state.listCancel != nil {
		state.listCancel()
	}
	ctx, cancel := context.WithCancel(context.Background())
	state.listCancel = cancel
	state.loading = true
	state.errText = ""
	generation := state.auditGen.Add(1)

	s.dispatch(func() {
		integrationRows, auditErr := state.integrations.Audit(ctx)
		hookRows := state.hooks.Diagnose()
		s.applyOnWindow(func() {
			if generation != state.auditGen.Load() {
				return
			}
			state.loading = false
			state.loaded = true
			if auditErr != nil {
				slog.Warn("agent integration audit failed", "error", auditErr)
				state.errText = integrationErrorText(auditErr)
			} else {
				state.errText = ""
			}
			state.applyAudit(integrationRows, hookRows)
		})
	})
}

// applyAudit joins the fresh audit rows into the card list: every registry
// provider in registry order, plus hook-only extras (Aider) appended.
func (state *providersUIState) applyAudit(integrationRows []agent.ProviderIntegrationHealth, hookRows []agenthooks.Diagnosis) {
	integrationByKey := make(map[string]agent.ProviderIntegrationHealth, len(integrationRows))
	for _, row := range integrationRows {
		integrationByKey[string(row.Provider)] = row
	}
	hookByKey := make(map[string]agenthooks.Diagnosis, len(hookRows))
	for _, row := range hookRows {
		key := string(row.Provider)
		if mapped, ok := statusHookFor(row.Provider); ok {
			key = string(mapped)
		}
		hookByKey[key] = row
	}

	rows := make([]providerRow, 0, len(history.AllAgents)+1)
	for _, provider := range history.AllAgents {
		row := providerRow{provider: provider, name: provider.DisplayName()}
		if integration, ok := integrationByKey[row.key()]; ok {
			integration := integration
			row.integration = &integration
		}
		if hook, ok := hookByKey[row.key()]; ok {
			hook := hook
			row.hook = &hook
			row.hookID = hook.Provider
		}
		rows = append(rows, row)
	}
	for _, hook := range hookRows {
		if _, ok := statusHookFor(hook.Provider); ok {
			continue
		}
		hook := hook
		rows = append(rows, providerRow{hookID: hook.Provider, name: hook.Provider.DisplayName(), hook: &hook})
	}
	state.rows = rows
}

// actionInFlight reports whether this row facet owns the in-flight action.
func (state *providersUIState) actionInFlight(row providerRow, facet providerFacet) bool {
	return state.actionRunning && state.actionFacet == facet && state.actionKey == row.key()
}

// runProviderAction executes the one safe action for a card facet off the
// presentation path, then reconciles the facet from a fresh audit.
func (s *Shell) runProviderAction(row providerRow, facet providerFacet) {
	state := &s.providers
	if state.actionRunning || state.integrations == nil || state.hooks == nil {
		return
	}
	action := row.actionLabel(facet)
	if action == "" {
		return
	}
	state.actionRunning = true
	state.actionFacet = facet
	state.actionKey = row.key()
	generation := state.auditGen.Add(1)

	s.dispatch(func() {
		switch facet {
		case facetIntegration:
			actionErr := state.integrations.Install(context.Background(), row.provider)
			fresh, auditErr := state.integrations.RefreshProvider(context.Background(), row.provider)
			s.applyOnWindow(func() {
				if generation != state.auditGen.Load() {
					return
				}
				state.actionRunning = false
				if auditErr != nil {
					slog.Warn("agent integration audit failed", "error", auditErr)
					state.errText = integrationErrorText(auditErr)
					return
				}
				// Merge the refreshed facet into the audited snapshot.
				for index := range state.rows {
					if state.rows[index].key() == row.key() {
						fresh := fresh
						state.rows[index].integration = &fresh
						break
					}
				}
				switch {
				case actionErr != nil:
					slog.Warn("agent integration action failed", "action", action, "error", actionErr)
					state.pendingToast = action + " failed. Try again."
				case fresh.State == agent.HealthCurrent:
					state.pendingToast = row.name + " integration is current"
				default:
					state.pendingToast = action + " finished: " + healthStateText(fresh.State)
				}
			})
		case facetStatusHook:
			fresh, actionErr := state.hooks.Install(row.hookID)
			s.applyOnWindow(func() {
				if generation != state.auditGen.Load() {
					return
				}
				state.actionRunning = false
				// Merge the fresh diagnosis into the audited snapshot.
				key := row.key()
				for index := range state.rows {
					if state.rows[index].key() == key {
						fresh := fresh
						state.rows[index].hook = &fresh
						break
					}
				}
				switch {
				case actionErr != nil:
					state.pendingToast = action + " failed: " + actionErr.Error()
				case fresh.State == agenthooks.StateOK:
					state.pendingToast = row.name + " status hook is current"
				default:
					state.pendingToast = action + " finished: " + hookStateText(fresh.State)
				}
			})
		}
	})
}

// integrationErrorText is the plain-language reason an integration audit
// failed. The raw error (an exec path, the CLI's stderr) goes to the log, not
// to the screen.
func integrationErrorText(err error) string {
	switch {
	case errors.Is(err, herdr.ErrCLINotFound):
		return "Couldn't find Herdr. Install it, then try again."
	case errors.Is(err, context.DeadlineExceeded):
		return "Checking took too long. Try again."
	default:
		return "Couldn't read agent status. Try again."
	}
}

// consumeProvidersToast returns the pending transient result once.
func (s *Shell) consumeProvidersToast(c *ui.Context) {
	if s.providers.pendingToast == "" {
		return
	}
	c.Toast(s.providers.pendingToast)
	s.providers.pendingToast = ""
}

// healthTone maps an integration health state onto the semantic palette.
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

// healthStateText is the shared user-facing integration health text.
func healthStateText(state agent.IntegrationHealthState) string {
	switch state {
	case agent.HealthChecking:
		return "Checking"
	case agent.HealthCurrent:
		return "Up to date"
	case agent.HealthOutdated:
		return "Update available"
	case agent.HealthNotInstalled:
		return "Not installed"
	case agent.HealthManagedByHerdr:
		return "Managed by Herdr"
	case agent.HealthManagedBridgeReady:
		return "Ready"
	case agent.HealthDeferred:
		return "Not available yet"
	case agent.HealthUnsupported:
		return "Not supported"
	case agent.HealthError:
		return "Error"
	default:
		return string(state)
	}
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
		return "Up to date"
	case agenthooks.StateNotInstalled:
		return "Not installed"
	case agenthooks.StateOutdated:
		return "Update available"
	case agenthooks.StateUnsupported:
		return "Not supported"
	default:
		return string(state)
	}
}
