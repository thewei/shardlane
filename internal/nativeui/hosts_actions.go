package nativeui

/**
 * [INPUT]: 依赖 internal/sshconfig（索引/原子写回/ssh -G 求值）、herdr Manager（CreateTab/ShellReady/SendAgentKeys 启动事务）、Shell 投影与路由
 * [OUTPUT]: hosts 页动作：ensureHostsIndex/refreshHostsIndex、connectHost/launchSSH、openHostEditor/saveHostEditor、toggle/loadHostEffective、registerHostMachine、sshCommandLine
 * [POS]: 连接复用脚本启动事务原语（Tab/Pane/PTY 归 Herdr，Shardlane 只等 shell 并送入命令行）；保存走 sshconfig 原子写；日志只记操作名、运行时 ID 与计数
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"context"
	"errors"
	"fmt"
	"io/fs"
	"log/slog"
	"os"
	"path/filepath"
	"strings"

	"github.com/wh-studio/herdr-client/internal/herdr"
	"github.com/wh-studio/herdr-client/internal/sshconfig"
)

// hostsEditorMaxBytes bounds one editable config file. Real ssh configs are
// tiny; the cap keeps a pathological file from being pulled into memory.
const hostsEditorMaxBytes = 1 << 20

// hostsConfigSkeleton seeds a brand-new config file. It is editor text —
// nothing touches disk until an explicit save.
const hostsConfigSkeleton = `# OpenSSH client configuration.
# Host aliases saved here are listed on the Hosts page; ssh reads this
# file on every connection.

# Host myserver
#   HostName server.example.com
#   User deploy
`

// ensureHostsIndex lazily loads the listing on first page render.
func (s *Shell) ensureHostsIndex() {
	if !s.hosts.loaded && !s.hosts.loading {
		s.refreshHostsIndex(false)
	}
}

// refreshHostsIndex re-reads ~/.ssh/config and its Include closure on the
// background lane. A missing config is a first-run state, not an error.
func (s *Shell) refreshHostsIndex(force bool) {
	if s.hosts.loading {
		return
	}
	if !force && s.hosts.loaded {
		return
	}
	s.hosts.loading = true
	go func() {
		var (
			index   *sshconfig.Index
			readErr error
			missing bool
		)
		path, pathErr := sshconfig.DefaultPath()
		if pathErr != nil {
			readErr = pathErr
		} else {
			index, readErr = sshconfig.IndexPath(path)
			if errors.Is(readErr, fs.ErrNotExist) {
				missing, readErr = true, nil
			}
		}
		s.applyOnUI(func() {
			s.hosts.loading = false
			s.hosts.loaded = true
			s.hosts.missing = missing
			if readErr != nil {
				s.hosts.err = readErr.Error()
			} else {
				s.hosts.err = ""
			}
			if index != nil {
				s.hosts.mainPath = path
				s.hosts.entries = index.Entries
				s.hosts.groups = index.Groups
				s.hosts.files = index.Files
				slog.Debug("hosts index refreshed", "entries", len(index.Entries), "files", len(index.Files))
			}
		})
	}()
}

// connectHost opens a new Herdr-owned Tab in the selected Project and types
// the ssh line into its shell — the script-launch transaction, never a
// client PTY. The alias goes to ssh untouched; the system client resolves it.
func (s *Shell) connectHost(alias string) {
	alias = strings.TrimSpace(alias)
	if alias == "" {
		return
	}
	// Option-like tokens never reach an ssh argv (ssh_command would quote
	// them, and quoting does not stop flag parsing).
	if strings.HasPrefix(alias, "-") {
		s.status = "That host name looks like an ssh option and cannot be connected"
		return
	}
	if s.activeInstance == "" || s.selectedProjectID == "" {
		s.status = "Connecting needs an active workspace and project"
		return
	}
	instance, projectID := s.activeInstance, s.selectedProjectID
	cwd := s.projectCWD(projectID)
	s.status = "Opening an ssh tab…"
	go func() {
		projection, err := s.launchSSH(instance, projectID, cwd, alias)
		s.applyOnUI(func() {
			if err != nil {
				s.status = "ssh launch failed"
				s.errText = err.Error()
				return
			}
			slog.Info("ssh tab launched", "pane", projection.FocusedPaneID)
			s.router.Push(routeWorkspace)
			s.selectProject(projectID)
			if tabID := tabIDForPane(projection, projection.FocusedPaneID); tabID != "" && tabByID(s.projection, tabID) != nil {
				s.selectTab(tabID)
			}
			s.status = "ssh launched in a new tab"
		})
	}()
}

// launchSSH mirrors the verified script launch: create the Tab, wait for
// its shell, send the command line as terminal input.
func (s *Shell) launchSSH(instance, projectID, cwd, alias string) (herdr.Projection, error) {
	ctx, cancel := context.WithTimeout(context.Background(), scriptRunTimeout)
	defer cancel()
	projection, err := s.runtime.CreateTab(instance, projectID, cwd)
	if err != nil {
		return projection, fmt.Errorf("create ssh tab: %w", err)
	}
	paneID := projection.FocusedPaneID
	if paneID == "" {
		return projection, fmt.Errorf("ssh tab has no focused pane")
	}
	ready, err := s.runtime.ShellReady(ctx, instance, paneID)
	if err != nil {
		return projection, fmt.Errorf("probe ssh shell: %w", err)
	}
	if !ready {
		slog.Warn("ssh shell not confirmed ready; sending anyway", "pane", paneID)
	}
	if err := s.runtime.SendAgentKeys(ctx, instance, paneID, []string{sshCommandLine(alias)}); err != nil {
		return projection, fmt.Errorf("send ssh input: %w", err)
	}
	return projection, nil
}

func (s *Shell) projectCWD(projectID string) string {
	for _, project := range s.projection.Projects {
		if project.ID == projectID {
			return project.CWD
		}
	}
	return ""
}

func tabIDForPane(projection herdr.Projection, paneID string) string {
	for _, pane := range projection.Panes {
		if pane.ID == paneID {
			return pane.TabID
		}
	}
	return ""
}

// sshCommandLine renders the terminal line for one alias. Aliases come from
// the user's own config but may carry spaces (quoted Host patterns), so
// anything outside the safe set gets single-quoted for the interactive shell.
func sshCommandLine(alias string) string {
	quoted := alias
	if !safeAlias(alias) {
		quoted = "'" + strings.ReplaceAll(alias, "'", `'\''`) + "'"
	}
	return "ssh " + quoted + "\n"
}

func safeAlias(alias string) bool {
	if alias == "" || strings.HasPrefix(alias, "-") {
		return false
	}
	for _, r := range alias {
		switch {
		case r >= 'a' && r <= 'z', r >= 'A' && r <= 'Z', r >= '0' && r <= '9':
		case r == '.' || r == '-' || r == '_' || r == '@':
		default:
			return false
		}
	}
	return true
}

// openHostEditor loads one config file into the raw editor (create seeds
// the skeleton for a not-yet-existing file). A dirty editor blocks switching
// files so unsaved work cannot be silently dropped.
func (s *Shell) openHostEditor(path string, create bool) {
	if s.hosts.saving || path == "" {
		return
	}
	if s.hosts.editing && s.hosts.editorFile != path && s.hostsEditorDirty() {
		s.status = "Save or discard the open SSH config editor first"
		return
	}
	go func() {
		data, err := os.ReadFile(path)
		switch {
		case err == nil:
			if len(data) > hostsEditorMaxBytes {
				s.applyOnUI(func() { s.status = "SSH config is too large to edit here" })
				return
			}
		case create && errors.Is(err, fs.ErrNotExist):
			data = nil
		default:
			s.applyOnUI(func() { s.status = "Cannot read SSH config: " + err.Error() })
			return
		}
		text := string(data)
		loaded := text
		if create {
			text, loaded = hostsConfigSkeleton, ""
		}
		s.applyOnUI(func() {
			s.hosts.editing = true
			s.hosts.editorFile = path
			s.hosts.editorText = text
			s.hosts.editorLoaded = loaded
			s.hosts.editorErr = ""
			s.hosts.editorConfirmClose = false
			s.hosts.selected = ""
		})
	}()
}

// saveHostEditor writes exactly the editor text through sshconfig.SaveFile
// (backup, mode preservation, symlink-safe atomic replace).
func (s *Shell) saveHostEditor() {
	if !s.hosts.editing || s.hosts.saving {
		return
	}
	file, text := s.hosts.editorFile, s.hosts.editorText
	s.hosts.saving = true
	s.hosts.editorErr = ""
	go func() {
		err := sshconfig.SaveFile(file, text)
		s.applyOnUI(func() {
			s.hosts.saving = false
			if err != nil {
				s.hosts.editorErr = err.Error()
				return
			}
			slog.Info("ssh config saved", "bytes", len(text))
			s.closeHostEditor()
			s.status = "SSH config saved — previous copy kept as " + filepath.Base(file) + ".bak"
			s.refreshHostsIndex(true)
		})
	}()
}

// toggleHostEffective shows or loads the ssh -G view for one alias.
func (s *Shell) toggleHostEffective(alias string) {
	if s.hosts.effOpen && s.hosts.effFor == alias {
		s.hosts.effOpen = false
		return
	}
	s.loadHostEffective(alias)
}

func (s *Shell) loadHostEffective(alias string) {
	if s.hosts.effBusy {
		return
	}
	s.hosts.effOpen = true
	s.hosts.effFor = alias
	s.hosts.effBusy = true
	s.hosts.effErr = ""
	s.hosts.eff = nil
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), sshconfig.EffectiveTimeout)
		defer cancel()
		kvs, err := sshconfig.EffectiveConfig(ctx, alias)
		s.applyOnUI(func() {
			s.hosts.effBusy = false
			if err != nil {
				s.hosts.effErr = err.Error()
				return
			}
			s.hosts.eff = kvs
		})
	}()
}

// registerHostMachine resolves the alias to a concrete target and hands it
// to the existing Herdr machine flow (interactive `herdr machine add` in
// Terminal). The profile stays Herdr-owned end to end.
func (s *Shell) registerHostMachine(alias string) {
	alias = strings.TrimSpace(alias)
	if alias == "" || strings.HasPrefix(alias, "-") {
		return
	}
	s.status = "Resolving ssh target…"
	go func() {
		target := alias
		ctx, cancel := context.WithTimeout(context.Background(), sshconfig.EffectiveTimeout)
		if resolved, err := sshconfig.ResolveTarget(ctx, alias); err == nil && resolved != "" {
			target = resolved
		}
		cancel()
		s.applyOnUI(func() { s.addRemoteMachine(target) })
	}()
}
