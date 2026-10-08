package nativeui

import (
	"strings"

	"github.com/egoist/mygo/ui"
)

// commitSurface renders the center Commit surface (plan §22): subject/body,
// selected-file checklist, totals, branch identity and the stale-state
// fence status. It is a surface, not a modal.
func (s *Shell) commitSurface(c *ui.Context) {
	t := c.Theme()
	tokens := designTokens(t.Dark)
	sp := Spacing()

	snap := s.gitSnapshot()
	if s.git == nil || s.git.root == "" {
		emptyState(c, "No Git Repository", "The selected tab is not inside a Git repository.")
		return
	}
	if snap == nil || len(snap.Files) == 0 {
		emptyState(c, "Nothing to Commit", "The working tree is clean.")
		return
	}

	ui.Scroll(c).Grow(1).Children(func() {
		ui.Column(c).FillWidth().Padding(sp.L).Gap(sp.M).Children(func() {
			// Identity line: N files · +A -D on branch.
			add, del := snap.TotalAdditions, snap.TotalDeletions
			ui.Row(c).FillWidth().Gap(sp.S).AlignItems(ui.Center).Children(func() {
				ui.Textf(c, "%d file%s · +%d -%d", len(snap.Files), pluralS(len(snap.Files)), add, del).
					FontSize(Typography().Caption).TextColor(t.TextMuted)
				if s.surface.commit.Captured {
					statusPill(c, "review verified", ToneSuccess)
				} else if s.surface.commit.ErrText != "" {
					statusPill(c, "fence failed", ToneError)
				} else {
					statusPill(c, "capturing review state…", ToneWorking)
				}
			})

			// Subject.
			ui.Column(c).FillWidth().Gap(sp.XXS).Children(func() {
				ui.Row(c).FillWidth().Gap(sp.S).AlignItems(ui.Center).Children(func() {
					ui.Text(c, "Subject").FontSize(Typography().Caption).FontWeight(600).Grow(1)
					amend := s.surface.commit.Amend
					ui.Checkbox(c, &amend, "")
					s.surface.commit.Amend = amend
					ui.Text(c, "Amend last commit").FontSize(Typography().Caption).TextColor(t.TextMuted)
				})
				if s.surface.commit.Amend && !s.surface.commit.AmendPrefilled {
					s.surface.commit.AmendPrefilled = true
					s.amendPrefill()
				}
				if !s.surface.commit.Amend {
					s.surface.commit.AmendPrefilled = false
				}
				// Height is explicit: Grow inside this intrinsic-height Column
				// collapsed the subject input to 0px — invisible and
				// unclickable (2026-10-06 F26).
				ui.TextInput(c, &s.surface.commit.Subject).Grow(0).Height(30).Placeholder("Required commit summary").Label("Commit subject")
			})

			// Body.
			ui.Column(c).FillWidth().Gap(sp.XXS).Children(func() {
				ui.Text(c, "Description (optional)").FontSize(Typography().Caption).FontWeight(600)
				ui.TextArea(c, &s.surface.commit.Body).Grow(0).Height(72).Placeholder("Extra detail").Label("Commit description")
			})

			// Selected-file checklist (GWB-223 selection checklist).
			ui.Column(c).FillWidth().Gap(sp.XXS).Children(func() {
				ui.Text(c, "Files to include").FontSize(Typography().Caption).FontWeight(600)
				for i := range snap.Files {
					cf := snap.Files[i]
					checked := s.surface.commit.Selected[cf.Path]
					ui.Row(c).FillWidth().Padding(sp.XXS, sp.XS).Gap(sp.S).AlignItems(ui.Center).Children(func() {
						ui.Checkbox(c, &checked, "")
						s.surface.commit.Selected[cf.Path] = checked
						ui.Text(c, cf.Status.Letter()).FontSize(Typography().Micro).FontWeight(700).
							TextColor(tokens.StatusColor(statusTone(cf.Status), t.Dark))
						ui.Text(c, cf.Path).FontSize(Typography().BodySmall).Grow(1).SingleLine()
						ui.Text(c, "+"+itoa(cf.Additions)+" -"+itoa(cf.Deletions)).
							FontSize(Typography().Micro).TextColor(t.TextMuted).SingleLine()
					})
				}
			})

			// Error / result area.
			if s.surface.commit.ErrText != "" {
				ui.Row(c).FillWidth().Padding(sp.S, sp.M).Background(t.Danger.Alpha(0.12)).
					Radius(Radius().Control).Children(func() {
					ui.Text(c, s.surface.commit.ErrText).FontSize(Typography().Caption).TextColor(t.Danger).Grow(1)
				})
			}
			if s.surface.commit.CommittedHash != "" {
				ui.Row(c).FillWidth().Padding(sp.S, sp.M).Background(t.Success.Alpha(0.12)).
					Radius(Radius().Control).Children(func() {
					ui.Textf(c, "Committed %s. Working tree refreshed below.", s.surface.commit.CommittedHash).
						FontSize(Typography().Caption).TextColor(t.Success).Grow(1)
				})
			}

			// Commit action row.
			ui.Row(c).FillWidth().Gap(sp.S).AlignItems(ui.Center).Children(func() {
				selected := s.selectedCommitPaths()
				label := "Commit " + itoa(len(selected)) + " file"
				if len(selected) != 1 {
					label += "s"
				}
				if s.surface.commit.Amend {
					label = "Amend " + label
				}
				ready := len(selected) > 0 && strings.TrimSpace(s.surface.commit.Subject) != "" &&
					!s.surface.commit.InFlight && s.surface.commit.Captured
				button := ui.PrimaryButton(c, label).FontSize(Typography().Body)
				if !ready {
					button.Opacity(0.5)
				}
				if ready && button.Clicked() {
					s.submitCommitAmend(strings.TrimSpace(s.surface.commit.Subject),
						s.surface.commit.Body, selected, s.surface.commit.Amend)
				}
				ui.Spacer(c)
				if s.surface.commit.InFlight {
					ui.Spinner(c).Size(14, 14)
					ui.Text(c, "Committing…").FontSize(Typography().Caption).TextColor(t.TextMuted)
				}
			})
		})
	})
}

// selectedCommitPaths gathers the checked paths in snapshot order.
func (s *Shell) selectedCommitPaths() []string {
	snap := s.gitSnapshot()
	if snap == nil {
		return nil
	}
	var paths []string
	for _, cf := range snap.Files {
		if s.surface.commit.Selected[cf.Path] {
			paths = append(paths, cf.Path)
		}
	}
	return paths
}
