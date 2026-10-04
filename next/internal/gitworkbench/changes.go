package gitworkbench

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"time"
)

// ChangeStatus is the coarse working-tree change kind for one path.
type ChangeStatus string

const (
	StatusModified   ChangeStatus = "modified"
	StatusAdded      ChangeStatus = "added"
	StatusDeleted    ChangeStatus = "deleted"
	StatusRenamed    ChangeStatus = "renamed"
	StatusCopied     ChangeStatus = "copied"
	StatusTypeChange ChangeStatus = "typechange"
	StatusConflicted ChangeStatus = "conflicted"
	StatusUntracked  ChangeStatus = "untracked"
)

// Letter is the one-character status code used in tree rows (M/A/D/R/C/T/U/X).
func (s ChangeStatus) Letter() string {
	switch s {
	case StatusModified:
		return "M"
	case StatusAdded:
		return "A"
	case StatusDeleted:
		return "D"
	case StatusRenamed:
		return "R"
	case StatusCopied:
		return "C"
	case StatusTypeChange:
		return "T"
	case StatusConflicted:
		return "X"
	case StatusUntracked:
		return "U"
	}
	return "?"
}

// ChangeFile models one changed path in the working tree (plan §16 model).
type ChangeFile struct {
	Path        string
	OldPath     string // rename/copy source
	Status      ChangeStatus
	Additions   int
	Deletions   int
	Binary      bool
	Generated   bool
	TooLarge    bool
	Untracked   bool
	Fingerprint string
	Patch       []byte // parsed-model source; nil when TooLarge/Binary
	Hunks       []Hunk
	Truncated   bool // patch retained below full size
	// Staged/Unstaged mark which sides of git's status carry the change,
	// driving the Stage/Unstage file actions. Both false for untracked.
	Staged   bool
	Unstaged bool
	// StagedHunks and UnstagedHunks are the per-side parses (HEAD vs index,
	// index vs worktree) behind the staged/unstaged split views; each nil
	// when that side carries no change or the side patch failed.
	StagedHunks   []Hunk
	UnstagedHunks []Hunk
}

// ChangesSnapshot is the immutable fact set for one repository root at one
// moment (plan §14/§21). Renders never recompute it; the cache owns refresh.
type ChangesSnapshot struct {
	Root      string
	Head      string
	Branch    string
	NoHead    bool
	Signature string
	Files     []ChangeFile

	TotalAdditions int
	TotalDeletions int

	// Truncated marks whole-snapshot degradation (untracked > 1000 or the
	// memory budget overflow); UI shows an explicit degraded note.
	Truncated          bool
	TruncatedUntracked int

	GeneratedAt time.Time
	Err         string
}

// ByPath returns the change record for path, if present.
func (s *ChangesSnapshot) ByPath(path string) (ChangeFile, bool) {
	for i := range s.Files {
		if s.Files[i].Path == path {
			return s.Files[i], true
		}
	}
	return ChangeFile{}, false
}

// Healthy reports whether the snapshot carries usable facts.
func (s *ChangesSnapshot) Healthy() bool { return s.Err == "" && s.Root != "" }

// Fingerprint derives a per-file staleness identity: status + paths + line
// stats + patch digest. A changed fingerprint means the visible diff no
// longer describes the file and the commit fence must fail closed.
// It is a presentation/cache identity only — never the commit safety
// authority (that is the live object/content identity, see commit.go).
func fingerprint(status ChangeStatus, path, oldPath string, add, del int, patch []byte) string {
	h := sha256.New()
	fmt.Fprintf(h, "%s\x00%s\x00%s\x00%d\x00%d\x00", status, path, oldPath, add, del)
	if len(patch) > 0 {
		n := len(patch)
		if n > 64<<10 {
			n = 64 << 10
		}
		h.Write(patch[:n])
		fmt.Fprintf(h, "\x00%d", len(patch))
	}
	return hex.EncodeToString(h.Sum(nil))[:16]
}
