package platform

import (
	"context"
	"errors"
	"os/exec"
	"runtime"
	"strings"
	"time"
)

// DesktopActionTimeout bounds short-lived desktop shell helper operations.
const DesktopActionTimeout = 3 * time.Second

// WriteClipboard writes text to the platform clipboard using standard desktop tools.
func WriteClipboard(text string) error {
	ctx, cancel := context.WithTimeout(context.Background(), DesktopActionTimeout)
	defer cancel()

	var cmd *exec.Cmd
	switch runtime.GOOS {
	case "darwin":
		cmd = exec.CommandContext(ctx, "pbcopy")
	case "windows":
		cmd = exec.CommandContext(ctx, "clip")
	default:
		if _, err := exec.LookPath("wl-copy"); err == nil {
			cmd = exec.CommandContext(ctx, "wl-copy")
		} else if _, err := exec.LookPath("xclip"); err == nil {
			cmd = exec.CommandContext(ctx, "xclip", "-selection", "clipboard")
		} else {
			cmd = exec.CommandContext(ctx, "xsel", "--clipboard", "--input")
		}
	}
	cmd.Stdin = strings.NewReader(text)
	return cmd.Run()
}

// RevealFile reveals the path in Finder/Explorer/file manager.
func RevealFile(path string) error {
	ctx, cancel := context.WithTimeout(context.Background(), DesktopActionTimeout)
	defer cancel()

	var cmd *exec.Cmd
	switch runtime.GOOS {
	case "darwin":
		cmd = exec.CommandContext(ctx, "open", "-R", path)
	case "windows":
		cmd = exec.CommandContext(ctx, "explorer", "/select,", path)
	default:
		// xdg-open on directory or file
		cmd = exec.CommandContext(ctx, "xdg-open", path)
	}
	return cmd.Run()
}

// OpenFile opens the file with the default editor or application.
func OpenFile(path string) error {
	ctx, cancel := context.WithTimeout(context.Background(), DesktopActionTimeout)
	defer cancel()

	var cmd *exec.Cmd
	switch runtime.GOOS {
	case "darwin":
		cmd = exec.CommandContext(ctx, "open", "-t", path)
	case "windows":
		cmd = exec.CommandContext(ctx, "cmd", "/c", "start", "", path)
	default:
		cmd = exec.CommandContext(ctx, "xdg-open", path)
	}
	return cmd.Run()
}

// OpenURL opens the target URL in the default web browser.
func OpenURL(rawURL string) error {
	if rawURL == "" {
		return errors.New("empty URL")
	}
	ctx, cancel := context.WithTimeout(context.Background(), DesktopActionTimeout)
	defer cancel()

	var cmd *exec.Cmd
	switch runtime.GOOS {
	case "darwin":
		cmd = exec.CommandContext(ctx, "open", rawURL)
	case "windows":
		cmd = exec.CommandContext(ctx, "cmd", "/c", "start", "", rawURL)
	default:
		cmd = exec.CommandContext(ctx, "xdg-open", rawURL)
	}
	return cmd.Run()
}
