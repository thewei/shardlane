package nativeui

/**
 * [INPUT]: 依赖 sessiontrash.Store（经 SessionTrashView 接缝）、HistoryService.Forget/Scan、Sessions 列表/选择状态与 Herdr 投影里 Agent 的会话身份
 * [OUTPUT]: 提供 Sessions 回收站：移入（带撤销提示）、放回、永久删除/清空（二次确认）、回收站视图的行/计数/筛选，以及对运行中 Agent 会话与刚写入会话的保护
 * [POS]: Sessions 页的删除层；只经 internal/sessiontrash 搬动 Provider 会话文件，不新增 Herdr 方法；日志只记操作名、数量与稳定 id，从不记标题或正文
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"path/filepath"
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/history"
	"github.com/wh-studio/herdr-client/internal/sessiontrash"
)

// SessionTrashView is the slice of the Sessions Trash the shell consumes.
// The narrow seam keeps the presentation testable with deterministic fakes.
type SessionTrashView interface {
	Trash(meta history.SessionMeta) (sessiontrash.Item, error)
	Restore(id string) (sessiontrash.Item, error)
	Delete(id string) error
	Empty() (int, error)
	List() ([]sessiontrash.Item, error)
}

// historyForgetter is the optional service seam that drops catalog rows of
// sessions whose files just left the provider roots; the production
// HistoryService has it, plain fakes do not need it.
type historyForgetter interface{ Forget(keys ...string) int }

// toastAction is a toast with one button — Undo after a delete.
type toastAction struct {
	message string
	label   string
	action  func()
}

const (
	deleteForeverConfirm = "history-delete-forever"
	emptyTrashConfirm    = "history-empty-trash"
	confirmIDSeparator   = "\n"
)

// requestTrashList reads the Trash on the background lane; the newest request
// wins, so a stale listing can never overwrite a newer one.
func (s *Shell) requestTrashList() {
	store := s.hist.trash
	if store == nil {
		return
	}
	s.hist.trashRequested = true
	generation := s.hist.trashGen.Add(1)
	s.dispatch(func() {
		items, err := store.List()
		s.applyOnWindow(func() {
			if generation != s.hist.trashGen.Load() {
				return
			}
			s.hist.trashLoaded = true
			if err != nil {
				slog.Warn("list session trash failed", "error", err)
				return
			}
			s.hist.trashItems = items
		})
	})
}

// trashSummary projects one Trash entry onto the list row the Sessions page
// draws: the row key is the entry id (what Put Back and Delete Forever take),
// the date column shows when it was deleted, and an entry whose file cannot
// be read still gets a row named after its file.
func trashSummary(item sessiontrash.Item) history.SessionSummary {
	summary := item.Summary
	if summary.Meta.Key == "" {
		summary.Meta = history.SessionMeta{
			Agent:     item.Agent,
			Title:     strings.TrimSuffix(filepath.Base(item.File), ".jsonl"),
			SizeBytes: item.Size,
		}
	}
	summary.Meta.Key = item.ID
	summary.Meta.FilePath = item.File
	summary.Meta.UpdatedAt = item.DeletedAt.UnixMilli()
	return summary
}

// trashRows is the Trash view's list: the entries that pass the Agent tab and
// the search field, newest deletion first.
func (s *Shell) trashRows() []history.SessionSummary {
	query := strings.ToLower(strings.TrimSpace(s.hist.query))
	rows := make([]history.SessionSummary, 0, len(s.hist.trashItems))
	for _, item := range s.hist.trashItems {
		if s.hist.provider != "" && string(item.Agent) != s.hist.provider {
			continue
		}
		row := trashSummary(item)
		if query != "" && !trashRowMatches(row.Meta, query) {
			continue
		}
		rows = append(rows, row)
	}
	return rows
}

// trashRowMatches is the Sessions search over one Trash row: title, project,
// path, session id and Agent, case-insensitive — what the catalog search
// covers for live sessions.
func trashRowMatches(meta history.SessionMeta, lowerQuery string) bool {
	for _, field := range []string{meta.Title, meta.ProjectName, meta.ProjectPath, meta.ID, string(meta.Agent), meta.Agent.DisplayName()} {
		if strings.Contains(strings.ToLower(field), lowerQuery) {
			return true
		}
	}
	return false
}

// historyProviders is the Agent tab set of the view being shown: the
// catalog's Providers, or — in the Trash — those with something in it.
func (s *Shell) historyProviders() []history.AgentID {
	if !s.hist.showTrash {
		return s.hist.providers
	}
	seen := make(map[history.AgentID]bool, 4)
	var providers []history.AgentID
	for _, item := range s.hist.trashItems {
		if !seen[item.Agent] {
			seen[item.Agent] = true
			providers = append(providers, item.Agent)
		}
	}
	return providers
}

// setHistoryTrashView switches the page between the sessions and the Trash.
// The Agent tab resets (the two views list different Providers) and a
// selection never outlives the view it was made in.
func (s *Shell) setHistoryTrashView(on bool) {
	if s.hist.showTrash == on {
		return
	}
	s.hist.showTrash = on
	s.hist.selecting = false
	s.hist.provider = ""
	if on {
		s.requestTrashList()
	}
	s.applyHistoryFilters()
}

// sessionInUse reports whether a running Agent (as Herdr measured it) is
// working in this very session: its provider session identity names the
// file, or the native id. Such a session is never moved.
func (s *Shell) sessionInUse(meta history.SessionMeta) bool {
	for _, live := range s.projection.Agents {
		identity := live.AgentSession
		if identity == nil {
			continue
		}
		provider, ok := history.ParseAgentID(identity.Agent)
		if !ok || provider != meta.Agent {
			continue
		}
		locator := history.ResolveSessionSourceLocator(provider, identity.Kind, identity.Source, identity.Value)
		switch locator.Kind {
		case history.LocatorFilePath:
			if filepath.Clean(locator.Identity) == filepath.Clean(meta.FilePath) {
				return true
			}
		case history.LocatorNativeID:
			if locator.Identity == meta.ID {
				return true
			}
		}
	}
	return false
}

// sessionMetasForKeys resolves session keys against the loaded list, in the
// order given; keys the list no longer holds are dropped.
func (s *Shell) sessionMetasForKeys(keys []string) []history.SessionMeta {
	byKey := make(map[string]history.SessionMeta, len(s.hist.sessions))
	for _, summary := range s.hist.sessions {
		byKey[summary.Meta.Key] = summary.Meta
	}
	metas := make([]history.SessionMeta, 0, len(keys))
	for _, key := range keys {
		if meta, ok := byKey[key]; ok {
			metas = append(metas, meta)
		}
	}
	return metas
}

// moveSessionsToTrash deletes sessions the reversible way: their files move
// into the Trash on the background lane, the rows leave the list at once, and
// the toast offers Undo. A session an Agent is working in, or that was just
// written to, stays where it is and is counted in the toast.
func (s *Shell) moveSessionsToTrash(metas []history.SessionMeta) {
	store := s.hist.trash
	if store == nil || len(metas) == 0 {
		return
	}
	movable := make([]history.SessionMeta, 0, len(metas))
	stillInUse := 0
	for _, meta := range metas {
		if s.sessionInUse(meta) {
			stillInUse++
			continue
		}
		movable = append(movable, meta)
	}
	if len(movable) == 0 {
		s.pendingToast = trashRefusalText(stillInUse, 0)
		return
	}
	s.dispatch(func() {
		moved := make([]sessiontrash.Item, 0, len(movable))
		keys := make([]string, 0, len(movable))
		busy, failed := 0, 0
		for _, meta := range movable {
			item, err := store.Trash(meta)
			switch {
			case err == nil:
				moved = append(moved, item)
				keys = append(keys, meta.Key)
			case errors.Is(err, sessiontrash.ErrBusy):
				busy++
			default:
				failed++
				slog.Warn("move session to trash failed", "session", meta.Key, "error", err)
			}
		}
		slog.Info("sessions moved to trash", "moved", len(moved), "in_use", stillInUse+busy, "failed", failed)
		s.applyOnWindow(func() {
			if len(moved) > 0 {
				s.afterSessionsTrashed(keys)
			}
			s.announceTrashed(moved, stillInUse+busy, failed)
		})
	})
}

// afterSessionsTrashed brings the page in line with files that just left the
// provider roots: catalog rows dropped, the list and counts reloaded, and
// anything that was showing one of the sessions closed.
func (s *Shell) afterSessionsTrashed(keys []string) {
	gone := make(map[string]bool, len(keys))
	for _, key := range keys {
		gone[key] = true
	}
	if forgetter, ok := s.hist.service.(historyForgetter); ok {
		forgetter.Forget(keys...)
	}
	kept := s.hist.sessions[:0:0]
	for _, summary := range s.hist.sessions {
		if !gone[summary.Meta.Key] {
			kept = append(kept, summary)
		}
	}
	s.hist.sessions = kept
	for _, key := range keys {
		delete(s.hist.selected, key)
	}
	if gone[s.hist.preview.key] {
		s.closeHistoryPreview()
	}
	if gone[s.chatConversationID] {
		s.releaseHistoryChat()
	}
	if path := s.router.Path(); strings.HasPrefix(path, routeHistory+"/") && gone[strings.TrimPrefix(path, routeHistory+"/")] {
		s.showHistoryIndex()
	}
	s.requestHistoryList()
	s.requestTrashList()
}

// announceTrashed tells the user what happened: moved sessions come with
// Undo, a refusal says why nothing moved.
func (s *Shell) announceTrashed(moved []sessiontrash.Item, stillInUse, failed int) {
	if len(moved) == 0 {
		s.pendingToast = trashRefusalText(stillInUse, failed)
		return
	}
	message := "Moved to Trash"
	if len(moved) > 1 {
		message = fmt.Sprintf("Moved %d sessions to Trash", len(moved))
	}
	if stillInUse > 0 {
		message += fmt.Sprintf(" · %d still in use", stillInUse)
	}
	if failed > 0 {
		message += fmt.Sprintf(" · %d failed", failed)
	}
	ids := make([]string, 0, len(moved))
	for _, item := range moved {
		ids = append(ids, item.ID)
	}
	s.pendingToastAction = toastAction{message: message, label: "Undo", action: func() { s.restoreFromTrash(ids) }}
}

// trashRefusalText is the toast when nothing could be moved.
func trashRefusalText(stillInUse, failed int) string {
	if stillInUse > 0 && failed == 0 {
		return "Still in use by an Agent — try again when it's idle."
	}
	return "Couldn't move to Trash."
}

// restoreFromTrash puts Trash entries back where they came from, reconciles
// the catalog with what returned (they come back through the same scan that
// found them the first time), and reloads both lists.
func (s *Shell) restoreFromTrash(ids []string) {
	store := s.hist.trash
	if store == nil || len(ids) == 0 {
		return
	}
	service := s.hist.service
	s.dispatch(func() {
		restored, taken, failed := 0, 0, 0
		for _, id := range ids {
			_, err := store.Restore(id)
			switch {
			case err == nil:
				restored++
			case errors.Is(err, sessiontrash.ErrExists):
				taken++
			default:
				failed++
				slog.Warn("put session back failed", "entry", id, "error", err)
			}
		}
		slog.Info("sessions put back", "restored", restored, "exists", taken, "failed", failed)
		if restored > 0 && service != nil {
			ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
			if _, err := service.Scan(ctx); err != nil {
				slog.Warn("history scan after put back failed", "error", err)
			}
			cancel()
		}
		s.applyOnWindow(func() {
			s.dropTrashItems(ids)
			s.requestHistoryList()
			s.requestTrashList()
			s.pendingToast = putBackText(restored, taken, failed)
		})
	})
}

// putBackText is the toast after Put Back.
func putBackText(restored, taken, failed int) string {
	if restored == 0 {
		if taken > 0 {
			return "Can't put back — a session file already exists there."
		}
		return "Couldn't put back."
	}
	message := "Put back"
	if restored > 1 {
		message = fmt.Sprintf("Put back %d sessions", restored)
	}
	if skipped := taken + failed; skipped > 0 {
		message += fmt.Sprintf(" · %d skipped", skipped)
	}
	return message
}

// confirmDeleteForever asks before entries are removed from the Mac for good.
func (s *Shell) confirmDeleteForever(ids []string) {
	if len(ids) == 0 {
		return
	}
	message := "This session will be removed from this Mac. This can't be undone."
	if len(ids) > 1 {
		message = fmt.Sprintf("These %d sessions will be removed from this Mac. This can't be undone.", len(ids))
	}
	s.openConfirm(deleteForeverConfirm, strings.Join(ids, confirmIDSeparator), "Delete Forever", message)
}

// confirmEmptyTrash asks before the whole Trash is removed for good.
func (s *Shell) confirmEmptyTrash() {
	if len(s.hist.trashItems) == 0 {
		return
	}
	s.openConfirm(emptyTrashConfirm, "", "Empty Trash",
		fmt.Sprintf("All %d session%s in the Trash will be removed from this Mac. This can't be undone.",
			len(s.hist.trashItems), pluralS(len(s.hist.trashItems))))
}

// deleteFromTrash removes confirmed entries for good.
func (s *Shell) deleteFromTrash(ids []string) {
	store := s.hist.trash
	if store == nil || len(ids) == 0 {
		return
	}
	s.dispatch(func() {
		deleted := 0
		for _, id := range ids {
			if err := store.Delete(id); err != nil {
				slog.Warn("delete trashed session failed", "entry", id, "error", err)
				continue
			}
			deleted++
		}
		slog.Info("trashed sessions deleted", "deleted", deleted, "requested", len(ids))
		s.applyOnWindow(func() {
			s.dropTrashItems(ids)
			s.requestTrashList()
			switch {
			case deleted == 0:
				s.pendingToast = "Couldn't delete."
			case deleted == 1:
				s.pendingToast = "Deleted"
			default:
				s.pendingToast = fmt.Sprintf("Deleted %d sessions", deleted)
			}
		})
	})
}

// emptyTrash removes every entry for good.
func (s *Shell) emptyTrash() {
	store := s.hist.trash
	if store == nil {
		return
	}
	s.dispatch(func() {
		removed, err := store.Empty()
		if err != nil {
			slog.Warn("empty session trash failed", "removed", removed, "error", err)
		}
		slog.Info("session trash emptied", "removed", removed)
		s.applyOnWindow(func() {
			s.hist.trashItems = nil
			s.clearHistorySelection()
			s.requestTrashList()
			if err != nil {
				s.pendingToast = "Couldn't empty the Trash."
				return
			}
			s.pendingToast = "Trash emptied"
		})
	})
}

// dropTrashItems takes entries off the Trash list (and the selection) at
// once; the reload that follows confirms it.
func (s *Shell) dropTrashItems(ids []string) {
	gone := make(map[string]bool, len(ids))
	for _, id := range ids {
		gone[id] = true
		delete(s.hist.selected, id)
	}
	kept := s.hist.trashItems[:0:0]
	for _, item := range s.hist.trashItems {
		if !gone[item.ID] {
			kept = append(kept, item)
		}
	}
	s.hist.trashItems = kept
}

// trashMenu is the one action list of a Trash row.
func (s *Shell) trashMenu(m *ui.Menu, meta history.SessionMeta) {
	if m.Item("Put Back").Chosen() {
		s.restoreFromTrash([]string{meta.Key})
	}
	if meta.FilePath != "" && m.Item("Reveal in Finder").Chosen() {
		mygo.Shell.ShowItemInFolder(meta.FilePath)
	}
	m.Separator()
	if m.Item("Delete Forever…").Chosen() {
		s.confirmDeleteForever([]string{meta.Key})
	}
}

// historyTrashToggle switches the list between the sessions and the Trash,
// with the Trash total beside its label.
func (s *Shell) historyTrashToggle(c *ui.Context) {
	t := c.Theme()
	toggle := ui.ButtonBase(c).Height(Controls().RegularHeight).Padding(0, Spacing().L).Gap(Spacing().S).
		Radius(Radius().Control).AlignItems(ui.Center).Label("Trash").Tooltip("Deleted sessions")
	gorexToggleFace(toggle, gorexColorsOf(t.Dark), s.hist.showTrash)
	toggle.Children(func() {
		ui.Icon(c, iconTrash).Size(13, 13).TextColor(t.TextMuted).PassThrough()
		ui.Text(c, "Trash").FontSize(Typography().Body).PassThrough()
		if count := len(s.hist.trashItems); count > 0 {
			ui.Text(c, itoa(count)).FontSize(Typography().Caption).TextColor(t.TextMuted).PassThrough()
		}
	})
	if toggle.Clicked() {
		s.setHistoryTrashView(!s.hist.showTrash)
	}
}

// trashAvailable says whether the page offers the Trash at all: a store is
// attached, and — like every control with nothing to act on — it shows once
// something is in it, and stays while its view is open.
func (s *Shell) trashAvailable() bool {
	return s.hist.trash != nil && (len(s.hist.trashItems) > 0 || s.hist.showTrash)
}
