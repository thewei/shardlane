package agent

/**
 * [INPUT]: user-reviewed, bounded commit prompt and explicit Pi binary preference
 * [OUTPUT]: one ephemeral, tool-free Pi CLI completion or a bounded error
 * [POS]: provider adapter only; no workspace Pane, Herdr session or Git mutation
 * [PROTOCOL]: update this header on modification and check CLAUDE.md
 */

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"
)

const (
	PiCommitTimeout          = 90 * time.Second
	MaxPiCommitResponseBytes = 8192
	MaxPiCommitInputBytes    = 16000
)

type PiCommitRequest struct {
	Executable string
	Root       string
	Prompt     string
}

type PiCommitResponse struct {
	Message string
}

func ResolvePiExecutable(preference string) (string, error) {
	configured := strings.TrimSpace(preference)
	if configured != "" {
		if !filepath.IsAbs(configured) {
			return "", errors.New("Pi executable must be an absolute path")
		}
		if st, err := os.Stat(configured); err != nil || st.IsDir() || st.Mode()&0o111 == 0 {
			return "", errors.New("configured Pi CLI is not executable; check Git Preferences")
		}
		return configured, nil
	}
	if pi, err := exec.LookPath("pi"); err == nil {
		return pi, nil
	}
	home, _ := os.UserHomeDir()
	candidates := []string{filepath.Join(home, ".local/bin/pi"), filepath.Join(home, ".bun/bin/pi"), filepath.Join(home, ".npm-global/bin/pi"), "/opt/homebrew/bin/pi", "/usr/local/bin/pi"}
	// Devspace and other monorepos often install Pi at the workspace
	// level rather than as a global CLI. Finder does not inherit shell PATH.
	for _, pattern := range []string{
		filepath.Join(home, "Workspaces/*/devspace/node_modules/.bin/pi"),
		filepath.Join(home, "Workspaces/*/*/node_modules/.bin/pi"),
	} {
		matches, _ := filepath.Glob(pattern)
		candidates = append(candidates, matches...)
	}
	for _, p := range candidates {
		if st, err := os.Stat(p); err == nil && !st.IsDir() && st.Mode()&0o111 != 0 {
			return p, nil
		}
	}
	return "", errors.New("Pi CLI not found. Install Pi or choose its executable in Settings → Git")
}

// GeneratePiCommitSuggestion deliberately disables all Pi tools, sessions,
// project context files, extensions, skills and local approval. The contents
// go through stdin, never argv, logs, local session files, or terminal panes.
// Authentication/model selection belongs to Pi's existing user configuration.
func GeneratePiCommitSuggestion(ctx context.Context, req PiCommitRequest) (PiCommitResponse, error) {
	if strings.TrimSpace(req.Prompt) == "" || len(req.Prompt) > MaxPiCommitInputBytes {
		return PiCommitResponse{}, errors.New("commit prompt is empty or exceeds the safety limit")
	}
	if req.Root == "" || !filepath.IsAbs(req.Root) {
		return PiCommitResponse{}, errors.New("Git repository location is unavailable")
	}
	binary, err := ResolvePiExecutable(req.Executable)
	if err != nil {
		return PiCommitResponse{}, err
	}
	ctx, cancel := context.WithTimeout(ctx, PiCommitTimeout)
	defer cancel()
	command := exec.CommandContext(ctx, binary, "--print", "--mode", "text",
		"--no-tools", "--no-extensions", "--no-skills", "--no-prompt-templates",
		"--no-context-files", "--no-approve", "--no-session",
		"Write the commit message using only the supplied stdin review and user rules.")
	command.Dir = req.Root
	command.Env = append(piRuntimeEnv(binary), "PWD="+req.Root)
	command.Stdin = strings.NewReader(req.Prompt)
	command.WaitDelay = 2 * time.Second
	// Drop stdout beyond the bound but mark the response as unusable.
	out := &boundedPiWriter{limit: MaxPiCommitResponseBytes}
	command.Stdout = out
	command.Stderr = &boundedPiWriter{limit: 1024}
	if err := command.Run(); err != nil {
		if ctx.Err() != nil {
			return PiCommitResponse{}, ctx.Err()
		}
		return PiCommitResponse{}, fmt.Errorf("Pi generation failed (%v). Check Pi login/model in Terminal", err)
	}
	if out.exceeded {
		return PiCommitResponse{}, errors.New("Pi response exceeded the commit-message limit")
	}
	msg := strings.TrimSpace(out.buffer.String())
	if msg == "" {
		return PiCommitResponse{}, errors.New("Pi returned an empty suggestion; check Pi model configuration")
	}
	return PiCommitResponse{Message: msg}, nil
}

// piRuntimeEnv adds only known executable directories to the child PATH.
// No login shell is sourced and no user startup script is executed. This
// lets Finder-launched native apps run the Pi CLI's node shim safely.
func piRuntimeEnv(binary string) []string {
	path := os.Getenv("PATH")
	extra := []string{filepath.Dir(binary), "/opt/homebrew/bin", "/usr/local/bin"}
	home, _ := os.UserHomeDir()
	for _, dir := range []string{
		filepath.Join(home, ".volta/bin"), filepath.Join(home, ".bun/bin"),
		filepath.Join(home, ".local/bin"),
	} {
		extra = append(extra, dir)
	}
	for _, pattern := range []string{
		filepath.Join(home, ".vite-plus/js_runtime/node/*/bin/node"),
		filepath.Join(home, ".nvm/versions/node/*/bin/node"),
	} {
		matches, _ := filepath.Glob(pattern)
		for _, node := range matches {
			if st, err := os.Stat(node); err == nil && !st.IsDir() && st.Mode()&0o111 != 0 {
				extra = append(extra, filepath.Dir(node))
			}
		}
	}
	for _, dir := range extra {
		if dir != "" {
			path += string(os.PathListSeparator) + dir
		}
	}
	return append(os.Environ(), "PATH="+path)
}

type boundedPiWriter struct {
	buffer   bytes.Buffer
	limit    int
	exceeded bool
}

func (w *boundedPiWriter) Write(p []byte) (int, error) {
	n := len(p)
	remain := w.limit - w.buffer.Len()
	if remain < 0 {
		remain = 0
	}
	if len(p) > remain {
		w.exceeded = true
		p = p[:remain]
	}
	_, _ = w.buffer.Write(p)
	return n, nil
}
