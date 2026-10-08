package agent

import (
	"testing"
	"time"

	"github.com/wh-studio/herdr-client/internal/history"
)

func TestParseRuntimePhase(t *testing.T) {
	cases := []struct {
		status        string
		launchPending bool
		want          AgentRuntimePhase
	}{
		{"working", false, PhaseWorking},
		{"WORKING", false, PhaseWorking},
		{"blocked", false, PhaseBlocked},
		{"idle", false, PhaseIdle},
		{"done", false, PhaseDone},
		{"failed", false, PhaseFailed},
		{"mystery", false, PhaseUnknown},
		{"", false, PhaseUnknown},
		// The typed launch_pending flag outranks the status string.
		{"idle", true, PhaseLaunching},
		{"working", true, PhaseLaunching},
	}
	for _, tc := range cases {
		if got := ParseRuntimePhase(tc.status, tc.launchPending); got != tc.want {
			t.Fatalf("ParseRuntimePhase(%q, %v) = %v, want %v", tc.status, tc.launchPending, got, tc.want)
		}
	}
}

func TestClassifySendability(t *testing.T) {
	cases := map[AgentRuntimePhase]AgentSendability{
		PhaseWorking:   MidTurn,
		PhaseLaunching: MidTurn,
		PhaseBlocked:   NeedsTerminal,
		PhaseFailed:    NeedsTerminal,
		PhaseIdle:      Sendable,
		PhaseDone:      Sendable,
		PhaseUnknown:   SendUnknown,
	}
	for phase, want := range cases {
		if got := ClassifySendability(phase); got != want {
			t.Fatalf("ClassifySendability(%v) = %v, want %v", phase, got, want)
		}
	}
}

func TestClassifyTransition(t *testing.T) {
	cases := []struct {
		previous, next AgentRuntimePhase
		want           AgentTransition
	}{
		{PhaseUnknown, PhaseWorking, TransitionFirstObservation},
		{PhaseWorking, PhaseWorking, TransitionNone},
		{PhaseWorking, PhaseDone, TransitionReadyForReview},
		{PhaseBlocked, PhaseDone, TransitionReadyForReview},
		{PhaseWorking, PhaseBlocked, TransitionNeedsAttention},
		{PhaseIdle, PhaseBlocked, TransitionNone},
		{PhaseWorking, PhaseIdle, TransitionReady},
		{PhaseWorking, PhaseFailed, TransitionFailed},
		{PhaseDone, PhaseWorking, TransitionReviewCleared},
		{PhaseIdle, PhaseDone, TransitionNone},
	}
	for _, tc := range cases {
		if got := ClassifyTransition(tc.previous, tc.next); got != tc.want {
			t.Fatalf("ClassifyTransition(%v→%v) = %v, want %v", tc.previous, tc.next, got, tc.want)
		}
	}
}

// TestMarkerStoreLifecycle pins the original marker semantics end to end:
// transitions create markers only off-screen, visiting clears unread only,
// explicit review clears both, done→working clears review, release removes.
func TestMarkerStoreLifecycle(t *testing.T) {
	key := AgentKey{InstanceID: "default", TerminalID: "term-1"}
	store := NewMarkerStore()

	// Initial projection never manufactures markers.
	if markers := store.Observe(key, TransitionFirstObservation, false); markers.Unread || markers.ReviewPending {
		t.Fatalf("initial projection manufactured markers: %+v", markers)
	}

	// working → done off-screen starts review + unread.
	if markers := store.Observe(key, TransitionReadyForReview, false); !markers.ReviewPending || !markers.Unread {
		t.Fatalf("ready-for-review markers = %+v", markers)
	}
	// Visiting clears unread but not review-pending.
	if markers := store.Visit(key); markers.Unread || !markers.ReviewPending {
		t.Fatalf("visited markers = %+v", markers)
	}
	// done → working clears the stale review.
	if markers := store.Observe(key, TransitionReviewCleared, true); markers.ReviewPending {
		t.Fatalf("review not cleared: %+v", markers)
	}
	// blocked off-screen marks unread again.
	if markers := store.Observe(key, TransitionNeedsAttention, false); !markers.Unread {
		t.Fatalf("attention unread = %+v", markers)
	}
	// The pre-existing unread persists until the Agent is visited.
	if markers := store.Get(key); !markers.Unread {
		t.Fatalf("unread lost before visit: %+v", markers)
	}
	// Attention on-screen does not CREATE unread for a fresh Agent.
	onScreen := AgentKey{InstanceID: "default", TerminalID: "term-onscreen"}
	if markers := store.Observe(onScreen, TransitionNeedsAttention, true); markers.Unread {
		t.Fatalf("on-screen attention created unread: %+v", markers)
	}
	// Explicit Mark reviewed clears everything.
	store.Observe(key, TransitionReadyForReview, false)
	if markers := store.MarkReviewed(key); markers.Unread || markers.ReviewPending {
		t.Fatalf("mark reviewed markers = %+v", markers)
	}
	// Release removes the entry entirely.
	store.Observe(key, TransitionReadyForReview, false)
	store.Release(key)
	if markers := store.Get(key); markers.Unread || markers.ReviewPending {
		t.Fatalf("released markers = %+v", markers)
	}
}

func TestMarkerStoreRetainBoundsToLiveAgents(t *testing.T) {
	store := NewMarkerStore()
	live := AgentKey{InstanceID: "default", TerminalID: "term-live"}
	gone := AgentKey{InstanceID: "default", TerminalID: "term-gone"}
	store.Observe(live, TransitionReadyForReview, false)
	store.Observe(gone, TransitionReadyForReview, false)

	store.Retain(map[AgentKey]bool{live: true})
	if markers := store.Get(gone); markers.Unread || markers.ReviewPending {
		t.Fatal("released Agent markers survived Retain")
	}
	if markers := store.Get(live); !markers.ReviewPending {
		t.Fatalf("live markers lost: %+v", markers)
	}
}

func TestProjectAgentCard(t *testing.T) {
	model := ProjectAgentCard(AgentCardInputs{
		InstanceID:  "default",
		TerminalID:  "term-1",
		PaneID:      "pane-1",
		TabID:       "tab-1",
		ProjectID:   "w1",
		ProjectName: "Demo",
		Provider:    history.AgentClaudeCode,
		Title:       "", Name: "", // fallback chain exercised
		AgentStatus: "working",
		Attention:   1,
		Markers:     AgentMarkers{Unread: true},
		Revision:    7,
	})
	if model.Title != "Claude Code" {
		t.Fatalf("title fallback = %q", model.Title)
	}
	if model.RuntimePhase != PhaseWorking || model.Sendability != MidTurn {
		t.Fatalf("phase/sendability = %v/%v", model.RuntimePhase, model.Sendability)
	}
	if !model.Unread || model.ReviewPending {
		t.Fatalf("markers = %+v", model)
	}
	if model.PrimaryAction() != ActionOpenAgent {
		t.Fatalf("working primary action = %v", model.PrimaryAction())
	}

	// Working is the honest mid-flight dimension: working/launching
	// runtime phases and the working operational projection count; idle,
	// done, blocked, failed and the hysteresis window do not. The
	// directory's Working bucket shares this predicate.
	if !model.Working() {
		t.Fatal("a working card must report Working")
	}
	for _, status := range []string{"idle", "done", "blocked", "failed"} {
		card := ProjectAgentCard(AgentCardInputs{TerminalID: "term-x", AgentStatus: status})
		if card.Working() {
			t.Fatalf("status %q must not report Working", status)
		}
	}
	launching := ProjectAgentCard(AgentCardInputs{TerminalID: "term-x", AgentStatus: "idle", LaunchPending: true})
	if !launching.Working() {
		t.Fatal("a launch-pending card must report Working")
	}
	idleButOpWorking := ProjectAgentCard(AgentCardInputs{TerminalID: "term-x", AgentStatus: "idle", Attention: OpWorking})
	if !idleButOpWorking.Working() {
		t.Fatal("the working operational projection must report Working")
	}
	hysteresis := model
	hysteresis.RuntimePhase, hysteresis.Attention, hysteresis.ActiveUntil = PhaseIdle, OpIdle, time.Now().Add(time.Minute)
	if hysteresis.Working() {
		t.Fatal("the sort hysteresis must not make an idle card Working")
	}

	// Blocked agents route to the Terminal, never a generic prompt.
	model = ProjectAgentCard(AgentCardInputs{
		InstanceID: "default", TerminalID: "term-2",
		Provider: history.AgentCodex, AgentStatus: "blocked",
	})
	if model.PrimaryAction() != ActionOpenTerminal {
		t.Fatalf("blocked primary action = %v", model.PrimaryAction())
	}
	if model.Sendability != NeedsTerminal {
		t.Fatalf("blocked sendability = %v", model.Sendability)
	}

	// Done + review pending offers the explicit review action.
	model = ProjectAgentCard(AgentCardInputs{
		InstanceID: "default", TerminalID: "term-3",
		Provider: history.AgentCodex, AgentStatus: "done",
		Markers: AgentMarkers{ReviewPending: true},
	})
	if model.PrimaryAction() != ActionNeedsReview {
		t.Fatalf("review primary action = %v", model.PrimaryAction())
	}

	// Unknown status fails closed.
	model = ProjectAgentCard(AgentCardInputs{
		InstanceID: "default", TerminalID: "term-4",
		Provider: history.AgentCodex, AgentStatus: "mystery",
	})
	if model.Sendability != SendUnknown || model.RuntimePhase != PhaseUnknown {
		t.Fatalf("unknown card = %v/%v", model.Sendability, model.RuntimePhase)
	}
}
