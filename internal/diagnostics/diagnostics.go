// Package diagnostics owns the P16 Diagnostics & Logs and P17 Diagnostic export
// capabilities (0.9 P16/P17 / WIX-200..223). Diagnostics are strictly sanitized:
// no terminal output, no conversation transcripts, no file previews, and no
// credentials or provider secrets ever enter diagnostic snapshots or exports.
package diagnostics

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"strings"
	"time"

	"github.com/egoist/mygo"

	"github.com/wh-studio/herdr-client/internal/applog"
)

// LogViewerCap is the maximum number of recent log entries kept in memory (WIX-210).
const LogViewerCap = 5000

// Snapshot captures safe system, runtime, and integration facts for troubleshooting.
type Snapshot struct {
	ShardlaneVersion  string            `json:"shardlane_version"`
	GoVersion         string            `json:"go_version"`
	MyGoVersion       string            `json:"mygo_version"`
	OS                string            `json:"os"`
	Arch              string            `json:"arch"`
	HerdrProtocol     uint32            `json:"herdr_protocol,omitempty"`
	HerdrVersion      string            `json:"herdr_version,omitempty"`
	ActiveInstance    string            `json:"active_instance,omitempty"`
	LogPath           string            `json:"log_path,omitempty"`
	GeneratedAtUnixMS int64             `json:"generated_at_unix_ms"`
	IntegrationStates map[string]string `json:"integration_states,omitempty"`
}

// LogEntry is one structured log record displayed in the viewer.
type LogEntry struct {
	Time    string `json:"time"`
	Level   string `json:"level"`
	Message string `json:"msg"`
	Raw     string `json:"raw"`
}

// SecretRegex matches common API keys, tokens, and bearer credentials for redaction.
var SecretRegex = regexp.MustCompile(`(?i)(api[_-]?key|bearer\s+[a-z0-9_\-\.]+|token|password|secret|auth)[=:\s"']+([a-z0-9_\-\.]{8,})`)

// HomeRegex is initialized to redact user home directories.
func RedactHome(path, home string) string {
	if home == "" {
		return path
	}
	if strings.HasPrefix(path, home) {
		return "~" + strings.TrimPrefix(path, home)
	}
	return path
}

// SanitizeText strips secrets and normalizes home directory references.
func SanitizeText(text, home string) string {
	if home != "" {
		text = strings.ReplaceAll(text, home, "~")
	}
	// Redact keys/tokens
	text = SecretRegex.ReplaceAllStringFunc(text, func(match string) string {
		parts := strings.SplitN(match, "=", 2)
		if len(parts) == 2 {
			return parts[0] + "=REDACTED"
		}
		parts = strings.SplitN(match, ":", 2)
		if len(parts) == 2 {
			return parts[0] + ":\"REDACTED\""
		}
		return "[REDACTED_CREDENTIAL]"
	})
	return text
}

// BuildSnapshot derives the safe diagnostics snapshot.
func BuildSnapshot(herdrVersion string, herdrProtocol uint32, activeInstance string, home string) Snapshot {
	logPath := applog.Path()
	if home != "" && logPath != "" {
		logPath = RedactHome(logPath, home)
	}

	// Versions come from their owners: the packaged app version (mygo.json →
	// Info.plist) and the compiled MyGo framework, never hardcoded copies
	// that drift on every release. Unpackaged runs (tests, bare binary) have
	// no bundle version and truthfully report "dev".
	version := mygo.App.Version()
	if version == "" {
		version = "dev"
	}

	return Snapshot{
		ShardlaneVersion:  version,
		GoVersion:         runtime.Version(),
		MyGoVersion:       mygo.Version,
		OS:                runtime.GOOS,
		Arch:              runtime.GOARCH,
		HerdrProtocol:     herdrProtocol,
		HerdrVersion:      herdrVersion,
		ActiveInstance:    activeInstance,
		LogPath:           logPath,
		GeneratedAtUnixMS: time.Now().UnixMilli(),
	}
}

// ReadRecentLogs reads up to LogViewerCap lines from the active log file.
func ReadRecentLogs(maxLines int) ([]LogEntry, error) {
	logPath := applog.Path()
	if logPath == "" {
		return nil, nil
	}

	file, err := os.Open(logPath)
	if err != nil {
		return nil, err
	}
	defer file.Close()

	if maxLines <= 0 || maxLines > LogViewerCap {
		maxLines = LogViewerCap
	}

	var lines []string
	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		lines = append(lines, scanner.Text())
		if len(lines) > maxLines*2 {
			// keep bounded to prevent runaway memory
			lines = lines[len(lines)-maxLines:]
		}
	}

	if len(lines) > maxLines {
		lines = lines[len(lines)-maxLines:]
	}

	entries := make([]LogEntry, 0, len(lines))
	for _, l := range lines {
		trimmed := strings.TrimSpace(l)
		if trimmed == "" {
			continue
		}
		var parsed struct {
			Time  string `json:"time"`
			Level string `json:"level"`
			Msg   string `json:"msg"`
		}
		_ = json.Unmarshal([]byte(trimmed), &parsed)
		entries = append(entries, LogEntry{
			Time:    parsed.Time,
			Level:   strings.ToUpper(parsed.Level),
			Message: parsed.Msg,
			Raw:     trimmed,
		})
	}

	return entries, nil
}

// FilterLogs filters entries by level and keyword in memory.
func FilterLogs(entries []LogEntry, level string, search string) []LogEntry {
	level = strings.ToUpper(strings.TrimSpace(level))
	search = strings.ToLower(strings.TrimSpace(search))

	var out []LogEntry
	for _, e := range entries {
		if level != "" && level != "ALL" && e.Level != level {
			continue
		}
		if search != "" {
			if !strings.Contains(strings.ToLower(e.Raw), search) &&
				!strings.Contains(strings.ToLower(e.Message), search) {
				continue
			}
		}
		out = append(out, e)
	}
	return out
}

// ExportArchive represents the sanitized export bundle (P17).
type ExportArchive struct {
	Diagnostics Snapshot   `json:"diagnostics"`
	Logs        []LogEntry `json:"logs"`
	ExportedAt  string     `json:"exported_at"`
}

// BuildExportArchive generates the complete sanitized export string.
func BuildExportArchive(snapshot Snapshot, logs []LogEntry, home string) (string, error) {
	sanitizedLogs := make([]LogEntry, len(logs))
	for i, e := range logs {
		sanitizedLogs[i] = LogEntry{
			Time:    e.Time,
			Level:   e.Level,
			Message: SanitizeText(e.Message, home),
			Raw:     SanitizeText(e.Raw, home),
		}
	}

	archive := ExportArchive{
		Diagnostics: snapshot,
		Logs:        sanitizedLogs,
		ExportedAt:  time.Now().UTC().Format(time.RFC3339),
	}

	data, err := json.MarshalIndent(archive, "", "  ")
	if err != nil {
		return "", err
	}
	return string(data), nil
}

// ExportToFile writes the sanitized bundle to a destination path.
func ExportToFile(destPath string, snapshot Snapshot, logs []LogEntry, home string) error {
	content, err := BuildExportArchive(snapshot, logs, home)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(destPath), 0o755); err != nil {
		return err
	}
	return os.WriteFile(destPath, []byte(content), 0o600)
}

// FormatSnapshotSummary creates a brief human-readable diagnostics overview.
func FormatSnapshotSummary(s Snapshot) string {
	return fmt.Sprintf("Shardlane v%s (%s/%s) | Go %s | MyGo %s | Herdr proto %d",
		s.ShardlaneVersion, s.OS, s.Arch, s.GoVersion, s.MyGoVersion, s.HerdrProtocol)
}
