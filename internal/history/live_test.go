package history

import (
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"reflect"
	"sync"
	"testing"
	"time"
)

// fakeLiveSource is the transport fake for CONV-12: the decoder pipeline is
// driven through the LiveSource seam without touching the filesystem. The
// mutex mirrors the kernel serialization a real file transport provides
// when the pump goroutine polls while the test appends.
type fakeLiveSource struct {
	mu    sync.Mutex
	bytes []byte
}

func (s *fakeLiveSource) Size() (int64, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return int64(len(s.bytes)), nil
}

func (s *fakeLiveSource) ReadAt(p []byte, off int64) (int, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if off > int64(len(s.bytes)) {
		return 0, io.EOF
	}
	n := copy(p, s.bytes[off:])
	if n < len(p) {
		return n, io.EOF
	}
	return n, nil
}

func (s *fakeLiveSource) append(chunk []byte) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.bytes = append(s.bytes, chunk...)
}

func (s *fakeLiveSource) size() int {
	s.mu.Lock()
	defer s.mu.Unlock()
	return len(s.bytes)
}

const liveClaudeFixture = "" +
	"{\"type\":\"user\",\"cwd\":\"/work/demo\",\"gitBranch\":\"main\",\"timestamp\":\"2026-08-01T01:00:00Z\",\"message\":{\"content\":\"Fix the parser\"}}\n" +
	"{\"type\":\"assistant\",\"cwd\":\"/work/demo\",\"timestamp\":\"2026-08-01T01:00:01Z\",\"message\":{\"id\":\"m1\",\"model\":\"claude-sonnet-4-5\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5},\"content\":[{\"type\":\"thinking\",\"thinking\":\"Inspect first\"},{\"type\":\"tool_use\",\"id\":\"tool-1\",\"name\":\"Read\",\"input\":{\"file_path\":\"src/lib.rs\"}},{\"type\":\"text\",\"text\":\"I found it.\"}]}}\n" +
	"{\"type\":\"user\",\"cwd\":\"/work/demo\",\"timestamp\":\"2026-08-01T01:00:02Z\",\"message\":{\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"tool-1\",\"content\":\"source text\"}]}}\n" +
	"{\"type\":\"assistant\",\"cwd\":\"/work/demo\",\"timestamp\":\"2026-08-01T01:00:03Z\",\"message\":{\"id\":\"m2\",\"content\":[{\"type\":\"text\",\"text\":\"Done.\"}]}}\n"

const liveCodexFixture = "" +
	"{\"timestamp\":\"2026-08-02T09:15:00Z\",\"type\":\"session_meta\",\"payload\":{\"cwd\":\"/work/codex-demo\",\"originator\":\"codex_cli_rs\",\"git\":{\"branch\":\"feature/history\"}}}\n" +
	"{\"timestamp\":\"2026-08-02T09:15:01Z\",\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-5\"}}\n" +
	"{\"timestamp\":\"2026-08-02T09:15:02Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"Build history\"}]}}\n" +
	"{\"timestamp\":\"2026-08-02T09:15:03Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"function_call\",\"call_id\":\"call-1\",\"name\":\"shell\",\"arguments\":\"{\\\"command\\\":\\\"cargo test\\\"}\"}}\n" +
	"{\"timestamp\":\"2026-08-02T09:15:04Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"function_call_output\",\"call_id\":\"call-1\",\"output\":\"ok\"}}\n" +
	"{\"timestamp\":\"2026-08-02T09:15:05Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Implemented.\"}]}}\n"

// TestLiveCapabilityRegistryCoversEveryAgent pins the CONV-11 table: one
// entry per AllAgents agent, Mode column mirroring the Rust live registry.
func TestLiveCapabilityRegistryCoversEveryAgent(t *testing.T) {
	for _, agent := range AllAgents {
		capability, ok := LiveCapabilityFor(agent)
		if !ok {
			t.Fatalf("agent %q missing from live registry", agent)
		}
		switch capability.Mode {
		case LiveModeAppendLog, LiveModeHookJournal, LiveModeNone:
		default:
			t.Fatalf("agent %q has unknown live mode %q", agent, capability.Mode)
		}
	}
	appendLog := map[AgentID]bool{
		AgentClaudeCode: true, AgentCodex: true, AgentCursor: true,
		AgentCommandCode: true, AgentPi: true, AgentOMP: true, AgentKimi: true,
	}
	hookJournal := map[AgentID]bool{AgentAntigravity: true}
	for _, agent := range AllAgents {
		capability, _ := LiveCapabilityFor(agent)
		wantAppend := appendLog[agent]
		if (capability.Mode == LiveModeAppendLog) != wantAppend {
			t.Fatalf("agent %q mode = %q, append-log parity broken", agent, capability.Mode)
		}
		if (capability.Mode == LiveModeHookJournal) != hookJournal[agent] {
			t.Fatalf("agent %q mode = %q, hook-journal parity broken", agent, capability.Mode)
		}
	}
	live := LiveCapableAgents()
	if len(live) != len(appendLog)+len(hookJournal) {
		t.Fatalf("live capable agents = %v", live)
	}
	// DecoderReady only where the Go line decoder exists; the rest fail
	// closed instead of guessing provider semantics (CONV-16).
	ready := map[AgentID]bool{AgentClaudeCode: true, AgentCodex: true, AgentPi: true, AgentOMP: true}
	for _, agent := range AllAgents {
		capability, _ := LiveCapabilityFor(agent)
		if capability.DecoderReady != ready[agent] {
			t.Fatalf("agent %q decoder ready = %v, want %v", agent, capability.DecoderReady, ready[agent])
		}
	}
}

// TestLiveDecoderFactoryFailsClosed pins the CONV-11/16 factory contract:
// None providers, unregistered agents, and declared-but-unported decoders
// all return ErrLiveDecoderUnavailable.
func TestLiveDecoderFactoryFailsClosed(t *testing.T) {
	for _, agent := range []AgentID{AgentQoder, AgentCopilot, AgentAntigravity, "unknown"} {
		decoder, err := NewLiveDecoder(agent)
		if err == nil || decoder != nil {
			t.Fatalf("agent %q: expected fail-closed error", agent)
		}
		if !errors.Is(err, ErrLiveDecoderUnavailable) {
			t.Fatalf("agent %q: error = %v", agent, err)
		}
	}
}

// TestLiveWatcherAppendCursorAndPartialLine covers CONV-12/13 on the fake
// transport: hydration, append-only reads, partial-line buffering, and
// duplicate-wake idempotence.
func TestLiveWatcherAppendCursorAndPartialLine(t *testing.T) {
	source := &fakeLiveSource{}
	decoder, err := NewLiveDecoder(AgentClaudeCode)
	if err != nil {
		t.Fatal(err)
	}
	watcher := NewLiveWatcher(source, decoder)

	// Partial hydration: an unterminated line never emits a row.
	source.append([]byte("{\"type\":\"user\",\"message\":{\"content\":\"Fix the parser\"}}\n{\"type\":\"ass"))
	sync, err := watcher.Poll()
	if err != nil {
		t.Fatal(err)
	}
	if !sync.Hydrated || len(sync.Messages) != 1 || len(sync.Appended) != 1 {
		t.Fatalf("hydrate sync = %+v", sync)
	}
	if decoder.Cursor() != int64(source.size()) {
		t.Fatalf("cursor = %d, want all consumed bytes %d", decoder.Cursor(), source.size())
	}

	// Complete the partial assistant line: it appears as the provisional
	// pending tail (Claude commits assistant turns at role boundaries).
	source.append([]byte("istant\",\"timestamp\":\"2026-08-01T01:00:01Z\",\"message\":{\"id\":\"m1\",\"content\":[{\"type\":\"text\",\"text\":\"I found it.\"}]}}\n"))
	sync, err = watcher.Poll()
	if err != nil {
		t.Fatal(err)
	}
	if len(sync.Appended) != 0 || len(sync.Changed) != 0 {
		t.Fatalf("assistant turn must not commit before its boundary: %+v", sync)
	}
	if sync.Pending == nil || sync.Pending.Text != "I found it." {
		t.Fatalf("pending tail = %+v", sync.Pending)
	}
	if decoder.Cursor() != int64(source.size()) {
		t.Fatalf("cursor = %d, want all consumed bytes %d", decoder.Cursor(), source.size())
	}

	// A following user line commits the tail as exactly one row.
	source.append([]byte("{\"type\":\"user\",\"message\":{\"content\":\"and?\"}}\n"))
	sync, err = watcher.Poll()
	if err != nil {
		t.Fatal(err)
	}
	if len(sync.Appended) != 2 || sync.Appended[0].Message.Text != "I found it." || sync.Appended[1].Message.Text != "and?" {
		t.Fatalf("boundary sync = %+v", sync)
	}
	if sync.Pending != nil {
		t.Fatalf("pending tail survived its boundary: %+v", sync.Pending)
	}

	// Duplicate wake with no new bytes: idempotent, no updates.
	sync, err = watcher.Poll()
	if err != nil {
		t.Fatal(err)
	}
	if sync.HasUpdates() {
		t.Fatalf("duplicate wake produced updates: %+v", sync)
	}
	if sync.Hydrated || sync.Reset {
		t.Fatalf("duplicate wake flipped sync flags: %+v", sync)
	}
}

// TestLiveClaudeFullEqualsIncremental pins CONV-14: arbitrary transport
// splits decode to exactly the full (EOF) parse projection, including the
// trailing partial line and the pending assistant tail.
func TestLiveClaudeFullEqualsIncremental(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "claude-live.jsonl")
	if err := os.WriteFile(path, []byte(liveClaudeFixture), 0o600); err != nil {
		t.Fatal(err)
	}
	parsed, err := ParseClaudeTranscript(SessionFileRef{Agent: AgentClaudeCode, NativeID: "live", FilePath: path})
	if err != nil {
		t.Fatal(err)
	}

	for _, split := range []int{1, 3, 17, 64, len(liveClaudeFixture)} {
		decoder, err := NewLiveDecoder(AgentClaudeCode)
		if err != nil {
			t.Fatal(err)
		}
		source := &fakeLiveSource{}
		watcher := NewLiveWatcher(source, decoder)
		for offset := 0; offset < len(liveClaudeFixture); offset += split {
			end := offset + split
			if end > len(liveClaudeFixture) {
				end = len(liveClaudeFixture)
			}
			source.append([]byte(liveClaudeFixture[offset:end]))
			if _, err := watcher.Poll(); err != nil {
				t.Fatal(err)
			}
		}
		sync := decoder.Finalize()
		if !reflect.DeepEqual(decoder.Projection(), parsed.Mainline) {
			t.Fatalf("split %d: incremental rows differ from full parse\nincremental=%+v\nfull=%+v",
				split, decoder.Projection(), parsed.Mainline)
		}
		if sync.UnknownLines != parsed.UnknownLineCount {
			t.Fatalf("split %d: unknown lines = %d, want %d", split, sync.UnknownLines, parsed.UnknownLineCount)
		}
		if sync.Pending != nil {
			t.Fatalf("split %d: pending tail survived finalize", split)
		}
	}
}

// TestLiveFileSourceGrowthAppendOnly pins CONV-13/14 on the real file
// transport: a grown file reads only the appended window and the final
// projection equals the full parse.
func TestLiveFileSourceGrowthAppendOnly(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "claude-live.jsonl")
	head := liveClaudeFixture
	if err := os.WriteFile(path, []byte(head), 0o600); err != nil {
		t.Fatal(err)
	}
	decoder, err := NewLiveDecoder(AgentClaudeCode)
	if err != nil {
		t.Fatal(err)
	}
	watcher := NewLiveWatcher(NewFileLiveSource(path), decoder)
	sync, err := watcher.Poll()
	if err != nil {
		t.Fatal(err)
	}
	// The trailing m2 assistant turn stays pending: two committed rows.
	if !sync.Hydrated || len(sync.Messages) != 2 {
		t.Fatalf("hydrate = %+v (%d rows)", sync, len(sync.Messages))
	}

	growth := []byte("{\"type\":\"user\",\"cwd\":\"/work/demo\",\"timestamp\":\"2026-08-01T01:00:04Z\",\"message\":{\"content\":\"ship it\"}}\n")
	file, err := os.OpenFile(path, os.O_APPEND|os.O_WRONLY, 0o600)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := file.Write(growth); err != nil {
		t.Fatal(err)
	}
	file.Close()

	sync, err = watcher.Poll()
	if err != nil {
		t.Fatal(err)
	}
	if sync.Hydrated || sync.Reset {
		t.Fatalf("growth sync flags = %+v", sync)
	}
	// The growth line first commits the pending m2 tail, then itself.
	if len(sync.Appended) != 2 || sync.Appended[0].Message.Text != "Done." || sync.Appended[1].Message.Text != "ship it" {
		t.Fatalf("growth sync = %+v", sync)
	}

	sync = decoder.Finalize()
	parsed, err := ParseClaudeTranscript(SessionFileRef{Agent: AgentClaudeCode, NativeID: "live", FilePath: path})
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(decoder.Projection(), parsed.Mainline) {
		t.Fatal("grown file incremental rows differ from full parse")
	}
}

// TestLiveCodexFullEqualsIncremental pins CONV-15: the Codex rollout decodes
// incrementally to exactly the full parse projection.
func TestLiveCodexFullEqualsIncremental(t *testing.T) {
	parsed := func(t *testing.T, data []byte) ParsedTranscript {
		t.Helper()
		dir := t.TempDir()
		path := filepath.Join(dir, "rollout-live.jsonl")
		if err := os.WriteFile(path, data, 0o600); err != nil {
			t.Fatal(err)
		}
		parsed, err := ParseCodexTranscript(SessionFileRef{Agent: AgentCodex, NativeID: "live", FilePath: path})
		if err != nil {
			t.Fatal(err)
		}
		return parsed
	}(t, []byte(liveCodexFixture))

	for _, split := range []int{5, 29, 128, len(liveCodexFixture)} {
		decoder, err := NewLiveDecoder(AgentCodex)
		if err != nil {
			t.Fatal(err)
		}
		source := &fakeLiveSource{}
		watcher := NewLiveWatcher(source, decoder)
		for offset := 0; offset < len(liveCodexFixture); offset += split {
			end := offset + split
			if end > len(liveCodexFixture) {
				end = len(liveCodexFixture)
			}
			source.append([]byte(liveCodexFixture[offset:end]))
			if _, err := watcher.Poll(); err != nil {
				t.Fatal(err)
			}
		}
		sync := decoder.Finalize()
		if !reflect.DeepEqual(decoder.Projection(), parsed.Mainline) {
			t.Fatalf("split %d: incremental rows differ from full parse\nincremental=%+v\nfull=%+v",
				split, decoder.Projection(), parsed.Mainline)
		}
		if sync.UnknownLines != parsed.UnknownLineCount {
			t.Fatalf("split %d: unknown lines = %d, want %d", split, sync.UnknownLines, parsed.UnknownLineCount)
		}
		var toolFound bool
		for _, row := range decoder.Projection() {
			for _, call := range row.ToolCalls {
				if call.Name == "shell" && call.Output != nil && *call.Output == "ok" {
					toolFound = true
				}
			}
		}
		if !toolFound {
			t.Fatalf("split %d: tool output backfill missing", split)
		}
	}
}

// TestLiveClaudeToolResultBackfillChangedRow covers CONV-17: a tool result
// appended after the tool call's row is committed updates the existing row
// (Changed) instead of duplicating it.
func TestLiveClaudeToolResultBackfillChangedRow(t *testing.T) {
	source := &fakeLiveSource{}
	decoder, err := NewLiveDecoder(AgentClaudeCode)
	if err != nil {
		t.Fatal(err)
	}
	watcher := NewLiveWatcher(source, decoder)

	// User turn plus the assistant tool_use turn (still pending); the
	// fixture's own tool_result line is deliberately excluded so the test
	// appends the first result itself.
	head := liveClaudeFixture[:len(liveClaudeFixture)-len("{\"type\":\"user\",\"cwd\":\"/work/demo\",\"timestamp\":\"2026-08-01T01:00:02Z\",\"message\":{\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"tool-1\",\"content\":\"source text\"}]}}\n")-len("{\"type\":\"assistant\",\"cwd\":\"/work/demo\",\"timestamp\":\"2026-08-01T01:00:03Z\",\"message\":{\"id\":\"m2\",\"content\":[{\"type\":\"text\",\"text\":\"Done.\"}]}}\n")]
	source.append([]byte(head))
	sync, err := watcher.Poll()
	if err != nil {
		t.Fatal(err)
	}
	if len(sync.Changed) != 0 {
		t.Fatalf("unexpected changed rows on hydration: %+v", sync.Changed)
	}
	if len(sync.Messages) != 1 || sync.Pending == nil || sync.Pending.Text != "I found it." {
		t.Fatalf("hydration = %d rows, pending %+v", len(sync.Messages), sync.Pending)
	}

	// A plain user line first commits the pending turns (m1 with a
	// not-yet-backfilled tool call, then the m2 tail).
	boundary := []byte("{\"type\":\"user\",\"timestamp\":\"2026-08-01T01:00:05Z\",\"message\":{\"content\":\"continue\"}}\n")
	source.append(boundary)
	sync, err = watcher.Poll()
	if err != nil {
		t.Fatal(err)
	}
	if len(sync.Appended) != 2 || sync.Appended[0].Message.Text != "I found it." || sync.Appended[1].Message.Text != "continue" {
		t.Fatalf("boundary sync = %+v", sync)
	}
	for _, call := range sync.Appended[0].Message.ToolCalls {
		if call.Output != nil {
			t.Fatalf("tool output backfilled before the result arrived: %+v", call)
		}
	}

	// Now the tool result updates the committed row in place.
	toolResult := []byte("{\"type\":\"user\",\"timestamp\":\"2026-08-01T01:00:06Z\",\"message\":{\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"tool-1\",\"content\":\"source text\"}]}}\n")
	source.append(toolResult)
	sync, err = watcher.Poll()
	if err != nil {
		t.Fatal(err)
	}
	if len(sync.Appended) != 0 {
		t.Fatalf("tool result duplicated rows: %+v", sync.Appended)
	}
	if len(sync.Changed) != 1 || sync.Changed[0].Index != 1 {
		t.Fatalf("changed rows = %+v, want index 1", sync.Changed)
	}
	tool := sync.Changed[0].Message.ToolCalls[0]
	if tool.Output == nil || *tool.Output != "source text" {
		t.Fatalf("backfilled tool = %+v", tool)
	}
}

// TestLiveCodexToolOutputChangedRow covers CONV-17 for Codex: the
// function_call_output updates the assistant host row in place.
func TestLiveCodexToolOutputChangedRow(t *testing.T) {
	source := &fakeLiveSource{}
	decoder, err := NewLiveDecoder(AgentCodex)
	if err != nil {
		t.Fatal(err)
	}
	watcher := NewLiveWatcher(source, decoder)

	source.append([]byte(liveCodexFixture))
	sync, err := watcher.Poll()
	if err != nil {
		t.Fatal(err)
	}
	if len(sync.Changed) != 0 || len(sync.Appended) != 3 {
		t.Fatalf("hydrate deltas = appended %d changed %d", len(sync.Appended), len(sync.Changed))
	}

	// A second output for the same call id updates the same row again.
	source.append([]byte("{\"timestamp\":\"2026-08-02T09:15:07Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"function_call_output\",\"call_id\":\"call-1\",\"output\":\"updated\"}}\n"))
	sync, err = watcher.Poll()
	if err != nil {
		t.Fatal(err)
	}
	if len(sync.Appended) != 0 {
		t.Fatalf("tool output duplicated rows: %+v", sync.Appended)
	}
	if len(sync.Changed) != 1 {
		t.Fatalf("changed rows = %+v, want 1", sync.Changed)
	}
	if got := sync.Changed[0].Message.ToolCalls[0].Output; got == nil || *got != "updated" {
		t.Fatalf("backfilled output = %v", got)
	}
}

// TestLiveCodexFallbackFlipResetsProjection covers CONV-18 for the Codex
// fallback view: when real content arrives the active view flips and the
// sync demands an explicit projection rebuild.
func TestLiveCodexFallbackFlipResetsProjection(t *testing.T) {
	source := &fakeLiveSource{}
	decoder, err := NewLiveDecoder(AgentCodex)
	if err != nil {
		t.Fatal(err)
	}
	watcher := NewLiveWatcher(source, decoder)

	fallbackView := "" +
		"{\"timestamp\":\"2026-08-02T09:15:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"hello\"}}\n" +
		"{\"timestamp\":\"2026-08-02T09:15:01Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\",\"message\":\"hi there\"}}\n"
	source.append([]byte(fallbackView))
	sync, err := watcher.Poll()
	if err != nil {
		t.Fatal(err)
	}
	if !sync.Hydrated || len(sync.Messages) != 2 {
		t.Fatalf("fallback hydration = %+v", sync)
	}

	realView := []byte("{\"timestamp\":\"2026-08-02T09:15:02Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"real answer\"}]}}\n")
	source.append(realView)
	sync, err = watcher.Poll()
	if err != nil {
		t.Fatal(err)
	}
	if !sync.Reset {
		t.Fatalf("view flip must demand a projection reset: %+v", sync)
	}
	if len(sync.Messages) != 1 || sync.Messages[0].Text != "real answer" {
		t.Fatalf("rebuilt projection = %+v", sync.Messages)
	}
}

// TestLiveWatcherTruncateResets covers CONV-18: a truncated or replaced
// source produces one explicit reset-and-rehydrate sync.
func TestLiveWatcherTruncateResets(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "claude-live.jsonl")
	if err := os.WriteFile(path, []byte(liveClaudeFixture), 0o600); err != nil {
		t.Fatal(err)
	}
	decoder, err := NewLiveDecoder(AgentClaudeCode)
	if err != nil {
		t.Fatal(err)
	}
	watcher := NewLiveWatcher(NewFileLiveSource(path), decoder)
	if _, err := watcher.Poll(); err != nil {
		t.Fatal(err)
	}

	replacement := []byte("{\"type\":\"user\",\"cwd\":\"/work/other\",\"message\":{\"content\":\"fresh start\"}}\n")
	if err := os.WriteFile(path, replacement, 0o600); err != nil {
		t.Fatal(err)
	}
	sync, err := watcher.Poll()
	if err != nil {
		t.Fatal(err)
	}
	if !sync.Reset || !sync.Hydrated {
		t.Fatalf("truncate sync = %+v, want reset+hydrate", sync)
	}
	if len(sync.Messages) != 1 || sync.Messages[0].Text != "fresh start" {
		t.Fatalf("rehydrated projection = %+v", sync.Messages)
	}
	if decoder.Cursor() != int64(len(replacement)) {
		t.Fatalf("cursor = %d, want %d", decoder.Cursor(), len(replacement))
	}
}

// TestLiveClaudePendingTailAndFinalize pins the provisional streaming tail:
// the unflushed Claude assistant turn renders as Pending without becoming a
// committed row until the boundary or Finalize commits it.
func TestLiveClaudePendingTailAndFinalize(t *testing.T) {
	source := &fakeLiveSource{}
	decoder, err := NewLiveDecoder(AgentClaudeCode)
	if err != nil {
		t.Fatal(err)
	}
	watcher := NewLiveWatcher(source, decoder)

	head := liveClaudeFixture
	source.append([]byte(head))
	sync, err := watcher.Poll()
	if err != nil {
		t.Fatal(err)
	}
	// The trailing m2 assistant turn is still pending: 4 committed rows
	// (user, m1, tool-result user, m2) flush at the role boundaries; the
	// fixture ends with m2 fully written so it is committed only via
	// pending flush at the next boundary — here Pending must expose it.
	if sync.Pending == nil || sync.Pending.Text != "Done." {
		t.Fatalf("pending tail = %+v", sync.Pending)
	}
	if sync.Pending.Seq != int64(len(sync.Messages)) {
		t.Fatalf("pending seq = %d, want provisional tail %d", sync.Pending.Seq, len(sync.Messages))
	}

	// Finalize commits the tail exactly once.
	sync = decoder.Finalize()
	if sync.Pending != nil {
		t.Fatalf("pending tail survived finalize: %+v", sync.Pending)
	}
	if len(sync.Appended) != 1 || sync.Appended[0].Message.Text != "Done." {
		t.Fatalf("finalize appended = %+v", sync.Appended)
	}
}

// TestResolveLiveSource pins the CONV-08 typed-identity contract: kind=path
// is the exact source; kind=id resolves through the provider roots;
// metadata-only and unknown identities fail closed.
func TestResolveLiveSource(t *testing.T) {
	home := t.TempDir()
	claudeDir := filepath.Join(home, ".claude", "projects", "proj")
	if err := os.MkdirAll(claudeDir, 0o755); err != nil {
		t.Fatal(err)
	}
	sessionPath := filepath.Join(claudeDir, "sess-abc.jsonl")
	if err := os.WriteFile(sessionPath, []byte("{\"type\":\"user\"}\n"), 0o600); err != nil {
		t.Fatal(err)
	}

	// Exact path identity passes through.
	path, err := ResolveLiveSource(home, AgentClaudeCode, "path", "transcript", "/explicit/session.jsonl")
	if err != nil || path != "/explicit/session.jsonl" {
		t.Fatalf("path identity = %q, %v", path, err)
	}

	// Native id resolves through the provider root.
	path, err = ResolveLiveSource(home, AgentClaudeCode, "id", "transcript", "sess-abc")
	if err != nil || path != sessionPath {
		t.Fatalf("id identity = %q, %v", path, err)
	}

	// Unknown ids, metadata-only and unsupported providers fail closed.
	for _, tc := range []struct {
		agent       AgentID
		kind, value string
	}{
		{AgentClaudeCode, "id", "missing-session"},
		{AgentClaudeCode, "fingerprint", "whatever"},
		{AgentQoder, "id", "sess-abc"},
	} {
		if path, err := ResolveLiveSource(home, tc.agent, tc.kind, "transcript", tc.value); err == nil {
			t.Fatalf("%s/%s: expected fail-closed error, got %q", tc.agent, tc.kind, path)
		} else if !errors.Is(err, ErrLiveSourceUnresolved) {
			t.Fatalf("%s/%s: error = %v", tc.agent, tc.kind, err)
		}
	}
}

// TestDrainWakesCoalescesStorm pins the CONV-19 deterministic seam: a burst
// of queued wakes folds into one poll.
func TestDrainWakesCoalescesStorm(t *testing.T) {
	wake := make(chan struct{}, 8)
	if got := drainWakes(wake); got != 0 {
		t.Fatalf("empty wake channel drained %d", got)
	}
	for i := 0; i < 5; i++ {
		wake <- struct{}{}
	}
	if got := drainWakes(wake); got != 5 {
		t.Fatalf("drained %d wakes, want 5", got)
	}
	if got := drainWakes(wake); got != 0 {
		t.Fatalf("second drain got %d, want 0", got)
	}
}

// TestLivePumpBackstopAndCoalescing drives the pump over the fake
// transport: the timer backstop picks up silent appends, and a wake burst
// collapses into a single update sync.
func TestLivePumpBackstopAndCoalescing(t *testing.T) {
	source := &fakeLiveSource{bytes: []byte(liveClaudeFixture)}
	decoder, err := NewLiveDecoder(AgentClaudeCode)
	if err != nil {
		t.Fatal(err)
	}
	watcher := NewLiveWatcher(source, decoder)
	wake := make(chan struct{}, 16)
	updates := make(chan LiveSync, 16)
	pump := NewLivePump(watcher, wake)
	pump.Backstop = 20 * time.Millisecond
	pump.OnSync = func(sync LiveSync, err error) {
		if err != nil {
			t.Errorf("poll error: %v", err)
			return
		}
		if sync.HasUpdates() {
			updates <- sync
		}
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	done := make(chan struct{})
	go func() {
		pump.Run(ctx)
		close(done)
	}()

	waitForUpdate := func() LiveSync {
		t.Helper()
		select {
		case sync := <-updates:
			return sync
		case <-time.After(2 * time.Second):
			t.Fatalf("timed out waiting for update sync")
			return LiveSync{}
		}
	}

	// Backstop hydration: with no caller-side PumpOnce, the pump observes
	// the silent source (and the wakeless append) within one backstop tick.
	extra := []byte("{\"type\":\"user\",\"cwd\":\"/work/demo\",\"timestamp\":\"2026-08-01T01:00:06Z\",\"message\":{\"content\":\"wakeless\"}}\n")
	source.append(extra)
	first := waitForUpdate()
	if !first.Hydrated {
		t.Fatalf("first backstop sync must hydrate: %+v", first)
	}
	if n := len(first.Appended); n == 0 || first.Appended[n-1].Message.Text != "wakeless" {
		t.Fatalf("backstop sync = %+v", first)
	}

	// Wake storm: five wakes for one append must produce one update sync.
	source.append([]byte("{\"type\":\"user\",\"cwd\":\"/work/demo\",\"timestamp\":\"2026-08-01T01:00:07Z\",\"message\":{\"content\":\"storm\"}}\n"))
	for i := 0; i < 5; i++ {
		select {
		case wake <- struct{}{}:
		default:
		}
	}
	second := waitForUpdate()
	if n := len(second.Appended); n == 0 || second.Appended[n-1].Message.Text != "storm" {
		t.Fatalf("storm sync = %+v", second)
	}
	select {
	case extraSync := <-updates:
		t.Fatalf("wake storm produced extra update syncs: %+v", extraSync)
	case <-time.After(60 * time.Millisecond):
	}

	cancel()
	select {
	case <-done:
	case <-time.After(2 * time.Second):
		t.Fatal("pump did not stop on context cancel")
	}
}
