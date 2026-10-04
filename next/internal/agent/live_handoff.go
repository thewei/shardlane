package agent

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"os"
	"strings"
	"sync"
	"time"

	"github.com/wh-studio/herdr-client/next/internal/history"
)

// Live Handoff (0.6 §19 / HANDOFF-01..09): move full semantic context from
// one live Agent to a new Agent/provider while leaving the source untouched.
// The fences run in the audited order — source reservation (caller-held),
// exact-identity read, pending-operation fence, bounded settle cycles,
// source-freshness fence, snapshot, exactly one target briefing — and every
// failure class of §19.6 stays distinct. There is no unsafe "Handoff now":
// a Working source first does "Handoff after current turn".

// SettledState is the typed settle-wait outcome: blocked is a real state the
// source can land in while waiting, never a generic timeout.
type SettledState string

const (
	SettledIdle    SettledState = "idle"
	SettledBlocked SettledState = "blocked"
)

// HandoffSource is the authoritative live-occupant read (§19.2): status and
// launch-pending feed the SAME sendability SSOT as the prompt transaction,
// and the typed session identity proves the exact occupant.
type HandoffSource struct {
	PaneID        string
	AgentStatus   string
	LaunchPending bool
	Revision      int64
	// Session is the occupant's provider session identity (Kind = provider,
	// Value = typed locator "id:<native>"/"path:<file>"); nil when the agent
	// has no provider session yet.
	Session *LiveAgentIdentity
}

// HandoffSourceRuntime is the source-read seam of the handoff fences.
// Production binds the verified Herdr adapter; tests bind deterministic
// fakes.
type HandoffSourceRuntime interface {
	// SourceAgent reads the live occupant of the source pane; nil with no
	// error means the pane has no live agent right now.
	SourceAgent(ctx context.Context, paneID string) (*HandoffSource, error)
	// WaitAgentSettled waits for a working turn to reach idle or blocked.
	WaitAgentSettled(ctx context.Context, paneID string, timeoutMS int64) (SettledState, error)
}

// PendingOperations is the queue seam of the HANDOFF-06 fence: an older
// accepted operation (queued prompt, recoverable failure, uncertain
// delivery) still owns user intent that a snapshot would silently omit.
type PendingOperations interface {
	HasUnresolvedForConversation(id string) bool
}

// LiveHandoffRequest is one planned handoff. The engine owns the target
// agent and prompt; ProjectPath overrides the source project when set.
type LiveHandoffRequest struct {
	Source             ContinuationSource
	SourcePaneID       string
	Target             history.AgentID
	ProjectPath        string
	SourceConversation string
	Instruction        string
}

// LiveHandoffFailureKind enumerates the distinct §19.6 UI outcomes.
type LiveHandoffFailureKind string

const (
	HandoffSourceUnresolved      LiveHandoffFailureKind = "source-unresolved"
	HandoffWaitFailed            LiveHandoffFailureKind = "wait-failed"
	HandoffSourceIdentityChanged LiveHandoffFailureKind = "source-identity-changed"
	HandoffSourceBusy            LiveHandoffFailureKind = "source-busy"
	HandoffSourceBlocked         LiveHandoffFailureKind = "source-blocked"
	HandoffWaitForSourceFlush    LiveHandoffFailureKind = "wait-for-source-flush"
	HandoffSnapshotFailed        LiveHandoffFailureKind = "snapshot-failed"
	HandoffTransferFailed        LiveHandoffFailureKind = "transfer-failed"
	HandoffCreatedNeedsAttention LiveHandoffFailureKind = "created-needs-attention"
	HandoffSourceHasPendingOp    LiveHandoffFailureKind = "source-has-pending-operation"
)

// LiveHandoffFailure is one typed handoff failure. CreatedNeedsAttention
// carries the committed target so clients preserve/navigate to it instead of
// retrying the whole operation (§19.6).
type LiveHandoffFailure struct {
	Kind   LiveHandoffFailureKind
	Detail string
	// Committed target structure: set for CreatedNeedsAttention.
	Committed bool
	TabID     string
	PaneID    string
	Phase     CreatedAgentPhase
}

func (e *LiveHandoffFailure) Error() string {
	return fmt.Sprintf("%s: %s", e.Kind, e.Detail)
}

// SourceFidelity records what the freshness fence PROVED about the snapshot
// (§19.4/P0-08): VerifiedFlush observed a post-settle advance (the flushed
// output of the just-completed turn); StableStat only proved the file
// quiescent, which is not completeness.
type SourceFidelity string

const (
	FidelityVerifiedFlush SourceFidelity = "verified-flush"
	FidelityStableStat    SourceFidelity = "stable-stat"
)

// LiveHandoffOutcome is one executed handoff; the source Agent was never
// stopped, closed, or mutated.
type LiveHandoffOutcome struct {
	Launch         LaunchOutcome
	BriefingSHA256 string
	SourceFidelity SourceFidelity
}

// HandoffTiming carries the fence knobs; tests shrink them and drive the
// flush simulation deterministically through PollHook.
type HandoffTiming struct {
	SettleTimeoutMS int64
	FlushTimeoutMS  int64
	PollMS          int64
	MaxSettleCycles int
	// PollHook runs before every freshness poll (test seam: deterministic
	// provider flush simulation).
	PollHook func(iteration int)
}

func DefaultHandoffTiming() HandoffTiming {
	return HandoffTiming{
		SettleTimeoutMS: 60_000,
		FlushTimeoutMS:  5_000,
		PollMS:          250,
		MaxSettleCycles: 3,
	}
}

func (t HandoffTiming) normalized() HandoffTiming {
	if t.SettleTimeoutMS <= 0 {
		t.SettleTimeoutMS = 60_000
	}
	if t.FlushTimeoutMS <= 0 {
		t.FlushTimeoutMS = 5_000
	}
	if t.PollMS <= 0 {
		t.PollMS = 250
	}
	if t.MaxSettleCycles <= 0 {
		t.MaxSettleCycles = 3
	}
	return t
}

// sourceStat is the freshness-fence observation: mtime plus size.
type sourceStat struct {
	mtimeMS int64
	size    int64
}

func statSource(filePath string) (sourceStat, bool) {
	info, err := os.Stat(filePath)
	if err != nil {
		return sourceStat{}, false
	}
	return sourceStat{mtimeMS: info.ModTime().UnixMilli(), size: info.Size()}, true
}

// handoffState classifies the source through the SAME sendability SSOT as
// the prompt transaction (§19.2/R2-08): never a parallel status reading.
func handoffState(source *HandoffSource) (agentSendabilityState, bool) {
	phase := ParseRuntimePhase(source.AgentStatus, source.LaunchPending)
	switch ClassifySendability(phase) {
	case Sendable:
		return stateSettled, true
	case MidTurn:
		return stateMidTurn, true
	case NeedsTerminal:
		return stateBlocked, true
	default:
		return stateUnknown, false
	}
}

type agentSendabilityState int

const (
	stateSettled agentSendabilityState = iota
	stateMidTurn
	stateBlocked
	stateUnknown
)

// RunLiveHandoff executes one live handoff. Blocking; callers run it on a
// background lane. The Reservation (the per-conversation lock shared with
// the prompt transaction) must be held across the fences and the launch
// classification so a concurrent prompt cannot start a new source turn
// inside the snapshot boundary; nil skips it (deterministic test seam).
func RunLiveHandoff(
	ctx context.Context,
	sourceRuntime HandoffSourceRuntime,
	launchRuntime LaunchRuntime,
	preparer ProjectPreparer,
	request LiveHandoffRequest,
	pending PendingOperations,
	reservation sync.Locker,
	timing HandoffTiming,
) (LiveHandoffOutcome, *LiveHandoffFailure) {
	if reservation != nil {
		reservation.Lock()
		defer reservation.Unlock()
	}
	timing = timing.normalized()

	// 1. Authoritative occupant read + exact identity verification.
	readOccupant := func() (*HandoffSource, *LiveHandoffFailure) {
		source, err := sourceRuntime.SourceAgent(ctx, request.SourcePaneID)
		if err != nil {
			return nil, &LiveHandoffFailure{Kind: HandoffSourceUnresolved, Detail: err.Error()}
		}
		if source == nil {
			return nil, &LiveHandoffFailure{Kind: HandoffSourceUnresolved,
				Detail: "the source agent is no longer present in the runtime projection"}
		}
		return source, nil
	}
	verifyIdentity := func(current *HandoffSource) *LiveHandoffFailure {
		if current.Session != nil && historySessionMatchesHerdrIdentity(
			request.Source.Agent, request.Source.ID, request.Source.FilePath, *current.Session) {
			return nil
		}
		return &LiveHandoffFailure{Kind: HandoffSourceIdentityChanged,
			Detail: "the source conversation changed before the handoff snapshot"}
	}

	current, failure := readOccupant()
	if failure != nil {
		return LiveHandoffOutcome{}, failure
	}
	if failure := verifyIdentity(current); failure != nil {
		return LiveHandoffOutcome{}, failure
	}

	// 2. Pending-operation fence (HANDOFF-06): snapshotting with an older
	// accepted operation in flight would omit that user intent.
	if pending != nil && pending.HasUnresolvedForConversation(request.SourceConversation) {
		return LiveHandoffOutcome{}, &LiveHandoffFailure{Kind: HandoffSourceHasPendingOp,
			Detail: "the source conversation has a pending operation; deliver or cancel it before handing off"}
	}

	baseline, statOK := statSource(request.Source.FilePath)
	if !statOK {
		return LiveHandoffOutcome{}, &LiveHandoffFailure{Kind: HandoffSnapshotFailed,
			Detail: "the source file cannot be stat-probed for a freshness fence"}
	}

	// 3. Bounded settle cycles ("Handoff after current turn", AC-08): a
	// source that keeps starting turns is busy, never an in-flight snapshot.
	waited := false
	settled := false
	for cycle := 0; cycle < timing.MaxSettleCycles; cycle++ {
		state, known := handoffState(current)
		switch {
		case !known:
			return LiveHandoffOutcome{}, &LiveHandoffFailure{Kind: HandoffSourceUnresolved,
				Detail: "the source agent status is unknown; refusing an unproven snapshot"}
		case state == stateBlocked:
			return LiveHandoffOutcome{}, &LiveHandoffFailure{Kind: HandoffSourceBlocked,
				Detail: "the source agent is blocked and needs the Terminal; resolve it before handing off full context"}
		case state == stateSettled:
			settled = true
		case state == stateMidTurn:
			waited = true
			settleState, err := sourceRuntime.WaitAgentSettled(ctx, request.SourcePaneID, timing.SettleTimeoutMS)
			if err != nil {
				return LiveHandoffOutcome{}, &LiveHandoffFailure{Kind: HandoffWaitFailed, Detail: err.Error()}
			}
			if settleState == SettledBlocked {
				return LiveHandoffOutcome{}, &LiveHandoffFailure{Kind: HandoffSourceBlocked,
					Detail: "the source agent is blocked and needs the Terminal; resolve it before handing off full context"}
			}
			if current, failure = readOccupant(); failure != nil {
				return LiveHandoffOutcome{}, failure
			}
			if failure := verifyIdentity(current); failure != nil {
				return LiveHandoffOutcome{}, failure
			}
		}
		if settled {
			break
		}
	}
	if !settled {
		return LiveHandoffOutcome{}, &LiveHandoffFailure{Kind: HandoffSourceBusy,
			Detail: "the source agent kept starting turns; retry the handoff when it settles"}
	}

	// 4. Source-freshness fence (§19.4/AC-07): a stat advance observed
	// before the settle proves nothing. The proof is a post-settle advance
	// (waited) or two consecutive identical stats (already settled,
	// quiescent — not completeness). A provider that cannot satisfy its
	// fence fails closed instead of transferring a hash-valid but stale
	// "full context".
	var fidelity SourceFidelity
	advance := func(iteration int) bool {
		if timing.PollHook != nil {
			timing.PollHook(iteration)
		}
		stat, ok := statSource(request.Source.FilePath)
		return ok && (stat.mtimeMS > baseline.mtimeMS || stat.size > baseline.size)
	}
	if waited {
		deadline := time.Now().Add(time.Duration(timing.FlushTimeoutMS) * time.Millisecond)
		for iteration := 0; ; iteration++ {
			if advance(iteration) {
				fidelity = FidelityVerifiedFlush
				break
			}
			if !time.Now().Before(deadline) {
				return LiveHandoffOutcome{}, &LiveHandoffFailure{Kind: HandoffWaitForSourceFlush,
					Detail: "the source did not flush the completed turn yet; retry the handoff shortly"}
			}
			sleepContext(ctx, timing.PollMS)
			if ctx.Err() != nil {
				return LiveHandoffOutcome{}, &LiveHandoffFailure{Kind: HandoffWaitFailed, Detail: ctx.Err().Error()}
			}
		}
	} else {
		// Already settled at entry: the baseline was sampled at entry, so
		// one quiesce poll with an unchanged re-stat proves quiescence.
		if advance(0) {
			return LiveHandoffOutcome{}, &LiveHandoffFailure{Kind: HandoffWaitForSourceFlush,
				Detail: "the source is still flushing; retry the handoff shortly"}
		}
		fidelity = FidelityStableStat
	}

	// 5. Snapshot + briefing. The source Agent is never stopped, closed, or
	// mutated; the parsed file is the immutable boundary the reservation
	// protected.
	briefing, sha, failure := buildLiveHandoffBriefing(request, timing)
	if failure != nil {
		return LiveHandoffOutcome{}, failure
	}

	// 6. Canonical target launch with exactly one initial briefing.
	projectPath := strings.TrimSpace(request.ProjectPath)
	if projectPath == "" {
		projectPath = request.Source.ProjectPath
	}
	intent := LaunchIntent{
		OperationID: "handoff-" + request.Source.ID,
		ProjectPath: projectPath,
		Mode:        LaunchModeBuild,
		Permission:  PermissionAskApproval,
		Agent:       request.Target,
		Prompt:      briefing,
	}
	outcome, launchFailure := RunLaunchTransaction(ctx, launchRuntime, preparer, intent, DefaultTimings())
	if launchFailure == nil {
		return LiveHandoffOutcome{Launch: outcome, BriefingSHA256: sha, SourceFidelity: fidelity}, nil
	}
	// §19.6: a committed target whose briefing was definitively rejected is
	// a transfer failure that names the target; any other committed or
	// uncertain post-commit state must be preserved as
	// CreatedNeedsAttention — never retried into a duplicate.
	if launchFailure.Committed && launchFailure.Phase == PhaseInitialPrompt {
		return LiveHandoffOutcome{}, &LiveHandoffFailure{Kind: HandoffTransferFailed,
			Detail:    fmt.Sprintf("the target agent started (pane=%s, tab=%s) but the briefing was rejected: %s", launchFailure.PaneID, launchFailure.TabID, launchFailure.Detail),
			Committed: true, TabID: launchFailure.TabID, PaneID: launchFailure.PaneID, Phase: launchFailure.Phase,
		}
	}
	if launchFailure.Committed || launchFailure.Uncertain {
		return LiveHandoffOutcome{}, &LiveHandoffFailure{Kind: HandoffCreatedNeedsAttention,
			Detail:    launchFailure.Detail,
			Committed: true, TabID: launchFailure.TabID, PaneID: launchFailure.PaneID, Phase: launchFailure.Phase,
		}
	}
	return LiveHandoffOutcome{}, &LiveHandoffFailure{Kind: HandoffTransferFailed, Detail: launchFailure.Detail}
}

// buildLiveHandoffBriefing parses the settled live provider source and
// builds the single bounded briefing prompt plus its hash.
func buildLiveHandoffBriefing(request LiveHandoffRequest, timing HandoffTiming) (string, string, *LiveHandoffFailure) {
	reference := history.SessionFileRef{Agent: request.Source.Agent, NativeID: request.Source.ID, FilePath: request.Source.FilePath}
	messages, err := parseLiveSource(reference)
	if err != nil {
		return "", "", &LiveHandoffFailure{Kind: HandoffSnapshotFailed, Detail: err.Error()}
	}
	briefing := BuildTransferBriefing(request.Source, request.Target, messages, DefaultTransferBriefingLimits())
	if instruction := strings.TrimSpace(request.Instruction); instruction != "" {
		briefing += "\n\nContinuation instruction: " + instruction
	}
	sum := sha256.Sum256([]byte(briefing))
	return briefing, hex.EncodeToString(sum[:]), nil
}

// parseLiveSource dispatches the settled live file to the shipped adapters.
// Providers without a parser fail closed — a handoff never guesses.
func parseLiveSource(reference history.SessionFileRef) ([]history.TranscriptMessage, error) {
	var parsed history.ParsedTranscript
	var err error
	switch reference.Agent {
	case history.AgentClaudeCode:
		parsed, err = history.ParseClaudeTranscript(reference)
	case history.AgentCodex:
		parsed, err = history.ParseCodexTranscript(reference)
	default:
		return nil, fmt.Errorf("no live-source parser for provider %q", reference.Agent)
	}
	if err != nil {
		return nil, err
	}
	return parsed.Mainline, nil
}

// sleepContext sleeps for the poll interval but wakes early on cancellation.
func sleepContext(ctx context.Context, ms int64) {
	timer := time.NewTimer(time.Duration(ms) * time.Millisecond)
	defer timer.Stop()
	select {
	case <-ctx.Done():
	case <-timer.C:
	}
}
