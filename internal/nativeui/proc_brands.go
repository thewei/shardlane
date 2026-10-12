package nativeui

import (
	"embed"
	"strings"
	"sync"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/history"
)

/**
 * [INPUT]: 依赖 mygo/ui 的 SVG, history.AgentID, 内嵌 brands/*.svg 资源与内置服务矢量图标
 * [OUTPUT]: 对外提供 procBrand, procBrandAssets, procFriendlyName, isShell, Shell.procMarkForPane, Shell.procMarkForPaneWithTheme
 * [POS]: 进程与服务/工具品牌标识解析器，将前台进程与 Agent CLI/微服务映射为矢量图标/品牌 Logo 与友好名称，被 terminal/project_tree/breadcrumb 消费
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

// builtinServiceSVGs provides crisp Lucide-compatible vector marks for
// popular dev servers, databases, cloud tools, and local inference runtimes
// that do not have standalone files in brands/*.svg.
var builtinServiceSVGs = map[string]*ui.SVG{
	"nginx":      icon(`<path d="M4 4h4l8 12V4h4v16h-4L8 8v12H4V4z"/>`),
	"caddy":      icon(`<circle cx="12" cy="12" r="9"/><path d="M15 9a4.5 4.5 0 1 0 0 6"/>`),
	"apache":     icon(`<path d="m14 2-8 12c-1.5 2-1 4 1 5s3.5-.5 5-2l6-9"/><path d="m9 10 7-3M7 15l6-3M12 7l-5 4"/>`),
	"traefik":    icon(`<circle cx="12" cy="5" r="3"/><circle cx="6" cy="19" r="3"/><circle cx="18" cy="19" r="3"/><path d="M12 8v4M12 12l-4 4M12 12l4 4"/>`),
	"mongodb":    icon(`<path d="M12 2s-7 6-7 12a7 7 0 0 0 14 0c0-6-7-12-7-12z"/><path d="M12 2v20"/>`),
	"sqlite":     icon(`<ellipse cx="12" cy="6" rx="8" ry="3"/><path d="M4 6v6c0 1.66 3.58 3 8 3s8-1.34 8-3V6"/><path d="M4 12v6c0 1.66 3.58 3 8 3s8-1.34 8-3v-6"/>`),
	"clickhouse": icon(`<rect x="4" y="8" width="2.5" height="12" rx="1"/><rect x="8.5" y="4" width="2.5" height="16" rx="1"/><rect x="13" y="10" width="2.5" height="10" rx="1"/><rect x="17.5" y="6" width="2.5" height="14" rx="1"/>`),
	"supabase":   icon(`<path d="m13 2-9 11h8l-1 9 9-11h-8l1-9z"/>`),
	"kubernetes": icon(`<circle cx="12" cy="12" r="8"/><circle cx="12" cy="12" r="3"/><path d="M12 4V1M12 23v-3M4 12H1M23 12h-3M6.34 6.34 4.22 4.22M19.78 19.78l-2.12-2.12M6.34 17.66l-2.12 2.12M19.78 4.22l-2.12 2.12"/>`),
	"terraform":  icon(`<rect x="3" y="3" width="7" height="8" rx="1"/><rect x="14" y="3" width="7" height="8" rx="1"/><rect x="8.5" y="13" width="7" height="8" rx="1"/>`),
	"java":       icon(`<path d="M17 8h1a4 4 0 0 1 0 8h-1M3 8h14v9a4 4 0 0 1-4 4H7a4 4 0 0 1-4-4Z"/><line x1="6" y1="2" x2="6" y2="4"/><line x1="10" y1="2" x2="10" y2="4"/><line x1="14" y1="2" x2="14" y2="4"/>`),
	"dotnet":     icon(`<circle cx="5" cy="18" r="1.5"/><path d="M9 6v12l6-12v12M19 6v12h4M19 12h3M19 6h4"/>`),
	"elixir":     icon(`<path d="M12 2c-4 5-7 9-7 13a7 7 0 0 0 14 0c0-4-3-8-7-13z"/><path d="M12 11a3 3 0 0 0-3 3c0 1.66 1.34 3 3 3"/>`),
	"cloudflare": icon(`<path d="M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9Z"/>`),
	"ollama":     icon(`<path d="M9 3v4M15 3v4M8 7h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2V9a2 2 0 0 1 2-2Z"/><circle cx="10" cy="12" r="1"/><circle cx="14" cy="12" r="1"/>`),
	"aider":      icon(`<circle cx="8" cy="9" r="3"/><circle cx="16" cy="9" r="3"/><path d="M4 19a5 5 0 0 1 8-2 5 5 0 0 1 8 2"/>`),
}

// agentProcAliases maps known coding agent CLI command names to their
// corresponding history.AgentID.
var agentProcAliases = map[string]history.AgentID{
	"antigravity":     history.AgentAntigravity,
	"agy":             history.AgentAntigravity,
	"antigravity-cli": history.AgentAntigravity,
	"claude":          history.AgentClaudeCode,
	"claude-code":     history.AgentClaudeCode,
	"codex":           history.AgentCodex,
	"codex-cli":       history.AgentCodex,
	"gemini":          history.AgentGemini,
	"gemini-cli":      history.AgentGemini,
	"copilot":         history.AgentCopilot,
	"github-copilot":  history.AgentCopilot,
	"gh-copilot":      history.AgentCopilot,
	"cursor":          history.AgentCursor,
	"cursor-agent":    history.AgentCursor,
	"opencode":        history.AgentOpenCode,
	"open-code":       history.AgentOpenCode,
	"command-code":    history.AgentCommandCode,
	"commandcode":     history.AgentCommandCode,
	"kiro":            history.AgentKiro,
	"pi":              history.AgentPi,
	"pi-coding-agent": history.AgentPi,
	"omp":             history.AgentOMP,
	"oh-my-pi":        history.AgentOMP,
	"grok":            history.AgentGrok,
	"grok-build":      history.AgentGrok,
	"kimi":            history.AgentKimi,
	"kimi-code":       history.AgentKimi,
	"deepseek":        history.AgentDSH,
	"dsh":             history.AgentDSH,
	"deepseek-coder":  history.AgentDSH,
	"qoder":           history.AgentQoder,
	"qoder-cli":       history.AgentQoder,
}

// agentForProc resolves an executable/process name to an AgentID if it represents
// a known coding agent CLI.
func agentForProc(proc string) (history.AgentID, bool) {
	base := strings.ToLower(strings.TrimSpace(proc))
	base = strings.TrimSuffix(base, ".exe")
	if base == "" {
		return "", false
	}
	if agent, ok := agentProcAliases[base]; ok {
		return agent, true
	}
	return history.ParseAgentID(base)
}

// procBrandAssets maps a foreground process name (lowercased, as Herdr's
// pane.process_info reports it) to its brand asset. One program may come
// under several names.
var procBrandAssets = map[string]string{
	// Web servers & reverse proxies
	"nginx":          "nginx",
	"caddy":          "caddy",
	"apache":         "apache",
	"httpd":          "apache",
	"traefik":        "traefik",
	"ngrok":          "cloudflare",
	"cloudflared":    "cloudflare",

	// Databases & storage
	"mongo":          "mongodb",
	"mongod":         "mongodb",
	"mongosh":        "mongodb",
	"sqlite":         "sqlite",
	"sqlite3":        "sqlite",
	"clickhouse":     "clickhouse",
	"clickhouse-server": "clickhouse",
	"clickhouse-client": "clickhouse",
	"cockroach":      "postgresql",
	"mariadb":        "mysql",
	"psql":           "postgresql",
	"postgres":       "postgresql",
	"postgresql":     "postgresql",
	"mysqld":         "mysql",
	"mysql":          "mysql",
	"redis-server":   "redis",
	"redis-cli":      "redis",
	"memcached":      "redis",
	"supabase":       "supabase",

	// Local AI / LLM & assistant CLIs
	"ollama":         "ollama",
	"vllm":           "ollama",
	"llama-server":   "ollama",
	"llamacpp":       "ollama",
	"aider":          "aider",
	"anthropic":      "anthropic",

	// Node.js & JS/TS toolchain
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
	"webpack":        "nodedotjs",
	"rollup":         "nodedotjs",
	"esbuild":        "nodedotjs",
	"turbo":          "nodedotjs",
	"turborepo":      "nodedotjs",
	"parcel":         "nodedotjs",
	"astro":          "nodedotjs",
	"remix":          "nodedotjs",
	"nuxt":           "nodedotjs",
	"svelte":         "nodedotjs",
	"serve":          "nodedotjs",
	"http-server":    "nodedotjs",
	"prisma":         "nodedotjs",
	"drizzle-kit":    "nodedotjs",

	// Python toolchain & frameworks
	"python":         "python",
	"python3":        "python",
	"python3.12":     "python",
	"pip":            "python",
	"uv":             "python",
	"poetry":         "python",
	"pipenv":         "python",
	"pdm":            "python",
	"conda":          "python",
	"mamba":          "python",
	"uvicorn":        "python",
	"gunicorn":       "python",
	"celery":         "python",
	"flask":          "python",
	"django":         "python",
	"fastapi":        "python",
	"streamlit":      "python",
	"gradio":         "python",
	"pytest":         "python",
	"ruff":           "python",
	"black":          "python",
	"mypy":           "python",
	"jupyter":        "python",
	"ipython":        "python",

	// Go toolchain & servers
	"go":             "go",
	"golang":         "go",
	"air":            "go",
	"hugo":           "go",
	"gin":            "go",
	"fiber":          "go",

	// Rust toolchain
	"cargo":          "rust",
	"rustc":          "rust",
	"rust-analyzer":  "rust",
	"cargo-watch":    "rust",
	"tauri":          "rust",
	"trunk":          "rust",

	// Java / JVM
	"java":           "java",
	"javac":          "java",
	"mvn":            "java",
	"maven":          "java",
	"gradle":         "java",
	"gradlew":        "java",
	"sbt":            "java",
	"kotlin":         "kotlin",
	"kotlinc":        "kotlin",

	// .NET & Elixir
	"dotnet":         "dotnet",
	"csharp":         "dotnet",
	"elixir":         "elixir",
	"mix":            "elixir",
	"iex":            "elixir",

	// Containers & DevOps
	"docker":         "docker",
	"dockerd":        "docker",
	"docker-compose": "docker",
	"podman":         "docker",
	"colima":         "docker",
	"kubectl":        "kubernetes",
	"k9s":            "kubernetes",
	"minikube":       "kubernetes",
	"kind":           "kubernetes",
	"helm":           "kubernetes",
	"terraform":      "terraform",
	"tofu":           "terraform",
	"pulumi":         "terraform",

	// Ruby & PHP & Swift
	"ruby":           "ruby",
	"rails":          "ruby",
	"rake":           "ruby",
	"irb":            "ruby",
	"swift":          "swift",
	"swiftc":         "swift",
	"php":            "php",
	"php-fpm":        "php",

	// Git, Editors, Terminal tools
	"git":            "git",
	"lazygit":        "git",
	"tig":            "git",
	"gh":             "git",
	"tmux":           "tmux",
	"nvim":           "neovim",
	"vim":            "vim",
	"vi":             "vim",
	"bash":           "gnubash",
	"sh":             "gnubash",
	"emacs":          "gnuemacs",
	"hx":             "helix",
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
		// Register inline service SVGs
		for name, svg := range builtinServiceSVGs {
			procBrandSVGs[name] = svg
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
// index's foreground process observation. If the process is an Agent CLI,
// it returns that Agent's brand mark; if it is a recognized service/tool,
// it returns the tool's brand SVG; otherwise it reports false.
func (s *Shell) procMarkForPane(paneID string) (visualMark, bool) {
	return s.procMarkForPaneWithTheme(paneID, true)
}

// procMarkForPaneWithTheme resolves one Pane's runtime brand mark respecting
// the active theme's dark/light setting for Agent marks.
func (s *Shell) procMarkForPaneWithTheme(paneID string, dark bool) (visualMark, bool) {
	act, ok := s.serviceIndex.Panes[paneID]
	if !ok || act.Proc == "" || isShell(act.Proc) {
		return visualMark{}, false
	}
	if agent, ok := agentForProc(act.Proc); ok {
		return agentMark(agent, dark), true
	}
	if svg := procBrand(act.Proc); svg != nil {
		return visualMark{svg: svg}, true
	}
	return visualMark{}, false
}

var procFriendlyNames = map[string]string{
	// Agent CLIs
	"antigravity":     "Antigravity CLI",
	"agy":             "Antigravity CLI",
	"antigravity-cli": "Antigravity CLI",
	"codex":           "Codex",
	"codex-cli":       "Codex",
	"claude":          "Claude Code",
	"claude-code":     "Claude Code",
	"gemini":          "Gemini CLI",
	"gemini-cli":      "Gemini CLI",
	"copilot":         "Copilot CLI",
	"cursor":          "Cursor",
	"opencode":        "OpenCode",
	"command-code":    "Command Code",
	"commandcode":     "Command Code",
	"kiro":            "Kiro",
	"pi":              "Pi",
	"omp":             "Oh My Pi",
	"grok":            "Grok Build",
	"kimi":            "Kimi Code",
	"deepseek":        "DeepSeek",
	"dsh":             "DeepSeek Harness",
	"qoder":           "Qoder",
	"aider":           "Aider",

	// Web servers & proxies
	"nginx":          "Nginx",
	"caddy":          "Caddy",
	"apache":         "Apache",
	"httpd":          "Apache",
	"traefik":        "Traefik",
	"ngrok":          "ngrok",
	"cloudflared":    "Cloudflare Tunnel",

	// Databases & storage
	"mongo":          "MongoDB",
	"mongod":         "MongoDB",
	"mongosh":        "MongoDB",
	"sqlite":         "SQLite",
	"sqlite3":        "SQLite",
	"clickhouse":     "ClickHouse",
	"clickhouse-server": "ClickHouse",
	"cockroach":      "CockroachDB",
	"mariadb":        "MariaDB",
	"psql":           "PostgreSQL",
	"postgres":       "PostgreSQL",
	"postgresql":     "PostgreSQL",
	"mysql":          "MySQL",
	"mysqld":         "MySQL",
	"redis-cli":      "Redis",
	"redis-server":   "Redis",
	"memcached":      "Memcached",
	"supabase":       "Supabase",

	// Local AI / LLM
	"ollama":         "Ollama",
	"vllm":           "vLLM",
	"llama-server":   "llama.cpp Server",

	// Node.js & Web dev
	"node":           "Node",
	"nodejs":         "Node",
	"tsx":            "Node",
	"ts-node":        "Node",
	"vite":           "Vite",
	"next":           "Next.js",
	"bun":            "Bun",
	"deno":           "Deno",
	"npm":            "npm",
	"pnpm":           "pnpm",
	"yarn":           "Yarn",
	"webpack":        "Webpack",
	"rollup":         "Rollup",
	"esbuild":        "esbuild",
	"turbo":          "Turborepo",
	"turborepo":      "Turborepo",
	"astro":          "Astro",
	"nuxt":           "Nuxt",
	"remix":          "Remix",
	"svelte":         "Svelte",
	"prisma":         "Prisma",

	// Python
	"python":         "Python",
	"python3":        "Python",
	"uv":             "uv",
	"poetry":         "Poetry",
	"pipenv":         "Pipenv",
	"conda":          "Conda",
	"celery":         "Celery",
	"flask":          "Flask",
	"django":         "Django",
	"fastapi":        "FastAPI",
	"streamlit":      "Streamlit",
	"jupyter":        "Jupyter",

	// Go & Rust
	"go":             "Go",
	"cargo":          "Cargo",
	"rustc":          "Rust",
	"air":            "Air",
	"hugo":           "Hugo",
	"tauri":          "Tauri",

	// JVM, .NET, Elixir
	"java":           "Java",
	"mvn":            "Maven",
	"maven":          "Maven",
	"gradle":         "Gradle",
	"gradlew":        "Gradle",
	"kotlin":         "Kotlin",
	"dotnet":         ".NET",
	"elixir":         "Elixir",

	// Containers & DevOps
	"docker":         "Docker",
	"docker-compose": "Docker Compose",
	"podman":         "Podman",
	"colima":         "Colima",
	"kubectl":        "Kubectl",
	"k9s":            "k9s",
	"helm":           "Helm",
	"minikube":       "Minikube",
	"terraform":      "Terraform",
	"pulumi":         "Pulumi",

	// Git, Editors, System
	"lazygit":        "Git Changes",
	"tig":            "Git Log",
	"git":            "Git",
	"gh":             "GitHub CLI",
	"vim":            "Vim",
	"nvim":           "Neovim",
	"hx":             "Helix",
	"emacs":          "Emacs",
	"htop":           "Activity",
	"btop":           "Activity",
	"top":            "Activity",
	"ssh":            "SSH",
	"tmux":           "tmux",
	"ruby":           "Ruby",
	"php":            "PHP",
	"swift":          "Swift",
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
	if agent, ok := agentForProc(base); ok {
		return agent.DisplayName()
	}
	if cut := strings.LastIndexAny(base, ".0123456789"); cut > 0 {
		if name, ok := procFriendlyNames[base[:cut]]; ok {
			return name
		}
		if agent, ok := agentForProc(base[:cut]); ok {
			return agent.DisplayName()
		}
	}
	if len(base) > 0 {
		return strings.ToUpper(base[:1]) + base[1:]
	}
	return proc
}
