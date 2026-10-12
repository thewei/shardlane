package nativeui

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/agenthooks"
)

// TestProvidersPageHookLeadsIntegrationFallsBack pins the single-mechanism
// surface: the direct status hook is the lead facet for hook-covered
// providers, the Herdr integration is demoted to a fallback lane whose
// repair action only stays available while the hook cannot do the job, and
// each facet action still reconciles from a fresh audit.
func TestProvidersPageHookLeadsIntegrationFallsBack(t *testing.T) {
	shell := NewShell()
	installed := map[string]bool{}
	var commands []string
	shell.providers.integrations = agent.NewIntegrationHealthServiceWithRunner(func(ctx context.Context, args []string) (string, error) {
		commands = append(commands, args[0]+" "+args[1])
		if args[1] == "install" {
			installed[args[2]] = true
			return "", nil
		}
		state := func(target string) string {
			if installed[target] {
				return "current (v10)"
			}
			return "not installed"
		}
		return "claude: " + state("claude") + " (/Users/demo/.claude/hooks/herdr-agent-state.sh)\n" +
			"codex: current (v8) (/Users/demo/.codex/herdr-agent-state.sh)\n" +
			"antigravity-cli: " + state("antigravity-cli") + " (/Users/demo/.gemini/config/hooks/herdr-agent-state.sh)\n", nil
	})
	home := t.TempDir()
	shell.providers.hooks = agenthooks.NewManagerWithHome(home)

	shell.router.Replace("/settings/providers")
	tester := ui.NewTester(shell.View, 1200, 800)

	// Route entry audited the integration facet exactly once.
	audits := 0
	for _, command := range commands {
		if command == "integration status" {
			audits++
		}
	}
	if audits != 1 {
		t.Fatalf("route-entry integration audits = %d, want 1", audits)
	}

	// Claude Code renders as ONE hook-led card, not as two parallel rows
	// from two systems. The fallback lane is visible beside it.
	if got := countTextExact(tester.Texts(), "Claude Code"); got != 1 {
		t.Fatalf("Claude Code shown %d times, want one merged card; texts=%q", got, tester.Texts())
	}
	if !tester.HasText("Herdr integration") {
		t.Fatalf("fallback lane missing; texts=%q", tester.Texts())
	}

	// Both hook-covered providers start with an unhealthy hook, so each
	// fallback lane is still load-bearing and offers its repair action.
	if got := countTextExact(tester.Texts(), "Install integration"); got != 2 {
		t.Fatalf("fallback repair actions = %d, want 2; texts=%q", got, tester.Texts())
	}
	if got := countTextExact(tester.Texts(), "Install status hook"); got != 2 {
		t.Fatalf("hook actions = %d, want 2; texts=%q", got, tester.Texts())
	}

	// The remaining fallback repair (claude's hook is still not installed)
	// works end to end: herdr integration install runs off render and the
	// facet reconciles from a fresh audit.
	if err := tester.Click("Install integration"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !installed["claude"] {
		t.Fatalf("herdr integration install did not run; installed=%v", installed)
	}
	if !tester.HasText("Claude Code integration is current") {
		t.Fatalf("integration toast missing; texts=%q", tester.Texts())
	}
	if got := countTextExact(tester.Texts(), "Install integration"); got != 1 {
		t.Fatalf("fallback repair actions after repair = %d, want 1 (antigravity); texts=%q", got, tester.Texts())
	}

	// Installing Claude Code's hook flips its lead facet to Current and
	// demotes its fallback lane: the repair action disappears, leaving only
	// Antigravity's load-bearing fallback action.
	if err := tester.Click("Install status hook"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if shell.providers.actionRunning {
		t.Fatal("hook action still marked in flight after reconciliation")
	}
	script := filepath.Join(home, ".shardlane", "hooks", "shardlane-claude-hook.sh")
	if _, err := os.Stat(script); err != nil {
		t.Fatalf("hook script not installed: %v", err)
	}
	wiring, err := os.ReadFile(filepath.Join(home, ".claude", "settings.json"))
	if err != nil || !strings.Contains(string(wiring), "shardlane-claude-hook.sh") {
		t.Fatalf("claude settings not wired (err=%v)", err)
	}
	if !tester.HasText("Claude Code status hook is current") {
		t.Fatalf("hook toast missing; texts=%q", tester.Texts())
	}
	if !tester.HasText("Live status is on") {
		t.Fatalf("reconciled hook detail missing; texts=%q", tester.Texts())
	}
	if got := countTextExact(tester.Texts(), "Install integration"); got != 1 {
		t.Fatalf("fallback repair actions after hook install = %d, want 1; texts=%q", got, tester.Texts())
	}
	if got := countTextExact(tester.Texts(), "Install status hook"); got != 1 {
		t.Fatalf("hook actions after install = %d, want 1; texts=%q", got, tester.Texts())
	}
}

// TestProvidersPageHookOnlyAndDeferred pins the honest edges of the merged
// surface: hook-only extras (Aider) get one informational card with no
// action, and deferred providers never expose one.
func TestProvidersPageHookOnlyAndDeferred(t *testing.T) {
	shell := NewShell()
	shell.providers.integrations = agent.NewIntegrationHealthServiceWithRunner(func(ctx context.Context, args []string) (string, error) {
		return "", nil
	})
	shell.providers.hooks = agenthooks.NewManagerWithHome(t.TempDir())

	shell.router.Replace("/settings/providers")
	tester := ui.NewTester(shell.View, 1200, 800)

	if !tester.HasText("Aider") {
		t.Fatalf("aider informational card missing; texts=%q", tester.Texts())
	}
	if !tester.HasText("Status comes from Herdr's screen detection") {
		t.Fatalf("aider honest detail missing; texts=%q", tester.Texts())
	}
	if tester.HasText("Update integration") || tester.HasText("Update status hook") {
		t.Fatalf("unexpected update actions; texts=%q", tester.Texts())
	}
	if !tester.HasText("Not available yet") {
		t.Fatalf("deferred provider state missing; texts=%q", tester.Texts())
	}
	// The empty integration audit leaves every registry provider
	// unsupported or deferred: no integration action may appear. Hook
	// actions may legitimately appear for provisionable providers.
	if tester.HasText("Install integration") {
		t.Fatalf("integration action offered on a non-actionable audit; texts=%q", tester.Texts())
	}
}

func countTextExact(texts []string, want string) int {
	count := 0
	for _, text := range texts {
		if text == want {
			count++
		}
	}
	return count
}
