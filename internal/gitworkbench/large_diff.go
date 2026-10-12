package gitworkbench

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
)

// Large diffs wait behind a Show button. A file past LargeDiffLines changed
// lines (or whose patch outgrows MaxPatchBytesPerFile) keeps its counts in
// the snapshot and loses its patch; the bulk reads leave the biggest of them
// out by pathspec so one lockfile cannot fail the whole review; and
// ReadFileDiff reads a single deferred file when the user asks for it.

// emitOptions tune emitChangeFiles.
type emitOptions struct {
	// Root is the working tree whose file stats stand in for a deferred
	// file's patch in its fingerprint; empty for a commit, which cannot change.
	Root string
	// DeferAll defers every text file: the bulk patch was too large to read.
	DeferAll bool
}

// largeDiffExcludes is the pathspec tail that keeps the files whose numstat is
// past LargeDiffLines out of a bulk diff read, nil when there are none. A
// renamed file goes under both names, or its source would arrive as one
// enormous deletion. Past maxDiffExcludes files the rest are read, and
// deferred after.
func largeDiffExcludes(entries []porcelainEntry, numstat map[string]numstatLine) []string {
	var skip []string
	for _, e := range entries {
		ns, ok := numstat[e.Path]
		if !ok || ns.Binary || ns.Add+ns.Del <= LargeDiffLines {
			continue
		}
		if len(skip) >= maxDiffExcludes {
			break
		}
		skip = append(skip, ":(exclude,literal)"+e.Path)
		if e.OldPath != "" && e.OldPath != e.Path {
			skip = append(skip, ":(exclude,literal)"+e.OldPath)
		}
	}
	if len(skip) == 0 {
		return nil
	}
	return append([]string{"--", "."}, skip...)
}

// readDiff runs one diff read without the files in skip. Output past
// MaxReadOutput is not an error here: whole is false and nothing was read.
func (r *Runner) readDiff(ctx context.Context, root string, skip []string, args ...string) (out []byte, whole bool, err error) {
	out, err = r.Read(ctx, root, slices.Concat(args, skip)...)
	if ge, ok := GitError(err); ok && ge.Class == ErrTooLarge {
		return nil, false, nil
	}
	return out, err == nil, err
}

// fileStamp is a file's size and modification time, as bytes.
func fileStamp(info os.FileInfo) []byte {
	return []byte(fmt.Sprintf("deferred\x00%d\x00%d", info.Size(), info.ModTime().UnixNano()))
}

// deferredStamp stands in for the patch of a deferred file in its
// fingerprint, so editing the file moves the identity even when the line
// counts do not change.
func deferredStamp(root, path string) []byte {
	if root != "" {
		if info, err := os.Lstat(filepath.Join(root, path)); err == nil {
			return fileStamp(info)
		}
	}
	return []byte("deferred")
}

// FileDiff is what an on-demand read of one deferred file produced: its
// combined hunks, and each side's when changes sit staged and unstaged at
// once (otherwise the combined hunks are the side).
type FileDiff struct {
	Hunks         []Hunk
	StagedHunks   []Hunk
	UnstagedHunks []Hunk
}

// ReadFileDiff reads the diff of one deferred file: through Git for a tracked
// path, against base (the DiffBase of its snapshot), and from the disk for an
// untracked one. The output is bounded by MaxOnDemandPatchBytes; past it the
// error is ErrTooLarge.
func (r *Runner) ReadFileDiff(ctx context.Context, root, base string, cf ChangeFile) (FileDiff, error) {
	if cf.Untracked {
		return readUntrackedDiff(root, cf.Path)
	}
	spec := []string{"--", ":(literal)" + cf.Path}
	if cf.OldPath != "" && cf.OldPath != cf.Path {
		spec = append(spec, ":(literal)"+cf.OldPath)
	}
	read := func(args ...string) ([]Hunk, error) {
		out, err := r.run(ctx, OpRead, OnDemandTimeout, MaxOnDemandPatchBytes, readEnv(), root, slices.Concat(args, spec))
		if err != nil {
			return nil, err
		}
		fp := parsePatchSetLimit(out, MaxOnDemandPatchBytes)[cf.Path]
		switch {
		case fp == nil:
			return nil, nil
		case fp.tooLarge:
			return nil, errDiffTooLarge()
		}
		return fp.hunks, nil
	}

	var d FileDiff
	var err error
	if d.Hunks, err = read("diff", "--no-color", "--no-ext-diff", "--no-textconv", "-M", "-U3", "--submodule=short", base); err != nil {
		return FileDiff{}, err
	}
	if cf.Staged && cf.Unstaged {
		if d.StagedHunks, err = read("diff", "--cached", "--no-color", "--no-ext-diff", "--no-textconv", "-M", "-U3"); err != nil {
			return FileDiff{}, err
		}
		if d.UnstagedHunks, err = read("diff", "--no-color", "--no-ext-diff", "--no-textconv", "-M", "-U3"); err != nil {
			return FileDiff{}, err
		}
	}
	return d, nil
}

// readUntrackedDiff reads an untracked file from the disk as the addition it is.
func readUntrackedDiff(root, path string) (FileDiff, error) {
	cf, ok := untrackedEntryWith(root, path, true)
	switch {
	case !ok:
		return FileDiff{}, &Error{Class: ErrStaleState, OpClass: OpRead, Op: "diff", err: errors.New("the file is gone")}
	case cf.TooLarge:
		return FileDiff{}, errDiffTooLarge()
	}
	return FileDiff{Hunks: cf.Hunks}, nil
}

func errDiffTooLarge() error {
	return &Error{Class: ErrTooLarge, OpClass: OpRead, Op: "diff", err: errors.New("diff is too large to show")}
}
