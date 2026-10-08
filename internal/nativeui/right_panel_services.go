package nativeui

import (
	"fmt"
	"path/filepath"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/scripts"
	"github.com/wh-studio/herdr-client/internal/services"
)

// servicesToolView renders the Services surface: user scripts with real
// Herdr-backed launch, and observed listening ports only (GWB-004/005/006).
// No hard-coded sample ports exist anywhere in this view.
func (s *Shell) servicesToolView(c *ui.Context) {
	t := c.Theme()
	sp := Spacing()
	root := s.rightPanel.currentRoot

	if root == "" {
		emptyState(c, "No Active Project", "Select a project tab to manage services.")
		return
	}

	ui.Scroll(c).Grow(1).Children(func() {
		ui.Column(c).FillWidth().Padding(sp.M).Gap(sp.L).Children(func() {
			// Section 1: User Scripts
			ui.Column(c).FillWidth().Gap(sp.S).Children(func() {
				ui.Row(c).FillWidth().AlignItems(ui.Center).Children(func() {
					ui.Text(c, "Scripts").FontSize(Typography().Section).FontWeight(650).Grow(1)
				})

				var scriptList []scripts.ScriptDefinition
				if s.scriptStore != nil {
					scriptList = s.scriptStore.List(root)
				}

				if len(scriptList) == 0 {
					// F117: the store has no creation UI, so the empty state
					// must say where scripts live.
					hint := "No scripts configured for this directory."
					if s.userDataDir != "" {
						hint = "No scripts configured for this directory. Add entries to " +
							filepath.Join(s.userDataDir, "scripts.json")
					}
					ui.Text(c, hint).
						FontSize(Typography().Caption).TextColor(t.TextMuted)
				} else {
					for _, item := range scriptList {
						sc := item
						ui.Row(c).FillWidth().Padding(sp.S, sp.M).Gap(sp.S).
							AlignItems(ui.Center).Radius(Radius().Control).
							Background(designTokens(t.Dark).Content).Children(func() {
							ui.Text(c, sc.Name).FontSize(Typography().BodySmall).FontWeight(600).Grow(1).SingleLine()
							if sc.OneShot {
								statusPill(c, "One-shot", ToneMuted)
							} else {
								statusPill(c, "Service", ToneInfo)
							}
							if s.scriptBusy[sc.Name] {
								ui.Spinner(c).Size(12, 12)
							} else if ui.Button(c, "Run").FontSize(Typography().Micro).Clicked() {
								s.runScriptCommand(root, sc)
							}
						})
					}
				}
			})

			// Section 2: Observed listening ports (real probe snapshot only).
			ui.Column(c).FillWidth().Gap(sp.S).Children(func() {
				ui.Text(c, "Listening Ports & Local Preview").FontSize(Typography().Section).FontWeight(650)
				ports := s.observedPorts()
				if len(ports) == 0 {
					ui.Text(c, "No listening local ports observed.").
						FontSize(Typography().Caption).TextColor(t.TextMuted)
				}
				for _, port := range ports {
					p := port
					ui.Row(c).FillWidth().Padding(sp.S, sp.M).Gap(sp.S).
						AlignItems(ui.Center).Radius(Radius().Control).
						Background(designTokens(t.Dark).Content).Children(func() {
						ui.Text(c, fmt.Sprintf(":%d", p)).FontSize(Typography().BodySmall).Bold().Grow(1)
						if ui.Button(c, "Open Preview").FontSize(Typography().Micro).Clicked() {
							s.openLocalPreviewURL(services.LocalPreviewURL(p))
						}
					})
				}
			})
		})
	})
}
