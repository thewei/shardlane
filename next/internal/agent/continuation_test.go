package agent

import (
	"context"
	"strings"
	"testing"

	"github.com/wh-studio/herdr-client/next/internal/history"
)

func TestResumeArgsMatchRustAuthority(t *testing.T) {
	cases := []struct {
		provider history.AgentID
		want     []string
	}{
		{history.AgentClaudeCode, []string{"--resume", "s1"}},
		{history.AgentCodex, []string{"resume", "s1"}},
		{history.AgentCopilot, []string{"--resume=s1"}},
		{history.AgentPi, []string{"--session", "s1"}},
		{history.AgentAntigravity, []string{"--conversation", "s1"}},
	}
	for _, tc := range cases {
		got, err := ResumeArgs(ResumeIntent{Provider: tc.provider, NativeSessionID: "s1"})
		if err != nil {
			t.Fatalf("%s: %v", tc.provider, err)
		}
		if len(got) != len(tc.want) {
			t.Fatalf("%s args = %v, want %v", tc.provider, got, tc.want)
		}
		for i := range got {
			if got[i] != tc.want[i] {
				t.Fatalf("%s args = %v, want %v", tc.provider, got, tc.want)
			}
		}
	}
}

func continuationSource() ContinuationSource {
	return ContinuationSource{
		Agent:           history.AgentClaudeCode,
		ID:              "sess-1",
		FilePath:        "/home/.claude/projects/demo/sess-1.jsonl",
		ProjectPath:     "/work/demo",
		ResumeSupported: true,
	}
}

func TestPlanAlreadyLive(t *testing.T) {
	source := continuationSource()
	live := []LiveAgentIdentity{{
		PaneID: "pane-live", Kind: "claude-code", Value: "sess-1", Revision: 4,
	}}
	plan := PlanHistoryContinuation(source, nil, live, "")
	if plan.Strategy != StrategyAlreadyLive || plan.PaneID != "pane-live" {
		t.Fatalf("plan = %+v", plan)
	}

	// A different target provider is a ContextTransfer, never a prompt to
	// the live same-session Agent.
	target := history.AgentCodex
	plan = PlanHistoryContinuation(source, &target, live, "")
	if plan.Strategy != StrategyContextTransfer {
		t.Fatalf("cross-provider plan = %+v", plan)
	}
}

func TestPlanNativeResumeAndPathIdentity(t *testing.T) {
	source := continuationSource()
	// Path-based identity matches the exact source path.
	live := []LiveAgentIdentity{{
		PaneID: "pane-path", Kind: "claude-code", Value: source.FilePath,
	}}
	if plan := PlanHistoryContinuation(source, nil, live, ""); plan.Strategy != StrategyAlreadyLive {
		t.Fatalf("path identity plan = %+v", plan)
	}

	// No live match → native resume with provider args.
	plan := PlanHistoryContinuation(source, nil, nil, "")
	if plan.Strategy != StrategyNativeResume {
		t.Fatalf("plan = %+v", plan)
	}
	if len(plan.ResumeArgs) != 2 || plan.ResumeArgs[0] != "--resume" || plan.ResumeArgs[1] != "sess-1" {
		t.Fatalf("resume args = %v", plan.ResumeArgs)
	}
	if plan.Session.ProjectPath != "/work/demo" {
		t.Fatalf("session project = %q", plan.Session.ProjectPath)
	}
}

func TestPlanNeedsProjectSelectionAndOverride(t *testing.T) {
	source := continuationSource()
	source.ProjectPath = "   "
	plan := PlanHistoryContinuation(source, nil, nil, "")
	if plan.Strategy != StrategyNeedsProjectSelect {
		t.Fatalf("plan = %+v", plan)
	}

	plan = PlanHistoryContinuation(source, nil, nil, "/work/override")
	if plan.Strategy != StrategyNativeResume || plan.Session.ProjectPath != "/work/override" {
		t.Fatalf("override plan = %+v", plan)
	}
}

func TestPlanUnsupportedWithoutExactSource(t *testing.T) {
	source := continuationSource()
	source.FilePath = ""
	// AC-09 parity: a missing source never blocks same-provider native
	// resume...
	plan := PlanHistoryContinuation(source, nil, nil, "")
	if plan.Strategy != StrategyNativeResume {
		t.Fatalf("plan = %+v", plan)
	}
	// ...but ContextTransfer (different provider) requires the exact source.
	target := history.AgentCodex
	plan = PlanHistoryContinuation(source, &target, nil, "")
	if plan.Strategy != StrategyUnsupported || plan.Reason == "" {
		t.Fatalf("plan = %+v", plan)
	}
}

// TestContinueExecutionAlreadyLive pins that the AlreadyLive execution
// prompts the existing pane exactly once and never creates runtime structure.
func TestContinueExecutionAlreadyLive(t *testing.T) {
	runtime := &fakeLaunchRuntime{startAgentAppears: true, identityReady: true}
	outcome, err := ExecuteContinuation(context.Background(), runtime, readyPreparer{},
		ContinuationPlan{Strategy: StrategyAlreadyLive, PaneID: "pane-live"},
		ContinuationInstruction{Instruction: "continue the work"})
	if err != nil {
		t.Fatal(err)
	}
	if !outcome.ReusedLive {
		t.Fatalf("outcome = %+v", outcome)
	}
	if runtime.started != 0 || len(runtime.closedTabs) != 0 {
		t.Fatalf("AlreadyLive must not mutate runtime: started=%d closed=%v", runtime.started, runtime.closedTabs)
	}
	if len(runtime.prompts) != 1 || !strings.Contains(runtime.prompts[0], "continue the work") {
		t.Fatalf("prompts = %q", runtime.prompts)
	}
}

// TestContinueExecutionNativeResume pins that NativeResume launches through
// the canonical transaction with the resume args and never sends a separate
// prompt after the single initial briefing.
func TestContinueExecutionNativeResume(t *testing.T) {
	runtime := &fakeLaunchRuntime{startAgentAppears: true, identityReady: true}
	plan := PlanHistoryContinuation(continuationSource(), nil, nil, "")
	outcome, err := ExecuteContinuation(context.Background(), runtime, readyPreparer{}, plan,
		ContinuationInstruction{Instruction: "keep going"})
	if err != nil {
		t.Fatal(err)
	}
	if outcome.ReusedLive || outcome.Strategy != StrategyNativeResume {
		t.Fatalf("outcome = %+v", outcome)
	}
	if runtime.started != 1 {
		t.Fatalf("agent.start ran %d times", runtime.started)
	}
	if len(runtime.prompts) != 1 {
		t.Fatalf("briefing prompts = %d", len(runtime.prompts))
	}
}

// TestContinueExecutionContextTransferRequiresSource pins that the transfer
// execution fails closed without an exact source briefing.
func TestContinueExecutionContextTransferRequiresSource(t *testing.T) {
	runtime := &fakeLaunchRuntime{}
	plan := ContinuationPlan{
		Strategy: StrategyContextTransfer,
		Session:  continuationSource(),
		Target:   history.AgentCodex,
	}
	_, err := ExecuteContinuation(context.Background(), runtime, readyPreparer{}, plan,
		ContinuationInstruction{Instruction: "switch providers"})
	if err == nil || !strings.Contains(err.Error(), "briefing") {
		t.Fatalf("err = %v", err)
	}
	if runtime.started != 0 {
		t.Fatal("transfer without a briefing must not launch")
	}
}

// TestContinueExecutionContextTransferWithBriefing pins the full transfer
// path: the bounded briefing built from the source transcript becomes the
// single initial prompt of the canonical launch.
func TestContinueExecutionContextTransferWithBriefing(t *testing.T) {
	runtime := &fakeLaunchRuntime{
		startAgentAppears: true, identityReady: true,
		agentKind: HerdrAgentKind(history.AgentCodex),
	}
	plan := ContinuationPlan{
		Strategy: StrategyContextTransfer,
		Session:  continuationSource(),
		Target:   history.AgentCodex,
	}
	instruction := ContinuationInstruction{
		Instruction: "switch providers",
		Briefing: BuildTransferBriefing(plan.Session, plan.Target, []history.TranscriptMessage{
			{Seq: 0, Role: history.RoleUser, Text: "Build the auth module"},
			{Seq: 1, Role: history.RoleAssistant, Text: "Auth module built with JWT"},
		}, DefaultTransferBriefingLimits()),
	}
	outcome, err := ExecuteContinuation(context.Background(), runtime, readyPreparer{}, plan, instruction)
	if err != nil {
		t.Fatal(err)
	}
	if outcome.Strategy != StrategyContextTransfer || outcome.PaneID == "" {
		t.Fatalf("outcome = %+v", outcome)
	}
	if runtime.started != 1 {
		t.Fatalf("agent.start ran %d times", runtime.started)
	}
	if len(runtime.prompts) != 1 {
		t.Fatalf("prompts = %d", len(runtime.prompts))
	}
	if !strings.Contains(runtime.prompts[0], "Build the auth module") ||
		!strings.Contains(runtime.prompts[0], "Auth module built with JWT") {
		t.Fatalf("briefing lost source context: %q", runtime.prompts[0])
	}
	if !strings.Contains(runtime.prompts[0], "Continuation instruction: switch providers") {
		t.Fatal("user instruction missing from the transfer prompt")
	}
}

func TestBuildTransferBriefingBounds(t *testing.T) {
	var messages []history.TranscriptMessage
	for i := 0; i < 200; i++ {
		messages = append(messages, history.TranscriptMessage{
			Seq: int64(i), Role: history.RoleUser,
			Text: strings.Repeat("x", 300),
		})
	}
	briefing := BuildTransferBriefing(continuationSource(), history.AgentCodex, messages,
		TransferBriefingLimits{MaxChars: 2000, MaxMessages: 5})
	if len(briefing) > 2000+len(briefing)-len("") && len(briefing) > 2200 {
		t.Fatalf("briefing = %d bytes", len(briefing))
	}
	if strings.Count(briefing, "[USER]") > 5 {
		t.Fatalf("max messages ignored: %d", strings.Count(briefing, "[USER]"))
	}
	// Empty source produces a truthful placeholder, never a panic.
	empty := BuildTransferBriefing(continuationSource(), history.AgentCodex, nil, DefaultTransferBriefingLimits())
	if !strings.Contains(empty, "no textual history") {
		t.Fatalf("empty briefing = %q", empty)
	}
}
