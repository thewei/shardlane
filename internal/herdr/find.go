package herdr

import (
	"context"
	"encoding/json"
)

/**
 * [INPUT]: 依赖 herdr socket API：pane.scroll / pane.get / pane.copy_motion / pane.copy_search（协议 22，形状以参考客户端与 live schema 为准）
 * [OUTPUT]: 提供 Manager.PaneScrollInfo / Manager.PaneFind（共享视口查找与跳转，带 previous 续搜与 stale 检测）
 * [POS]: 终端查找的数据面：查找匹配并滚动共享视口，附加的 MyGo 终端跟随渲染；不进入 copy mode，不改任何客户端游标
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// PanePoint is a row/col cell in the pane's stored history.
type PanePoint struct {
	Row int64 `json:"row"`
	Col int64 `json:"col"`
}

// PaneMatch is one match's extent.
type PaneMatch struct {
	Start PanePoint `json:"start"`
	End   PanePoint `json:"end"`
}

// PaneFindResult is one search step: the exact total, the 1-based current
// ordinal (0 = none) and the current match.
type PaneFindResult struct {
	Total      int64
	Current    int64
	HasCurrent bool
	Match      PaneMatch
	HasMatch   bool
}

// PaneScrollInfo reads the pane's shared viewport position.
func (m *Manager) PaneScrollInfo(ctx context.Context, session, paneID string) (maxOffsetFromBottom, offsetFromBottom int64, err error) {
	socket, err := m.reachableSocket(session)
	if err != nil {
		return 0, 0, err
	}
	var raw json.RawMessage
	if err := callRPCWithContext(ctx, socket, "pane.get", map[string]any{"pane_id": paneID}, false, &raw); err != nil {
		return 0, 0, err
	}
	var payload struct {
		Pane struct {
			Scroll struct {
				MaxOffsetFromBottom int64 `json:"max_offset_from_bottom"`
				OffsetFromBottom    int64 `json:"offset_from_bottom"`
			} `json:"scroll"`
		} `json:"pane"`
	}
	if err := decodeTypedResult(raw, "pane", &payload); err != nil {
		return 0, 0, err
	}
	return payload.Pane.Scroll.MaxOffsetFromBottom, payload.Pane.Scroll.OffsetFromBottom, nil
}

// PaneFind searches the pane's stored history for literal text and jumps
// the shared viewport to the match — the attached terminal follows the
// scroll. previous continues from the last match while the content revision
// still matches; a changed pane surfaces as an error the caller shows
// verbatim ("Pane changed. Search again.").
func (m *Manager) PaneFind(ctx context.Context, session, paneID, query, direction string, previous *PaneMatch, jump bool) (PaneFindResult, error) {
	socket, err := m.reachableSocket(session)
	if err != nil {
		return PaneFindResult{}, err
	}
	// The search cursor anchors on the current viewport row: read the
	// scroll position, then take copy_motion's line_end to acquire the
	// content revision without entering copy mode.
	maxOffset, offset, err := m.PaneScrollInfo(ctx, session, paneID)
	if err != nil {
		return PaneFindResult{}, err
	}
	row := maxOffset - offset
	var raw json.RawMessage
	if err := callRPCWithContext(ctx, socket, "pane.copy_motion", map[string]any{
		"pane_id": paneID,
		"cursor":  map[string]any{"row": row, "col": 0},
		"motion":  "line_end",
	}, false, &raw); err != nil {
		return PaneFindResult{}, err
	}
	var motion struct {
		Cursor          PanePoint `json:"cursor"`
		ContentRevision int64     `json:"content_revision"`
	}
	if err := decodeTypedResult(raw, "cursor", &motion); err != nil {
		return PaneFindResult{}, err
	}

	params := map[string]any{
		"pane_id":          paneID,
		"query":            query,
		"direction":        direction,
		"content_revision": motion.ContentRevision,
	}
	if previous != nil {
		params["cursor"] = map[string]any{"row": previous.Start.Row, "col": previous.Start.Col}
		params["previous"] = map[string]any{
			"start": map[string]any{"row": previous.Start.Row, "col": previous.Start.Col},
			"end":   map[string]any{"row": previous.End.Row, "col": previous.End.Col},
		}
	} else if direction == "backward" {
		params["cursor"] = map[string]any{"row": motion.Cursor.Row, "col": motion.Cursor.Col}
	} else {
		params["cursor"] = map[string]any{"row": row, "col": 0}
	}
	if err := callRPCWithContext(ctx, socket, "pane.copy_search", params, false, &raw); err != nil {
		return PaneFindResult{}, err
	}
	var result struct {
		Matches []struct {
			Start PanePoint `json:"start"`
			End   PanePoint `json:"end"`
		} `json:"matches"`
		Total   int64  `json:"total"`
		Current *int64 `json:"current"`
		Match   *struct {
			Start PanePoint `json:"start"`
			End   PanePoint `json:"end"`
		} `json:"match"`
		ContentRevision int64 `json:"content_revision"`
	}
	if err := decodeTypedResult(raw, "copy_search", &result); err != nil {
		return PaneFindResult{}, err
	}
	out := PaneFindResult{Total: result.Total}
	if result.Current != nil {
		out.Current, out.HasCurrent = *result.Current, true
	}
	if result.Match != nil {
		out.Match, out.HasMatch = PaneMatch{Start: result.Match.Start, End: result.Match.End}, true
	}
	// Jump the shared viewport to the match: the attached terminal renders
	// the scrolled history on its next frame.
	if jump && out.HasMatch {
		if err := m.scrollPaneToRow(ctx, session, paneID, maxOffset-out.Match.Start.Row); err != nil {
			return out, err
		}
	}
	return out, nil
}

// scrollPaneToRow scrolls the pane so the given absolute history row is on
// screen (offset_from_bottom derived from the current max).
func (m *Manager) scrollPaneToRow(ctx context.Context, session, paneID string, absoluteRow int64) error {
	socket, err := m.reachableSocket(session)
	if err != nil {
		return err
	}
	var raw json.RawMessage
	if err := callRPCWithContext(ctx, socket, "pane.get", map[string]any{"pane_id": paneID}, false, &raw); err != nil {
		return err
	}
	var payload struct {
		Pane struct {
			Scroll struct {
				MaxOffsetFromBottom int64 `json:"max_offset_from_bottom"`
			} `json:"scroll"`
		} `json:"pane"`
	}
	if err := decodeTypedResult(raw, "pane", &payload); err != nil {
		return err
	}
	offset := payload.Pane.Scroll.MaxOffsetFromBottom - absoluteRow
	if offset < 0 {
		offset = 0
	}
	var result map[string]any
	return callRPCWithContext(ctx, socket, "pane.scroll", map[string]any{
		"pane_id":            paneID,
		"offset_from_bottom": offset,
	}, true, &result)
}
