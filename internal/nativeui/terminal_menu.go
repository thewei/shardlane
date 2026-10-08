package nativeui

import (
	"github.com/egoist/mygo/ui"
)

// terminalPaneMenu is the right-click menu of one attached Herdr Pane. It
// mirrors the project tree's pane menu (the same actions and labels), so
// the visible surface and the sidebar stay one action implementation, and
// adds terminal-only entries (Paste). Right click reaches it because the
// attach transport strips the daemon's forced mouse-tracking modes; see
// mouseModeFilter.
func (s *Shell) terminalPaneMenu(c *ui.Context, m *ui.Menu, surface *terminalSurface) {
	s.paneOverflowItems(m, surface.paneID, surface.label)
	m.Separator()
	if m.Item("Paste").Chosen() {
		if text := c.ReadClipboard(); text != "" {
			surface.term.Paste(text)
		}
	}
}
