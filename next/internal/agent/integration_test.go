package agent

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/wh-studio/herdr-client/next/internal/history"
)

const integrationStatusFixture = `pi: current (v9) (/Users/demo/.pi/agent/extensions/herdr-agent-state.ts)
omp: outdated (v8 < v10) (/Users/demo/.omp/agent/extensions/herdr-omp-agent-state.ts)
claude: current (v10) (/Users/demo/.claude/hooks/herdr-agent-state.sh)
codex: current (v8) (/Users/demo/.codex/herdr-agent-state.sh)
opencode: outdated (v10 < v13) (/Users/demo/.config/opencode/plugins/herdr-agent-state.js)
kimi: not installed (/Users/demo/.kimi-code/hooks/herdr-agent-state.sh)
antigravity-cli: current (v3) (/Users/demo/.gemini/config/hooks/herdr-agent-state.sh)
letta (experimental): not installed (/Users/demo/.letta/hooks/herdr-agent-session.sh)
garbage line without colon
`

func healthServiceWithFixture() *IntegrationHealthService {
	return &IntegrationHealthService{runner: func(ctx context.Context, args []string) (string, error) {
		if strings.Join(args, " ") != "integration status" {
			return "", errors.New("unexpected command")
		}
		return integrationStatusFixture, nil
	}}
}

func TestIntegrationRegistryCoversEveryProviderExactlyOnce(t *testing.T) {
	seen := make(map[string]bool)
	for _, provider := range history.AllAgents {
		entry := IntegrationFor(provider)
		if entry.Strategy == "" {
			t.Fatalf("%s has no strategy", provider)
		}
		if entry.Strategy != StrategyDeferred && entry.Target == "" {
			t.Fatalf("%s has no install target", provider)
		}
		if seen[entry.Target] {
			t.Fatalf("target %q claimed twice", entry.Target)
		}
		seen[entry.Target] = true
	}
	if entry := IntegrationFor("mystery"); entry.Strategy != StrategyDeferred {
		t.Fatalf("unknown provider must fail closed deferred, got %q", entry.Strategy)
	}
}

func TestParseIntegrationStatus(t *testing.T) {
	states := parseIntegrationStatus(integrationStatusFixture)
	if got := states["pi"]; got.state != "current" || got.version != "v9" || got.path != "/Users/demo/.pi/agent/extensions/herdr-agent-state.ts" {
		t.Fatalf("pi = %+v", got)
	}
	if got := states["omp"]; got.state != "outdated" || got.version != "v8" || got.latest != "v10" {
		t.Fatalf("omp = %+v", got)
	}
	if got := states["kimi"]; got.state != "not installed" || got.version != "" {
		t.Fatalf("kimi = %+v", got)
	}
	if got := states["letta"]; got.state != "not installed" {
		t.Fatalf("letta (annotated label) = %+v", got)
	}
}

func TestAuditReconcilesStrategies(t *testing.T) {
	service := healthServiceWithFixture()
	rows, err := service.Audit(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	byProvider := make(map[history.AgentID]ProviderIntegrationHealth, len(rows))
	for _, row := range rows {
		byProvider[row.Provider] = row
	}

	claude := byProvider[history.AgentClaudeCode]
	if claude.Strategy != StrategyHerdrOfficial || claude.State != HealthCurrent || claude.Version != "v10" {
		t.Fatalf("claude = %+v", claude)
	}
	omp := byProvider[history.AgentOMP]
	if omp.State != HealthOutdated || omp.LatestVersion != "v10" || !omp.Actionable() || omp.ActionLabel() != "Update" {
		t.Fatalf("omp = %+v", omp)
	}
	kimi := byProvider[history.AgentKimi]
	if kimi.State != HealthNotInstalled || kimi.ActionLabel() != "Install" {
		t.Fatalf("kimi = %+v", kimi)
	}
	gemini := byProvider[history.AgentGemini]
	if gemini.Strategy != StrategyHerdrScreenWithManagedSessionBridge || gemini.State != HealthManagedByHerdr || gemini.Actionable() {
		t.Fatalf("gemini must be managed-by-herdr without actions: %+v", gemini)
	}
	qoder := byProvider[history.AgentQoder]
	if qoder.State != HealthDeferred || qoder.Actionable() {
		t.Fatalf("qoder must be deferred without actions: %+v", qoder)
	}
	// commandcode is a managed bridge whose target this Herdr does not list.
	commandCode := byProvider[history.AgentCommandCode]
	if commandCode.Strategy != StrategyManagedLifecycleBridge {
		t.Fatalf("commandcode strategy = %q", commandCode.Strategy)
	}
}

func TestInstallGatesByStrategy(t *testing.T) {
	var installed []string
	service := &IntegrationHealthService{runner: func(ctx context.Context, args []string) (string, error) {
		installed = append(installed, strings.Join(args, " "))
		return "", nil
	}}

	if err := service.Install(context.Background(), history.AgentClaudeCode); err != nil {
		t.Fatal(err)
	}
	if len(installed) != 1 || installed[0] != "integration install claude" {
		t.Fatalf("install commands = %v", installed)
	}
	if err := service.Install(context.Background(), history.AgentQoder); err == nil {
		t.Fatal("deferred provider must not be installable")
	}
	if err := service.Install(context.Background(), history.AgentGemini); err == nil {
		t.Fatal("screen-managed provider must not be installable by Shardlane")
	}
}

func TestAuditSurfacesCLIError(t *testing.T) {
	service := &IntegrationHealthService{runner: func(ctx context.Context, args []string) (string, error) {
		return "", errors.New("herdr not found")
	}}
	if _, err := service.Audit(context.Background()); err == nil {
		t.Fatal("expected the CLI error to surface")
	}
}
