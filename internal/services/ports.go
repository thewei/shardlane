// Package services owns resident service monitoring and port intelligence
// (0.9 P6 / WIX-090..105). Port discovery probes local listening TCP sockets
// on background lanes with a stale-while-refresh cache (never from render).
package services

import (
	"context"
	"fmt"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"time"
)

// PortObservation maps a PID to its open listening TCP ports.
type PortObservation struct {
	PID   int      `json:"pid"`
	Ports []uint16 `json:"ports"`
}

// PreviewIntent marks an observed port that can be opened in Local Preview (P8).
type PreviewIntent struct {
	Port uint16 `json:"port"`
	URL  string `json:"url"`
}

// LocalPreviewURL generates a loopback target for the port.
func LocalPreviewURL(port uint16) string {
	return fmt.Sprintf("http://127.0.0.1:%d", port)
}

// PortProbe provides thread-safe port observation with caching.
type PortProbe struct {
	mu          sync.Mutex
	lastScan    time.Time
	cachedPorts map[int][]uint16
	runner      func(ctx context.Context) (string, error)
	cwdRunner   func(ctx context.Context, pids []int) (map[int]string, error)
}

// DefaultPortScanInterval is the minimum interval between actual lsof calls.
const DefaultPortScanInterval = 10 * time.Second

// NewPortProbe creates a live probe using lsof on macOS/Linux.
func NewPortProbe() *PortProbe {
	return &PortProbe{
		cachedPorts: make(map[int][]uint16),
		runner: func(ctx context.Context) (string, error) {
			cmd := exec.CommandContext(ctx, "lsof", "-Pan", "-iTCP", "-sTCP:LISTEN")
			out, err := cmd.Output()
			return string(out), err
		},
		cwdRunner: func(ctx context.Context, pids []int) (map[int]string, error) {
			if len(pids) == 0 {
				return nil, nil
			}
			var pidStrs []string
			for _, pid := range pids {
				pidStrs = append(pidStrs, strconv.Itoa(pid))
			}
			cmd := exec.CommandContext(ctx, "lsof", "-p", strings.Join(pidStrs, ","), "-a", "-d", "cwd", "-Fn")
			out, err := cmd.Output()
			if err != nil {
				return nil, err
			}
			return ParseLsofCWD(string(out)), nil
		},
	}
}

// NewPortProbeWithRunner allows testing with mock lsof output.
func NewPortProbeWithRunner(runner func(ctx context.Context) (string, error)) *PortProbe {
	return &PortProbe{
		cachedPorts: make(map[int][]uint16),
		runner:      runner,
	}
}

// SetCWDRunner configures the cwd lookup runner for testing.
func (p *PortProbe) SetCWDRunner(fn func(ctx context.Context, pids []int) (map[int]string, error)) {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.cwdRunner = fn
}

// PortsForPID returns the listening ports associated with a process PID.
func (p *PortProbe) PortsForPID(ctx context.Context, pid int) []uint16 {
	p.mu.Lock()
	defer p.mu.Unlock()

	if time.Since(p.lastScan) > DefaultPortScanInterval {
		if out, err := p.runner(ctx); err == nil {
			p.cachedPorts = ParseLsofListeningPorts(out)
			p.lastScan = time.Now()
		}
	}

	return p.cachedPorts[pid]
}

// ParseLsofListeningPorts parses the raw stdout of `lsof -Pan -iTCP -sTCP:LISTEN`
// into the PID→ports shape (WIX-090) on top of the richer listener parse.
func ParseLsofListeningPorts(output string) map[int][]uint16 {
	result := make(map[int][]uint16)
	for pid, snap := range ParseLsofListeners(output) {
		result[pid] = snap.Ports
	}
	return result
}

// ParseLsofCWD parses the output of `lsof -p ... -a -d cwd -Fn`.
func ParseLsofCWD(output string) map[int]string {
	result := make(map[int]string)
	currentPID := 0
	for _, line := range strings.Split(output, "\n") {
		line = strings.TrimSpace(line)
		if strings.HasPrefix(line, "p") {
			if pid, err := strconv.Atoi(line[1:]); err == nil {
				currentPID = pid
			}
		} else if strings.HasPrefix(line, "n") && currentPID != 0 {
			result[currentPID] = line[1:]
		}
	}
	return result
}

// ObserveAll runs the default listening-port probe (lsof) and returns the
// parsed per-PID port map. It is the snapshot source for the Services view;
// render paths must read cached results only.
func (p *PortProbe) ObserveAll(ctx context.Context) (map[int][]uint16, error) {
	out, err := p.runner(ctx)
	if err != nil {
		return nil, err
	}
	return ParseLsofListeningPorts(out), nil
}

// ObserveForRoot returns listening ports belonging to processes whose working
// directory is contained within root (or root itself), preventing unrelated machine
// processes from cluttering the project Services panel (P1-09).
func (p *PortProbe) ObserveForRoot(ctx context.Context, root string) ([]uint16, error) {
	byPID, err := p.ObserveAll(ctx)
	if err != nil {
		return nil, err
	}
	if root == "" || len(byPID) == 0 {
		return nil, nil
	}

	cleanRoot := filepath.Clean(root)
	var pids []int
	for pid := range byPID {
		pids = append(pids, pid)
	}

	p.mu.Lock()
	cwdFn := p.cwdRunner
	p.mu.Unlock()

	var matchingPorts []uint16
	if cwdFn != nil {
		cwds, err := cwdFn(ctx, pids)
		if err == nil {
			for pid, dir := range cwds {
				cleanDir := filepath.Clean(dir)
				if cleanDir == cleanRoot || strings.HasPrefix(cleanDir, cleanRoot+string(filepath.Separator)) {
					matchingPorts = append(matchingPorts, byPID[pid]...)
				}
			}
		}
	}

	return matchingPorts, nil
}
