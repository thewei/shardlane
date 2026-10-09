package nativeui

import (
	"fmt"
	"os"
	"strings"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/applog"
	"github.com/wh-studio/herdr-client/internal/diagnostics"
	"github.com/wh-studio/herdr-client/internal/settings"
)

var appearanceOptions = []string{"System", "Light", "Dark"}

func appearanceIndex(value string) int {
	switch value {
	case "light":
		return 1
	case "dark":
		return 2
	default:
		return 0
	}
}

func appearanceFromIndex(index int) string {
	if index < 0 || index >= len(appearanceOptions) {
		return "system"
	}
	return strings.ToLower(appearanceOptions[index])
}

// settingsSections is the Settings inner-page navigation; it renders in the
// app sidebar while the section content owns the route area.
var settingsSections = []struct {
	path  string
	label string
	icon  *ui.SVG
}{
	{"/settings/general", "General", iconSettings},
	{"/settings/terminal", "Terminal", iconTerminal},
	{"/settings/providers", "Providers", iconAgent},
	{"/settings/git", "Git & AI", iconBranch},
	{"/settings/runtime", "Runtime", iconWorkspace},
	{"/settings/diagnostics", "Diagnostics", iconSearch},
}

// settingsNav is the Settings sidebar body: the section navigation that used
// to be the page's own inner column (inner-page style).
func (s *Shell) settingsNav(c *ui.Context) {
	ui.Text(c, "Settings").FontSize(13).Bold().Padding(6, 8, 8)
	for _, section := range settingsSections {
		section := section
		selected := s.router.Path() == section.path
		if navButton(c, section.icon, section.label, "", selected).Clicked() && !selected {
			s.router.Push(section.path)
		}
	}
}

// settingsPage is the one layout owner for Settings. The heading remains
// fixed outside the scroll viewport; each section renders only controls.
const settingsContentMaxWidth float32 = 920

func settingsPage(c *ui.Context, title, subtitle string, body func()) {
	ui.Column(c).Grow(1).MinWidth(0).Children(func() {
		pageHeader(c, title, subtitle)
		ui.Scroll(c).Grow(1).MinHeight(0).Children(func() {
			ui.Column(c).FillWidth().MaxWidth(settingsContentMaxWidth).
				Padding(Spacing().XL).Gap(Spacing().L).Children(body)
		})
	})
}

func (s *Shell) settingsLayout(c *ui.Context, r *ui.Route) {
	// The sheet is gone (2026-10-06): the page rides in a gorex card on
	// the window gradient.
	ui.Column(c).Grow(1).MinWidth(0).Children(func() {
		r.View(c, func(r *ui.Route) {
			switch {
			case r.Match("/general") || r.Match("/"):
				r.Title("Settings · General")
				settingsPage(c, "General", "Appearance, layout and window preferences.", func() { s.settingsGeneral(c) })
			case r.Match("/terminal"):
				r.Title("Settings · Terminal")
				settingsPage(c, "Terminal", "Native Terminal presentation for attached Herdr Panes.", func() { s.settingsTerminal(c) })
			case r.Match("/providers"):
				r.Title("Settings · Providers")
				settingsPage(c, "Providers", "Integration health and supported provider actions.", func() { s.settingsProviders(c) })
			case r.Match("/git"):
				r.Title("Settings · Git & AI")
				settingsPage(c, "Git & AI", "Commit rules, Pi Agent and safe one-shot message generation.", func() { s.settingsGit(c) })
			case r.Match("/runtime"):
				r.Title("Settings · Runtime")
				settingsPage(c, "Runtime", "Current Herdr session and application runtime facts.", func() { s.settingsRuntime(c) })
			case r.Match("/diagnostics"):
				r.Title("Settings · Diagnostics & Logs")
				settingsPage(c, "Diagnostics & Logs", "Local diagnostic facts and sanitized export.", func() { s.settingsDiagnostics(c) })
			default:
				r.Title("Settings · General")
				// Unknown sections stay usable through the canonical fallback.
				settingsPage(c, "General", "Appearance, layout and window preferences.", func() { s.settingsGeneral(c) })
			}
		})
	})
}

// settingsGeneral/terminal/runtime compose the shared settingsCard primitive
// with the official ui.Form/Field pair (DS-03/DS-05).
func (s *Shell) settingsGeneral(c *ui.Context) {
	ui.Column(c).FillWidth().Gap(Spacing().L).Children(func() {
		settingsCard(c, "Appearance", func() {
			ui.Form(c, func() {
				ui.Field(c, "Theme", func() {
					chosen := appearanceIndex(s.settings.General.Appearance)
					if ui.Segmented(c, &chosen, appearanceOptions...).Changed() {
						s.applySettings(func(v *settings.Settings) error {
							v.General.Appearance = appearanceFromIndex(chosen)
							return nil
						})
					}
				}).Description("Applies to the native shell chrome.")

				ui.Field(c, "Density", func() {
					densityOpts := []string{"Compact", "Default", "Comfortable"}
					idx := 1
					switch s.settings.General.Density {
					case "compact":
						idx = 0
					case "comfortable":
						idx = 2
					}
					if ui.Segmented(c, &idx, densityOpts...).Changed() {
						s.applySettings(func(v *settings.Settings) error {
							switch idx {
							case 0:
								v.General.Density = "compact"
							case 2:
								v.General.Density = "comfortable"
							default:
								v.General.Density = "default"
							}
							return nil
						})
					}
				}).Description("Controls row height and spacing across the sidebar.")

				ui.Field(c, "High contrast", func() {
					hc := s.settings.General.HighContrast
					if ui.Checkbox(c, &hc, "Enhanced contrast").Changed() {
						s.applySettings(func(v *settings.Settings) error {
							v.General.HighContrast = hc
							return nil
						})
					}
				}).Description("Increases contrast of borders, dividers, and secondary text.")
			})
		})
		settingsCard(c, "Window", func() {
			ui.Form(c, func() {
				ui.Field(c, "Restore window state", func() {
					restore := s.settings.General.RestoreWindow
					if ui.Checkbox(c, &restore, "Enabled").Changed() {
						s.applySettings(func(v *settings.Settings) error {
							v.General.RestoreWindow = restore
							return nil
						})
					}
				}).Description("Reopen the window where it was last placed, when that still intersects a display.")
			})
		})
	})
}

func (s *Shell) settingsTerminal(c *ui.Context) {
	ui.Column(c).FillWidth().Gap(Spacing().L).Children(func() {
		settingsCard(c, "Presentation", func() {
			ui.Text(c, "Changes apply to newly attached Herdr Panes. Herdr keeps terminal identity, scrollback semantics and process ownership.").FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)

			ui.Form(c, func() {
				ui.Field(c, "Font family", func() {
					field := ui.TextInput(c, &s.settingsFontDraft).Width(220).Label("Font family")
					if field.Submitted() {
						s.applySettings(func(v *settings.Settings) error {
							v.Terminal.FontFamily = strings.TrimSpace(s.settingsFontDraft)
							return nil
						})
					}
					if !field.Focused() {
						s.settingsFontDraft = s.settings.Terminal.FontFamily
					}
				}).Description("Commit with Return.")
				ui.Field(c, "Font size", func() {
					size := s.settings.Terminal.FontSize
					if ui.NumberInput(c, &size, 6, 32, 1).Changed() {
						s.applySettings(func(v *settings.Settings) error {
							v.Terminal.FontSize = size
							return nil
						})
					}
				}).Description("Points.")
				ui.Field(c, "Line height", func() {
					height := s.settings.Terminal.LineHeight
					if ui.NumberInput(c, &height, 1.0, 2.5, 0.02).Changed() {
						s.applySettings(func(v *settings.Settings) error {
							v.Terminal.LineHeight = height
							return nil
						})
					}
				}).Description("Multiplier.")
				ui.Field(c, "Scrollback", func() {
					megabytes := float64(s.settings.Terminal.Scrollback) / (1024 * 1024)
					if ui.NumberInput(c, &megabytes, 1, 1024, 1).Changed() {
						s.applySettings(func(v *settings.Settings) error {
							v.Terminal.Scrollback = int(megabytes) * 1024 * 1024
							return nil
						})
					}
				}).Description("Megabytes kept by the native terminal view.")
				ui.Field(c, "Option as Alt", func() {
					alt := s.settings.Terminal.OptionAsAlt
					if ui.Checkbox(c, &alt, "Enabled").Changed() {
						s.applySettings(func(v *settings.Settings) error {
							v.Terminal.OptionAsAlt = alt
							return nil
						})
					}
				}).Description("Send Option-key combinations as Alt in the terminal.")
				ui.Field(c, "Transparent background", func() {
					transparent := s.settings.Terminal.Transparent
					if ui.Checkbox(c, &transparent, "Enabled").Changed() {
						s.applySettings(func(v *settings.Settings) error {
							v.Terminal.Transparent = transparent
							return nil
						})
					}
				}).Description("Let the window gradient show through the terminal paper. Applies to newly attached Panes.")
			})
		})
	})
}

func (s *Shell) settingsRuntime(c *ui.Context) {
	t := c.Theme()
	ui.Column(c).FillWidth().Gap(Spacing().L).Children(func() {
		ui.Text(c, fmt.Sprintf("Workspace: %s", fallbackText(s.activeInstance, "none")))
		ui.Text(c, fmt.Sprintf("Herdr protocol: %d", s.projection.Protocol)).TextColor(t.TextMuted)
		ui.Text(c, fmt.Sprintf("Projects: %d · Tabs: %d · Panes: %d · Agents: %d", len(s.projection.Projects), len(s.projection.Tabs), len(s.projection.Panes), len(s.projection.Agents))).TextColor(t.TextMuted)
		panelCard(c, func() {
			ui.Text(c, "Diagnostics").FontWeight(650)
			ui.Text(c, fmt.Sprintf("Log cap: %d MB × %d files", applog.DefaultMaxBytes/(1024*1024), applog.DefaultBackups+1)).FontSize(10).TextColor(t.TextMuted)
			logPath := applog.Path()
			if logPath == "" {
				ui.Text(c, "File logging is not initialized.").FontSize(10).TextColor(t.TextMuted)
			} else {
				ui.Text(c, logPath).FontSize(10).TextColor(t.TextMuted).SingleLine()
				if ui.Button(c, "Show Log in Finder").Clicked() {
					mygo.Shell.ShowItemInFolder(logPath)
				}
			}
		})
		if ui.Button(c, "Refresh Runtime").Clicked() {
			s.reloadInstances(false)
		}
	})
}

// settingsDiagnostics renders the P16 Diagnostics snapshot & P17 Sanitized export.
func (s *Shell) settingsDiagnostics(c *ui.Context) {
	t := c.Theme()
	sp := Spacing()
	home, _ := os.UserHomeDir()
	snap := diagnostics.BuildSnapshot(s.projection.Version, s.projection.Protocol, s.activeInstance, home)

	ui.Column(c).FillWidth().Gap(sp.L).Children(func() {

		// System snapshot card
		settingsCard(c, "System & Runtime Facts", func() {
			ui.Form(c, func() {
				ui.Field(c, "Version", func() {
					ui.Text(c, fmt.Sprintf("Shardlane %s (Go %s / MyGo %s)", snap.ShardlaneVersion, snap.GoVersion, snap.MyGoVersion)).TextColor(t.TextMuted)
				})
				ui.Field(c, "Environment", func() {
					ui.Text(c, fmt.Sprintf("%s / %s", snap.OS, snap.Arch)).TextColor(t.TextMuted)
				})
				ui.Field(c, "Herdr Protocol", func() {
					ui.Text(c, fmt.Sprintf("Protocol %d (Version %s)", snap.HerdrProtocol, fallbackText(snap.HerdrVersion, "connected"))).TextColor(t.TextMuted)
				})
				ui.Field(c, "Active Workspace", func() {
					ui.Text(c, fallbackText(snap.ActiveInstance, "none")).TextColor(t.TextMuted)
				})
				ui.Field(c, "Active Log Path", func() {
					ui.Text(c, fallbackText(snap.LogPath, "in-memory")).TextColor(t.TextMuted).SingleLine()
				})
			})

			ui.Row(c).Padding(sp.M, 0, 0).Gap(sp.S).AlignItems(ui.Center).Children(func() {
				if ui.Button(c, "Export Sanitized Diagnostics").Clicked() {
					logs, _ := diagnostics.ReadRecentLogs(500)
					exported, err := diagnostics.BuildExportArchive(snap, logs, home)
					if err == nil {
						s.copyToClipboard(exported)
						s.diagExportStatus = "Sanitized diagnostics copied to clipboard (all secrets redacted)."
					} else {
						s.diagExportStatus = "Export failed: " + err.Error()
					}
				}
				if s.diagExportStatus != "" {
					ui.Text(c, s.diagExportStatus).FontSize(Typography().Caption).TextColor(t.TextMuted)
				}
			})
		})

		// Log Viewer card (WIX-210)
		settingsCard(c, "Application Logs (Recent 5,000)", func() {
			rawLogs, _ := diagnostics.ReadRecentLogs(5000)
			filtered := diagnostics.FilterLogs(rawLogs, s.diagLogLevelFilter, s.diagSearchQuery)

			ui.Row(c).FillWidth().Gap(sp.S).AlignItems(ui.Center).Children(func() {
				levelOpts := []string{"ALL", "INFO", "WARN", "ERROR", "DEBUG"}
				levelIdx := 0
				for i, lvl := range levelOpts {
					if lvl == s.diagLogLevelFilter {
						levelIdx = i
						break
					}
				}
				if ui.Segmented(c, &levelIdx, levelOpts...).Changed() {
					s.diagLogLevelFilter = levelOpts[levelIdx]
				}

				ui.Box(c).Grow(1)
				ui.Text(c, fmt.Sprintf("%d of %d entries", len(filtered), len(rawLogs))).
					FontSize(Typography().Caption).TextColor(t.TextMuted)
			})

			ui.Scroll(c).Height(300).Background(designTokens(t.Dark).Content).
				Radius(Radius().Control).Padding(sp.S).Children(func() {
				if len(filtered) == 0 {
					ui.Text(c, "No log entries match the active filter.").FontSize(Typography().Caption).TextColor(t.TextMuted)
					return
				}
				ui.Column(c).FillWidth().Gap(2).Children(func() {
					displayLimit := len(filtered)
					if displayLimit > 100 {
						displayLimit = 100
					}
					for i := 0; i < displayLimit; i++ {
						entry := filtered[i]
						ui.Row(c).FillWidth().Gap(sp.S).AlignItems(ui.Center).Children(func() {
							tone := ToneMuted
							switch entry.Level {
							case "ERROR":
								tone = ToneError
							case "WARN":
								tone = ToneWarning
							case "INFO":
								tone = ToneInfo
							}
							statusPill(c, entry.Level, tone)
							msgText := entry.Message
							if msgText == "" {
								msgText = entry.Raw
							}
							ui.Text(c, msgText).FontSize(Typography().Micro).SingleLine().Grow(1)
						})
					}
				})
			})
		})
	})
}
