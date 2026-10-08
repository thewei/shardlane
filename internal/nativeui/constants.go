package nativeui

import "time"

// Shared behavioral and policy constants for Native UI (plan P2-13).
const (
	PortRefreshInterval   = 10 * time.Second
	PortProbeTimeout      = 3 * time.Second
	TerminalSearchTimeout = 3 * time.Second
	DiffRevealGuardWindow = 350 * time.Millisecond
	TreeIndentWidth       = 14
)
