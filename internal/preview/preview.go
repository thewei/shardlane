// Package preview owns the P8 Local Preview capability (0.9 P8 / WIX-120..127).
// It creates a dedicated project-scoped MyGo WebView Window strictly bound to
// approved loopback targets (localhost, 127.0.0.1, [::1]). External navigations
// are intercepted and diverted to the system browser.
package preview

import (
	"errors"
	"fmt"
	"log/slog"
	"net"
	"net/url"
	"strings"
	"sync"

	"github.com/egoist/mygo"
	"github.com/wh-studio/herdr-client/internal/platform"
)

// ErrNotLoopback indicates an attempt to preview a non-loopback address.
var ErrNotLoopback = errors.New("target is not an approved loopback address (must be localhost, 127.0.0.1, or [::1])")

// IsLoopbackURL checks whether the given raw URL targets an approved local loopback host.
func IsLoopbackURL(rawURL string) bool {
	u, err := url.Parse(strings.TrimSpace(rawURL))
	if err != nil {
		return false
	}
	// Scheme must be http or https
	scheme := strings.ToLower(u.Scheme)
	if scheme != "http" && scheme != "https" {
		return false
	}

	host := u.Host
	if h, _, err := net.SplitHostPort(u.Host); err == nil {
		host = h
	}
	host = strings.Trim(strings.ToLower(host), "[]")

	if host == "localhost" || host == "127.0.0.1" || host == "::1" {
		return true
	}

	ip := net.ParseIP(host)
	if ip != nil && ip.IsLoopback() {
		return true
	}

	return false
}

// NormalizePreviewTarget validates and returns a clean loopback preview URL.
func NormalizePreviewTarget(raw string) (string, error) {
	raw = strings.TrimSpace(raw)
	if !strings.HasPrefix(raw, "http://") && !strings.HasPrefix(raw, "https://") {
		raw = "http://" + raw
	}
	if !IsLoopbackURL(raw) {
		return "", fmt.Errorf("%w: %s", ErrNotLoopback, raw)
	}
	return raw, nil
}

// WindowController manages opening, reusing, and tracking preview windows.
type WindowController struct {
	mu  sync.Mutex
	win *mygo.Window
	// openExternal diverts a non-loopback navigation to the system
	// browser. It reports what came of the attempt: the default handler
	// surfaces failures in the operation log; hosts may inject one that
	// also reports in their own UI (MyGo 0.2.14 OpenURLThen semantics —
	// page navigation events carry no ui.Context, so the outcome flows
	// through this seam).
	openExternal func(url string) error
}

// WithOpenExternal replaces the default external-browser handler.
func WithOpenExternal(fn func(url string) error) func(*WindowController) {
	return func(c *WindowController) { c.openExternal = fn }
}

// NewWindowController creates a controller with the default external browser handler.
func NewWindowController(opts ...func(*WindowController)) *WindowController {
	c := &WindowController{
		openExternal: openExternalBrowser,
	}
	for _, opt := range opts {
		opt(c)
	}
	return c
}

// OpenPreviewWindow creates or focuses a dedicated MyGo WebView window for an approved loopback URL.
func (c *WindowController) OpenPreviewWindow(targetURL string) (*mygo.Window, error) {
	cleanURL, err := NormalizePreviewTarget(targetURL)
	if err != nil {
		return nil, err
	}

	c.mu.Lock()
	defer c.mu.Unlock()

	if c.win != nil && !c.win.IsDestroyed() {
		c.win.Show()
		c.win.Focus()
		return c.win, nil
	}

	win := mygo.NewWindow(mygo.WindowOptions{
		Title:  fmt.Sprintf("Shardlane Preview — %s", cleanURL),
		URL:    cleanURL,
		Width:  960,
		Height: 680,
	})

	// Intercept external navigations (0.9 P8 invariant: preview is not a general browser)
	win.Page().OnWillNavigate(func(e *mygo.NavigateEvent) {
		if !IsLoopbackURL(e.URL) {
			e.PreventDefault()
			if c.openExternal != nil {
				if err := c.openExternal(e.URL); err != nil {
					// Query strings can carry tokens; log the
					// destination shape only, never the full URL.
					slog.Warn("open external URL failed", "destination", externalDestination(e.URL), "error", err)
				}
			}
		}
	})

	c.win = win
	win.OnClosed(func() {
		c.mu.Lock()
		c.win = nil
		c.mu.Unlock()
	})

	win.Show()
	return win, nil
}

func (c *WindowController) CurrentWindow() *mygo.Window {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.win
}

func openExternalBrowser(rawURL string) error {
	return platform.OpenURL(rawURL)
}

// externalDestination reduces a URL to scheme://host for bounded logging.
func externalDestination(rawURL string) string {
	u, err := url.Parse(rawURL)
	if err != nil {
		return "(unparsable)"
	}
	return u.Scheme + "://" + u.Host
}
