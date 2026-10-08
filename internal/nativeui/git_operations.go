package nativeui

import (
	"context"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

// Explicit repository operations behind the Changes surface, orchestrated
// over the gitworkbench Runner: every action below runs in a background lane
// on explicit user intent, refreshes the snapshot, and reports through a
// toast. Destructive actions reach here only through the confirm dialog.

// gitOpTimeout bounds one working-tree operation (network ops bound
// themselves in the Runner).
const gitOpTimeout = gitworkbench.MutateTimeout

// runGitOp runs one repository mutation off the UI lane and reports the
// outcome as a toast; successful ops refresh the snapshot.
func (s *Shell) runGitOp(name string, run func(ctx context.Context, root string) (string, error)) {
	if s.git == nil || s.git.root == "" || s.git.opBusy {
		return
	}
	root := s.git.root
	s.git.opBusy = true
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), gitOpTimeout)
		defer cancel()
		notice, err := run(ctx, root)
		s.applyOnUI(func() {
			s.git.opBusy = false
			if s.git == nil || s.git.root != root {
				return
			}
			switch {
			case err != nil:
				s.pendingToast = name + " failed: " + firstErrLine(err)
			case notice != "":
				s.pendingToast = notice
			}
			if err == nil {
				s.git.cache.Invalidate(root)
				s.git.preflight = nil
				s.git.expectDrift = true
				s.ensureGitSnapshot(true)
				s.loadBranches()
			}
		})
	}()
}

func firstErrLine(err error) string {
	line := err.Error()
	if i := strings.IndexByte(line, '\n'); i > 0 {
		line = line[:i]
	}
	if len(line) > 200 {
		line = line[:200]
	}
	return strings.TrimSpace(line)
}

// stageFiles moves the worktree changes of paths into the index.
func (s *Shell) stageFiles(paths []string) {
	if len(paths) == 0 {
		return
	}
	s.runGitOp("Stage", func(ctx context.Context, root string) (string, error) {
		if err := s.git.runner.Stage(ctx, root, paths); err != nil {
			return "", err
		}
		return "Staged " + pluralFiles(len(paths)), nil
	})
}

// unstageFiles moves the index changes of paths back to the worktree.
func (s *Shell) unstageFiles(paths []string) {
	if len(paths) == 0 {
		return
	}
	noHead := s.git.snapshot != nil && s.git.snapshot.NoHead
	s.runGitOp("Unstage", func(ctx context.Context, root string) (string, error) {
		if err := s.git.runner.Unstage(ctx, root, noHead, paths); err != nil {
			return "", err
		}
		return "Unstaged " + pluralFiles(len(paths)), nil
	})
}

// discardChanges throws away worktree changes after the confirm dialog.
// kind "git-discard" carries tracked paths, "git-delete-untracked" carries
// untracked files (deleted, not recoverable — hence the confirmation).
func (s *Shell) discardChanges(paths []string) {
	tracked, untracked := s.splitTrackedUntracked(paths)
	if len(tracked) > 0 {
		s.openConfirm("git-discard", joinPathsTarget(tracked),
			"Discard changes in "+pluralFiles(len(tracked))+"?",
			"The reviewed changes to these files are thrown away and cannot be recovered.")
	}
	if len(untracked) > 0 {
		s.openConfirm("git-delete-untracked", joinPathsTarget(untracked),
			"Delete "+pluralFiles(len(untracked))+"?",
			"These untracked files are deleted from disk and cannot be recovered.")
	}
}

func (s *Shell) fetchRepo() {
	s.runGitOp("Fetch", func(ctx context.Context, root string) (string, error) {
		remote := s.git.runner.DefaultRemote(ctx, root)
		if err := s.git.runner.Fetch(ctx, root, remote); err != nil {
			return "", err
		}
		return "Fetched " + remote, nil
	})
}

func (s *Shell) pullRepo() {
	s.runGitOp("Pull", func(ctx context.Context, root string) (string, error) {
		remote := s.git.runner.DefaultRemote(ctx, root)
		branch := s.git.runner.CurrentBranch(ctx, root)
		if branch == "" {
			return "", errGitOp("detached HEAD: check out a branch first")
		}
		if err := s.git.runner.PullFFOnly(ctx, root, remote, branch); err != nil {
			return "", err
		}
		return "Pulled " + remote + "/" + branch, nil
	})
}

func (s *Shell) pushRepo() {
	s.runGitOp("Push", func(ctx context.Context, root string) (string, error) {
		remote := s.git.runner.DefaultRemote(ctx, root)
		branch := s.git.runner.CurrentBranch(ctx, root)
		if branch == "" {
			return "", errGitOp("detached HEAD: check out a branch first")
		}
		setUpstream := !s.git.upstream.OK
		if err := s.git.runner.Push(ctx, root, remote, branch, setUpstream); err != nil {
			return "", err
		}
		notice := "Pushed " + remote + "/" + branch
		if setUpstream {
			notice += " (tracking set)"
		}
		return notice, nil
	})
}

func (s *Shell) stashChanges() {
	s.runGitOp("Stash", func(ctx context.Context, root string) (string, error) {
		if err := s.git.runner.StashPush(ctx, root, ""); err != nil {
			return "", err
		}
		return "Changes stashed", nil
	})
}

func (s *Shell) popStash(ref string) {
	s.runGitOp("Stash Pop", func(ctx context.Context, root string) (string, error) {
		if err := s.git.runner.StashPop(ctx, root, ref); err != nil {
			return "", err
		}
		return "Stash applied", nil
	})
}

func (s *Shell) dropStash(ref string) {
	s.openConfirm("git-drop-stash", ref, "Drop "+ref+"?",
		"The stashed changes are deleted permanently.")
}

func (s *Shell) deleteBranch(name string) {
	s.openConfirm("git-delete-branch", name, "Delete branch "+name+"?",
		"Git's safe delete: an unmerged branch is refused, not forced away.")
}

func (s *Shell) undoLastCommit() {
	s.openConfirm("git-undo-commit", "", "Undo last commit?",
		"The commit is undone with a soft reset: its changes return to the index, nothing is discarded.")
}

// createTag creates a lightweight tag at HEAD.
func (s *Shell) createTag(name string) {
	if strings.TrimSpace(name) == "" {
		return
	}
	s.runGitOp("Create Tag", func(ctx context.Context, root string) (string, error) {
		if err := s.git.runner.CreateTag(ctx, root, name); err != nil {
			return "", err
		}
		return "Created tag " + name, nil
	})
}

// deleteTagConfirm removes a tag after confirmation.
func (s *Shell) deleteTagConfirm(name string) {
	s.openConfirm("git-delete-tag", name, "Delete tag "+name+"?",
		"The tag is removed from the repository. Commits are not touched.")
}

// addWorktree checks out a new work tree at an absolute path.
func (s *Shell) addWorktree(path string) {
	if strings.TrimSpace(path) == "" {
		return
	}
	s.runGitOp("Add Worktree", func(ctx context.Context, root string) (string, error) {
		if err := s.git.runner.AddWorktree(ctx, root, path); err != nil {
			return "", err
		}
		return "Worktree added at " + path, nil
	})
}

// reviewCommit switches the review to one commit's diff (All Commits).
func (s *Shell) reviewCommit(hash string) {
	if s.git == nil || s.git.root == "" || hash == "" {
		return
	}
	if s.git.source == gdSourceCommit && s.git.commitHash == hash {
		return
	}
	root := s.git.root
	s.git.source = gdSourceCommit
	s.git.commitHash = hash
	s.git.commitSnap = nil
	s.git.commitMeta = nil
	s.git.gdFiles = nil
	s.git.gdFilesFor = nil
	s.git.gdRows = nil
	s.git.gdRowsDirty = true
	s.git.gdList = ui.ListState{}
	s.git.gdListEl = nil
	s.git.gdCurrent = 0
	s.git.gdSelFile, s.git.gdSelHunk = -1, -1
	s.git.gdFinding, s.git.gdQuery = false, ""
	s.git.gdMatches, s.git.gdMatchesFor = nil, ""
	s.git.gdHScroll = map[string]float32{}
	s.git.gdFileMatches = map[int]bool{}

	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), gitworkbench.ReadTimeout*2)
		defer cancel()
		snap, err := s.git.runner.CommitSnapshot(ctx, root, hash)
		var meta *gitworkbench.CommitInfo
		if commits, cerr := s.git.runner.ListCommits(ctx, root, 500); cerr == nil {
			for i := range commits {
				if commits[i].Hash == hash {
					meta = &commits[i]
					break
				}
			}
		}
		s.applyOnUI(func() {
			if s.git == nil || s.git.root != root || s.git.source != gdSourceCommit || s.git.commitHash != hash {
				return
			}
			if err != nil {
				s.pendingToast = "Could not read commit: " + firstErrLine(err)
				s.reviewLocalChanges()
				return
			}
			s.git.commitSnap = snap
			s.git.commitMeta = meta
			s.gdSetFiles()
		})
	}()
}

// reviewLocalChanges switches the review back to the worktree (and refreshes
// it, since it may have moved while a commit was up).
func (s *Shell) reviewLocalChanges() {
	if s.git == nil {
		return
	}
	if s.git.source == gdSourceWorktree {
		return
	}
	s.git.source = gdSourceWorktree
	s.git.commitSnap = nil
	s.git.commitMeta = nil
	s.git.gdFiles = nil
	s.git.gdFilesFor = nil
	s.git.gdRows = nil
	s.git.gdRowsDirty = true
	s.git.gdList = ui.ListState{}
	s.git.gdListEl = nil
	s.git.gdCurrent = 0
	s.git.gdFinding, s.git.gdQuery = false, ""
	s.ensureGitSnapshot(true)
}

// gdReviewingCommit reports whether the surface shows a commit's diff.
func (s *Shell) gdReviewingCommit() bool {
	return s.git != nil && s.git.source == gdSourceCommit
}

// confirmations dispatch (from dialogs.submitConfirm).

func (s *Shell) runGitConfirm(kind, target string) {
	switch kind {
	case "git-discard":
		s.runGitOp("Discard", func(ctx context.Context, root string) (string, error) {
			paths := splitPathsTarget(target)
			if err := s.git.runner.Discard(ctx, root, paths); err != nil {
				return "", err
			}
			return "Discarded changes in " + pluralFiles(len(paths)), nil
		})
	case "git-delete-untracked":
		s.runGitOp("Delete", func(ctx context.Context, root string) (string, error) {
			paths := splitPathsTarget(target)
			if err := s.git.runner.DeleteUntracked(root, paths); err != nil {
				return "", err
			}
			return "Deleted " + pluralFiles(len(paths)), nil
		})
	case "git-delete-branch":
		s.runGitOp("Delete Branch", func(ctx context.Context, root string) (string, error) {
			if err := s.git.runner.DeleteBranch(ctx, root, target); err != nil {
				return "", err
			}
			return "Deleted branch " + target, nil
		})
	case "git-drop-stash":
		s.runGitOp("Drop Stash", func(ctx context.Context, root string) (string, error) {
			if err := s.git.runner.StashDrop(ctx, root, target); err != nil {
				return "", err
			}
			return "Dropped " + target, nil
		})
	case "git-undo-commit":
		s.runGitOp("Undo Commit", func(ctx context.Context, root string) (string, error) {
			if err := s.git.runner.UndoLastCommit(ctx, root); err != nil {
				return "", err
			}
			return "Last commit undone (changes kept staged)", nil
		})
	case "git-delete-tag":
		s.runGitOp("Delete Tag", func(ctx context.Context, root string) (string, error) {
			if err := s.git.runner.DeleteTag(ctx, root, target); err != nil {
				return "", err
			}
			return "Deleted tag " + target, nil
		})
	}
}

// amendPrefill loads HEAD's message into the Commit surface for an amend.
func (s *Shell) amendPrefill() {
	if s.git == nil || s.git.root == "" {
		return
	}
	root := s.git.root
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), gitworkbench.ReadTimeout)
		defer cancel()
		subject, body, err := s.git.runner.LastCommitMessage(ctx, root)
		s.applyOnUI(func() {
			if err != nil || s.git == nil || s.git.root != root {
				return
			}
			if strings.TrimSpace(s.surface.commit.Subject) == "" {
				s.surface.commit.Subject = subject
			}
			if strings.TrimSpace(s.surface.commit.Body) == "" {
				s.surface.commit.Body = body
			}
		})
	}()
}

// pluralFiles writes "1 file" / "N files".
func pluralFiles(n int) string {
	if n == 1 {
		return "1 file"
	}
	return itoa(n) + " files"
}

// splitTrackedUntracked sorts paths by their snapshot status: untracked
// files delete from disk, tracked ones restore from HEAD.
func (s *Shell) splitTrackedUntracked(paths []string) (tracked, untracked []string) {
	for _, p := range paths {
		if cf, ok := s.selectedChangeFileInfo(p); ok && cf.Untracked {
			untracked = append(untracked, p)
		} else {
			tracked = append(tracked, p)
		}
	}
	return tracked, untracked
}

func joinPathsTarget(paths []string) string { return strings.Join(paths, "\x00") }
func splitPathsTarget(target string) []string {
	return strings.Split(target, "\x00")
}

type errGitOp string

func (e errGitOp) Error() string { return string(e) }
