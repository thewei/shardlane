package agent

import (
	"fmt"
	"strings"

	"github.com/wh-studio/herdr-client/internal/history"
)

// Lightweight Agent usage projection (0.7 §17.6): model/token facts the
// History adapters already parsed, consumed by the quick-panel secondary
// line and the 0.8 usage-facts display. Unknown facts stay nil/empty and
// no second accounting model is created — the richer per-account
// ledger/aggregation stays the 0.8 domain ("Unknown stays None; never
// guessed").

// UsageSource names the provenance of one usage snapshot.
type UsageSource string

const (
	// UsageSourceNone: no provider-reported usage facts exist.
	UsageSourceNone UsageSource = "none"
	// UsageSourceHistory: facts parsed from the provider session metadata.
	UsageSourceHistory UsageSource = "history-session"
	// UsageSourceRuntime: facts reported by the runtime projection.
	UsageSourceRuntime UsageSource = "runtime-facts"
)

// QuotaSnapshot is a provider-reported allowance window. The shape is fixed
// by §17.6, but it is filled only from provable provider facts — the
// audited v1 found none, so it stays nil rather than invented.
type QuotaSnapshot struct {
	Label string
	// UsedPercent as the provider reported it (0..100).
	UsedPercent int
	// ResetsAtUnixMS is 0 when the provider gave no reset time.
	ResetsAtUnixMS int64
}

// AgentUsageSnapshot is the per-Agent usage projection shared by the tray
// quick-panel line and the 0.8 usage-facts display. CostUSD stays nil until
// a provider reports a provable cost fact; the plan fixes the field so the
// display contract is stable.
type AgentUsageSnapshot struct {
	AgentKey  AgentKey
	Model     string
	Provider  history.AgentID
	Tokens    *int64
	CostUSD   *float64
	Quota     *QuotaSnapshot
	Source    UsageSource
	Complete  bool
	UpdatedAt int64
}

// BuildAgentUsageSnapshot projects one Agent's usage from the exact History
// session metadata — metadata only, never a transcript parse.
func BuildAgentUsageSnapshot(key AgentKey, meta history.SessionMeta) AgentUsageSnapshot {
	snapshot := AgentUsageSnapshot{
		AgentKey:  key,
		Provider:  meta.Agent,
		Tokens:    meta.TokensUsed,
		UpdatedAt: meta.UpdatedAt,
	}
	if meta.Model != nil {
		snapshot.Model = *meta.Model
	}
	switch {
	case snapshot.Model != "" && snapshot.Tokens != nil:
		snapshot.Source = UsageSourceHistory
		snapshot.Complete = true
	case snapshot.Model != "" || snapshot.Tokens != nil:
		snapshot.Source = UsageSourceHistory
	default:
		snapshot.Source = UsageSourceNone
	}
	return snapshot
}

// WithQuota attaches provable provider allowance facts; a quota-capable
// provider's most relevant window replaces the token count in the rendered
// line (§17.2).
func (s AgentUsageSnapshot) WithQuota(quota QuotaSnapshot) AgentUsageSnapshot {
	s.Quota = &quota
	return s
}

// HasFacts reports whether any usage fact exists to render.
func (s AgentUsageSnapshot) HasFacts() bool {
	return s.Source != UsageSourceNone
}

// FormatUsageLine renders the §17.2 secondary line, e.g.
// "Sonnet · 38k tok · $0.21 est." — unknown segments are omitted and a
// snapshot without facts formats to "" so the row stays quiet. v1 has no
// provable account label, so no portal segment is invented.
func (s AgentUsageSnapshot) FormatUsageLine() string {
	segments := make([]string, 0, 3)
	if s.Model != "" {
		segments = append(segments, s.Model)
	}
	switch {
	case s.Quota != nil:
		segments = append(segments, fmt.Sprintf("%s %d%% used", s.Quota.Label, s.Quota.UsedPercent))
	case s.Tokens != nil && *s.Tokens > 0:
		segments = append(segments, fmt.Sprintf("%s tok", FormatTokenCount(*s.Tokens)))
	}
	if s.CostUSD != nil && *s.CostUSD > 0 {
		segments = append(segments, fmt.Sprintf("$%.2f est.", *s.CostUSD))
	}
	return strings.Join(segments, " · ")
}

// FormatTokenCount renders compact provider-reported totals: 950, 38k,
// 1.2M — never a raw full-width integer in a secondary line.
func FormatTokenCount(tokens int64) string {
	switch {
	case tokens >= 1_000_000:
		return compactCount(float64(tokens)/1_000_000) + "M"
	case tokens >= 1_000:
		return compactCount(float64(tokens)/1_000) + "k"
	default:
		return fmt.Sprintf("%d", tokens)
	}
}

// compactCount formats one decimal and drops a trailing ".0" (38.0 → 38).
func compactCount(value float64) string {
	text := fmt.Sprintf("%.1f", value)
	return strings.TrimSuffix(text, ".0")
}
