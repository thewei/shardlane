package services

import (
	"context"
	"testing"
)

const sampleLsofOutput = `COMMAND     PID   USER   FD   TYPE             DEVICE SIZE/OFF NODE NAME
node      12345 wilson   23u  IPv6 0x1234567890abcdef      0t0  TCP *:3000 (LISTEN)
node      12345 wilson   24u  IPv4 0xabcdef1234567890      0t0  TCP 127.0.0.1:3000 (LISTEN)
postgres  23456 wilson    7u  IPv4 0x1111222233334444      0t0  TCP 127.0.0.1:5432 (LISTEN)
vite      34567 wilson   19u  IPv6 0x5555666677778888      0t0  TCP [::1]:5173 (LISTEN)
`

// TestParseLsofListeningPorts pins WIX-090: PID→ports mapping, port extraction,
// and deduplication across dual-stack sockets.
func TestParseLsofListeningPorts(t *testing.T) {
	ports := ParseLsofListeningPorts(sampleLsofOutput)

	if len(ports) != 3 {
		t.Fatalf("expected 3 distinct PIDs with listening ports, got %d", len(ports))
	}

	// PID 12345 has port 3000 on IPv4 & IPv6, should be deduplicated to one 3000
	nodePorts := ports[12345]
	if len(nodePorts) != 1 || nodePorts[0] != 3000 {
		t.Fatalf("expected PID 12345 to have [3000], got %+v", nodePorts)
	}

	// PID 23456 has 5432
	pgPorts := ports[23456]
	if len(pgPorts) != 1 || pgPorts[0] != 5432 {
		t.Fatalf("expected PID 23456 to have [5432], got %+v", pgPorts)
	}

	// PID 34567 has 5173
	vitePorts := ports[34567]
	if len(vitePorts) != 1 || vitePorts[0] != 5173 {
		t.Fatalf("expected PID 34567 to have [5173], got %+v", vitePorts)
	}
}

// TestPortProbeCaching pins WIX-091: background cache reuses the last scan
// within the scan interval and doesn't spam runner execution.
func TestPortProbeCaching(t *testing.T) {
	calls := 0
	probe := NewPortProbeWithRunner(func(ctx context.Context) (string, error) {
		calls++
		return sampleLsofOutput, nil
	})

	ctx := context.Background()
	ports1 := probe.PortsForPID(ctx, 12345)
	if len(ports1) != 1 || ports1[0] != 3000 {
		t.Fatalf("expected port 3000, got %+v", ports1)
	}
	if calls != 1 {
		t.Fatalf("expected 1 runner call, got %d", calls)
	}

	// Immediate second call should use cache without invoking runner
	ports2 := probe.PortsForPID(ctx, 34567)
	if len(ports2) != 1 || ports2[0] != 5173 {
		t.Fatalf("expected port 5173, got %+v", ports2)
	}
	if calls != 1 {
		t.Fatalf("expected still 1 runner call due to caching, got %d", calls)
	}
}

// TestLocalPreviewURL pins P8 loopback target format.
func TestLocalPreviewURL(t *testing.T) {
	if got := LocalPreviewURL(3000); got != "http://127.0.0.1:3000" {
		t.Fatalf("expected http://127.0.0.1:3000, got %q", got)
	}
	if got := LocalPreviewURL(5173); got != "http://127.0.0.1:5173" {
		t.Fatalf("expected http://127.0.0.1:5173, got %q", got)
	}
}

// TestParseLsofListeners pins the named listener parse behind the sidebar
// service index: command names travel with the deduplicated ports.
func TestParseLsofListeners(t *testing.T) {
	listeners := ParseLsofListeners(sampleLsofOutput)
	if len(listeners) != 3 {
		t.Fatalf("expected 3 listeners, got %d", len(listeners))
	}
	node := listeners[12345]
	if node.Command != "node" || len(node.Ports) != 1 || node.Ports[0] != 3000 {
		t.Fatalf("node = %+v", node)
	}
	if listeners[34567].Command != "vite" {
		t.Fatalf("vite = %+v", listeners[34567])
	}
}

// TestObserveListenersFillsCWD pins the best-effort cwd enrichment and its
// tolerance for a failing cwd lookup.
func TestObserveListenersFillsCWD(t *testing.T) {
	probe := NewPortProbeWithRunner(func(ctx context.Context) (string, error) {
		return sampleLsofOutput, nil
	})
	probe.SetCWDRunner(func(ctx context.Context, pids []int) (map[int]string, error) {
		return map[int]string{12345: "/repo"}, nil
	})
	listeners, err := probe.ObserveListeners(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if listeners[12345].CWD != "/repo" {
		t.Fatalf("cwd = %q", listeners[12345].CWD)
	}
	if listeners[23456].CWD != "" {
		t.Fatalf("unlisted cwd = %q", listeners[23456].CWD)
	}

	// A failing cwd lookup keeps the listener facts usable.
	probe.SetCWDRunner(func(ctx context.Context, pids []int) (map[int]string, error) {
		return nil, context.DeadlineExceeded
	})
	listeners, err = probe.ObserveListeners(context.Background())
	if err != nil || len(listeners) != 3 {
		t.Fatalf("listeners after cwd failure = %v, %v", listeners, err)
	}
}
