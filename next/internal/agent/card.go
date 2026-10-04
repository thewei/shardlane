package agent

import (
	"time"

	"github.com/wh-studio/herdr-client/next/internal/history"
)

// AgentCardModel is the one immutable, UI-independent model every Agent
// presentation consumes (0.5 §5). It carries no colors and no toolkit
// types. Runtime phase, attention, markers, sendability and integration
// health stay separate dimensions by construction.
type AgentCardModel struct {
	Key            AgentKey
	Provider       history.AgentID
	ProviderLabel  string
	Title          string
	ProjectID      string
	ProjectName    string
	TabID          string
	PaneID         string
	RuntimePhase   AgentRuntimePhase
	Attention      OperationalState
	Sendability    AgentSendability
	Unread         bool
	ReviewPending  bool
	ConversationID string
	Revision       int64
	// ActiveUntil anchors the sort hysteresis (0.5 §9): until this instant
	// an Idle card keeps sorting in the Working group. Zero means never
	// recently active. The honest status pill/bucket never reads it.
	ActiveUntil time.Time
}

// AgentCardInputs are the authoritative facts one card is projected from.
type AgentCardInputs struct {
	InstanceID    string
	TerminalID    string
	PaneID        string
	TabID         string
	ProjectID     string
	ProjectName   string
	Provider      history.AgentID
	Title         string
	Name          string
	AgentStatus   string
	LaunchPending bool
	Revision      int64

	// ConversationID binds a known conversation identity when one exists
	// (History continuation / live binding); empty means none yet.
	ConversationID string
	// Attention is the shared 0.4 operational projection for this agent.
	Attention OperationalState
	// Markers are the client-owned unread/review facts.
	Markers AgentMarkers
}

// ProjectAgentCard projects one immutable AgentCardModel from the
// authoritative inputs. All consumers (Sidebar, Header, /agents, switcher,
// future Chat) must derive their cards through this function instead of
// re-interpreting raw status fields.
func ProjectAgentCard(in AgentCardInputs) AgentCardModel {
	phase := ParseRuntimePhase(in.AgentStatus, in.LaunchPending)
	return AgentCardModel{
		Key:            AgentKey{InstanceID: in.InstanceID, TerminalID: in.TerminalID},
		Provider:       in.Provider,
		ProviderLabel:  in.Provider.DisplayName(),
		Title:          AgentTitle(in.Title, in.Name, in.Provider),
		ProjectID:      in.ProjectID,
		ProjectName:    in.ProjectName,
		TabID:          in.TabID,
		PaneID:         in.PaneID,
		RuntimePhase:   phase,
		Attention:      in.Attention,
		Sendability:    ClassifySendability(phase),
		Unread:         in.Markers.Unread,
		ReviewPending:  in.Markers.ReviewPending,
		ConversationID: in.ConversationID,
		Revision:       in.Revision,
	}
}

// PrimaryAction is the state-driven card action (0.5 §7). It never offers a
// generic prompt and never guesses PTY keys: blocked/failed agents route to
// the Terminal because their sendability says NeedsTerminal.
type PrimaryAction string

const (
	ActionOpenAgent     PrimaryAction = "open-agent"
	ActionOpenTerminal  PrimaryAction = "open-terminal"
	ActionNeedsReview   PrimaryAction = "mark-review-available"
	ActionCommittedOpen PrimaryAction = "open-committed-target"
)

// Working reports whether the agent is mid-flight: the operational
// projection says working, or the runtime phase is working or launching.
// It is the honest activity dimension — the sort hysteresis (ActiveUntil)
// never makes it true — and drives at-a-glance activity indicators.
func (c AgentCardModel) Working() bool {
	return c.Attention == OpWorking || c.RuntimePhase == PhaseWorking || c.RuntimePhase == PhaseLaunching
}

// PrimaryAction derives the safe primary action for the card state.
// CreatedNeedsAttention (a committed launch/continue with a failure detail)
// opens the committed target; it must never offer a blind retry.
func (c AgentCardModel) PrimaryAction() PrimaryAction {
	switch c.RuntimePhase {
	case PhaseBlocked, PhaseFailed:
		return ActionOpenTerminal
	case PhaseDone:
		if c.ReviewPending {
			return ActionNeedsReview
		}
		return ActionOpenAgent
	default:
		return ActionOpenAgent
	}
}
