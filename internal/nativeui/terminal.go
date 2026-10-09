package nativeui

import (
	"fmt"
	"log/slog"
	"time"

	"github.com/egoist/mygo/plugins/terminal"
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
	"github.com/wh-studio/herdr-client/internal/settings"
)

// terminalOptionsFromSettings maps persisted presentation preferences onto the
// official MyGo terminal options. It never carries Herdr terminal identity or
// runtime state; Herdr owns the PTY, scrollback semantics and process.
func terminalOptionsFromSettings(value settings.TerminalSettings) terminal.Options {
	return terminal.Options{
		Font: terminal.Font{
			Family:     fontStack(value.FontFamily),
			Size:       float32(value.FontSize),
			LineHeight: float32(value.LineHeight),
		},
		Scrollback:  value.Scrollback,
		OptionAsAlt: value.OptionAsAlt,
		Transparent: value.Transparent,
	}
}

// LocalDragSelect is a per-pane decision, not a setting: panes whose
// content lives in Herdr's scrollback (shells, scrollback-backed TUIs
// like agy) keep the primary-button drag and the right click local to the
// view — plain-drag text selection and a right-click menu — while the
// wheel still reports (routeAttachMouse moves Herdr's viewport). True
// alt-screen Agent TUIs (pi; no scrollback range) keep full reporting:
// their wheel must reach the program, and selection is Shift+drag.
func localDragSelectFor(geometry paneGeometry) bool {
	if geometry.pane.Agent != "" {
		return geometry.pane.Scroll != nil && geometry.pane.Scroll.MaxOffsetFromBottom > 0
	}
	return true
}

// fontStack keeps one user-chosen family first and proven monospace
// fallbacks behind it. The Nerd Font families only serve glyphs the
// earlier fonts lack — the Private Use Area icons CLIs like eza and lsd
// print, which the system cascade never resolves — and are skipped when
// not installed.
func fontStack(family string) string {
	return family + ", Menlo, Cascadia Mono, JetBrainsMono Nerd Font Mono, SauceCodePro Nerd Font Mono, Hack Nerd Font Mono, Symbols Nerd Font, monospace"
}

type terminalSurface struct {
	key     string
	paneID  string
	label   string
	term    *terminal.Terminal
	area    herdr.LayoutRect
	rect    herdr.LayoutRect
	focused bool
	// dragSelect mirrors the LocalDragSelect option the surface's terminal
	// was created with; syncTerminals reattaches when the classification
	// flips (fresh Agent panes gain their scroll metrics after first
	// output). The view reads the option per pointer event, but the
	// Terminal's opts are fixed at New.
	dragSelect bool
	// cwd, agent and agentStatus feed the gorex pane card header; all are
	// presentation snapshots from the last projection. agent is Herdr's
	// own classification of the pane's Agent TUI (pi and friends): it
	// routes the wheel, not just the label.
	cwd         string
	agent       string
	agentStatus string
	// scrollOffset mirrors the Herdr viewport offset this window last asked
	// for (pane.scroll is absolute); scrollMax is the runtime's clamp.
	// reconcileSurfaceScroll keeps both honest from projections, and
	// routeAttachMouse steps the offset per reported wheel event.
	scrollOffset uint64
	scrollMax    uint64
}

type paneGeometry struct {
	pane    herdr.Pane
	area    herdr.LayoutRect
	rect    herdr.LayoutRect
	focused bool
}

// terminalCanvas lays the visible Panes out over the workspace canvas as
// gorex cards. The geometry is computed in DIPs from the canvas bounds —
// percent frames cannot carry the gutter reliably, and the daemon's pane
// rects are contiguous cells. Each pane frame is inset by half a
// gorexGap on every side (gorexFrameInset), so edges land a full gorexGap
// out (canvas pad + frame inset) and adjacent cards keep a full gorexGap
// between them. The card itself must NOT carry a Margin: a filled child
// overflows its box by the margin (mygo fills the parent's content box
// and shifts, it does not shrink), which painted every card through the
// right/bottom gutter and glued adjacent cards together (2026-10-07
// spacing report, pixel-measured).
func (s *Shell) terminalCanvas(c *ui.Context) {
	k := gorexColorsOf(c.Theme().Dark)
	canvas := ui.Box(c).Grow(1).MinWidth(0).MinHeight(0)
	bounds := canvas.Bounds()
	s.terminalCanvasBounds = bounds
	surfaces := s.visibleSurfaces()
	// Absolute children position relative to the canvas, while Bounds()
	// reports window coordinates: only the size carries over.
	inner := gorexInsetRect(ui.Rect{W: bounds.W, H: bounds.H}, gorexCardsPad)
	if inner.W <= 0 || inner.H <= 0 {
		return
	}
	canvas.Children(func() {
		for _, surface := range surfaces {
			surface := surface
			left, top, width, height := surfacePercent(surface.area, surface.rect)
			w := inner.W*width/100 - 2*gorexFrameInset
			h := inner.H*height/100 - 2*gorexFrameInset
			if w <= 0 || h <= 0 {
				continue
			}
			frame := ui.Box(c.Key(surface.key)).
				Absolute().
				Left(inner.X + inner.W*left/100 + gorexFrameInset).
				Top(inner.Y + inner.H*top/100 + gorexFrameInset).
				Width(w).
				Height(h)
			frame.Children(func() {
				s.paneCard(c, k, surface)
			})
		}
	})
}

// gorexInsetRect shrinks r by d on every side (DIPs).
func gorexInsetRect(r ui.Rect, d float32) ui.Rect {
	return ui.Rect{X: r.X + d, Y: r.Y + d, W: r.W - 2*d, H: r.H - 2*d}
}

// paneCard draws one attached Herdr Pane as a gorex card: a rounded,
// translucent surface with a hairline border and a soft double shadow —
// a notch stronger when the Pane is the selected one — the pane header
// over its terminal. The gutter lives in the frame (see terminalCanvas);
// the card fills its frame exactly.
func (s *Shell) paneCard(c *ui.Context, k *gorexColors, surface *terminalSurface) {
	focused := surface.focused
	card := ui.Column(c).Fill().Radius(gorexCardR).Clip().MinWidth(0).MinHeight(0)
	// The card surface is the opaque paper (gorex cardFocused) on both
	// states: the translucent fill made the card edge melt into the pale
	// gradient, and the gutter to the right panel read as zero (2026-10-06
	// terminal-view spacing report). Focus still shows through a stronger
	// border and shadow. The Transparent-terminal preference swaps in a
	// deliberately see-through paper so the window gradient shows through
	// the undrawn terminal background — the user's opt-in look.
	bg, border, shadow := k.cardFocused, k.cardBorder, k.shadow
	if s.settings.Terminal.Transparent {
		bg = k.cardTranslucent
	}
	if focused {
		border, shadow = k.cardBorderFocused, k.shadowFocused
	}
	card.Background(bg).Border(1, border).
		Shadow(0, 1, 2, 0, shadow).
		Shadow(0, 6, 22, -2, shadow)
	card.Transition(ui.ElementTransition{Colors: true, Duration: 160 * time.Millisecond})
	hovered := card.Hovered()
	card.Children(func() {
		s.paneCardHeader(c, k, surface, focused, hovered)
		body := ui.Box(c).Grow(1).MinHeight(0).Padding(0, 5, 6, 5)
		body.Children(func() {
			// Real-terminal semantics (2026-10-06 F144, user-approved): the
			// view owns every pointer event — no overlay child. With the
			// daemon's forced tracking intact (mouseModeFilter retired) it
			// reports clicks, wheel and drags back through the conn, where
			// routeAttachMouse gives them Herdr's meanings; Shift+drag
			// selects locally (⌘C copies), and plain input forwards exactly
			// like any terminal hosting Herdr. Right click is a program
			// event; the pane menu lives on the card header, the sidebar
			// rows and the title-bar ⋯ menu.
			view := terminal.View(c, surface.term).Fill()
			if surface.focused {
				view.AutoFocus()
			}
			if view.Focused() && surface.paneID != s.selectedPaneID {
				s.selectPane(surface.paneID)
			}
		})
	})
}

// paneCardHeader is a gorex pane header: the terminal glyph, the Pane's
// label and working directory, its Agent's status badge, and — focused or
// hovered — the pane action buttons. The buttons call the same shared
// actions as the pane menu (splitPaneAction and friends), never a second
// implementation.
func (s *Shell) paneCardHeader(c *ui.Context, k *gorexColors, surface *terminalSurface, focused, hovered bool) {
	h := ui.Row(c).Height(gorexHeaderH).Padding(0, gorexGap, 0, 12).Gap(7).AlignItems(ui.Center).MinWidth(0)
	// The pane menu lives here since the F144 real-terminal round: the
	// terminal surface's right click is a program event now, so the card
	// header is the pointer-owned pane-menu affordance (with the sidebar
	// rows and the title-bar ⋯ menu; one paneOverflowItems builder).
	h.ContextMenu(func(m *ui.Menu) {
		s.terminalPaneMenu(c, m, surface)
	})
	if h.DoubleClicked() {
		s.selectPane(surface.paneID)
		s.zoomPaneAction(surface.paneID)
	} else if h.Clicked() {
		s.selectPane(surface.paneID)
	}
	h.Children(func() {
		ui.Icon(c, iconTerminal).Size(14.5, 14.5).TextColor(k.text).Shrink(0)
		ui.Row(c).Grow(1).MinWidth(0).Gap(5).AlignItems(ui.Center).ClipX().Children(func() {
			label := surface.label
			if label == "" {
				label = "Terminal"
			}
			ui.Text(c, label).FontSize(12.5).FontWeight(600).TextColor(k.text).
				SingleLine().Ellipsis("…").Shrink(0).MaxWidthPercent(80)
			if cwd := tildePath(surface.cwd); cwd != "" {
				ui.Text(c, cwd).FontSize(12.5).FontWeight(500).TextColor(k.text.Alpha(0.86)).
					SingleLine().Ellipsis("…").Shrink(1).MinWidth(0)
			}
			s.paneStatusDot(c, k, surface)
			// The pane menu trigger sits against the title (2026-10-06 F148,
			// user request): the terminal surface's right click is a program
			// event now, so the pane menu needs a visible, always-there home
			// on the pane itself — same paneOverflowItems builder as the
			// sidebar rows and the title-bar ⋯ menu, never a second menu.
			// Always rendered (not hover-gated): a menu anchor that vanishes
			// when the pointer enters the dropdown would be a trap.
			more := gorexIconButton(c, k, iconEllipsis, "Pane menu", "Pane menu", false, 22, 13)
			more.Menu(func(m *ui.Menu) {
				s.terminalPaneMenu(c, m, surface)
			})
		})
		show := focused || hovered
		ui.Row(c).Gap(1).AlignItems(ui.Center).Shrink(0).
			Opacity(map[bool]float32{true: 1, false: 0}[show]).Children(func() {
			if !show {
				return
			}
			if gorexIconButton(c, k, iconSplit, "Split Right", "Split Right", false, 26, 15).Clicked() {
				s.selectPane(surface.paneID)
				s.splitPaneAction(surface.paneID, "right")
			}
			if gorexIconButton(c, k, iconSplitDown, "Split Down", "Split Down", false, 26, 15).Clicked() {
				s.selectPane(surface.paneID)
				s.splitPaneAction(surface.paneID, "down")
			}
			if gorexIconButton(c, k, iconZoom, "Zoom / Unzoom", "Zoom / Unzoom", false, 26, 15).Clicked() {
				s.selectPane(surface.paneID)
				s.zoomPaneAction(surface.paneID)
			}
			if gorexIconButton(c, k, iconClose, "Close Pane", "Close Pane", false, 26, 15).Clicked() {
				s.closePaneAction(surface.paneID)
			}
		})
	})
}

// paneStatusDot mirrors the Pane's Agent state as a gorex badge: a halo
// dot while it works or is ready for review, a plain dot when it needs
// someone. Panes without an Agent show nothing.
func (s *Shell) paneStatusDot(c *ui.Context, k *gorexColors, surface *terminalSurface) {
	if surface.agentStatus == "" {
		return
	}
	switch normalizeRuntimeStatus(surface.agentStatus) {
	case opWorking:
		gorexActivityDot(c, k.busy, 6).Tooltip("Working")
	case opReadyForReview:
		gorexActivityDot(c, k.busy, 6).Tooltip("Ready for review")
	case opNeedsAttention:
		gorexAttentionDot(c, k.attention, "Needs attention")
	}
}

func (s *Shell) syncTerminals(projection herdr.Projection) error {
	started := time.Now()
	spawned, closed := 0, 0
	desired := visiblePaneGeometry(projection, s.selectedTabID, s.selectedPaneID)
	for key, surface := range s.terminals {
		if _, ok := desired[key]; !ok {
			slog.Debug("detach native terminal", "instance", s.activeInstance, "pane_id", surface.paneID, "terminal_id", key)
			_ = surface.term.Close()
			delete(s.terminals, key)
			closed++
		}
	}
	for key, geometry := range desired {
		if surface := s.terminals[key]; surface != nil {
			if want := localDragSelectFor(geometry); want != surface.dragSelect {
				// The classification flipped (fresh Agent pane gained its
				// scroll metrics, or its scrollback emptied): the option is
				// fixed at New, so reattach once rather than behaving
				// wrongly until the next natural reattach.
				_ = surface.term.Close()
				delete(s.terminals, key)
			} else {
				surface.area = geometry.area
				surface.rect = geometry.rect
				surface.focused = geometry.focused
				surface.label = geometry.pane.Label
				surface.cwd = geometry.pane.CWD
				surface.agent = geometry.pane.Agent
				surface.agentStatus = geometry.pane.AgentStatus
				s.reconcileSurfaceScroll(surface, geometry.pane)
				continue
			}
		}
		if s.win == nil {
			// Headless (tests): there is no surface to host an attachment,
			// and spawning a real `herdr terminal attach` child from a unit
			// test would leak processes into the developer's runtime.
			continue
		}
		command, err := s.runtime.TerminalAttachCommand(s.activeInstance, geometry.pane.TerminalID)
		if err != nil {
			return err
		}
		slog.Info("attach native terminal", "instance", s.activeInstance, "pane_id", geometry.pane.ID, "terminal_id", geometry.pane.TerminalID)
		options := terminalOptionsFromSettings(s.settings.Terminal)
		options.LocalDragSelect = localDragSelectFor(geometry)
		// The gorex palettes (2026-10-06): soft ink on the panes' paper.
		applyGorexTerminalTheme(&options, s.settings.General.Appearance)
		// Prefer the app-owned pty Conn: it carries the real-terminal mouse
		// semantics (F144) — Write routes reported SGR pointer events into
		// routeAttachMouse. Without a local pty, fall back to the plugin's
		// own spawn (no router: the daemon drops reported events, the view
		// still Shift+drag-selects locally).
		var conn *attachConn
		if c, err := newAttachConn(command, s.activeInstance, geometry.pane.ID, geometry.pane.TerminalID); err == nil {
			conn = c
			options.Conn = conn
			options.Command = nil
		} else {
			slog.Warn("native attach pty unavailable; unfiltered attach", "error", err)
			options.Command = command
		}
		surface := &terminalSurface{
			key:         key,
			paneID:      geometry.pane.ID,
			label:       geometry.pane.Label,
			area:        geometry.area,
			rect:        geometry.rect,
			focused:     geometry.focused,
			cwd:         geometry.pane.CWD,
			agent:       geometry.pane.Agent,
			agentStatus: geometry.pane.AgentStatus,
			dragSelect:  options.LocalDragSelect,
		}
		if conn != nil {
			conn.onMouse = func(seq string) {
				s.applyOnWindow(func() { s.routeAttachMouse(surface, seq) })
			}
		}
		term, err := terminal.New(options)
		if err != nil {
			return fmt.Errorf("open Pane %s terminal: %w", geometry.pane.ID, err)
		}
		surface.term = term
		if ps := geometry.pane.Scroll; ps != nil {
			surface.scrollOffset = ps.OffsetFromBottom
			surface.scrollMax = ps.MaxOffsetFromBottom
		}
		s.terminals[key] = surface
		spawned++
	}
	s.prunePaneScrolls(desired)
	s.pruneAgentMouse(desired)
	// Attachment churn is the switching-lag suspect; one bounded line per
	// churn sync (timings per attach come from attachFrameTiming).
	if spawned > 0 || closed > 0 {
		slog.Info("native terminals synced", "instance", s.activeInstance, "spawned", spawned, "closed", closed, "duration_ms", time.Since(started).Milliseconds())
	}
	return nil
}

func visiblePaneGeometry(projection herdr.Projection, selectedTabID, selectedPaneID string) map[string]paneGeometry {
	result := make(map[string]paneGeometry)
	paneByID := make(map[string]herdr.Pane, len(projection.Panes))
	for _, pane := range projection.Panes {
		paneByID[pane.ID] = pane
	}
	layout, ok := layoutForTab(projection, selectedTabID)
	if ok && len(layout.Panes) > 0 && layout.Area.Width > 0 && layout.Area.Height > 0 {
		if layout.Zoomed {
			if pane, exists := paneByID[layout.FocusedPaneID]; exists && pane.TerminalID != "" {
				key := pane.TerminalID
				result[key] = paneGeometry{pane: pane, area: layout.Area, rect: layout.Area, focused: true}
			}
			return result
		}
		for _, lp := range layout.Panes {
			pane, exists := paneByID[lp.PaneID]
			if !exists || pane.TerminalID == "" {
				continue
			}
			key := pane.TerminalID
			result[key] = paneGeometry{
				pane: pane, area: layout.Area, rect: lp.Rect,
				focused: pane.ID == selectedPaneID,
			}
		}
		return result
	}
	if pane, exists := paneByID[selectedPaneID]; exists && pane.TerminalID != "" && pane.TabID == selectedTabID {
		unit := herdr.LayoutRect{Width: 1, Height: 1}
		result[pane.TerminalID] = paneGeometry{pane: pane, area: unit, rect: unit, focused: true}
	}
	return result
}

func (s *Shell) visibleSurfaces() []*terminalSurface {
	visible := make([]*terminalSurface, 0, len(s.terminals))
	for _, surface := range s.terminals {
		visible = append(visible, surface)
	}
	return visible
}

func surfacePercent(area, rect herdr.LayoutRect) (left, top, width, height float32) {
	if area.Width == 0 || area.Height == 0 {
		return 0, 0, 100, 100
	}
	left = float32(int(rect.X)-int(area.X)) / float32(area.Width) * 100
	top = float32(int(rect.Y)-int(area.Y)) / float32(area.Height) * 100
	width = float32(rect.Width) / float32(area.Width) * 100
	height = float32(rect.Height) / float32(area.Height) * 100
	return
}

func (s *Shell) closeAllTerminals() {
	if len(s.terminals) > 0 {
		slog.Debug("close native terminal attachments", "instance", s.activeInstance, "count", len(s.terminals))
	}
	for key, surface := range s.terminals {
		_ = surface.term.Close()
		delete(s.terminals, key)
	}
}
