package nativeui

import (
	"fmt"
	"math"
	"os"
	"strings"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/diagnostics"
	"github.com/wh-studio/herdr-client/internal/settings"
)

/**
 * [INPUT]: 依赖 github.com/egoist/mygo, github.com/egoist/mygo/ui, settings, diagnostics
 * [OUTPUT]: 对外提供 Shell.settingsLayout/settingsGeneral/settingsTerminal/settingsDiagnostics
 * [POS]: nativeui 设置页面宿主与分组行容器（settingsGroup），呈现并持久化用户外观与运行时偏好
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

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

// settingsSection is one entry of the Settings navigation.
type settingsSection struct {
	path  string
	label string
	icon  *ui.SVG
	tone  TileTone
}

// settingsSections is the Settings inner-page navigation; it renders in the
// app sidebar while the section content owns the route area. The title bar
// names the open section from the same table (route_header.go).
var settingsSections = []settingsSection{
	{"/settings/general", "General", iconSettings, TileGray},
	{"/settings/terminal", "Terminal", iconTerminal, TileGreen},
	{"/settings/providers", "Agents", iconAgent, TilePurple},
	{"/settings/git", "Git", iconBranch, TileOrange},
	{"/settings/diagnostics", "Diagnostics", iconInfo, TileBlue},
}

// settingsNav is the Settings sidebar body: the section navigation that used
// to be the page's own inner column (inner-page style). The title bar names
// the page, so the list starts at its first section.
func (s *Shell) settingsNav(c *ui.Context) {
	for _, section := range settingsSections {
		section := section
		selected := s.router.Path() == section.path
		if navTileButton(c, section.icon, section.tone, section.label, selected).Clicked() && !selected {
			s.router.Push(section.path)
		}
	}
}

// settingsPage is the one layout owner for Settings. The title bar names the
// open section, so the page is the scroll viewport and each section renders
// only controls.
const settingsContentMaxWidth float32 = 760

func settingsPage(c *ui.Context, body func()) {
	ui.Column(c).Grow(1).MinWidth(0).Children(func() {
		ui.Scroll(c).Grow(1).MinHeight(0).Children(func() {
			contentColumn(c, formColumnWidth, func() {
				ui.Column(c).FillWidth().Padding(pageTopInset(), Spacing().XL, Spacing().XL).Gap(Spacing().XL).Children(body)
			})
		})
	})
}

func (s *Shell) settingsLayout(c *ui.Context, r *ui.Route) {
	ui.Column(c).Grow(1).MinWidth(0).Children(func() {
		r.View(c, func(r *ui.Route) {
			switch {
			case r.Match("/general") || r.Match("/"):
				r.Title("Settings · General")
				settingsPage(c, func() { s.settingsGeneral(c) })
			case r.Match("/terminal"):
				r.Title("Settings · Terminal")
				settingsPage(c, func() { s.settingsTerminal(c) })
			case r.Match("/providers"):
				r.Title("Settings · Agents")
				settingsPage(c, func() { s.settingsProviders(c) })
			case r.Match("/git"):
				r.Title("Settings · Git")
				settingsPage(c, func() { s.settingsGit(c) })
			case r.Match("/diagnostics"):
				r.Title("Settings · Diagnostics")
				settingsPage(c, func() { s.settingsDiagnostics(c) })
			default:
				r.Title("Settings · General")
				// Unknown sections stay usable through the canonical fallback.
				settingsPage(c, func() { s.settingsGeneral(c) })
			}
		})
	})
}

// settingSwitch is a labeled on/off preference: it reports whether the user
// flipped it this frame and the new value.
func settingSwitch(c *ui.Context, label string, on bool) (bool, bool) {
	value := on
	changed := ui.Switch(c, &value).Label(label).Changed()
	return value, changed
}

// settingText is a labeled text preference committed with Return; while it
// is not being edited it mirrors the stored value.
func (s *Shell) settingText(c *ui.Context, label, placeholder string, draft *string, stored string, password bool, commit func(v *settings.Settings, text string)) {
	field := ui.TextInput(c, draft).Width(240).Label(label).Placeholder(placeholder)
	if password {
		field.Password()
	}
	if field.Submitted() {
		text := strings.TrimSpace(*draft)
		s.applySettings(func(v *settings.Settings) error {
			commit(v, text)
			return nil
		})
	}
	if !field.Focused() {
		*draft = stored
	}
}

func (s *Shell) settingsGeneral(c *ui.Context) {
	t := c.Theme()
	settingsGroup(c, "Appearance",
		row("Theme", "", func() {
			chosen := appearanceIndex(s.settings.General.Appearance)
			if ui.Segmented(c, &chosen, appearanceOptions...).Label("Theme").Changed() {
				s.applySettings(func(v *settings.Settings) error {
					v.General.Appearance = appearanceFromIndex(chosen)
					return nil
				})
			}
		}),
		row("Sidebar density", "", func() {
			densityOpts := []string{"Compact", "Default", "Comfortable"}
			idx := 1
			switch s.settings.General.Density {
			case "compact":
				idx = 0
			case "comfortable":
				idx = 2
			}
			if ui.Segmented(c, &idx, densityOpts...).Label("Sidebar density").Changed() {
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
		}),
		row("Increase contrast", "", func() {
			if hc, changed := settingSwitch(c, "Increase contrast", s.settings.General.HighContrast); changed {
				s.applySettings(func(v *settings.Settings) error {
					v.General.HighContrast = hc
					return nil
				})
			}
		}),
		row("Window opacity", "", func() {
			pct := math.Round(s.settings.General.EffectiveOpacity() * 100)
			ui.Row(c).AlignItems(ui.Center).Gap(Spacing().M).Children(func() {
				if ui.Slider(c, &pct, 50, 100).Step(1).Width(160).Label("Window opacity").Changed() {
					s.applySettings(func(v *settings.Settings) error {
						v.General.Opacity = pct / 100
						return nil
					})
				}
				ui.Text(c, fmt.Sprintf("%d%%", int(pct))).Width(36).FontSize(Typography().Body).TextColor(t.TextMuted)
			})
		}),
	)
	settingsGroup(c, "Window",
		row("Restore window position", "", func() {
			if restore, changed := settingSwitch(c, "Restore window position", s.settings.General.RestoreWindow); changed {
				s.applySettings(func(v *settings.Settings) error {
					v.General.RestoreWindow = restore
					return nil
				})
			}
		}),
	)
	settingsGroup(c, "Voice input",
		row("Endpoint", "", func() {
			s.settingText(c, "Endpoint", "https://api.openai.com/v1", &s.settingsVoiceEndpointDraft, s.settings.Voice.EndpointURL, false,
				func(v *settings.Settings, text string) { v.Voice.EndpointURL = text })
		}),
		row("API key", "", func() {
			s.settingText(c, "API key", "Uses OPENAI_API_KEY", &s.settingsVoiceKeyDraft, s.settings.Voice.APIKey, true,
				func(v *settings.Settings, text string) { v.Voice.APIKey = text })
		}),
		row("Model", "", func() {
			s.settingText(c, "Model", "whisper-1", &s.settingsVoiceModelDraft, s.settings.Voice.Model, false,
				func(v *settings.Settings, text string) { v.Voice.Model = text })
		}),
	)
}

func (s *Shell) settingsTerminal(c *ui.Context) {
	settingsGroup(c, "Font",
		row("Font family", "", func() {
			s.settingText(c, "Font family", "", &s.settingsFontDraft, s.settings.Terminal.FontFamily, false,
				func(v *settings.Settings, text string) { v.Terminal.FontFamily = text })
		}),
		row("Font size", "", func() {
			size := s.settings.Terminal.FontSize
			if ui.NumberInput(c, &size, 6, 32, 1).Label("Font size").Changed() {
				s.applySettings(func(v *settings.Settings) error {
					v.Terminal.FontSize = size
					return nil
				})
			}
		}),
		row("Line height", "", func() {
			height := s.settings.Terminal.LineHeight
			if ui.NumberInput(c, &height, 1.0, 2.5, 0.02).Label("Line height").Changed() {
				s.applySettings(func(v *settings.Settings) error {
					v.Terminal.LineHeight = height
					return nil
				})
			}
		}),
	)
	settingsGroup(c, "Behavior",
		row("Scrollback (MB)", "", func() {
			megabytes := float64(s.settings.Terminal.Scrollback) / (1024 * 1024)
			if ui.NumberInput(c, &megabytes, 1, 1024, 1).Label("Scrollback").Changed() {
				s.applySettings(func(v *settings.Settings) error {
					v.Terminal.Scrollback = int(megabytes) * 1024 * 1024
					return nil
				})
			}
		}),
		row("Use Option as Alt", "", func() {
			if alt, changed := settingSwitch(c, "Use Option as Alt", s.settings.Terminal.OptionAsAlt); changed {
				s.applySettings(func(v *settings.Settings) error {
					v.Terminal.OptionAsAlt = alt
					return nil
				})
			}
		}),
		row("Transparent background", "", func() {
			if transparent, changed := settingSwitch(c, "Transparent background", s.settings.Terminal.Transparent); changed {
				s.applySettings(func(v *settings.Settings) error {
					v.Terminal.Transparent = transparent
					return nil
				})
			}
		}),
	)
	ui.Text(c, "Changes apply to terminals opened afterwards.").
		FontSize(Typography().Caption).TextColor(c.Theme().TextMuted).Padding(0, 0, 0, Spacing().XS)
}

// systemName is the platform's product name for GOOS.
func systemName(goos string) string {
	if goos == "darwin" {
		return "macOS"
	}
	return goos
}

// diagLogLevels are the log filter values and the labels shown for them.
var (
	diagLogLevelValues = []string{"ALL", "INFO", "WARN", "ERROR", "DEBUG"}
	diagLogLevelLabels = []string{"All", "Info", "Warning", "Error", "Debug"}
)

// settingsDiagnostics renders the diagnostics snapshot, the sanitized export
// and the recent log viewer.
func (s *Shell) settingsDiagnostics(c *ui.Context) {
	t := c.Theme()
	sp := Spacing()
	home, _ := os.UserHomeDir()
	snap := diagnostics.BuildSnapshot(s.projection.Version, s.projection.Protocol, s.activeInstance, home)

	settingsGroup(c, "About",
		row("Shardlane", "", func() { valueText(c, snap.ShardlaneVersion) }),
		row("System", "", func() { valueText(c, fmt.Sprintf("%s · %s", systemName(snap.OS), snap.Arch)) }),
		row("Herdr", "", func() {
			valueText(c, fallbackText(snap.HerdrVersion, "connected"))
		}),
		row("Session", "", func() { valueText(c, fallbackText(snap.ActiveInstance, "none")) }),
		row("Log file", compactSearchPath(snap.LogPath), func() {
			if snap.LogPath != "" && ui.Button(c, "Show in Finder").Clicked() {
				mygo.Shell.ShowItemInFolder(snap.LogPath)
			}
		}),
	)

	settingsGroup(c, "Compatibility",
		row("Rich chat view", "Turn off to use the classic chat view.", func() {
			if useWeb, changed := settingSwitch(c, "Rich chat view", s.settings.General.WebViewChat); changed {
				if useWeb {
					s.chatWebviewBroken = false // re-arm a latched creation failure
				}
				s.applySettings(func(v *settings.Settings) error {
					v.General.WebViewChat = useWeb
					return nil
				})
			}
		}),
		row("Rich file preview", "CodeMirror-highlighted preview on the Files page.", func() {
			if useWeb, changed := settingSwitch(c, "Rich file preview", s.settings.General.WebViewFilesPreview); changed {
				if useWeb {
					s.filesWebviewBroken = false // re-arm a latched creation failure
				}
				s.applySettings(func(v *settings.Settings) error {
					v.General.WebViewFilesPreview = useWeb
					return nil
				})
			}
		}),
	)

	ui.Row(c).Gap(sp.M).AlignItems(ui.Center).Children(func() {
		if ui.Button(c, "Copy Diagnostics").Clicked() {
			logs, _ := diagnostics.ReadRecentLogs(500)
			exported, err := diagnostics.BuildExportArchive(snap, logs, home)
			if err == nil {
				s.copyToClipboard(exported)
				s.diagExportStatus = "Copied to the clipboard. Secrets are redacted."
			} else {
				s.diagExportStatus = "Couldn't copy diagnostics: " + err.Error()
			}
		}
		if s.diagExportStatus != "" {
			ui.Text(c, s.diagExportStatus).FontSize(Typography().Caption).TextColor(t.TextMuted)
		}
	})

	settingsCard(c, "Logs", func() {
		rawLogs, _ := diagnostics.ReadRecentLogs(5000)
		filtered := diagnostics.FilterLogs(rawLogs, s.diagLogLevelFilter, s.diagSearchQuery)

		ui.Row(c).FillWidth().Gap(sp.S).AlignItems(ui.Center).Children(func() {
			levelIdx := 0
			for i, lvl := range diagLogLevelValues {
				if lvl == s.diagLogLevelFilter {
					levelIdx = i
					break
				}
			}
			if ui.Segmented(c, &levelIdx, diagLogLevelLabels...).Label("Log level").Changed() {
				s.diagLogLevelFilter = diagLogLevelValues[levelIdx]
			}

			ui.Box(c).Grow(1)
			ui.Text(c, fmt.Sprintf("%d of %d entries", len(filtered), len(rawLogs))).
				FontSize(Typography().Caption).TextColor(t.TextMuted)
		})

		ui.Scroll(c).Height(300).Background(designTokens(t.Dark).Content).
			Radius(Radius().Control).Padding(sp.S).Children(func() {
			if len(filtered) == 0 {
				ui.Text(c, "No matching entries").FontSize(Typography().Caption).TextColor(t.TextMuted)
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
						ui.Text(c, msgText).FontSize(Typography().Caption).SingleLine().Grow(1)
					})
				}
			})
		})
	})
}
