package nativeui

import (
	"context"
	"time"

	"github.com/wh-studio/herdr-client/next/internal/agent"
	"github.com/wh-studio/herdr-client/next/internal/history"
)

// §17.2 usage secondary line: the quick-panel/status rows render a cached
// History-meta usage projection ("Sonnet · 38k tok · $0.21 est.") when the
// bound session metadata carries model/token facts. The refresh runs on the
// dispatch lane against the read-only catalog; the render only reads the
// resulting map — no render-time IO.

// usageProjectionThrottle mirrors the 0.8 aggregator's one-minute throttle.
const usageProjectionThrottle = time.Minute

// usageLineFor reads the cached per-Agent usage line (pure map read).
func (s *Shell) usageLineFor(key agent.AgentKey) string {
	if s.usageByAgent == nil {
		return ""
	}
	return s.usageByAgent[key].FormatUsageLine()
}

// refreshUsageProjection reloads the cached History-meta usage projection
// for the live agents: exact session identities (id/path kinds) match
// catalog metadata, and the matched SessionMeta feeds the §17.6 snapshot.
// Throttled; force bypasses (e.g. the quick panel opening).
func (s *Shell) refreshUsageProjection(force bool) {
	service := s.hist.service
	if service == nil {
		return
	}
	if !force && time.Since(s.usageRefreshedAt) < usageProjectionThrottle {
		return
	}
	s.usageRefreshedAt = time.Now()

	// Capture the live identities on the UI lane; the background lane only
	// reads the catalog and these captured facts.
	type usageTarget struct {
		key      agent.AgentKey
		provider history.AgentID
		kind     string
		value    string
	}
	cards := s.workbenchCards()
	targets := make([]usageTarget, 0, len(cards))
	for _, runtimeAgent := range s.projection.Agents {
		if runtimeAgent.AgentSession == nil {
			continue
		}
		provider, ok := history.ParseAgentID(runtimeAgent.AgentSession.Agent)
		if !ok {
			continue
		}
		for _, card := range cards {
			if card.Key.TerminalID == runtimeAgent.TerminalID {
				targets = append(targets, usageTarget{
					key:      card.Key,
					provider: provider,
					kind:     runtimeAgent.AgentSession.Kind,
					value:    runtimeAgent.AgentSession.Value,
				})
				break
			}
		}
	}
	if len(targets) == 0 {
		return
	}

	s.dispatch(func() {
		summaries, err := service.Recent(context.Background(), 100)
		if err != nil {
			return
		}
		snapshots := make(map[agent.AgentKey]agent.AgentUsageSnapshot, len(targets))
		for _, target := range targets {
			for _, summary := range summaries {
				if usageIdentityMatches(target.provider, target.kind, target.value, summary.Meta) {
					snapshots[target.key] = agent.BuildAgentUsageSnapshot(target.key, summary.Meta)
					break
				}
			}
		}
		s.applyOnWindow(func() {
			s.usageByAgent = snapshots
		})
	})
}

// usageIdentityMatches one typed session identity against a catalog
// metadata row: same provider, and the declared kind's value equal to the
// native id ("id") or the exact source path ("path"). Never cwd/mtime.
func usageIdentityMatches(provider history.AgentID, kind, value string, meta history.SessionMeta) bool {
	if meta.Agent != provider || value == "" {
		return false
	}
	switch kind {
	case "id":
		return value == meta.ID
	case "path":
		return value == meta.FilePath
	default:
		return false
	}
}
