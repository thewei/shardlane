package nativeui

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
)

/**
 * [INPUT]: 依赖 agent 集成审计与 agenthooks 直连 hook 诊断（providers_ui 统一状态机）、mygo/ui、ds_components 状态件
 * [OUTPUT]: 对外提供 settingsProviders（Settings→Providers 健康页：每个 Provider 一张卡，Integration + Status hook 两个面）
 * [POS]: Providers 健康面；一次审计覆盖两面，且只发生在路由进入/手动刷新/动作完成之后，绝不随渲染轮询
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// settingsProviders is the unified Native Providers health surface: one
// card per provider joining the Herdr integration facet and, where one
// exists, the direct status hook facet. The audit runs on route entry,
// manual refresh and after actions — never from render and never on a poll.
func (s *Shell) settingsProviders(c *ui.Context) {
	if !s.providers.loaded && !s.providers.loading && s.providers.errText == "" {
		s.requestProvidersAudit()
	}
	s.consumeProvidersToast(c)

	sp := Spacing()
	ui.Column(c).FillWidth().Gap(sp.L).Children(func() {
		if s.providers.errText != "" {
			inlineNotice(c, ToneError, s.providers.errText, func() {
				s.providers.loading = false
				s.providers.errText = ""
				s.requestProvidersAudit()
			})
		}

		ui.Row(c).Gap(sp.M).AlignItems(ui.Center).Children(func() {
			if ui.Button(c, "Refresh").Clicked() {
				s.requestProvidersAudit()
			}
			if s.providers.loading {
				ui.Spinner(c).Size(14, 14)
				ui.Text(c, "Checking providers…").FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
			}
		})

		if s.providers.loaded {
			for _, row := range s.providers.rows {
				row := row
				s.providerCard(c, row)
			}
		} else if s.providers.loading {
			loadingState(c, "Checking providers…")
		}
	})
}

// providerCard renders one provider's merged card. Hook-covered providers
// lead with our direct status hook; the Herdr integration becomes the
// fallback lane below it — muted while the hook is healthy (nothing to
// repair), actionable again only while the hook cannot do the job.
// Providers without a status hook keep the Herdr integration as the
// single (and only) mechanism.
func (s *Shell) providerCard(c *ui.Context, row providerRow) {
	settingsCard(c, "", func() {
		if row.hook != nil {
			hook := row.hook
			formRow(c, row.name, hook.Detail, func() {
				s.facetControls(c, row, facetStatusHook, hookStateText(hook.State), hookStateTone(hook.State))
			})
			if row.integration != nil {
				integration := row.integration
				tone := healthTone(integration.State)
				if row.hookHealthy() {
					// The fallback is not load-bearing; present it as
					// information, never as a warning or a nag.
					tone = ToneMuted
				}
				formRow(c, "Herdr integration (fallback)", integrationRowDetail(*integration), func() {
					s.facetControls(c, row, facetIntegration, healthStateText(integration.State), tone)
				})
			}
			return
		}
		if row.integration != nil {
			integration := row.integration
			formRow(c, row.name, integrationRowDetail(*integration), func() {
				s.facetControls(c, row, facetIntegration, healthStateText(integration.State), healthTone(integration.State))
			})
		} else {
			// The integration audit failed or has not reconciled this
			// provider; the inline notice above carries the reason.
			formRow(c, row.name, "", func() {
				s.facetControls(c, row, facetIntegration, "Unknown", ToneMuted)
			})
		}
	})
}

// facetControls renders one facet's state pill plus its single safe action
// with the in-flight duplicate-disabled state.
func (s *Shell) facetControls(c *ui.Context, row providerRow, facet providerFacet, stateText string, tone StatusTone) {
	ui.Row(c).Gap(Spacing().M).AlignItems(ui.Center).Children(func() {
		statusPill(c, stateText, tone)
		inFlight := s.providers.actionInFlight(row, facet)
		label := row.actionLabel(facet)
		switch {
		case inFlight:
			ui.Spinner(c).Size(14, 14)
			ui.Button(c, label).Disabled(true)
		case label != "":
			if ui.Button(c, label).Clicked() {
				s.runProviderAction(row, facet)
			}
		}
	})
}

func integrationRowDetail(row agent.ProviderIntegrationHealth) string {
	// The state pill already speaks the deferred/managed state (2026-10-06
	// round five): repeating it as the subtitle read as a rendering bug.
	detail := agent.StrategyLabel(row.Strategy)
	if detail == healthStateText(row.State) {
		detail = ""
	}
	if row.Version != "" {
		if detail != "" {
			detail += " · "
		}
		detail += row.Version
		if row.LatestVersion != "" {
			detail += fmt.Sprintf(" (latest %s)", row.LatestVersion)
		}
	}
	if row.Path != "" {
		if detail != "" {
			detail += " · "
		}
		detail += row.Path
	}
	return detail
}
