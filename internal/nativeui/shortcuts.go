package nativeui

import (
	"strings"

	"github.com/egoist/mygo/ui"

	"github.com/wh-studio/herdr-client/internal/commandcenter"
	"github.com/wh-studio/herdr-client/internal/settings"
)

type shortcutAction uint8

const (
	shortcutSearch shortcutAction = iota + 1
	shortcutSettings
	shortcutRefresh
	shortcutBack
	shortcutForward
)

type shortcutBinding struct {
	Modifiers ui.Modifiers
	Key       ui.Key
	Action    shortcutAction
}

var defaultShortcutBindings = []shortcutBinding{
	{Modifiers: ui.Cmd, Key: ui.KeyK, Action: shortcutSearch},
	{Modifiers: ui.Cmd, Key: ui.KeyComma, Action: shortcutSettings},
	{Modifiers: ui.Cmd, Key: ui.KeyR, Action: shortcutRefresh},
	{Modifiers: ui.Cmd, Key: ui.KeyBracketLeft, Action: shortcutBack},
	{Modifiers: ui.Cmd, Key: ui.KeyBracketRight, Action: shortcutForward},
}

// shortcutBindings resolves the effective route shortcuts: the persisted
// settings are authoritative (they were stored but never consumed before
// 2026-10-06), falling back per-action to the default when unset or
// unparsable, so a broken edit can only lose one binding.
func shortcutBindings(value settings.ShortcutSettings) []shortcutBinding {
	bindings := make([]shortcutBinding, 0, len(defaultShortcutBindings))
	appendBinding := func(action shortcutAction, configured string, fallback shortcutBinding) {
		if binding, ok := parseShortcut(configured); ok {
			binding.Action = action
			bindings = append(bindings, binding)
			return
		}
		bindings = append(bindings, fallback)
	}
	appendBinding(shortcutSearch, value.Search, defaultShortcutBindings[0])
	appendBinding(shortcutSettings, value.Settings, defaultShortcutBindings[1])
	appendBinding(shortcutRefresh, value.Refresh, defaultShortcutBindings[2])
	appendBinding(shortcutBack, value.Back, defaultShortcutBindings[3])
	appendBinding(shortcutForward, value.Forward, defaultShortcutBindings[4])
	return bindings
}

// parseShortcut reads the persisted spelling ("Cmd+Shift+N", "Ctrl+K",
// "Cmd+Comma") into modifiers and a key.
func parseShortcut(value string) (shortcutBinding, bool) {
	var binding shortcutBinding
	if strings.TrimSpace(value) == "" {
		return binding, false
	}
	for _, part := range strings.Split(value, "+") {
		switch strings.ToLower(strings.TrimSpace(part)) {
		case "cmd", "super":
			binding.Modifiers |= ui.Cmd
		case "ctrl", "control":
			binding.Modifiers |= ui.Ctrl
		case "shift":
			binding.Modifiers |= ui.Shift
		case "alt", "option":
			binding.Modifiers |= ui.Alt
		default:
			key, ok := parseKey(part)
			if !ok {
				return shortcutBinding{}, false
			}
			if binding.Key != 0 {
				return shortcutBinding{}, false
			}
			binding.Key = key
		}
	}
	if binding.Key == 0 {
		return binding, false
	}
	return binding, true
}

func parseKey(value string) (ui.Key, bool) {
	switch strings.ToLower(strings.TrimSpace(value)) {
	case "comma":
		return ui.KeyComma, true
	case "[", "bracketleft":
		return ui.KeyBracketLeft, true
	case "]", "bracketright":
		return ui.KeyBracketRight, true
	case "enter", "return":
		return ui.KeyEnter, true
	case "escape", "esc":
		return ui.KeyEscape, true
	case "tab":
		return ui.KeyTab, true
	case "space":
		return ui.KeySpace, true
	}
	name := strings.ToUpper(strings.TrimSpace(value))
	if len(name) == 1 && name[0] >= 'A' && name[0] <= 'Z' {
		// MyGo letter keys are an enum run (KeyA + offset), not ASCII codes.
		return ui.KeyA + ui.Key(name[0]-'A'), true
	}
	if len(name) == 1 && name[0] >= '0' && name[0] <= '9' {
		return ui.Key0 + ui.Key(name[0]-'0'), true
	}
	if key, ok := map[string]ui.Key{
		"UP": ui.KeyUp, "DOWN": ui.KeyDown, "LEFT": ui.KeyLeft, "RIGHT": ui.KeyRight,
		"F1": ui.KeyF1, "F2": ui.KeyF2, "F3": ui.KeyF3, "F4": ui.KeyF4,
		"F5": ui.KeyF5, "F6": ui.KeyF6, "F7": ui.KeyF7, "F8": ui.KeyF8,
		"F9": ui.KeyF9, "F10": ui.KeyF10, "F11": ui.KeyF11, "F12": ui.KeyF12,
	}[name]; ok {
		return key, true
	}
	return 0, false
}

func (s *Shell) handleRouteShortcuts(c *ui.Context) {
	for _, binding := range shortcutBindings(s.settings.Shortcuts) {
		if c.Shortcut(binding.Modifiers, binding.Key) {
			s.runShortcut(binding.Action)
		}
	}
}

// handleSurfaceShortcuts routes surface-aware shortcuts (plan §33): the
// active WorkspacePrimarySurface owns Cmd/Ctrl+F and Cmd/Ctrl+Enter; hidden
// surfaces never receive them.
func (s *Shell) handleSurfaceShortcuts(c *ui.Context) {
	if s.router.Path() != routeWorkspace {
		return
	}
	if c.Shortcut(ui.Cmd|ui.Shift, ui.KeyC) {
		s.toggleTerminalChat()
		return
	}
	switch s.surface.current() {
	case WorkspaceSurfaceChat:
		if c.Shortcut(0, ui.KeyEscape) {
			s.showSurface(WorkspaceSurfaceTerminal)
		}
	case WorkspaceSurfaceDiff:
		// The godiff-style surface owns Find (⌘F); its bar and marks are
		// part of the surface render.
		if c.Shortcut(ui.Cmd, ui.KeyF) {
			if s.git != nil {
				s.git.gdFinding = !s.git.gdFinding
				if !s.git.gdFinding {
					s.git.gdMatchesFor = "\x00"
					s.git.gdRowsDirty = true
				}
			}
		}
	case WorkspaceSurfaceCommit:
		if c.Shortcut(ui.Cmd, ui.KeyEnter) {
			if selected := s.selectedCommitPaths(); len(selected) > 0 &&
				strings.TrimSpace(s.surface.commit.Subject) != "" && s.surface.commit.Captured {
				s.submitCommit(strings.TrimSpace(s.surface.commit.Subject),
					s.surface.commit.Body, selected)
			}
		}
	case WorkspaceSurfaceTerminal:
		// Terminal surface Cmd+F opens the Terminal find bar driven by the
		// live-verified Herdr CopySearch adapter (GWB-010/§31).
		if c.Shortcut(ui.Cmd, ui.KeyF) {
			s.openTerminalFind()
		}
	}
}

// openTerminalFind surfaces the Terminal find bar over the visible Terminal
// surface; the adapter executes against the selected pane only.
func (s *Shell) openTerminalFind() {
	if len(s.terminals) == 0 {
		return
	}
	if pane := s.selectedPane(); pane != nil && pane.TerminalID != "" {
		if surface := s.terminals[pane.TerminalID]; surface != nil {
			s.terminalFind.open = true
			s.terminalFind.paneID = pane.ID
			s.terminalFind.terminalID = pane.TerminalID
		}
	}
}

func (s *Shell) runShortcut(action shortcutAction) {
	switch action {
	case shortcutSearch:
		// The standalone Search page was removed (2026-10-06 product
		// decision): ⌘K is the unified search palette, which jumps to the
		// matching project/tab/pane/agent/history destination.
		s.openCommandCenter(commandcenter.ScopeAll)
	case shortcutSettings:
		if s.holdCommitNavigation() {
			return
		}
		s.router.Push(routeSettings)
	case shortcutRefresh:
		if s.router.Path() == routeWorkspace {
			if s.surface.current() == WorkspaceSurfaceDiff {
				// The Diff surface owns Refresh on its Git context.
				s.refreshGitChanges()
				return
			}
			s.reloadInstances(false)
		}
	case shortcutBack:
		if !s.holdCommitNavigation() {
			s.router.Back()
		}
	case shortcutForward:
		if !s.holdCommitNavigation() {
			s.router.Forward()
		}
	}
}
