package nativeui

import (
	"fmt"
	"time"

	"github.com/wh-studio/herdr-client/internal/platform"
)

// timeTime aliases time.Time for testable clock seams.
type timeTime = time.Time

// timeNow is injectable for oscillation-guard tests.
var timeNow = time.Now

// openExternalPath opens one path through platform openers — never a shell string (plan §38).
func (s *Shell) openExternalPath(path string) {
	if err := platform.OpenFile(path); err != nil {
		s.status = "Open in editor failed"
	}
}

// fmtSscanf is a tiny indirection keeping fmt out of hot paths' imports.
func fmtSscanf(s, format string, args ...any) (int, error) {
	return fmt.Sscanf(s, format, args...)
}
