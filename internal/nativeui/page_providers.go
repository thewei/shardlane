package nativeui

import (
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/agenthooks"
)

/**
 * [INPUT]: 依赖 agent 集成审计与 agenthooks 直连 hook 诊断（providers_ui 统一状态机）、mygo/ui、ds_components 状态件
 * [OUTPUT]: 对外提供 settingsProviders（Settings→Agents 健康页：单一分组列表，每个 Agent 一行，Integration + Status hook 两个面）
 * [POS]: Providers 健康面；一次审计覆盖两面，且只发生在路由进入/手动刷新/动作完成之后，绝不随渲染轮询
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// settingsProviders is the unified Native Agents health surface: one list
// joining, per agent, the Herdr integration facet and, where one exists, the
// direct status hook facet. The audit runs on route entry, manual refresh and
// after actions — never from render and never on a poll.
func (s *Shell) settingsProviders(c *ui.Context) {
	if !s.providers.loaded && !s.providers.loading && s.providers.errText == "" {
		s.requestProvidersAudit()
	}
	s.consumeProvidersToast(c)

	sp := Spacing()
	t := c.Theme()
	if s.providers.errText != "" {
		inlineNotice(c, ToneError, s.providers.errText, func() {
			s.providers.loading = false
			s.providers.errText = ""
			s.requestProvidersAudit()
		})
	}

	ui.Row(c).FillWidth().Gap(sp.M).AlignItems(ui.Center).Children(func() {
		if s.providers.loading {
			ui.Spinner(c).Size(14, 14)
			ui.Text(c, "Checking…").FontSize(Typography().Caption).TextColor(t.TextMuted)
		}
		ui.Box(c).Grow(1)
		if ui.Button(c, "Refresh").Clicked() {
			s.requestProvidersAudit()
		}
	})

	if s.providers.loaded {
		var rows []settingsRow
		for _, row := range s.providers.rows {
			rows = append(rows, s.providerRows(c, row)...)
		}
		settingsGroup(c, "", rows...)
	} else if s.providers.loading {
		loadingState(c, "Checking…")
	}
}

// providerRows renders one agent as list rows. Hook-covered agents lead with
// our direct status hook; the Herdr integration becomes the fallback row
// below it — muted while the hook is healthy (nothing to repair), actionable
// again only while the hook cannot do the job. Agents without a status hook
// keep the Herdr integration as their single (and only) mechanism.
func (s *Shell) providerRows(c *ui.Context, row providerRow) []settingsRow {
	lead := func() { agentTile(c, row.provider, 24) }
	blank := func() { ui.Box(c).Width(24).Shrink(0) }
	if row.hook != nil {
		hook := row.hook
		rows := []settingsRow{{
			lead: lead, label: row.name, note: hookNote(hook),
			control: func() {
				s.facetControls(c, row, facetStatusHook, hookStateText(hook.State), hookStateTone(hook.State))
			},
		}}
		if row.integration != nil {
			integration := row.integration
			tone := healthTone(integration.State)
			if row.hookHealthy() {
				// The fallback is not load-bearing; present it as
				// information, never as a warning or a nag.
				tone = ToneMuted
			}
			rows = append(rows, settingsRow{
				lead: blank, attached: true, label: "Herdr integration", note: integrationNote(*integration),
				control: func() {
					s.facetControls(c, row, facetIntegration, healthStateText(integration.State), tone)
				},
			})
		}
		return rows
	}
	if row.integration != nil {
		integration := row.integration
		return []settingsRow{{
			lead: lead, label: row.name, note: integrationNote(*integration),
			control: func() {
				s.facetControls(c, row, facetIntegration, healthStateText(integration.State), healthTone(integration.State))
			},
		}}
	}
	// The integration audit failed or has not reconciled this agent; the
	// inline notice above carries the reason.
	return []settingsRow{{
		lead: lead, label: row.name,
		control: func() { s.facetControls(c, row, facetIntegration, "Unknown", ToneMuted) },
	}}
}

// facetControls renders one facet's state pill plus its single safe action
// with the in-flight duplicate-disabled state.
func (s *Shell) facetControls(c *ui.Context, row providerRow, facet providerFacet, stateText string, tone StatusTone) {
	ui.Row(c).Gap(Spacing().M).AlignItems(ui.Center).Children(func() {
		statusPill(c, stateText, tone)
		inFlight := s.providers.actionInFlight(row, facet)
		label := row.actionLabel(facet)
		verb, _, _ := strings.Cut(label, " ")
		switch {
		case inFlight:
			ui.Spinner(c).Size(14, 14)
			ui.Button(c, verb).Label(label).Disabled(true)
		case label != "":
			if ui.Button(c, verb).Label(label).Clicked() {
				s.runProviderAction(row, facet)
			}
		}
	})
}

// hookNote is the short line under an agent's status hook: what the hook
// gives the user, not how it is wired.
func hookNote(hook *agenthooks.Diagnosis) string {
	switch hook.State {
	case agenthooks.StateOK:
		return "Live status is on"
	case agenthooks.StateOutdated:
		return "Update to keep live status working"
	case agenthooks.StateNotInstalled:
		return "Install to show live status"
	case agenthooks.StateError:
		return hook.Detail
	default:
		return "Status comes from Herdr's screen detection"
	}
}

// integrationNote is the version line of a Herdr integration; the mechanism
// behind it stays out of sight.
func integrationNote(row agent.ProviderIntegrationHealth) string {
	if row.Version == "" {
		return ""
	}
	note := row.Version
	if row.LatestVersion != "" {
		note += fmt.Sprintf(" (latest %s)", row.LatestVersion)
	}
	return note
}
