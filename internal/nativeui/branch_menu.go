// [INPUT]: Cached local branches and existing safe branch mutation/confirmation flows.
// [OUTPUT]: Current-first branch search plus the guarded switch/create entry points the sidebar switchers call.
// [POS]: Git branch actions only; the sidebar owns reference navigation and grouping, sidebar_switchers the popover.
// [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
package nativeui

import (
	"strings"

	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

// matchingLocalBranches keeps the current branch visible first when
// it matches the query. The filter is local presentation only.
func matchingLocalBranches(branches []gitworkbench.Branch, filter string) []gitworkbench.Branch {
	query := strings.ToLower(strings.TrimSpace(filter))
	result := make([]gitworkbench.Branch, 0, len(branches))
	for _, branch := range branches {
		if branch.Current && strings.Contains(strings.ToLower(branch.Name), query) {
			result = append(result, branch)
		}
	}
	for _, branch := range branches {
		if !branch.Current && strings.Contains(strings.ToLower(branch.Name), query) {
			result = append(result, branch)
		}
	}
	return result
}

// confirmSwitchBranch asks for explicit confirmation when the worktree is
// dirty (plan §23.2 preflight), then switches.
func (s *Shell) confirmSwitchBranch(name string) {
	if !s.dirtyWorktree() {
		s.switchBranch(name)
		return
	}
	s.openConfirm("git-switch-branch", name, "Switch Branch With Local Changes?",
		"Your uncommitted changes come along to "+name+". Git refuses if they conflict.")
}

// confirmCreateBranch validates the name then creates.
func (s *Shell) confirmCreateBranch(name string) {
	s.createBranch(name)
}

