package agent

import (
	"context"
	"errors"
	"fmt"
	"strings"

	"github.com/wh-studio/herdr-client/next/internal/history"
)

// Continuation strategy order matches the original Host exactly:
// AlreadyLive → NativeResume → ContextTransfer → NeedsProjectSelection →
// Unsupported.
type ContinuationStrategy string

const (
	StrategyAlreadyLive        ContinuationStrategy = "already-live"
	StrategyNativeResume       ContinuationStrategy = "native-resume"
	StrategyContextTransfer    ContinuationStrategy = "context-transfer"
	StrategyNeedsProjectSelect ContinuationStrategy = "needs-project-selection"
	StrategyUnsupported        ContinuationStrategy = "unsupported"
)

// ResumeIntent names one resumable provider-native session.
type ResumeIntent struct {
	Provider        history.AgentID
	NativeSessionID string
}

// ResumeArgs ports the resume.rs authority: the provider-native resume argv
// for one agent. All registered providers have resume parts.
func ResumeArgs(intent ResumeIntent) ([]string, error) {
	id := intent.NativeSessionID
	switch intent.Provider {
	case history.AgentClaudeCode:
		return []string{"--resume", id}, nil
	case history.AgentCodex:
		return []string{"resume", id}, nil
	case history.AgentCopilot:
		return []string{"--resume=" + id}, nil
	case history.AgentCursor:
		return []string{"--resume", id}, nil
	case history.AgentPi:
		return []string{"--session", id}, nil
	case history.AgentOMP:
		return []string{"--session", id}, nil
	case history.AgentOpenCode:
		return []string{"--session", id}, nil
	case history.AgentCommandCode:
		return []string{"--session", id}, nil
	case history.AgentAntigravity:
		return []string{"--conversation", id}, nil
	case history.AgentGemini:
		return []string{"--resume", id}, nil
	case history.AgentKiro:
		return []string{"--resume", id}, nil
	case history.AgentKimi:
		return []string{"--resume", id}, nil
	case history.AgentGrok:
		return []string{"--resume", id}, nil
	case history.AgentDSH:
		return []string{"--resume", id}, nil
	case history.AgentQoder:
		return []string{"--resume", id}, nil
	default:
		return nil, fmt.Errorf("resume isn't supported for %s yet", intent.Provider.DisplayName())
	}
}

// LiveAgentIdentity is the Herdr-typed session identity of one live Agent
// (from agent.get's agent_session), the exact match key for AlreadyLive.
type LiveAgentIdentity struct {
	PaneID   string
	Kind     string // the typed session's provider kind, e.g. "claude-code"
	Value    string // the provider-native session id or source path
	Revision int64
}

// ContinuationSource is the read-only History fact for one conversation.
type ContinuationSource struct {
	Agent           history.AgentID
	ID              string // provider-native session id
	FilePath        string // exact source path
	ProjectPath     string
	ResumeSupported bool
}

// ContinuationPlan is one planned, read-only continuation — no runtime
// mutation has happened when it is returned.
type ContinuationPlan struct {
	Strategy ContinuationStrategy
	Reason   string // Unsupported detail

	// AlreadyLive.
	PaneID   string
	Identity LiveAgentIdentity

	// NativeResume / ContextTransfer.
	Session    ContinuationSource
	Target     history.AgentID
	ResumeArgs []string
}

// PlanHistoryContinuation ports the read-only planner: target gates
// AlreadyLive; project resolution precedes any runtime mutation; NativeResume
// never requires a readable transfer source; only ContextTransfer does.
func PlanHistoryContinuation(source ContinuationSource, target *history.AgentID, liveAgents []LiveAgentIdentity, projectOverride string) ContinuationPlan {
	effective := source.Agent
	if target != nil {
		effective = *target
	}

	// 1. AlreadyLive: exact same session live and the same target provider.
	if effective == source.Agent {
		for _, live := range liveAgents {
			if historySessionMatchesHerdrIdentity(source.Agent, source.ID, source.FilePath, live) {
				return ContinuationPlan{
					Strategy: StrategyAlreadyLive,
					PaneID:   live.PaneID,
					Identity: live,
				}
			}
		}
	}

	// Project resolution precedes any runtime mutation: the explicit
	// override wins over the stored path; a stored path may satisfy alone.
	projectPath := strings.TrimSpace(source.ProjectPath)
	if override := strings.TrimSpace(projectOverride); override != "" {
		projectPath = override
	}
	if projectPath == "" {
		return ContinuationPlan{Strategy: StrategyNeedsProjectSelect}
	}
	source.ProjectPath = projectPath

	// 2. NativeResume: same provider with exact resumable native session.
	if effective == source.Agent && ResumeSupported(source.Agent) {
		args, err := ResumeArgs(ResumeIntent{Provider: source.Agent, NativeSessionID: source.ID})
		if err != nil {
			return ContinuationPlan{Strategy: StrategyUnsupported, Reason: err.Error()}
		}
		return ContinuationPlan{Strategy: StrategyNativeResume, Session: source, Target: effective, ResumeArgs: args}
	}

	// 3. ContextTransfer: different provider (or same provider without exact
	// native resume). Only this strategy requires the exact provider source.
	if source.FilePath == "" || source.ID == "" {
		return ContinuationPlan{Strategy: StrategyUnsupported, Reason: "the conversation's provider-native source is unavailable"}
	}
	return ContinuationPlan{Strategy: StrategyContextTransfer, Session: source, Target: effective}
}

// historySessionMatchesHerdrIdentity ports the exact identity matcher:
// same provider kind, and the typed identity value equals the history
// session id ("id") or the exact source path ("path"). Never cwd/mtime.
func historySessionMatchesHerdrIdentity(historyAgent history.AgentID, historyID, historyPath string, live LiveAgentIdentity) bool {
	if live.Kind != string(historyAgent) {
		return false
	}
	// The typed identity arrives as "kind:value" pairs resolved by the
	// transport; the value alone is the authoritative match fact.
	switch {
	case strings.HasPrefix(live.Value, "id:"):
		return strings.TrimPrefix(live.Value, "id:") == historyID
	case strings.HasPrefix(live.Value, "path:"):
		return strings.TrimPrefix(live.Value, "path:") == historyPath
	default:
		return live.Value == historyID || live.Value == historyPath
	}
}

// ContinuationInstruction carries the optional user instruction.
type ContinuationInstruction struct {
	Instruction string
	// Briefing for ContextTransfer: the bounded provider-source briefing
	// built by the caller from the read-only transcript.
	Briefing string
}

// ExecuteContinuation executes one planned continuation over the launch
// runtime. AlreadyLive prompts the existing pane; NativeResume and
// ContextTransfer launch through the canonical transaction with exactly one
// initial briefing.
func ExecuteContinuation(ctx context.Context, runtime LaunchRuntime, preparer ProjectPreparer, plan ContinuationPlan, instruction ContinuationInstruction) (ContinuationOutcome, error) {
	switch plan.Strategy {
	case StrategyAlreadyLive:
		if strings.TrimSpace(instruction.Instruction) != "" {
			if err := runtime.PromptAgentOnce(ctx, plan.PaneID, instruction.Instruction); err != nil {
				return ContinuationOutcome{}, err
			}
		}
		return ContinuationOutcome{ReusedLive: true, PaneID: plan.PaneID}, nil

	case StrategyNativeResume:
		intent := LaunchIntent{
			OperationID:       "continue-" + plan.Session.ID,
			ProjectPath:       plan.Session.ProjectPath,
			Mode:              LaunchModeBuild,
			Permission:        PermissionAskApproval,
			Agent:             plan.Session.Agent,
			Prompt:            instruction.Instruction,
			ExtraArgs:         plan.ResumeArgs,
			SkipInitialPrompt: strings.TrimSpace(instruction.Instruction) == "",
		}
		outcome, failure := RunLaunchTransaction(ctx, runtime, preparer, intent, DefaultTimings())
		if failure != nil {
			return ContinuationOutcome{}, failure
		}
		return ContinuationOutcome{Strategy: StrategyNativeResume, PaneID: outcome.PaneID, TabID: outcome.TabID}, nil

	case StrategyContextTransfer:
		briefing := strings.TrimSpace(instruction.Briefing)
		if briefing == "" {
			return ContinuationOutcome{}, errors.New("context transfer requires the bounded source briefing; none could be built")
		}
		// The briefing is the single initial prompt; the user's continuation
		// instruction is appended to it rather than sent as a second prompt.
		prompt := briefing
		if instruction.Instruction != "" {
			prompt += "\n\nContinuation instruction: " + instruction.Instruction
		}
		intent := LaunchIntent{
			OperationID: "transfer-" + plan.Session.ID,
			ProjectPath: plan.Session.ProjectPath,
			Mode:        LaunchModeBuild,
			Permission:  PermissionAskApproval,
			Agent:       plan.Target,
			Prompt:      prompt,
		}
		outcome, failure := RunLaunchTransaction(ctx, runtime, preparer, intent, DefaultTimings())
		if failure != nil {
			return ContinuationOutcome{}, failure
		}
		return ContinuationOutcome{Strategy: StrategyContextTransfer, PaneID: outcome.PaneID, TabID: outcome.TabID}, nil

	case StrategyNeedsProjectSelect:
		return ContinuationOutcome{}, errors.New("select a Project before continuing")

	default:
		return ContinuationOutcome{}, errors.New("unsupported continuation: " + plan.Reason)
	}
}

// ContinuationOutcome is the executed result (ReusedLive vs Launched).
type ContinuationOutcome struct {
	ReusedLive bool
	Strategy   ContinuationStrategy
	PaneID     string
	TabID      string
}

// TransferBriefingLimits bound the ContextTransfer briefing built from the
// source transcript (0.5 §17: bounded transfer semantics — the full M5
// artifact store lands with the 0.6 Conversation service).
type TransferBriefingLimits struct {
	MaxChars    int
	MaxMessages int
}

func DefaultTransferBriefingLimits() TransferBriefingLimits {
	return TransferBriefingLimits{MaxChars: 12000, MaxMessages: 40}
}

// BuildTransferBriefing builds the bounded ContextTransfer briefing from the
// source transcript window read out of the history page cache: a header
// naming source provider/session, then the most recent messages (bounded
// count, then bounded characters, Unicode-safe) with their roles. The result
// is the single initial prompt of the transfer launch.
// clipBounded truncates on a rune boundary with an ellipsis marker.
func clipBounded(text string, max int) string {
	if len(text) <= max {
		return text
	}
	end := max
	for end > 0 && !isRuneStart(text, end) {
		end--
	}
	return text[:end] + "…"
}

func isRuneStart(text string, index int) bool {
	if index >= len(text) {
		return true
	}
	return text[index]&0xC0 != 0x80
}

func BuildTransferBriefing(source ContinuationSource, target history.AgentID, messages []history.TranscriptMessage, limits TransferBriefingLimits) string {
	if limits.MaxMessages <= 0 {
		limits = DefaultTransferBriefingLimits()
	}
	if limits.MaxChars <= 0 {
		limits = DefaultTransferBriefingLimits()
		limits.MaxChars = 12000
	}

	var recent []history.TranscriptMessage
	for i := len(messages) - 1; i >= 0 && len(recent) < limits.MaxMessages; i-- {
		if strings.TrimSpace(messages[i].Text) == "" {
			continue
		}
		recent = append([]history.TranscriptMessage{messages[i]}, recent...)
	}

	var builder strings.Builder
	builder.WriteString("You are continuing work originally started with another agent.\n")
	builder.WriteString(fmt.Sprintf("Source: %s session %s. Target: %s.\n\n", source.Agent.DisplayName(), source.ID, target.DisplayName()))
	builder.WriteString("Conversation so far (most recent context):\n")

	var body strings.Builder
	bodyChars := 0
	wrote := false
	for _, message := range recent {
		if strings.TrimSpace(message.Text) == "" {
			continue
		}
		text := clipBounded(message.Text, 1200)
		line := "[" + strings.ToUpper(string(message.Role)) + "] " + strings.ReplaceAll(text, "\n", " ") + "\n"
		if bodyChars+len(line) > limits.MaxChars {
			break
		}
		body.WriteString(line)
		bodyChars += len(line)
		wrote = true
	}
	if !wrote {
		body.WriteString("(no textual history was available in the source)\n")
	}
	builder.WriteString(body.String())
	builder.WriteString("\nContinue this work seamlessly in this session; do not repeat the history.")
	return builder.String()
}
