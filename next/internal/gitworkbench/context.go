package gitworkbench

import (
	"context"
	"os"
	"path/filepath"
	"strings"
)

// MaxContextWindowBytes bounds one context-expansion read.
const MaxContextWindowBytes = 1 << 20 // 1 MiB

// FileLines reads a bounded window of working-tree file lines for
// unchanged-context expansion (plan §19.4). Read-only and bounded; callers
// run it on background lanes only — never from a render pass.
// start is 1-based; count lines from there.
func (r *Runner) FileLines(ctx context.Context, root, relPath string, start, count int) ([]string, error) {
	if start < 1 {
		start = 1
	}
	if count <= 0 {
		return nil, nil
	}
	full := filepath.Join(root, relPath)
	info, err := os.Stat(full)
	if err != nil || info.IsDir() {
		return nil, err
	}
	if info.Size() > MaxContextWindowBytes*4 {
		return nil, &Error{Class: ErrTooLarge, OpClass: OpRead, Op: "context"}
	}
	data, err := os.ReadFile(full)
	if err != nil {
		return nil, err
	}
	if len(data) > MaxContextWindowBytes*4 {
		data = data[:MaxContextWindowBytes*4]
	}
	lines := strings.Split(strings.ReplaceAll(string(data), "\r\n", "\n"), "\n")
	if n := len(lines); n > 0 && lines[n-1] == "" {
		lines = lines[:n-1]
	}
	if start > len(lines) {
		return nil, nil
	}
	end := start - 1 + count
	if end > len(lines) {
		end = len(lines)
	}
	return lines[start-1 : end], nil
}

// FileLineCount reports the working-tree line count for expansion bounds.
func (r *Runner) FileLineCount(root, relPath string) (int, error) {
	lines, err := r.FileLines(context.Background(), root, relPath, 1, 1<<30/2)
	if err != nil {
		return 0, err
	}
	return len(lines), nil
}
