package nativeui

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/agenthooks"
)

/**
 * [INPUT]: 依赖 agent 集成审计与 agenthooks Hook Doctor、mygo/ui、ds_components 状态件
 * [OUTPUT]: 对外提供 settingsProviders（Settings→Providers 健康页：Herdr 集成审计 + Agent Hooks 检查安装）
 * [POS]: Providers/Integrations 健康面；审计只发生在路由进入/手动刷新/动作完成之后，绝不随渲染轮询
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// settingsProviders is the Native Providers/Integrations health surface
// (INT-04/05/07). Audits run on route entry, manual refresh and after
// actions — never from render and never on a poll.
func (s *Shell) settingsProviders(c *ui.Context) {
	if !s.integrations.loaded && !s.integrations.loading && s.integrations.errText == "" {
		s.requestIntegrationAudit()
	}
	s.consumeIntegrationToast(c)

	sp := Spacing()
	ui.Column(c).FillWidth().Gap(sp.L).Children(func() {

		if s.integrations.errText != "" {
			inlineNotice(c, ToneError, s.integrations.errText, func() {
				s.integrations.loading = false
				s.integrations.errText = ""
				s.requestIntegrationAudit()
			})
		}

		ui.Row(c).Gap(sp.M).AlignItems(ui.Center).Children(func() {
			if ui.Button(c, "Refresh Integration Health").Clicked() {
				s.requestIntegrationAudit()
			}
			if s.integrations.loading {
				ui.Spinner(c).Size(14, 14)
				ui.Text(c, "Checking integrations…").FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
			}
		})

		if s.integrations.loaded {
			for _, row := range s.integrations.rows {
				row := row
				s.integrationRow(c, row)
			}
		} else if s.integrations.loading {
			loadingState(c, "Checking provider integrations…")
		}

		s.hookDoctorSection(c)
	})
}

// hookDoctorSection renders the direct-connect Agent Hook audit/install
// surface: one row per provider with its script/wiring diagnosis and the
// single safe install/update action. Same audit discipline as the
// integration rows above: entry, manual refresh, after-action — never poll.
func (s *Shell) hookDoctorSection(c *ui.Context) {
	sp := Spacing()
	ui.Text(c, "Agent Hooks").FontSize(Typography().Title).FontWeight(600)
	ui.Text(c, "Detect and install the lightweight status hooks that report Claude Code and Antigravity activity into this app").
		FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)

	ui.Row(c).Gap(sp.M).AlignItems(ui.Center).Children(func() {
		if ui.Button(c, "Check Agent Hooks").Clicked() {
			s.requestHookDoctorAudit()
		}
		if s.hookDoctor.loading {
			ui.Spinner(c).Size(14, 14)
			ui.Text(c, "Checking hook scripts…").FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
		}
	})

	if s.hookDoctor.errText != "" {
		inlineNotice(c, ToneError, s.hookDoctor.errText, func() {
			s.hookDoctor.errText = ""
			s.requestHookDoctorAudit()
		})
	}
	if !s.hookDoctor.loaded {
		if !s.hookDoctor.loading && s.hookDoctor.errText == "" {
			s.requestHookDoctorAudit()
		}
		return
	}
	for _, row := range s.hookDoctor.rows {
		row := row
		s.hookDoctorRow(c, row)
	}
}

// hookDoctorRow renders one provider's hook diagnosis: state pill, detail,
// and the safe action with its in-flight duplicate-disabled state.
func (s *Shell) hookDoctorRow(c *ui.Context, row agenthooks.Diagnosis) {
	settingsCard(c, "", func() {
		formRow(c, row.Provider.DisplayName(), row.Detail, func() {
			ui.Row(c).Gap(Spacing().M).AlignItems(ui.Center).Children(func() {
				statusPill(c, hookStateText(row.State), hookStateTone(row.State))
				inFlight := s.hookDoctor.actionRunning && s.hookDoctor.actionProvider == row.Provider
				switch {
				case inFlight:
					ui.Spinner(c).Size(14, 14)
					ui.Button(c, row.ActionLabel()).Disabled(true)
				case row.Actionable():
					if ui.Button(c, row.ActionLabel()).Clicked() {
						s.runHookDoctorAction(row.Provider)
					}
				}
			})
		})
	})
}

// integrationRow renders one provider row: strategy, health, detail, and the
// single safe action with its in-flight duplicate-disabled state.
func (s *Shell) integrationRow(c *ui.Context, row agent.ProviderIntegrationHealth) {
	settingsCard(c, "", func() {
		formRow(c, row.Provider.DisplayName(), integrationRowDetail(row), func() {
			ui.Row(c).Gap(Spacing().M).AlignItems(ui.Center).Children(func() {
				statusPill(c, healthStateText(row.State), healthTone(row.State))
				inFlight := s.integrations.actionRunning && s.integrations.actionProvider == row.Provider
				switch {
				case inFlight:
					ui.Spinner(c).Size(14, 14)
					ui.Button(c, row.ActionLabel()).Disabled(true)
				case row.Actionable():
					label := row.ActionLabel()
					if ui.Button(c, label).Clicked() {
						s.runIntegrationAction(row.Provider)
					}
				}
			})
		})
		if row.Detail != "" {
			ui.Text(c, row.Detail).FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
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
