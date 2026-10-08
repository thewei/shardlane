package agent

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/wh-studio/herdr-client/internal/history"
)

// fakeUsageSessions is the deterministic catalog source.
type fakeUsageSessions struct {
	summaries []history.SessionSummary
	calls     int
	err       error
}

func (f *fakeUsageSessions) Recent(ctx context.Context, limit int) ([]history.SessionSummary, error) {
	f.calls++
	if f.err != nil {
		return nil, f.err
	}
	return f.summaries, nil
}

func usageSession(provider history.AgentID, id, file string, tokens int64, updatedMS, mtimeMS int64) history.SessionSummary {
	meta := history.SessionMeta{
		Agent:     provider,
		Key:       string(provider) + ":" + id,
		ID:        id,
		FilePath:  file,
		UpdatedAt: updatedMS,
		MtimeMS:   mtimeMS,
	}
	if tokens > 0 {
		tokensCopy := tokens
		meta.TokensUsed = &tokensCopy
	}
	return history.SessionSummary{Meta: meta}
}

func usageAggregator(t *testing.T, source *fakeUsageSessions, now time.Time) (*UsageAggregator, string) {
	t.Helper()
	cachePath := filepath.Join(t.TempDir(), "usage-cache.json")
	aggregator := NewUsageAggregator(cachePath, source, "")
	aggregator.Now = func() time.Time { return now }
	aggregator.Day = func(ms int64) string { return time.UnixMilli(ms).UTC().Format("2006-01-02") }
	return aggregator, cachePath
}

// TestUsageAggregatorGroupsByProviderAccountAndDay pins the 0.8 aggregate:
// provider-reported session tokens book to the provable account and the
// local day of last activity; zero-token and other-day sessions stay out.
func TestUsageAggregatorGroupsByProviderAccountAndDay(t *testing.T) {
	now := time.Date(2026, 10, 5, 12, 0, 0, 0, time.UTC)
	today := now.UnixMilli()
	yesterday := now.Add(-24 * time.Hour).UnixMilli()
	source := &fakeUsageSessions{summaries: []history.SessionSummary{
		usageSession(history.AgentCodex, "s1", "/codex/s1.jsonl", 1000, today, 500),
		usageSession(history.AgentCodex, "s2", "/codex/s2.jsonl", 500, today, 700),
		usageSession(history.AgentClaudeCode, "s3", "/claude/s3.jsonl", 300, today, 900),
		usageSession(history.AgentCodex, "s4", "/codex/s4.jsonl", 9999, yesterday, 400),
		usageSession(history.AgentCodex, "s5", "/codex/s5.jsonl", 0, today, 300),
	}}
	aggregator, _ := usageAggregator(t, source, now)

	snapshot := aggregator.RefreshAt(context.Background())
	if snapshot == nil || snapshot.Day != "2026-10-05" {
		t.Fatalf("snapshot = %+v", snapshot)
	}
	if len(snapshot.Accounts) != 2 {
		t.Fatalf("accounts = %+v", snapshot.Accounts)
	}
	// Codex first (stable insertion order), Claude second.
	if snapshot.Accounts[0].Provider != history.AgentCodex || snapshot.Accounts[0].TokensUsed != 1500 {
		t.Fatalf("codex account = %+v", snapshot.Accounts[0])
	}
	// v1: no provable Codex account in the test home → masked display name.
	if snapshot.Accounts[0].AccountID != nil || snapshot.Accounts[0].LabelMasked != "Codex" {
		t.Fatalf("codex label = %+v", snapshot.Accounts[0])
	}
	if snapshot.Accounts[1].Provider != history.AgentClaudeCode || snapshot.Accounts[1].TokensUsed != 300 {
		t.Fatalf("claude account = %+v", snapshot.Accounts[1])
	}
	if snapshot.TokensToday() != 1800 {
		t.Fatalf("tokens today = %d", snapshot.TokensToday())
	}
	// Invalidation fingerprint: max mtime + source count (all 5 files).
	if snapshot.MaxMtimeMS == nil || *snapshot.MaxMtimeMS != 900 || snapshot.SourceFiles != 5 {
		t.Fatalf("fingerprint = %+v", snapshot)
	}
	if got := snapshot.FormatTokensToday(); got != "1.8k tok today" {
		t.Fatalf("format = %q", got)
	}
}

// TestUsageAggregatorThrottleAndInvalidation pins the cache discipline:
// within the throttle window the cached snapshot returns without a recompute;
// an unchanged fingerprint keeps the cached snapshot across a manual refresh;
// a changed source fingerprint (new file → new count/mtime) recomputes.
func TestUsageAggregatorThrottleAndInvalidation(t *testing.T) {
	now := time.Date(2026, 10, 5, 12, 0, 0, 0, time.UTC)
	source := &fakeUsageSessions{summaries: []history.SessionSummary{
		usageSession(history.AgentCodex, "s1", "/codex/s1.jsonl", 1000, now.UnixMilli(), 500),
	}}
	aggregator, _ := usageAggregator(t, source, now)

	first := aggregator.RefreshAt(context.Background())
	if source.calls != 1 {
		t.Fatalf("calls = %d", source.calls)
	}

	// Within the throttle: cached, no recompute.
	if cached := aggregator.RefreshIfDue(context.Background()); cached != first || source.calls != 1 {
		t.Fatalf("throttled refresh recomputed: calls = %d", source.calls)
	}

	// Manual refresh with an unchanged fingerprint: cache survives, no
	// persist churn (source consulted, snapshot kept).
	aggregator.Now = func() time.Time { return now.Add(2 * time.Minute) }
	kept := aggregator.RefreshAt(context.Background())
	if kept != first || source.calls != 2 {
		t.Fatalf("unchanged fingerprint recomputed: calls = %d", source.calls)
	}

	// A new source file invalidates: recompute with the new fingerprint.
	source.summaries = append(source.summaries,
		usageSession(history.AgentClaudeCode, "s3", "/claude/s3.jsonl", 300, now.UnixMilli(), 800))
	changed := aggregator.RefreshAt(context.Background())
	if changed == first || changed.TokensToday() != 1300 || *changed.MaxMtimeMS != 800 {
		t.Fatalf("changed snapshot = %+v", changed)
	}
	if changed.Accounts[0].LabelMasked != "Codex" || changed.Accounts[1].LabelMasked != "Claude Code" {
		t.Fatalf("labels = %+v", changed.Accounts)
	}
}

// TestUsageAggregatorCachePersistence pins the JSON cache: a fresh
// aggregator loads the persisted snapshot without touching the source.
func TestUsageAggregatorCachePersistence(t *testing.T) {
	now := time.Date(2026, 10, 5, 12, 0, 0, 0, time.UTC)
	source := &fakeUsageSessions{summaries: []history.SessionSummary{
		usageSession(history.AgentCodex, "s1", "/codex/s1.jsonl", 1000, now.UnixMilli(), 500),
	}}
	aggregator, cachePath := usageAggregator(t, source, now)
	first := aggregator.RefreshAt(context.Background())

	reborn := NewUsageAggregator(cachePath, &fakeUsageSessions{}, "")
	reborn.Now = aggregator.Now
	reborn.Day = aggregator.Day
	reborn.LoadCache()
	if cached := reborn.Cached(); cached == nil || cached.TokensToday() != first.TokensToday() {
		t.Fatalf("reloaded cache = %+v", cached)
	}
}

// TestUsageAggregatorSourceErrorKeepsCache pins the failure mode: a source
// error returns the cached snapshot (never an empty fact set).
func TestUsageAggregatorSourceErrorKeepsCache(t *testing.T) {
	now := time.Date(2026, 10, 5, 12, 0, 0, 0, time.UTC)
	source := &fakeUsageSessions{summaries: []history.SessionSummary{
		usageSession(history.AgentCodex, "s1", "/codex/s1.jsonl", 1000, now.UnixMilli(), 500),
	}}
	aggregator, _ := usageAggregator(t, source, now)
	first := aggregator.RefreshAt(context.Background())

	source.err = context.DeadlineExceeded
	if cached := aggregator.RefreshAt(context.Background()); cached != first {
		t.Fatalf("source error replaced the cache: %+v", cached)
	}
}

// TestMaskAccountLabel pins the masking: four characters of a provable id,
// provider display name otherwise, raw ids never exposed.
func TestMaskAccountLabel(t *testing.T) {
	long := "abcdefgh"
	cases := []struct {
		account *string
		want    string
	}{
		{nil, "Codex"},
		{strPtrUsage(""), "Codex"},
		{strPtrUsage("   "), "Codex"},
		{strPtrUsage("abc"), "codex:abc…"},
		{strPtrUsage(long), "codex:abcd…"},
	}
	for _, tc := range cases {
		if got := MaskAccountLabel(history.AgentCodex, tc.account); got != tc.want {
			t.Fatalf("MaskAccountLabel(%v) = %q, want %q", tc.account, got, tc.want)
		}
	}
}

func strPtrUsage(value string) *string { return &value }

// TestCodexAccountIDReadsAuthJSON pins the provable identity source:
// account_id from CODEX_HOME/auth.json; missing/corrupt files stay nil.
func TestCodexAccountIDReadsAuthJSON(t *testing.T) {
	home := t.TempDir()
	codexHome := filepath.Join(home, ".codex")
	if err := os.MkdirAll(codexHome, 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("CODEX_HOME", codexHome)

	if codexAccountID(home) != nil {
		t.Fatal("missing auth.json must stay nil")
	}
	if err := os.WriteFile(filepath.Join(codexHome, "auth.json"), []byte("{not json"), 0o600); err != nil {
		t.Fatal(err)
	}
	if codexAccountID(home) != nil {
		t.Fatal("corrupt auth.json must stay nil")
	}
	auth := `{"tokens":{"account_id":"acct-1234"}}`
	if err := os.WriteFile(filepath.Join(codexHome, "auth.json"), []byte(auth), 0o600); err != nil {
		t.Fatal(err)
	}
	account := codexAccountID(home)
	if account == nil || *account != "acct-1234" {
		t.Fatalf("account = %#v", account)
	}
	if !json.Valid([]byte(auth)) {
		t.Fatal("fixture drifted")
	}
}
