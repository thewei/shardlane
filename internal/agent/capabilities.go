// Package agent owns the UI-independent Agent product semantics for the MyGo
// rewrite: the provider capability projection, launch request identity, and
// (later) the full launch transaction. It never starts provider processes;
// Herdr remains the runtime authority.
package agent

import "github.com/wh-studio/herdr-client/internal/history"

// liveCapability mirrors the shardlane-history live registry authority.
type liveCapability string

const (
	liveAppendLog   liveCapability = "append-log"
	liveHookJournal liveCapability = "hook-journal"
	liveNone        liveCapability = "none"
)

// exposure mirrors the registry product exposure levels.
type exposure string

const (
	exposureStable  exposure = "stable"
	exposurePreview exposure = "preview"
	exposureHidden  exposure = "hidden"
)

type providerRegistryEntry struct {
	live     liveCapability
	exposure exposure
}

// providerRegistry is ported from the shardlane-history PROVIDERS authority.
// Unknown providers fail closed as hidden rather than being guessed.
var providerRegistry = map[history.AgentID]providerRegistryEntry{
	history.AgentClaudeCode:  {liveAppendLog, exposureStable},
	history.AgentCodex:       {liveAppendLog, exposureStable},
	history.AgentCopilot:     {liveNone, exposureHidden},
	history.AgentCursor:      {liveAppendLog, exposurePreview},
	history.AgentOpenCode:    {liveNone, exposureHidden},
	history.AgentCommandCode: {liveAppendLog, exposurePreview},
	history.AgentKiro:        {liveNone, exposureHidden},
	history.AgentGemini:      {liveNone, exposureHidden},
	history.AgentPi:          {liveAppendLog, exposureStable},
	history.AgentOMP:         {liveAppendLog, exposureStable},
	history.AgentGrok:        {liveNone, exposureHidden},
	history.AgentKimi:        {liveAppendLog, exposureHidden},
	history.AgentAntigravity: {liveHookJournal, exposurePreview},
	history.AgentDSH:         {liveNone, exposureHidden},
	history.AgentQoder:       {liveNone, exposurePreview},
}

// planSupport expresses how Plan mode is requested per provider.
type planSupport string

const (
	planNativeCLI      planSupport = "native-cli"
	planPostLaunchKeys planSupport = "post-launch-keys"
	planPromptPrefix   planSupport = "prompt-prefix"
)

// bridgeKind expresses the provider companion-bridge transport. It is
// descriptive only; no bridge runs in the MyGo client yet.
type bridgeKind string

const (
	bridgeNone        bridgeKind = "none"
	bridgeCommandHook bridgeKind = "command-hook"
	bridgeExtension   bridgeKind = "extension"
	bridgePlugin      bridgeKind = "plugin"
)

// Environment carries the caller-supplied user/CLI facts the capability
// projection never derives on its own.
type Environment struct {
	Enabled   bool
	Installed bool
}

// Capabilities is one provider's product capability row, composed — never
// re-derived — from the authorities. Start Agent stays disabled product-wide
// until the full launch transaction lands; this row carries no launch path.
type Capabilities struct {
	Provider          history.AgentID `json:"provider"`
	DisplayName       string          `json:"display_name"`
	Exposure          string          `json:"exposure"`
	Enabled           bool            `json:"enabled"`
	Installed         bool            `json:"installed"`
	Startable         bool            `json:"startable"`
	SemanticLive      bool            `json:"semantic_live"`
	NativeResume      bool            `json:"native_resume"`
	PlanMode          string          `json:"plan_mode"`
	PermissionModes   bool            `json:"permission_modes"`
	BridgeKind        string          `json:"bridge_kind"`
	UnavailableReason string          `json:"unavailable_reason,omitempty"`
}

// integrationDeferred lists providers whose runtime integration strategy is
// explicitly "not claimed yet" (Rust registry: Deferred).
func integrationDeferred(agent history.AgentID) bool {
	return agent == history.AgentQoder
}

// ResumeSupported pins the resume.rs authority: every registered provider has
// resume parts today.
func ResumeSupported(agent history.AgentID) bool {
	_, ok := providerRegistry[agent]
	return ok
}

// ProjectCapabilities composes one provider's product capability row.
func ProjectCapabilities(agent history.AgentID, environment Environment) Capabilities {
	registry, known := providerRegistry[agent]
	if !known {
		registry = providerRegistryEntry{live: liveNone, exposure: exposureHidden}
	}
	startable := environment.Enabled && environment.Installed && !integrationDeferred(agent)

	capabilities := Capabilities{
		Provider:        agent,
		DisplayName:     agent.DisplayName(),
		Exposure:        string(registry.exposure),
		Enabled:         environment.Enabled,
		Installed:       environment.Installed,
		Startable:       startable,
		SemanticLive:    registry.live != liveNone,
		NativeResume:    ResumeSupported(agent),
		PlanMode:        string(planModeSupport(agent)),
		PermissionModes: permissionModes(agent),
		BridgeKind:      string(bridgeFor(agent)),
	}
	if reason := unavailableReason(agent, registry.exposure, environment, startable); reason != "" {
		capabilities.UnavailableReason = reason
	}
	return capabilities
}

// ProjectAllCapabilities snapshots every registered provider.
func ProjectAllCapabilities(environment func(history.AgentID) Environment) []Capabilities {
	rows := make([]Capabilities, 0, len(history.AllAgents))
	for _, agent := range history.AllAgents {
		rows = append(rows, ProjectCapabilities(agent, environment(agent)))
	}
	return rows
}

func planModeSupport(agent history.AgentID) planSupport {
	switch agent {
	case history.AgentClaudeCode, history.AgentGemini:
		return planNativeCLI
	case history.AgentCodex:
		return planPostLaunchKeys
	default:
		return planPromptPrefix
	}
}

func permissionModes(agent history.AgentID) bool {
	switch agent {
	case history.AgentClaudeCode, history.AgentCodex, history.AgentGemini:
		return true
	default:
		return false
	}
}

func bridgeFor(agent history.AgentID) bridgeKind {
	switch agent {
	case history.AgentClaudeCode, history.AgentCodex, history.AgentCursor,
		history.AgentCopilot, history.AgentGemini, history.AgentAntigravity:
		return bridgeCommandHook
	case history.AgentPi, history.AgentOMP:
		return bridgeExtension
	case history.AgentOpenCode:
		return bridgePlugin
	default:
		return bridgeNone
	}
}

func unavailableReason(agent history.AgentID, agentExposure exposure, environment Environment, startable bool) string {
	switch {
	case agentExposure == exposureHidden:
		return agent.DisplayName() + " is not available in this build"
	case !environment.Enabled:
		return agent.DisplayName() + " is disabled in Settings"
	case !environment.Installed:
		return "Setup required"
	case !startable:
		return agent.DisplayName() + " runtime integration is pending"
	default:
		return ""
	}
}
