package agent

import (
	"context"
	"errors"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/wh-studio/herdr-client/next/internal/history"
)

func launchRequest(id string) StartAgentRequest {
	return StartAgentRequest{
		RequestID: id,
		Instance:  "default",
		ProjectID: "w1",
		Provider:  history.AgentCodex,
		CWD:       "/work/demo",
	}
}

// TestLaunchRegistryConcurrentDuplicatesLaunchOnce drives N simultaneous
// duplicate Submits through a start barrier: the callback must run exactly
// once and every caller observes the same outcome.
func TestLaunchRegistryConcurrentDuplicatesLaunchOnce(t *testing.T) {
	const rounds = 5
	const callers = 24
	for round := 0; round < rounds; round++ {
		registry := NewLaunchRegistry()
		var launches atomic.Int32
		start := make(chan struct{})
		ownerStarted := make(chan struct{})
		var once sync.Once

		launch := func(ctx context.Context, r StartAgentRequest) (LaunchOutcomeRecord, error) {
			once.Do(func() { close(ownerStarted) })
			// Hold the in-flight window open so every duplicate arrives
			// while the owner is still running.
			time.Sleep(30 * time.Millisecond)
			launches.Add(1)
			return LaunchOutcomeRecord{RequestID: r.RequestID, Launched: true, AgentKey: "pane-1"}, nil
		}

		var wg sync.WaitGroup
		outcomes := make([]LaunchOutcomeRecord, callers)
		errs := make([]error, callers)
		for i := 0; i < callers; i++ {
			wg.Add(1)
			go func(i int) {
				defer wg.Done()
				<-start
				outcomes[i], errs[i] = registry.Submit(context.Background(), launchRequest("req-1"), launch)
			}(i)
		}
		close(start)
		wg.Wait()

		if launches.Load() != 1 {
			t.Fatalf("round %d: launch callback ran %d times", round, launches.Load())
		}
		for i := range outcomes {
			if errs[i] != nil {
				t.Fatalf("round %d caller %d: %v", round, i, errs[i])
			}
			if outcomes[i] != (LaunchOutcomeRecord{RequestID: "req-1", Launched: true, AgentKey: "pane-1"}) {
				t.Fatalf("round %d caller %d outcome = %+v", round, i, outcomes[i])
			}
		}
	}
}

// TestLaunchRegistryConflictRejectsDifferentBody pins the conflict contract
// both while a request is in flight and after it completed.
func TestLaunchRegistryConflictRejectsDifferentBody(t *testing.T) {
	registry := NewLaunchRegistry()
	release := make(chan struct{})
	ownerClaimed := make(chan struct{})
	var once sync.Once
	var launches atomic.Int32
	launch := func(ctx context.Context, r StartAgentRequest) (LaunchOutcomeRecord, error) {
		once.Do(func() { close(ownerClaimed) })
		launches.Add(1)
		<-release
		return LaunchOutcomeRecord{RequestID: r.RequestID, Launched: true}, nil
	}

	ownerDone := make(chan struct{})
	go func() {
		defer close(ownerDone)
		_, _ = registry.Submit(context.Background(), launchRequest("req-1"), launch)
	}()

	// Deterministically wait until the owner has claimed the RequestID.
	<-ownerClaimed

	conflicting := launchRequest("req-1")
	conflicting.CWD = "/work/other"
	if _, err := registry.Submit(context.Background(), conflicting, launch); err == nil {
		t.Fatal("in-flight conflicting body must be rejected")
	} else if _, ok := err.(*ConflictingRequestError); !ok {
		t.Fatalf("conflict error type = %T", err)
	}

	close(release)
	<-ownerDone
	if launches.Load() != 1 {
		t.Fatalf("launches = %d", launches.Load())
	}

	// After completion the conflict check still applies.
	if _, err := registry.Submit(context.Background(), conflicting, launch); err == nil {
		t.Fatal("completed conflicting body must be rejected")
	}
	if launches.Load() != 1 {
		t.Fatalf("conflict invoked launch: %d", launches.Load())
	}
}

// TestLaunchRegistryWaiterCancellationDoesNotRelaunch pins that a cancelled
// waiter neither cancels the owner nor causes a second launch, and that the
// completed result still replays afterwards.
func TestLaunchRegistryWaiterCancellationDoesNotRelaunch(t *testing.T) {
	registry := NewLaunchRegistry()
	release := make(chan struct{})
	ownerClaimed := make(chan struct{})
	var once sync.Once
	var launches atomic.Int32
	launch := func(ctx context.Context, r StartAgentRequest) (LaunchOutcomeRecord, error) {
		once.Do(func() { close(ownerClaimed) })
		launches.Add(1)
		<-release
		return LaunchOutcomeRecord{RequestID: r.RequestID, Launched: true, AgentKey: "pane-9"}, nil
	}

	ownerDone := make(chan struct{})
	go func() {
		defer close(ownerDone)
		_, _ = registry.Submit(context.Background(), launchRequest("req-1"), launch)
	}()

	// Deterministically wait until the owner has claimed the RequestID.
	<-ownerClaimed

	waiterCtx, cancelWaiter := context.WithCancel(context.Background())
	waiterDone := make(chan struct{})
	var waiterErr error
	go func() {
		defer close(waiterDone)
		_, waiterErr = registry.Submit(waiterCtx, launchRequest("req-1"), launch)
	}()

	// Give the waiter time to park on the in-flight call, then cancel it.
	time.Sleep(20 * time.Millisecond)
	cancelWaiter()
	<-waiterDone
	if !errors.Is(waiterErr, context.Canceled) {
		t.Fatalf("waiter err = %v, want context.Canceled", waiterErr)
	}

	close(release)
	<-ownerDone
	if launches.Load() != 1 {
		t.Fatalf("launches = %d after waiter cancellation", launches.Load())
	}

	replay, err := registry.Submit(context.Background(), launchRequest("req-1"), launch)
	if err != nil || replay.AgentKey != "pane-9" {
		t.Fatalf("replay = (%+v, %v)", replay, err)
	}
	if launches.Load() != 1 {
		t.Fatalf("replay invoked launch: %d", launches.Load())
	}
}

// TestLaunchRegistryOwnerErrorIsReplayed pins that a failed launch is
// recorded and replayed (not retried) for equivalent duplicates.
func TestLaunchRegistryOwnerErrorIsReplayed(t *testing.T) {
	registry := NewLaunchRegistry()
	var launches atomic.Int32
	launch := func(ctx context.Context, r StartAgentRequest) (LaunchOutcomeRecord, error) {
		launches.Add(1)
		return LaunchOutcomeRecord{}, errors.New("herdr unreachable")
	}

	first, err := registry.Submit(context.Background(), launchRequest("req-1"), launch)
	if err == nil || launches.Load() != 1 {
		t.Fatalf("first submit = (%+v, %v, %d launches)", first, err, launches.Load())
	}
	second, err := registry.Submit(context.Background(), launchRequest("req-1"), launch)
	if err == nil || launches.Load() != 1 {
		t.Fatalf("replay = (%+v, %v, %d launches)", second, err, launches.Load())
	}
}
