package nativeui

/**
 * [INPUT]: cached per-repository Git sequencer state and existing mutation confirmations
 * [OUTPUT]: consistent Git conflict guidance and Continue/Abort controls
 * [POS]: Git primary Diff/Review inspector presentation, no Git work in render path
 * [PROTOCOL]: update this header on change, then check CLAUDE.md
 */

import (
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
		continueButton := compactButton(c, "Continue "+gitOperationLabel(state.Kind))
		continueButton.Disabled(s.git.opBusy || state.Unmerged > 0)
		if state.Unmerged > 0 {
			continueButton.Tooltip("Resolve and stage every conflicted file first")
		}
		if continueButton.Clicked() {
			s.continueSequencer()
		}
		abortButton := compactButton(c, "Abort "+gitOperationLabel(state.Kind))
		abortButton.Disabled(s.git.opBusy)
		if abortButton.Clicked() {
			s.confirmAbortSequencer()
		}
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
			FontSize(Typography().Section).FontWeight(WeightSemibold)
		if state.Unmerged > 0 {
			ui.Text(c, pluralFiles(state.Unmerged)+" with conflicts. Resolve and stage, then refresh.").
				FontSize(Typography().Caption)
		} else {
			ui.Text(c, "No conflicts left. Review the staged result, then continue.").
				FontSize(Typography().Caption)
		}
		ui.Row(c).FillWidth().Gap(Spacing().S).AlignItems(ui.Center).Children(func() {
			s.gitSequencerActions(c)
			if len(state.UnmergedPaths) > 0 {
				label := "Show conflict files"
				if s.gitConflictFilesExpanded {
					label = "Hide conflict files"
				}
				if compactButton(c, label).Clicked() {
					s.gitConflictFilesExpanded = !s.gitConflictFilesExpanded
				}
			}
		})
		if s.gitConflictFilesExpanded {
			ui.Column(c).FillWidth().Gap(Spacing().XXS).Children(func() {
				ui.Text(c, "Files needing resolution").FontSize(Typography().Caption).FontWeight(WeightSemibold)
				for _, path := range state.UnmergedPaths {
					_, index := s.gdFileForPath(path)
					link := compactButton(c, "Review Conflict: "+path).MaxWidth(270).Tooltip(path)
					link.Disabled(index < 0)
					if link.Clicked() && index >= 0 {
						s.revealDiffFile(path)
					}
				}
			})
		}
	})
}
