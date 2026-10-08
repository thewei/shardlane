package herdr

import (
	"fmt"
	"strings"
)

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
