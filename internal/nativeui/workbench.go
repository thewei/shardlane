package nativeui

import (
	"time"

	"github.com/egoist/mygo"
	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/herdr"
	"github.com/wh-studio/herdr-client/internal/history"
)

// workbenchState is the Shell's 0.5 Agent workbench presentation state: the
// client-owned markers, the reconciled directory, per-agent previous phases,
// and the MRU visit order. Runtime authority stays with Herdr.
type workbenchState struct {
	markers   *agent.MarkerStore
	directory *agent.AgentDirectory
	previous  map[agent.AgentKey]agent.AgentRuntimePhase
	mru       []agent.AgentKey
	// recentWorking anchors the directory sort hysteresis: the instant each
	// Agent's Working status may stop ordering it near the top. Agent hooks
	// report idle between every short task loop, and without the anchor the
	// honest bucket flip would keep reshuffling the list.
	recentWorking map[agent.AgentKey]time.Time

	// /agents + header popover client-local filter state.
	filterAll  bool
	filter     agent.AgentDirectoryBucket
	headerOpen bool

	// switcher is the MRU overlay state (0.5 §13).
	switcherOpen   bool
	switcherIndex  int
	switcherOrigin agent.AgentKey
	switcherHad    bool
}

// reconcileWorkbench rebuilds the Agent directory from the just-applied
// Herdr projection: one classified transition per agent, one marker store
// update, one directory replacement per reconciled snapshot (0.5 §21: one
// event burst → one projection update; no render-time IO).
func (s *Shell) reconcileWorkbench() {
	if s.workbench.markers == nil {
		s.workbench.markers = agent.NewMarkerStore()
	}
	if s.workbench.directory == nil {
		s.workbench.directory = agent.NewAgentDirectory()
	}
	if s.workbench.previous == nil {
		s.workbench.previous = make(map[agent.AgentKey]agent.AgentRuntimePhase)
	}
	if s.workbench.recentWorking == nil {
		s.workbench.recentWorking = make(map[agent.AgentKey]time.Time)
	}

	projectName := make(map[string]string, len(s.projection.Projects))
	for _, project := range s.projection.Projects {
		projectName[project.ID] = project.Label
	}

	cards := make([]agent.AgentCardModel, 0, len(s.projection.Agents))
	live := make(map[agent.AgentKey]bool, len(s.projection.Agents))
	now := time.Now()
	for _, runtimeAgent := range s.projection.Agents {
		key := agent.AgentKey{InstanceID: s.activeInstance, TerminalID: runtimeAgent.TerminalID}
		phase := agent.ParseRuntimePhase(runtimeAgent.Status, false)
		previous := s.workbench.previous[key]
		transition := agent.ClassifyTransition(previous, phase)
		s.workbench.previous[key] = phase
		if phase == agent.PhaseWorking {
			s.workbench.recentWorking[key] = now.Add(agent.WorkingHysteresis)
		}

		selected := runtimeAgent.PaneID != "" && runtimeAgent.PaneID == s.selectedPaneID
		markers := s.workbench.markers.Observe(key, transition, selected)
		s.notifyAgentTransition(runtimeAgent, transition)

		provider, _ := history.ParseAgentID(runtimeAgent.Kind)
		card := agent.ProjectAgentCard(agent.AgentCardInputs{
			InstanceID:  key.InstanceID,
			TerminalID:  key.TerminalID,
			PaneID:      runtimeAgent.PaneID,
			TabID:       runtimeAgent.TabID,
			ProjectID:   runtimeAgent.ProjectID,
			ProjectName: projectName[runtimeAgent.ProjectID],
			Provider:    provider,
			Title:       agentTitleFrom(runtimeAgent),
			AgentStatus: runtimeAgent.Status,
			Revision:    int64(runtimeAgent.Revision),
			Attention:   agent.ParseOperationalState(runtimeAgent.Status),
			Markers:     markers,
		})
		card.ActiveUntil = s.workbench.recentWorking[key]
		cards = append(cards, card)
		live[key] = true
	}
	s.workbench.markers.Retain(live)
	for key := range s.workbench.recentWorking {
		if !live[key] {
			delete(s.workbench.recentWorking, key)
		}
	}
	s.workbench.directory.Replace(cards)
	s.pruneAgentMRU(live)
}

// agentTitleFrom applies the title fallback: name → provider display name
// → "Agent" (the projection carries no separate task title yet).
func agentTitleFrom(runtimeAgent herdr.Agent) string {
	if runtimeAgent.Name != "" {
		return runtimeAgent.Name
	}
	return ""
}

// notifyAgentTransition delivers the narrow transition notification through
// MyGo when the platform supports it. Markers stay the in-app source of
// truth even when OS delivery is unavailable (0.5 §15).
func (s *Shell) notifyAgentTransition(runtimeAgent herdr.Agent, transition agent.AgentTransition) {
	notification := agent.NotificationFor(transition, agentTitleFrom(runtimeAgent))
	if notification == nil {
		return
	}
	// Headless builds (unit tests) have no window and no app loop: MyGo's
	// notification calls hop to the main thread and would block forever.
	if s.win == nil {
		return
	}
	if !mygo.NotificationsSupported() {
		return
	}
	showAgentNotification(s.activeInstance, runtimeAgent, notification)
}

// workbenchCards returns the reconciled cards in actionable-first order.
func (s *Shell) workbenchCards() []agent.AgentCardModel {
	if s.workbench.directory == nil {
		return nil
	}
	return s.workbench.directory.All()
}

// workbenchFiltered returns the cards for the /agents workbench filter.
func (s *Shell) workbenchFiltered() []agent.AgentCardModel {
	cards := s.workbenchCards()
	if s.workbench.filterAll {
		return cards
	}
	filtered := make([]agent.AgentCardModel, 0, len(cards))
	for _, card := range cards {
		if agent.BucketOf(card) == s.workbench.filter {
			filtered = append(filtered, card)
		}
	}
	return filtered
}

// openAgentCard navigates client-locally to the Agent's owning pane (which
// lands blocked agents on their Terminal), records the MRU visit and clears
// unread. It never emits a Herdr focus RPC.
func (s *Shell) openAgentCard(card agent.AgentCardModel) {
	s.workbench.markers.Visit(card.Key)
	s.recordAgentVisit(card.Key)
	if card.PaneID != "" {
		s.selectPane(card.PaneID)
	}
	if s.router.Path() != routeWorkspace {
		s.router.Push(routeWorkspace)
	}
}

// markAgentReviewed is the explicit review action (0.5 §12): it never runs
// implicitly on visit.
func (s *Shell) markAgentReviewed(key agent.AgentKey) {
	s.workbench.markers.MarkReviewed(key)
	s.reconcileWorkbenchMarkersOnly()
}

// reconcileWorkbenchMarkersOnly refreshes marker facts on the existing cards
// without a new transition classification pass.
func (s *Shell) reconcileWorkbenchMarkersOnly() {
	cards := s.workbenchCards()
	updated := make([]agent.AgentCardModel, 0, len(cards))
	for _, card := range cards {
		card.Unread = s.workbench.markers.Get(card.Key).Unread
		card.ReviewPending = s.workbench.markers.Get(card.Key).ReviewPending
		updated = append(updated, card)
	}
	s.workbench.directory.Replace(updated)
	// Marker facts render in the floating panel too; it repaints only when
	// invalidated (it gets no input between user interactions).
	s.invalidateQuickPanel()
}

// recordAgentVisit maintains the MRU order (0.5 §13): most recent first,
// bounded to 10 live Agent identities.
func (s *Shell) recordAgentVisit(key agent.AgentKey) {
	next := make([]agent.AgentKey, 0, len(s.workbench.mru)+1)
	next = append(next, key)
	for _, existing := range s.workbench.mru {
		if existing != key {
			next = append(next, existing)
		}
	}
	if len(next) > 10 {
		next = next[:10]
	}
	s.workbench.mru = next
}

func (s *Shell) pruneAgentMRU(live map[agent.AgentKey]bool) {
	pruned := make([]agent.AgentKey, 0, len(s.workbench.mru))
	for _, key := range s.workbench.mru {
		if live[key] {
			pruned = append(pruned, key)
		}
	}
	if len(pruned) > 10 {
		pruned = pruned[:10]
	}
	s.workbench.mru = pruned
}

// AgentMRUList returns the current MRU order for the switcher overlay.
func (s *Shell) AgentMRUList() []agent.AgentCardModel {
	cards := make([]agent.AgentCardModel, 0, len(s.workbench.mru))
	for _, key := range s.workbench.mru {
		if card, ok := s.workbench.directory.Get(key); ok {
			cards = append(cards, card)
		}
	}
	return cards
}
