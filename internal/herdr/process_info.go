package herdr

import (
	"fmt"
)

// PaneProcessInfoProcess is one live foreground process of a Pane, decoded
// from the live schema's PaneProcessInfoProcess (protocol 22).
type PaneProcessInfoProcess struct {
	PID     int      `json:"pid"`
	Name    string   `json:"name"`
	Argv0   string   `json:"argv0,omitempty"`
	Cmdline string   `json:"cmdline,omitempty"`
	CWD     string   `json:"cwd,omitempty"`
	Argv    []string `json:"argv,omitempty"`
}

// PaneProcessInfo is Herdr's measured process state for one Pane
// (pane.process_info): the shell PID, the foreground process group id, and
// the processes currently in that group.
type PaneProcessInfo struct {
	PaneID              string                   `json:"pane_id"`
	ShellPID            int                      `json:"shell_pid"`
	ForegroundPGID      int                      `json:"foreground_process_group_id"`
	TTY                 string                   `json:"tty,omitempty"`
	ForegroundProcesses []PaneProcessInfoProcess `json:"foreground_processes"`
}

type paneProcessInfoResult struct {
	Type        string          `json:"type"`
	ProcessInfo PaneProcessInfo `json:"process_info"`
}

// PaneProcessInfo reports the live foreground processes of one Pane. It is a
// point-in-time measurement over the session socket; callers poll it on
// their own cadence. Unlike the projection calls it skips reachability work —
// it only makes sense right after a live projection arrived.
func (m *Manager) PaneProcessInfo(session, paneID string) (*PaneProcessInfo, error) {
	socket, err := m.socketFor(session)
	if err != nil {
		return nil, err
	}
	return paneProcessInfoAt(socket, paneID)
}

func paneProcessInfoAt(socketPath, paneID string) (*PaneProcessInfo, error) {
	var result paneProcessInfoResult
	if err := callRPC(socketPath, "pane.process_info", map[string]any{"pane_id": paneID}, false, &result); err != nil {
		return nil, err
	}
	if result.Type != "pane_process_info" {
		return nil, fmt.Errorf("Herdr pane.process_info returned %q", result.Type)
	}
	info := result.ProcessInfo
	return &info, nil
}
