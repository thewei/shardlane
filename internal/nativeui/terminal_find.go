package nativeui

import (
	"context"
	"log/slog"
	"strings"
	"sync/atomic"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
)

// terminalFindState drives the native Terminal find bar over the live
// Herdr `pane.copy_search` adapter (GWB-010, plan §31). The bar exists only
// on the Terminal surface; it owns the match count and previous/next steps.
type terminalFindState struct {
	open       bool
	query      string
	paneID     string
	terminalID string
	// result presentation
	total    int
	current  int
	busy     bool
	searched bool
	errText  string
	gen      atomic.Uint64
}

// terminalFindBar renders the floating find bar. Hidden surfaces never
// receive it (surface focus ownership, plan §34).
func (s *Shell) terminalFindBar(c *ui.Context) {
	if !s.terminalFind.open || s.surface.current() != WorkspaceSurfaceTerminal {
		return
	}
	t := c.Theme()
	sp := Spacing()

	ui.Row(c).FillWidth().Padding(sp.XS, sp.M).Gap(sp.S).AlignItems(ui.Center).
		Background(designTokens(t.Dark).Panel).BorderWidth(0, 0, 1, 0).
		BorderColor(designTokens(t.Dark).BorderSubtle).Children(func() {
		field := ui.SearchField(c, &s.terminalFind.query).Grow(1).Label("Terminal find query")
		if field.Changed() {
			s.runTerminalFind("forward")
		}
		count := terminalFindCountLabel(&s.terminalFind)
		ui.Text(c, count).FontSize(Typography().Micro).TextColor(t.TextMuted).SingleLine()
		if s.terminalFind.busy {
			ui.Spinner(c).Size(12, 12)
		}
		if ui.Button(c, "‹").Clicked() {
			s.runTerminalFind("backward")
		}
		if ui.Button(c, "›").Clicked() {
			s.runTerminalFind("forward")
		}
		if ui.Button(c, "Done").Clicked() {
			s.terminalFind.open = false
			s.terminalFind.errText = ""
		}
	})
	if s.terminalFind.errText != "" {
		ui.Text(c, s.terminalFind.errText).FontSize(Typography().Micro).TextColor(t.Danger)
	}
}

// terminalFindCountLabel keeps the match counter honest: an empty query has
// not searched anything yet, and a non-empty query that has not completed a
// search shows no claim either (2026-10-06 round five: "no matches" used to
// render before the first keystroke).
func terminalFindCountLabel(state *terminalFindState) string {
	if strings.TrimSpace(state.query) == "" || !state.searched {
		return ""
	}
	if state.total > 0 {
		return itoa(state.current+1) + " of " + itoa(state.total)
	}
	return "no matches"
}

// runTerminalFind executes one CopySearch transaction against the bound
// pane. Errors surface in the bar; no client-side VT shadow index exists.
// A pane that is still streaming rejects the copy with "pane content
// changed"; the search retries once after a short settle delay before it
// reports failure (2026-10-06 F21: live panes were unsearchable).
func (s *Shell) runTerminalFind(direction string) {
	if s.terminalFind.paneID == "" || strings.TrimSpace(s.terminalFind.query) == "" {
		return
	}
	s.terminalFind.busy = true
	gen := s.terminalFind.gen.Add(1)

	session := s.activeInstance
	paneID := s.terminalFind.paneID
	query := s.terminalFind.query
	go func() {
		result, err := s.copySearchOnce(session, paneID, query, direction)
		if err != nil && isPaneContentChanged(err) {
			time.Sleep(700 * time.Millisecond)
			result, err = s.copySearchOnce(session, paneID, query, direction)
		}
		s.applyGuarded(s.terminalFind.gen.Load, gen, func() {
			s.terminalFind.busy = false
			s.terminalFind.searched = true
			if err != nil {
				if isPaneContentChanged(err) {
					slog.Debug("terminal find gave up on streaming pane", "pane", paneID, "err", err)
					s.terminalFind.errText = "The pane is still outputting — wait a moment, then search again."
				} else {
					s.terminalFind.errText = "Search unavailable: " + err.Error()
				}
				return
			}
			s.terminalFind.errText = ""
			if result == nil {
				s.terminalFind.total = 0
				s.terminalFind.current = 0
				return
			}
			s.terminalFind.total = int(result.Total)
			if result.Current != nil {
				s.terminalFind.current = int(*result.Current)
			} else {
				s.terminalFind.current = 0
			}
		})
	}()
}

// copySearchOnce performs one bounded CopySearch RPC.
func (s *Shell) copySearchOnce(session, paneID, query, direction string) (*herdr.PaneCopySearchResult, error) {
	ctx, cancel := context.WithTimeout(context.Background(), TerminalSearchTimeout)
	defer cancel()
	return s.runtime.CopySearch(ctx, session, herdr.PaneCopySearchParams{
		PaneID:    paneID,
		Query:     query,
		Direction: direction,
	})
}

// isPaneContentChanged reports whether the Herdr copy_search rejection was
// the benign live-pane race rather than a real failure.
func isPaneContentChanged(err error) bool {
	return err != nil && strings.Contains(err.Error(), "pane content changed")
}
