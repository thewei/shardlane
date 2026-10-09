package nativeui

/**
 * [INPUT]: selected reviewed Git snapshot, Pi suggestion state and privacy preference
 * [OUTPUT]: compact disclosure-based Pi drafting UI within the Commit editor
 * [POS]: one optional Commit subsection; no duplicate modal, Git task or provider runtime
 * [PROTOCOL]: update this header on change, then check CLAUDE.md
 */

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

// commitAISection is collapsed until explicitly opened. Its optional
// source preview and paste editor also stay collapsed until requested,
// keeping the primary Commit form short and predictable.
func (s *Shell) commitAISection(c *ui.Context, snap *gitworkbench.ChangesSnapshot) {
	sp := Spacing()
	t := c.Theme()
	if !s.commitPromptPreviewOpen {
		open := ui.Button(c, "AI Commit Message…").FontSize(Typography().Caption).
			Tooltip("Generate a candidate with Pi or review a prompt to copy")
		open.Disabled(len(s.selectedCommitPaths()) == 0)
		if open.Clicked() {
			s.commitPromptPreviewOpen = true
			s.commitPromptIncludeExcerpt = false
			s.commitPromptTextVisible = false
			s.commitAIManualEntryOpen = false
		}
		return
	}
	preview := gitworkbench.PrepareCommitPrompt(snap, s.selectedCommitPaths(), s.commitPromptIncludeExcerpt)
	ui.Column(c).FillWidth().MinWidth(0).Gap(sp.S).Padding(sp.M).
		Border(1, designTokens(t.Dark).BorderSubtle).Radius(Radius().Control).Children(func() {
		ui.Row(c).FillWidth().Gap(sp.S).AlignItems(ui.Center).Children(func() {
			ui.Text(c, "AI commit message").FontSize(Typography().Section).FontWeight(650).Grow(1)
			if ui.Button(c, "Hide AI").FontSize(Typography().Caption).Clicked() {
				s.cancelPiCommitSuggestion()
				s.commitPromptPreviewOpen = false
			}
		})
		ui.Textf(c, "%d included · %d sensitive excluded", preview.IncludedFiles, preview.ExcludedFiles).
			FontSize(Typography().Caption).TextColor(t.TextMuted)
		ui.Checkbox(c, &s.commitPromptIncludeExcerpt, "Include code excerpts (explicit opt-in)")
		ui.Text(c, "Pi drafts a message only. Review it before committing. Rules: Settings → Git & AI.").
			FontSize(Typography().Caption).TextColor(t.TextMuted)
		if preview.Text == "" {
			ui.Text(c, "No eligible selected files. Sensitive paths stay excluded.").
				FontSize(Typography().Caption).TextColor(t.Warning)
		}
		if preview.Truncated {
			ui.Text(c, "Review shortened to the safety limit.").FontSize(Typography().Caption).
				TextColor(t.Warning)
		}
		ui.Row(c).FillWidth().Gap(sp.S).Wrap().AlignItems(ui.Center).Children(func() {
			if s.piCommitRunning {
				ui.Spinner(c).Size(14, 14)
				ui.Text(c, "Generating with Pi…").FontSize(Typography().Caption).TextColor(t.TextMuted)
				if ui.Button(c, "Cancel Generation").Clicked() {
					s.cancelPiCommitSuggestion()
					s.piCommitError = "Pi generation canceled."
				}
			} else {
				generate := ui.PrimaryButton(c, "Generate with Pi")
				generate.Disabled(preview.Text == "" || !s.surface.commit.Captured || s.surface.commit.InFlight)
				if generate.Clicked() {
					s.startPiCommitSuggestion()
				}
			}
			review := ui.Button(c, "Review prompt")
			review.Disabled(preview.Text == "")
			if review.Clicked() {
				s.commitPromptTextVisible = !s.commitPromptTextVisible
			}
			if !s.commitAIManualEntryOpen && s.commitAIResponse == "" {
				if ui.Button(c, "Paste suggestion…").Clicked() {
					s.commitAIManualEntryOpen = true
				}
			}
		})
		if s.piCommitError != "" {
			ui.Text(c, s.piCommitError).FontSize(Typography().Caption).TextColor(t.TextMuted)
		}
		if s.commitPromptTextVisible && preview.Text != "" {
			ui.Column(c).FillWidth().Height(118).Clip().
				Background(designTokens(t.Dark).Panel).Children(func() {
				ui.Scroll(c).Grow(1).Children(func() {
					ui.Text(c, preview.Text).FontSize(Typography().Micro).TextColor(t.TextMuted)
				})
			})
			if ui.Button(c, "Copy Reviewed AI Prompt").Clicked() {
				s.copyToClipboard(preview.Text)
				s.pendingToast = "Reviewed prompt copied. No AI request was sent."
			}
		}
		if s.commitAIManualEntryOpen || s.commitAIResponse != "" {
			ui.Text(c, "Suggested commit message").FontSize(Typography().Caption).FontWeight(650)
			ui.TextArea(c, &s.commitAIResponse).Height(76).Grow(0).
				Placeholder("Subject followed by optional body").Label("AI message suggestion")
			if s.commitAIError != "" {
				ui.Text(c, s.commitAIError).FontSize(Typography().Caption).TextColor(t.Danger)
			}
			if s.commitAIReplaceConfirm {
				ui.Text(c, "Replace your existing commit message?").FontSize(Typography().Caption).TextColor(t.Warning)
				ui.Row(c).Gap(sp.S).Children(func() {
					if ui.Button(c, "Keep Current Message").Clicked() {
						s.commitAIReplaceConfirm = false
					}
					replace := ui.Button(c, "Replace Message")
					replace.Disabled(s.surface.commit.InFlight || s.piCommitRunning)
					if replace.Clicked() {
						s.replaceCommitMessageWithSuggestion()
					}
				})
			} else {
				use := ui.Button(c, "Use Suggested Message")
				use.Disabled(strings.TrimSpace(s.commitAIResponse) == "" || s.surface.commit.InFlight || s.piCommitRunning)
				if use.Clicked() {
					s.applyManualAISuggestion()
				}
			}
		}
	})
}
