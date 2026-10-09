package nativeui

/**
 * [INPUT]: cached per-repository Git sequencer state and existing mutation confirmations
 * [OUTPUT]: consistent Git conflict guidance and Continue/Abort controls
 * [POS]: Git primary Diff/Review inspector presentation, no Git work in render path
 * [PROTOCOL]: update this header on change, then check CLAUDE.md
 */

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

func gitOperationLabel(kind gitworkbench.SequencerKind) string {
	switch kind {
	case gitworkbench.SequencerMerge:
		return "Merge"
	case gitworkbench.SequencerRevert:
		return "Revert"
	case gitworkbench.SequencerCherryPick:
		return "Cherry-pick"
	default:
		return "Git operation"
	}
}

// gitSequencerActions is shared by the Diff state strip and Inspector,
// avoiding competing semantics for two different sets of buttons.
func (s *Shell) gitSequencerActions(c *ui.Context) {
	if s.git == nil || !s.git.sequencerChecked {
		return
	}
	state := s.git.sequencer
	if !state.Active() {
		return
	}
	if state.Kind == gitworkbench.SequencerUnsupported {
		ui.Text(c, "This Git operation must be resolved in Terminal.").
			FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
	} else {
		continueButton := ui.Button(c, "Continue "+gitOperationLabel(state.Kind))
		continueButton.Disabled(s.git.opBusy || state.Unmerged > 0)
		if state.Unmerged > 0 {
			continueButton.Tooltip("Resolve and stage all unmerged files, then Refresh Changes")
		}
		if continueButton.Clicked() {
			s.continueSequencer()
		}
		abortButton := ui.Button(c, "Abort "+gitOperationLabel(state.Kind))
		abortButton.Disabled(s.git.opBusy)
		if abortButton.Clicked() {
			s.confirmAbortSequencer()
		}
	}
	refresh := ui.Button(c, "Refresh Git State")
	if refresh.Clicked() && !s.git.opBusy {
		s.refreshGitChanges()
	}
}

// gitSequencerBanner appears above any worktree state, including an empty
// filtered file list. The user can always find a way back to resolving a
// paused Git operation, instead of mistaking it for a failed toast.
func (s *Shell) gitSequencerBanner(c *ui.Context) {
	if s.git == nil || !s.git.sequencerChecked || !s.git.sequencer.Active() {
		return
	}
	state := s.git.sequencer
	ui.Column(c).FillWidth().Padding(Spacing().S, Spacing().L).Gap(Spacing().XS).
		Background(c.Theme().Warning.Alpha(0.10)).Children(func() {
		ui.Text(c, gitOperationLabel(state.Kind)+" needs attention").
			FontSize(Typography().Section).FontWeight(650)
		if state.Unmerged > 0 {
			ui.Text(c, fmt.Sprintf("%d unresolved file(s) · Resolve each conflict, stage, then refresh.", state.Unmerged)).
				FontSize(Typography().Caption)
		} else {
			ui.Text(c, "No unresolved index entries. Review staged resolution before continuing.").
				FontSize(Typography().Caption)
		}
		ui.Row(c).FillWidth().Gap(Spacing().S).AlignItems(ui.Center).Children(func() {
			s.gitSequencerActions(c)
		})
	})
}

func (s *Shell) gitSequencerInspector(c *ui.Context) {
	if s.git == nil || !s.git.sequencerChecked || !s.git.sequencer.Active() {
		return
	}
	state := s.git.sequencer
	settingsCard(c, "Git Operation", func() {
		historyFact(c, "Operation", gitOperationLabel(state.Kind), "")
		historyFact(c, "Unresolved", fmt.Sprintf("%d", state.Unmerged), "")
		if len(state.UnmergedPaths) > 0 {
			ui.Text(c, "Files needing resolution").FontSize(Typography().Caption).FontWeight(650)
			for _, path := range state.UnmergedPaths {
				path := path
				_, idx := s.gdFileForPath(path)
				link := ui.Button(c, "Review conflict: "+path).FontSize(Typography().Caption).MaxWidth(225)
				link.Tooltip(path)
				link.Disabled(idx < 0)
				if link.Clicked() && idx >= 0 {
					s.revealDiffFile(path)
				}
			}
			if extra := state.Unmerged - len(state.UnmergedPaths); extra > 0 {
				ui.Textf(c, "…and %d more unresolved files", extra).FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
			}
		}
		ui.Text(c, "Review conflict markers in the editor; stage each resolved file. Continue keeps the resolution; Abort may discard in-progress resolution edits.").
			FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
		ui.Text(c, "Resolve or Abort from the Git worktree banner above the Diff Review.").FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
	})
}
