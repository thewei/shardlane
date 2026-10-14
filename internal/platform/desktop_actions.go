package platform

import (
	"context"
	"os/exec"
	"runtime"
	"time"
)

// DesktopActionTimeout bounds short-lived desktop shell helper operations.
const DesktopActionTimeout = 3 * time.Second

// OpenFile opens the file with the default editor or application. MyGo's
// Shell.OpenPath opens a file with the application of its type; macOS's
// `open -t` is the text editor, which the framework has no call for.
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
