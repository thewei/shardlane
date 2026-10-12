package history

import (
	"context"
	"fmt"
	"log/slog"
	"os"
	"path/filepath"
	"runtime"
	"sync"
)

// maxScanWorkers bounds how many provider files one round parses at once:
// enough to cut a cold scan to a fraction, few enough to leave cores to the
// UI that is already drawing.
const maxScanWorkers = 4

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

// Scanner reconciles the in-memory catalog with the observed provider
// sources. Missing roots are legitimate empty sources; a root that exists but
// cannot be read aborts the round so cleanup never follows an incomplete
// observation.
type Scanner struct {
	catalog *Catalog
	roots   []SourceRoot
	parse   func(reference SessionFileRef) (ParsedTranscript, error)
	workers int
}

func NewScanner(catalog *Catalog, roots []SourceRoot) *Scanner {
	return &Scanner{catalog: catalog, roots: roots, parse: parseByAgent, workers: min(maxScanWorkers, runtime.GOMAXPROCS(0))}
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
// for their list-row metadata, then remove sessions whose files were not
// observed. Cancelled rounds return ctx.Err() without cleanup.
func (s *Scanner) Scan(ctx context.Context) (ScanResult, error) {
	result := ScanResult{}

	known := s.catalog.KnownFiles()
	seen := make(map[string]bool)
	var changed []SessionFileRef
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
			if stamp, ok := known[reference.FilePath]; ok && stamp.mtimeMS == reference.MtimeMS && stamp.size == reference.SizeBytes {
				continue
			}
			changed = append(changed, reference)
		}
	}
	indexed, err := s.indexChanged(ctx, changed)
	result.Changed = indexed
	if err != nil {
		return result, err
	}
	result.Removed = s.catalog.RemoveMissing(seen)
	return result, nil
}

// indexedRow is one parsed source waiting for its turn to enter the catalog.
type indexedRow struct {
	meta        SessionMeta
	mtimeMS     int64
	description string
	ready       bool
}

// indexChanged parses the changed sources on up to s.workers goroutines and
// commits their rows in listing order, so the catalog ends up exactly as a
// sequential round would leave it (of two files with the same session key the
// later one wins, whichever parse finishes first). The first failure cancels
// the rest of the round and is returned; the rows committed before it stay,
// and the caller skips cleanup.
func (s *Scanner) indexChanged(ctx context.Context, references []SessionFileRef) (int, error) {
	if len(references) == 0 {
		return 0, nil
	}
	round, cancel := context.WithCancel(ctx)
	defer cancel()

	var (
		mu      sync.Mutex
		rows    = make([]indexedRow, len(references))
		next    int // first row not yet committed
		failure error
		wg      sync.WaitGroup
	)
	jobs := make(chan int)
	for range max(1, min(s.workers, len(references))) {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for i := range jobs {
				row, err := s.parseRow(round, references[i])
				mu.Lock()
				switch {
				case err != nil:
					// A source that cannot be parsed fails the round, not the
					// catalog: nothing is cleaned after it.
					if failure == nil {
						failure = err
						cancel()
					}
				case failure == nil:
					rows[i] = row
					for next < len(rows) && rows[next].ready {
						s.catalog.Put(rows[next].meta, rows[next].mtimeMS, rows[next].description)
						next++
					}
				}
				mu.Unlock()
			}
		}()
	}
dispatch:
	for i := range references {
		select {
		case jobs <- i:
		case <-round.Done():
			break dispatch
		}
	}
	close(jobs)
	wg.Wait()

	if failure != nil {
		return next, failure
	}
	return next, ctx.Err()
}

// parseRow parses one changed source exactly once, keeps only the session's
// list-row metadata, and drops the transcript.
func (s *Scanner) parseRow(ctx context.Context, reference SessionFileRef) (indexedRow, error) {
	if err := ctx.Err(); err != nil {
		return indexedRow{}, err
	}
	transcript, err := s.parse(reference)
	if err != nil {
		return indexedRow{}, fmt.Errorf("parse %s source %s: %w", reference.Agent, reference.FilePath, err)
	}
	slog.Debug("indexed history source", "agent", string(reference.Agent), "session", transcript.Meta.Key, "messages", len(transcript.Mainline))
	return indexedRow{
		meta:        transcript.Meta,
		mtimeMS:     reference.MtimeMS,
		description: descriptionFromMessages(transcript.Mainline),
		ready:       true,
	}, nil
}
