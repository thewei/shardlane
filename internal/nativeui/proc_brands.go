package nativeui

import (
	"embed"
	"strings"
	"sync"

	"github.com/egoist/mygo/ui"
)

/**
 * [INPUT]: 依赖 mygo/ui 的 SVG, herdr-client/internal/herdr 的 Pane 数据, 内嵌 brands/*.svg 资源
 * [OUTPUT]: 对外提供 procBrand, procBrandAssets, procFriendlyName, isShell, Shell.procMarkForPane
 * [POS]: 进程与工具品牌标识解析器，将 Herdr 观察到的前台进程映射为矢量图标与友好名称，被 terminal/project_tree/breadcrumb 消费
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// Runtime brand marks for sidebar Pane rows and pane card headers: when a
// Pane's foreground process is a recognizable program — a dev server, a
// database, an editor, or an Agent CLI — its header and row show that
// program's brand icon instead of the generic terminal glyph. The SVGs come
// from the gorex reference project's brand set (CC0, brands/LICENSE.md);
// unknown programs keep the default mark.

//go:embed brands/*.svg
var procBrandFS embed.FS

var (
	procBrandOnce   sync.Once
	procBrandSVGs   = map[string]*ui.SVG{}
	procBrandByProc = map[string]string{}
	procBrandMu     sync.Mutex
)

// procBrandAssets maps a foreground process name (lowercased, as Herdr's
// pane.process_info reports it) to its brand asset. One program may come
// under several names.
var procBrandAssets = map[string]string{
	"claude":         "claude",
	"codex":          "openai",
	"node":           "nodedotjs",
	"nodejs":         "nodedotjs",
	"tsx":            "nodedotjs",
	"ts-node":        "nodedotjs",
	"vite":           "nodedotjs",
	"next":           "nodedotjs",
	"npm":            "npm",
	"npx":            "npm",
	"pnpm":           "pnpm",
	"yarn":           "yarn",
	"bun":            "bun",
	"bunx":           "bun",
	"deno":           "deno",
	"python":         "python",
	"python3":        "python",
	"python3.12":     "python",
	"pip":            "python",
	"uvicorn":        "python",
	"gunicorn":       "python",
	"go":             "go",
	"golang":         "go",
	"cargo":          "rust",
	"rustc":          "rust",
	"rust-analyzer":  "rust",
	"docker":         "docker",
	"dockerd":        "docker",
	"docker-compose": "docker",
	"ruby":           "ruby",
	"rails":          "ruby",
	"rake":           "ruby",
	"irb":            "ruby",
	"swift":          "swift",
	"swiftc":         "swift",
	"php":            "php",
	"php-fpm":        "php",
	"psql":           "postgresql",
	"postgres":       "postgresql",
	"postgresql":     "postgresql",
	"mysqld":         "mysql",
	"mysql":          "mysql",
	"redis-server":   "redis",
	"redis-cli":      "redis",
	"tmux":           "tmux",
	"nvim":           "neovim",
	"vim":            "vim",
	"vi":             "vim",
	"bash":           "gnubash",
	"sh":             "gnubash",
	"emacs":          "gnuemacs",
	"hx":             "helix",
	"kotlin":         "kotlin",
	"kotlinc":        "kotlin",
	"git":            "git",
	"lazygit":        "git",
	"tig":            "git",
	"htop":           "htop",
	"btop":           "htop",
	"top":            "htop",
}

// procBrand returns the brand SVG for a foreground process name, or nil
// when the program has no known mark.
func procBrand(proc string) *ui.SVG {
	procBrandOnce.Do(func() {
		entries, err := procBrandFS.ReadDir("brands")
		if err == nil {
			for _, entry := range entries {
				name := entry.Name()
				if !strings.HasSuffix(name, ".svg") {
					continue
				}
				data, err := procBrandFS.ReadFile("brands/" + name)
				if err != nil {
					continue
				}
				procBrandSVGs[strings.TrimSuffix(name, ".svg")] = ui.MustParseSVG(data)
			}
		}
		// Index the process-name table against the assets that actually
		// shipped; a name without its asset falls through to the digit-strip
		// retry and nil, never to a missing SVG.
		for proc, asset := range procBrandAssets {
			if _, ok := procBrandSVGs[asset]; ok {
				procBrandByProc[proc] = asset
			}
		}
	})
	procBrandMu.Lock()
	defer procBrandMu.Unlock()

	base := strings.ToLower(strings.TrimSpace(proc))
	base = strings.TrimSuffix(base, ".exe")
	if base == "" {
		return nil
	}
	for {
		if asset, ok := procBrandByProc[base]; ok {
			return procBrandSVGs[asset]
		}
		// Versioned binaries report as python3.13 or go1.24: strip the
		// trailing .NNN / digit run and try the shorter name.
		cut := strings.LastIndexAny(base, ".0123456789")
		if cut <= 0 {
			return nil
		}
		base = base[:cut]
	}
}

// procMarkForPane resolves one Pane's runtime brand mark from the service
// index's foreground process observation. Nil when the Pane runs nothing
// with a known mark (Agent panes keep their provider mark).
func (s *Shell) procMarkForPane(paneID string) (visualMark, bool) {
	if act, ok := s.serviceIndex.Panes[paneID]; ok {
		if svg := procBrand(act.Proc); svg != nil {
			return visualMark{svg: svg}, true
		}
	}
	return visualMark{}, false
}

var procFriendlyNames = map[string]string{
	"codex":          "Codex",
	"claude":         "Claude Code",
	"node":           "Node",
	"nodejs":         "Node",
	"tsx":            "Node",
	"ts-node":        "Node",
	"vite":           "Vite",
	"next":           "Next.js",
	"bun":            "Bun",
	"deno":           "Deno",
	"python":         "Python",
	"python3":        "Python",
	"lazygit":        "Git Changes",
	"tig":            "Git Log",
	"git":            "Git",
	"vim":            "Vim",
	"nvim":           "Neovim",
	"hx":             "Helix",
	"emacs":          "Emacs",
	"htop":           "Activity",
	"btop":           "Activity",
	"top":            "Activity",
	"ssh":            "SSH",
	"docker":         "Docker",
	"docker-compose": "Docker Compose",
	"go":             "Go",
	"cargo":          "Cargo",
	"rustc":          "Rust",
	"npm":            "npm",
	"pnpm":           "pnpm",
	"yarn":           "Yarn",
	"ruby":           "Ruby",
	"php":            "PHP",
	"psql":           "PostgreSQL",
	"postgres":       "PostgreSQL",
	"postgresql":     "PostgreSQL",
	"mysql":          "MySQL",
	"mysqld":         "MySQL",
	"redis-cli":      "Redis",
	"redis-server":   "Redis",
	"tmux":           "tmux",
	"swift":          "Swift",
	"kotlin":         "Kotlin",
	"bash":           "Bash",
	"zsh":            "Zsh",
	"fish":           "Fish",
	"powershell":     "PowerShell",
	"pwsh":           "PowerShell",
	"cmd":            "Command Prompt",
}

var knownShells = map[string]bool{
	"zsh": true, "bash": true, "fish": true, "sh": true, "dash": true,
	"nu": true, "pwsh": true, "elvish": true, "xonsh": true, "tcsh": true,
	"csh": true, "ksh": true, "cmd": true, "powershell": true,
}

// isShell reports whether proc is a recognized shell executable.
func isShell(proc string) bool {
	base := strings.ToLower(strings.TrimSpace(proc))
	base = strings.TrimSuffix(base, ".exe")
	return knownShells[base]
}

// procFriendlyName maps a process binary name to its human-readable title.
func procFriendlyName(proc string) string {
	base := strings.ToLower(strings.TrimSpace(proc))
	base = strings.TrimSuffix(base, ".exe")
	if name, ok := procFriendlyNames[base]; ok {
		return name
	}
	if cut := strings.LastIndexAny(base, ".0123456789"); cut > 0 {
		if name, ok := procFriendlyNames[base[:cut]]; ok {
			return name
		}
	}
	if len(base) > 0 {
		return strings.ToUpper(base[:1]) + base[1:]
	}
	return proc
}
