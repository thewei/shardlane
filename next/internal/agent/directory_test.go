package agent

import (
	"reflect"
	"testing"
	"time"

	"github.com/wh-studio/herdr-client/next/internal/history"
)

func cardForTest(key, title, status string, attention OperationalState, unread, review bool) AgentCardModel {
	return ProjectAgentCard(AgentCardInputs{
		InstanceID: "default", TerminalID: key,
		Provider: history.AgentCodex, Title: title, AgentStatus: status,
		Attention: attention, Markers: AgentMarkers{Unread: unread, ReviewPending: review},
	})
}

func TestAgentDirectoryActionableFirstOrder(t *testing.T) {
	directory := NewAgentDirectory()
	directory.Replace([]AgentCardModel{
		cardForTest("idle-1", "Zulu idle", "idle", OpIdle, false, false),
		cardForTest("working-1", "Alpha working", "working", OpWorking, false, false),
		cardForTest("review-1", "Mike review", "done", OpReadyForReview, false, true),
		cardForTest("blocked-1", "Bravo blocked", "blocked", OpNeedsAttention, false, false),
		cardForTest("working-2", "Yankee working unread", "working", OpWorking, true, false),
	})

	cards := directory.All()
	if len(cards) != 5 {
		t.Fatalf("cards = %d", len(cards))
	}
	if BucketOf(cards[0]) != BucketNeedsAttention || cards[0].Title != "Bravo blocked" {
		t.Fatalf("first card = %q (%v)", cards[0].Title, BucketOf(cards[0]))
	}
	if BucketOf(cards[1]) != BucketReadyForReview || cards[1].Title != "Mike review" {
		t.Fatalf("second card = %q (%v)", cards[1].Title, BucketOf(cards[1]))
	}
	// Within Working: unread first, then alphabetical.
	if cards[2].Title != "Yankee working unread" || cards[3].Title != "Alpha working" {
		t.Fatalf("working order = %q, %q", cards[2].Title, cards[3].Title)
	}
	if cards[4].Title != "Zulu idle" {
		t.Fatalf("last card = %q", cards[4].Title)
	}
}

func TestAgentDirectorySummaryAndFilter(t *testing.T) {
	directory := NewAgentDirectory()
	directory.Replace([]AgentCardModel{
		cardForTest("b1", "Blocked", "blocked", OpNeedsAttention, false, false),
		cardForTest("r1", "Review", "done", OpReadyForReview, false, true),
		cardForTest("w1", "Working", "working", OpWorking, false, false),
		cardForTest("i1", "Idle", "idle", OpIdle, false, false),
	})

	summary := directory.Summary()
	if summary.NeedsAttention != 1 || summary.ReviewPending != 1 || summary.Working != 1 || summary.Total != 4 {
		t.Fatalf("summary = %+v", summary)
	}

	attention := directory.Filter(BucketNeedsAttention)
	if len(attention) != 1 || attention[0].Title != "Blocked" {
		t.Fatalf("attention filter = %+v", attention)
	}
	if got := directory.Filter(BucketIdle); len(got) != 1 || got[0].Title != "Idle" {
		t.Fatalf("idle filter = %+v", got)
	}
}

func TestAgentDirectoryGet(t *testing.T) {
	key := AgentKey{InstanceID: "default", TerminalID: "term-1"}
	directory := NewAgentDirectory()
	directory.Replace([]AgentCardModel{cardForTest("term-1", "Found", "idle", OpIdle, false, false)})

	card, ok := directory.Get(key)
	if !ok || card.Title != "Found" {
		t.Fatalf("get = (%+v, %v)", card, ok)
	}
	if _, ok := directory.Get(AgentKey{InstanceID: "other", TerminalID: "term-1"}); ok {
		t.Fatal("instance-scoped key must not match another instance")
	}
}

// TestAgentDirectoryWorkingHysteresis pins the §9 sort hysteresis: a card
// inside its ActiveUntil window keeps the Working position even though its
// honest bucket (and status pill) is Idle, and drops back once it expires.
func TestAgentDirectoryWorkingHysteresis(t *testing.T) {
	now := time.Unix(1000, 0)
	directory := NewAgentDirectory()
	directory.now = func() time.Time { return now }

	working := cardForTest("w1", "Alpha working", "working", OpWorking, false, false)
	resting := cardForTest("i1", "Zulu idle", "idle", OpIdle, false, false)
	loopGap := cardForTest("i2", "Mid idle", "idle", OpIdle, false, false)
	loopGap.ActiveUntil = now.Add(WorkingHysteresis) // anchor refreshed by the last working loop

	directory.Replace([]AgentCardModel{resting, loopGap, working})
	cards := directory.All()
	if cards[0].Title != "Alpha working" || cards[1].Title != "Mid idle" {
		t.Fatalf("recently active card must keep the working slot: %q, %q", cards[0].Title, cards[1].Title)
	}

	// After the window the same card falls back to the honest idle group.
	now = now.Add(WorkingHysteresis + time.Second)
	cards = directory.All()
	if cards[0].Title != "Alpha working" {
		t.Fatalf("working card must lead: %q", cards[0].Title)
	}
	if cards[1].Title != "Mid idle" || cards[2].Title != "Zulu idle" {
		t.Fatalf("expired card must rejoin idle (alphabetical): %q, %q", cards[1].Title, cards[2].Title)
	}
	// The honest bucket never changed.
	if BucketOf(loopGap) != BucketIdle {
		t.Fatalf("hysteresis must not change the honest bucket: %v", BucketOf(loopGap))
	}
}

// TestAgentDirectoryDeterministicTies pins the stable tail: equal-ranked
// cards must order by identity, never by Go's randomized map iteration
// order, or the list reshuffles on every reconciled snapshot.
func TestAgentDirectoryDeterministicTies(t *testing.T) {
	directory := NewAgentDirectory()
	want := []string{"term-1", "term-2", "term-3", "term-4", "term-5"}
	for attempt := 0; attempt < 25; attempt++ {
		directory.Replace([]AgentCardModel{
			cardForTest("term-5", "Same", "idle", OpIdle, false, false),
			cardForTest("term-1", "Same", "idle", OpIdle, false, false),
			cardForTest("term-4", "Same", "idle", OpIdle, false, false),
			cardForTest("term-2", "Same", "idle", OpIdle, false, false),
			cardForTest("term-3", "Same", "idle", OpIdle, false, false),
		})
		var got []string
		for _, card := range directory.All() {
			got = append(got, card.Key.TerminalID)
		}
		if !reflect.DeepEqual(got, want) {
			t.Fatalf("attempt %d: order = %v, want %v", attempt, got, want)
		}
	}
}
