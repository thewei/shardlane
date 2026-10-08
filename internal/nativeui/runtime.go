package nativeui

import (
	"log/slog"

	"github.com/wh-studio/herdr-client/internal/herdr"
)

func (s *Shell) reloadInstances(autoOpen bool) {
	if s.win == nil || s.loading && !autoOpen {
		return
	}
	s.loading = true
	s.errText = ""
	s.status = "Loading Herdr workspaces…"
	slog.Debug("reload Herdr instances", "auto_open", autoOpen)
	generation := s.generation.Add(1)
	go func() {
		// The combined list includes one entry per saved remote machine; a
		// machine CLI read failure degrades to the local list only.
		instances, err := s.runtime.ListAllInstances()
		if err != nil {
			s.updateError(generation, err)
			return
		}
		// The session hint only steers startup (autoOpen): a stale hint
		// falls back to the default instance. A mid-session refresh keeps
		// the current instance and surfaces the failure if it is gone.
		preferred := s.activeInstance
		if autoOpen {
			resolved := preferredInstanceName(instances, preferred)
			if resolved != preferred && preferred != "" {
				slog.Info("session instance not listed; falling back", "instance", preferred, "preferred", resolved)
			}
			preferred = resolved
		}
		s.win.Update(func() {
			if generation != s.generation.Load() {
				return
			}
			s.instances = instances
			s.loading = false
			s.offline = false
			if preferred == "" {
				s.status = "Choose a Herdr workspace"
			}
		})
		if preferred != "" {
			s.openInstanceAsync(preferred, generation)
		}
	}()
}

func (s *Shell) openInstance(name string) {
	if name == "" || s.win == nil {
		return
	}
	generation := s.generation.Add(1)
	s.loading = true
	s.errText = ""
	s.status = "Opening " + s.instanceLabel(name) + "…"
	slog.Info("open Herdr workspace", "instance", name)
	s.openInstanceAsync(name, generation)
}

// instanceLabel renders a menu-facing name for an instance key; machine keys
// would leak their "machine/<id>" form into status text otherwise.
func (s *Shell) instanceLabel(name string) string {
	for _, instance := range s.instances {
		if instance.Name == name && instance.DisplayName != "" {
			return instance.DisplayName
		}
	}
	return name
}

func (s *Shell) openInstanceAsync(name string, generation uint64) {
	go func() {
		projection, err := s.runtime.Projection(name)
		if err != nil {
			s.updateError(generation, err)
			return
		}
		instances, _ := s.runtime.ListAllInstances()
		s.win.Update(func() {
			if generation != s.generation.Load() {
				return
			}
			slog.Info("Herdr workspace ready", "instance", name, "protocol", projection.Protocol, "projects", len(projection.Projects), "tabs", len(projection.Tabs), "panes", len(projection.Panes), "agents", len(projection.Agents))
			if s.activeInstance != name {
				s.clearLocalSelection()
				s.closeAllTerminals()
				// Service activity is per-instance; the next scan rebuilds it.
				s.serviceIndex = sidebarServiceIndex{}
			}
			s.activeInstance = name
			if len(instances) > 0 {
				s.instances = instances
			}
			s.loading = false
			s.errText = ""
			s.offline = false
			s.status = "Native Terminal"
			s.applyProjection(projection, true)
		})
	}()
}

func (s *Shell) mutate(fn func() (herdr.Projection, error)) {
	s.mutateWithSelection(false, fn)
}

// mutateSelecting is reserved for user-initiated structural operations whose
// verified Herdr result semantically selects the newly created target (for
// example New Tab / Split Pane). Ordinary runtime snapshots never steal this
// window's local selection.
func (s *Shell) mutateSelecting(fn func() (herdr.Projection, error)) {
	s.mutateWithSelection(true, fn)
}

func (s *Shell) mutateWithSelection(adoptResult bool, fn func() (herdr.Projection, error)) {
	if s.activeInstance == "" || s.win == nil || s.loading {
		return
	}
	generation := s.generation.Add(1)
	s.loading = true
	s.errText = ""
	go func() {
		projection, err := fn()
		if err != nil {
			s.updateError(generation, err)
			return
		}
		s.win.Update(func() {
			if generation != s.generation.Load() {
				return
			}
			s.loading = false
			s.offline = false
			s.status = "Native Terminal"
			if adoptResult {
				s.adoptRuntimeSelection(projection)
			}
			s.applyProjection(projection, true)
		})
	}()
}

func (s *Shell) updateError(generation uint64, err error) {
	slog.Warn("native runtime operation failed", "generation", generation, "error", err)
	if s.win == nil {
		return
	}
	s.win.Update(func() {
		if generation != s.generation.Load() {
			return
		}
		s.loading = false
		s.offline = true
		s.errText = err.Error()
		s.status = "Herdr unavailable"
	})
}

func (s *Shell) applyProjection(projection herdr.Projection, restartWatcher bool) {
	slog.Debug("apply Herdr projection", "instance", s.activeInstance, "projects", len(projection.Projects), "tabs", len(projection.Tabs), "panes", len(projection.Panes), "agents", len(projection.Agents), "restart_watcher", restartWatcher)
	s.reconcileLocalSelection(projection)
	s.projection = projection
	s.reconcileWorkbench()
	s.wakeLiveTail()
	s.updateTray()
	s.updateDockBadge()
	s.refreshUsageProjection(false)
	s.refreshGitMeta()
	// The floating Quick Panel shares this reconciled state but gets no
	// input of its own while open; push the repaint to it.
	s.invalidateQuickPanel()
	if s.router.Path() == routeWorkspace {
		// Terminal attachments stay managed even while Diff/Commit is the
		// visible surface (GWB-041/043): sync reconciles invalid ones, the
		// canvas renders only on the Terminal surface.
		if err := s.syncTerminals(projection); err != nil {
			s.errText = err.Error()
			s.status = "Terminal attach failed"
			return
		}
		s.syncWorkspaceForSelection()
	} else {
		s.closeAllTerminals()
	}
	s.selected = selectionSidebarKey(projection, s.selectedProjectID, s.selectedTabID, s.selectedPaneID)
	// Pane membership may have changed under the running services; the scan
	// re-checks staleness itself, so this is at most one cheap no-op.
	s.kickServiceScan()
	if restartWatcher {
		s.startWatcher(projection)
	}
}
