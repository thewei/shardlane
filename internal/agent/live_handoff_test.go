package agent

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/wh-studio/herdr-client/internal/history"
)

// fakeHandoffSource is the deterministic source-read seam: the occupant
// projection is scriptable and the settle wait walks a scripted outcome
// sequence (HANDOFF-04 busy/settled cycles).
type fakeHandoffSource struct {
	mu             sync.Mutex
	source         *HandoffSource
	settleSequence []SettledState
	settleErr      error
	reWork         bool // after settling, the occupant starts working again
	waitCalls      int
}

func (f *fakeHandoffSource) SourceAgent(ctx context.Context, paneID string) (*HandoffSource, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.source == nil {
		return nil, nil
	}
	copied := *f.source
	if f.source.Session != nil {
		session := *f.source.Session
		copied.Session = &session
	}
	return &copied, nil
}

func (f *fakeHandoffSource) WaitAgentSettled(ctx context.Context, paneID string, timeoutMS int64) (SettledState, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.waitCalls++
	if f.settleErr != nil {
		return "", f.settleErr
	}
	state := SettledIdle
	if len(f.settleSequence) > 0 {
		state = f.settleSequence[0]
		f.settleSequence = f.settleSequence[1:]
	}
	switch state {
	case SettledIdle:
		if f.reWork {
			f.source.AgentStatus = "working"
		} else {
			f.source.AgentStatus = "idle"
			f.source.LaunchPending = false
		}
	case SettledBlocked:
		f.source.AgentStatus = "blocked"
	}
	return state, nil
}

// fakeHandoffLaunch is the target-launch harness: exactly-once observables
// plus deterministic post-commit failure injection (HANDOFF-07/08).
type fakeHandoffLaunch struct {
	mu              sync.Mutex
	workspaceID     string
	createdTabs     int
	started         int
	prompts         []string
	startErr        error
	promptErr       error
	promptUncertain bool
	idleErr         error
}

func (f *fakeHandoffLaunch) CreateTabWithoutFocus(ctx context.Context, workspaceID, cwd string) (TabCreated, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.createdTabs++
	return TabCreated{TabID: "tab-1", PaneID: "pane-1"}, nil
}

func (f *fakeHandoffLaunch) CreateWorkspaceWithoutFocus(ctx context.Context, cwd string) (WorkspaceCreated, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	id := f.workspaceID
	if id == "" {
		id = "w-new"
	}
	return WorkspaceCreated{WorkspaceID: id, Tab: TabCreated{TabID: "tab-1", PaneID: "pane-1"}}, nil
}

func (f *fakeHandoffLaunch) WorkspaceIDForPath(ctx context.Context, projectPath string) (string, error) {
	return f.workspaceID, nil
}

func (f *fakeHandoffLaunch) WaitShellReady(ctx context.Context, paneID string, timeoutMS int64) error {
	return nil
}

func (f *fakeHandoffLaunch) StartAgent(ctx context.Context, params AgentStartParams) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.started++
	return f.startErr
}

type handoffUncertainError struct{}

func (e *handoffUncertainError) Error() string { return "delivery uncertain" }

func (f *fakeHandoffLaunch) ErrorIsUncertain(err error) bool {
	var uncertain *handoffUncertainError
	return errors.As(err, &uncertain)
}

func (f *fakeHandoffLaunch) WaitAgentIdle(ctx context.Context, paneID string, timeoutMS int64) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.idleErr
}

func (f *fakeHandoffLaunch) AgentByPane(ctx context.Context, paneID string) (*StartedAgent, error) {
	return &StartedAgent{PaneID: paneID, Agent: HerdrAgentKind(history.AgentCodex), InteractiveReady: true}, nil
}

func (f *fakeHandoffLaunch) SendAgentKeys(ctx context.Context, paneID string, keys []string) error {
	return nil
}

func (f *fakeHandoffLaunch) PromptAgentOnce(ctx context.Context, paneID string, text string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.prompts = append(f.prompts, text)
	if f.promptUncertain {
		return &handoffUncertainError{}
	}
	return f.promptErr
}

func (f *fakeHandoffLaunch) RenameTab(ctx context.Context, tabID, label string) error   { return nil }
func (f *fakeHandoffLaunch) RenamePane(ctx context.Context, paneID, label string) error { return nil }
func (f *fakeHandoffLaunch) CloseTab(ctx context.Context, tabID string) error           { return nil }

const handoffFixture = "{\"type\":\"user\",\"cwd\":\"/work/demo\",\"message\":{\"content\":\"build the thing\"}}\n" +
	"{\"type\":\"assistant\",\"message\":{\"id\":\"m1\",\"content\":[{\"type\":\"text\",\"text\":\"Half done.\"}]}}\n"

type handoffFixtureSet struct {
	dir      string
	request  LiveHandoffRequest
	source   *fakeHandoffSource
	launch   *fakeHandoffLaunch
	timing   HandoffTiming
	identity *HandoffSource
}

func newHandoffFixture(t *testing.T) *handoffFixtureSet {
	t.Helper()
	dir := t.TempDir()
	path := filepath.Join(dir, "live-session.jsonl")
	if err := os.WriteFile(path, []byte(handoffFixture), 0o600); err != nil {
		t.Fatal(err)
	}
	identity := &HandoffSource{
		PaneID:      "pane-7",
		AgentStatus: "idle",
		Session:     &LiveAgentIdentity{Kind: string(history.AgentClaudeCode), Value: "id:live-1"},
	}
	set := &handoffFixtureSet{
		dir: dir,
		request: LiveHandoffRequest{
			Source:             ContinuationSource{Agent: history.AgentClaudeCode, ID: "live-1", FilePath: path, ProjectPath: dir},
			SourcePaneID:       "pane-7",
			Target:             history.AgentCodex,
			SourceConversation: "live:claude-code:pane-7",
		},
		source:   &fakeHandoffSource{source: identity},
		launch:   &fakeHandoffLaunch{},
		timing:   HandoffTiming{SettleTimeoutMS: 100, FlushTimeoutMS: 200, PollMS: 1, MaxSettleCycles: 2},
		identity: identity,
	}
	return set
}

func (s *handoffFixtureSet) run(t *testing.T) (LiveHandoffOutcome, *LiveHandoffFailure) {
	t.Helper()
	return RunLiveHandoff(context.Background(), s.source, s.launch, readyPreparer{}, s.request, nil, nil, s.timing)
}

// TestLiveHandoffIdentityFences pins HANDOFF-01: the occupant must exist and
// prove the exact session identity (id or path kinds); anything else fails
// closed before any runtime mutation.
func TestLiveHandoffIdentityFences(t *testing.T) {
	// Missing occupant.
	set := newHandoffFixture(t)
	set.source.source = nil
	if _, failure := set.run(t); failure == nil || failure.Kind != HandoffSourceUnresolved {
		t.Fatalf("missing occupant failure = %#v", failure)
	}

	// Wrong occupant identity.
	set = newHandoffFixture(t)
	set.identity.Session.Value = "id:other-session"
	if _, failure := set.run(t); failure == nil || failure.Kind != HandoffSourceIdentityChanged {
		t.Fatalf("wrong identity failure = %#v", failure)
	}

	// Occupant without a provider session.
	set = newHandoffFixture(t)
	set.identity.Session = nil
	if _, failure := set.run(t); failure == nil || failure.Kind != HandoffSourceIdentityChanged {
		t.Fatalf("sessionless occupant failure = %#v", failure)
	}

	// Exact id identity proceeds to the launch.
	set = newHandoffFixture(t)
	outcome, failure := set.run(t)
	if failure != nil {
		t.Fatalf("id identity handoff failed: %#v", failure)
	}
	if outcome.SourceFidelity != FidelityStableStat {
		t.Fatalf("settled-entry fidelity = %q", outcome.SourceFidelity)
	}

	// Exact path identity also matches.
	set = newHandoffFixture(t)
	set.identity.Session.Value = "path:" + set.request.Source.FilePath
	if _, failure := set.run(t); failure != nil {
		t.Fatalf("path identity handoff failed: %#v", failure)
	}
}

// TestLiveHandoffEligibilityMatrix pins HANDOFF-02: a blocked source needs
// the Terminal (full-context handoff would lie) and an unknown status is
// refused, both before any target launch.
func TestLiveHandoffEligibilityMatrix(t *testing.T) {
	set := newHandoffFixture(t)
	set.identity.AgentStatus = "blocked"
	if _, failure := set.run(t); failure == nil || failure.Kind != HandoffSourceBlocked {
		t.Fatalf("blocked failure = %#v", failure)
	}
	if set.launch.started != 0 {
		t.Fatal("blocked source must never reach a target launch")
	}

	set = newHandoffFixture(t)
	set.identity.AgentStatus = "mystery-state"
	if _, failure := set.run(t); failure == nil || failure.Kind != HandoffSourceUnresolved {
		t.Fatalf("unknown status failure = %#v", failure)
	}
}

// TestLiveHandoffSettleCycles pins HANDOFF-04: a working source first does
// "Handoff after current turn" (settle → identity re-verification →
// snapshot); a source that keeps starting turns is busy; wait errors and
// settle-to-blocked stay distinct.
func TestLiveHandoffSettleCycles(t *testing.T) {
	// Mid-turn settles once: proceeds with a verified post-settle flush.
	set := newHandoffFixture(t)
	set.identity.AgentStatus = "working"
	set.timing.PollHook = func(iteration int) {
		if iteration == 0 {
			file, err := os.OpenFile(set.request.Source.FilePath, os.O_APPEND|os.O_WRONLY, 0o600)
			if err != nil {
				t.Fatal(err)
			}
			file.WriteString("{\"type\":\"assistant\",\"message\":{\"id\":\"m2\",\"content\":[{\"type\":\"text\",\"text\":\"Rest done.\"}]}}\n")
			file.Close()
		}
	}
	outcome, failure := set.run(t)
	if failure != nil {
		t.Fatalf("settled handoff failed: %#v", failure)
	}
	if outcome.SourceFidelity != FidelityVerifiedFlush || set.source.waitCalls != 1 {
		t.Fatalf("outcome = %+v waitCalls = %d", outcome, set.source.waitCalls)
	}

	// The source keeps starting turns: busy after the bounded cycles.
	set = newHandoffFixture(t)
	set.identity.AgentStatus = "working"
	set.source.reWork = true
	if _, failure := set.run(t); failure == nil || failure.Kind != HandoffSourceBusy {
		t.Fatalf("busy failure = %#v", failure)
	}
	if set.source.waitCalls != set.timing.MaxSettleCycles {
		t.Fatalf("wait calls = %d, want %d", set.source.waitCalls, set.timing.MaxSettleCycles)
	}

	// Settle wait error.
	set = newHandoffFixture(t)
	set.identity.AgentStatus = "working"
	set.source.settleErr = errors.New("wait transport failed")
	if _, failure := set.run(t); failure == nil || failure.Kind != HandoffWaitFailed {
		t.Fatalf("wait failure = %#v", failure)
	}

	// Settling into blocked.
	set = newHandoffFixture(t)
	set.identity.AgentStatus = "working"
	set.source.settleSequence = []SettledState{SettledBlocked}
	if _, failure := set.run(t); failure == nil || failure.Kind != HandoffSourceBlocked {
		t.Fatalf("settled-to-blocked failure = %#v", failure)
	}
}

// TestLiveHandoffFreshnessFence pins HANDOFF-05: only a post-settle advance
// proves VerifiedFlush; a settled-entry source must be quiescent (StableStat
// — explicitly not completeness); any other observation fails closed.
func TestLiveHandoffFreshnessFence(t *testing.T) {
	appendFlush := func(set *handoffFixtureSet) {
		set.timing.PollHook = func(iteration int) {
			if iteration == 0 {
				file, err := os.OpenFile(set.request.Source.FilePath, os.O_APPEND|os.O_WRONLY, 0o600)
				if err != nil {
					t.Fatal(err)
				}
				file.WriteString("{\"type\":\"user\",\"message\":{\"content\":\"more\"}}\n")
				file.Close()
			}
		}
	}

	// Waited, no post-settle flush: stale, fail closed.
	set := newHandoffFixture(t)
	set.identity.AgentStatus = "working"
	if _, failure := set.run(t); failure == nil || failure.Kind != HandoffWaitForSourceFlush {
		t.Fatalf("stale flush failure = %#v", failure)
	}

	// Waited, provider flushes after the settle: verified.
	set = newHandoffFixture(t)
	set.identity.AgentStatus = "working"
	appendFlush(set)
	outcome, failure := set.run(t)
	if failure != nil || outcome.SourceFidelity != FidelityVerifiedFlush {
		t.Fatalf("verified flush = %#v / %#v", outcome, failure)
	}

	// Settled at entry, quiescent: StableStat.
	set = newHandoffFixture(t)
	outcome, failure = set.run(t)
	if failure != nil || outcome.SourceFidelity != FidelityStableStat {
		t.Fatalf("stable stat = %#v / %#v", outcome, failure)
	}

	// Settled at entry but the file moves during the quiesce poll: the
	// snapshot boundary is not quiet — fail closed.
	set = newHandoffFixture(t)
	appendFlush(set)
	if _, failure := set.run(t); failure == nil || failure.Kind != HandoffWaitForSourceFlush {
		t.Fatalf("moving settled source failure = %#v", failure)
	}
}

// TestLiveHandoffPendingOperationFence pins HANDOFF-06: an older accepted
// operation (queued prompt, recoverable failure, uncertain tombstone) blocks
// the snapshot; an empty ledger proceeds.
func TestLiveHandoffPendingOperationFence(t *testing.T) {
	set := newHandoffFixture(t)
	blocking := &fakePendingOperations{unresolved: true}
	if _, failure := RunLiveHandoff(context.Background(), set.source, set.launch, readyPreparer{}, set.request, blocking, nil, set.timing); failure == nil || failure.Kind != HandoffSourceHasPendingOp {
		t.Fatalf("pending-op failure = %#v", failure)
	}
	if set.launch.started != 0 {
		t.Fatal("pending operation must block before any target launch")
	}
	if blocking.asked != 1 {
		t.Fatalf("queue asked %d times", blocking.asked)
	}

	// Without a ledger seam (or after delivery), the handoff proceeds.
	set = newHandoffFixture(t)
	if _, failure := set.run(t); failure != nil {
		t.Fatalf("nil ledger handoff failed: %#v", failure)
	}
}

type fakePendingOperations struct {
	unresolved bool
	asked      int
}

func (f *fakePendingOperations) HasUnresolvedForConversation(id string) bool {
	f.asked++
	return f.unresolved
}

// TestLiveHandoffTransfersExactlyOnce pins HANDOFF-07: exactly one target
// agent, exactly one initial briefing (the instruction appended once), the
// briefing hash covers the delivered prompt, and the source file is left
// byte-identical (§19.5 source protection).
func TestLiveHandoffTransfersExactlyOnce(t *testing.T) {
	set := newHandoffFixture(t)
	set.request.Instruction = "finish the parser"
	before, err := os.ReadFile(set.request.Source.FilePath)
	if err != nil {
		t.Fatal(err)
	}
	outcome, failure := set.run(t)
	if failure != nil {
		t.Fatalf("handoff failed: %#v", failure)
	}
	if set.launch.started != 1 || len(set.launch.prompts) != 1 {
		t.Fatalf("started = %d prompts = %d", set.launch.started, len(set.launch.prompts))
	}
	prompt := set.launch.prompts[0]
	if !strings.Contains(prompt, "Claude Code") || !strings.Contains(prompt, "build the thing") ||
		strings.Count(prompt, "Continuation instruction:") != 1 ||
		!strings.Contains(prompt, "finish the parser") {
		t.Fatalf("briefing prompt = %q", prompt)
	}
	sum := sha256.Sum256([]byte(prompt))
	if outcome.BriefingSHA256 != hex.EncodeToString(sum[:]) {
		t.Fatalf("briefing sha = %q", outcome.BriefingSHA256)
	}
	after, err := os.ReadFile(set.request.Source.FilePath)
	if err != nil {
		t.Fatal(err)
	}
	if string(before) != string(after) {
		t.Fatal("the live source file was mutated by the handoff")
	}

	// The snapshot projection is deterministic: same source and instruction
	// → same hash.
	again := newHandoffFixture(t)
	again.request.Instruction = "finish the parser"
	againOutcome, againFailure := again.run(t)
	if againFailure != nil || againOutcome.BriefingSHA256 != outcome.BriefingSHA256 {
		t.Fatalf("second handoff sha = %q / failure %#v", againOutcome.BriefingSHA256, againFailure)
	}
}

// TestLiveHandoffFailureClasses pins HANDOFF-08 and the §19.6 taxonomy: a
// committed post-commit phase is CreatedNeedsAttention with the exact
// target preserved; a definitively rejected briefing is a transfer failure
// that still names the target; an uncertain briefing never pretends
// certainty.
func TestLiveHandoffFailureClasses(t *testing.T) {
	// Committed not-ready failure (idle wait after a certain start):
	// CreatedNeedsAttention with the exact target preserved.
	set := newHandoffFixture(t)
	set.launch.idleErr = errors.New("readiness window elapsed")
	_, failure := set.run(t)
	if failure == nil || failure.Kind != HandoffCreatedNeedsAttention || !failure.Committed ||
		failure.Phase != PhaseNotReady || failure.PaneID != "pane-1" || failure.TabID != "tab-1" {
		t.Fatalf("not-ready failure = %#v", failure)
	}

	// Definitively rejected briefing: transfer failure naming the target.
	set = newHandoffFixture(t)
	set.launch.promptErr = errors.New("prompt refused")
	_, failure = set.run(t)
	if failure == nil || failure.Kind != HandoffTransferFailed || !failure.Committed ||
		failure.PaneID != "pane-1" || !strings.Contains(failure.Detail, "briefing was rejected") {
		t.Fatalf("rejected briefing failure = %#v", failure)
	}

	// Uncertain briefing: CreatedNeedsAttention, never a blind retry.
	set = newHandoffFixture(t)
	set.launch.promptUncertain = true
	_, failure = set.run(t)
	if failure == nil || failure.Kind != HandoffCreatedNeedsAttention ||
		failure.Phase != PhaseInitialPromptUncertain {
		t.Fatalf("uncertain briefing failure = %#v", failure)
	}

	// Pre-launch failure (empty project): plain transfer failure.
	set = newHandoffFixture(t)
	set.request.Source.ProjectPath = ""
	set.request.ProjectPath = ""
	_, failure = set.run(t)
	if failure == nil || failure.Kind != HandoffTransferFailed || failure.Committed {
		t.Fatalf("pre-launch failure = %#v", failure)
	}
}

// gateReservation is the deterministic reservation seam: Lock blocks until
// the test releases it, proving the reservation is acquired before the
// fences run (HANDOFF-03).
type gateReservation struct {
	acquired chan struct{}
	release  chan struct{}
}

func (g *gateReservation) Lock()   { close(g.acquired); <-g.release }
func (g *gateReservation) Unlock() {}

// TestLiveHandoffReservationHeldBeforeFences pins HANDOFF-03's order
// guarantee: the source reservation is acquired before the first occupant
// read, so a competing prompt cannot slip into the snapshot boundary.
func TestLiveHandoffReservationHeldBeforeFences(t *testing.T) {
	set := newHandoffFixture(t)
	gate := &gateReservation{acquired: make(chan struct{}), release: make(chan struct{})}
	done := make(chan struct{})
	go func() {
		defer close(done)
		RunLiveHandoff(context.Background(), set.source, set.launch, readyPreparer{}, set.request, nil, gate, set.timing)
	}()

	select {
	case <-gate.acquired:
	case <-time.After(2 * time.Second):
		t.Fatal("handoff never acquired the reservation")
	}
	// While the reservation is held, no fence has run yet.
	if set.launch.started != 0 {
		t.Fatal("fences ran before the reservation was granted")
	}
	close(gate.release)
	<-done
	if set.launch.started != 1 {
		t.Fatalf("started = %d after release", set.launch.started)
	}
}

// countedReservation is a real blocking reservation that also observes how
// many fenced sections overlap: if the engine did not hold the reservation
// across its fences, two concurrent handoffs would both increment `held`.
type countedReservation struct {
	inner   sync.Mutex
	held    atomic.Int32
	maxHeld atomic.Int32
}

func (c *countedReservation) Lock() {
	c.inner.Lock()
	if v := c.held.Add(1); v > c.maxHeld.Load() {
		c.maxHeld.Store(v)
	}
}

func (c *countedReservation) Unlock() {
	c.held.Add(-1)
	c.inner.Unlock()
}

// TestLiveHandoffConcurrentCallsSerialize runs two handoffs concurrently on
// one reservation: the fenced sections never overlap (queue/ledger race
// guard) and both complete exactly once.
func TestLiveHandoffConcurrentCallsSerialize(t *testing.T) {
	reservation := &countedReservation{}
	launches := make(chan int, 2)
	for i := 0; i < 2; i++ {
		set := newHandoffFixture(t)
		go func() {
			RunLiveHandoff(context.Background(), set.source, set.launch, readyPreparer{}, set.request, nil, reservation, set.timing)
			launches <- set.launch.started
		}()
	}
	for i := 0; i < 2; i++ {
		select {
		case started := <-launches:
			if started != 1 {
				t.Fatalf("handoff started %d targets", started)
			}
		case <-time.After(5 * time.Second):
			t.Fatal("concurrent handoffs did not finish")
		}
	}
	if reservation.maxHeld.Load() != 1 {
		t.Fatalf("max concurrent fenced sections = %d, want 1", reservation.maxHeld.Load())
	}
}

// TestHandoffTimingNormalization pins the knob defaults (bounded fences by
// construction).
func TestHandoffTimingNormalization(t *testing.T) {
	normalized := HandoffTiming{}.normalized()
	if normalized.SettleTimeoutMS != 60_000 || normalized.FlushTimeoutMS != 5_000 ||
		normalized.PollMS != 250 || normalized.MaxSettleCycles != 3 {
		t.Fatalf("normalized timing = %+v", normalized)
	}
	if fmt.Sprint(DefaultHandoffTiming().MaxSettleCycles) != "3" {
		t.Fatal("default settle cycles changed")
	}
}
