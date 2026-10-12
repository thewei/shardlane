package herdr

/**
 * [INPUT]: 依赖 Herdr protocol 22 的 workspace.move/tab.move/pane.swap/pane.move 与既有 mutateAndProject RPC
 * [OUTPUT]: 提供结构排序、Pane 移动与交换以及已有 Workspace/Tab/Pane 操作
 * [POS]: Herdr 运行时 mutation 适配器；提交后只从 session.snapshot 获取权威布局
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"fmt"
	"strings"
)

// PaneMoveDestination models the destination payload for the Protocol 22
// pane.move wire method.
type PaneMoveDestination struct {
	Type         string   `json:"type"`                    // "tab", "new_tab", "new_workspace"
	TabID        string   `json:"tab_id,omitempty"`        // required when Type == "tab"
	Split        string   `json:"split,omitempty"`         // "right" | "down" (required when Type == "tab")
	TargetPaneID string   `json:"target_pane_id,omitempty"` // optional target pane within tab
	Ratio        *float64 `json:"ratio,omitempty"`         // optional split ratio
	WorkspaceID  string   `json:"workspace_id,omitempty"`  // optional when Type == "new_tab"
	Label        string   `json:"label,omitempty"`         // optional
	TabLabel     string   `json:"tab_label,omitempty"`     // optional when Type == "new_workspace"
}

// MovePane relocates one Pane to a destination: another Tab (split right/down against an optional target pane),
// a new Tab, or a new Workspace. Backend method is pane.move (Protocol 22).
func (m *Manager) MovePane(session, paneID string, dest PaneMoveDestination, focus bool) (Projection, error) {
	if strings.TrimSpace(paneID) == "" {
		return Projection{}, fmt.Errorf("pane id is required")
	}
	destType := strings.TrimSpace(dest.Type)
	if destType == "" {
		return Projection{}, fmt.Errorf("destination type is required")
	}
	destMap := map[string]any{"type": destType}
	switch destType {
	case "tab":
		if strings.TrimSpace(dest.TabID) == "" {
			return Projection{}, fmt.Errorf("tab_id is required for tab destination")
		}
		destMap["tab_id"] = strings.TrimSpace(dest.TabID)
		split := strings.TrimSpace(dest.Split)
		if split == "" {
			split = "right"
		}
		if split != "right" && split != "down" {
			return Projection{}, fmt.Errorf("unsupported pane split direction %q", split)
		}
		destMap["split"] = split
		if target := strings.TrimSpace(dest.TargetPaneID); target != "" {
			destMap["target_pane_id"] = target
		}
		if dest.Ratio != nil {
			destMap["ratio"] = *dest.Ratio
		}
	case "new_tab":
		if ws := strings.TrimSpace(dest.WorkspaceID); ws != "" {
			destMap["workspace_id"] = ws
		}
		if lbl := strings.TrimSpace(dest.Label); lbl != "" {
			destMap["label"] = lbl
		}
	case "new_workspace":
		if lbl := strings.TrimSpace(dest.Label); lbl != "" {
			destMap["label"] = lbl
		}
		if tabLbl := strings.TrimSpace(dest.TabLabel); tabLbl != "" {
			destMap["tab_label"] = tabLbl
		}
	default:
		return Projection{}, fmt.Errorf("unsupported destination type %q", destType)
	}

	params := map[string]any{
		"pane_id":     strings.TrimSpace(paneID),
		"destination": destMap,
		"focus":       focus,
	}
	return m.mutateAndProject(session, "pane.move", params)
}

// CreateProject creates one Herdr runtime workspace inside the selected
// top-level Herdr instance. Product UI calls this a Project; the wire method
// remains workspace.create because that is the backend fact.
func (m *Manager) CreateProject(session, cwd, label string) (Projection, error) {
	params := map[string]any{"focus": true}
	if cwd = strings.TrimSpace(cwd); cwd != "" {
		params["cwd"] = cwd
	}
	if label = strings.TrimSpace(label); label != "" {
		params["label"] = label
	}
	return m.mutateAndProject(session, "workspace.create", params)
}

func (m *Manager) RenameProject(session, projectID, label string) (Projection, error) {
	if strings.TrimSpace(projectID) == "" || strings.TrimSpace(label) == "" {
		return Projection{}, fmt.Errorf("project id and label are required")
	}
	return m.mutateAndProject(session, "workspace.rename", map[string]any{
		"workspace_id": projectID,
		"label":        strings.TrimSpace(label),
	})
}

func (m *Manager) CloseProject(session, projectID string) (Projection, error) {
	if strings.TrimSpace(projectID) == "" {
		return Projection{}, fmt.Errorf("project id is required")
	}
	return m.mutateAndProject(session, "workspace.close", map[string]any{
		"workspace_id": projectID,
	})
}

// MoveProject reorders a Herdr workspace in its session; insert_index is
// the final index after removing it from its old position.
func (m *Manager) MoveProject(session, projectID string, index uint) (Projection, error) {
	if strings.TrimSpace(projectID) == "" {
		return Projection{}, fmt.Errorf("project id is required")
	}
	return m.mutateAndProject(session, "workspace.move", map[string]any{
		"workspace_id": projectID, "insert_index": index,
	})
}

// MoveTab reorders a Tab within its current Herdr workspace. This protocol
// method cannot move Tabs between Projects; callers must fence the parent.
func (m *Manager) MoveTab(session, tabID string, index uint) (Projection, error) {
	if strings.TrimSpace(tabID) == "" {
		return Projection{}, fmt.Errorf("tab id is required")
	}
	return m.mutateAndProject(session, "tab.move", map[string]any{
		"tab_id": tabID, "insert_index": index,
	})
}

// SwapPanes exchanges two positions in one Herdr-owned Pane layout.
// The explicit pair avoids inferring neighbors from the local visual grid.
func (m *Manager) SwapPanes(session, sourceID, targetID string) (Projection, error) {
	if strings.TrimSpace(sourceID) == "" || strings.TrimSpace(targetID) == "" || sourceID == targetID {
		return Projection{}, fmt.Errorf("two distinct pane ids are required")
	}
	return m.mutateAndProject(session, "pane.swap", map[string]any{
		"source_pane_id": sourceID, "target_pane_id": targetID,
	})
}

func (m *Manager) CreateTab(session, projectID, cwd string) (Projection, error) {
	params := map[string]any{
		"workspace_id": projectID,
		"focus":        true,
	}
	if cwd = strings.TrimSpace(cwd); cwd != "" {
		params["cwd"] = cwd
	}
	return m.mutateAndProject(session, "tab.create", params)
}

func (m *Manager) RenameTab(session, tabID, label string) (Projection, error) {
	if strings.TrimSpace(tabID) == "" || strings.TrimSpace(label) == "" {
		return Projection{}, fmt.Errorf("tab id and label are required")
	}
	return m.mutateAndProject(session, "tab.rename", map[string]any{
		"tab_id": tabID,
		"label":  strings.TrimSpace(label),
	})
}

func (m *Manager) CloseTab(session, tabID string) (Projection, error) {
	if strings.TrimSpace(tabID) == "" {
		return Projection{}, fmt.Errorf("tab id is required")
	}
	return m.mutateAndProject(session, "tab.close", map[string]any{"tab_id": tabID})
}

func (m *Manager) SplitPane(session, paneID, direction string) (Projection, error) {
	direction = strings.TrimSpace(direction)
	if direction != "right" && direction != "down" {
		return Projection{}, fmt.Errorf("unsupported pane split direction %q", direction)
	}
	if strings.TrimSpace(paneID) == "" {
		return Projection{}, fmt.Errorf("pane id is required")
	}
	return m.mutateAndProject(session, "pane.split", map[string]any{
		"target_pane_id": paneID,
		"direction":      direction,
		"focus":          true,
	})
}

func (m *Manager) RenamePane(session, paneID, label string) (Projection, error) {
	if strings.TrimSpace(paneID) == "" {
		return Projection{}, fmt.Errorf("pane id is required")
	}
	value := strings.TrimSpace(label)
	var wireLabel any
	if value == "" {
		wireLabel = nil
	} else {
		wireLabel = value
	}
	return m.mutateAndProject(session, "pane.rename", map[string]any{
		"pane_id": paneID,
		"label":   wireLabel,
	})
}

func (m *Manager) ClosePane(session, paneID string) (Projection, error) {
	if strings.TrimSpace(paneID) == "" {
		return Projection{}, fmt.Errorf("pane id is required")
	}
	return m.mutateAndProject(session, "pane.close", map[string]any{"pane_id": paneID})
}

func (m *Manager) TogglePaneZoom(session, paneID string) (Projection, error) {
	if strings.TrimSpace(paneID) == "" {
		return Projection{}, fmt.Errorf("pane id is required")
	}
	return m.mutateAndProject(session, "pane.zoom", map[string]any{
		"pane_id": paneID,
		"mode":    "toggle",
	})
}

// ScrollPane moves Herdr's authoritative scroll viewport for one Pane to an
// absolute offset above the bottom of Herdr-owned scrollback (0 = live).
// Verified against the live protocol schema (PaneScrollParams: pane_id +
// offset_from_bottom). It deliberately does not refetch the snapshot: wheel
// gestures are high-frequency, and the event watcher / next projection
// reconcile the authoritative metrics.
func (m *Manager) ScrollPane(session, paneID string, offsetFromBottom uint64) error {
	if strings.TrimSpace(paneID) == "" {
		return fmt.Errorf("pane id is required")
	}
	socket, err := m.reachableSocket(session)
	if err != nil {
		return err
	}
	var result map[string]any
	return callRPC(socket, "pane.scroll", map[string]any{
		"pane_id":            paneID,
		"offset_from_bottom": offsetFromBottom,
	}, true, &result)
}

// SendPaneText writes literal text into one Pane's input, verbatim —
// escape sequences included (PaneSendTextParams: pane_id + text, verified
// against the live protocol schema and probed against a running daemon).
// This is how wheel events reach an Agent TUI pane: the program tracks the
// mouse itself, the attach channel drops mouse bytes (probed 2026-10-05),
// and this socket write lands in the Pane's PTY directly. Like ScrollPane
// it never refetches the snapshot: wheel gestures are high-frequency and
// the next projection reconciles.
func (m *Manager) SendPaneText(session, paneID, text string) error {
	if strings.TrimSpace(paneID) == "" {
		return fmt.Errorf("pane id is required")
	}
	if text == "" {
		return nil
	}
	socket, err := m.reachableSocket(session)
	if err != nil {
		return err
	}
	var result map[string]any
	return callRPC(socket, "pane.send_text", map[string]any{
		"pane_id": paneID,
		"text":    text,
	}, true, &result)
}

func (m *Manager) mutateAndProject(session, method string, params map[string]any) (Projection, error) {
	socket, err := m.reachableSocket(session)
	if err != nil {
		return Projection{}, err
	}
	var result map[string]any
	if err := callRPC(socket, method, params, true, &result); err != nil {
		// DeliveryUncertainError intentionally escapes unchanged. Mutations are
		// never retried automatically; the event watcher can reconcile later.
		return Projection{}, err
	}
	return m.projectionReadyAt(socket)
}
