package nativeui

/**
 * [INPUT]: 依赖只读 HistoryService 的列表/详情状态和共享 MyGo Design Tokens
 * [OUTPUT]: 提供 History 列表、Provider 过滤与有界会话详情内容
 * [POS]: History 的主内容页，仅中间阅读域拥有 transcript；右侧详情交给 context_panel
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"fmt"
	"math"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/history"
)

// distinctAgents returns the agent kinds present in the summaries, in
// first-seen order — the dynamic History filter rail source (F37).
func distinctAgents(summaries []history.SessionSummary) []history.AgentID {
	seen := make(map[history.AgentID]bool, 4)
	out := make([]history.AgentID, 0, 4)
	for _, summary := range summaries {
		id := summary.Meta.Agent
		if id == "" || seen[id] {
			continue
		}
		seen[id] = true
		out = append(out, id)
	}
	return out
}

// historyNav is the History sidebar body (inner-page style): the view
// navigation plus the list filters that used to be the page's own toolbar
// row. Filter intents keep issuing through the same request path.
func (s *Shell) historyNav(c *ui.Context) {
	path := s.router.Path()
	ui.Text(c, "History").FontSize(13).Bold().Padding(6, 8, 8)
	if navButton(c, iconHistory, "All conversations", "", path == routeHistory).Clicked() {
		s.showHistoryIndex()
	}
	if navButton(c, iconFolder, "By Project", "", path == "/history-projects").Clicked() && path != "/history-projects" {
		if strings.HasPrefix(path, routeHistory+"/") {
			s.cancelHistoryDetailRequest()
		}
		s.router.Push("/history-projects")
	}
	// The grouped page and transcript detail don't consume list search,
	// Provider filters, or the list refresh action. Never render a control
	// whose effect cannot be seen in the currently selected page.
	if path != routeHistory {
		ui.Spacer(c)
		return
	}
	sectionLabel(c, "Filter")
	field := ui.SearchField(c, &s.hist.query).Label("Search History").FillWidth()
	if field.Changed() || field.Submitted() {
		s.applyHistoryFilters()
	}
	ui.Text(c, historyResultLabel(s.hist.loading, s.hist.loaded, s.hist.errText, len(s.hist.sessions))).
		FontSize(Typography().Caption).TextColor(c.Theme().TextMuted).Padding(2, 8, 5)
	if navButton(c, nil, "All", "", s.hist.provider == "").Clicked() && s.hist.provider != "" {
		s.hist.provider = ""
		s.applyHistoryFilters()
	}
	for _, id := range s.hist.providers {
		id := id
		if navButton(c, nil, id.DisplayName(), "", s.hist.provider == string(id)).Clicked() && s.hist.provider != string(id) {
			s.hist.provider = string(id)
			s.applyHistoryFilters()
		}
	}
	if historyFiltersActive(s.hist.query, s.hist.provider) {
		if ui.Button(c, "Clear filters").Clicked() {
			s.clearHistoryFilters()
		}
	}
	ui.Spacer(c)
	if navButton(c, iconRefresh, "Refresh", "", false).Clicked() {
		s.refreshHistoryScan()
	}
}

func (s *Shell) historyPage(c *ui.Context) {
	if !s.hist.loaded && !s.hist.loading && s.hist.errText == "" {
		s.requestHistoryList()
	}
	// The sheet is gone (2026-10-06): the page rides in a gorex card on
	// the window gradient.
	ui.Column(c).Grow(1).MinWidth(0).Children(func() {
		pageHeader(c, "History", "Read-only conversations from provider sources, indexed into Shardlane's disposable catalog.")
		if s.hist.errText != "" {
			inlineNotice(c, ToneError, s.hist.errText, func() {
				s.hist.loading = false
				s.hist.errText = ""
				s.requestHistoryList()
			})
		}
		s.historyListBody(c)
	})
}

func (s *Shell) historyListBody(c *ui.Context) {
	switch {
	case s.hist.service == nil:
		emptyState(c, "History is unavailable", "The read-only history catalog could not be opened in this session.")
	case s.hist.loading && len(s.hist.sessions) == 0:
		loadingState(c, "Loading History…")
	case s.hist.errText != "" && len(s.hist.sessions) == 0:
		emptyState(c, "History could not load", "Use Retry above to reload the conversations.")
	case s.hist.loaded && len(s.hist.sessions) == 0 && historyFiltersActive(s.hist.query, s.hist.provider):
		ui.Column(c).Grow(1).Center().Gap(Spacing().M).Children(func() {
			ui.Text(c, "No matching conversations").Bold()
			ui.Text(c, "Try another search or provider.").FontSize(Typography().BodySmall).TextColor(c.Theme().TextMuted)
			if ui.Button(c, "Clear filters").Clicked() {
				s.clearHistoryFilters()
			}
		})
	case s.hist.loaded && len(s.hist.sessions) == 0:
		emptyState(c, "No conversations found", "Conversations appear here once provider history sources are indexed.")
	default:
		sp := Spacing()
		ui.Column(c).Grow(1).Padding(0, sp.XL, sp.L).Children(func() {
			// RENDER-01: one persistent native List owns row construction and
			// keyboard/selection behavior for the bounded result set.
			s.hist.listState.Key = func(row int) any { return s.hist.sessions[row].Meta.Key }
			s.hist.listState.Label = func(row int) string { return s.hist.sessions[row].Meta.Title }
			s.hist.listState.Selected = &s.hist.listSelected
			list := ui.List(c, &s.hist.listState, len(s.hist.sessions), func(i int) {
				s.historyRow(c, s.hist.sessions[i])
			}).Gap(6)
			if list.Changed() && s.hist.listSelected >= 0 && s.hist.listSelected < len(s.hist.sessions) {
				s.router.Push("/history/" + s.hist.sessions[s.hist.listSelected].Meta.Key)
			}
			if s.hist.loading {
				ui.Text(c, "Updating…").FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
			}
		})
	}
}

func (s *Shell) historyRow(c *ui.Context, summary history.SessionSummary) {
	t := c.Theme()
	tokens := designTokens(t.Dark)
	sp := Spacing()
	typ := Typography()
	selected := s.hist.detail != nil && s.hist.detail.key == summary.Meta.Key
	row := ui.ButtonBase(c).FillWidth().Padding(10, sp.L).Gap(10).Radius(Radius().Row).Label(summary.Meta.Title)
	if tint, ok := rowTint(tokens, selected, row.Hovered()); ok {
		row.Background(tint)
	}
	row.Border(1, tokens.BorderSubtle)
	row.Children(func() {
		ui.Column(c).Grow(1).MinWidth(0).Gap(3).Children(func() {
			ui.Row(c).Gap(sp.S).AlignItems(ui.Center).Children(func() {
				ui.Text(c, fallbackText(summary.Meta.Title, history.Untitled)).FontSize(typ.Body + 0.5).FontWeight(600).Grow(1).SingleLine().Tooltip(fallbackText(summary.Meta.Title, history.Untitled))
				providerBadge(c, summary.Meta.Agent)
			})
			// F102: the title is derived from the first user message and so is
			// the catalog description — when one is a prefix of the other the
			// second line read the same text twice. Show only genuinely new
			// description content; the project already has its own metadata row.
			description := historyRowDescription(summary)
			if description != "" {
				ui.Text(c, description).FontSize(typ.BodySmall).TextColor(t.TextMuted).SingleLine().Tooltip(description)
			}
			ui.Row(c).Gap(sp.S).Children(func() {
				if summary.Meta.ProjectName != "" {
					ui.Text(c, summary.Meta.ProjectName).FontSize(typ.Caption).TextColor(t.TextMuted).SingleLine().Tooltip(summary.Meta.ProjectName)
				}
				if stamp := historyTimestamp(summary.Meta.UpdatedAt); stamp != "" {
					ui.Text(c, stamp).FontSize(typ.Caption).TextColor(t.TextMuted)
				}
				ui.Text(c, fmt.Sprintf("%d message%s", summary.Meta.MessageCount, pluralS(int(summary.Meta.MessageCount)))).FontSize(typ.Caption).TextColor(t.TextMuted)
			})
		})
	})
}

// historyFiltersActive indicates a scoped view, not an empty catalog.
func historyFiltersActive(query, provider string) bool {
	return strings.TrimSpace(query) != "" || provider != ""
}

// historyResultLabel describes only the bounded page returned by History,
// never claims a catalog-wide total when the result hits the query limit.
func historyResultLabel(loading, loaded bool, errText string, count int) string {
	switch {
	case loading:
		return "Searching…"
	case errText != "":
		return "Unable to load results"
	case !loaded:
		return "Results not loaded"
	case count == HistoryListLimit:
		return fmt.Sprintf("Latest %d results · Newest first", count)
	case count == 1:
		return "1 result · Newest first"
	default:
		return fmt.Sprintf("%d results · Newest first", count)
	}
}

// historyDetailPage renders the bounded Native transcript window for one
// conversation. Entering or switching conversations starts a bounded open;
// stale requests are dropped by generation and cancelled by context.
func (s *Shell) historyDetailPage(c *ui.Context, id string) {
	if s.hist.detail == nil || s.hist.detail.key != id {
		s.openHistoryConversation(id)
	}
	t := c.Theme()
	tokens := designTokens(t.Dark)
	// The sheet is gone (2026-10-06): the page rides in a gorex card on
	// the window gradient.
	ui.Column(c).Grow(1).MinWidth(0).Children(func() {
		title := "History Detail"
		subtitle := id
		if s.hist.detail != nil && s.hist.detail.meta.Title != "" {
			title = s.hist.detail.meta.Title
			subtitle = strings.Join([]string{s.hist.detail.meta.ProjectName, s.hist.detail.meta.Agent.DisplayName()}, " · ")
		}
		ui.Row(c).Padding(18, 20, 14).Gap(10).AlignItems(ui.Center).
			BorderWidth(0, 0, 1, 0).BorderColor(tokens.BorderSubtle).Children(func() {
			if ui.Button(c, "Back to History").Clicked() {
				s.showHistoryIndex()
			}
			ui.Column(c).Grow(1).MinWidth(0).Gap(2).Children(func() {
				// F47 symmetry: the list rows carry a tooltip; the detail header
				// truncated the same title with no way to read it.
				ui.Text(c, title).FontSize(16).Bold().SingleLine().Tooltip(title)
				ui.Text(c, subtitle).FontSize(10.5).TextColor(t.TextMuted).SingleLine()
			})
		})
		s.historyDetailBody(c)
	})
}

func (s *Shell) historyDetailBody(c *ui.Context) {
	detail := s.hist.detail
	t := c.Theme()
	if detail == nil {
		return
	}
	if detail.loading {
		ui.Column(c).Grow(1).Center().Gap(8).Children(func() {
			ui.Spinner(c).Size(24, 24)
			ui.Text(c, "Loading conversation…").TextColor(t.TextMuted)
		})
		return
	}
	if detail.errText != "" {
		ui.Column(c).Grow(1).Center().Gap(8).Children(func() {
			ui.Text(c, detail.errText).TextColor(t.Danger)
			if ui.Button(c, "Back to History").Clicked() {
				s.showHistoryIndex()
			}
		})
		return
	}
	window := detail.window
	blocks := historyBlocksFromMessages(window.Messages)

	// Consume the scroll intents once per applied window (CLOSURE-03): an
	// anchor jump reveals its message, earlier reveals the window top, later
	// follows the end. Normal user scrolling is untouched.
	pendingAnchor := false
	switch {
	case detail.anchorPending && window.AnchorIndex >= 0 &&
		window.AnchorIndex-window.Start >= 0 && window.AnchorIndex-window.Start < len(blocks):
		pendingAnchor = true
		detail.anchorPending = false
	case detail.scrollToTop:
		detail.scroll.Y = 0
		detail.scrollToTop = false
	case detail.scrollToBot:
		detail.scroll.Y = math.MaxFloat32
		detail.scrollToBot = false
	}

	scroll := ui.Scroll(c).Grow(1).TrackScroll(&detail.scroll)
	scroll.Children(func() {
		ui.Column(c).FillWidth().Padding(14, 20, 16).Gap(10).Children(func() {
			if window.HasEarlier() {
				earlier := window.Start
				if ui.Button(c, fmt.Sprintf("Load %d earlier message%s", earlier, pluralS(earlier))).Clicked() {
					s.pageHistoryDetail(true)
				}
			}
			for _, block := range blocks {
				view := s.historyBlockView(c, block)
				if pendingAnchor && block.Seq == window.Messages[window.AnchorIndex-window.Start].Seq {
					view.ScrollIntoView()
				}
			}
			if window.HasLater() {
				later := window.TotalMessages - window.Start - len(window.Messages)
				if ui.Button(c, fmt.Sprintf("Load %d later message%s", later, pluralS(later))).Clicked() {
					s.pageHistoryDetail(false)
				}
			}
			ui.Text(c, fmt.Sprintf("Showing %d–%d of %d messages (bounded window)",
				window.Start+1, window.Start+len(window.Messages), window.TotalMessages)).
				FontSize(10).TextColor(t.TextMuted)
		})
	})
}

// historyBlockView renders one normalized block with native widgets.
// Thinking renders collapsed behind a disclosure; long text previews end
// with an explicit truncation marker.
func (s *Shell) historyBlockView(c *ui.Context, block historyBlock) ui.Element {
	t := c.Theme()
	tokens := designTokens(t.Dark)
	var view ui.Element
	ui.Box(c).Children(func() {
		view = ui.Column(c).FillWidth().Padding(10, 12).Gap(5).Radius(8).Background(tokens.Panel).Border(1, tokens.BorderSubtle).Children(func() {
			roleLabel := strings.ToUpper(string(block.Role))
			switch block.Kind {
			case historyBlockMeta:
				roleLabel = "SYSTEM"
			case historyBlockCompactSummary:
				roleLabel = "COMPACTED"
			}
			ui.Row(c).Gap(6).AlignItems(ui.Center).Children(func() {
				ui.Text(c, roleLabel).FontSize(9).FontWeight(700).TextColor(t.TextMuted)
				if block.Timestamp != nil && *block.Timestamp > 0 {
					ui.Text(c, historyTimestamp(*block.Timestamp)).FontSize(9).TextColor(t.TextMuted)
				}
			})
			if block.Text != "" {
				// Natural height only: Grow inside this Column collapses
				// multi-line text to a wrong measured height and the message
				// bleeds across neighbouring cards (2026-10-06 F54).
				ui.Text(c, block.Text).FontSize(11.5)
			}
			if block.Truncated {
				ui.Text(c, "Preview truncated — full text stays in the source.").FontSize(9).TextColor(t.TextMuted)
			}
			if block.Thinking != "" {
				// DS-05: the official Collapsible owns the thinking disclosure.
				expanded := s.blockExpanded(block.Seq, "thinking")
				col := ui.Collapsible(c, "Thinking", &expanded, func() {
					ui.Text(c, block.Thinking).FontSize(Typography().BodySmall).TextColor(t.TextMuted)
				})
				col.Changed() // apply a pending toggle now, so the store keeps it
				if expanded != s.blockExpanded(block.Seq, "thinking") {
					s.setBlockExpanded(block.Seq, "thinking", expanded)
				}
			}
			for _, card := range block.Tools {
				toolCard(c, card.Call.Name, card.Call.InputPreview, card.Call.Output, card.Call.IsError)
			}
		})
	})
	return view
}

// blockExpanded reads the per-detail presentation expansion state.
func (s *Shell) blockExpanded(seq int64, part string) bool {
	if s.hist.detail == nil {
		return false
	}
	return s.hist.detail.expanded[expansionKey(seq, part)]
}

func (s *Shell) setBlockExpanded(seq int64, part string, value bool) {
	if s.hist.detail == nil {
		return
	}
	if s.hist.detail.expanded == nil {
		s.hist.detail.expanded = make(map[string]bool)
	}
	s.hist.detail.expanded[expansionKey(seq, part)] = value
}

func expansionKey(seq int64, part string) string {
	return fmt.Sprintf("%d:%s", seq, part)
}

func firstLine(text string) string {
	if line, _, ok := strings.Cut(text, "\n"); ok {
		return line
	}
	return text
}

func historyTimestamp(ms int64) string {
	if ms <= 0 {
		return ""
	}
	return time.UnixMilli(ms).Local().Format("2006-01-02 15:04")
}

// historyRowDescription picks the list-card preview (F102): the catalog
// description duplicates the title whenever both are derived from the first
// user message. In that case omit the preview: Project is already metadata.
func historyRowDescription(summary history.SessionSummary) string {
	title := strings.TrimSpace(summary.Meta.Title)
	description := strings.TrimSpace(summary.Description)
	if description == "" {
		return ""
	}
	if title == "" {
		return description
	}
	shorter, longer := title, description
	if len(shorter) > len(longer) {
		shorter, longer = longer, shorter
	}
	if strings.HasPrefix(longer, shorter) {
		return ""
	}
	return description
}
