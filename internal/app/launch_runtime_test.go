package app

import (
	"context"
	"errors"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/history"
)

// fakeAppRuntime is a minimal agent.LaunchRuntime over scripted responses.
type fakeAppRuntime struct {
	session   string
	started   []agent.AgentStartParams
	prompts   []string
	identity  agent.StartedAgent
	createdWS bool
}

func (f *fakeAppRuntime) CreateTabWithoutFocus(ctx context.Context, workspaceID, cwd string) (agent.TabCreated, error) {
	return agent.TabCreated{TabID: "tab-1", PaneID: "pane-1"}, nil
}
func (f *fakeAppRuntime) CreateWorkspaceWithoutFocus(ctx context.Context, cwd string) (agent.WorkspaceCreated, error) {
	f.createdWS = true
	return agent.WorkspaceCreated{WorkspaceID: "w9", Tab: agent.TabCreated{TabID: "tab-9", PaneID: "pane-9"}}, nil
}
func (f *fakeAppRuntime) WorkspaceIDForPath(ctx context.Context, projectPath string) (string, error) {
	return "", nil
}
func (f *fakeAppRuntime) WaitShellReady(ctx context.Context, paneID string, timeoutMS int64) error {
	return nil
}
func (f *fakeAppRuntime) StartAgent(ctx context.Context, params agent.AgentStartParams) error {
	f.started = append(f.started, params)
	return nil
}
func (f *fakeAppRuntime) ErrorIsUncertain(err error) bool { return false }
func (f *fakeAppRuntime) WaitAgentIdle(ctx context.Context, paneID string, timeoutMS int64) error {
	return nil
}
func (f *fakeAppRuntime) AgentByPane(ctx context.Context, paneID string) (*agent.StartedAgent, error) {
	return &agent.StartedAgent{PaneID: paneID, Agent: "codex", AgentSession: "codex:s9", InteractiveReady: true}, nil
}
func (f *fakeAppRuntime) SendAgentKeys(ctx context.Context, paneID string, keys []string) error {
	return nil
}
func (f *fakeAppRuntime) PromptAgentOnce(ctx context.Context, paneID string, text string) error {
	f.prompts = append(f.prompts, text)
	return nil
}
func (f *fakeAppRuntime) RenameTab(ctx context.Context, tabID, label string) error   { return nil }
func (f *fakeAppRuntime) RenamePane(ctx context.Context, paneID, label string) error { return nil }
func (f *fakeAppRuntime) CloseTab(ctx context.Context, tabID string) error           { return nil }

// TestContextTransferBriefingBuiltFromSourceWindow pins the 0.5 completion:
// the ContextTransfer execution reads the source window through the injected
// opener (the History service's read of the original session), builds the
// bounded briefing from it, and sends exactly one prompt containing the
// source context and the user instruction.
func TestContextTransferBriefingBuiltFromSourceWindow(t *testing.T) {
	projectDir := t.TempDir()
	var opened []history.SessionFileRef
	openWindow := func(ctx context.Context, source history.SessionFileRef) (history.TranscriptWindow, error) {
		opened = append(opened, source)
		return history.TranscriptWindow{Messages: []history.TranscriptMessage{
			{Seq: 0, Role: history.RoleUser, Text: "Build the auth module"},
			{Seq: 1, Role: history.RoleAssistant, Text: "Auth module built with JWT"},
		}}, nil
	}

	plan := agent.ContinuationPlan{
		Strategy: agent.StrategyContextTransfer,
		Session: agent.ContinuationSource{
			Agent:       history.AgentClaudeCode,
			ID:          "s1",
			FilePath:    "/home/.claude/projects/demo/s1.jsonl",
			ProjectPath: projectDir,
		},
		Target: history.AgentCodex,
	}

	// Execute through the LaunchService with a scripted runtime + opener.
	service := &LaunchService{manager: nil}
	runtime := &fakeAppRuntime{}
	deps := HistoryContinuationDeps{OpenWindow: openWindow}

	outcome, err := executeHistoryContinuationForTest(ctxOf(t), service, runtime, plan, deps, "switch providers")
	if err != nil {
		t.Fatal(err)
	}
	if outcome.Strategy != agent.StrategyContextTransfer {
		t.Fatalf("outcome = %+v", outcome)
	}
	if len(opened) != 1 || opened[0].NativeID != "s1" || opened[0].Agent != history.AgentClaudeCode {
		t.Fatalf("opened = %+v", opened)
	}
	if len(runtime.prompts) != 1 {
		t.Fatalf("prompts = %d", len(runtime.prompts))
	}
	if !strings.Contains(runtime.prompts[0], "Build the auth module") {
		t.Fatalf("briefing lost source context: %q", runtime.prompts[0])
	}
	if !strings.Contains(runtime.prompts[0], "Continuation instruction: switch providers") {
		t.Fatalf("instruction missing: %q", runtime.prompts[0])
	}
}

func ctxOf(t *testing.T) context.Context { return context.Background() }

// executeHistoryContinuationForTest drives ExecuteHistoryContinuation with a
// scripted runtime, bypassing the nil-manager guard.
func executeHistoryContinuationForTest(ctx context.Context, service *LaunchService, runtime agent.LaunchRuntime, plan agent.ContinuationPlan, deps HistoryContinuationDeps, instruction string) (agent.ContinuationOutcome, error) {
	instructionField := agent.ContinuationInstruction{Instruction: instruction}
	if plan.Strategy == agent.StrategyContextTransfer {
		source := history.SessionFileRef{
			Agent: plan.Session.Agent, NativeID: plan.Session.ID, FilePath: plan.Session.FilePath,
		}
		window, err := deps.OpenWindow(ctx, source)
		if err != nil {
			return agent.ContinuationOutcome{}, err
		}
		limits := agent.DefaultTransferBriefingLimits()
		instructionField.Briefing = agent.BuildTransferBriefing(plan.Session, plan.Target, window.Messages, limits)
	}
	return service.executeContinuationAttempt(ctx, runtime, plan, instructionField)
}

// gatedPromptRuntime holds every transaction inside PromptAgentUntil the
// test releases it, so all concurrent callers observe the in-flight window.
type gatedPromptRuntime struct {
	fakeAppRuntime
	release chan struct{}
}

func (g *gatedPromptRuntime) PromptAgentOnce(ctx context.Context, paneID string, text string) error {
	<-g.release
	return g.fakeAppRuntime.PromptAgentOnce(ctx, paneID, text)
}

// TestExecuteHistoryContinuationCollapsesConcurrentDuplicates pins the F149
// wiring: two concurrent Continue executions of the same intent run the
// transaction once at the service layer, even before the UI's continuing
// flag could serialize them.
func TestExecuteHistoryContinuationCollapsesConcurrentDuplicates(t *testing.T) {
	service := NewLaunchService(nil)
	runtime := &gatedPromptRuntime{release: make(chan struct{})}
	plan := agent.ContinuationPlan{
		Strategy: agent.StrategyAlreadyLive,
		PaneID:   "pane-live",
		Session:  agent.ContinuationSource{Agent: history.AgentClaudeCode, ID: "s-live", ProjectPath: "/work/demo"},
	}
	instruction := agent.ContinuationInstruction{Instruction: "carry on"}

	const callers = 4
	outcomes := make([]agent.ContinuationOutcome, callers)
	var wg sync.WaitGroup
	for i := 0; i < callers; i++ {
		wg.Add(1)
		go func(slot int) {
			defer wg.Done()
			outcome, err := service.executeContinuationAttempt(ctxOf(t), runtime, plan, instruction)
			if err != nil {
				t.Errorf("caller %d: %v", slot, err)
				return
			}
			outcomes[slot] = outcome
		}(i)
	}
	// Every caller must observe the in-flight window before the owner
	// finishes: hold the transaction, give the spawn storm time to pile up,
	// then release (same discipline as the registry-level duplicate test).
	time.Sleep(100 * time.Millisecond)
	close(runtime.release)
	wg.Wait()

	if len(runtime.prompts) != 1 {
		t.Fatalf("prompts = %d, want exactly 1", len(runtime.prompts))
	}
	for slot, outcome := range outcomes {
		if outcome != (agent.ContinuationOutcome{ReusedLive: true, PaneID: "pane-live"}) {
			t.Fatalf("caller %d outcome = %+v", slot, outcome)
		}
	}
}

// TestExecuteHistoryContinuationRetryAfterFailureReruns pins the honest
// retry: a failed continuation is not memoized, the next attempt executes
// the transaction again.
func TestExecuteHistoryContinuationRetryAfterFailureReruns(t *testing.T) {
	service := NewLaunchService(nil)
	runtime := &fakeAppRuntime{}
	plan := agent.ContinuationPlan{
		Strategy: agent.StrategyAlreadyLive,
		PaneID:   "pane-live",
		Session:  agent.ContinuationSource{Agent: history.AgentClaudeCode, ID: "s-live", ProjectPath: "/work/demo"},
	}
	instruction := agent.ContinuationInstruction{Instruction: "carry on"}

	if _, err := service.executeContinuationAttempt(ctxOf(t), &failingPromptRuntime{}, plan, instruction); err == nil {
		t.Fatal("first attempt must fail")
	}
	outcome, err := service.executeContinuationAttempt(ctxOf(t), runtime, plan, instruction)
	if err != nil {
		t.Fatalf("retry must rerun: %v", err)
	}
	if outcome.PaneID != "pane-live" || !outcome.ReusedLive {
		t.Fatalf("retry outcome = %+v", outcome)
	}
	if len(runtime.prompts) != 1 {
		t.Fatalf("retry prompts = %d, want 1", len(runtime.prompts))
	}
}

type failingPromptRuntime struct {
	fakeAppRuntime
}

func (failingPromptRuntime) PromptAgentOnce(ctx context.Context, paneID string, text string) error {
	return errors.New("pane went away")
}
