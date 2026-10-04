package agent

import (
	"context"
	"fmt"
	"os/exec"
	"strconv"
	"strings"

	"github.com/wh-studio/herdr-client/next/internal/history"
)

// IntegrationHealthState is the presentation-safe health vocabulary (INT-03).
type IntegrationHealthState string

const (
	HealthChecking           IntegrationHealthState = "checking"
	HealthCurrent            IntegrationHealthState = "current"
	HealthOutdated           IntegrationHealthState = "outdated"
	HealthNotInstalled       IntegrationHealthState = "not-installed"
	HealthManagedByHerdr     IntegrationHealthState = "managed-by-herdr"
	HealthManagedBridgeReady IntegrationHealthState = "managed-bridge-ready"
	HealthDeferred           IntegrationHealthState = "deferred"
	HealthUnsupported        IntegrationHealthState = "unsupported"
	HealthError              IntegrationHealthState = "error"
)

// ProviderIntegrationHealth is one provider's reconciled integration row.
type ProviderIntegrationHealth struct {
	Provider      history.AgentID        `json:"provider"`
	Target        string                 `json:"target"`
	Strategy      IntegrationStrategy    `json:"strategy"`
	State         IntegrationHealthState `json:"state"`
	Version       string                 `json:"version,omitempty"`
	LatestVersion string                 `json:"latest_version,omitempty"`
	Path          string                 `json:"path,omitempty"`
	Detail        string                 `json:"detail,omitempty"`
}

// Actionable reports whether the row exposes an install/update action that
// is safe for its strategy (INT-03): only Herdr official integration (and a
// Herdr-managed managed-bridge target) is ever installed or updated, and
// never from render.
func (h ProviderIntegrationHealth) Actionable() bool {
	switch h.Strategy {
	case StrategyHerdrOfficial:
		return h.State == HealthNotInstalled || h.State == HealthOutdated
	case StrategyManagedLifecycleBridge:
		return h.State == HealthNotInstalled || h.State == HealthOutdated
	default:
		return false
	}
}

// ActionLabel names the safe action for the row, if any.
func (h ProviderIntegrationHealth) ActionLabel() string {
	if !h.Actionable() {
		return ""
	}
	if h.State == HealthNotInstalled {
		return "Install"
	}
	return "Update"
}

// IntegrationHealthService audits real provider integration state through
// the Herdr CLI (INT-02). It never polls; callers audit on route entry,
// manual refresh and after actions.
type IntegrationHealthService struct {
	// runner stands in for the Herdr CLI in tests; nil uses the real CLI.
	runner func(ctx context.Context, args []string) (string, error)
}

func NewIntegrationHealthService() *IntegrationHealthService {
	return &IntegrationHealthService{}
}

// NewIntegrationHealthServiceWithRunner injects the CLI transport. It is the
// seam for deterministic tests and for environments where the Herdr CLI is
// resolved elsewhere.
func NewIntegrationHealthServiceWithRunner(runner func(ctx context.Context, args []string) (string, error)) *IntegrationHealthService {
	return &IntegrationHealthService{runner: runner}
}

func (s *IntegrationHealthService) run(ctx context.Context, args []string) (string, error) {
	if s.runner != nil {
		return s.runner(ctx, args)
	}
	command := exec.CommandContext(ctx, "herdr", args...)
	output, err := command.Output()
	if err != nil {
		return "", fmt.Errorf("herdr %s: %w", strings.Join(args, " "), err)
	}
	return string(output), nil
}

// Audit reconciles every registered provider against one `herdr integration
// status` observation (INT-02/INT-07).
func (s *IntegrationHealthService) Audit(ctx context.Context) ([]ProviderIntegrationHealth, error) {
	output, err := s.run(ctx, []string{"integration", "status"})
	if err != nil {
		return nil, err
	}
	targetStates := parseIntegrationStatus(output)

	rows := make([]ProviderIntegrationHealth, 0, len(history.AllAgents))
	for _, provider := range history.AllAgents {
		rows = append(rows, s.reconcileProvider(provider, targetStates))
	}
	return rows, nil
}

// RefreshProvider audits and returns one provider's row (INT-07).
func (s *IntegrationHealthService) RefreshProvider(ctx context.Context, provider history.AgentID) (ProviderIntegrationHealth, error) {
	output, err := s.run(ctx, []string{"integration", "status"})
	if err != nil {
		return ProviderIntegrationHealth{}, err
	}
	return s.reconcileProvider(provider, parseIntegrationStatus(output)), nil
}

// Install runs the safe install/update action for one provider. It is only
// valid for strategies where Herdr official integration owns installation;
// the caller runs it off the presentation path and re-audits afterwards.
func (s *IntegrationHealthService) Install(ctx context.Context, provider history.AgentID) error {
	entry := IntegrationFor(provider)
	switch entry.Strategy {
	case StrategyHerdrOfficial, StrategyManagedLifecycleBridge:
	default:
		return fmt.Errorf("install is not available for %s (%s)", provider.DisplayName(), StrategyLabel(entry.Strategy))
	}
	if entry.Target == "" {
		return fmt.Errorf("no Herdr integration target for %s", provider.DisplayName())
	}
	_, err := s.run(ctx, []string{"integration", "install", entry.Target})
	return err
}

// reconcileProvider maps the strategy authority plus the observed CLI state
// onto the presentation-safe health vocabulary (INT-06): Shardlane never
// installs a parallel lifecycle hook beside Herdr official integration.
func (s *IntegrationHealthService) reconcileProvider(provider history.AgentID, observed map[string]targetState) ProviderIntegrationHealth {
	entry := IntegrationFor(provider)
	row := ProviderIntegrationHealth{
		Provider: provider,
		Target:   entry.Target,
		Strategy: entry.Strategy,
	}
	switch entry.Strategy {
	case StrategyDeferred:
		row.State = HealthDeferred
		// F111: the old line ("no actions claimed") spoke integration
		// contract, not user outcome.
		row.Detail = "Integration pending — nothing has been set up for this provider yet."
		return row
	case StrategyHerdrScreenOnly, StrategyHerdrScreenWithManagedSessionBridge:
		// Lifecycle authority is the Herdr screen manifest; Shardlane never
		// installs anything here.
		row.State = HealthManagedByHerdr
		row.Detail = "Managed by the Herdr screen manifest"
		return row
	}

	state, ok := observed[entry.Target]
	if !ok {
		row.State = HealthUnsupported
		// F110: the raw target slug ("commandcode", "dsh") leaked into the
		// sentence; the provider's display name is what the row is about.
		row.Detail = "This Herdr install does not manage " + provider.DisplayName() + " sessions."
		return row
	}
	row.Version, row.LatestVersion, row.Path = state.version, state.latest, state.path
	switch state.state {
	case "current":
		if entry.Strategy == StrategyManagedLifecycleBridge {
			row.State = HealthManagedBridgeReady
		} else {
			row.State = HealthCurrent
		}
	case "outdated":
		row.State = HealthOutdated
	case "not installed":
		row.State = HealthNotInstalled
	default:
		row.State = HealthError
		row.Detail = "unrecognized integration state " + state.state
	}
	return row
}

// targetState is one parsed `herdr integration status` line.
type targetState struct {
	state   string
	version string
	latest  string
	path    string
}

// parseIntegrationStatus parses the text contract of `herdr integration
// status`: one `label: state (versions) (path)` line per target, where the
// label may itself carry annotations such as "letta (experimental)".
func parseIntegrationStatus(output string) map[string]targetState {
	states := make(map[string]targetState)
	for _, line := range strings.Split(output, "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		label, rest, ok := strings.Cut(line, ":")
		if !ok {
			continue
		}
		fields := strings.Fields(label)
		if len(fields) == 0 {
			continue
		}
		target := fields[0]

		state := targetState{}
		rest = strings.TrimSpace(rest)
		if index := strings.Index(rest, "("); index >= 0 {
			state.state = strings.TrimSpace(rest[:index])
			rest = strings.TrimSpace(rest[index:])
		} else {
			state.state = rest
			rest = ""
		}
		switch state.state {
		case "current":
			state.version = firstVersionToken(rest)
		case "outdated":
			versions := versionTokens(rest)
			if len(versions) > 0 {
				state.version = versions[0]
			}
			if len(versions) > 1 {
				state.latest = versions[1]
			}
		}
		if index := strings.LastIndex(rest, "("); index >= 0 {
			state.path = strings.TrimSuffix(rest[index:], ")")
			state.path = strings.TrimPrefix(state.path, "(")
		}
		states[target] = state
	}
	return states
}

// versionTokens lists the vN version tokens in a status fragment, with the
// separator punctuation removed.
func versionTokens(text string) []string {
	cleaned := strings.NewReplacer("(", " ", ")", " ", "<", " ").Replace(text)
	var versions []string
	for _, field := range strings.Fields(cleaned) {
		if len(field) > 1 && field[0] == 'v' {
			if _, err := strconv.Atoi(field[1:]); err == nil {
				versions = append(versions, field)
			}
		}
	}
	return versions
}

func firstVersionToken(text string) string {
	versions := versionTokens(text)
	if len(versions) == 0 {
		return ""
	}
	return versions[0]
}
