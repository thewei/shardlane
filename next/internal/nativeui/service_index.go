package nativeui

import (
	"context"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/wh-studio/herdr-client/next/internal/herdr"
	"github.com/wh-studio/herdr-client/next/internal/services"
)

// The service index answers one sidebar question: which Panes (and which of
// their collapsed ancestors) have something running right now. A Pane runs
// a service when one of its foreground processes (Herdr pane.process_info)
// owns a listening TCP port from the local lsof snapshot; daemons that left
// the foreground group fall back to a working-directory match against the
// Pane CWDs, attributed to the Pane when unambiguous and to the Project
// otherwise. Everything below runs on background lanes — render reads the
// cached index only.

const (
	// serviceScanInterval is the background rescan cadence: services start
	// and stop without Herdr events, so the index needs its own clock.
	serviceScanInterval = 10 * time.Second
	// serviceScanMinGap debounces kicks fired by projection bursts.
	serviceScanMinGap = 5 * time.Second
	// serviceScanTimeout bounds one whole scan (lsof + per-pane RPCs).
	serviceScanTimeout = 8 * time.Second
	// serviceScanWorkers bounds concurrent pane.process_info calls.
	serviceScanWorkers = 4
)

// paneServiceActivity is one observed service: the first listening
// foreground process name and its deduplicated, sorted ports.
type paneServiceActivity struct {
	Proc  string
	Ports []uint16
}

// foregroundPaneService matches one Pane's foreground processes against the
// listener snapshot. It returns the activity and the listener PIDs it
// consumed, so cwd-fallback attribution can skip them.
func foregroundPaneService(info *herdr.PaneProcessInfo, listeners map[int]services.ListenerSnapshot) (paneServiceActivity, []int) {
	if info == nil || len(listeners) == 0 {
		return paneServiceActivity{}, nil
	}
	var act paneServiceActivity
	var attributed []int
	for _, fg := range info.ForegroundProcesses {
		snap, ok := listeners[fg.PID]
		if !ok {
			continue
		}
		attributed = append(attributed, fg.PID)
		if act.Proc == "" {
			act.Proc = fg.Name
		}
		act.Ports = mergePorts(act.Ports, snap.Ports)
	}
	if len(act.Ports) == 0 {
		return paneServiceActivity{}, nil
	}
	sort.Slice(act.Ports, func(i, j int) bool { return act.Ports[i] < act.Ports[j] })
	return act, attributed
}

func mergePorts(dst, src []uint16) []uint16 {
	for _, port := range src {
		if port == 0 {
			continue
		}
		found := false
		for _, existing := range dst {
			if existing == port {
				found = true
				break
			}
		}
		if !found {
			dst = append(dst, port)
		}
	}
	return dst
}

// dirContains reports whether dir equals or lives under root.
func dirContains(root, dir string) bool {
	if root == "" || dir == "" {
		return false
	}
	if root == dir {
		return true
	}
	return strings.HasPrefix(dir, strings.TrimRight(root, "/")+string(filepath.Separator))
}

// sidebarServiceIndex is the merged per-row service view behind the sidebar:
// Panes hold foreground-matched services plus unambiguous cwd fallbacks;
// Projects hold the ambiguous daemons their Panes' directories contain.
type sidebarServiceIndex struct {
	Panes    map[string]paneServiceActivity
	Projects map[string]paneServiceActivity
}

// computeSidebarServices crosses the projection with the listener snapshot:
// foreground processes first (exact Pane attribution), then leftover
// listeners matched by working directory.
func computeSidebarServices(projection herdr.Projection, listeners map[int]services.ListenerSnapshot, infos map[string]*herdr.PaneProcessInfo) sidebarServiceIndex {
	index := sidebarServiceIndex{
		Panes:    make(map[string]paneServiceActivity),
		Projects: make(map[string]paneServiceActivity),
	}
	attributed := make(map[int]bool)
	for _, pane := range projection.Panes {
		act, consumed := foregroundPaneService(infos[pane.ID], listeners)
		for _, pid := range consumed {
			attributed[pid] = true
		}
		if len(act.Ports) > 0 {
			index.Panes[pane.ID] = act
		}
	}

	for pid, snap := range listeners {
		if attributed[pid] || snap.CWD == "" {
			continue
		}
		daemon := paneServiceActivity{Proc: snap.Command, Ports: sortedPorts(snap.Ports)}
		var matches []herdr.Pane
		for _, pane := range projection.Panes {
			if dirContains(pane.CWD, snap.CWD) {
				matches = append(matches, pane)
			}
		}
		if len(matches) == 1 {
			pane := matches[0]
			extra := index.Panes[pane.ID]
			extra.Proc = firstText(extra.Proc, daemon.Proc)
			extra.Ports = mergePorts(extra.Ports, daemon.Ports)
			index.Panes[pane.ID] = extra
			continue
		}
		seen := make(map[string]bool)
		for _, pane := range matches {
			if seen[pane.ProjectID] {
				continue
			}
			seen[pane.ProjectID] = true
			extra := index.Projects[pane.ProjectID]
			extra.Proc = firstText(extra.Proc, daemon.Proc)
			extra.Ports = mergePorts(extra.Ports, daemon.Ports)
			index.Projects[pane.ProjectID] = extra
		}
	}
	return index
}

func sortedPorts(ports []uint16) []uint16 {
	out := append([]uint16(nil), ports...)
	sort.Slice(out, func(i, j int) bool { return out[i] < out[j] })
	return out
}

func firstText(values ...string) string {
	for _, value := range values {
		if strings.TrimSpace(value) != "" {
			return value
		}
	}
	return ""
}

// sidebarActivity is the rollup marker behind a tree row's icon-corner dot:
// something is running in this row or below it.
type sidebarActivity struct {
	On     bool
	Tone   StatusTone
	Detail string
}

// buildSidebarActivity rolls service and agent activity up the sidebar
// hierarchy: Panes light for their own services (their Agent state already
// owns the icon-corner dot), Tabs and Projects light for whatever runs
// beneath them — services in green, otherwise the strongest Agent state.
func buildSidebarActivity(projection herdr.Projection, services sidebarServiceIndex) map[string]sidebarActivity {
	index := make(map[string]sidebarActivity, len(projection.Panes)+len(projection.Tabs)+len(projection.Projects))

	panesByTab := make(map[string][]string, len(projection.Tabs))
	tabsByProject := make(map[string][]string, len(projection.Projects))
	for _, pane := range projection.Panes {
		panesByTab[pane.TabID] = append(panesByTab[pane.TabID], pane.ID)
	}
	for _, tab := range projection.Tabs {
		tabsByProject[tab.ProjectID] = append(tabsByProject[tab.ProjectID], tab.ID)
	}
	agentStateByPane := make(map[string]operationalState, len(projection.Agents))
	for i := range projection.Agents {
		agent := &projection.Agents[i]
		agentStateByPane[agent.PaneID] = normalizeRuntimeStatus(agent.Status)
	}

	for _, pane := range projection.Panes {
		if svc := services.Panes[pane.ID]; len(svc.Ports) > 0 {
			index[pane.ID] = sidebarActivity{On: true, Tone: ToneSuccess, Detail: serviceActivityDetail(svc)}
		}
	}
	for _, tab := range projection.Tabs {
		if act, on := rollupActivity(index, agentStateByPane, panesByTab[tab.ID]); on {
			index[tab.ID] = act
		}
	}
	for _, project := range projection.Projects {
		var paneIDs []string
		for _, tabID := range tabsByProject[project.ID] {
			paneIDs = append(paneIDs, panesByTab[tabID]...)
		}
		act, on := rollupActivity(index, agentStateByPane, paneIDs)
		if daemon := services.Projects[project.ID]; len(daemon.Ports) > 0 {
			act.On = true
			act.Tone = ToneSuccess
			act.Detail = joinDetails(act.Detail, serviceActivityDetail(daemon))
			on = true
		}
		if on {
			index[project.ID] = act
		}
	}
	return index
}

// rollupActivity aggregates descendant Panes for a Tab/Project row.
func rollupActivity(index map[string]sidebarActivity, agentStateByPane map[string]operationalState, paneIDs []string) (sidebarActivity, bool) {
	act := sidebarActivity{}
	serviceCount := 0
	agentCount := 0
	strongest := opUnknown
	for _, paneID := range paneIDs {
		if sub, on := index[paneID]; on && sub.Tone == ToneSuccess {
			serviceCount++
			act.Detail = joinDetails(act.Detail, sub.Detail)
		}
		if state, ok := agentStateByPane[paneID]; ok && state != opUnknown {
			agentCount++
			if operationalPriority(state) < operationalPriority(strongest) {
				strongest = state
			}
		}
	}
	switch {
	case serviceCount > 0:
		act.On, act.Tone = true, ToneSuccess
	case agentCount > 0:
		act.On = true
		if strongest == opUnknown || strongest == opIdle {
			act.Tone = ToneMuted
		} else {
			act.Tone = operationalTone(strongest)
		}
		act.Detail = pluralUnits(agentCount, "agent") + " running"
	default:
		return sidebarActivity{}, false
	}
	return act, true
}

// serviceActivityDetail is the tooltip fact for one service:
// "node on :3000, :8080" (ports capped to keep the tooltip readable).
func serviceActivityDetail(act paneServiceActivity) string {
	label := firstText(act.Proc, "service")
	parts := make([]string, 0, len(act.Ports))
	for i, port := range act.Ports {
		if i == 4 {
			parts = append(parts, "…")
			break
		}
		parts = append(parts, ":"+strconv.Itoa(int(port)))
	}
	return label + " on " + strings.Join(parts, ", ")
}

func joinDetails(parts ...string) string {
	joined := make([]string, 0, len(parts))
	for _, part := range parts {
		if part != "" {
			joined = append(joined, part)
		}
	}
	return strings.Join(joined, " · ")
}

func pluralUnits(count int, unit string) string {
	if count == 1 {
		return "1 " + unit
	}
	return strconv.Itoa(count) + " " + unit + "s"
}

// startServiceScan runs the periodic index refresh for the window's
// lifetime; Close cancels it.
func (s *Shell) startServiceScan() {
	if s.serviceScanCancel != nil {
		return
	}
	ctx, cancel := context.WithCancel(context.Background())
	s.serviceScanCancel = cancel
	go func() {
		ticker := time.NewTicker(serviceScanInterval)
		defer ticker.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case <-ticker.C:
				if s.win == nil {
					continue
				}
				s.win.Update(func() { s.kickServiceScan() })
			}
		}
	}()
}

// kickServiceScan launches one scan when the index is stale. UI lane only:
// it snapshots the projection's panes before leaving it.
func (s *Shell) kickServiceScan() {
	if s.win == nil || s.activeInstance == "" || len(s.projection.Panes) == 0 {
		return
	}
	if !s.servicesScannedAt.IsZero() && time.Since(s.servicesScannedAt) < serviceScanMinGap {
		return
	}
	s.servicesScannedAt = time.Now()
	instance := s.activeInstance
	panes := append([]herdr.Pane(nil), s.projection.Panes...)
	generation := s.generation.Load()
	go s.scanPaneServices(instance, panes, generation)
}

// scanPaneServices builds the next index off the UI lane: one listener
// snapshot, then bounded pane.process_info calls, applied only when the
// window still shows the same instance and generation.
func (s *Shell) scanPaneServices(instance string, panes []herdr.Pane, generation uint64) {
	ctx, cancel := context.WithTimeout(context.Background(), serviceScanTimeout)
	defer cancel()

	listeners, err := s.portProbe.ObserveListeners(ctx)
	if err != nil {
		// Without the local socket snapshot there is nothing honest to
		// light up; keep the previous index instead of guessing.
		return
	}

	infos := make(map[string]*herdr.PaneProcessInfo, len(panes))
	sem := make(chan struct{}, serviceScanWorkers)
	var wg sync.WaitGroup
	var mu sync.Mutex
	for _, pane := range panes {
		pane := pane
		wg.Add(1)
		sem <- struct{}{}
		go func() {
			defer wg.Done()
			defer func() { <-sem }()
			info, err := s.runtime.PaneProcessInfo(instance, pane.ID)
			if err != nil {
				return
			}
			mu.Lock()
			infos[pane.ID] = info
			mu.Unlock()
		}()
	}
	wg.Wait()

	next := computeSidebarServices(herdr.Projection{Panes: panes}, listeners, infos)
	if s.win == nil {
		return
	}
	s.win.Update(func() {
		if generation != s.generation.Load() || instance != s.activeInstance {
			return
		}
		s.serviceIndex = next
	})
}
