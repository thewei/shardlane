package nativeui

import (
	"log/slog"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
)

func (s *Shell) openTextDialog(kind, target, title, label, value string) {
	s.dialogKind, s.dialogTarget = kind, target
	s.dialogTitle, s.dialogLabel, s.dialogValue = title, label, value
	s.dialogOpen = true
}

func (s *Shell) openConfirm(kind, target, title, message string) {
	s.confirmKind, s.confirmTarget = kind, target
	s.confirmRepoRoot = ""
	if strings.HasPrefix(kind, "git-") && s.git != nil {
		s.confirmRepoRoot = s.git.root
	}
	s.confirmTitle, s.confirmMessage = title, message
	s.confirmOpen = true
}

func (s *Shell) dialogs(c *ui.Context) {
	if s.mergeDialogOpen && (s.git == nil || s.mergeRepoRoot == "" || s.mergeRepoRoot != s.git.root) {
		s.mergeDialogOpen = false
		s.pendingToast = "Repository changed. Reopen Merge for the current repository."
	}
	if s.mergeDialogOpen {
		ui.Modal(c, &s.mergeDialogOpen, func() {
			ui.Column(c).Width(430).MaxWidth(480).Gap(Spacing().M).Children(func() {
				ui.Text(c, "Merge branch").FontSize(Typography().Title).FontWeight(650)
				ui.Text(c, s.mergeTarget).FontSize(Typography().Body).FontWeight(600).SingleLine().Tooltip(s.mergeTarget)
				ui.Segmented(c, &s.mergeStrategy, "Fast-forward only", "Create merge commit").FillWidth()
				if s.mergeStrategy == 0 {
					ui.Text(c, "Only moves the branch forward when histories have not diverged. No merge commit is created.").FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
				} else {
					ui.Text(c, "Creates a merge commit. Conflicts may require resolving files before Continue or Abort.").FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
				}
				ui.Row(c).FillWidth().Gap(Spacing().S).Justify(ui.End).Children(func() {
					if ui.Button(c, "Cancel Merge").Clicked() {
						s.mergeDialogOpen = false
					}
					if ui.PrimaryButton(c, "Review merge").Clicked() {
						target, strategy := s.mergeTarget, s.mergeStrategy
						s.mergeDialogOpen = false
						if strategy == 1 {
							s.confirmMergeNoFFBranch(target)
						} else {
							s.confirmMergeBranch(target)
						}
					}
				})
			})
		})
	}
	// Commit always owns one modal, regardless of which Git entry opened it.
	// MyGo dismisses Modal on backdrop/Escape; we turn such dismissals into
	// the same explicit guarded cancel flow on the next frame.
	if s.surface.current() == WorkspaceSurfaceCommit {
		if !s.commitDialogOpen {
			s.commitDialogOpen = true
			s.requestCancelCommit()
		}
		if s.surface.current() == WorkspaceSurfaceCommit {
			ui.Modal(c, &s.commitDialogOpen, func() {
				_, height := c.Size()
				ui.Column(c).Width(680).MaxWidth(760).Height(min(height-100, 600)).
					MinHeight(240).MinWidth(0).Gap(Spacing().M).Children(func() {
					ui.Text(c, "Commit changes").FontSize(Typography().Title).FontWeight(700)
					ui.Text(c, "Review the selected files and message before committing.").FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
					s.commitSurface(c)
				})
			})
		}
	}
	if s.dialogOpen {
		ui.Modal(c, &s.dialogOpen, func() {
			ui.Text(c, s.dialogTitle).Bold().FontSize(16)
			field := ui.TextInput(c, &s.dialogValue).Label(s.dialogLabel).AutoFocus().Width(360)
			submit := field.Submitted()
			ui.Row(c).Gap(8).Justify(ui.End).Children(func() {
				if ui.Button(c, "Cancel").Clicked() {
					s.dialogOpen = false
				}
				if ui.PrimaryButton(c, "Save").Clicked() {
					submit = true
				}
			})
			if submit && strings.TrimSpace(s.dialogValue) != "" {
				kind, target, value := s.dialogKind, s.dialogTarget, strings.TrimSpace(s.dialogValue)
				s.dialogOpen = false
				s.submitTextDialog(kind, target, value)
			}
		})
	}
	if s.confirmOpen {
		// Destructive confirms must not default-focus the destructive
		// button: AlertDialog focuses and Enter-clicks its LAST button, so
		// Cancel goes last (Enter dismisses safely; Escape still cancels by
		// label) and Confirm is index 0 (2026-10-06 F69).
		if ui.AlertDialog(c, &s.confirmOpen, s.confirmTitle, s.confirmMessage, "Confirm", "Cancel") == 0 {
			kind, target := s.confirmKind, s.confirmTarget
			s.confirmOpen = false
			s.submitConfirm(kind, target)
		}
	}
}

func (s *Shell) submitTextDialog(kind, target, value string) {
	switch kind {
	case "create-workspace":
		s.createWorkspace(value)
	case "rename-workspace":
		s.renameWorkspace(target, value)
	case "add-machine":
		s.addRemoteMachine(value)
	case "rename-machine":
		s.renameMachine(target, value)
	case "rename-project":
		s.mutate(func() (herdr.Projection, error) { return s.runtime.RenameProject(s.activeInstance, target, value) })
	case "rename-tab":
		s.mutate(func() (herdr.Projection, error) { return s.runtime.RenameTab(s.activeInstance, target, value) })
	case "rename-pane":
		s.mutate(func() (herdr.Projection, error) { return s.runtime.RenamePane(s.activeInstance, target, value) })
	case "git-add-worktree":
		s.addWorktree(value)
	}
}

func (s *Shell) submitConfirm(kind, target string) {
	if strings.HasPrefix(kind, "git-") && (s.git == nil || s.confirmRepoRoot == "" || s.git.root != s.confirmRepoRoot) {
		s.pendingToast = "Repository changed. Please reopen the Git action before confirming."
		return
	}
	switch kind {
	case "delete-workspace":
		s.deleteWorkspace(target)
	case "forget-machine":
		s.forgetMachine(target)
	case "close-project":
		s.mutate(func() (herdr.Projection, error) { return s.runtime.CloseProject(s.activeInstance, target) })
	case "close-tab":
		s.mutate(func() (herdr.Projection, error) { return s.runtime.CloseTab(s.activeInstance, target) })
	case "close-pane":
		s.mutate(func() (herdr.Projection, error) { return s.runtime.ClosePane(s.activeInstance, target) })
	default:
		s.runGitConfirm(kind, target)
	}
}

func (s *Shell) createWorkspace(displayName string) {
	if s.win == nil || s.loading {
		return
	}
	generation := s.generation.Add(1)
	s.loading = true
	s.status = "Creating Workspace…"
	slog.Info("create workspace requested", "display_name", displayName)
	go func() {
		instance, err := s.runtime.CreateInstance(displayName)
		if err != nil {
			s.updateError(generation, err)
			return
		}
		projection, err := s.runtime.Projection(instance.Name)
		if err != nil {
			s.updateError(generation, err)
			return
		}
		instances, _ := s.runtime.ListAllInstances()
		slog.Info("workspace created", "instance", instance.Name)
		s.win.Update(func() {
			if generation != s.generation.Load() {
				return
			}
			s.instances = instances
			s.clearLocalSelection()
			s.closeAllTerminals()
			s.activeInstance = instance.Name
			s.loading = false
			s.errText = ""
			s.offline = false
			s.status = "Native Terminal"
			s.applyProjection(projection, true)
		})
	}()
}

func (s *Shell) renameWorkspace(name, displayName string) {
	if s.win == nil || s.loading {
		return
	}
	generation := s.generation.Add(1)
	s.loading = true
	slog.Info("rename workspace requested", "instance", name)
	go func() {
		if _, err := s.runtime.RenameInstance(name, displayName); err != nil {
			s.updateError(generation, err)
			return
		}
		instances, err := s.runtime.ListAllInstances()
		if err != nil {
			s.updateError(generation, err)
			return
		}
		slog.Info("workspace renamed", "instance", name)
		s.win.Update(func() {
			if generation != s.generation.Load() {
				return
			}
			s.instances = instances
			s.loading = false
			s.errText = ""
			s.offline = false
		})
	}()
}

func (s *Shell) deleteWorkspace(name string) {
	if s.win == nil || s.loading {
		return
	}
	generation := s.generation.Add(1)
	s.loading = true
	slog.Info("delete workspace requested", "instance", name)
	go func() {
		if err := s.runtime.DeleteInstance(name); err != nil {
			s.updateError(generation, err)
			return
		}
		slog.Info("workspace deleted", "instance", name)
		instances, err := s.runtime.ListAllInstances()
		if err != nil {
			s.updateError(generation, err)
			return
		}
		s.win.Update(func() {
			if generation != s.generation.Load() {
				return
			}
			if name == s.activeInstance {
				if s.watchCancel != nil {
					s.watchCancel()
					s.watchCancel = nil
				}
				s.closeAllTerminals()
				s.activeInstance = ""
				s.projection = herdr.Projection{}
				s.clearLocalSelection()
				s.selected = ""
			}
			s.instances = instances
			s.loading = false
			s.errText = ""
			s.offline = false
			s.status = "Choose a Herdr workspace"
		})
	}()
}
