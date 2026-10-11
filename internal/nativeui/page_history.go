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
	"github.com/wh-studio/herdr-client/internal/settings"
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

// historyNav is the Sessions sidebar body (N19 #9: History is now the
// Sessions view): the index entry and refresh — search, provider filters,
// the recycle toggle and the count live in the page toolbar.
func (s *Shell) historyNav(c *ui.Context) {
	path := s.router.Path()
	ui.Text(c, "Sessions").FontSize(13).Bold().Padding(6, 8, 8)
	if navButton(c, iconHistory, "All sessions", "", path == routeHistory).Clicked() {
		s.showHistoryIndex()
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
	sp := Spacing()
	k := gorexColorsOf(c.Theme().Dark)
	ui.Column(c).Grow(1).MinWidth(0).Children(func() {
		pageHeader(c, "Sessions", "Read-only conversations from provider sources, grouped by project.")
		if s.hist.errText != "" {
			inlineNotice(c, ToneError, s.hist.errText, func() {
				s.hist.loading = false
				s.hist.errText = ""
				s.requestHistoryList()
			})
		}
		// The toolbar (N19 #9): search, provider filter, the recycle toggle
		// and the count — inside the page, over the grouped project list.
		ui.Row(c).FillWidth().Padding(0, sp.XL, 0).Gap(sp.S).AlignItems(ui.Center).Children(func() {
			field := ui.SearchField(c, &s.hist.query).Label("Search sessions").FillWidth()
			if field.Changed() || field.Submitted() {
				s.applyHistoryFilters()
			}
			if gorexIconButton(c, k, iconTrash, "Recycle bin", "Hidden sessions", s.hist.showHidden, gorexIconBtn, 15).Clicked() {
				s.hist.showHidden = !s.hist.showHidden
			}
			if gorexIconButton(c, k, iconRefresh, "Refresh", "Refresh", false, gorexIconBtn, 15).Clicked() {
				s.refreshHistoryScan()
			}
		})
		groups := s.groupedSessionView()
		shown := 0
		for _, group := range groups {
			shown += len(group.sessions)
		}
		count := fmt.Sprintf("%d sessions", shown)
		if s.hist.showHidden {
			count = fmt.Sprintf("%d hidden", shown)
		}
		ui.Row(c).FillWidth().Padding(0, sp.XL, 0).Gap(sp.S).AlignItems(ui.Center).Children(func() {
			ui.Text(c, count).FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
			chosen := 0
			for i, id := range s.hist.providers {
				if string(id) == s.hist.provider {
					chosen = i + 1
				}
			}
			options := make([]string, 0, len(s.hist.providers)+1)
			options = append(options, "All providers")
			for _, id := range s.hist.providers {
				options = append(options, id.DisplayName())
			}
			if ui.Segmented(c, &chosen, options...).Changed() {
				if chosen == 0 {
					s.hist.provider = ""
				} else if chosen <= len(s.hist.providers) {
					s.hist.provider = string(s.hist.providers[chosen-1])
				}
				s.applyHistoryFilters()
			}
		})
		s.historyListBody(c)
	})
}

func (s *Shell) historyListBody(c *ui.Context) {
	switch {
	case s.hist.service == nil:
		emptyState(c, "Sessions are unavailable", "The read-only history catalog could not be opened in this session.")
	case s.hist.loading && len(s.hist.sessions) == 0:
		loadingState(c, "Loading sessions…")
	case s.hist.errText != "" && len(s.hist.sessions) == 0:
		emptyState(c, "Sessions could not load", "Use Refresh above to reload the conversations.")
	case s.hist.loaded && len(s.hist.sessions) == 0 && historyFiltersActive(s.hist.query, s.hist.provider):
		ui.Column(c).Grow(1).Center().Gap(Spacing().M).Children(func() {
			ui.Text(c, "No matching sessions").Bold()
			ui.Text(c, "Try another search or provider.").FontSize(Typography().BodySmall).TextColor(c.Theme().TextMuted)
			if ui.Button(c, "Clear filters").Clicked() {
				s.clearHistoryFilters()
			}
		})
	case s.hist.loaded && len(s.hist.sessions) == 0:
		emptyState(c, "No sessions yet", "Sessions appear here once provider history sources are indexed.")
	default:
		sp := Spacing()
		ui.Scroll(c).Grow(1).Children(func() {
			ui.Column(c).FillWidth().Padding(0, sp.XL, sp.L).Gap(sp.S).Children(func() {
				for _, group := range s.groupedSessionView() {
					s.sessionGroup(c, group)
				}
				if s.hist.loading {
					ui.Text(c, "Updating…").FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
				}
			})
		})
	}
}

// sessionGroupView is one project's slice of the filtered session list.
type sessionGroupView struct {
	path     string
	name     string
	sessions []history.SessionSummary
}

// groupedSessionView groups the filtered sessions by project path — the
// Sessions view is project-grouped, only (N19 #9) — the group order
// following each project's most recent session.
func (s *Shell) groupedSessionView() []sessionGroupView {
	hidden := s.hiddenSessionSet()
	groups := map[string]*sessionGroupView{}
	order := []string{}
	for _, summary := range s.hist.sessions {
		if summary.Meta.Archived {
			continue
		}
		if s.hist.showHidden != hidden[summary.Meta.Key] {
			continue
		}
		path := summary.Meta.ProjectPath
		if path == "" {
			path = "No project"
		}
		group, seen := groups[path]
		if !seen {
			name := summary.Meta.ProjectName
			if name == "" {
				name = path
			}
			group = &sessionGroupView{path: path, name: name}
			groups[path] = group
			order = append(order, path)
		}
		group.sessions = append(group.sessions, summary)
	}
	view := make([]sessionGroupView, 0, len(order))
	for _, path := range order {
		view = append(view, *groups[path])
	}
	for i := 0; i < len(view); i++ {
		for j := i + 1; j < len(view); j++ {
			if sessionGroupLatest(view[j]) > sessionGroupLatest(view[i]) {
				view[i], view[j] = view[j], view[i]
			}
		}
	}
	return view
}

func sessionGroupLatest(group sessionGroupView) int64 {
	latest := int64(0)
	for _, summary := range group.sessions {
		if summary.Meta.UpdatedAt > latest {
			latest = summary.Meta.UpdatedAt
		}
	}
	return latest
}

// sessionGroup renders one collapsible project group (name, path, count).
func (s *Shell) sessionGroup(c *ui.Context, group sessionGroupView) {
	sp := Spacing()
	tokens := designTokens(c.Theme().Dark)
	collapsed := s.hist.groupCollapsed[group.path]
	header := ui.ButtonBase(c).FillWidth().Padding(8, sp.L).Gap(8).AlignItems(ui.Center).
		Radius(Radius().Row).Border(1, tokens.BorderSubtle).Background(tokens.Panel).
		Label("Project " + group.name)
	header.Children(func() {
		ui.Text(c, map[bool]string{true: "▸", false: "▾"}[collapsed]).FontSize(11).TextColor(c.Theme().TextMuted)
		ui.Text(c, group.name).FontSize(Typography().Body).FontWeight(650).SingleLine().Shrink(0)
		ui.Text(c, group.path).FontSize(Typography().Caption).TextColor(c.Theme().TextMuted).
			SingleLine().Ellipsis("…").Grow(1).MinWidth(0).Tooltip(group.path)
		ui.Text(c, fmt.Sprintf("%d sessions", len(group.sessions))).FontSize(Typography().Caption).TextColor(c.Theme().TextMuted).Shrink(0)
	})
	if header.Clicked() {
		s.hist.groupCollapsed[group.path] = !collapsed
	}
	if collapsed {
		return
	}
	for _, summary := range group.sessions {
		summary := summary
		s.sessionRow(c, summary)
	}
}

// sessionRow renders one session: title strip (expand toggle + view /
// copy-resume / hide-restore actions) and the expanded fact sheet.
func (s *Shell) sessionRow(c *ui.Context, summary history.SessionSummary) {
	sp := Spacing()
	tokens := designTokens(c.Theme().Dark)
	expanded := s.hist.rowExpanded[summary.Meta.Key]
	hidden := s.hiddenSessionSet()[summary.Meta.Key]
	row := ui.Column(c).FillWidth().Padding(6, sp.M, 8).Gap(3).Radius(Radius().Row).
		Border(1, tokens.BorderSubtle).Background(tokens.Content)
	row.Children(func() {
		strip := ui.Row(c).FillWidth().Gap(sp.S).AlignItems(ui.Center)
		strip.Children(func() {
			if ui.Button(c, map[bool]string{true: "▾", false: "▸"}[expanded]).FontSize(Typography().Caption).Clicked() {
				s.hist.rowExpanded[summary.Meta.Key] = !expanded
			}
			providerBadge(c, summary.Meta.Agent)
			ui.Text(c, fallbackText(summary.Meta.Title, history.Untitled)).
				FontSize(Typography().BodySmall).FontWeight(600).Grow(1).MinWidth(0).
				SingleLine().Ellipsis("…").Tooltip(fallbackText(summary.Meta.Title, history.Untitled))
			if stamp := historyTimestamp(summary.Meta.UpdatedAt); stamp != "" {
				ui.Text(c, stamp).FontSize(Typography().Caption).TextColor(c.Theme().TextMuted).Shrink(0)
			}
			if ui.Button(c, "View").FontSize(Typography().Caption).Clicked() {
				s.router.Push("/history/" + summary.Meta.Key)
			}
			if ui.Button(c, "Copy resume").FontSize(Typography().Caption).Clicked() {
				s.copySessionResume(summary.Meta)
			}
			label := "Hide"
			if hidden {
				label = "Restore"
			}
			if ui.Button(c, label).FontSize(Typography().Caption).Clicked() {
				s.toggleSessionHidden(summary.Meta.Key)
			}
		})
		meta := ui.Row(c).FillWidth().Gap(sp.S).AlignItems(ui.Center).Padding(0, 0, 0, 22)
		meta.Children(func() {
			if summary.Meta.Model != nil && *summary.Meta.Model != "" {
				ui.Text(c, *summary.Meta.Model).FontSize(Typography().Caption).TextColor(c.Theme().TextMuted).SingleLine()
			}
			ui.Text(c, fmt.Sprintf("%d message%s", summary.Meta.MessageCount, pluralS(int(summary.Meta.MessageCount)))).
				FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
			if summary.Meta.TokensUsed != nil && *summary.Meta.TokensUsed > 0 {
				ui.Text(c, fmt.Sprintf("%d tokens", *summary.Meta.TokensUsed)).FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
			}
		})
		if expanded {
			s.sessionFacts(c, summary)
		}
	})
}

// sessionFacts is the expanded fact sheet: time range, folder, session id,
// resume command and the provider source file (the screenshot's 继续条目).
func (s *Shell) sessionFacts(c *ui.Context, summary history.SessionSummary) {
	sp := Spacing()
	ui.Column(c).FillWidth().Padding(2, sp.M, 4, 22).Gap(3).Children(func() {
		s.sessionFactRow(c, "Time", sessionTimeRange(summary.Meta))
		s.sessionFactRow(c, "Folder", summary.Meta.ProjectPath)
		s.sessionFactRow(c, "Session ID", summary.Meta.ID)
		s.sessionFactRow(c, "Resume", sessionResumeCommand(summary.Meta))
		s.sessionFactRow(c, "File", summary.Meta.FilePath)
		ui.Row(c).FillWidth().Gap(sp.S).Padding(0, 0, 2).Children(func() {
			if ui.Button(c, "View session").FontSize(Typography().Caption).Clicked() {
				s.router.Push("/history/" + summary.Meta.Key)
			}
			if ui.Button(c, "Copy resume command").FontSize(Typography().Caption).Clicked() {
				s.copySessionResume(summary.Meta)
			}
		})
	})
}

// sessionFactRow renders one label/value line with a copy for long values.
func (s *Shell) sessionFactRow(c *ui.Context, label, value string) {
	if value == "" {
		return
	}
	sp := Spacing()
	ui.Row(c).FillWidth().Gap(sp.S).AlignItems(ui.Center).Children(func() {
		ui.Text(c, label).FontSize(Typography().Caption).TextColor(c.Theme().TextMuted).
			Width(72).Shrink(0)
		ui.Text(c, value).FontSize(Typography().Caption).Grow(1).MinWidth(0).
			SingleLine().Ellipsis("…").Tooltip(value)
		if len(value) > 24 {
			if ui.Button(c, "Copy").FontSize(Typography().Caption).Clicked() {
				s.copyToClipboard(value)
			}
		}
	})
}

// sessionTimeRange formats created→updated when they differ.
func sessionTimeRange(meta history.SessionMeta) string {
	created := historyTimestamp(meta.CreatedAt)
	updated := historyTimestamp(meta.UpdatedAt)
	switch {
	case created == "":
		return updated
	case created == updated:
		return created
	default:
		return created + " → " + updated
	}
}

// sessionResumeCommand builds the provider's own resume invocation prefixed
// with the project folder — the copyable "继续" command.
func sessionResumeCommand(meta history.SessionMeta) string {
	cd := ""
	if meta.ProjectPath != "" {
		cd = fmt.Sprintf("cd %q && ", meta.ProjectPath)
	}
	switch meta.Agent {
	case history.AgentClaudeCode:
		return cd + fmt.Sprintf("claude --resume %s", meta.ID)
	case history.AgentCodex:
		return cd + fmt.Sprintf("codex resume %s", meta.ID)
	case history.AgentPi:
		return cd + fmt.Sprintf("pi --resume %s", meta.ID)
	default:
		return cd + fmt.Sprintf("# no known resume command for %s (session %s)", meta.Agent, meta.ID)
	}
}

// hiddenSessionSet reads the local recycle list as a set.
func (s *Shell) hiddenSessionSet() map[string]bool {
	hidden := map[string]bool{}
	for _, key := range s.settings.Session.HiddenSessions {
		hidden[key] = true
	}
	return hidden
}

// toggleSessionHidden hides or restores one session locally — provider
// files are never touched (the history catalog is read-only).
func (s *Shell) toggleSessionHidden(key string) {
	s.applySettings(func(v *settings.Settings) error {
		next := make([]string, 0, len(v.Session.HiddenSessions)+1)
		for _, existing := range v.Session.HiddenSessions {
			if existing != key {
				next = append(next, existing)
			}
		}
		if !s.hiddenSessionSet()[key] {
			if len(next) < 500 {
				next = append(next, key)
			}
		}
		v.Session.HiddenSessions = next
		return nil
	})
}

// copySessionResume copies the resume command for one session.
func (s *Shell) copySessionResume(meta history.SessionMeta) {
	s.copyToClipboard(sessionResumeCommand(meta))
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
