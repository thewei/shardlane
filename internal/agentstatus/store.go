package agentstatus

import (
	"sync"
	"time"
)

/**
 * [INPUT]: 依赖 internal/agentstatus 的 Parse/Report/Set、sync、time
 * [OUTPUT]: 对外提供 Store 及其 NewStore、ApplyProgramReport、ApplyHookStatus、PaneClosed
 * [POS]: internal/agentstatus 的合并策略层：每 pane 一个记录集，裁定 OSC 7501 与 Hook 两条状态来源谁生效；nativeui 的 apphook 与终端回调各自喂入
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// paneState is one pane's status truth. records is the OSC 7501 record
// set; hook is the last hook status that was allowed through; lastOSC is
// when the protocol stream last spoke.
type paneState struct {
	records Set
	hook    string
	lastOSC time.Time
}

// oscAuthority bounds how long a transient record suppresses hook events:
// a 7501-native agent reports at every turn boundary, so a working record
// with no protocol report for two minutes while hooks still fire means the
// protocol speaker is gone (a crash leaves no clear). After the window the
// hook stream takes the pane back.
const oscAuthority = 2 * time.Minute

// Store keeps per-pane status and decides, for every pane, whether the
// native OSC 7501 stream or the hook stream is authoritative right now.
// It is safe for concurrent use: hook events arrive on the apphook
// server's goroutine, program reports on a terminal's goroutine.
type Store struct {
	mu    sync.Mutex
	panes map[string]*paneState
}

// NewStore makes an empty store.
func NewStore() *Store {
	return &Store{panes: make(map[string]*paneState)}
}

// pane returns the pane's state, creating it when missing.
func (s *Store) pane(paneID string) *paneState {
	p, ok := s.panes[paneID]
	if !ok {
		p = &paneState{}
		s.panes[paneID] = p
	}
	return p
}

// ApplyProgramReport parses one OSC 7501 report body from the pane's
// terminal and applies it to the record set. A native report always wins:
// the pane's status source becomes the protocol stream. It returns the
// status word to display, or false when the body was not a valid report.
func (s *Store) ApplyProgramReport(paneID string, body []byte, now time.Time) (string, bool) {
	rep, ok := Parse(body)
	if !ok {
		return "", false
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	p := s.pane(paneID)
	p.records.Apply(rep, now)
	p.lastOSC = now
	if word := rep.StatusWord(); word != "" && rep.State != StateClear {
		return word, true
	}
	return p.display(), true
}

// ApplyHookStatus feeds one hook status event and decides whether it may
// pass. While the pane's 7501 record set holds a transient state
// (working/blocked) the protocol stream is mid-flight and the hook event
// is dropped — a 7501-native agent always reports those transitions
// itself. Once the records hold only surviving states (idle/done/error,
// or nothing at all), a hook event means a hook-only program owns the
// pane, and it takes over. `released` mirrors process exit: transient
// records are dropped either way.
func (s *Store) ApplyHookStatus(paneID string, status string, now time.Time) (string, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	p := s.pane(paneID)
	fresh := now.Sub(p.lastOSC) <= oscAuthority
	if p.records.Transient() && fresh && status != "released" {
		return p.display(), false
	}
	if status == "released" {
		p.records.DropTransient()
	}
	p.hook = status
	return status, true
}

// Display returns the pane's current status word, or "" when the pane
// is unknown. It is the read side of the store: the same resolution the
// apply paths return, without touching state.
func (s *Store) Display(paneID string) string {
	s.mu.Lock()
	defer s.mu.Unlock()
	p, ok := s.panes[paneID]
	if !ok {
		return ""
	}
	return p.display()
}

// PaneClosed forgets the pane: its records die with the terminal, and the
// next program starts from whichever source speaks first.
func (s *Store) PaneClosed(paneID string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	delete(s.panes, paneID)
}

// display resolves the pane's current status word: the 7501 record set
// when it has anything to show, else the hook status that passed last.
func (p *paneState) display() string {
	if rep, ok := p.records.Display(); ok {
		if word := rep.StatusWord(); word != "" {
			return word
		}
	}
	return p.hook
}
