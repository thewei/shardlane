package nativeui

/**
 * [INPUT]: 依赖 agent.PlanHistoryContinuation 的只读规划、app.LaunchService.ExecuteHistoryContinuation（既有 Herdr launch transaction）、HistoryService.Open（ContextTransfer 简报的原文尾部窗口）与共享确认框
 * [OUTPUT]: 提供 Sessions 的 Continue / Continue in <Agent>：规划只读；会话已在运行则直接定位其窗格，否则经确认后由 Herdr 新建 Tab 续接，成功后选中新窗格
 * [POS]: History 通往 Runtime 的唯一出口——不自建进程/终端；会话文件始终只读，所有运行时变更走 Herdr launch transaction
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"context"
	"errors"
	"log/slog"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/app"
	"github.com/wh-studio/herdr-client/internal/herdr"
	"github.com/wh-studio/herdr-client/internal/history"
)

// continuationRuntime is the Herdr side of Continue: the continuation
// engine, and the projection that shows what it created. The narrow seam
// keeps the presentation testable without starting agents.
type continuationRuntime interface {
	ExecuteHistoryContinuation(ctx context.Context, session string, plan agent.ContinuationPlan, instruction string, deps app.HistoryContinuationDeps) (agent.ContinuationOutcome, error)
	Projection(session string) (herdr.Projection, error)
}

// herdrContinuation binds the production services to that seam.
type herdrContinuation struct {
	launch  *app.LaunchService
	runtime *herdr.Manager
}

func (h herdrContinuation) ExecuteHistoryContinuation(ctx context.Context, session string, plan agent.ContinuationPlan, instruction string, deps app.HistoryContinuationDeps) (agent.ContinuationOutcome, error) {
	return h.launch.ExecuteHistoryContinuation(ctx, session, plan, instruction, deps)
}

func (h herdrContinuation) Projection(session string) (herdr.Projection, error) {
	return h.runtime.Projection(session)
}

// continueRequest is one planned Continue waiting for the user's
// confirmation — the only point where History reaches the runtime.
type continueRequest struct {
	meta history.SessionMeta
	plan agent.ContinuationPlan
}

// continuation resolves what Continue acts on: the injected runtime (tests),
// else the production launch service over the active Herdr.
func (s *Shell) continuation() continuationRuntime {
	if s.hist.continuation != nil {
		return s.hist.continuation
	}
	if s.launch == nil || s.runtime == nil {
		return nil
	}
	return herdrContinuation{launch: s.launch, runtime: s.runtime}
}

// canContinue: Continue needs a Herdr Workspace to start in and no other
// Continue still running.
func (s *Shell) canContinue() bool {
	return s.continuation() != nil && s.activeInstance != "" && s.hist.service != nil && !s.hist.continuing
}

// continueTargets are the other Agents a session can move to: the providers
// the user already has history with (so they exist on this machine), minus
// the session's own and those the product does not start.
func (s *Shell) continueTargets(meta history.SessionMeta) []history.AgentID {
	var targets []history.AgentID
	for _, provider := range s.hist.providers {
		if provider == meta.Agent {
			continue
		}
		capabilities := agent.ProjectCapabilities(provider, agent.Environment{Enabled: true, Installed: true})
		if capabilities.UnavailableReason == "" {
			targets = append(targets, provider)
		}
	}
	return targets
}

// continueMenuItems adds Continue (same Agent) and Continue in (another
// Agent) to a session menu.
func (s *Shell) continueMenuItems(m *ui.Menu, meta history.SessionMeta) {
	can := s.canContinue()
	if m.Item("Continue").Disabled(!can).Chosen() {
		s.requestContinue(meta, nil)
	}
	if targets := s.continueTargets(meta); len(targets) > 0 {
		m.Submenu("Continue in", func(sub *ui.Menu) {
			for _, target := range targets {
				if sub.Item(target.DisplayName()).Disabled(!can).Chosen() {
					s.requestContinue(meta, &target)
				}
			}
		})
	}
}

// requestContinue plans one continuation — read-only — and acts on the
// plan's strategy: a session that is already running just shows its pane;
// one that needs a new Agent process asks first.
func (s *Shell) requestContinue(meta history.SessionMeta, target *history.AgentID) {
	if !s.canContinue() {
		return
	}
	source := agent.ContinuationSource{
		Agent:           meta.Agent,
		ID:              meta.ID,
		FilePath:        meta.FilePath,
		ProjectPath:     meta.ProjectPath,
		ResumeSupported: agent.ResumeSupported(meta.Agent),
	}
	plan := agent.PlanHistoryContinuation(source, target, s.liveAgentIdentities(), "")
	switch plan.Strategy {
	case agent.StrategyAlreadyLive:
		s.showPane(plan.PaneID)
		s.pendingToast = "Already running — showing its pane."
	case agent.StrategyNativeResume, agent.StrategyContextTransfer:
		s.hist.pendingContinue = &continueRequest{meta: meta, plan: plan}
		title, message := continueConfirmText(meta, plan)
		s.openConfirm("history-continue", meta.Key, title, message)
	case agent.StrategyNeedsProjectSelect:
		s.pendingToast = "This session has no project folder, so it can't be continued here."
	default:
		slog.Info("history continue unavailable", "agent", meta.Agent, "reason", plan.Reason)
		s.pendingToast = "This session can't be continued from here."
	}
}

// liveAgentIdentities lists the running Agents' typed session identities,
// the exact keys the planner matches a session against.
func (s *Shell) liveAgentIdentities() []agent.LiveAgentIdentity {
	var live []agent.LiveAgentIdentity
	for _, runtimeAgent := range s.projection.Agents {
		session := runtimeAgent.AgentSession
		if session == nil || runtimeAgent.PaneID == "" {
			continue
		}
		provider, ok := history.ParseAgentID(session.Agent)
		if !ok {
			continue
		}
		value := session.Value
		switch session.Kind {
		case "id":
			value = "id:" + value
		case "path":
			value = "path:" + value
		}
		live = append(live, agent.LiveAgentIdentity{
			PaneID:   runtimeAgent.PaneID,
			Kind:     string(provider),
			Value:    value,
			Revision: int64(runtimeAgent.Revision),
		})
	}
	return live
}

// continueConfirmText says what the confirmed Continue will do: the new
// tab, the Agent, and — when another Agent takes over — that it is handed
// the session's latest messages.
func continueConfirmText(meta history.SessionMeta, plan agent.ContinuationPlan) (title, message string) {
	target := plan.Target.DisplayName()
	project := fallbackText(meta.ProjectName, shortDir(meta.ProjectPath))
	session := continueSessionName(meta)
	title = "Continue in " + target + "?"
	if plan.Strategy == agent.StrategyContextTransfer {
		return title, "Opens a new tab in “" + project + "” with " + target +
			" and gives it the latest messages of “" + session + "” as context. The session file isn't changed."
	}
	return title, "Opens a new tab in “" + project + "” and resumes “" + session + "”. The session file isn't changed."
}

// continueSessionName is the session title, bounded for a dialog line.
func continueSessionName(meta history.SessionMeta) string {
	title := fallbackText(meta.Title, history.Untitled)
	runes := []rune(title)
	if len(runes) <= 48 {
		return title
	}
	return string(runes[:48]) + "…"
}

// runContinue is the confirmed Continue: the engine runs off the UI lane
// (a launch waits on the new Agent for up to a couple of minutes) and the
// result lands through applyOnWindow.
func (s *Shell) runContinue(key string) {
	request := s.hist.pendingContinue
	s.hist.pendingContinue = nil
	runner := s.continuation()
	if request == nil || request.meta.Key != key || runner == nil || s.activeInstance == "" || s.hist.service == nil || s.hist.continuing {
		return
	}
	s.hist.continuing = true
	instance := s.activeInstance
	service := s.hist.service
	plan := request.plan
	target := plan.Target
	s.pendingToast = "Starting " + target.DisplayName() + "…"
	slog.Info("history continue requested", "instance", instance, "agent", request.meta.Agent, "target", target, "strategy", plan.Strategy)
	// A transfer's briefing is read from the original session file, the
	// newest window of it, at launch time.
	deps := app.HistoryContinuationDeps{
		OpenWindow: func(ctx context.Context, _ history.SessionFileRef) (history.TranscriptWindow, error) {
			return service.Open(ctx, history.ConversationWindowRequest{ConversationID: key, Tail: true})
		},
	}
	s.dispatch(func() {
		outcome, err := runner.ExecuteHistoryContinuation(context.Background(), instance, plan, "", deps)
		pane := continuedPane(outcome, err)
		var projection *herdr.Projection
		if pane != "" {
			if fresh, projectionErr := runner.Projection(instance); projectionErr == nil {
				projection = &fresh
			}
		}
		s.applyOnWindow(func() {
			s.hist.continuing = false
			if err != nil {
				slog.Warn("history continue failed", "instance", instance, "target", target, "error", err)
				s.pendingToast = continueFailureText(target, err)
			} else {
				slog.Info("history continue started", "instance", instance, "target", target, "pane_id", pane)
				s.pendingToast = "Continuing in " + target.DisplayName()
			}
			if pane == "" || s.activeInstance != instance {
				return
			}
			if projection != nil {
				s.applyProjection(*projection, false)
			}
			s.showPane(pane)
		})
	})
}

// continuedPane is the pane the engine created: the outcome's on success,
// the committed or possibly-started one on a late failure (the Agent may
// exist; the user must see it rather than retry into a duplicate).
func continuedPane(outcome agent.ContinuationOutcome, err error) string {
	if err == nil {
		return outcome.PaneID
	}
	var failure *agent.LaunchFailure
	if errors.As(err, &failure) && (failure.Committed || failure.Uncertain) {
		return failure.PaneID
	}
	return ""
}

// continueFailureText is the one user-facing line per failure class; the
// engine's own detail goes to the log, not the toast.
func continueFailureText(target history.AgentID, err error) string {
	name := target.DisplayName()
	var failure *agent.LaunchFailure
	if !errors.As(err, &failure) {
		return "Couldn't continue in " + name + "."
	}
	switch failure.Kind {
	case "project-unavailable", "branch-preparation":
		// Continue never asks for a branch, so a failed preparation is the
		// project folder itself.
		return "The project folder isn't available."
	case "runtime-create":
		return "Couldn't open a new tab."
	case "agent-start":
		return name + " didn't start. Check that it's installed."
	case "agent-start-uncertain":
		return name + " may have started — check its tab."
	case "agent-created":
		switch failure.Phase {
		case agent.PhaseNotReady:
			return name + " started but isn't ready yet."
		case agent.PhaseInitialPromptUncertain:
			return name + " may not have received the context — check its tab."
		default:
			return name + " started, but the context couldn't be delivered."
		}
	case "operation-conflict":
		return "This session is already being continued."
	default:
		return "Couldn't continue in " + name + "."
	}
}

// showPane brings one pane into view: select it, and show the Terminal.
func (s *Shell) showPane(paneID string) {
	s.selectPane(paneID)
	s.showViewTerminal()
}
