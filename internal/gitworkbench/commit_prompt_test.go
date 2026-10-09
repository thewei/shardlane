package gitworkbench

import (
	"strings"
	"testing"
)

func TestCommitMessagePromptBoundedSelectedOnly(t *testing.T) {
	snap := &ChangesSnapshot{Files: []ChangeFile{
		{Path: "chosen.go", Status: StatusModified, Additions: 1, Hunks: []Hunk{{Lines: []DiffLine{{Kind: KindAdd, Text: "func example() {}"}}}}},
		{Path: "private.key", Status: StatusAdded, Additions: 1, Hunks: []Hunk{{Lines: []DiffLine{{Kind: KindAdd, Text: "secret material"}}}}},
	}}
	prompt := CommitMessagePrompt(snap, []string{"chosen.go"})
	if !strings.Contains(prompt, "chosen.go") || strings.Contains(prompt, "func example") {
		t.Fatalf("legacy prompt must default to metadata-only: %s", prompt)
	}
	if strings.Contains(prompt, "private.key") || strings.Contains(prompt, "secret material") {
		t.Fatal("non-selected content must never enter the copied prompt")
	}
	if !strings.Contains(prompt, "untrusted data") {
		t.Fatal("prompt must treat patch content as untrusted")
	}
	if got := CommitMessagePrompt(snap, []string{"nonexistent"}); got != "" {
		t.Fatal("no valid selected path should return an empty prompt")
	}
	if got := CommitMessagePrompt(nil, []string{"chosen.go"}); got != "" {
		t.Fatal("nil review should not produce a prompt")
	}
}

func TestCommitMessagePromptHardLimit(t *testing.T) {
	lines := make([]DiffLine, 2000)
	for i := range lines {
		lines[i] = DiffLine{Kind: KindAdd, Text: strings.Repeat("A", 80)}
	}
	snap := &ChangesSnapshot{Files: []ChangeFile{{Path: "large.go", Status: StatusModified, Hunks: []Hunk{{Lines: lines}}}}}
	preview := PrepareCommitPrompt(snap, []string{"large.go"}, true)
	if len(preview.Text) > MaxCommitPromptBytes || !preview.Truncated {
		t.Fatalf("oversized/non-truncated opt-in prompt: %d; truncated=%v", len(preview.Text), preview.Truncated)
	}
}
