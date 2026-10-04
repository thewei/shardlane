package services

import (
	"context"
	"net"
	"strconv"
	"strings"
)

// ListenerSnapshot is one process's listening TCP sockets with its command
// name and working directory, from an lsof snapshot.
type ListenerSnapshot struct {
	PID     int
	Command string
	Ports   []uint16
	// CWD is the process working directory from the follow-up lsof lookup;
	// empty when that lookup failed or is unavailable.
	CWD string
}

// ParseLsofListeners parses the raw stdout of
// `lsof -Pan -iTCP -sTCP:LISTEN` keeping the command name alongside the
// deduplicated ports per PID. The COMMAND column is lsof-truncated to nine
// characters; it is a hint for labels, never an identity.
func ParseLsofListeners(output string) map[int]ListenerSnapshot {
	result := make(map[int]ListenerSnapshot)
	for _, line := range strings.Split(output, "\n") {
		trimmed := strings.TrimSpace(line)
		if trimmed == "" || strings.HasPrefix(trimmed, "COMMAND") {
			continue
		}

		fields := strings.Fields(trimmed)
		if len(fields) < 9 {
			continue
		}

		// Field 1 is PID, field 0 the (truncated) command name.
		pid, err := strconv.Atoi(fields[1])
		if err != nil {
			continue
		}

		port := parsePortField(fields[8])
		if port == 0 {
			continue
		}
		snap := result[pid]
		snap.PID = pid
		snap.Command = fields[0]
		for _, existing := range snap.Ports {
			if existing == port {
				port = 0
				break
			}
		}
		if port == 0 {
			continue
		}
		snap.Ports = append(snap.Ports, port)
		result[pid] = snap
	}
	return result
}

// parsePortField extracts the listening port from an lsof NAME column such
// as `*:3000`, `127.0.0.1:5432` or `[::1]:5173`; 0 when it is not one.
func parsePortField(addrField string) uint16 {
	_, portStr, err := net.SplitHostPort(addrField)
	if err != nil {
		// Some formats might be "*:3000" or "localhost:8080"
		idx := strings.LastIndex(addrField, ":")
		if idx == -1 {
			return 0
		}
		portStr = addrField[idx+1:]
	}
	portNum, err := strconv.ParseUint(portStr, 10, 16)
	if err != nil || portNum <= 0 {
		return 0
	}
	return uint16(portNum)
}

// ObserveListeners snapshots the machine's listening TCP sockets with
// command names and best-effort working directories (one listener lsof plus
// one cwd lsof). Snapshots keep empty CWD when the lookup fails, so callers
// can still use the PID-level facts.
func (p *PortProbe) ObserveListeners(ctx context.Context) (map[int]ListenerSnapshot, error) {
	p.mu.Lock()
	runner, cwdRunner := p.runner, p.cwdRunner
	p.mu.Unlock()

	out, err := runner(ctx)
	if err != nil {
		return nil, err
	}
	listeners := ParseLsofListeners(out)
	if cwdRunner != nil && len(listeners) > 0 {
		pids := make([]int, 0, len(listeners))
		for pid := range listeners {
			pids = append(pids, pid)
		}
		if cwds, err := cwdRunner(ctx, pids); err == nil {
			for pid, cwd := range cwds {
				snap, ok := listeners[pid]
				if !ok {
					continue
				}
				snap.CWD = cwd
				listeners[pid] = snap
			}
		}
	}
	return listeners, nil
}
