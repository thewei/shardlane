package history

import (
	"context"
	"fmt"
	"log/slog"
	"os"
	"path/filepath"
)

// SourceRoot is one provider-owned data root the scanner observes. Shardlane
// never writes to it.
type SourceRoot struct {
	Agent     AgentID
	Directory string
	NativeID  func(stem string) string
}

// DefaultRoots resolves the default provider data roots under the given home
// directory. CODEX_HOME overrides the Codex root when set.
func DefaultRoots(home string) []SourceRoot {
	roots := []SourceRoot{{
		Agent:     AgentClaudeCode,
		Directory: filepath.Join(home, ".claude", "projects"),
		NativeID:  func(stem string) string { return stem },
	}}
	codexHome := os.Getenv("CODEX_HOME")
	if codexHome == "" {
		codexHome = filepath.Join(home, ".codex")
	}
	rollout := rolloutNativeID
	roots = append(roots,
		SourceRoot{Agent: AgentCodex, Directory: filepath.Join(codexHome, "sessions"), NativeID: rollout},
		SourceRoot{Agent: AgentCodex, Directory: filepath.Join(codexHome, "archived_sessions"), NativeID: rollout},
		SourceRoot{Agent: AgentPi, Directory: filepath.Join(home, ".pi", "agent", "sessions"), NativeID: piRolloutNativeID},
		SourceRoot{Agent: AgentOMP, Directory: filepath.Join(home, ".omp", "agent", "sessions"), NativeID: piRolloutNativeID},
	)
	return roots
}

// ScanResult summarizes one scanner round.
type ScanResult struct {
	Scanned int
	Changed int
	Removed int
}

// Scanner reconciles the disposable catalog with the observed provider
// sources. Missing roots are legitimate empty sources; a root that exists but
// cannot be read aborts the round so cleanup never follows an incomplete
// observation.
type Scanner struct {
	catalog *Catalog
	roots   []SourceRoot
	parse   func(reference SessionFileRef) (ParsedTranscript, error)
}

func NewScanner(catalog *Catalog, roots []SourceRoot) *Scanner {
	return &Scanner{catalog: catalog, roots: roots, parse: parseByAgent}
}

// parseByAgent dispatches to the shipped adapters. Unknown providers fail
// closed instead of being guessed.
func parseByAgent(reference SessionFileRef) (ParsedTranscript, error) {
	switch reference.Agent {
	case AgentClaudeCode:
		return ParseClaudeTranscript(reference)
	case AgentCodex:
		return ParseCodexTranscript(reference)
	case AgentPi, AgentOMP:
		return ParsePiTranscript(reference)
	default:
		return ParsedTranscript{}, fmt.Errorf("no history adapter for provider %q", reference.Agent)
	}
}

// Scan runs one reconciliation round: list roots, parse changed sources once
// (metadata + FTS + page cache), then remove sessions whose files were not
// observed. Cancelled rounds return ctx.Err() without cleanup.
func (s *Scanner) Scan(ctx context.Context) (ScanResult, error) {
	result := ScanResult{}

	known, err := s.catalog.KnownFiles()
	if err != nil {
		return result, err
	}
	seen := make(map[string]bool)
	for _, root := range s.roots {
		references, err := listJSONLRefs(root.Directory, root.Agent, root.NativeID)
		if err != nil {
			return result, fmt.Errorf("observe root %s: %w", root.Directory, err)
		}
		for _, reference := range references {
			if err := ctx.Err(); err != nil {
				return result, err
			}
			seen[reference.FilePath] = true
			result.Scanned++
			if mtime, ok := known[reference.FilePath]; ok && mtime == reference.MtimeMS {
				continue
			}
			if err := s.indexReference(ctx, reference); err != nil {
				return result, err
			}
			result.Changed++
		}
	}
	removed, err := s.catalog.RemoveMissing(seen)
	if err != nil {
		return result, err
	}
	result.Removed = removed
	return result, nil
}

// indexReference parses the changed source exactly once, writes session
// metadata/FTS, prewarms the page cache, and drops the full transcript.
func (s *Scanner) indexReference(ctx context.Context, reference SessionFileRef) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	transcript, err := s.parse(reference)
	if err != nil {
		// An unparseable source is a scan error for this round, not a
		// reason to corrupt or clean the catalog.
		return fmt.Errorf("parse %s source %s: %w", reference.Agent, reference.FilePath, err)
	}
	units := unitsFromMessages(transcript.Mainline)
	if err := s.catalog.WriteSession(transcript.Meta, reference.MtimeMS, units); err != nil {
		return err
	}
	if err := s.catalog.CacheTranscript(reference, transcript); err != nil {
		return err
	}
	slog.Debug("indexed history source", "agent", string(reference.Agent), "session", transcript.Meta.Key, "messages", len(transcript.Mainline))
	return nil
}
