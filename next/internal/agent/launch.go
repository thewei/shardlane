package agent

import (
	"context"
	"fmt"
	"strings"
	"sync"

	"github.com/wh-studio/herdr-client/next/internal/history"
)

// StartAgentRequest is the launch intent DTO (AGENT-02): explicit targets and
// a caller-chosen request identity. It is the unit of idempotency for the
// launch transaction; no Herdr socket method is invoked here.
type StartAgentRequest struct {
	// RequestID is the caller-chosen idempotency identity. The same ID must
	// never produce two launches.
	RequestID string          `json:"request_id"`
	Instance  string          `json:"instance"`
	ProjectID string          `json:"project_id"`
	Provider  history.AgentID `json:"provider"`
	CWD       string          `json:"cwd"`
	Prompt    string          `json:"prompt,omitempty"`
	// ResumeSource optionally names the History conversation key this launch
	// continues. Execution stays behind the transaction.
	ResumeSource string `json:"resume_source,omitempty"`
}

// Validate enforces the explicit-target contract: no guessed instances,
// projects, or working directories.
func (r StartAgentRequest) Validate() error {
	switch {
	case strings.TrimSpace(r.RequestID) == "":
		return fmt.Errorf("request id is required")
	case strings.TrimSpace(r.Instance) == "":
		return fmt.Errorf("instance is required")
	case strings.TrimSpace(r.ProjectID) == "":
		return fmt.Errorf("project id is required")
	case strings.TrimSpace(r.CWD) == "":
		return fmt.Errorf("cwd is required")
	}
	if _, ok := history.ParseAgentID(string(r.Provider)); !ok {
		return fmt.Errorf("unknown provider %q", r.Provider)
	}
	return nil
}

// ConflictingRequestError reports a same-RequestID submit whose body differs
// from the original: replaying it would be unsafe idempotency.
type ConflictingRequestError struct {
	RequestID string
}

func (e *ConflictingRequestError) Error() string {
	return fmt.Sprintf("request %q already exists with a different body", e.RequestID)
}

// LaunchOutcomeRecord records what one accepted request produced.
type LaunchOutcomeRecord struct {
	RequestID string `json:"request_id"`
	Launched  bool   `json:"launched"`
	// AgentKey identifies the created Agent once the transaction exists.
	AgentKey string `json:"agent_key,omitempty"`
}

// launchCall is the single in-flight owner for one RequestID. Duplicate
// Submits await the same done channel; the launch callback runs exactly once
// and never under the registry mutex.
type launchCall struct {
	request StartAgentRequest
	done    chan struct{}
	outcome LaunchOutcomeRecord
	err     error
}

type claimState uint8

const (
	claimLaunch claimState = iota
	claimAwait
	claimReplay
	claimConflict
)

// LaunchRegistry is the concurrent idempotency seam (CLOSURE-02): one
// RequestID maps to at most one launch callback invocation; equivalent
// duplicates await and replay the same result; a conflicting body is
// rejected; waiter cancellation never creates a second launch.
type LaunchRegistry struct {
	mu    sync.Mutex
	calls map[string]*launchCall
}

func NewLaunchRegistry() *LaunchRegistry {
	return &LaunchRegistry{calls: make(map[string]*launchCall)}
}

// Submit resolves one request idempotently. launch is invoked at most once
// per RequestID.
func (r *LaunchRegistry) Submit(ctx context.Context, request StartAgentRequest, launch func(context.Context, StartAgentRequest) (LaunchOutcomeRecord, error)) (LaunchOutcomeRecord, error) {
	if err := request.Validate(); err != nil {
		return LaunchOutcomeRecord{}, err
	}

	call, state := r.claim(request)
	switch state {
	case claimConflict:
		return LaunchOutcomeRecord{}, &ConflictingRequestError{RequestID: request.RequestID}
	case claimReplay:
		// done is closed: the recorded outcome/err are visible without locks.
		return call.outcome, call.err
	case claimAwait:
		return r.await(ctx, call)
	default: // claimLaunch: this caller is the single owner.
		outcome, err := launch(ctx, request)
		r.mu.Lock()
		call.outcome, call.err = outcome, err
		if outcome.RequestID == "" {
			call.outcome.RequestID = request.RequestID
		}
		r.mu.Unlock()
		close(call.done)
		if err != nil {
			return LaunchOutcomeRecord{}, err
		}
		return call.outcome, nil
	}
}

// claim classifies this submit against the registry state for its RequestID.
func (r *LaunchRegistry) claim(request StartAgentRequest) (*launchCall, claimState) {
	r.mu.Lock()
	defer r.mu.Unlock()
	existing, ok := r.calls[request.RequestID]
	if !ok {
		call := &launchCall{request: request, done: make(chan struct{})}
		r.calls[request.RequestID] = call
		return call, claimLaunch
	}
	if existing.request != request {
		return nil, claimConflict
	}
	select {
	case <-existing.done:
		return existing, claimReplay
	default:
		return existing, claimAwait
	}
}

// await blocks on the in-flight owner's result. A cancelled waiter returns
// early; the owner keeps running and its result stays recorded.
func (r *LaunchRegistry) await(ctx context.Context, call *launchCall) (LaunchOutcomeRecord, error) {
	select {
	case <-call.done:
		return call.outcome, call.err
	case <-ctx.Done():
		return LaunchOutcomeRecord{}, ctx.Err()
	}
}

// Outcome reports the recorded outcome for a completed request id, if any.
func (r *LaunchRegistry) Outcome(requestID string) (LaunchOutcomeRecord, bool) {
	r.mu.Lock()
	defer r.mu.Unlock()
	call, ok := r.calls[requestID]
	if !ok {
		return LaunchOutcomeRecord{}, false
	}
	select {
	case <-call.done:
		return call.outcome, true
	default:
		return LaunchOutcomeRecord{}, false
	}
}
