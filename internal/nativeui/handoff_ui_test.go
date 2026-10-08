package nativeui

import (
	"strings"
	"testing"

	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/herdr"
	"github.com/wh-studio/herdr-client/internal/history"
)

// TestProjectHandoffActionStates pins HANDOFF-09's action state machine:
// every §19.6 failure class maps to exactly one distinct user outcome, and
// only committed targets navigate client-locally — never a blind retry.
func TestProjectHandoffActionStates(t *testing.T) {
	success := &agent.LiveHandoffOutcome{SourceFidelity: agent.FidelityVerifiedFlush, BriefingSHA256: "sha"}
	states := map[string]handoffActionState{
		"success":    projectHandoffAction(success, nil),
		"stable":     projectHandoffAction(&agent.LiveHandoffOutcome{SourceFidelity: agent.FidelityStableStat}, nil),
		"unresolved": projectHandoffAction(nil, &agent.LiveHandoffFailure{Kind: agent.HandoffSourceUnresolved}),
		"wait":       projectHandoffAction(nil, &agent.LiveHandoffFailure{Kind: agent.HandoffWaitFailed}),
		"identity":   projectHandoffAction(nil, &agent.LiveHandoffFailure{Kind: agent.HandoffSourceIdentityChanged}),
		"busy":       projectHandoffAction(nil, &agent.LiveHandoffFailure{Kind: agent.HandoffSourceBusy}),
		"blocked":    projectHandoffAction(nil, &agent.LiveHandoffFailure{Kind: agent.HandoffSourceBlocked}),
		"flush":      projectHandoffAction(nil, &agent.LiveHandoffFailure{Kind: agent.HandoffWaitForSourceFlush}),
		"snapshot":   projectHandoffAction(nil, &agent.LiveHandoffFailure{Kind: agent.HandoffSnapshotFailed}),
		"pending-op": projectHandoffAction(nil, &agent.LiveHandoffFailure{Kind: agent.HandoffSourceHasPendingOp}),
		"cna": projectHandoffAction(nil, &agent.LiveHandoffFailure{
			Kind: agent.HandoffCreatedNeedsAttention, Committed: true, PaneID: "pane-9", Detail: "readiness window elapsed",
		}),
		"transfer": projectHandoffAction(nil, &agent.LiveHandoffFailure{
			Kind: agent.HandoffTransferFailed, Committed: true, PaneID: "pane-1", Detail: "briefing rejected",
		}),
		"pre-launch": projectHandoffAction(nil, &agent.LiveHandoffFailure{
			Kind: agent.HandoffTransferFailed, Detail: "no runtime",
		}),
	}

	messages := map[string]bool{}
	for name, state := range states {
		if state.Message == "" {
			t.Fatalf("%s produced no message", name)
		}
		if messages[state.Message] {
			t.Fatalf("%s duplicated an existing outcome message: %q", name, state.Message)
		}
		messages[state.Message] = true
	}

	if got := states["success"].Message; got != "Handed off with a verified source flush." {
		t.Fatalf("success message = %q", got)
	}
	if got := states["stable"].Message; !strings.Contains(got, "not proven complete") {
		t.Fatalf("stable-stat message must preserve the fidelity caveat: %q", got)
	}
	// Only committed targets navigate.
	if states["cna"].NavigatePane != "pane-9" {
		t.Fatalf("created-needs-attention must route to the committed pane: %+v", states["cna"])
	}
	if states["transfer"].NavigatePane != "pane-1" {
		t.Fatalf("committed transfer must name its target pane: %+v", states["transfer"])
	}
	if states["pre-launch"].NavigatePane != "" {
		t.Fatalf("pre-launch failure must not navigate: %+v", states["pre-launch"])
	}
	for _, name := range []string{"success", "unresolved", "busy", "blocked", "flush", "pending-op"} {
		if states[name].NavigatePane != "" {
			t.Fatalf("%s must not navigate: %+v", name, states[name])
		}
	}
}

// TestSplitHandoffIdentity pins the typed-locator split feeding the handoff
// request's exact source facts.
func TestSplitHandoffIdentity(t *testing.T) {
	if id, path := splitHandoffIdentity("id:session-1"); id != "session-1" || path != "" {
		t.Fatalf("id split = %q / %q", id, path)
	}
	if id, path := splitHandoffIdentity("path:/tmp/live.jsonl"); id != "" || path != "/tmp/live.jsonl" {
		t.Fatalf("path split = %q / %q", id, path)
	}
	if id, _ := splitHandoffIdentity("bare-id"); id != "bare-id" {
		t.Fatalf("bare split = %q", id)
	}
}

// TestRequestLiveHandoffGuards pins the action guards: without a launch
// service or a resolvable live session the action refuses up front and
// never touches the runtime.
func TestRequestLiveHandoffGuards(t *testing.T) {
	shell, card := liveBindShell(t, "claude-code", "/tmp/does-not-matter.jsonl")

	// No resolvable identity in the projection: the refusal surfaces before
	// any runtime access (NewShell binds a default launch service).
	shell.projection = herdr.Projection{}
	shell.requestLiveHandoff(card, history.AgentCodex, "move it")
	if shell.chatOutcome == "" {
		t.Fatal("missing identity must surface a refusal")
	}

	// No launch service bound: the action returns without touching state.
	shell.launch = nil
	shell.chatOutcome = ""
	shell.requestLiveHandoff(card, history.AgentCodex, "move it")
	if shell.chatOutcome != "" {
		t.Fatalf("guard message without launch = %q", shell.chatOutcome)
	}
}
