package nativeui

import (
	"log/slog"

	"github.com/wh-studio/herdr-client/next/internal/herdr"
)

/**
 * [INPUT]: 依赖 Shell 的 runtime（ScrollPane/SendPaneText）、terminalSurface 的 scrollOffset/scrollMax、herdr.Projection
 * [OUTPUT]: 对外提供 dispatchPaneScroll（pane.scroll 权重视口派发）、queueAgentMouse（pane.send_text 派发）、reconcileSurfaceScroll/prunePaneScrolls/pruneAgentMouse
 * [POS]: nativeui 的滚动权威同步层（F144 真终端改造后）：滚轮不再经 UI，routeAttachMouse 从 attach 流里喂数据，这里只负责去重派发与投影对账
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// Wheel scrolling over an attached pane follows real-terminal semantics
// (2026-10-06 F144, user-approved): the terminal view reports the wheel as
// SGR mouse events through the attach conn (the daemon's forced tracking is
// legitimate now), and routeAttachMouse hands them to the dispatch lanes
// below — pane.scroll for ordinary panes (Herdr owns the viewport; the
// attach stream re-renders the moved viewport and the next projection
// reconciles), pane.send_text for Agent TUI panes (the program tracks the
// mouse; the attach channel drops mouse bytes, probed 2026-10-06).

// paneScrollDispatch serializes pane.scroll mutations for one pane: at most
// one RPC in flight, the newest absolute offset wins, and intermediates are
// safe to drop because offsets are absolute.
type paneScrollDispatch struct {
	pending  int
	queued   bool
	inflight bool
	failed   bool
}

// agentMouseDispatch serializes pane.send_text wheel writes for one pane:
// sequences accumulate while one RPC is in flight, so a wheel burst costs
// one send per round trip and keeps its order.
type agentMouseDispatch struct {
	pending  string
	inflight bool
	failed   bool
}

func (s *Shell) queueAgentMouse(paneID, seq string) {
	d := s.agentMouseDispatchFor(paneID)
	d.pending += seq
	s.pumpAgentMouse(paneID)
}

func (s *Shell) agentMouseDispatchFor(paneID string) *agentMouseDispatch {
	if s.agentMouse == nil {
		s.agentMouse = make(map[string]*agentMouseDispatch)
	}
	d := s.agentMouse[paneID]
	if d == nil {
		d = &agentMouseDispatch{}
		s.agentMouse[paneID] = d
	}
	return d
}

// pruneAgentMouse drops per-pane wheel dispatch state for panes that left
// the visible geometry, unless a send is still in flight.
func (s *Shell) pruneAgentMouse(desired map[string]paneGeometry) {
	live := make(map[string]bool, len(desired))
	for _, geometry := range desired {
		live[geometry.pane.ID] = true
	}
	for paneID, d := range s.agentMouse {
		if !live[paneID] && !d.inflight {
			delete(s.agentMouse, paneID)
		}
	}
}

func (s *Shell) pumpAgentMouse(paneID string) {
	d := s.agentMouse[paneID]
	if d == nil || d.inflight || d.pending == "" {
		return
	}
	if s.agentMouseSink != nil {
		// Test hook replaces the send; drain exactly like production.
		text := d.pending
		d.pending = ""
		s.agentMouseSink(paneID, text)
		return
	}
	if s.win == nil {
		d.pending = ""
		return
	}
	d.inflight = true
	text, instance, generation := d.pending, s.activeInstance, s.generation.Load()
	d.pending = ""
	go func() {
		err := s.runtime.SendPaneText(instance, paneID, text)
		s.win.Update(func() {
			if generation != s.generation.Load() {
				return
			}
			d.inflight = false
			if err != nil {
				// Wheel gestures are high-frequency: record the transition
				// once, never surface a modal per tick.
				if !d.failed {
					slog.Debug("agent mouse send failed", "pane_id", paneID, "error", err)
				}
				d.failed = true
			} else {
				d.failed = false
			}
			s.pumpAgentMouse(paneID)
		})
	}()
}

// dispatchPaneScroll sends the pane's newest scroll offset to Herdr without
// blocking the UI thread. paneScrollSink replaces the mutation in tests.
func (s *Shell) dispatchPaneScroll(paneID string, offset int) {
	if s.paneScrollSink != nil {
		s.paneScrollSink(paneID, offset)
		return
	}
	d := s.paneScrollDispatch(paneID)
	d.pending = offset
	d.queued = true
	s.pumpPaneScroll(paneID)
}

func (s *Shell) paneScrollDispatch(paneID string) *paneScrollDispatch {
	if s.paneScrolls == nil {
		s.paneScrolls = make(map[string]*paneScrollDispatch)
	}
	d := s.paneScrolls[paneID]
	if d == nil {
		d = &paneScrollDispatch{}
		s.paneScrolls[paneID] = d
	}
	return d
}

func (s *Shell) pumpPaneScroll(paneID string) {
	d := s.paneScrolls[paneID]
	if d == nil || !d.queued || d.inflight {
		return
	}
	if s.win == nil {
		d.queued = false
		return
	}
	d.queued = false
	d.inflight = true
	offset, instance, generation := d.pending, s.activeInstance, s.generation.Load()
	go func() {
		err := s.runtime.ScrollPane(instance, paneID, uint64(offset))
		s.win.Update(func() {
			if generation != s.generation.Load() {
				return
			}
			d.inflight = false
			if err != nil {
				// The authoritative viewport is untouched; the next
				// projection reconciles the mirror. Wheel gestures are
				// high-frequency: record the transition once, never surface
				// a modal per tick.
				if !d.failed {
					slog.Debug("pane scroll failed", "pane_id", paneID, "offset", offset, "error", err)
				}
				d.failed = true
			} else {
				d.failed = false
			}
			s.pumpPaneScroll(paneID)
		})
	}()
}

// reconcileSurfaceScroll adopts Herdr's authoritative viewport metrics when
// no mutation is in flight; an in-flight scroll reconciles from the next
// projection after the RPC lands instead of reverting mid-gesture.
func (s *Shell) reconcileSurfaceScroll(surface *terminalSurface, pane herdr.Pane) {
	ps := pane.Scroll
	if ps == nil {
		return
	}
	surface.scrollMax = ps.MaxOffsetFromBottom
	if d := s.paneScrolls[surface.paneID]; d != nil && d.inflight {
		return
	}
	surface.scrollOffset = ps.OffsetFromBottom
}

// prunePaneScrolls drops per-pane dispatch state for panes that left the
// visible geometry, unless a mutation is still in flight.
func (s *Shell) prunePaneScrolls(desired map[string]paneGeometry) {
	live := make(map[string]bool, len(desired))
	for _, geometry := range desired {
		live[geometry.pane.ID] = true
	}
	for paneID, d := range s.paneScrolls {
		if !live[paneID] && !d.inflight {
			delete(s.paneScrolls, paneID)
		}
	}
}
