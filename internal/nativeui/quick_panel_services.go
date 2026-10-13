package nativeui

/**
 * [INPUT]: selected live Herdr tab root, script definitions and bounded observed-port snapshot
 * [OUTPUT]: compact Services tab inside the existing Agent activity floating panel
 * [POS]: shared Header Agent/Services popover; no extra right rail or runtime owner
 * [PROTOCOL]: update this header and check CLAUDE.md
 */

import (
	"fmt"
	"path/filepath"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/scripts"
	"github.com/wh-studio/herdr-client/internal/services"
)

func (s *Shell) quickPanelServices(c *ui.Context) {
	t := c.Theme()
	sp := Spacing()
	root := s.selectedTabCWD()
	ui.Scroll(c).Grow(1).MinHeight(0).Children(func() {
		ui.Column(c).FillWidth().Padding(sp.M, sp.L, sp.L).Gap(sp.L).Children(func() {
			if root == "" {
				emptyState(c, "No project selected", "Pick a tab to see its scripts and ports.")
				return
			}
			ui.Row(c).FillWidth().Gap(sp.S).AlignItems(ui.Center).Children(func() {
				ui.Icon(c, iconFolder).Size(14, 14).TextColor(t.TextMuted)
				ui.Text(c, filepath.Base(root)).FontSize(Typography().Caption).FontWeight(WeightSemibold).
					Grow(1).MinWidth(0).SingleLine().Tooltip(root)
			})
			// Scripts have a real Herdr launch path; the button is present
			// only for configured definitions, never for sample commands.
			ui.Column(c).FillWidth().Gap(sp.S).Children(func() {
				var defs []scripts.ScriptDefinition
				if s.scriptStore != nil {
					defs = s.scriptStore.List(root)
				}
				ui.Row(c).FillWidth().AlignItems(ui.Center).Children(func() {
					ui.Text(c, "Scripts").FontSize(Typography().Section).FontWeight(WeightSemibold).Grow(1)
					ui.Textf(c, "%d", len(defs)).FontSize(Typography().Caption).TextColor(t.TextMuted)
				})
				if len(defs) == 0 {
					ui.Text(c, "No scripts").
						FontSize(Typography().Caption).TextColor(t.TextMuted)
				}
				for _, def := range defs {
					script := def
					ui.Row(c).FillWidth().Gap(sp.S).Padding(sp.S).Radius(Radius().Control).
						Background(designTokens(t.Dark).Panel).AlignItems(ui.Center).Children(func() {
						ui.Icon(c, iconTerminal).Size(14, 14).TextColor(t.TextMuted)
						ui.Column(c).Grow(1).MinWidth(0).Gap(sp.XXS).Children(func() {
							ui.Text(c, script.Name).FontSize(Typography().BodySmall).FontWeight(WeightSemibold).
								SingleLine().Tooltip(script.Name)
							kind := "Service"
							if script.OneShot {
								kind = "One-shot"
							}
							ui.Text(c, kind).FontSize(Typography().Micro).TextColor(t.TextMuted)
						})
						run := compactButton(c, "Run")
						run.Disabled(s.scriptBusy[script.Name])
						if run.Clicked() {
							s.runScriptCommand(root, script)
						}
					})
				}
			})
			ui.Divider(c)
			ui.Column(c).FillWidth().Gap(sp.S).Children(func() {
				ports := s.observedPorts()
				ui.Row(c).FillWidth().AlignItems(ui.Center).Children(func() {
					ui.Text(c, "Listening ports").FontSize(Typography().Section).FontWeight(WeightSemibold).Grow(1)
					ui.Textf(c, "%d", len(ports)).FontSize(Typography().Caption).TextColor(t.TextMuted)
				})
				if len(ports) == 0 {
					ui.Text(c, "No open ports").
						FontSize(Typography().Caption).TextColor(t.TextMuted)
				}
				for _, port := range ports {
					p := port
					ui.Row(c).FillWidth().Gap(sp.S).Padding(sp.S).Radius(Radius().Control).
						Background(designTokens(t.Dark).Panel).AlignItems(ui.Center).Children(func() {
						ui.Icon(c, iconGlobe).Size(14, 14).TextColor(t.TextMuted)
						ui.Text(c, fmt.Sprintf("localhost:%d", p)).FontSize(Typography().BodySmall).Grow(1)
						if compactButton(c, "Preview").Clicked() {
							s.openLocalPreviewURL(services.LocalPreviewURL(p))
						}
					})
				}
			})
		})
	})
}
