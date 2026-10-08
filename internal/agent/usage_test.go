package agent

import (
	"testing"

	"github.com/wh-studio/herdr-client/internal/history"
)

func usageMeta(model *string, tokens *int64) history.SessionMeta {
	return history.SessionMeta{
		Agent:      history.AgentClaudeCode,
		Key:        "claude-code:session-1",
		ID:         "session-1",
		Model:      model,
		TokensUsed: tokens,
		UpdatedAt:  1_760_000_000_000,
	}
}

func strPtr(value string) *string { return &value }
func i64Ptr(value int64) *int64   { return &value }

// TestBuildAgentUsageSnapshotFromHistoryMeta pins the §17.6 projection: the
// History adapter's parsed metadata is the only source; completeness is
// claimed only when both model and token facts exist; a fact-less session
// stays Source none.
func TestBuildAgentUsageSnapshotFromHistoryMeta(t *testing.T) {
	key := AgentKey{InstanceID: "inst", TerminalID: "term-1"}

	full := BuildAgentUsageSnapshot(key, usageMeta(strPtr("claude-sonnet-4-5"), i64Ptr(38000)))
	if !full.Complete || full.Source != UsageSourceHistory || !full.HasFacts() {
		t.Fatalf("full snapshot = %+v", full)
	}
	if full.Model != "claude-sonnet-4-5" || *full.Tokens != 38000 || full.Provider != history.AgentClaudeCode {
		t.Fatalf("full fields = %+v", full)
	}
	if full.CostUSD != nil || full.Quota != nil {
		t.Fatalf("unprovable facts must stay nil: %+v", full)
	}

	partial := BuildAgentUsageSnapshot(key, usageMeta(strPtr("claude-sonnet-4-5"), nil))
	if partial.Complete || partial.Source != UsageSourceHistory || !partial.HasFacts() {
		t.Fatalf("model-only snapshot = %+v", partial)
	}

	partial = BuildAgentUsageSnapshot(key, usageMeta(nil, i64Ptr(1200)))
	if partial.Complete || partial.Source != UsageSourceHistory || partial.Model != "" {
		t.Fatalf("tokens-only snapshot = %+v", partial)
	}

	empty := BuildAgentUsageSnapshot(key, usageMeta(nil, nil))
	if empty.HasFacts() || empty.Source != UsageSourceNone || empty.Complete {
		t.Fatalf("fact-less snapshot = %+v", empty)
	}
}

// TestFormatUsageLine pins the §17.2 secondary line: unknown segments are
// omitted, quota-capable providers show the allowance window instead of the
// token count, and a fact-less snapshot renders quiet.
func TestFormatUsageLine(t *testing.T) {
	cases := []struct {
		name     string
		snapshot AgentUsageSnapshot
		want     string
	}{
		{
			name: "full",
			snapshot: AgentUsageSnapshot{
				Model: "Sonnet", Tokens: i64Ptr(38000), CostUSD: floatPtr(0.21),
			},
			want: "Sonnet · 38k tok · $0.21 est.",
		},
		{
			name:     "model only",
			snapshot: AgentUsageSnapshot{Model: "Sonnet"},
			want:     "Sonnet",
		},
		{
			name:     "tokens only",
			snapshot: AgentUsageSnapshot{Tokens: i64Ptr(950)},
			want:     "950 tok",
		},
		{
			name:     "quota replaces tokens",
			snapshot: AgentUsageSnapshot{Model: "Sonnet", Tokens: i64Ptr(38000)}.WithQuota(QuotaSnapshot{Label: "5h", UsedPercent: 42}),
			want:     "Sonnet · 5h 42% used",
		},
		{
			name:     "zero tokens omitted",
			snapshot: AgentUsageSnapshot{Model: "Sonnet", Tokens: i64Ptr(0)},
			want:     "Sonnet",
		},
		{
			name:     "quiet",
			snapshot: AgentUsageSnapshot{},
			want:     "",
		},
	}
	for _, tc := range cases {
		if got := tc.snapshot.FormatUsageLine(); got != tc.want {
			t.Fatalf("%s: FormatUsageLine = %q, want %q", tc.name, got, tc.want)
		}
	}
}

func floatPtr(value float64) *float64 { return &value }

// TestFormatTokenCount pins the compact totals: never a raw full-width
// integer in a secondary line.
func TestFormatTokenCount(t *testing.T) {
	cases := []struct {
		tokens int64
		want   string
	}{
		{0, "0"},
		{950, "950"},
		{1000, "1k"},
		{38000, "38k"},
		{38_500, "38.5k"},
		{1_250_000, "1.2M"},
		{15_000_000, "15M"},
	}
	for _, tc := range cases {
		if got := FormatTokenCount(tc.tokens); got != tc.want {
			t.Fatalf("FormatTokenCount(%d) = %q, want %q", tc.tokens, got, tc.want)
		}
	}
}
