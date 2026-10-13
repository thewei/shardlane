package agenthooks

import (
	"bytes"
	"encoding/json"
	"fmt"
	"log/slog"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
)

/**
 * [INPUT]: 依赖 os/filepath/encoding/json、内嵌 hookScriptTemplate（script.go）与用户 HOME 下的 provider 配置文件
 * [OUTPUT]: 对外提供 Manager、NewManager、NewManagerWithHome、ProviderID/State/Diagnosis、Manager.Diagnose、Manager.Install
 * [POS]: Agent Hook 一键安装与环境诊断（Hook Doctor）的领域包：脚本供给、provider 接线检测与安全幂等写入；UI 在 nativeui，socket 服务在 apphook
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// ProviderID names one supported coding-agent CLI.
type ProviderID string

const (
	ProviderClaude      ProviderID = "claude"
	ProviderAntigravity ProviderID = "antigravity"
	ProviderAider       ProviderID = "aider"
)

// DisplayName is the user-facing provider name.
func (p ProviderID) DisplayName() string {
	switch p {
	case ProviderClaude:
		return "Claude Code"
	case ProviderAntigravity:
		return "Antigravity CLI"
	case ProviderAider:
		return "Aider"
	default:
		return string(p)
	}
}

// State is the presentation-safe hook diagnosis vocabulary.
type State string

const (
	StateOK           State = "ok"
	StateNotInstalled State = "not-installed"
	StateOutdated     State = "outdated"
	StateUnsupported  State = "unsupported"
	StateError        State = "error"
)

// Diagnosis is one provider's reconciled hook row. Detail is composed
// user-language text; the UI renders it verbatim.
type Diagnosis struct {
	Provider ProviderID `json:"provider"`
	State    State      `json:"state"`
	Detail   string     `json:"detail,omitempty"`
	Path     string     `json:"path,omitempty"`
	Version  int        `json:"version,omitempty"` // installed script version, 0 = none
	Wired    []string   `json:"wired,omitempty"`   // event names with our wiring
	Missing  []string   `json:"missing,omitempty"` // wanted event names without wiring
}

// Actionable reports whether the row exposes the one safe install/update
// action. Actions never run from render; the Providers page owns timing.
func (d Diagnosis) Actionable() bool {
	return d.State == StateNotInstalled || d.State == StateOutdated
}

// ActionLabel names the safe action for the row, if any.
func (d Diagnosis) ActionLabel() string {
	switch d.State {
	case StateNotInstalled:
		return "Install"
	case StateOutdated:
		return "Update"
	default:
		return ""
	}
}

// Manager provisions and diagnoses the Shardlane direct-connect agent
// hooks. It writes exactly two kinds of files: our own scripts under
// ~/.shardlane/hooks/ (never overwriting foreign files) and narrow,
// idempotent edits to each provider's hook wiring — everything else in
// the provider configs is preserved byte-for-value with a rolling backup.
type Manager struct {
	home string
}

// NewManager returns a Manager against the real user home.
func NewManager() *Manager {
	home, err := os.UserHomeDir()
	if err != nil || home == "" {
		home = "."
	}
	return &Manager{home: home}
}

// NewManagerWithHome injects the home root; it is the test seam.
func NewManagerWithHome(home string) *Manager {
	return &Manager{home: home}
}

// claudeHookEvents is the proven wiring set for Claude Code. The script
// also understands UserPromptSubmit/SessionEnd; they stay unwired until a
// verified need lands (the doctor provisions only what shipped verified).
var claudeHookEvents = []string{"SessionStart", "PreToolUse", "PostToolUse", "PermissionRequest", "Stop"}

// antigravityHookEvents is the proven wiring set for Antigravity CLI,
// grouped under the "shardlane-hook" owner key of ~/.gemini/config/hooks.json.
var antigravityHookEvents = []string{"PreInvocation", "Stop"}

// antigravityOwnerKey is the hooks.json group this package owns. Foreign
// groups (herdr, orca-status, …) are never touched.
const antigravityOwnerKey = "shardlane-hook"

type providerSpec struct {
	id         ProviderID
	scriptName string
	events     []string
}

func (m *Manager) spec(id ProviderID) (providerSpec, bool) {
	switch id {
	case ProviderClaude:
		return providerSpec{id: id, scriptName: "shardlane-claude-hook.sh", events: claudeHookEvents}, true
	case ProviderAntigravity:
		return providerSpec{id: id, scriptName: "shardlane-antigravity-hook.sh", events: antigravityHookEvents}, true
	default:
		return providerSpec{}, false
	}
}

func (m *Manager) hooksDir() string { return filepath.Join(m.home, ".shardlane", "hooks") }

func (m *Manager) scriptPath(id ProviderID) string {
	spec, ok := m.spec(id)
	if !ok {
		return ""
	}
	return filepath.Join(m.hooksDir(), spec.scriptName)
}

func (m *Manager) claudeSettingsPath() string {
	return filepath.Join(m.home, ".claude", "settings.json")
}

func (m *Manager) antigravityHooksPath() string {
	return filepath.Join(m.home, ".gemini", "config", "hooks.json")
}

// socketPath mirrors apphook.DefaultSocketPath for diagnosis detail only.
func (m *Manager) socketPath() string {
	return filepath.Join(m.home, ".shardlane", "run", "agent-status.sock")
}

// Diagnose reconciles every supported provider's hook state, plus the
// Aider row that explains why it is not provisionable. Pure reads; safe
// on route entry and manual refresh.
func (m *Manager) Diagnose() []Diagnosis {
	rows := []Diagnosis{m.diagnoseProvider(ProviderClaude), m.diagnoseProvider(ProviderAntigravity)}
	rows = append(rows, m.diagnoseAider())
	return rows
}

func (m *Manager) diagnoseProvider(id ProviderID) Diagnosis {
	if _, ok := m.spec(id); !ok {
		return Diagnosis{Provider: id, State: StateUnsupported, Detail: "Unsupported provider"}
	}
	path := m.scriptPath(id)
	row := Diagnosis{Provider: id, Path: path}

	owned, version, err := readOwnedScript(path)
	switch {
	case err != nil:
		row.State = StateError
		row.Detail = err.Error()
		return row
	case owned == foreignOwned:
		row.State = StateError
		row.Detail = fmt.Sprintf("%s exists but was not installed by Shardlane; refusing to touch it", path)
		return row
	case version == 0:
		row.State = StateNotInstalled
	case version < hookScriptVersion:
		row.State = StateOutdated
	default:
		row.State = StateOK
	}
	row.Version = version

	wired, missing := m.wiringState(id, claudeHookEvents)
	if id == ProviderAntigravity {
		wired, missing = m.wiringState(id, antigravityHookEvents)
	}
	row.Wired, row.Missing = wired, missing
	if len(missing) > 0 && (row.State == StateOK || row.State == StateOutdated) {
		// One Install action fixes script and wiring together, so any
		// missing wiring presents as the same actionable state.
		row.State = StateNotInstalled
	}
	row.Detail = m.describe(row)
	return row
}

// diagnoseAider reports the honest fallback: Aider ships no hook
// mechanism, so pane status comes from Herdr's own screen detection.
func (m *Manager) diagnoseAider() Diagnosis {
	row := Diagnosis{Provider: ProviderAider, State: StateUnsupported}
	config := filepath.Join(m.home, ".aider.conf.yml")
	if _, err := os.Stat(config); err == nil {
		row.Detail = "Aider has no hook mechanism; its pane status comes from Herdr screen detection (no action needed)"
		return row
	}
	row.Detail = "Not detected on this machine; if you use Aider, its pane status comes from Herdr screen detection"
	return row
}

func (m *Manager) describe(row Diagnosis) string {
	switch row.State {
	case StateOK:
		return fmt.Sprintf("Hook script v%d wired to all %d events; reports to %s", row.Version, len(row.Wired), m.socketPath())
	case StateOutdated:
		return fmt.Sprintf("Hook script v%d is outdated (want v%d); updating rewrites the script and keeps wiring", row.Version, hookScriptVersion)
	case StateNotInstalled:
		missing := ""
		if len(row.Missing) > 0 {
			missing = "; missing wiring: " + strings.Join(row.Missing, ", ")
		}
		if row.Version == 0 {
			return "Hook script not installed" + missing
		}
		return fmt.Sprintf("Hook script v%d present but not fully wired", row.Version) + missing
	default:
		return row.Detail
	}
}

// wiringState checks each wanted event's wiring for our script command.
func (m *Manager) wiringState(id ProviderID, events []string) (wired, missing []string) {
	for _, event := range events {
		if m.eventWired(id, event) {
			wired = append(wired, event)
		} else {
			missing = append(missing, event)
		}
	}
	sort.Strings(wired)
	sort.Strings(missing)
	return wired, missing
}

// Install provisions one provider end to end: write the script (never
// overwriting foreign files), then wire the provider config idempotently.
// It returns the fresh diagnosis; call it from a real user action only.
func (m *Manager) Install(id ProviderID) (Diagnosis, error) {
	if _, ok := m.spec(id); !ok {
		return Diagnosis{}, fmt.Errorf("unsupported provider %q", id)
	}
	if err := m.writeScript(id); err != nil {
		return Diagnosis{}, err
	}
	switch id {
	case ProviderClaude:
		if err := m.wireClaude(); err != nil {
			return Diagnosis{}, err
		}
	case ProviderAntigravity:
		if err := m.wireAntigravity(); err != nil {
			return Diagnosis{}, err
		}
	}
	slog.Info("agent hooks provisioned", "provider", string(id), "script", m.scriptPath(id))
	return m.diagnoseProvider(id), nil
}

// writeScript installs the universal hook script at the provider's path.
// Foreign files at the same path are never touched.
func (m *Manager) writeScript(id ProviderID) error {
	path := m.scriptPath(id)
	owned, version, err := readOwnedScript(path)
	if err != nil {
		return err
	}
	if owned == foreignOwned {
		return fmt.Errorf("%s exists but was not installed by Shardlane; remove or rename it first", path)
	}
	if owned == oursOwned && version == hookScriptVersion {
		return nil
	}
	if err := os.MkdirAll(m.hooksDir(), 0o755); err != nil {
		return fmt.Errorf("create %s: %w", m.hooksDir(), err)
	}
	if err := atomicWrite(path, []byte(hookScriptTemplate), 0o755); err != nil {
		return err
	}
	slog.Info("agent hook script written", "path", path, "version", hookScriptVersion)
	return nil
}

type scriptOwnership int

const (
	scriptMissing scriptOwnership = iota
	oursOwned
	foreignOwned
)

var versionRe = regexp.MustCompile(`(?m)^#\s*SHARDLANE_HOOK_VERSION=(\d+)\s*$`)

// readOwnedScript classifies the file at path and extracts its marker
// version. owned=foreignOwned means a foreign file occupies our path.
func readOwnedScript(path string) (owned scriptOwnership, version int, err error) {
	data, readErr := os.ReadFile(path)
	if readErr != nil {
		if os.IsNotExist(readErr) {
			return scriptMissing, 0, nil
		}
		return scriptMissing, 0, fmt.Errorf("read %s: %w", path, readErr)
	}
	if !strings.Contains(string(data), hookScriptMarker) {
		return foreignOwned, 0, nil
	}
	if match := versionRe.FindSubmatch(data); match != nil {
		if parsed, parseErr := strconv.Atoi(string(match[1])); parseErr == nil {
			version = parsed
		}
	}
	return oursOwned, version, nil
}

// wireClaude idempotently adds our command under each wanted event of
// ~/.claude/settings.json's hooks map, preserving all other content.
func (m *Manager) wireClaude() error {
	path := m.claudeSettingsPath()
	root, err := readJSONObject(path)
	if err != nil {
		return err
	}
	changed := false
	hooks := objectOf(root["hooks"])
	command := fmt.Sprintf("bash %s claude", shellQuote(m.scriptPath(ProviderClaude)))
	for _, event := range claudeHookEvents {
		entries := arrayOf(hooks[event])
		if wiringContains(entries, filepath.Base(m.scriptPath(ProviderClaude))) {
			continue
		}
		entry := map[string]any{
			"matcher": "*",
			"hooks": []any{map[string]any{
				"type":    "command",
				"command": command,
			}},
		}
		hooks[event] = append(entries, entry)
		changed = true
	}
	root["hooks"] = hooks
	if !changed {
		return nil
	}
	return backupAndWriteJSON(path, root)
}

// wireAntigravity idempotently adds our event commands under the
// "shardlane-hook" owner key of ~/.gemini/config/hooks.json, preserving
// foreign owner groups (herdr, orca-status, …).
func (m *Manager) wireAntigravity() error {
	path := m.antigravityHooksPath()
	root, err := readJSONObject(path)
	if err != nil {
		return err
	}
	group := objectOf(root[antigravityOwnerKey])
	changed := false
	script := m.scriptPath(ProviderAntigravity)
	for _, event := range antigravityHookEvents {
		entries := arrayOf(group[event])
		if wiringContains(entries, filepath.Base(script)) {
			continue
		}
		group[event] = append(entries, map[string]any{
			"type":    "command",
			"command": fmt.Sprintf("bash %s agy %s", shellQuote(script), event),
			"timeout": json.Number("5"),
		})
		changed = true
	}
	if enabled, ok := group["enabled"].(bool); !ok || !enabled {
		group["enabled"] = true
		changed = true
	}
	root[antigravityOwnerKey] = group
	if !changed {
		return nil
	}
	return backupAndWriteJSON(path, root)
}

func (m *Manager) eventWired(id ProviderID, event string) bool {
	var data []byte
	var err error
	switch id {
	case ProviderClaude:
		data, err = os.ReadFile(m.claudeSettingsPath())
	case ProviderAntigravity:
		data, err = os.ReadFile(m.antigravityHooksPath())
	default:
		return false
	}
	if err != nil {
		return false
	}
	var root map[string]any
	if err := json.Unmarshal(data, &root); err != nil {
		return false
	}
	var entries []any
	if id == ProviderClaude {
		entries = arrayOf(objectOf(root["hooks"])[event])
	} else {
		entries = arrayOf(objectOf(root[antigravityOwnerKey])[event])
	}
	return wiringContains(entries, filepath.Base(m.scriptPath(id)))
}

// wiringContains walks hook entry shapes (nested {matcher,hooks:[…]} and
// legacy flat {type,command}) looking for our script filename.
func wiringContains(entries []any, scriptName string) bool {
	for _, entry := range entries {
		obj, ok := entry.(map[string]any)
		if !ok {
			continue
		}
		if command, ok := obj["command"].(string); ok && strings.Contains(command, scriptName) {
			return true
		}
		if wiringContains(arrayOf(obj["hooks"]), scriptName) {
			return true
		}
	}
	return false
}

func shellQuote(path string) string {
	return "'" + strings.ReplaceAll(path, "'", `'\''`) + "'"
}

// readJSONObject loads a JSON object preserving number fidelity.
func readJSONObject(path string) (map[string]any, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return map[string]any{}, nil
		}
		return nil, fmt.Errorf("read %s: %w", path, err)
	}
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.UseNumber()
	var root map[string]any
	if err := decoder.Decode(&root); err != nil {
		return nil, fmt.Errorf("parse %s: %w", path, err)
	}
	return root, nil
}

// backupAndWriteJSON writes root back atomically, keeping one rolling
// backup beside the original (before the first modification of this run).
func backupAndWriteJSON(path string, root map[string]any) error {
	if original, err := os.ReadFile(path); err == nil {
		backup := path + ".shardlane-hook-backup"
		if err := atomicWrite(backup, original, 0o644); err != nil {
			return fmt.Errorf("backup %s: %w", backup, err)
		}
	}
	var buf bytes.Buffer
	encoder := json.NewEncoder(&buf)
	encoder.SetEscapeHTML(false)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(root); err != nil {
		return err
	}
	return atomicWrite(path, buf.Bytes(), 0o644)
}

// atomicWrite writes via a same-directory temp file plus rename, creating
// the parent directory when missing (fresh machines may lack ~/.claude or
// ~/.gemini/config entirely).
func atomicWrite(path string, data []byte, mode os.FileMode) error {
	if dir := filepath.Dir(path); dir != "" {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			return fmt.Errorf("create %s: %w", dir, err)
		}
	}
	tmp := path + ".tmp-shardlane"
	if err := os.WriteFile(tmp, data, mode); err != nil {
		return fmt.Errorf("write %s: %w", tmp, err)
	}
	if err := os.Rename(tmp, path); err != nil {
		_ = os.Remove(tmp)
		return fmt.Errorf("replace %s: %w", path, err)
	}
	return nil
}

func objectOf(value any) map[string]any {
	if obj, ok := value.(map[string]any); ok {
		return obj
	}
	return map[string]any{}
}

func arrayOf(value any) []any {
	if list, ok := value.([]any); ok {
		return list
	}
	return nil
}
