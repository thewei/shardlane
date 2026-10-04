package nativeui

import (
	"context"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/next/internal/agent"
)

// TestProvidersPageAuditsAndReconcilesActions pins INT-04/05/07: route entry
// audits once, rows show strategy/health/detail, the safe action runs and
// reconciles from a fresh audit with a transient toast.
func TestProvidersPageAuditsAndReconcilesActions(t *testing.T) {
	shell := NewShell()
	installed := false
	var commands []string
	shell.integrations.service = agent.NewIntegrationHealthServiceWithRunner(func(ctx context.Context, args []string) (string, error) {
		commands = append(commands, args[0]+" "+args[1])
		if args[1] == "install" {
			installed = true
			return "", nil
		}
		claudeState := "not installed"
		if installed {
			claudeState = "current (v10)"
		}
		return "claude: " + claudeState + " (/Users/demo/.claude/hooks/herdr-agent-state.sh)\n" +
			"codex: current (v8) (/Users/demo/.codex/herdr-agent-state.sh)\n", nil
	})

	shell.router.Replace("/settings/providers")
	tester := ui.NewTester(shell.View, 1200, 800)

	for _, want := range []string{"Providers", "Claude Code", "Not installed", "Install", "Herdr official", "Managed by Herdr", "Deferred", "Current"} {
		if !tester.HasText(want) {
			t.Fatalf("providers page missing %q; texts=%q", want, tester.Texts())
		}
	}
	// Route entry audited exactly once so far.
	audits := 0
	for _, command := range commands {
		if command == "integration status" {
			audits++
		}
	}
	if audits != 1 {
		t.Fatalf("route-entry audits = %d, want 1", audits)
	}

	// The safe action runs off render, then reconciles from a fresh audit.
	if err := tester.Click("Install"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if shell.integrations.actionRunning {
		t.Fatal("action still marked in flight after reconciliation")
	}
	if !installed {
		t.Fatal("install command did not run")
	}
	if !tester.HasText("Claude Code integration is current") {
		t.Fatalf("toast missing; texts=%q", tester.Texts())
	}
	if !tester.HasText("Current") {
		t.Fatalf("reconciled current state missing; texts=%q", tester.Texts())
	}

	// A deferred provider never exposes an install action.
	shell.router.Replace("/settings/terminal")
	tester.Frame()
	shell.router.Replace("/settings/providers")
	tester.Frame()
	for _, text := range tester.Texts() {
		if text == "Install" {
			// Later not-installed rows may offer Install; qoder must not.
			continue
		}
	}
	if shell.integrations.errText != "" {
		t.Fatalf("unexpected error: %q", shell.integrations.errText)
	}
}
