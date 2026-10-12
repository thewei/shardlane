package nativeui

/**
 * [INPUT]: 依赖真实 Codex rollout 夹具、官方 MyGo headless Tester 与 continuationRuntime 假体（引擎从不真正启动 Agent）
 * [OUTPUT]: 验证 Continue：先确认再动、同 Agent 续接/跨 Agent 交接的规划与简报来源、已在运行只定位窗格、无项目目录的说明、失败分类与已创建窗格的定位、菜单入口与禁用态
 * [POS]: Sessions 通往 Runtime 的唯一出口的回归；运行时变更只经 Herdr launch transaction，这里全部以假体观察
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"context"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/app"
	"github.com/wh-studio/herdr-client/internal/herdr"
	"github.com/wh-studio/herdr-client/internal/history"
)

// fakeContinuation records what Continue asks of Herdr. The engine never
// runs: the test decides what it would have created or how it fails.
type fakeContinuation struct {
	instance   string
	plans      []agent.ContinuationPlan
	window     history.TranscriptWindow
	outcome    agent.ContinuationOutcome
	err        error
	projection herdr.Projection
}

func (f *fakeContinuation) ExecuteHistoryContinuation(ctx context.Context, session string, plan agent.ContinuationPlan, _ string, deps app.HistoryContinuationDeps) (agent.ContinuationOutcome, error) {
	f.instance = session
	f.plans = append(f.plans, plan)
	if plan.Strategy == agent.StrategyContextTransfer {
		window, err := deps.OpenWindow(ctx, history.SessionFileRef{})
		if err != nil {
			return agent.ContinuationOutcome{}, err
		}
		f.window = window
	}
	return f.outcome, f.err
}

func (f *fakeContinuation) Projection(string) (herdr.Projection, error) { return f.projection, nil }

// newContinueTestShell is the fixture session over a two-project Herdr
// projection; "p2" is the pane a successful Continue would have created.
func newContinueTestShell(t *testing.T) (*Shell, *fakeContinuation, history.SessionMeta) {
	t.Helper()
	shell, catalog, _ := newHistoryTestShell(t)
	fake := &fakeContinuation{
		outcome:    agent.ContinuationOutcome{PaneID: "p2", TabID: "t2"},
		projection: selectionFixture("w1", "t1", "p1"),
	}
	shell.hist.continuation = fake
	shell.activeInstance = "main"
	shell.projection = selectionFixture("w1", "t1", "p1")
	shell.adoptRuntimeSelection(shell.projection)
	meta := catalog.Session(previewTestKey)
	if meta == nil {
		t.Fatalf("fixture session %s missing", previewTestKey)
	}
	return shell, fake, *meta
}

// TestContinueAsksBeforeResumingTheSameAgent pins the confirm contract: the
// plan is read-only, nothing reaches Herdr until Confirm, and Cancel leaves
// the runtime untouched.
func TestContinueAsksBeforeResumingTheSameAgent(t *testing.T) {
	shell, fake, meta := newContinueTestShell(t)
	shell.router.Replace(routeHistory)
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()

	shell.requestContinue(meta, nil)
	tester.Frame()
	for _, want := range []string{"Continue in Codex?", "Confirm", "Cancel"} {
		if !tester.HasText(want) {
			t.Fatalf("confirm dialog missing %q; texts=%q", want, tester.Texts())
		}
	}
	if len(fake.plans) != 0 {
		t.Fatalf("the engine ran before the user confirmed: %d plans", len(fake.plans))
	}

	if err := tester.Click("Cancel"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if len(fake.plans) != 0 {
		t.Fatalf("Cancel still reached the engine: %d plans", len(fake.plans))
	}

	shell.requestContinue(meta, nil)
	shell.submitConfirm("history-continue", meta.Key)
	if len(fake.plans) != 1 {
		t.Fatalf("a confirmed Continue ran the engine %d times, want 1", len(fake.plans))
	}
	plan := fake.plans[0]
	wantArgs := []string{"resume", meta.ID}
	if plan.Strategy != agent.StrategyNativeResume || plan.Target != history.AgentCodex || strings.Join(plan.ResumeArgs, " ") != strings.Join(wantArgs, " ") {
		t.Fatalf("plan = %s target=%s args=%v, want a native resume of the same Agent", plan.Strategy, plan.Target, plan.ResumeArgs)
	}
	if fake.instance != "main" {
		t.Fatalf("launched in %q, want the active Workspace", fake.instance)
	}
	if shell.selectedPaneID != "p2" || shell.router.Path() != routeWorkspace {
		t.Fatalf("success left pane=%q route=%q, want the new pane in the Workspace", shell.selectedPaneID, shell.router.Path())
	}
	if shell.hist.continuing || !strings.Contains(shell.pendingToast, "Continuing in Codex") {
		t.Fatalf("continuing=%v toast=%q", shell.hist.continuing, shell.pendingToast)
	}
}

// TestContinueInAnotherAgentHandsOverTheNewestMessages pins the transfer: the
// target comes from the user's own providers, and the briefing source is the
// original session read at its newest end.
func TestContinueInAnotherAgentHandsOverTheNewestMessages(t *testing.T) {
	shell, fake, meta := newContinueTestShell(t)
	shell.hist.providers = []history.AgentID{history.AgentCodex, history.AgentClaudeCode, history.AgentGemini}

	targets := shell.continueTargets(meta)
	if len(targets) != 1 || targets[0] != history.AgentClaudeCode {
		t.Fatalf("targets = %v, want only Claude Code (not the session's own Agent, not a hidden one)", targets)
	}

	shell.router.Replace(routeHistory)
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()
	target := history.AgentClaudeCode
	shell.requestContinue(meta, &target)
	tester.Frame()
	for _, want := range []string{"Continue in Claude Code?", "latest messages"} {
		found := false
		for _, text := range tester.Texts() {
			if strings.Contains(text, want) {
				found = true
			}
		}
		if !found {
			t.Fatalf("confirm dialog missing %q; texts=%q", want, tester.Texts())
		}
	}

	shell.submitConfirm("history-continue", meta.Key)
	if len(fake.plans) != 1 || fake.plans[0].Strategy != agent.StrategyContextTransfer || fake.plans[0].Target != history.AgentClaudeCode {
		t.Fatalf("plans = %+v, want one context transfer to Claude Code", fake.plans)
	}
	last := fake.window.Messages[len(fake.window.Messages)-1]
	if !strings.Contains(last.Text, "Implemented.") {
		t.Fatalf("the briefing window ends with %q, want the session's newest message", last.Text)
	}
}

// TestContinueOfARunningSessionOnlyShowsItsPane pins AlreadyLive: no dialog,
// no launch — the pane that already runs the session is brought up.
func TestContinueOfARunningSessionOnlyShowsItsPane(t *testing.T) {
	shell, fake, meta := newContinueTestShell(t)
	live := selectionFixture("w1", "t1", "p1")
	live.Agents = []herdr.Agent{{
		TerminalID: "term2", PaneID: "p2", ProjectID: "w2", TabID: "t2",
		AgentSession: &herdr.AgentSessionIdentity{Agent: "codex", Kind: "id", Value: meta.ID},
	}}
	shell.projection = live
	shell.router.Replace(routeHistory)

	shell.requestContinue(meta, nil)
	if shell.confirmOpen || len(fake.plans) != 0 {
		t.Fatalf("a running session asked for confirmation (open=%v) or launched (%d)", shell.confirmOpen, len(fake.plans))
	}
	if shell.selectedPaneID != "p2" || shell.router.Path() != routeWorkspace {
		t.Fatalf("pane=%q route=%q, want the running pane in the Workspace", shell.selectedPaneID, shell.router.Path())
	}
	if !strings.Contains(shell.pendingToast, "Already running") {
		t.Fatalf("toast = %q", shell.pendingToast)
	}
}

// TestContinueWithoutAProjectFolderExplains pins NeedsProjectSelection: the
// user hears why instead of seeing a dialog that cannot succeed.
func TestContinueWithoutAProjectFolderExplains(t *testing.T) {
	shell, fake, meta := newContinueTestShell(t)
	meta.ProjectPath = ""

	shell.requestContinue(meta, nil)
	if shell.confirmOpen || len(fake.plans) != 0 {
		t.Fatalf("confirm open=%v plans=%d, want neither", shell.confirmOpen, len(fake.plans))
	}
	if !strings.Contains(shell.pendingToast, "no project folder") {
		t.Fatalf("toast = %q", shell.pendingToast)
	}
}

// TestContinueFailuresNameTheClassAndShowACreatedPane pins the failure text
// and the rule that a created Agent is shown instead of retried.
func TestContinueFailuresNameTheClassAndShowACreatedPane(t *testing.T) {
	cases := []struct {
		name         string
		err          error
		wantToast    string
		wantSelected string
	}{
		{"never started", &agent.LaunchFailure{Kind: "agent-start", Detail: "exec: not found"}, "Codex didn't start", "p1"},
		{"folder gone", &agent.LaunchFailure{Kind: "branch-preparation", Detail: "no such directory"}, "project folder isn't available", "p1"},
		{"started, not ready", &agent.LaunchFailure{Kind: "agent-created", Phase: agent.PhaseNotReady, Committed: true, PaneID: "p2"}, "isn't ready yet", "p2"},
		{"start uncertain", &agent.LaunchFailure{Kind: "agent-start-uncertain", Uncertain: true, PaneID: "p2"}, "may have started", "p2"},
		{"plain error", context.DeadlineExceeded, "Couldn't continue in Codex", "p1"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			shell, fake, meta := newContinueTestShell(t)
			fake.err = tc.err
			shell.router.Replace(routeHistory)

			shell.requestContinue(meta, nil)
			shell.submitConfirm("history-continue", meta.Key)
			if shell.hist.continuing {
				t.Fatal("a failed Continue left the lock held")
			}
			if !strings.Contains(shell.pendingToast, tc.wantToast) {
				t.Fatalf("toast = %q, want it to contain %q", shell.pendingToast, tc.wantToast)
			}
			if strings.Contains(shell.pendingToast, "exec:") || strings.Contains(shell.pendingToast, "no such directory") {
				t.Fatalf("toast leaks engine detail: %q", shell.pendingToast)
			}
			if shell.selectedPaneID != tc.wantSelected {
				t.Fatalf("selected pane = %q, want %q", shell.selectedPaneID, tc.wantSelected)
			}
		})
	}
}

// TestSessionMenuOffersContinueOnlyWhenItCanRun pins the entry points: the
// row menu carries Continue and Continue in, enabled with a Workspace to
// start in and disabled without one.
func TestSessionMenuOffersContinueOnlyWhenItCanRun(t *testing.T) {
	shell, fake, _ := newContinueTestShell(t)
	shell.router.Replace(routeHistory)
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()
	// The page's list load derives the providers from the catalog (Codex
	// only); the user's other Agents are what Continue in offers.
	shell.hist.providers = []history.AgentID{history.AgentCodex, history.AgentClaudeCode}

	if err := tester.RightClick("Build history"); err != nil {
		t.Fatal(err)
	}
	labels := strings.Join(tester.Menu(), "|")
	if !strings.Contains(labels, "Continue") || !strings.Contains(labels, "Continue in") {
		t.Fatalf("row menu = %q, want Continue and Continue in", labels)
	}
	if err := tester.ChooseMenuItem("Continue in", "Claude Code"); err != nil {
		t.Fatal(err)
	}
	if !shell.confirmOpen || shell.hist.pendingContinue == nil || shell.hist.pendingContinue.plan.Target != history.AgentClaudeCode {
		t.Fatalf("choosing Continue in > Claude Code did not stage a confirm (open=%v)", shell.confirmOpen)
	}
	if len(fake.plans) != 0 {
		t.Fatalf("the menu reached the engine: %d plans", len(fake.plans))
	}
	shell.confirmOpen = false

	shell.activeInstance = ""
	tester.Frame()
	if err := tester.RightClick("Build history"); err != nil {
		t.Fatal(err)
	}
	if err := tester.ChooseMenuItem("Continue"); err == nil || !strings.Contains(err.Error(), "disabled") {
		t.Fatalf("Continue without a Workspace to start in: err=%v, want the item disabled", err)
	}
}

// TestPreviewContinueButtonStagesTheConfirm pins the second entry point: the
// row's unfolded preview carries Continue, and it asks before acting.
func TestPreviewContinueButtonStagesTheConfirm(t *testing.T) {
	shell, fake, _ := newContinueTestShell(t)
	shell.router.Replace(routeHistory)
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()
	if err := tester.Click("Build history"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if err := tester.Click("Continue"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !tester.HasText("Continue in Codex?") || len(fake.plans) != 0 {
		t.Fatalf("Continue button: dialog=%v plans=%d; texts=%q", tester.HasText("Continue in Codex?"), len(fake.plans), tester.Texts())
	}
}
