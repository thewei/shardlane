package agent

import (
	"fmt"
	"path/filepath"
	"strings"

	"github.com/wh-studio/herdr-client/internal/history"
)

// LaunchMode and Permission mirror the Rust launch intent vocabulary.
type LaunchMode string

const (
	LaunchModeBuild LaunchMode = "build"
	LaunchModePlan  LaunchMode = "plan"
)

type Permission string

const (
	PermissionAskApproval Permission = "ask-approval"
	PermissionAutoApprove Permission = "auto-approve"
	PermissionFullAccess  Permission = "full-access"
)

// LaunchIntent is one canonical launch request (the Rust AgentLaunchIntent).
type LaunchIntent struct {
	OperationID       string          `json:"operation_id"`
	WorkspaceID       string          `json:"workspace_id,omitempty"`
	ProjectPath       string          `json:"project_path"`
	Branch            string          `json:"branch,omitempty"`
	Mode              LaunchMode      `json:"mode"`
	Permission        Permission      `json:"permission"`
	Agent             history.AgentID `json:"agent"`
	Prompt            string          `json:"prompt"`
	Attachments       []string        `json:"attachments,omitempty"`
	ExtraArgs         []string        `json:"extra_args,omitempty"`
	SkipInitialPrompt bool            `json:"skip_initial_prompt,omitempty"`
}

// AgentStartupArgs derives the provider launch argv for the mode/permission
// pair — a port of the audited Rust agent_startup_args.
func AgentStartupArgs(agent history.AgentID, mode LaunchMode, permission Permission) []string {
	switch agent {
	case history.AgentClaudeCode:
		baseMode := ""
		switch {
		case mode == LaunchModePlan:
			baseMode = "plan"
		case permission == PermissionAskApproval:
			baseMode = "default"
		case permission == PermissionAutoApprove:
			baseMode = "acceptEdits"
		case permission == PermissionFullAccess:
			baseMode = "bypassPermissions"
		}
		return []string{"--permission-mode", baseMode}
	case history.AgentCodex:
		switch permission {
		case PermissionAutoApprove:
			// R2-13: an approval-policy + sandbox-policy PAIR accepted by
			// every probed 0.142+ CLI, not a version-specific alias.
			return []string{"--sandbox", "workspace-write", "--ask-for-approval", "never"}
		case PermissionFullAccess:
			return []string{"--dangerously-bypass-approvals-and-sandbox"}
		default:
			return nil
		}
	case history.AgentGemini:
		approvalMode := "default"
		switch {
		case mode == LaunchModePlan:
			approvalMode = "plan"
		case permission == PermissionAutoApprove || permission == PermissionFullAccess:
			approvalMode = "auto-edit"
		}
		return []string{"--approval-mode=" + approvalMode}
	default:
		return nil
	}
}

// PostLaunchPlanKeys returns the verified keys that enter Plan mode after
// readiness for providers without a CLI plan flag (named-key operation on an
// exact Agent target, never raw terminal text).
func PostLaunchPlanKeys(agent history.AgentID) []string {
	if planModeSupport(agent) != planPostLaunchKeys {
		return nil
	}
	if agent == history.AgentCodex {
		return []string{"shift+tab"}
	}
	return nil
}

// BuildInitialPrompt builds the semantic initial prompt from the user text:
// plan intent for prompt-prefix providers is expressed here; branch and
// attachment context are appended exactly as the previous builder did.
func BuildInitialPrompt(intent LaunchIntent, cwd string) string {
	prompt := intent.Prompt
	if intent.Mode == LaunchModePlan && planModeSupport(intent.Agent) == planPromptPrefix {
		prompt = "Work in Plan mode first. Analyze the request, inspect the project, and produce a concrete plan before making changes.\n\n" + intent.Prompt
	}
	if branch := strings.TrimSpace(intent.Branch); branch != "" {
		prompt += "\n\nTarget branch: " + branch
	}
	if len(intent.Attachments) > 0 {
		prompt += "\n\nAttached file references:"
		for _, path := range intent.Attachments {
			if display, err := filepath.Rel(cwd, path); err == nil && !strings.HasPrefix(display, "..") {
				path = display
			}
			prompt += "\n- " + path
		}
	}
	return prompt
}

// LaunchDisplayLabel resolves the one rename applied after runtime creation:
// a high-confidence task title when available, else the provider name.
func LaunchDisplayLabel(intent LaunchIntent) (label string, taskTitle string, hasTaskTitle bool) {
	if title := SuggestTaskTitle(intent.Prompt); title != "" {
		return title, title, true
	}
	return intent.Agent.DisplayName(), "", false
}

// LaunchFingerprint is the canonical request fingerprint for launch
// idempotency. The operation id is deliberately excluded: reusing an id for
// another shape must be rejected as a conflict, never treated as a replay.
func LaunchFingerprint(intent LaunchIntent) string {
	return fmt.Sprintf("project=%s|workspace=%s|branch=%s|mode=%s|permission=%s|agent=%s|prompt=%s|attachments=%v|args=%v|skip_initial_prompt=%t",
		intent.ProjectPath, intent.WorkspaceID, intent.Branch, intent.Mode,
		intent.Permission, string(intent.Agent), intent.Prompt,
		intent.Attachments, intent.ExtraArgs, intent.SkipInitialPrompt)
}

// HerdrAgentKind maps a provider onto the Herdr agent kind string.
func HerdrAgentKind(agent history.AgentID) string {
	return string(agent)
}

// suggestTaskTitle extracts a high-confidence task title from the prompt:
// the first non-empty line, trimmed and bounded, when it reads like a task.
func SuggestTaskTitle(prompt string) string {
	for _, line := range strings.Split(prompt, "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		runes := []rune(line)
		if len(runes) > 60 {
			return string(runes[:60]) + "…"
		}
		return line
	}
	return ""
}
