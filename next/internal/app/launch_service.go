package app

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"sync"
	"time"

	"github.com/wh-studio/herdr-client/next/internal/agent"
	"github.com/wh-studio/herdr-client/next/internal/conversation"
	"github.com/wh-studio/herdr-client/next/internal/herdr"
	"github.com/wh-studio/herdr-client/next/internal/history"
)

// LaunchService binds the Agent-runtime transactions to the verified Herdr
// transport adapter. Herdr stays the sole runtime authority; the service
// only drives the schema-verified protocol methods. The New Task launch
// path was removed on 2026-10-07 (the page is gone); what remains serves
// chat follow-ups, live handoff and history continuation.
type LaunchService struct {
	manager  *herdr.Manager
	registry *agent.LaunchRegistry
	// reservations is the per-Conversation reservation coordinator shared
	// by the follow-up delivery worker, semantic prompts, and the live
	// handoff (0.6 §11/§19).
	reservations *conversation.Reservations
	// queue is the one-follow-up-per-conversation queue; the worker owns
	// its Delivering→Delivered/DeliveryUncertain state machine.
	queue      *conversation.FollowUpQueue
	queueWake  chan struct{}
	workerMu   sync.Mutex
	workerStop context.CancelFunc
	// ledgerPath owns the durable delivery ledger (0.6 §9.3): attached via
	// AttachLedgerPath, persisted on every queue mutation.
	ledgerMu   sync.Mutex
	ledgerPath string
}

func NewLaunchService(manager *herdr.Manager) *LaunchService {
	return &LaunchService{
		manager:      manager,
		registry:     agent.NewLaunchRegistry(),
		reservations: conversation.NewReservations(),
		queue:        conversation.NewFollowUpQueue(),
		queueWake:    make(chan struct{}, 1),
	}
}

// Registry is the idempotency seam every launch goes through.
func (s *LaunchService) Registry() *agent.LaunchRegistry { return s.registry }

// Queue exposes the follow-up queue for projection/cancel surfaces.
func (s *LaunchService) Queue() *conversation.FollowUpQueue { return s.queue }

// AttachLedgerPath makes the delivery ledger durable across restarts (0.6
// §9.3): the existing file loads immediately (a crash mid-delivery restores
// its item as DeliveryUncertain — the outcome is genuinely unknown, never
// guessed), and every subsequent ledger mutation persists atomically.
// Idempotent; the path wins over any previously attached ledger.
func (s *LaunchService) AttachLedgerPath(path string) {
	if path == "" {
		return
	}
	s.ledgerMu.Lock()
	defer s.ledgerMu.Unlock()
	if s.ledgerPath == path {
		return
	}
	s.ledgerPath = path
	s.loadLedger(path)
	s.queue.SetOnChanged(func() { s.persistLedger() })
}

// ledgerFileName is the ledger file inside the MyGo user-data directory.
const ledgerFileName = "follow-up-ledger.json"

func (s *LaunchService) loadLedger(path string) {
	data, err := os.ReadFile(path)
	if err != nil {
		return
	}
	var file struct {
		Items []conversation.QueueItem `json:"items"`
	}
	if json.Unmarshal(data, &file) != nil {
		return
	}
	// A crash mid-delivery leaves the outcome unknown: the honest restart
	// state is the uncertainty tombstone, never a silent resend.
	for i := range file.Items {
		if file.Items[i].State == conversation.Delivering {
			file.Items[i].State = conversation.DeliveryUncertain
			file.Items[i].Detail = "delivery outcome unknown across restart — inspect before retry"
		}
	}
	s.queue.Restore(file.Items)
}

func (s *LaunchService) persistLedger() {
	s.ledgerMu.Lock()
	path, items := s.ledgerPath, s.queue.Snapshot()
	s.ledgerMu.Unlock()
	if path == "" {
		return
	}
	data, err := json.MarshalIndent(struct {
		Items []conversation.QueueItem `json:"items"`
	}{Items: items}, "", "  ")
	if err != nil {
		return
	}
	tmp := path + ".tmp"
	if err := os.WriteFile(tmp, data, 0o600); err != nil {
		return
	}
	_ = os.Rename(tmp, path)
}

// PendingOperations is the handoff's pending-operation seam (HANDOFF-06):
// the same ledger the delivery worker owns.
func (s *LaunchService) PendingOperations() agent.PendingOperations { return s.queue }

// EnqueueFollowUp accepts one queued follow-up and wakes the worker.
func (s *LaunchService) EnqueueFollowUp(item conversation.QueueItem) error {
	if err := s.queue.Enqueue(item); err != nil {
		return err
	}
	select {
	case s.queueWake <- struct{}{}:
	default:
	}
	return nil
}

// StartFollowUpWorker starts the background delivery loop (idempotent).
func (s *LaunchService) StartFollowUpWorker() {
	s.workerMu.Lock()
	defer s.workerMu.Unlock()
	if s.workerStop != nil {
		return
	}
	ctx, cancel := context.WithCancel(context.Background())
	s.workerStop = cancel
	worker := conversation.NewDeliveryWorker(s.queue, &herdrDeliveryTransport{manager: s.manager}, s.reservations, s.queueWake)
	go worker.Run(ctx)
}

// StopFollowUpWorker stops the background delivery loop (idempotent).
func (s *LaunchService) StopFollowUpWorker() {
	s.workerMu.Lock()
	defer s.workerMu.Unlock()
	if s.workerStop != nil {
		s.workerStop()
		s.workerStop = nil
	}
}

// herdrDeliveryTransport binds the §11.3 pipeline to the verified Herdr
// methods: exact-target wait, typed session fingerprint, one semantic
// prompt, and the typed uncertainty of the prompt transaction.
type herdrDeliveryTransport struct {
	manager *herdr.Manager
}

// deliveryWaitTimeout bounds the exact-target wait; a timeout retains the
// queue (§11.4) for a later wake.
const deliveryWaitTimeoutMS int64 = 60_000

func (t *herdrDeliveryTransport) WaitTurnBoundary(ctx context.Context, item conversation.QueueItem) error {
	if t.manager == nil {
		return errors.New("herdr transport unavailable")
	}
	return t.manager.WaitAgentState(ctx, item.Instance, item.AgentPaneID, []string{"idle"}, deliveryWaitTimeoutMS)
}

func (t *herdrDeliveryTransport) OccupantFingerprint(ctx context.Context, item conversation.QueueItem) (string, error) {
	if t.manager == nil {
		return "", errors.New("herdr transport unavailable")
	}
	info, err := t.manager.AgentByPane(ctx, item.Instance, item.AgentPaneID)
	if err != nil {
		return "", err
	}
	if info == nil || info.AgentSession == nil {
		return "", nil
	}
	provider := info.AgentSession.Agent
	providerID, ok := history.ParseAgentID(provider)
	if !ok {
		return "", nil
	}
	return conversation.SessionIdentity{
		Provider: string(providerID),
		Kind:     info.AgentSession.Kind,
		Source:   info.AgentSession.Source,
		Value:    info.AgentSession.Value,
	}.Fingerprint(), nil
}

func (t *herdrDeliveryTransport) Deliver(ctx context.Context, item conversation.QueueItem) error {
	if t.manager == nil {
		return errors.New("herdr transport unavailable")
	}
	return t.manager.PromptAgent(ctx, item.Instance, item.AgentPaneID, item.Text)
}

func (t *herdrDeliveryTransport) IsUncertain(err error) bool {
	var uncertain *herdr.DeliveryUncertainError
	return errors.As(err, &uncertain)
}

// handoffReservation was replaced by the shared conversation.Reservations
// coordinator (s.reservations).

// HandoffLiveAgent executes one live handoff (0.6 §19/HANDOFF-01..09) from
// the bound session: exact-identity fences, pending-operation fence,
// bounded settle, freshness fence, live-source briefing, canonical target
// launch. The source Agent is never stopped, closed, or mutated. pending is
// the caller-owned follow-up queue seam (nil skips the fence); blocking, so
// callers run it on a background lane.
func (s *LaunchService) HandoffLiveAgent(ctx context.Context, session string, request agent.LiveHandoffRequest, pending agent.PendingOperations, timing agent.HandoffTiming) (agent.LiveHandoffOutcome, *agent.LiveHandoffFailure) {
	runtime := &herdrLaunchRuntime{manager: s.manager, session: session}
	return agent.RunLiveHandoff(ctx, runtime, runtime, Preparer{}, request, pending, s.reservations.For(request.SourceConversation), timing)
}

// PromptLiveAgent submits one semantic prompt to a live agent pane through
// the verified agent.prompt transport. Delivery uncertainty is typed by the
// transport and must be surfaced, never auto-retried.
func (s *LaunchService) PromptLiveAgent(ctx context.Context, session, paneID, text string) error {
	return s.manager.PromptAgent(ctx, session, paneID, text)
}

// RuntimeFor returns the session-bound launch runtime. The session is bound
// at click time so a workspace switch between form fill and submit cannot
// retarget the launch.
func (s *LaunchService) RuntimeFor(session string) agent.LaunchRuntime {
	return &herdrLaunchRuntime{manager: s.manager, session: session}
}

// Preparer is the transaction's project preparation seam. 0.4 validates the
// target directory and explicitly refuses branch worktree preparation
// (unsupported rather than guessed).
type Preparer struct{}

func (Preparer) Prepare(ctx context.Context, projectPath, branch string) (string, error) {
	if branch != "" {
		return "", fmt.Errorf("branch worktree preparation is not supported in this build")
	}
	info, err := os.Stat(projectPath)
	if err != nil {
		return "", fmt.Errorf("project path %s: %w", projectPath, err)
	}
	if !info.IsDir() {
		return "", fmt.Errorf("project path %s is not a directory", projectPath)
	}
	return projectPath, nil
}

// herdrLaunchRuntime implements agent.LaunchRuntime over the verified
// transport for one session.
type herdrLaunchRuntime struct {
	manager *herdr.Manager
	session string
}

func (r *herdrLaunchRuntime) CreateTabWithoutFocus(ctx context.Context, workspaceID, cwd string) (agent.TabCreated, error) {
	tabID, paneID, err := r.manager.CreateTabWithoutFocus(ctx, r.session, workspaceID, cwd)
	if err != nil {
		return agent.TabCreated{}, err
	}
	return agent.TabCreated{TabID: tabID, PaneID: paneID}, nil
}

func (r *herdrLaunchRuntime) CreateWorkspaceWithoutFocus(ctx context.Context, cwd string) (agent.WorkspaceCreated, error) {
	workspaceID, tabID, paneID, err := r.manager.CreateWorkspaceWithoutFocus(ctx, r.session, cwd)
	if err != nil {
		return agent.WorkspaceCreated{}, err
	}
	return agent.WorkspaceCreated{WorkspaceID: workspaceID, Tab: agent.TabCreated{TabID: tabID, PaneID: paneID}}, nil
}

func (r *herdrLaunchRuntime) WorkspaceIDForPath(ctx context.Context, projectPath string) (string, error) {
	projection, err := r.manager.Projection(r.session)
	if err != nil {
		return "", err
	}
	for _, project := range projection.Projects {
		if project.CWD == projectPath {
			return project.ID, nil
		}
	}
	return "", nil
}

func (r *herdrLaunchRuntime) WaitShellReady(ctx context.Context, paneID string, timeoutMS int64) error {
	deadline := time.Now().Add(time.Duration(timeoutMS) * time.Millisecond)
	for {
		ready, err := r.manager.ShellReady(ctx, r.session, paneID)
		if err != nil {
			return err
		}
		if ready {
			return nil
		}
		if !time.Now().Before(deadline) {
			return fmt.Errorf("pane %s shell did not become interactive in %dms", paneID, timeoutMS)
		}
		sleepContext(ctx, 150*time.Millisecond)
	}
}

func (r *herdrLaunchRuntime) StartAgent(ctx context.Context, params agent.AgentStartParams) error {
	_, err := r.manager.StartAgent(ctx, r.session, herdr.AgentStartParams{
		Name:      params.Name,
		Kind:      params.Kind,
		PaneID:    params.PaneID,
		Args:      params.Args,
		TimeoutMS: params.TimeoutMS,
	})
	return err
}

func (r *herdrLaunchRuntime) ErrorIsUncertain(err error) bool {
	var uncertain *herdr.DeliveryUncertainError
	return errors.As(err, &uncertain)
}

func (r *herdrLaunchRuntime) WaitAgentIdle(ctx context.Context, paneID string, timeoutMS int64) error {
	return r.manager.WaitAgentState(ctx, r.session, paneID, []string{"idle"}, timeoutMS)
}

// SourceAgent is the handoff fence's authoritative occupant read (0.6
// §19.2): status, launch-pending and the typed session identity come from
// the same Herdr projection the prompt transaction trusts.
func (r *herdrLaunchRuntime) SourceAgent(ctx context.Context, paneID string) (*agent.HandoffSource, error) {
	info, err := r.manager.AgentByPane(ctx, r.session, paneID)
	if err != nil {
		return nil, err
	}
	if info == nil {
		return nil, nil
	}
	source := &agent.HandoffSource{
		PaneID:        info.PaneID,
		AgentStatus:   info.AgentStatus,
		LaunchPending: info.LaunchPending,
		Revision:      info.Revision,
	}
	if info.AgentSession != nil {
		value := info.AgentSession.Value
		switch info.AgentSession.Kind {
		case "id":
			value = "id:" + info.AgentSession.Value
		case "path":
			value = "path:" + info.AgentSession.Value
		}
		source.Session = &agent.LiveAgentIdentity{
			Kind:  info.AgentSession.Agent,
			Value: value,
		}
	}
	return source, nil
}

// WaitAgentSettled waits for the working turn to reach idle or blocked and
// reports which: blocked is a typed outcome of the settle wait, never a
// generic timeout (0.6 §19.3).
func (r *herdrLaunchRuntime) WaitAgentSettled(ctx context.Context, paneID string, timeoutMS int64) (agent.SettledState, error) {
	if err := r.manager.WaitAgentState(ctx, r.session, paneID, []string{"idle", "blocked"}, timeoutMS); err != nil {
		return "", err
	}
	info, err := r.manager.AgentByPane(ctx, r.session, paneID)
	if err != nil {
		return "", err
	}
	if info != nil && info.AgentStatus == "blocked" {
		return agent.SettledBlocked, nil
	}
	return agent.SettledIdle, nil
}

func (r *herdrLaunchRuntime) AgentByPane(ctx context.Context, paneID string) (*agent.StartedAgent, error) {
	info, err := r.manager.AgentByPane(ctx, r.session, paneID)
	if err != nil {
		return nil, err
	}
	if info == nil {
		return nil, nil
	}
	started := &agent.StartedAgent{
		PaneID:           info.PaneID,
		Agent:            kindOfHerdrAgent(info),
		InteractiveReady: info.InteractiveReady,
		Revision:         info.Revision,
	}
	if info.AgentSession != nil {
		started.AgentSession = info.AgentSession.Agent
	}
	return started, nil
}

// kindOfHerdrAgent resolves the Herdr agent kind for the identity gate: the
// typed session's provider when present, else the Herdr display-agent field.
// The exact kind mapping is pinned by the transport contract tests.
func kindOfHerdrAgent(info *herdr.AgentInfo) string {
	if info.AgentSession != nil && info.AgentSession.Agent != "" {
		return info.AgentSession.Agent
	}
	if info.Agent != nil {
		return *info.Agent
	}
	return ""
}

func (r *herdrLaunchRuntime) SendAgentKeys(ctx context.Context, paneID string, keys []string) error {
	return r.manager.SendAgentKeys(ctx, r.session, paneID, keys)
}

func (r *herdrLaunchRuntime) PromptAgentOnce(ctx context.Context, paneID string, text string) error {
	return r.manager.PromptAgent(ctx, r.session, paneID, text)
}

func (r *herdrLaunchRuntime) RenameTab(ctx context.Context, tabID, label string) error {
	_, err := r.manager.RenameTab(r.session, tabID, label)
	return err
}

func (r *herdrLaunchRuntime) RenamePane(ctx context.Context, paneID, label string) error {
	_, err := r.manager.RenamePane(r.session, paneID, label)
	return err
}

func (r *herdrLaunchRuntime) CloseTab(ctx context.Context, tabID string) error {
	_, err := r.manager.CloseTab(r.session, tabID)
	return err
}

func sleepContext(ctx context.Context, d time.Duration) {
	timer := time.NewTimer(d)
	defer timer.Stop()
	select {
	case <-timer.C:
	case <-ctx.Done():
	}
}

// HistoryContinuationDeps carry the read-only history access the
// continuation executor needs to build a ContextTransfer briefing from the
// page cache. The opener closure is the HistoryService bounded window read.
type HistoryContinuationDeps struct {
	// OpenWindow reads a bounded transcript window from the history page
	// cache (page-cache hit or adapter parse fallback).
	OpenWindow func(ctx context.Context, source history.SessionFileRef) (history.TranscriptWindow, error)
	// Limits bound the briefing; zero selects defaults.
	Limits *agent.TransferBriefingLimits
}

// ExecuteHistoryContinuation executes one planned continuation. For
// ContextTransfer it reads the source window from the history page cache,
// builds the bounded briefing, and launches through the canonical
// transaction with exactly one initial prompt.
func (s *LaunchService) ExecuteHistoryContinuation(ctx context.Context, session string, plan agent.ContinuationPlan, instruction string, deps HistoryContinuationDeps) (agent.ContinuationOutcome, error) {
	instructionField := agent.ContinuationInstruction{Instruction: instruction}
	if plan.Strategy == agent.StrategyContextTransfer {
		if deps.OpenWindow == nil {
			return agent.ContinuationOutcome{}, errors.New("context transfer requires a history window source")
		}
		source := history.SessionFileRef{
			Agent:     plan.Session.Agent,
			NativeID:  plan.Session.ID,
			FilePath:  plan.Session.FilePath,
			MtimeMS:   0,
			SizeBytes: 0,
		}
		window, err := deps.OpenWindow(ctx, source)
		if err != nil {
			return agent.ContinuationOutcome{}, fmt.Errorf("read transfer source: %w", err)
		}
		limits := agent.DefaultTransferBriefingLimits()
		if deps.Limits != nil {
			limits = *deps.Limits
		}
		instructionField.Briefing = agent.BuildTransferBriefing(plan.Session, plan.Target, window.Messages, limits)
	}
	return agent.ExecuteContinuation(ctx, s.RuntimeFor(session), Preparer{}, plan, instructionField)
}
