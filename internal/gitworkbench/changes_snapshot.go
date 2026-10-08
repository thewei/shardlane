package gitworkbench

import (
	"context"
	"strings"
)

// Snapshot acquires the working-tree change snapshot for root: working tree
// + index vs HEAD, plus untracked files (plan §14). Repository without HEAD
// compares to the empty tree.
func (r *Runner) Snapshot(ctx context.Context, root string) (*ChangesSnapshot, error) {
	facts, err := r.Facts(ctx, root)
	if err != nil {
		return &ChangesSnapshot{Root: root, Err: err.Error(), GeneratedAt: r.now()}, err
	}
	snap := &ChangesSnapshot{
		Root:        root,
		Head:        facts.Head,
		Branch:      facts.Branch,
		NoHead:      facts.NoHead,
		Signature:   facts.Signature,
		GeneratedAt: r.now(),
	}

	porcelain, err := r.Read(ctx, root, "status", "--porcelain=v1", "-z", "--untracked-files=all")
	if err != nil {
		snap.Err = err.Error()
		return snap, err
	}
	entries := parsePorcelainZ(porcelain)

	// Untracked census first: bounded before any content reads.
	var untracked []string
	for _, e := range entries {
		if containsAny(e.XY, "?") {
			untracked = append(untracked, e.Path)
		}
	}
	snap.TruncatedUntracked = len(untracked)
	budget := &snapshotBudget{limit: MaxSnapshotBytes}

	trackedPatch := []byte(nil)
	numstat := map[string]numstatLine{}
	stagedHunks := map[string][]Hunk{}
	unstagedHunks := map[string][]Hunk{}
	hasStaged, hasUnstaged := false, false
	for _, e := range entries {
		if containsAny(e.XY, "?") {
			continue
		}
		if e.XY[0] != ' ' {
			hasStaged = true
		}
		if e.XY[1] != ' ' {
			hasUnstaged = true
		}
	}
	if len(entries) > len(untracked) {
		// The tree-vs-HEAD patch (index + worktree combined), no color,
		// no external diff, rename detection on (plan §14).
		diffRef := "HEAD"
		if facts.NoHead {
			diffRef = emptyTreeID(ctx, r, root)
		}
		patch, err := r.Read(ctx, root,
			"diff", "--no-color", "--no-ext-diff", "--no-textconv", "-M", "-U3",
			"--submodule=short", diffRef)
		if err != nil {
			snap.Err = err.Error()
			return snap, err
		}
		if !budget.take(int64(len(patch))) {
			snap.Truncated = true
		}
		trackedPatch = patch

		// NUL-delimited numstat carries arbitrary filenames verbatim (P1-07).
		ns, err := r.Read(ctx, root, "diff", "--numstat", "-z", "-M", diffRef)
		if err == nil {
			numstat = parseNumstatZ(ns)
		}

		// The per-side parses behind the staged/unstaged split views, only
		// when a side actually carries changes.
		if hasStaged {
			if p, err := r.Read(ctx, root,
				"diff", "--cached", "--no-color", "--no-ext-diff", "--no-textconv", "-M", "-U3"); err == nil {
				stagedHunks = hunksByPath(parsePatchSet(p))
			}
		}
		if hasUnstaged {
			if p, err := r.Read(ctx, root,
				"diff", "--no-color", "--no-ext-diff", "--no-textconv", "-M", "-U3"); err == nil {
				unstagedHunks = hunksByPath(parsePatchSet(p))
			}
		}
	}

	patches := parsePatchSet(trackedPatch)

	// Emit tracked entries in porcelain order.
	files, adds, dels := emitChangeFiles(entries, patches, numstat, stagedHunks, unstagedHunks)
	snap.Files, snap.TotalAdditions, snap.TotalDeletions = files, adds, dels

	// Untracked entries: content read per file, bounded (plan §15).
	sortStrings(untracked)
	if len(untracked) > MaxUntrackedFiles {
		untracked = untracked[:MaxUntrackedFiles]
		snap.Truncated = true
	}
	for _, path := range untracked {
		cf, ok := untrackedEntry(root, path)
		if !ok {
			continue
		}
		if !budget.take(int64(len(cf.Patch))) { // budget exhausted: degrade, stop reading
			snap.Truncated = true
			break
		}
		cf.Generated = IsGenerated(path)
		snap.Files = append(snap.Files, cf)
		snap.TotalAdditions += cf.Additions
	}

	sortFilesByPath(snap.Files)
	return snap, nil
}

// emitChangeFiles projects parsed status entries, patches and numstat into
// the snapshot's file list. Shared by the work-tree snapshot and the commit
// snapshot (whose entries carry the status in the first column only).
func emitChangeFiles(entries []porcelainEntry, patches map[string]*filePatch, numstat map[string]numstatLine, stagedHunks, unstagedHunks map[string][]Hunk) (files []ChangeFile, totalAdditions, totalDeletions int) {
	for _, e := range entries {
		if containsAny(e.XY, "?") {
			continue
		}
		status := statusToChange(e.XY)
		cf := ChangeFile{
			Path:          e.Path,
			OldPath:       e.OldPath,
			Status:        status,
			Staged:        e.XY[0] != ' ',
			Unstaged:      e.XY[1] != ' ',
			StagedHunks:   stagedHunks[e.Path],
			UnstagedHunks: unstagedHunks[e.Path],
		}
		if ns, ok := numstat[e.Path]; ok {
			cf.Additions = int(ns.Add)
			cf.Deletions = int(ns.Del)
			cf.Binary = ns.Binary
		}
		if fp, ok := patches[e.Path]; ok {
			cf.Patch = fp.raw
			cf.Hunks = fp.hunks
			cf.Binary = cf.Binary || fp.binary
			if fp.binary {
				cf.Additions, cf.Deletions = 0, 0
			}
			if len(fp.raw) > MaxPatchBytesPerFile {
				cf.TooLarge = true
				cf.Patch = nil
				cf.Hunks = nil
			}
		} else if status == StatusDeleted {
			cf.Binary = false
		}
		cf.Generated = IsGenerated(e.Path)
		cf.Fingerprint = fingerprint(status, cf.Path, cf.OldPath, cf.Additions, cf.Deletions, cf.Patch)
		files = append(files, cf)
		totalAdditions += cf.Additions
		totalDeletions += cf.Deletions
	}
	return files, totalAdditions, totalDeletions
}

// snapshotBudget tracks the whole-snapshot memory budget.
type snapshotBudget struct{ used, limit int64 }

// hunksByPath flattens a parsed patch set into per-path hunk lists.
func hunksByPath(set map[string]*filePatch) map[string][]Hunk {
	out := make(map[string][]Hunk, len(set))
	for path, fp := range set {
		out[path] = fp.hunks
	}
	return out
}

func (b *snapshotBudget) take(n int64) bool {
	if b.used+n > b.limit {
		return false
	}
	b.used += n
	return true
}

// emptyTreeID resolves Git's empty tree for the repository's hash algorithm.
func emptyTreeID(ctx context.Context, r *Runner, root string) string {
	out, err := r.Read(ctx, root, "hash-object", "-t", "tree", "--stdin")
	if err == nil {
		if id := strings.TrimSpace(string(out)); id != "" {
			return id
		}
	}
	return "4b825dc642cb6eb9a060e54bf8d69288fbee4904" // SHA-1 empty tree fallback
}
