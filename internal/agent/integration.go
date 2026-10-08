package agent

import "github.com/wh-studio/herdr-client/internal/history"

// IntegrationStrategy is the runtime integration strategy authority (INT-01),
// ported from the audited Rust agent-integrations registry: Shardlane never
// installs a competing lifecycle hook beside Herdr official integration.
type IntegrationStrategy string

const (
	// StrategyHerdrOfficial: Herdr's official integration is the sole runtime
	// authority; Shardlane only keeps `herdr integration install <target>`
	// current.
	StrategyHerdrOfficial IntegrationStrategy = "herdr-official"
	// StrategyHerdrScreenOnly: lifecycle belongs to the Herdr screen manifest.
	StrategyHerdrScreenOnly IntegrationStrategy = "herdr-screen-only"
	// StrategyHerdrScreenWithManagedSessionBridge: Herdr screen lifecycle plus
	// a provider-native session identity bridge only.
	StrategyHerdrScreenWithManagedSessionBridge IntegrationStrategy = "herdr-screen-session-bridge"
	// StrategyManagedLifecycleBridge: lifecycle stays with Herdr screen;
	// Shardlane adds a provider-native session bridge only.
	StrategyManagedLifecycleBridge IntegrationStrategy = "managed-lifecycle-bridge"
	// StrategyDeferred: explicitly not claimed yet — never a degraded fallback.
	StrategyDeferred IntegrationStrategy = "deferred"
)

// IntegrationEntry is one provider's strategy and Herdr install target.
type IntegrationEntry struct {
	Target   string
	Strategy IntegrationStrategy
}

// integrationRegistry is the strategy authority: every History AgentID has
// exactly one explicit strategy.
var integrationRegistry = map[history.AgentID]IntegrationEntry{
	history.AgentClaudeCode:  {Target: "claude", Strategy: StrategyHerdrOfficial},
	history.AgentCodex:       {Target: "codex", Strategy: StrategyHerdrOfficial},
	history.AgentCopilot:     {Target: "copilot", Strategy: StrategyHerdrOfficial},
	history.AgentCursor:      {Target: "cursor", Strategy: StrategyHerdrOfficial},
	history.AgentOpenCode:    {Target: "opencode", Strategy: StrategyHerdrOfficial},
	history.AgentCommandCode: {Target: "commandcode", Strategy: StrategyManagedLifecycleBridge},
	history.AgentKiro:        {Target: "kiro", Strategy: StrategyHerdrScreenWithManagedSessionBridge},
	history.AgentGemini:      {Target: "gemini", Strategy: StrategyHerdrScreenWithManagedSessionBridge},
	history.AgentPi:          {Target: "pi", Strategy: StrategyHerdrOfficial},
	history.AgentOMP:         {Target: "omp", Strategy: StrategyHerdrOfficial},
	history.AgentGrok:        {Target: "grok", Strategy: StrategyHerdrOfficial},
	history.AgentKimi:        {Target: "kimi", Strategy: StrategyHerdrOfficial},
	history.AgentAntigravity: {Target: "antigravity-cli", Strategy: StrategyHerdrOfficial},
	history.AgentDSH:         {Target: "dsh", Strategy: StrategyManagedLifecycleBridge},
	history.AgentQoder:       {Target: "qoder", Strategy: StrategyDeferred},
}

// IntegrationFor returns the explicit strategy entry for one provider.
// Unknown providers fail closed as deferred rather than being guessed.
func IntegrationFor(provider history.AgentID) IntegrationEntry {
	if entry, ok := integrationRegistry[provider]; ok {
		return entry
	}
	return IntegrationEntry{Strategy: StrategyDeferred}
}

// StrategyLabel is the shared display text for one strategy.
func StrategyLabel(strategy IntegrationStrategy) string {
	switch strategy {
	case StrategyHerdrOfficial:
		return "Herdr official"
	case StrategyHerdrScreenOnly:
		return "Herdr screen"
	case StrategyHerdrScreenWithManagedSessionBridge:
		return "Herdr screen + session bridge"
	case StrategyManagedLifecycleBridge:
		return "Managed session bridge"
	default:
		return "Deferred"
	}
}
