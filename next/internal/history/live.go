package history

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"os"
	"strings"
	"time"
)

// Live semantic source (0.6 §7): provider-owned append-only session files
// decoded incrementally into the same TranscriptMessage projection the full
// History parsers produce. No ANSI/TUI semantics; unknown provider records
// are counted, never guessed; providers without a ported line decoder fail
// closed at the factory.

// LiveMode mirrors the audited Rust live registry's LiveCapability column.
type LiveMode string

const (
	// LiveModeAppendLog is append-only JSONL tailing (Claude/Codex/Pi/Omp/...).
	LiveModeAppendLog LiveMode = "append-log"
	// LiveModeHookJournal is the provider-neutral Hook Journal transport.
	LiveModeHookJournal LiveMode = "hook-journal"
	// LiveModeNone means no live semantic source (History-only provider).
	LiveModeNone LiveMode = "none"
)

// LiveCapability is one provider's live semantic-source declaration. Mode
// mirrors the Rust PROVIDERS table; DecoderReady marks whether this Go
// migration has already ported the provider's line decoder. A declared mode
// without a ported decoder fails closed at the factory instead of guessing.
type LiveCapability struct {
	Mode         LiveMode
	DecoderReady bool
}

// liveRegistry covers every AllAgents entry exactly once (0.6 §7.2). The
// Mode column mirrors crates/herdr-history/src/live/registry.rs; only the
// providers whose Go line decoders exist in this package are DecoderReady.
var liveRegistry = map[AgentID]LiveCapability{
	AgentClaudeCode:  {Mode: LiveModeAppendLog, DecoderReady: true},
	AgentCodex:       {Mode: LiveModeAppendLog, DecoderReady: true},
	AgentCopilot:     {Mode: LiveModeNone},
	AgentCursor:      {Mode: LiveModeAppendLog},
	AgentOpenCode:    {Mode: LiveModeNone},
	AgentCommandCode: {Mode: LiveModeAppendLog},
	AgentKiro:        {Mode: LiveModeNone},
	AgentGemini:      {Mode: LiveModeNone},
	AgentPi:          {Mode: LiveModeAppendLog, DecoderReady: true},
	AgentOMP:         {Mode: LiveModeAppendLog, DecoderReady: true},
	AgentGrok:        {Mode: LiveModeNone},
	AgentKimi:        {Mode: LiveModeAppendLog},
	AgentAntigravity: {Mode: LiveModeHookJournal},
	AgentDSH:         {Mode: LiveModeNone},
	AgentQoder:       {Mode: LiveModeNone},
}

// LiveCapabilityFor returns the provider's live declaration; ok is false for
// unregistered agents.
func LiveCapabilityFor(agent AgentID) (LiveCapability, bool) {
	capability, ok := liveRegistry[agent]
	return capability, ok
}

// LiveCapableAgents lists providers with any live semantic source.
func LiveCapableAgents() []AgentID {
	agents := make([]AgentID, 0, len(liveRegistry))
	for _, agent := range AllAgents {
		if liveRegistry[agent].Mode != LiveModeNone {
			agents = append(agents, agent)
		}
	}
	return agents
}

// ErrLiveDecoderUnavailable is returned by NewLiveDecoder for providers
// whose Go line decoder has not been ported yet; callers must fail closed.
var ErrLiveDecoderUnavailable = errors.New("live decoder unavailable for provider")

// ErrLiveSourceUnresolved is returned when a measured session identity can
// neither be used directly (path) nor resolved through the provider roots
// (native id). Callers fail closed; they never guess a transcript path.
var ErrLiveSourceUnresolved = errors.New("live source unresolved for session identity")

// ResolveLiveSource maps one measured Herdr AgentSessionInfo identity onto
// the exact provider transcript file (0.6 §4.2, CONV-08): kind=path is the
// exact source; kind=id resolves through the provider roots; unknown kinds
// and metadata-only identities fail closed.
func ResolveLiveSource(home string, agent AgentID, kind, source, value string) (string, error) {
	locator := ResolveSessionSourceLocator(agent, kind, source, value)
	switch locator.Kind {
	case LocatorFilePath:
		if strings.TrimSpace(locator.Identity) == "" {
			return "", fmt.Errorf("%w: empty path identity", ErrLiveSourceUnresolved)
		}
		return locator.Identity, nil
	case LocatorNativeID:
		for _, root := range DefaultRoots(home) {
			if root.Agent != agent {
				continue
			}
			references, err := listJSONLRefs(root.Directory, root.Agent, root.NativeID)
			if err != nil {
				return "", err
			}
			for _, reference := range references {
				if reference.NativeID == locator.Identity {
					return reference.FilePath, nil
				}
			}
		}
		return "", fmt.Errorf("%w: %s session %q not found in roots", ErrLiveSourceUnresolved, agent, locator.Identity)
	default:
		return "", fmt.Errorf("%w: locator kind %q", ErrLiveSourceUnresolved, locator.Kind)
	}
}

// IndexedMessage pairs a projection index with its row for delta reports.
type IndexedMessage struct {
	Index   int
	Message TranscriptMessage
}

// LiveSync reports one decoder step (0.6 §7.3). Hydrated marks the first
// sync after construction or reset; Reset marks truncate/replace or a
// provider view flip; Appended/Changed are delta splices against the
// previous projection; Messages carries the authoritative full projection
// on rebuild syncs; Pending is the uncommitted Claude streaming tail
// (provisional, not yet a committed row), and PendingChanged reports that
// the tail appeared, grew, or committed since the previous sync.
type LiveSync struct {
	Hydrated       bool
	Reset          bool
	Appended       []IndexedMessage
	Changed        []IndexedMessage
	Messages       []TranscriptMessage
	Pending        *TranscriptMessage
	PendingChanged bool
	UnknownLines   uint32
}

// HasUpdates reports whether applying this sync would change what a
// receiver renders; duplicate wakes produce syncs where it is false
// (idempotent).
func (s LiveSync) HasUpdates() bool {
	return s.Hydrated || s.Reset || len(s.Appended) > 0 || len(s.Changed) > 0 || s.PendingChanged
}

// LiveDecoder is the transport-neutral incremental decoder for one live
// provider source (CONV-12): callers feed appended bytes; the decoder owns
// the append cursor, partial-line buffering, provider line parsing, and the
// delta report. Not safe for concurrent use; the owning pump serializes it.
type LiveDecoder struct {
	agent      AgentID
	claude     *claudeParseState
	codex      *codexParseState
	pi         *piParseState
	cursor     int64
	partial    []byte
	snapshot   []TranscriptMessage
	fallback   bool
	pendingKey string
	hydrated   bool
	reset      bool
}

// NewLiveDecoder builds the decoder for one provider. Providers without a
// ported line decoder return ErrLiveDecoderUnavailable (fail closed).
func NewLiveDecoder(agent AgentID) (*LiveDecoder, error) {
	capability, ok := LiveCapabilityFor(agent)
	if !ok || capability.Mode == LiveModeNone || !capability.DecoderReady {
		return nil, fmt.Errorf("%w: %s", ErrLiveDecoderUnavailable, agent)
	}
	decoder := &LiveDecoder{agent: agent}
	switch agent {
	case AgentClaudeCode:
		decoder.claude = newClaudeParseState()
	case AgentCodex:
		decoder.codex = newCodexParseState()
	case AgentPi, AgentOMP:
		decoder.pi = newPiParseState()
	default:
		return nil, fmt.Errorf("%w: %s", ErrLiveDecoderUnavailable, agent)
	}
	return decoder, nil
}

// Agent reports the provider this decoder was built for.
func (d *LiveDecoder) Agent() AgentID { return d.agent }

// Cursor reports the number of source bytes folded into complete lines.
func (d *LiveDecoder) Cursor() int64 { return d.cursor }

// Append folds newly appended source bytes into the projection. The cursor
// advances over every byte handed to the decoder: bytes after the last
// newline stay buffered (partial lines never emit corrupted rows) and are
// not re-read by the next poll, mirroring the Rust transport's
// "next poll starts after the seen partial" rule.
func (d *LiveDecoder) Append(chunk []byte) LiveSync {
	d.partial = append(d.partial, chunk...)
	d.cursor += int64(len(chunk))
	for {
		idx := bytes.IndexByte(d.partial, '\n')
		if idx < 0 {
			break
		}
		line := d.partial[:idx+1]
		d.partial = d.partial[idx+1:]
		d.feedLine(string(line))
	}
	return d.diff()
}

// Status reports the current sync state without new source bytes: the
// duplicate-wake idempotence path.
func (d *LiveDecoder) Status() LiveSync {
	return d.diff()
}

// Finalize settles the trailing partial line (if any) and commits any
// provisional assistant tail exactly as the full EOF parse would, for
// parity when the live session ends.
func (d *LiveDecoder) Finalize() LiveSync {
	if len(d.partial) > 0 && strings.TrimSpace(string(d.partial)) != "" {
		line := d.partial
		d.partial = nil
		d.feedLine(string(line))
	}
	if d.claude != nil {
		d.claude.flushPending()
	}
	return d.diff()
}

// Projection returns a deep copy of the current committed rows for the
// active provider view. Receivers bound the projection to their window;
// the decoder never decides UI bounds itself.
func (d *LiveDecoder) Projection() []TranscriptMessage {
	return d.current()
}

// PendingTail returns the provisional uncommitted streaming tail (Claude
// assistant accumulation), or nil. It is re-reported on every sync and is
// replaced by a committed row at the next role boundary or Finalize.
func (d *LiveDecoder) PendingTail() *TranscriptMessage {
	return d.pendingTail()
}

// Reset discards the projection and cursor: the next Append re-hydrates
// from the (replaced) source start.
func (d *LiveDecoder) Reset() {
	switch d.agent {
	case AgentClaudeCode:
		d.claude = newClaudeParseState()
	case AgentCodex:
		d.codex = newCodexParseState()
	case AgentPi, AgentOMP:
		d.pi = newPiParseState()
	}
	d.cursor = 0
	d.partial = nil
	d.snapshot = nil
	d.fallback = false
	d.pendingKey = ""
	d.hydrated = false
	d.reset = true
}

func (d *LiveDecoder) feedLine(line string) {
	if strings.TrimSpace(line) == "" {
		return
	}
	if d.claude != nil {
		d.claude.feedClaudeLine(line)
		return
	}
	if d.codex != nil {
		d.codex.feedCodexLine(line)
		return
	}
	if d.pi != nil {
		d.pi.feedPiLine(line)
		return
	}
}

// current returns a deep copy of the active projection. Tool outputs and
// reasoning merges mutate rows in place through shared backing arrays, so
// the diff snapshot must not alias the parse state's slices.
func (d *LiveDecoder) current() []TranscriptMessage {
	if d.claude != nil {
		return deepCopyMessages(d.claude.messages)
	}
	if d.codex != nil {
		return deepCopyMessages(d.codex.codexMessages())
	}
	if d.pi != nil {
		return deepCopyMessages(d.pi.messages)
	}
	return nil
}

func (d *LiveDecoder) unknownLines() uint32 {
	if d.claude != nil {
		return d.claude.unknownLines
	}
	if d.codex != nil {
		return d.codex.unknownLines
	}
	if d.pi != nil {
		return d.pi.unknownLines
	}
	return 0
}

func (d *LiveDecoder) pendingTail() *TranscriptMessage {
	if d.claude != nil {
		return d.claude.pendingTail()
	}
	return nil
}

func (d *LiveDecoder) diff() LiveSync {
	sync := LiveSync{UnknownLines: d.unknownLines()}
	messages := d.current()
	if d.codex != nil {
		fallback := d.codex.codexFallbackActive()
		if fallback != d.fallback {
			// The active Codex view flipped (first real response_item after
			// the event fallback): indices are not comparable across views,
			// so receivers rebuild their projection explicitly.
			d.fallback = fallback
			sync.Reset = true
		}
	}
	if d.reset {
		sync.Reset = true
		d.reset = false
	}
	if !d.hydrated {
		sync.Hydrated = true
		d.hydrated = true
	}
	if sync.Reset || sync.Hydrated {
		sync.Messages = messages
		sync.Appended, sync.Changed = diffMessages(nil, messages)
	} else {
		sync.Appended, sync.Changed = diffMessages(d.snapshot, messages)
	}
	d.snapshot = messages
	pending := d.pendingTail()
	pendingKey := ""
	if pending != nil {
		pendingKey = fmt.Sprintf("%d\x00%s", pending.Seq, pending.Text)
	}
	sync.PendingChanged = pendingKey != d.pendingKey
	d.pendingKey = pendingKey
	sync.Pending = pending
	return sync
}

func deepCopyMessages(messages []TranscriptMessage) []TranscriptMessage {
	if messages == nil {
		return nil
	}
	out := make([]TranscriptMessage, len(messages))
	copy(out, messages)
	for i := range out {
		if out[i].ToolCalls != nil {
			copied := make([]ToolCall, len(out[i].ToolCalls))
			copy(copied, out[i].ToolCalls)
			out[i].ToolCalls = copied
		}
	}
	return out
}

// diffMessages reports which rows were appended and which existing rows
// changed (tool-result backfill mutates earlier rows in place).
func diffMessages(old, new []TranscriptMessage) (appended, changed []IndexedMessage) {
	common := len(old)
	if len(new) < common {
		common = len(new)
	}
	for i := 0; i < common; i++ {
		if !transcriptMessageEqual(old[i], new[i]) {
			changed = append(changed, IndexedMessage{Index: i, Message: new[i]})
		}
	}
	for i := len(old); i < len(new); i++ {
		appended = append(appended, IndexedMessage{Index: i, Message: new[i]})
	}
	return appended, changed
}

func transcriptMessageEqual(a, b TranscriptMessage) bool {
	if a.Seq != b.Seq || a.Role != b.Role || a.Kind != b.Kind ||
		a.Text != b.Text || a.Truncated != b.Truncated {
		return false
	}
	if !stringPtrEqual(a.Thinking, b.Thinking) || !stringPtrEqual(a.Model, b.Model) {
		return false
	}
	if !int64PtrEqual(a.Timestamp, b.Timestamp) {
		return false
	}
	if len(a.ToolCalls) != len(b.ToolCalls) {
		return false
	}
	for i := range a.ToolCalls {
		if !toolCallEqual(a.ToolCalls[i], b.ToolCalls[i]) {
			return false
		}
	}
	return true
}

func toolCallEqual(a, b ToolCall) bool {
	if a.ID != b.ID || a.Name != b.Name || a.InputPreview != b.InputPreview || a.IsError != b.IsError {
		return false
	}
	if !stringPtrEqual(a.Input, b.Input) || !stringPtrEqual(a.Output, b.Output) || !stringPtrEqual(a.SidechainRef, b.SidechainRef) {
		return false
	}
	return true
}

func stringPtrEqual(a, b *string) bool {
	if a == nil || b == nil {
		return a == b
	}
	return *a == *b
}

func int64PtrEqual(a, b *int64) bool {
	if a == nil || b == nil {
		return a == b
	}
	return *a == *b
}

// LiveSource is the transport seam between a growing provider file and the
// decoder (CONV-12). Production uses the append-log file transport; tests
// substitute in-memory fakes.
type LiveSource interface {
	Size() (int64, error)
	ReadAt(p []byte, off int64) (int, error)
}

// FileLiveSource reads one append-only JSONL session file.
type FileLiveSource struct {
	path string
}

// NewFileLiveSource tails the provider file at path.
func NewFileLiveSource(path string) *FileLiveSource {
	return &FileLiveSource{path: path}
}

func (s *FileLiveSource) Size() (int64, error) {
	info, err := os.Stat(s.path)
	if err != nil {
		return 0, err
	}
	return info.Size(), nil
}

func (s *FileLiveSource) ReadAt(p []byte, off int64) (int, error) {
	file, err := os.Open(s.path)
	if err != nil {
		return 0, err
	}
	defer file.Close()
	return file.ReadAt(p, off)
}

// LiveWatcher polls one LiveSource into a LiveDecoder, reading only the
// appended byte window and re-hydrating after truncate/replace (CONV-13/18).
type LiveWatcher struct {
	source   LiveSource
	decoder  *LiveDecoder
	lastSize int64
}

// NewLiveWatcher binds a source to a decoder; the watcher owns polling.
func NewLiveWatcher(source LiveSource, decoder *LiveDecoder) *LiveWatcher {
	return &LiveWatcher{source: source, decoder: decoder, lastSize: -1}
}

// Poll applies the source's current state to the decoder. Duplicate wakes
// are idempotent (no new bytes → HasUpdates false); truncate/replace
// produces one explicit reset-and-rehydrate sync.
func (w *LiveWatcher) Poll() (LiveSync, error) {
	size, err := w.source.Size()
	if err != nil {
		return LiveSync{}, err
	}
	if (w.lastSize >= 0 && size < w.lastSize) || size < w.decoder.cursor {
		w.decoder.Reset()
		w.lastSize = -1
	}
	if size == w.decoder.cursor {
		w.lastSize = size
		return w.decoder.Status(), nil
	}
	window := make([]byte, size-w.decoder.cursor)
	if _, err := w.source.ReadAt(window, w.decoder.cursor); err != nil {
		return LiveSync{}, err
	}
	w.lastSize = size
	return w.decoder.Append(window), nil
}

// LivePump drives one LiveWatcher from wake signals with storm coalescing
// and a bounded timer backstop (0.6 §7.4): no high-frequency polling; a
// burst of wakes folds into a single poll; the backstop catches missed wake
// events. The pump serializes all decoder access.
type LivePump struct {
	watcher  *LiveWatcher
	Wake     <-chan struct{}
	Backstop time.Duration
	OnSync   func(LiveSync, error)
	// Done is closed exactly once when Run returns; unbinders and tests
	// join on it before touching the decoder again.
	Done chan struct{}
}

// NewLivePump drives watcher from the wake channel.
func NewLivePump(watcher *LiveWatcher, wake <-chan struct{}) *LivePump {
	return &LivePump{watcher: watcher, Wake: wake, Done: make(chan struct{})}
}

// drainWakes folds every already-queued wake signal into the next poll and
// reports how many were coalesced. It is the deterministic seam for
// wake-storm tests.
func drainWakes(wake <-chan struct{}) int {
	count := 0
	for {
		select {
		case <-wake:
			count++
		default:
			return count
		}
	}
}

// PumpOnce coalesces every queued wake and applies one poll through OnSync.
// The bind path calls it synchronously for hydration; Run drives it from
// wakes and the backstop. Duplicate polls are idempotent.
func (p *LivePump) PumpOnce() {
	drainWakes(p.Wake)
	if p.OnSync == nil {
		return
	}
	sync, err := p.watcher.Poll()
	p.OnSync(sync, err)
}

// Run pumps until ctx is cancelled, reacting to wakes and the backstop.
// The caller performs the initial hydration through PumpOnce on its own
// lane before starting Run (0.6 §7.3); a standalone Run observes the
// source within one backstop interval.
func (p *LivePump) Run(ctx context.Context) {
	defer close(p.Done)
	backstop := p.Backstop
	if backstop <= 0 {
		backstop = 2 * time.Second
	}
	ticker := time.NewTicker(backstop)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			p.PumpOnce()
		case <-p.Wake:
			p.PumpOnce()
		}
	}
}
