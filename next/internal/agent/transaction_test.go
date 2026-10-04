package agent

import (
	"context"
	"errors"
	"strings"
	"sync"
	"testing"

	"github.com/wh-studio/herdr-client/next/internal/history"
)

// fakeLaunchRuntime is the uncertain-delivery harness (AGENT-06A): every
// seam is scriptable, start delivery can be made uncertain, and the
// tab-reap/rename observables are recorded for assertions.
type fakeLaunchRuntime struct {
	mu sync.Mutex

	workspaceID   string
	createdTabs   int
	closedTabs    []string
	renamed       []string
	started       int
	prompts       []string
	planKeys      [][]string
	shellNotReady int

	// startErr is the error StartAgent returns.
	startErr error
	// agentKind is the Herdr kind the fake reports for the started agent.
	agentKind string
	// startAgentAppears: whether the pane ends up holding the intended agent
	// (the uncertain-start reconciliation proof).
	startAgentAppears bool
	identityReady     bool
}

func (f *fakeLaunchRuntime) CreateTabWithoutFocus(ctx context.Context, workspaceID, cwd string) (TabCreated, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.createdTabs++
	return TabCreated{TabID: "tab-1", PaneID: "pane-1"}, nil
}

func (f *fakeLaunchRuntime) CreateWorkspaceWithoutFocus(ctx context.Context, cwd string) (WorkspaceCreated, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.createdTabs++
	id := f.workspaceID
	if id == "" {
		id = "w-new"
	}
	return WorkspaceCreated{WorkspaceID: id, Tab: TabCreated{TabID: "tab-1", PaneID: "pane-1"}}, nil
}

func (f *fakeLaunchRuntime) WorkspaceIDForPath(ctx context.Context, projectPath string) (string, error) {
	return f.workspaceID, nil
}

func (f *fakeLaunchRuntime) WaitShellReady(ctx context.Context, paneID string, timeoutMS int64) error {
	if f.shellNotReady > 0 {
		f.shellNotReady--
		return errors.New("shell not ready")
	}
	return nil
}

func (f *fakeLaunchRuntime) StartAgent(ctx context.Context, params AgentStartParams) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.started++
	return f.startErr
}

func (f *fakeLaunchRuntime) ErrorIsUncertain(err error) bool {
	var uncertain *uncertainDeliveryError
	return errors.As(err, &uncertain)
}

type uncertainDeliveryError struct{ detail string }

func (e *uncertainDeliveryError) Error() string { return "delivery uncertain: " + e.detail }

func (f *fakeLaunchRuntime) WaitAgentIdle(ctx context.Context, paneID string, timeoutMS int64) error {
	return nil
}

func (f *fakeLaunchRuntime) AgentByPane(ctx context.Context, paneID string) (*StartedAgent, error) {
	if !f.startAgentAppears {
		return nil, nil
	}
	kind := f.agentKind
	if kind == "" {
		kind = HerdrAgentKind(history.AgentClaudeCode)
	}
	return &StartedAgent{
		PaneID: paneID, Agent: kind,
		AgentSession:     kind + ":session-1",
		InteractiveReady: f.identityReady,
	}, nil
}

func (f *fakeLaunchRuntime) SendAgentKeys(ctx context.Context, paneID string, keys []string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.planKeys = append(f.planKeys, keys)
	return nil
}

func (f *fakeLaunchRuntime) PromptAgentOnce(ctx context.Context, paneID string, text string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.prompts = append(f.prompts, text)
	return nil
}

func (f *fakeLaunchRuntime) RenameTab(ctx context.Context, tabID, label string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.renamed = append(f.renamed, "tab:"+label)
	return nil
}

func (f *fakeLaunchRuntime) RenamePane(ctx context.Context, paneID, label string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.renamed = append(f.renamed, "pane:"+label)
	return nil
}

func (f *fakeLaunchRuntime) CloseTab(ctx context.Context, tabID string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.closedTabs = append(f.closedTabs, tabID)
	return nil
}

func launchTestIntent() LaunchIntent {
	return LaunchIntent{
		OperationID: "op-123",
		ProjectPath: "/work/demo",
		Mode:        LaunchModeBuild,
		Permission:  PermissionAskApproval,
		Agent:       history.AgentClaudeCode,
		Prompt:      "Fix the flaky test\n\nsteps follow",
	}
}

func TestTransactionHappyPath(t *testing.T) {
	runtime := &fakeLaunchRuntime{startAgentAppears: true, identityReady: true}
	outcome, failure := RunLaunchTransaction(context.Background(), runtime, readyPreparer{}, launchTestIntent(), DefaultTimings())
	if failure != nil {
		t.Fatalf("failure = %+v", failure)
	}
	if outcome.PaneID != "pane-1" || outcome.TabID != "tab-1" {
		t.Fatalf("outcome = %+v", outcome)
	}
	if runtime.started != 1 {
		t.Fatalf("agent.start ran %d times", runtime.started)
	}
	if len(runtime.prompts) != 1 || !strings.Contains(runtime.prompts[0], "Fix the flaky test") {
		t.Fatalf("prompts = %q", runtime.prompts)
	}
	if len(runtime.closedTabs) != 0 {
		t.Fatalf("happy path closed tabs: %v", runtime.closedTabs)
	}
}

func TestTransactionValidatesIntent(t *testing.T) {
	runtime := &fakeLaunchRuntime{}
	empty := launchTestIntent()
	empty.Prompt = "   "
	if _, failure := RunLaunchTransaction(context.Background(), runtime, readyPreparer{}, empty, DefaultTimings()); failure == nil || failure.Kind != "project-unavailable" {
		t.Fatalf("empty prompt failure = %+v", failure)
	}
	noID := launchTestIntent()
	noID.OperationID = ""
	if _, failure := RunLaunchTransaction(context.Background(), runtime, readyPreparer{}, noID, DefaultTimings()); failure == nil || failure.Kind != "operation-conflict" {
		t.Fatalf("missing operation id failure = %+v", failure)
	}
	if runtime.createdTabs != 0 {
		t.Fatal("validation failures must not create runtime structure")
	}
}

func TestTransactionDefinitiveStartFailureReapsTab(t *testing.T) {
	runtime := &fakeLaunchRuntime{startErr: errors.New("provider cli missing")}
	_, failure := RunLaunchTransaction(context.Background(), runtime, readyPreparer{}, launchTestIntent(), DefaultTimings())
	if failure == nil || failure.Kind != "agent-start" || failure.Committed {
		t.Fatalf("failure = %+v", failure)
	}
	if runtime.started != 1 {
		t.Fatalf("agent.start ran %d times", runtime.started)
	}
	if len(runtime.closedTabs) != 1 || runtime.closedTabs[0] != "tab-1" {
		t.Fatalf("closed tabs = %v", runtime.closedTabs)
	}
	if len(runtime.prompts) != 0 {
		t.Fatal("no prompt may be sent after a definitive start failure")
	}
}

// TestTransactionUncertainStartReconciles pins AC-01: an uncertain
// agent.start is never repeated; the typed readiness verification proves the
// intended provider owns the pane and the transaction succeeds.
func TestTransactionUncertainStartReconciles(t *testing.T) {
	runtime := &fakeLaunchRuntime{
		startErr:          &uncertainDeliveryError{detail: "response not read"},
		startAgentAppears: true, identityReady: true,
	}
	outcome, failure := RunLaunchTransaction(context.Background(), runtime, readyPreparer{}, launchTestIntent(), DefaultTimings())
	if failure != nil {
		t.Fatalf("uncertain start that actually succeeded must reconcile: %+v", failure)
	}
	if outcome.PaneID != "pane-1" {
		t.Fatalf("reconciled outcome = %+v", outcome)
	}
	if runtime.started != 1 {
		t.Fatalf("agent.start ran %d times, want exactly 1", runtime.started)
	}
	if len(runtime.closedTabs) != 0 {
		t.Fatalf("uncertain start must not reap the tab: %v", runtime.closedTabs)
	}
	if len(runtime.prompts) != 1 {
		t.Fatalf("prompts = %d", len(runtime.prompts))
	}
}

// TestTransactionUncertainStartUnproven pins the failure side: when the
// readiness verification cannot prove the intended provider owns the pane,
// the failure is typed uncertain and the tab is NOT closed.
func TestTransactionUncertainStartUnproven(t *testing.T) {
	runtime := &fakeLaunchRuntime{
		startErr:          &uncertainDeliveryError{detail: "response not read"},
		startAgentAppears: false,
	}
	timings := DefaultTimings()
	timings.IdentityVerifyTimeoutMS = 20
	timings.IdentityPollMS = 1
	_, failure := RunLaunchTransaction(context.Background(), runtime, readyPreparer{}, launchTestIntent(), timings)
	if failure == nil || failure.Kind != "agent-start-uncertain" || !failure.Uncertain {
		t.Fatalf("failure = %+v", failure)
	}
	if len(runtime.closedTabs) != 0 {
		t.Fatalf("uncertain failure must not reap the tab: %v", runtime.closedTabs)
	}
}

// TestTransactionPostCommitPromptFailure pins P0-04: a prompt failure after
// the start commit reports the created structure (never "not started") and
// distinguishes uncertain delivery.
func TestTransactionPostCommitPromptFailure(t *testing.T) {
	runtime := &fakeLaunchRuntime{startAgentAppears: true, identityReady: true}
	// Wrap PromptAgentOnce by failing it through a failing prompt runtime.
	failing := &failingPromptRuntime{fakeLaunchRuntime: runtime, promptErr: errors.New("prompt write failed")}
	_, failure := RunLaunchTransaction(context.Background(), failing, readyPreparer{}, launchTestIntent(), DefaultTimings())
	if failure == nil || failure.Kind != "agent-created" || failure.Phase != PhaseInitialPrompt || !failure.Committed {
		t.Fatalf("failure = %+v", failure)
	}
	if failure.TabID != "tab-1" || failure.PaneID != "pane-1" {
		t.Fatalf("post-commit structure missing: %+v", failure)
	}

	uncertain := &failingPromptRuntime{fakeLaunchRuntime: runtime, promptErr: &uncertainDeliveryError{detail: "timeout"}}
	_, failure = RunLaunchTransaction(context.Background(), uncertain, readyPreparer{}, launchTestIntent(), DefaultTimings())
	if failure == nil || failure.Phase != PhaseInitialPromptUncertain {
		t.Fatalf("uncertain prompt failure = %+v", failure)
	}
}

// TestTransactionPlanKeysFailure pins AC-18: a Plan-keys failure after the
// commit is a typed post-commit failure, before any prompt.
func TestTransactionPlanKeysFailure(t *testing.T) {
	base := &fakeLaunchRuntime{startAgentAppears: true, identityReady: true, agentKind: HerdrAgentKind(history.AgentCodex)}
	failing := &failingKeysRuntime{fakeLaunchRuntime: base, keysErr: errors.New("keys rejected")}
	intent := launchTestIntent()
	intent.Mode = LaunchModePlan
	intent.Agent = history.AgentCodex
	_, failure := RunLaunchTransaction(context.Background(), failing, readyPreparer{}, intent, DefaultTimings())
	if failure == nil || failure.Kind != "agent-created" || failure.Phase != PhasePlanSetup {
		t.Fatalf("failure = %+v", failure)
	}
	if len(failing.prompts) != 0 {
		t.Fatal("plan-setup failure must precede the initial prompt")
	}
}

func TestTransactionStartupArgsAndPlanKeysParity(t *testing.T) {
	if got := AgentStartupArgs(history.AgentClaudeCode, LaunchModePlan, PermissionAskApproval); !equalArgs(got, []string{"--permission-mode", "plan"}) {
		t.Fatalf("claude plan args = %v", got)
	}
	if got := AgentStartupArgs(history.AgentCodex, LaunchModeBuild, PermissionAutoApprove); !equalArgs(got, []string{"--sandbox", "workspace-write", "--ask-for-approval", "never"}) {
		t.Fatalf("codex auto-approve args = %v", got)
	}
	if got := AgentStartupArgs(history.AgentGemini, LaunchModeBuild, PermissionFullAccess); !equalArgs(got, []string{"--approval-mode=auto-edit"}) {
		t.Fatalf("gemini args = %v", got)
	}
	if got := PostLaunchPlanKeys(history.AgentCodex); !equalArgs(got, []string{"shift+tab"}) {
		t.Fatalf("codex plan keys = %v", got)
	}
	if got := PostLaunchPlanKeys(history.AgentClaudeCode); got != nil {
		t.Fatalf("claude plan keys = %v", got)
	}

	intent := launchTestIntent()
	intent.Mode = LaunchModePlan
	intent.Agent = history.AgentGrok // prompt-prefix provider
	if prompt := BuildInitialPrompt(intent, "/work/demo"); !strings.HasPrefix(prompt, "Work in Plan mode first.") {
		t.Fatalf("prompt-prefix plan prompt = %q", prompt)
	}
	intent.Agent = history.AgentClaudeCode
	if prompt := BuildInitialPrompt(intent, "/work/demo"); strings.HasPrefix(prompt, "Work in Plan mode first.") {
		t.Fatal("native-cli plan provider must not get the plan prefix")
	}

	// The fingerprint excludes the operation id: a reused id with a different
	// shape is a conflict, never a replay.
	first := LaunchFingerprint(launchTestIntent())
	changed := launchTestIntent()
	changed.Prompt = "other"
	if LaunchFingerprint(changed) == first {
		t.Fatal("different intents must fingerprint differently")
	}
}

func equalArgs(got, want []string) bool {
	if len(got) != len(want) {
		return false
	}
	for i := range got {
		if got[i] != want[i] {
			return false
		}
	}
	return true
}

type readyPreparer struct{}

func (readyPreparer) Prepare(ctx context.Context, projectPath, branch string) (string, error) {
	return projectPath, nil
}

type failingPromptRuntime struct {
	*fakeLaunchRuntime
	promptErr error
}

func (f *failingPromptRuntime) PromptAgentOnce(ctx context.Context, paneID string, text string) error {
	return f.promptErr
}

type failingKeysRuntime struct {
	*fakeLaunchRuntime
	keysErr error
}

func (f *failingKeysRuntime) SendAgentKeys(ctx context.Context, paneID string, keys []string) error {
	return f.keysErr
}
