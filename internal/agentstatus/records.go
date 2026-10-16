package agentstatus

import (
	"strings"
	"time"
)

/**
 * [INPUT]: 依赖 internal/agentstatus 的 Parse/Report 与 time，无其他项目内依赖
 * [OUTPUT]: 对外提供 Set 及其 Apply、DropTransient、Reset、Display、Transient
 * [POS]: internal/agentstatus 的终端侧记录模型：一个终端一套记录，实现 spec rev 0.4 的整体替换、clear 子树、LRU 淘汰与 app 继承；store.go 每 pane 持有一个
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// maxRecords is the spec's record cap per terminal; a report that would
// create one more evicts the least recently updated record.
const maxRecords = 256

// record is one stored report; a report replaces its record completely.
type record struct {
	rep Report
	at  time.Time
}

// Set holds one terminal's records keyed by id path, with "" as the root.
type Set struct {
	records map[string]record
}

// Apply stores one report and applies the clear semantics: clear without
// an id removes every record; clear with an id removes that record and
// everything beneath it.
func (s *Set) Apply(rep Report, now time.Time) {
	if s.records == nil {
		s.records = make(map[string]record)
	}
	if rep.State == StateClear {
		if rep.ID == "" {
			s.records = make(map[string]record)
			return
		}
		delete(s.records, rep.ID)
		prefix := rep.ID + "/"
		for id := range s.records {
			if strings.HasPrefix(id, prefix) {
				delete(s.records, id)
			}
		}
		return
	}
	if len(s.records) >= maxRecords {
		if _, exists := s.records[rep.ID]; !exists {
			s.evictOldest()
		}
	}
	s.records[rep.ID] = record{rep: rep, at: now}
}

// evictOldest removes the least recently updated record. A set at the cap
// always has one; the incoming id is never the evictee.
func (s *Set) evictOldest() {
	var oldestID string
	var oldest time.Time
	first := true
	for id, rec := range s.records {
		if first || rec.at.Before(oldest) {
			oldestID, oldest, first = id, rec.at, false
		}
	}
	if !first {
		delete(s.records, oldestID)
	}
}

// DropTransient removes working and blocked records, as process exit and
// the next shell prompt (OSC 133 A) require. done, error and idle survive.
func (s *Set) DropTransient() {
	for id, rec := range s.records {
		if rec.rep.Transient() {
			delete(s.records, id)
		}
	}
}

// Reset removes every record, as a full reset (RIS) requires.
func (s *Set) Reset() {
	s.records = nil
}

// Transient reports whether any record currently holds a transient state.
func (s *Set) Transient() bool {
	for _, rec := range s.records {
		if rec.rep.Transient() {
			return true
		}
	}
	return false
}

// displayRank orders states for the display choice: the record a user
// most needs to see wins — blocked first, then working, failures,
// completions, and idle last. Ties go to the more recent record.
func displayRank(st State) int {
	switch st {
	case StateBlocked:
		return 0
	case StateWorking:
		return 1
	case StateError:
		return 2
	case StateDone:
		return 3
	default:
		return 4
	}
}

// Display picks the record the pane header shows: the best-ranked one
// across all records — a blocked child surfaces over a working root —
// with the root record winning ties as the program's own summary. It
// resolves app inheritance from the nearest ancestor that carries an app.
func (s *Set) Display() (Report, bool) {
	if len(s.records) == 0 {
		return Report{}, false
	}
	var best record
	first := true
	for id, rec := range s.records {
		if first {
			best, first = rec, false
			continue
		}
		rank, bestRank := displayRank(rec.rep.State), displayRank(best.rep.State)
		switch {
		case rank < bestRank:
			best = rec
		case rank == bestRank && id == "":
			best = rec
		case rank == bestRank && rec.at.After(best.at):
			best = rec
		}
	}
	return s.withInheritedApp(best.rep), true
}

// withInheritedApp fills an empty app from the nearest ancestor: the
// record's own app first, then each path prefix up to the root.
func (s *Set) withInheritedApp(rep Report) Report {
	if rep.App != "" {
		return rep
	}
	for prefix := rep.ID; ; {
		if rec, ok := s.records[prefix]; ok && rec.rep.App != "" {
			rep.App = rec.rep.App
			return rep
		}
		slash := strings.LastIndexByte(prefix, '/')
		if slash < 0 {
			if rec, ok := s.records[""]; ok && rec.rep.App != "" {
				rep.App = rec.rep.App
			}
			return rep
		}
		prefix = prefix[:slash]
	}
}
