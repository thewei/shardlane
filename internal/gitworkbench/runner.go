// Package gitworkbench owns Shardlane's local Git review model: one command
// runner, repository facts, working-tree change snapshots, a clean-room patch
// parser, word diff, changed-file trees, commit transactions and branch
// switching. It supersedes internal/gitintel as the single Git cache.
//
// Hard rules:
//   - every Git invocation is argv-based through exec.CommandContext; never a
//     shell string;
//   - reads carry bounded time/output and never mutate;
//   - mutations (add/commit/switch) are single-flight per repository root and
//     run only on explicit user intent;
//   - no automatic network fetch, no force, no reset --hard, no clean;
//   - errors classify by operation class; stderr text stays in the returned
//     error for the UI but is never logged with file/commit content by this
//     package.
package gitworkbench

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"sync"
	"time"
)

// Policy bounds for Git subprocess work (plan §13/§15/§35).
const (
	// ReadTimeout is the hard deadline for one read command.
	ReadTimeout = 5 * time.Second
	// MutateTimeout bounds one mutation (commit hooks may be slow).
	MutateTimeout = 30 * time.Second
	// MaxReadOutput bounds captured stdout of one read command.
	MaxReadOutput = 32 << 20 // 32 MiB
	// MaxSnapshotBytes is the whole-snapshot memory budget; a snapshot that
	// would exceed it degrades (TooLarge / Truncated), never freezes.
	MaxSnapshotBytes = 64 << 20 // 64 MiB
	// MaxPatchBytesPerFile bounds the rendered patch retained per file.
	MaxPatchBytesPerFile = 4 << 20 // 4 MiB
	// MaxUntrackedFiles bounds individually listed untracked entries.
	MaxUntrackedFiles = 1000
)

// OpClass separates read policy from mutation policy.
type OpClass string

const (
	OpRead   OpClass = "read"
	OpMutate OpClass = "mutate"
)

// ErrClass is a coarse, UI-safe classification of a Git failure.
type ErrClass string

const (
	ErrGitMissing     ErrClass = "git_missing"
	ErrNotARepo       ErrClass = "not_a_repo"
	ErrStaleState     ErrClass = "stale_state"
	ErrHookFailed     ErrClass = "hook_failed"
	ErrConflict       ErrClass = "conflict"
	ErrMergeInProgess ErrClass = "merge_in_progress"
	ErrRefusal        ErrClass = "git_refusal"
	ErrLocked         ErrClass = "index_locked"
	ErrTimeout        ErrClass = "timeout"
	ErrTooLarge       ErrClass = "too_large"
	ErrCancelled      ErrClass = "cancelled"
	ErrOther          ErrClass = "other"
)

// Error is a structured Git failure: it keeps the operation class and the
// raw stderr for surfacing in the UI, and a coarse class for branching.
type Error struct {
	Class    ErrClass
	OpClass  OpClass
	Op       string // git subcommand, e.g. "diff", "commit"
	ExitCode int
	Stderr   string // truncated; UI display only, never logged by this package
	err      error
}

func (e *Error) Error() string {
	if e.err != nil {
		return fmt.Sprintf("git %s: %v", e.Op, e.err)
	}
	detail := e.Stderr
	if len(detail) > 400 {
		detail = detail[:400]
	}
	if detail != "" {
		return fmt.Sprintf("git %s failed (%s): %s", e.Op, e.Class, detail)
	}
	return fmt.Sprintf("git %s failed (%s)", e.Op, e.Class)
}

func (e *Error) Unwrap() error { return e.err }

// GitError extracts the structured *Error from err, if any.
func GitError(err error) (*Error, bool) {
	var ge *Error
	if errors.As(err, &ge) {
		return ge, true
	}
	return nil, false
}

// classifyErr maps stderr text to a coarse class. It never inspects content
// beyond Git's own protocol words.
func classifyErr(stderr string) ErrClass {
	switch {
	case containsAny(stderr, "fatal: not a git repository"):
		return ErrNotARepo
	case containsAny(stderr, "cannot do a partial commit during a merge",
		"you are in the middle of a merge", "CHERRY_PICK_HEAD", "REVERT_HEAD"):
		return ErrMergeInProgess
	case containsAny(stderr, "hook"):
		return ErrHookFailed
	case containsAny(stderr, "conflict", "would be overwritten", "Please commit your changes"):
		return ErrConflict
	case containsAny(stderr, "index.lock"):
		return ErrLocked
	case containsAny(stderr, "error: Your local changes", "could not", "does not allow"):
		return ErrRefusal
	default:
		return ErrOther
	}
}

func containsAny(s string, subs ...string) bool {
	for _, sub := range subs {
		if sub != "" && contains(s, sub) {
			return true
		}
	}
	return false
}

func contains(s, sub string) bool {
	return len(sub) > 0 && len(s) >= len(sub) && indexOf(s, sub) >= 0
}

func indexOf(s, sub string) int {
	for i := 0; i+len(sub) <= len(s); i++ {
		if s[i:i+len(sub)] == sub {
			return i
		}
	}
	return -1
}

// Runner owns every Git subprocess this package spawns. The zero value is
// usable; GitBinary overrides the executable in tests.
type Runner struct {
	// GitBinary defaults to "git".
	GitBinary string
	// Now overrides the clock for tests.
	Now func() time.Time

	mu       sync.Mutex
	mutating map[string]bool // repo root -> mutation in flight
}

func (r *Runner) binary() string {
	if r.GitBinary != "" {
		return r.GitBinary
	}
	return "git"
}

func (r *Runner) now() time.Time {
	if r.Now != nil {
		return r.Now()
	}
	return time.Now()
}

// readEnv is the stable read environment: no pagers, no prompts, no optional
// index locks, C locale for machine-stable output.
func readEnv() []string {
	return []string{
		"GIT_OPTIONAL_LOCKS=0",
		"GIT_PAGER=cat",
		"GIT_TERMINAL_PROMPT=0",
		"LC_ALL=C",
	}
}

// mutateEnv drops GIT_OPTIONAL_LOCKS (mutations may legitimately lock) and
// keeps the no-pager/no-prompt/C-locale guarantees.
func mutateEnv() []string {
	return []string{
		"GIT_PAGER=cat",
		"GIT_TERMINAL_PROMPT=0",
		"LC_ALL=C",
	}
}

// Read runs one bounded, read-only Git command in root. It never mutates
// worktree or index state (GIT_OPTIONAL_LOCKS=0 keeps even refreshes lockless).
func (r *Runner) Read(ctx context.Context, root string, args ...string) ([]byte, error) {
	return r.run(ctx, OpRead, ReadTimeout, MaxReadOutput, readEnv(), root, args)
}

// Mutate runs one explicit mutation command single-flight per repo root.
// Callers must have already revalidated state; the runner only serializes.
func (r *Runner) Mutate(ctx context.Context, root string, args ...string) ([]byte, error) {
	unlock, err := r.LockRepo(root, first(args))
	if err != nil {
		return nil, err
	}
	defer unlock()

	out, err := r.run(ctx, OpMutate, MutateTimeout, MaxReadOutput, mutateEnv(), root, args)
	if err != nil {
		// Roll the mutation into the error context for classification.
		if ge, ok := GitError(err); ok && ge.Class == ErrOther {
			ge.Class = classifyErr(ge.Stderr)
		}
	}
	return out, err
}

// Stdin runs one mutation with stdin content (commit message transport). The
// message travels by pipe, never by argv, avoiding argv length limits and
// keeping message text out of process listings.
func (r *Runner) Stdin(ctx context.Context, root, stdin string, args ...string) ([]byte, error) {
	unlock, err := r.LockRepo(root, first(args))
	if err != nil {
		return nil, err
	}
	defer unlock()

	out, err := r.runStdin(ctx, OpMutate, MutateTimeout, root, stdin, args)
	if err != nil {
		if ge, ok := GitError(err); ok && ge.Class == ErrOther {
			ge.Class = classifyErr(ge.Stderr)
		}
	}
	return out, err
}

// LockRepo serializes mutations per canonical repository root.
func (r *Runner) LockRepo(root, op string) (func(), error) {
	clean := filepath.Clean(root)
	if canonical, err := filepath.EvalSymlinks(clean); err == nil {
		clean = canonical
	}

	r.mu.Lock()
	if r.mutating == nil {
		r.mutating = map[string]bool{}
	}
	if r.mutating[clean] {
		r.mu.Unlock()
		return nil, &Error{Class: ErrOther, OpClass: OpMutate, Op: op, err: errors.New("another mutation is already in flight for this repository")}
	}
	r.mutating[clean] = true
	r.mu.Unlock()

	return func() {
		r.mu.Lock()
		delete(r.mutating, clean)
		r.mu.Unlock()
	}, nil
}

func first(args []string) string {
	if len(args) > 0 {
		return args[0]
	}
	return ""
}

func (r *Runner) run(ctx context.Context, class OpClass, timeout time.Duration, maxOut int64, env []string, root string, args []string) ([]byte, error) {
	if deadline, ok := ctx.Deadline(); !ok || time.Until(deadline) > timeout {
		var cancel context.CancelFunc
		ctx, cancel = context.WithTimeout(ctx, timeout)
		defer cancel()
	}
	cmd := exec.CommandContext(ctx, r.binary(), args...)
	cmd.Dir = root
	cmd.Env = append(environ(), env...)
	// WaitDelay unblocks Run when a killed Git leaves pipe-holding children
	// (hooks, pagers); without it the 5s read deadline could stall forever.
	cmd.WaitDelay = 500 * time.Millisecond

	var stdout bytes.Buffer
	var stderrBuf bytes.Buffer
	cmd.Stderr = &limitedWriter{buf: &stderrBuf, limit: 64 << 10} // 64 KiB cap on stderr
	stdoutLimiter := &limitedWriter{buf: &stdout, limit: maxOut}
	cmd.Stdout = stdoutLimiter
	err := cmd.Run()
	if stdoutLimiter.overflow {
		return nil, &Error{Class: ErrTooLarge, OpClass: class, Op: first(args), err: errors.New("command output exceeded size limit")}
	}
	if err != nil {
		ge := &Error{
			OpClass:  class,
			Op:       first(args),
			ExitCode: exitCode(err),
			Stderr:   truncate(stderrBuf.String(), 8000),
			err:      err,
		}
		switch {
		case errors.Is(ctx.Err(), context.DeadlineExceeded):
			ge.Class = ErrTimeout
		case errors.Is(ctx.Err(), context.Canceled):
			ge.Class = ErrCancelled
		case isExecNotFound(err):
			ge.Class = ErrGitMissing
		default:
			ge.Class = classifyErr(ge.Stderr)
		}
		return nil, ge
	}
	if stderrBuf.Len() > 0 && stdout.Len() == 0 {
		// Some Git informational paths write to stderr with exit 0.
		return stdout.Bytes(), nil
	}
	return stdout.Bytes(), nil
}

func (r *Runner) runStdin(ctx context.Context, class OpClass, timeout time.Duration, root, stdin string, args []string) ([]byte, error) {
	if deadline, ok := ctx.Deadline(); !ok || time.Until(deadline) > timeout {
		var cancel context.CancelFunc
		ctx, cancel = context.WithTimeout(ctx, timeout)
		defer cancel()
	}
	cmd := exec.CommandContext(ctx, r.binary(), args...)
	cmd.Dir = root
	cmd.Env = append(environ(), mutateEnv()...)
	cmd.WaitDelay = 500 * time.Millisecond
	cmd.Stdin = bytes.NewReader([]byte(stdin))

	var stdout bytes.Buffer
	var stderrBuf bytes.Buffer
	cmd.Stderr = &limitedWriter{buf: &stderrBuf, limit: 64 << 10}
	stdoutLimiter := &limitedWriter{buf: &stdout, limit: MaxReadOutput}
	cmd.Stdout = stdoutLimiter
	err := cmd.Run()
	if stdoutLimiter.overflow {
		return nil, &Error{Class: ErrTooLarge, OpClass: class, Op: first(args), err: errors.New("command output exceeded size limit")}
	}
	if err != nil {
		ge := &Error{
			Class:    classifyErr(stderrBuf.String()),
			OpClass:  class,
			Op:       first(args),
			ExitCode: exitCode(err),
			Stderr:   truncate(stderrBuf.String(), 8000),
			err:      err,
		}
		switch {
		case errors.Is(ctx.Err(), context.DeadlineExceeded):
			ge.Class = ErrTimeout
		case errors.Is(ctx.Err(), context.Canceled):
			ge.Class = ErrCancelled
		case isExecNotFound(err):
			ge.Class = ErrGitMissing
		}
		return nil, ge
	}
	return stdout.Bytes(), nil
}

// limitedWriter caps captured output and flags overflow.
type limitedWriter struct {
	buf      *bytes.Buffer
	limit    int64
	n        int64
	overflow bool
}

func (w *limitedWriter) Write(p []byte) (int, error) {
	room := w.limit - w.n
	if room <= 0 {
		w.overflow = true
		return len(p), nil // swallow the rest, keep process running
	}
	if int64(len(p)) > room {
		w.buf.Write(p[:room])
		w.n += room
		w.overflow = true
		return len(p), nil
	}
	w.buf.Write(p)
	w.n += int64(len(p))
	return len(p), nil
}

func truncate(s string, n int) string {
	if len(s) <= n {
		return s
	}
	return s[:n]
}

// exitCode extracts the subprocess exit status from an exec error.
func exitCode(err error) int {
	var exitErr *exec.ExitError
	if errors.As(err, &exitErr) {
		return exitErr.ExitCode()
	}
	return -1
}

// isExecNotFound reports whether the Git executable itself was missing.
func isExecNotFound(err error) bool {
	var execErr *exec.Error
	return errors.As(err, &execErr)
}

// environ returns the parent environment; the indirection lets tests pin a
// deterministic environment.
var environ = os.Environ
