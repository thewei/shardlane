// [INPUT]: Git refs/heads and refs/remotes via Runner.Read; Git's branch validation and mutation semantics.
// [OUTPUT]: Branch/RemoteBranch models, local and remote-tracking ref discovery, safe switch/create.
// [POS]: Git domain reference boundary, read-only remote enumeration never contacts network.
// [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
package gitworkbench

import (
	"context"
	"errors"
	"fmt"
	"strings"
)

// Branch is one local branch of the repository (plan §23.1: local only in
// core 0.10; no remote management, no ahead/behind network calls).
type Branch struct {
	Name      string
	Current   bool
	HeadShort string
}

// ListBranches enumerates local branches cheaply (no network).
func (r *Runner) ListBranches(ctx context.Context, root string) ([]Branch, error) {
	out, err := r.Read(ctx, root,
		"for-each-ref", "--format=%(refname:short)%09%(objectname:short)%09%(HEAD)", "refs/heads")
	if err != nil {
		return nil, err
	}
	var branches []Branch
	for _, line := range strings.Split(strings.TrimRight(string(out), "\n"), "\n") {
		if line == "" {
			continue
		}
		parts := strings.Split(line, "\t")
		b := Branch{Name: parts[0]}
		if len(parts) > 1 {
			b.HeadShort = parts[1]
		}
		if len(parts) > 2 && strings.TrimSpace(parts[2]) == "*" {
			b.Current = true
		}
		branches = append(branches, b)
	}
	return branches, nil
}

// RemoteBranch is one locally cached remote-tracking ref. Listing refs does
// not fetch, mutate or infer whether the remote is reachable.
type RemoteBranch struct {
	Remote    string
	Name      string
	HeadShort string
}

// ListRemoteBranches reads refs/remotes without network activity. Symbolic
// HEAD pointers are excluded; only actual branch refs belong in the tree.
func (r *Runner) ListRemoteBranches(ctx context.Context, root string) ([]RemoteBranch, error) {
	out, err := r.Read(ctx, root,
		"for-each-ref", "--format=%(refname:lstrip=2)%09%(objectname:short)%09%(symref)", "refs/remotes")
	if err != nil {
		return nil, err
	}
	var branches []RemoteBranch
	for _, line := range strings.Split(strings.TrimRight(string(out), "\n"), "\n") {
		parts := strings.SplitN(line, "\t", 3)
		if len(parts) < 2 || (len(parts) == 3 && parts[2] != "") {
			continue
		}
		remote, name, ok := strings.Cut(parts[0], "/")
		if !ok || remote == "" || name == "" || name == "HEAD" {
			continue
		}
		branches = append(branches, RemoteBranch{Remote: remote, Name: name, HeadShort: parts[1]})
	}
	return branches, nil
}

// ValidateBranchName checks a candidate name through Git's own ref rules
// (`check-ref-format --branch`), the authority for New Branch.
func (r *Runner) ValidateBranchName(ctx context.Context, root, name string) error {
	if strings.TrimSpace(name) == "" {
		return errors.New("branch name is required")
	}
	if _, err := r.Read(ctx, root, "check-ref-format", "--branch", name); err != nil {
		return fmt.Errorf("invalid branch name %q", name)
	}
	return nil
}

// SwitchResult reports the outcome of a branch switch.
type SwitchResult struct {
	Branch string
	// Carried notes Git's dirty-compatible carry of working changes.
	Created bool
}

// SwitchBranch moves to an existing local branch via `git switch` (plan
// §23.2): no force, no checkout --; Git's refusal is returned, not fought.
func (r *Runner) SwitchBranch(ctx context.Context, root, name string) (SwitchResult, error) {
	if err := r.validateLocalBranch(ctx, root, name); err != nil {
		return SwitchResult{}, err
	}
	if _, err := r.Mutate(ctx, root, "switch", "--", name); err != nil {
		return SwitchResult{}, err
	}
	return SwitchResult{Branch: name}, nil
}

// CreateBranch creates and checks out a new local branch (`git switch -c`).
// No tracking setup, no start-point customization in core 0.10. The name is
// validated by check-ref-format first, so it cannot start with '-' and the
// option-value form is injection-safe.
func (r *Runner) CreateBranch(ctx context.Context, root, name string) (SwitchResult, error) {
	if err := r.ValidateBranchName(ctx, root, name); err != nil {
		return SwitchResult{}, err
	}
	if _, err := r.Mutate(ctx, root, "switch", "--create", name); err != nil {
		return SwitchResult{}, err
	}
	return SwitchResult{Branch: name, Created: true}, nil
}

// validateLocalBranch refuses anything that is not an existing local branch
// name (plan §23.2: validate target as a local branch before switching).
func (r *Runner) validateLocalBranch(ctx context.Context, root, name string) error {
	if strings.TrimSpace(name) == "" {
		return errors.New("branch name is required")
	}
	branches, err := r.ListBranches(ctx, root)
	if err != nil {
		return err
	}
	for _, b := range branches {
		if b.Name == name {
			return nil
		}
	}
	return &Error{Class: ErrRefusal, OpClass: OpMutate, Op: "switch",
		err: fmt.Errorf("%q is not a local branch", name)}
}
