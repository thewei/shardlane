package herdr

import (
	"context"
	"encoding/json"
)

/**
 * [INPUT]: 依赖 herdr socket API：pane.get（协议 22，形状以 live schema 为准）
 * [OUTPUT]: 提供 Manager.PaneScrollInfo（共享视口位置读取；滚轮分流与 F144 真终端语义消费）
 * [POS]: 终端数据面的只读部分；查找链路已收敛到 Manager.CopySearch（terminal_find.go GWB-010），旧的 per-pane PaneFind 随其死 UI 一起移除
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

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
