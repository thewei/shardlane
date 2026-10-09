package nativeui

/**
 * [INPUT]: WorkspaceSurfaceCommit 唯一草稿、Git snapshot/preflight、DesignTokens 与用户明确操作
 * [OUTPUT]: MyGo 原生 Commit modal 内的表单、AI Prompt 复制、取消保护与 Commit 按钮
 * [POS]: Git commit editor presentation; never stages/commits while rendering
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"strings"

	"github.com/egoist/mygo/ui"
)

// commitSurface renders the single fenced Commit editor inside its native
// modal: subject/body, selected-file checklist, totals, branch identity,
// privacy-reviewed AI handoff and stale-state status. It does not own a
// second Git transaction or an independent primary work surface.
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

	ui.Column(c).Grow(1).MinHeight(0).FillWidth().Children(func() {
		ui.Scroll(c).Grow(1).MinHeight(0).Children(func() {
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
					ui.Row(c).FillWidth().Gap(sp.S).AlignItems(ui.Center).Children(func() {
						ui.Text(c, "Files to include").FontSize(Typography().Caption).FontWeight(600).Grow(1)
						ui.Textf(c, "%d / %d", len(s.selectedCommitPaths()), len(snap.Files)).
							FontSize(Typography().Micro).TextColor(t.TextMuted)
						if ui.Button(c, "Select all").FontSize(Typography().Micro).Clicked() {
							for _, file := range snap.Files {
								s.surface.commit.Selected[file.Path] = true
							}
							s.cancelPiCommitSuggestion()
						}
						if ui.Button(c, "Clear selection").FontSize(Typography().Micro).Clicked() {
							for _, file := range snap.Files {
								s.surface.commit.Selected[file.Path] = false
							}
							s.cancelPiCommitSuggestion()
						}
					})
					for i := range snap.Files {
						cf := snap.Files[i]
						checked := s.surface.commit.Selected[cf.Path]
						ui.Row(c).FillWidth().Padding(sp.XXS, sp.XS).Gap(sp.S).AlignItems(ui.Center).Children(func() {
							ui.Checkbox(c, &checked, "")
							s.surface.commit.Selected[cf.Path] = checked
							ui.Text(c, cf.Status.Letter()).FontSize(Typography().Micro).FontWeight(700).
								TextColor(tokens.StatusColor(statusTone(cf.Status), t.Dark))
							ui.Text(c, cf.Path).FontSize(Typography().BodySmall).Grow(1).MinWidth(0).SingleLine().Tooltip(cf.Path)
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

				s.commitAISection(c, snap)
			})
		})
		s.commitActionFooter(c)
	})
}

// commitActionFooter stays outside the Scroll viewport. A large file
// checklist or an expanded Pi review can never push Commit/Cancel offscreen.
func (s *Shell) commitActionFooter(c *ui.Context) {
	sp := Spacing()
	t := c.Theme()
	ui.Column(c).FillWidth().BorderWidth(1, 0, 0, 0).
		BorderColor(designTokens(t.Dark).BorderSubtle).Padding(sp.M, sp.L).Gap(sp.S).Children(func() {
		if s.commitDiscardPrompt {
			ui.Text(c, "Discard this commit message draft?").FontSize(Typography().Caption).FontWeight(650)
			ui.Row(c).Gap(sp.S).Children(func() {
				if ui.Button(c, "Keep Editing").Clicked() {
					s.commitDiscardPrompt = false
				}
				if ui.Button(c, "Discard Draft").Clicked() {
					s.cancelCommitSurface()
				}
			})
			return
		}
		ui.Row(c).FillWidth().Gap(sp.S).AlignItems(ui.Center).Children(func() {
			selected := s.selectedCommitPaths()
			label := "Commit " + itoa(len(selected)) + " file" + pluralS(len(selected))
			if s.surface.commit.Amend {
				label = "Amend " + label
			}
			ready := len(selected) > 0 && strings.TrimSpace(s.surface.commit.Subject) != "" &&
				!s.surface.commit.InFlight && s.surface.commit.Captured && !s.piCommitRunning
			ui.Textf(c, "%d selected", len(selected)).FontSize(Typography().Caption).TextColor(t.TextMuted)
			ui.Box(c).Grow(1)
			if ui.Button(c, "Cancel Commit").Clicked() {
				s.requestCancelCommit()
			}
			button := ui.PrimaryButton(c, label).FontSize(Typography().Body)
			button.Disabled(!ready)
			if button.Clicked() && ready {
				s.submitCommitAmend(strings.TrimSpace(s.surface.commit.Subject),
					s.surface.commit.Body, selected, s.surface.commit.Amend)
			}
			if s.surface.commit.InFlight {
				ui.Spinner(c).Size(14, 14)
			}
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
