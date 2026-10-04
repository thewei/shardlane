package nativeui

import (
	"context"
	"fmt"
	"log/slog"
	"strings"
	"time"

	"github.com/wh-studio/herdr-client/next/internal/platform"
	"github.com/wh-studio/herdr-client/next/internal/scripts"
)

// scriptRunTimeout bounds one Herdr-backed script launch transaction.
const scriptRunTimeout = 20 * time.Second

// runScriptCommand launches a user script through Herdr — never a
// client-owned process (GWB-006): the transaction creates a Tab in the
// script's directory, waits for its shell, and sends the script line as
// terminal input. Failures surface honestly; no status-only stubs.
func (s *Shell) runScriptCommand(root string, script scripts.ScriptDefinition) {
	if s.activeInstance == "" || s.selectedProjectID == "" {
		s.status = "Script needs an active workspace and project"
		return
	}
	if strings.TrimSpace(script.Command) == "" {
		s.status = "Script has no command"
		return
	}
	if s.scriptBusy == nil {
		s.scriptBusy = map[string]bool{}
	}
	if s.scriptBusy[script.Name] {
		return
	}
	s.scriptBusy[script.Name] = true
	s.status = fmt.Sprintf("Launching script %s…", script.Name)

	projectID := s.selectedProjectID
	instance := s.activeInstance
	go func() {
		err := s.launchScript(instance, projectID, root, script)
		s.win.Update(func() {
			delete(s.scriptBusy, script.Name)
			if err != nil {
				s.status = "Script launch failed"
				s.errText = err.Error()
				return
			}
			s.status = fmt.Sprintf("Script %s launched in a new tab", script.Name)
		})
	}()
}

// launchScript performs the transactional launch over verified Herdr calls.
func (s *Shell) launchScript(instance, projectID, root string, script scripts.ScriptDefinition) error {
	ctx, cancel := context.WithTimeout(context.Background(), scriptRunTimeout)
	defer cancel()

	projection, err := s.runtime.CreateTab(instance, projectID, root)
	if err != nil {
		return fmt.Errorf("create script tab: %w", err)
	}
	paneID := projection.FocusedPaneID
	if paneID == "" {
		return fmt.Errorf("script tab has no focused pane")
	}
	ready, err := s.runtime.ShellReady(ctx, instance, paneID)
	if err != nil {
		return fmt.Errorf("probe script shell: %w", err)
	}
	if !ready {
		slog.Warn("script shell not confirmed ready; sending anyway", "pane", paneID)
	}
	line := script.Command
	if !strings.HasSuffix(line, "\n") {
		line += "\n"
	}
	if err := s.runtime.SendAgentKeys(ctx, instance, paneID, []string{line}); err != nil {
		return fmt.Errorf("send script input: %w", err)
	}
	return nil
}

// observedPorts returns the real observed listening ports (GWB-004/005):
// no hard-coded samples. The probe runs on a bounded refresh; render reads
// the cached snapshot only.
func (s *Shell) observedPorts() []uint16 {
	if s.portProbe == nil {
		return nil
	}
	if time.Since(s.portsRefreshedAt) < 10*time.Second {
		return s.observedPortsList
	}
	// Kick a background refresh scoped to the active project root; show the previous snapshot meanwhile.
	s.portsRefreshedAt = time.Now()
	root := s.selectedTabCWD()
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
		defer cancel()
		matching, err := s.portProbe.ObserveForRoot(ctx, root)
		if err != nil {
			return
		}
		ports := dedupePorts(matching)
		if s.win == nil {
			s.observedPortsList = ports
			return
		}
		s.win.Update(func() {
			s.observedPortsList = ports
		})
	}()
	return s.observedPortsList
}

func dedupePorts(ports []uint16) []uint16 {
	seen := map[uint16]bool{}
	out := make([]uint16, 0, len(ports))
	for _, p := range ports {
		if !seen[p] {
			seen[p] = true
			out = append(out, p)
		}
	}
	return out
}

// copyTextToClipboard writes text through platform tools.
func (s *Shell) copyTextToClipboard(text string) {
	if err := platform.WriteClipboard(text); err != nil {
		s.status = "Clipboard unavailable"
	}
}

// openExternalPathParent reveals a path in the platform file manager.
func (s *Shell) openExternalPathParent(path string) {
	_ = platform.RevealFile(path)
}
