package agent

import (
	"context"
	"fmt"
	"strings"
	"time"
)

// CreatedAgentPhase names which post-commit phase failed (P0-04): the Agent
// already exists and callers must reconcile, never retry into a duplicate.
type CreatedAgentPhase string

const (
	PhaseNotReady               CreatedAgentPhase = "not-ready"
	PhaseStartUncertain         CreatedAgentPhase = "start-uncertain"
	PhasePlanSetup              CreatedAgentPhase = "plan-setup"
	PhaseInitialPrompt          CreatedAgentPhase = "initial-prompt"
	PhaseInitialPromptUncertain CreatedAgentPhase = "initial-prompt-uncertain"
	PhaseProjection             CreatedAgentPhase = "projection"
)

// LaunchFailure is the typed failure taxonomy of the transaction.
type LaunchFailure struct {
	Kind string // project-unavailable | branch-preparation | runtime-create | agent-start |
	// agent-start-uncertain | not-ready | plan-setup | initial-prompt |
	// initial-prompt-uncertain | agent-created | operation-conflict
	Detail string

	// Post-commit structure: set once agent.start may have committed, so
	// callers reconcile this exact target instead of re-launching.
	Committed bool
	TabID     string
	PaneID    string
	Phase     CreatedAgentPhase
	Uncertain bool
}

func (e *LaunchFailure) Error() string {
	return fmt.Sprintf("%s: %s", e.Kind, e.Detail)
}

func fail(kind, format string, args ...any) *LaunchFailure {
	return &LaunchFailure{Kind: kind, Detail: fmt.Sprintf(format, args...)}
}

// Runtime structures returned by the runtime adapter.
type TabCreated struct {
	TabID   string
	PaneID  string
	AgentID string
}

type WorkspaceCreated struct {
	WorkspaceID string
	Tab         TabCreated
}

type AgentStartParams struct {
	Name      string
	Kind      string
	PaneID    string
	Args      []string
	TimeoutMS int64
}

type StartedAgent struct {
	PaneID           string
	Agent            string
	AgentSession     string
	InteractiveReady bool
	Revision         int64
}

// LaunchRuntime is the Herdr transport seam of the transaction. Production
// binds it to verified Herdr protocol methods; tests bind fakes (the
// uncertain-delivery harness). The transaction never invents protocol
// methods beyond what the adapter implements.
type LaunchRuntime interface {
	CreateTabWithoutFocus(ctx context.Context, workspaceID, cwd string) (TabCreated, error)
	CreateWorkspaceWithoutFocus(ctx context.Context, cwd string) (WorkspaceCreated, error)
	WorkspaceIDForPath(ctx context.Context, projectPath string) (string, error)
	WaitShellReady(ctx context.Context, paneID string, timeoutMS int64) error
	StartAgent(ctx context.Context, params AgentStartParams) error
	ErrorIsUncertain(err error) bool
	WaitAgentIdle(ctx context.Context, paneID string, timeoutMS int64) error
	AgentByPane(ctx context.Context, paneID string) (*StartedAgent, error)
	SendAgentKeys(ctx context.Context, paneID string, keys []string) error
	PromptAgentOnce(ctx context.Context, paneID string, text string) error
	RenameTab(ctx context.Context, tabID, label string) error
	RenamePane(ctx context.Context, paneID, label string) error
	CloseTab(ctx context.Context, tabID string) error
}

// ProjectPreparer validates and prepares the target project/branch.
type ProjectPreparer interface {
	Prepare(ctx context.Context, projectPath, branch string) (cwd string, err error)
}

// Transaction timing knobs (Rust AGENT_START/SHELL_READY/READINESS/
// IDENTITY_VERIFY constants); tests shrink them for determinism.
type TransactionTimings struct {
	ShellReadyTimeoutMS     int64
	AgentStartTimeoutMS     int64
	ReadinessTimeoutMS      int64
	IdentityVerifyTimeoutMS int64
	IdentityPollMS          int64
	RetrySettleMS           int64
}

func DefaultTimings() TransactionTimings {
	return TransactionTimings{
		ShellReadyTimeoutMS:     10_000,
		AgentStartTimeoutMS:     30_000,
		ReadinessTimeoutMS:      60_000,
		IdentityVerifyTimeoutMS: 20_000,
		IdentityPollMS:          500,
		RetrySettleMS:           500,
	}
}

// LaunchOutcome is the authoritative result of one transaction.
type LaunchOutcome struct {
	PaneID       string
	TabID        string
	WorkspaceID  string
	TaskTitle    string
	HasTaskTitle bool
}

// RunLaunchTransaction executes the one canonical Agent launch transaction,
// porting the verified Rust phase order:
//
//	validate → prepare target → (integration ensure is fire-and-log upstream;
//	performed by the caller) → create runtime structure without global focus →
//	rename once → shell-ready gate → agent.start exactly once → uncertain-write
//	reconciliation → readiness/identity verification → plan keys → exactly one
//	semantic initial prompt → outcome.
func RunLaunchTransaction(ctx context.Context, runtime LaunchRuntime, preparer ProjectPreparer, intent LaunchIntent, timings TransactionTimings) (LaunchOutcome, *LaunchFailure) {
	if strings.TrimSpace(intent.OperationID) == "" {
		return LaunchOutcome{}, fail("operation-conflict", "operation id is required")
	}
	if strings.TrimSpace(intent.Prompt) == "" && !intent.SkipInitialPrompt {
		return LaunchOutcome{}, fail("project-unavailable", "agent task prompt is empty")
	}
	if strings.TrimSpace(intent.ProjectPath) == "" {
		return LaunchOutcome{}, fail("project-unavailable", "project path is empty")
	}

	cwd, prepErr := preparer.Prepare(ctx, intent.ProjectPath, intent.Branch)
	if prepErr != nil {
		return LaunchOutcome{}, fail("branch-preparation", "%s", prepErr.Error())
	}

	// Resolve the runtime structure without touching global focus: the
	// preferred workspace wins, else match by project path, else create.
	var created TabCreated
	workspaceID := intent.WorkspaceID
	switch {
	case workspaceID != "":
		tab, err := runtime.CreateTabWithoutFocus(ctx, workspaceID, cwd)
		if err != nil {
			return LaunchOutcome{}, fail("runtime-create", "%s", err.Error())
		}
		created = tab
	default:
		matched, err := runtime.WorkspaceIDForPath(ctx, intent.ProjectPath)
		if err != nil {
			return LaunchOutcome{}, fail("runtime-create", "%s", err.Error())
		}
		if matched != "" {
			tab, err := runtime.CreateTabWithoutFocus(ctx, matched, cwd)
			if err != nil {
				return LaunchOutcome{}, fail("runtime-create", "%s", err.Error())
			}
			created, workspaceID = tab, matched
		} else {
			workspace, err := runtime.CreateWorkspaceWithoutFocus(ctx, cwd)
			if err != nil {
				return LaunchOutcome{}, fail("runtime-create", "%s", err.Error())
			}
			created, workspaceID = workspace.Tab, workspace.WorkspaceID
		}
	}
	tabID, paneID := created.TabID, created.PaneID

	// One deterministic rename; failures never block launch.
	label, taskTitle, hasTaskTitle := LaunchDisplayLabel(intent)
	if err := runtime.RenameTab(ctx, tabID, label); err != nil {
		_ = err // fire-and-log upstream
	}
	if err := runtime.RenamePane(ctx, paneID, label); err != nil {
		_ = err
	}

	// agent.start is a mutation and executes exactly once: an uncertain
	// response is reconciled through the typed readiness verification below,
	// never repeated.
	launchArgs := append([]string(nil), intent.ExtraArgs...)
	launchArgs = append(launchArgs, AgentStartupArgs(intent.Agent, intent.Mode, intent.Permission)...)
	operationName := sanitizeOperationName(intent.OperationID)
	params := AgentStartParams{
		Name:      HerdrAgentKind(intent.Agent) + "-" + operationName,
		Kind:      HerdrAgentKind(intent.Agent),
		PaneID:    paneID,
		Args:      launchArgs,
		TimeoutMS: timings.AgentStartTimeoutMS,
	}
	startErr := shellGatedStart(ctx, runtime, paneID, params, timings)
	startUncertain := startErr != nil && runtime.ErrorIsUncertain(startErr)
	if startErr != nil && !startUncertain {
		// Definitive failure: the Agent never started; the created tab is
		// unused and is cleaned up exactly once.
		_ = runtime.CloseTab(ctx, tabID)
		return LaunchOutcome{}, fail("agent-start", "%s", startErr.Error())
	}
	if startUncertain {
		// Never repeat the mutation: the readiness verification below is the
		// reconciliation. On failure the tab is NOT closed — a valid first
		// Agent may already live there.
	}

	createdFailure := func(phase CreatedAgentPhase, detail string) *LaunchFailure {
		return &LaunchFailure{Kind: "agent-created", Detail: detail, Committed: true, TabID: tabID, PaneID: paneID, Phase: phase}
	}
	readinessFailure := func(message string) *LaunchFailure {
		if startUncertain {
			return &LaunchFailure{Kind: "agent-start-uncertain", Detail: message, TabID: tabID, PaneID: paneID, Uncertain: true}
		}
		return createdFailure(PhaseNotReady, message)
	}

	if err := runtime.WaitAgentIdle(ctx, paneID, timings.ReadinessTimeoutMS); err != nil {
		return LaunchOutcome{}, readinessFailure(err.Error())
	}

	// Typed identity verification inside a bounded window: typed-session
	// registration lags the readiness signal, and a single read raced cold
	// CLI boots into false failures upstream.
	deadline := time.Now().Add(time.Duration(timings.IdentityVerifyTimeoutMS) * time.Millisecond)
	for {
		started, err := runtime.AgentByPane(ctx, paneID)
		if err != nil {
			return LaunchOutcome{}, readinessFailure(err.Error())
		}
		if started == nil {
			if !time.Now().Before(deadline) {
				return LaunchOutcome{}, readinessFailure("started agent is not present in the runtime projection")
			}
			sleep(ctx, timings.IdentityPollMS)
			continue
		}
		// The intended provider must own this pane, through its typed session
		// kind or (providers without a typed session) the Herdr agent field.
		// The production adapter pins the exact kind mapping against the live
		// protocol schema.
		kind := herdrKindOfSession(started.AgentSession)
		if kind == "" {
			kind = started.Agent
		}
		sessionMatches := kind == HerdrAgentKind(intent.Agent)
		if sessionMatches && started.InteractiveReady {
			break
		}
		if !time.Now().Before(deadline) {
			return LaunchOutcome{}, readinessFailure(fmt.Sprintf(
				"agent identity/readiness verification failed (interactive_ready=%t)", started.InteractiveReady))
		}
		sleep(ctx, timings.IdentityPollMS)
	}

	// Verified post-ready Plan keys: failing to enter the requested mode is a
	// typed post-commit failure surfaced before the prompt (AC-18); the Agent
	// keeps running in its default mode.
	if intent.Mode == LaunchModePlan {
		if keys := PostLaunchPlanKeys(intent.Agent); len(keys) > 0 {
			if err := runtime.SendAgentKeys(ctx, paneID, keys); err != nil {
				return LaunchOutcome{}, createdFailure(PhasePlanSetup, err.Error())
			}
		}
	}

	// Exactly one semantic initial prompt; a failed prompt NEVER reads as
	// "not started" — uncertainty is typed on the runtime error.
	if !intent.SkipInitialPrompt {
		prompt := BuildInitialPrompt(intent, cwd)
		if err := runtime.PromptAgentOnce(ctx, paneID, prompt); err != nil {
			if runtime.ErrorIsUncertain(err) {
				return LaunchOutcome{}, createdFailure(PhaseInitialPromptUncertain, err.Error())
			}
			return LaunchOutcome{}, createdFailure(PhaseInitialPrompt, err.Error())
		}
	}

	return LaunchOutcome{
		PaneID: paneID, TabID: tabID, WorkspaceID: workspaceID,
		TaskTitle: taskTitle, HasTaskTitle: hasTaskTitle,
	}, nil
}

// shellGatedStart gates agent.start behind the interactive-shell readiness
// check, retried once after re-settling (the gate is read-only; the start
// mutation itself still executes at most once).
func shellGatedStart(ctx context.Context, runtime LaunchRuntime, paneID string, params AgentStartParams, timings TransactionTimings) error {
	gateErr := runtime.WaitShellReady(ctx, paneID, timings.ShellReadyTimeoutMS)
	if gateErr != nil {
		sleep(ctx, timings.RetrySettleMS)
		if retryErr := runtime.WaitShellReady(ctx, paneID, timings.ShellReadyTimeoutMS); retryErr != nil {
			return gateErr
		}
	}
	return runtime.StartAgent(ctx, params)
}

func sanitizeOperationName(operationID string) string {
	var builder strings.Builder
	for _, character := range operationID {
		if character >= 'a' && character <= 'z' || character >= 'A' && character <= 'Z' ||
			character >= '0' && character <= '9' || character == '-' {
			builder.WriteRune(character)
		}
		if builder.Len() >= 32 {
			break
		}
	}
	if builder.Len() == 0 {
		return "operation"
	}
	return builder.String()
}

func herdrKindOfSession(session string) string {
	if kind, _, ok := strings.Cut(session, ":"); ok {
		return kind
	}
	return session
}

func sleep(ctx context.Context, ms int64) {
	if ms <= 0 {
		return
	}
	timer := time.NewTimer(time.Duration(ms) * time.Millisecond)
	defer timer.Stop()
	select {
	case <-timer.C:
	case <-ctx.Done():
	}
}
