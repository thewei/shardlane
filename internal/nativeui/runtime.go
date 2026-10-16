package nativeui

/**
 * [INPUT]: 依赖 internal/herdr 的 ListAllInstances、Projection、UpgradeRuntime、ErrProtocolTooOld、ParseRef 与 HerdrUpdateTimeout
 * [OUTPUT]: 提供 Shell 的实例加载/打开/mutation 编排、失败分类（本地旧协议 → 可就地升级，记入 herdrUpgradeInstance）、会话定域的升级入口（bootstrap 页与 Settings 运行时行共享 beginHerdrUpgrade）与 checkHerdrRuntime 在线探针
 * [POS]: nativeui 的运行时编排层，连接 bootstrap_page 启动门与 herdr 适配器；不持有任何 Herdr 运行时状态
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"context"
	"errors"
	"log/slog"
	"time"

	"github.com/wh-studio/herdr-client/internal/herdr"
)

func (s *Shell) reloadInstances(autoOpen bool) {
	if s.win == nil || s.loading && !autoOpen {
		return
	}
	s.loading = true
	s.bootstrapFailed = false
	s.errText = ""
	// A fresh attempt also retires any stale update affordance; the
	// classification below re-derives it if the old server answers again.
	s.herdrUpgradeable = false
	s.herdrUpgradeInstance = ""
	s.herdrUpdateErr = ""
	s.herdrUpdateNotWrbl = false
	s.status = "Loading Herdr workspaces…"
	slog.Info("Herdr instance loading started", "auto_open", autoOpen)
	generation := s.generation.Add(1)
	go func() {
		started := time.Now()
		// The combined list includes one entry per saved remote machine; a
		// machine CLI read failure degrades to the local list only.
		instances, err := s.runtime.ListAllInstances()
		if err != nil {
			s.updateError(generation, err)
			return
		}
		slog.Info("Herdr instance list loaded", "count", len(instances), "elapsed_ms", time.Since(started).Milliseconds())
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
			if preferred == "" {
				s.loading = false
				s.bootstrapPending = false
				s.status = "Choose a Herdr workspace"
			} else {
				// Remain in the bootstrap screen until Projection and initial
				// attachment have finished. Do not flash empty navigation.
				s.loading = true
			}
			s.offline = false
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
		started := time.Now()
		projection, err := s.runtime.Projection(name)
		if err != nil {
			s.updateInstanceError(generation, name, err)
			return
		}
		slog.Info("Herdr projection received", "instance", name, "elapsed_ms", time.Since(started).Milliseconds())
		// Bootstrap already fetched the instance list. Re-enumerating all
		// sessions/machines here held the ready UI behind another CLI call,
		// which may stall independently of a successful Herdr snapshot.
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
				s.paneBusyIndex = make(map[string]paneBusyInfo)
			}
			s.activeInstance = name
			s.errText = ""
			s.offline = false
			s.status = ""
			s.applyProjection(projection, true)
			s.loading = false
			s.bootstrapPending = false
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
			s.status = ""
			if adoptResult {
				s.adoptRuntimeSelection(projection)
			}
			s.applyProjection(projection, true)
		})
	}()
}

func (s *Shell) updateError(generation uint64, err error) {
	s.updateInstanceError(generation, "", err)
}

// updateInstanceError records a runtime failure. When the failing instance is
// a known local ref, the old-protocol compatibility error additionally arms
// the bootstrap page's in-place Herdr update affordance.
func (s *Shell) updateInstanceError(generation uint64, instance string, err error) {
	slog.Warn("native runtime operation failed", "generation", generation, "instance", instance, "error", err)
	if s.win == nil {
		return
	}
	upgradeable := upgradeableFailure(instance, err)
	s.win.Update(func() {
		if generation != s.generation.Load() {
			return
		}
		s.herdrUpgradeable = upgradeable
		s.herdrUpgradeInstance = ""
		if upgradeable {
			s.herdrUpgradeInstance = instance
		}
		s.loading = false
		s.bootstrapFailed = s.bootstrapPending && s.projection.Protocol == 0
		s.bootstrapPending = false
		s.offline = true
		s.errText = err.Error()
		s.status = "Herdr unavailable"
	})
}

// upgradeableFailure reports whether a failed instance is one the bootstrap
// page can fix in place: a local Herdr whose server answered with a protocol
// below the supported minimum. Remote machines upgrade on their own host;
// every other failure is neither retryable into compatibility nor updatable
// from this machine.
func upgradeableFailure(instance string, err error) bool {
	return instance != "" &&
		!herdr.ParseRef(instance).IsRemote() &&
		errors.Is(err, herdr.ErrProtocolTooOld)
}

// herdrUpgradeTarget resolves the local session the runtime actions apply
// to. The bootstrap failure record wins over the active instance; remote
// machines upgrade on their own host, so local=false hides the actions.
func (s *Shell) herdrUpgradeTarget() (session string, local bool) {
	instance := s.herdrUpgradeInstance
	if instance == "" {
		instance = s.activeInstance
	}
	ref := herdr.ParseRef(instance)
	if ref.IsRemote() {
		return "", false
	}
	return ref.Session, true
}

// beginHerdrUpgrade runs Herdr's own updater from any surface that shows
// the in-place update (bootstrap page, Settings runtime row): Herdr
// downloads, verifies, and hands the running server to the new build live;
// an old server that survives the handoff is stopped so the next start
// brings up the upgraded binary. onSuccess runs on the UI thread once the
// runtime can continue; Shardlane never restarts servers itself.
func (s *Shell) beginHerdrUpgrade(onSuccess func()) {
	if s.herdrUpdating || s.win == nil {
		return
	}
	s.herdrUpdating = true
	s.herdrUpdateErr = ""
	s.herdrUpdateNotWrbl = false
	session, _ := s.herdrUpgradeTarget()
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), herdr.HerdrUpdateTimeout)
		defer cancel()
		err := herdr.UpgradeRuntime(ctx, session)
		s.win.Update(func() {
			s.herdrUpdating = false
			if err != nil {
				slog.Warn("Herdr update failed", "error", err)
				s.herdrUpdateNotWrbl = errors.Is(err, herdr.ErrUpdateNotWritable)
				s.herdrUpdateErr = err.Error()
				return
			}
			slog.Info("Herdr update finished")
			s.herdrUpgradeable = false
			onSuccess()
		})
	}()
}

// startHerdrUpgrade is the bootstrap page's continuation: return to the
// startup gate and reconnect through the normal flow.
func (s *Shell) startHerdrUpgrade() {
	s.beginHerdrUpgrade(func() {
		s.bootstrapPending = true
		s.reloadInstances(true)
	})
}

// updateHerdrFromSettings is the Settings runtime row's continuation: stay
// on the page, drop the stale probe, and refresh the current instance; a
// stopped server comes back through EnsureRunning on the upgraded binary.
func (s *Shell) updateHerdrFromSettings() {
	s.beginHerdrUpgrade(func() {
		s.herdrProbe = nil
		s.reloadInstances(false)
	})
}

// checkHerdrRuntime pings the local session's socket once and records the
// live runtime identity for the Settings runtime row. Raw RPC errors never
// reach the surface: the row speaks user language (F173).
func (s *Shell) checkHerdrRuntime() {
	if s.herdrProbing || s.win == nil {
		return
	}
	session, local := s.herdrUpgradeTarget()
	if !local {
		return
	}
	s.herdrProbing = true
	s.herdrProbeErr = ""
	go func() {
		probe, err := herdr.ProbeRuntime(session)
		s.win.Update(func() {
			s.herdrProbing = false
			if err != nil {
				slog.Warn("Herdr runtime probe failed", "error", err)
				s.herdrProbe = nil
				if errors.Is(err, herdr.ErrProtocolTooOld) {
					s.herdrProbeErr = "This Herdr is older than Shardlane supports."
				} else {
					s.herdrProbeErr = "Herdr is not reachable on this machine."
				}
				return
			}
			s.herdrProbeErr = ""
			s.herdrProbe = &probe
		})
	}()
}

func (s *Shell) applyProjection(projection herdr.Projection, restartWatcher bool) {
	slog.Debug("apply Herdr projection", "instance", s.activeInstance, "projects", len(projection.Projects), "tabs", len(projection.Tabs), "panes", len(projection.Panes), "agents", len(projection.Agents), "restart_watcher", restartWatcher)
	s.reconcileLocalSelection(projection)
	s.projection = projection
	// A reconciled projection is the authority that the runtime is healthy
	// again; the bootstrap page's update affordance retires with it.
	s.herdrUpgradeable = false
	s.herdrUpgradeInstance = ""
	s.herdrUpdateErr = ""
	s.herdrUpdateNotWrbl = false
	s.reconcileWorkbench()
	// Blocked agents must surface their structured prompts even when they
	// were blocked before this window existed: the spool only reports
	// transitions, so the freshly landed projection is the reconcile point.
	// Idempotent (pending-interaction guard inside).
	s.reconcileBlockedInteractions()
	s.wakeLiveTail()
	// Status edges (working→idle, blocked, …) must reach the WebView chat
	// even when this reconcile carries no transcript change: the composer
	// disposition derives from this projection.
	s.publishChatBridge()
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
