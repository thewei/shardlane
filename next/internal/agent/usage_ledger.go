package agent

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"sync"
	"time"

	"github.com/wh-studio/herdr-client/next/internal/history"
)

// Per-account usage aggregation (0.8, porting the audited Host
// agent_usage.rs facts layer): provider session files' cumulative token
// facts (already parsed by the History adapters) booked to the local day of
// the session's last activity. One row per (provider, provable local
// account, local day); provider-reported allowance windows fill in only
// from provable sources — the audited v1 found none, so windows stay empty
// rather than invented ("Unknown stays None; never guessed").

// UsageWindow is a provider-reported allowance window; the label is the
// provider's own (e.g. "5h" / "7d"), never invented locally.
type UsageWindow struct {
	Label          string `json:"label"`
	UsedPercent    int    `json:"used_percent"`
	ResetsAtUnixMS *int64 `json:"resets_at_unix_ms,omitempty"`
}

// AccountUsage is one (provider, provable account, local day) aggregate.
type AccountUsage struct {
	Provider    history.AgentID `json:"provider"`
	AccountID   *string         `json:"account_id,omitempty"`
	LabelMasked string          `json:"label_masked"`
	// Day is the local day (YYYY-MM-DD) the aggregate belongs to.
	Day        string `json:"day"`
	TokensUsed int64  `json:"tokens_used"`
	// Windows is empty when the provider's local state exposes no provable
	// allowance window.
	Windows []UsageWindow `json:"windows"`
}

// UsageSnapshot is the complete fact set of one aggregation round.
type UsageSnapshot struct {
	Day           string         `json:"day"`
	Accounts      []AccountUsage `json:"accounts"`
	GeneratedAtMS int64          `json:"generated_at_ms"`
	// MaxMtimeMS and SourceFiles are the invalidation fingerprint of the
	// observed provider sources.
	MaxMtimeMS  *int64 `json:"max_mtime_ms,omitempty"`
	SourceFiles int64  `json:"source_files"`
}

// TokensToday sums the provider-reported token totals across accounts.
func (s UsageSnapshot) TokensToday() int64 {
	total := int64(0)
	for _, account := range s.Accounts {
		total += account.TokensUsed
	}
	return total
}

// MaskAccountLabel builds the display label: provider name plus the first
// four characters of a provable account id; the raw account id never
// reaches logs or UI.
func MaskAccountLabel(provider history.AgentID, accountID *string) string {
	if accountID != nil {
		if trimmed := strings.TrimSpace(*accountID); trimmed != "" {
			keep := trimmed
			if len(keep) > 4 {
				keep = keep[:4]
			}
			return string(provider) + ":" + keep + "…"
		}
	}
	return provider.DisplayName()
}

// codexAccountID reads the provable local account identity from the Codex
// auth file (CODEX_HOME/auth.json, tokens.account_id). Read-only; nil when
// nothing is provable — never guessed.
func codexAccountID(home string) *string {
	codexHome := os.Getenv("CODEX_HOME")
	if codexHome == "" {
		if home == "" {
			return nil
		}
		codexHome = filepath.Join(home, ".codex")
	}
	data, err := os.ReadFile(filepath.Join(codexHome, "auth.json"))
	if err != nil {
		return nil
	}
	var auth struct {
		Tokens *struct {
			AccountID *string `json:"account_id"`
		} `json:"tokens"`
	}
	if json.Unmarshal(data, &auth) != nil || auth.Tokens == nil {
		return nil
	}
	return auth.Tokens.AccountID
}

// UsageSessionsSource supplies catalog metadata for aggregation (metadata
// only — the aggregator never parses transcripts).
type UsageSessionsSource interface {
	Recent(ctx context.Context, limit int) ([]history.SessionSummary, error)
}

// UsageAggregator is the cached facts layer: a JSON snapshot cache with
// mtime invalidation and a one-minute throttle. Zero IO on Cached.
type UsageAggregator struct {
	cachePath string
	sessions  UsageSessionsSource
	home      string

	mu          sync.Mutex
	cache       *UsageSnapshot
	lastRefresh time.Time

	// Injectable clock/day for deterministic tests; nil uses time.Now.
	Now func() time.Time
	Day func(ms int64) string
}

// UsageThrottle is the refresh throttle (audited default: one minute).
const UsageThrottle = time.Minute

// NewUsageAggregator builds the aggregator over the catalog source with the
// JSON cache at cachePath.
func NewUsageAggregator(cachePath string, sessions UsageSessionsSource, home string) *UsageAggregator {
	return &UsageAggregator{cachePath: cachePath, sessions: sessions, home: home}
}

func (a *UsageAggregator) now() time.Time {
	if a.Now != nil {
		return a.Now()
	}
	return time.Now()
}

func (a *UsageAggregator) day(ms int64) string {
	if a.Day != nil {
		return a.Day(ms)
	}
	return time.Now().Format("2006-01-02")
}

// Cached returns the current snapshot without any IO.
func (a *UsageAggregator) Cached() *UsageSnapshot {
	a.mu.Lock()
	defer a.mu.Unlock()
	return a.cache
}

// LoadCache reads the persisted snapshot (best effort; a corrupt cache is
// discarded).
func (a *UsageAggregator) LoadCache() {
	a.mu.Lock()
	defer a.mu.Unlock()
	data, err := os.ReadFile(a.cachePath)
	if err != nil {
		return
	}
	var snapshot UsageSnapshot
	if json.Unmarshal(data, &snapshot) != nil {
		return
	}
	a.cache = &snapshot
}

func (a *UsageAggregator) persist(snapshot UsageSnapshot) {
	if a.cachePath == "" {
		return
	}
	if err := os.MkdirAll(filepath.Dir(a.cachePath), 0o755); err != nil {
		return
	}
	data, err := json.Marshal(snapshot)
	if err != nil {
		return
	}
	_ = os.WriteFile(a.cachePath, data, 0o600)
}

// RefreshIfDue recomputes at most once per throttle window; within the
// window it returns the cached snapshot untouched.
func (a *UsageAggregator) RefreshIfDue(ctx context.Context) *UsageSnapshot {
	a.mu.Lock()
	if a.cache != nil && a.now().Sub(a.lastRefresh) < UsageThrottle {
		cached := a.cache
		a.mu.Unlock()
		return cached
	}
	a.mu.Unlock()
	return a.RefreshAt(ctx)
}

// RefreshAt recomputes unconditionally (manual refresh path). A recompute
// whose invalidation fingerprint did not change keeps the cached snapshot.
func (a *UsageAggregator) RefreshAt(ctx context.Context) *UsageSnapshot {
	sessions, err := a.sessions.Recent(ctx, 500)
	if err != nil {
		return a.Cached()
	}
	today := a.day(a.now().UnixMilli())

	var maxMtime int64
	var sourceFiles int64
	byAccount := map[string]*AccountUsage{}
	var order []string
	for _, summary := range sessions {
		meta := summary.Meta
		if meta.MtimeMS > maxMtime {
			maxMtime = meta.MtimeMS
		}
		sourceFiles++
		if meta.TokensUsed == nil || *meta.TokensUsed <= 0 {
			continue
		}
		if a.day(meta.UpdatedAt) != today {
			continue
		}
		var accountID *string
		if meta.Agent == history.AgentCodex {
			accountID = codexAccountID(a.home)
		}
		key := string(meta.Agent) + "\x00" + ptrValue(accountID)
		account, ok := byAccount[key]
		if !ok {
			account = &AccountUsage{
				Provider:    meta.Agent,
				AccountID:   accountID,
				LabelMasked: MaskAccountLabel(meta.Agent, accountID),
				Day:         today,
				Windows:     []UsageWindow{},
			}
			byAccount[key] = account
			order = append(order, key)
		}
		account.TokensUsed += *meta.TokensUsed
	}

	next := UsageSnapshot{
		Day:           today,
		GeneratedAtMS: a.now().UnixMilli(),
		SourceFiles:   sourceFiles,
	}
	if maxMtime > 0 {
		next.MaxMtimeMS = &maxMtime
	}
	for _, key := range order {
		next.Accounts = append(next.Accounts, *byAccount[key])
	}

	a.mu.Lock()
	defer a.mu.Unlock()
	if a.cache != nil && a.cache.invalidatesBy(next) {
		a.lastRefresh = a.now()
		return a.cache
	}
	a.cache = &next
	a.lastRefresh = a.now()
	a.persist(next)
	return a.cache
}

// invalidatesBy reports whether the cached snapshot is still valid against
// a recomputed fingerprint (same day, same accounts, same source
// fingerprint): recomputation is skipped only then.
func (s *UsageSnapshot) invalidatesBy(next UsageSnapshot) bool {
	if s.Day != next.Day || s.SourceFiles != next.SourceFiles {
		return false
	}
	if (s.MaxMtimeMS == nil) != (next.MaxMtimeMS == nil) {
		return false
	}
	if s.MaxMtimeMS != nil && *s.MaxMtimeMS != *next.MaxMtimeMS {
		return false
	}
	if len(s.Accounts) != len(next.Accounts) {
		return false
	}
	for i := range next.Accounts {
		if !reflect.DeepEqual(s.Accounts[i], next.Accounts[i]) {
			return false
		}
	}
	return true
}

func ptrValue(value *string) string {
	if value == nil {
		return ""
	}
	return *value
}

// FormatTokensToday renders the aggregator's daily total for display.
func (s UsageSnapshot) FormatTokensToday() string {
	return fmt.Sprintf("%s tok today", FormatTokenCount(s.TokensToday()))
}
