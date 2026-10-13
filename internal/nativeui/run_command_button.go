package nativeui

/**
 * [INPUT]: internal/scripts（Detect/MergeCommands/LastUsedStore）, runtime（ShellReady/SendPaneText）, paneBusyIndex
 * [OUTPUT]: paneRunCommandControl（Terminal 卡片 header 的项目命令运行控件：Run/Stop/菜单）、runCommandInPane/stopPaneCommand 事务、命令缓存
 * [POS]: presentation only — 运行与停止都经已验证的 pane.send_text 字面写入 Pane PTY（协议 22，2026-10-13 隔离 daemon 探针验证 \x03 中断），忙碌=pane.process_info 扫描事实；无新 Herdr 方法、无第二运行时
 * [PROTOCOL]: update this header and check CLAUDE.md
 */

import (
	"context"
	"log/slog"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/scripts"
)

// The pane header's run control turns project-defined commands
// (package.json scripts, Makefile targets, justfile recipes, user scripts)
// into one-click runs inside that exact Herdr Pane. The control reads the
// pane's busy fact from the service scan (no new RPC loop) and its command
// list from a root-keyed cache built off the render path (same shape as the
// observed-ports cache).

const (
	// commandCacheTTL bounds one root's detected-command cache.
	commandCacheTTL = 30 * time.Second
	// runCommandTimeout bounds one Run/Stop transaction (one readiness
	// probe + one send).
	runCommandTimeout = 10 * time.Second
)

// commandEntriesFor returns the merged command menu for root, reading the
// cache and kicking a background refresh when it is stale (render never
// touches the filesystem).
func (s *Shell) commandEntriesFor(root string) []scripts.CommandEntry {
	if root == "" {
		return nil
	}
	if root != s.cmdCacheRoot || time.Since(s.cmdCacheAt) >= commandCacheTTL {
		s.cmdCacheRoot = root
		s.cmdCacheAt = time.Now()
		s.dispatch(func() {
			detected := scripts.Detect(root)
			var defs []scripts.ScriptDefinition
			if s.scriptStore != nil {
				defs = s.scriptStore.List(root)
			}
			entries := scripts.MergeCommands(defs, detected)
			s.onUI(func() {
				if s.cmdCacheRoot != root {
					return
				}
				s.cmdCacheList = entries
				s.cmdCacheAt = time.Now()
			})
		})
	}
	return s.cmdCacheList
}

// lastCommandEntry resolves the entry the control runs on a plain click:
// the root's last-used command, when it still exists in the menu.
func (s *Shell) lastCommandEntry(root string, entries []scripts.CommandEntry) (scripts.CommandEntry, bool) {
	if s.lastUsedStore == nil || root == "" {
		return scripts.CommandEntry{}, false
	}
	name := s.lastUsedStore.Get(root)
	if name == "" {
		return scripts.CommandEntry{}, false
	}
	for _, entry := range entries {
		if entry.Name == name {
			return entry, true
		}
	}
	return scripts.CommandEntry{}, false
}

// paneRunCommandControl renders the pane card header's run control for one
// attached surface. Nothing renders without a local project directory or
// when the menu is empty:
//
//	idle                → Run <last> button + command menu
//	busy, no Agent      → Stop <proc> (one verified ctrl+c)
//	busy, Agent pane    → nothing (Agent lifecycle stays Chat-owned)
func (s *Shell) paneRunCommandControl(c *ui.Context, k *gorexColors, surface *terminalSurface) {
	root := surface.cwd
	busy, isBusy := s.paneBusyState(surface.paneID)
	agentPane := s.paneAgent(surface.paneID) != nil

	if isBusy {
		if agentPane {
			return
		}
		proc := "command"
		if len(busy.Procs) > 0 {
			proc = busy.Procs[0]
		}
		stop := gorexIconButton(c, k, iconSquare, "Stop "+proc, "Send Ctrl+C to "+proc, false, gorexPaneBtn, gorexPaneGlyph)
		if stop.Clicked() {
			s.stopPaneCommand(surface.paneID, proc)
		}
		return
	}

	entries := s.commandEntriesFor(root)
	if len(entries) == 0 {
		return
	}
	last, hasLast := s.lastCommandEntry(root, entries)
	label, tooltip := "Run", "Run a project command"
	if hasLast {
		label, tooltip = "Run "+last.Name, "Run "+last.Command
	}
	run := gorexIconButton(c, k, iconPlay, label, tooltip, false, gorexPaneBtn, gorexPaneGlyph)
	if run.Clicked() {
		if hasLast {
			s.runCommandInPane(surface.paneID, root, last)
		} else {
			s.status = "Pick a command from the pane's command menu"
		}
	}
	menu := gorexPulldown(c, k, iconTerminal, "Command menu", gorexPaneBtn, gorexPaneGlyph-4)
	menu.Menu(func(m *ui.Menu) {
		s.paneCommandMenu(m, surface, entries, last, hasLast)
	})
}

// paneCommandMenu fills the command menu: the last-used command first, then
// the merged list in stable order. Every item runs inside this exact pane.
func (s *Shell) paneCommandMenu(m *ui.Menu, surface *terminalSurface, entries []scripts.CommandEntry, last scripts.CommandEntry, hasLast bool) {
	ordered := make([]scripts.CommandEntry, 0, len(entries)+1)
	if hasLast {
		ordered = append(ordered, last)
	}
	for _, entry := range entries {
		if hasLast && entry.Name == last.Name {
			continue
		}
		ordered = append(ordered, entry)
	}
	for _, entry := range ordered {
		e := entry
		label := e.Name
		if hasLast && e.Name == last.Name {
			label = e.Name + "  ·  last used"
		}
		if m.Item(label).Chosen() {
			s.runCommandInPane(surface.paneID, surface.cwd, e)
		}
	}
}

// runCommandInPane types one command line into the pane's PTY —
// pane.send_text (the pane-scoped literal-write path; agent.send_keys is
// agent-targeted and rejects plain panes), pointed at the existing pane.
// The readiness probe absorbs the 10s scan's staleness: a stale Run can
// never type into a running process.
func (s *Shell) runCommandInPane(paneID, root string, entry scripts.CommandEntry) {
	if s.activeInstance == "" || paneID == "" || strings.TrimSpace(entry.Command) == "" {
		return
	}
	if s.win == nil {
		// Headless (the mutate rule): no window, no live transaction —
		// tests assert the render states and guards, never the runtime.
		return
	}
	if s.runBusy == nil {
		s.runBusy = make(map[string]bool)
	}
	if s.runBusy[paneID] {
		return
	}
	s.runBusy[paneID] = true
	s.status = "Running " + entry.Name + "…"

	instance := s.activeInstance
	s.dispatch(func() {
		ctx, cancel := context.WithTimeout(context.Background(), runCommandTimeout)
		defer cancel()

		ready, err := s.runtime.ShellReady(ctx, instance, paneID)
		var failure string
		switch {
		case err != nil:
			failure = err.Error()
		case !ready:
			failure = "this pane is busy — stop the current command first"
		default:
			line := entry.Command
			if !strings.HasSuffix(line, "\n") {
				line += "\n"
			}
			if err := s.runtime.SendPaneText(instance, paneID, line); err != nil {
				failure = err.Error()
			}
		}

		if failure == "" && s.lastUsedStore != nil {
			// Persistence stays off the UI lane; the render reads the
			// mutex-guarded map on its next frame.
			if err := s.lastUsedStore.Set(root, entry.Name); err != nil {
				slog.Warn("last-used command persist failed", "error", err)
			}
		}
		s.onUI(func() {
			delete(s.runBusy, paneID)
			if failure != "" {
				s.status = "Command failed"
				s.errText = failure
				return
			}
			s.status = "Running " + entry.Name
			slog.Info("command run", "pane", paneID, "command", entry.Name)
			// The pane just got busy; refresh the busy fact soon.
			s.kickServiceScan()
		})
	})
}

// stopPaneCommand interrupts the pane's foreground process with one ETX
// byte written through pane.send_text: the tty delivers SIGINT exactly as a
// physical Ctrl+C does (probed 2026-10-13 on an isolated daemon — the shell
// returned to the prompt). A readiness probe first: when the shell is
// already back at the prompt, the interrupt is skipped instead of clearing
// whatever the user is typing.
func (s *Shell) stopPaneCommand(paneID, proc string) {
	if s.activeInstance == "" || paneID == "" {
		return
	}
	if s.win == nil {
		// Headless (the mutate rule): see runCommandInPane.
		return
	}
	if s.runBusy == nil {
		s.runBusy = make(map[string]bool)
	}
	if s.runBusy[paneID] {
		return
	}
	s.runBusy[paneID] = true
	s.status = "Stopping " + proc + "…"

	instance := s.activeInstance
	s.dispatch(func() {
		ctx, cancel := context.WithTimeout(context.Background(), runCommandTimeout)
		defer cancel()

		var failure string
		ready, err := s.runtime.ShellReady(ctx, instance, paneID)
		switch {
		case err != nil:
			failure = err.Error()
		case ready:
			slog.Info("command stop skipped: pane idle", "pane", paneID)
		default:
			// ETX is the Ctrl+C byte: the tty turns it into SIGINT for the
			// foreground group, or hands it to the app verbatim when it
			// owns its input (raw mode) — identical to a real keypress.
			if err := s.runtime.SendPaneText(instance, paneID, "\x03"); err != nil {
				failure = err.Error()
			}
		}

		s.onUI(func() {
			delete(s.runBusy, paneID)
			if failure != "" {
				s.status = "Stop failed"
				s.errText = failure
				return
			}
			s.status = "Sent Ctrl+C to " + proc
			s.kickServiceScan()
		})
	})
}
