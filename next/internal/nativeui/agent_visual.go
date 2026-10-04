package nativeui

import (
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/next/internal/herdr"
	"github.com/wh-studio/herdr-client/next/internal/history"
)

// Per-provider marks for live Agents. Monochrome 24×24 paths that read at
// 14–16 DIP; unknown providers fall back to the generic person mark.
var (
	glyphClaude = icon(`<path d="M12 3v18M5.5 5.5l13 13M3 12h18M5.5 18.5l13-13"/>`)
	glyphCodex  = icon(`<rect x="4" y="4" width="16" height="16" rx="4"/><path d="m9 10 2 2-2 2M13 14h3"/>`)
	glyphGemini = icon(`<path d="M12 3c.6 5 3.9 8.4 9 9-5.1.6-8.4 4-9 9-.6-5-3.9-8.4-9-9 5.1-.6 8.4-4 9-9Z"/>`)
	glyphPi     = icon(`<circle cx="12" cy="12" r="8"/><path d="M9 9v6M15 9v6M9 12h6"/>`)
	glyphCoplot = icon(`<path d="M6 15a6 6 0 0 1 12 0"/><rect x="4" y="15" width="16" height="4" rx="2"/>`)
	glyphCursor = icon(`<path d="m5 3 14 8-6 2-2 6L5 3Z"/>`)
	glyphOpen   = icon(`<circle cx="9" cy="12" r="5"/><circle cx="15" cy="12" r="5"/>`)
	glyphKiro   = icon(`<path d="M12 3 4 21M12 3l8 18M7.5 14h9"/>`)
)

// agentGlyph maps a provider identity to its mark, falling back to the
// generic agent person for unknown providers.
func agentGlyph(provider history.AgentID) *ui.SVG {
	switch provider {
	case history.AgentClaudeCode:
		return glyphClaude
	case history.AgentCodex, history.AgentOMP, history.AgentCommandCode:
		return glyphCodex
	case history.AgentGemini:
		return glyphGemini
	case history.AgentPi:
		return glyphPi
	case history.AgentCopilot:
		return glyphCoplot
	case history.AgentCursor:
		return glyphCursor
	case history.AgentOpenCode:
		return glyphOpen
	case history.AgentKiro:
		return glyphKiro
	default:
		return iconAgent
	}
}

// paneAgent returns the live Agent bound to a Pane, if any.
func (s *Shell) paneAgent(paneID string) *herdr.Agent {
	for index := range s.projection.Agents {
		if s.projection.Agents[index].PaneID == paneID {
			return &s.projection.Agents[index]
		}
	}
	return nil
}

// statusDot is the small round Agent-state marker shown at a glyph's
// corner: red/orange = waiting for the user (attention), blue = working,
// green = ready for review, gray = idle/unknown. Colors resolve through
// the shared operational tones so the dot never disagrees with the status
// pills.
func statusDot(c *ui.Context, state operationalState) {
	t := c.Theme()
	tone := operationalTone(state)
	color := designTokens(t.Dark).StatusColor(tone, t.Dark)
	if state == opIdle || state == opUnknown {
		color = t.TextMuted
	}
	ui.Box(c).Size(7, 7).Radius(3.5).Background(color).Shrink(0)
}
