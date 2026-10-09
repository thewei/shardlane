package gitworkbench

import (
	"strings"
	"testing"
)

/**
 * [INPUT]: selected Git changes including confidential paths and untrusted diff content
 * [OUTPUT]: offline preview redaction, default metadata-only, hard bound and opt-in excerpt checks
 * [POS]: Git AI handoff domain regression; clipboard/AI never called by this package
 * [PROTOCOL]: update header and check CLAUDE.md
 */

func TestPrepareCommitPromptPrivacyAndOptIn(t *testing.T) {
	snap := &ChangesSnapshot{Files: []ChangeFile{
		{Path: "src/feature.go", Status: StatusModified, Additions: 1, Hunks: []Hunk{{Lines: []DiffLine{{Kind: KindAdd, Text: "const businessSecret = 8675309"}}}}},
		{Path: ".env.production", Status: StatusModified, Hunks: []Hunk{{Lines: []DiffLine{{Kind: KindAdd, Text: "API_KEY=very-private"}}}}},
		{Path: "config/secrets/token.txt", Status: StatusModified},
		{Path: "keys/service.pem", Status: StatusAdded},
	}}
	selected := []string{"src/feature.go", ".env.production", "config/secrets/token.txt", "keys/service.pem"}
	defaultPreview := PrepareCommitPrompt(snap, selected, false)
	if defaultPreview.IncludedFiles != 1 || defaultPreview.ExcludedFiles != 3 {
		t.Fatalf("selection summary: %+v", defaultPreview)
	}
	if strings.Contains(defaultPreview.Text, "businessSecret") || strings.Contains(defaultPreview.Text, "API_KEY") ||
		strings.Contains(defaultPreview.Text, ".env") || strings.Contains(defaultPreview.Text, "secrets/") ||
		strings.Contains(defaultPreview.Text, ".pem") {
		t.Fatalf("default preview leaked secrets/source: %q", defaultPreview.Text)
	}
	if !strings.Contains(defaultPreview.Text, "src/feature.go") {
		t.Fatalf("non-sensitive path omitted: %q", defaultPreview.Text)
	}
	optIn := PrepareCommitPrompt(snap, selected, true)
	if !strings.Contains(optIn.Text, "businessSecret") || strings.Contains(optIn.Text, "API_KEY") {
		t.Fatalf("opt-in should include safe-selected code but never sensitive paths: %q", optIn.Text)
	}
}

func TestPrepareCommitPromptRejectsOnlySensitiveSelection(t *testing.T) {
	snap := &ChangesSnapshot{Files: []ChangeFile{{Path: ".aws/credentials", Status: StatusModified}}}
	got := PrepareCommitPrompt(snap, []string{".aws/credentials"}, true)
	if got.Text != "" || got.IncludedFiles != 0 || got.ExcludedFiles != 1 {
		t.Fatalf("sensitive-only prompt must be unavailable: %+v", got)
	}
}

func TestPrepareCommitPromptEnforcesLengthLimit(t *testing.T) {
	rows := make([]DiffLine, 1000)
	for i := range rows {
		rows[i] = DiffLine{Kind: KindAdd, Text: strings.Repeat("diff", 130)}
	}
	snap := &ChangesSnapshot{Files: []ChangeFile{{Path: "src/large.go", Status: StatusModified, Hunks: []Hunk{{Lines: rows}}}}}
	got := PrepareCommitPrompt(snap, []string{"src/large.go"}, true)
	if len(got.Text) > maxCommitPreviewBytes || !got.Truncated {
		t.Fatalf("preview size/truncation incorrect: size=%d truncated=%v", len(got.Text), got.Truncated)
	}
}
