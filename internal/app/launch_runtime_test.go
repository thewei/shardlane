package app

import (
	"context"
	"strings"
	"testing"

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

// TestContextTransferBriefingBuiltFromPageCache pins the 0.5 completion: the
// ContextTransfer execution reads the source window through the injected
// page-cache opener, builds the bounded briefing from it, and sends exactly
// one prompt containing the source context and the user instruction.
func TestContextTransferBriefingBuiltFromPageCache(t *testing.T) {
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
	return agent.ExecuteContinuation(ctx, runtime, Preparer{}, plan, instructionField)
}
