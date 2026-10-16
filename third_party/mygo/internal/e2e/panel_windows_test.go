//go:build windows

package e2e

import (
	"sync/atomic"
	"testing"

	"github.com/egoist/mygo"
	win "github.com/egoist/mygo/internal/windows"
)

// TestPanelAltF4 asks to close a panel on Alt+F4, which has no window menu
// (TestPanelStyles), with a material behind its page.
func TestPanelAltF4(t *testing.T) {
	panel := newWindow(t, mygo.WindowOptions{Panel: true, Frameless: true, Transparent: true, Vibrancy: mygo.VibrancyPopover, Width: 400, Height: 200})
	panel.Page().LoadHTML(`<body style="background:transparent">panel</body>`, "")
	eventually(t, "the panel focused", panel.IsFocused)
	var closes atomic.Int32
	panel.OnClose(func(e *mygo.CloseEvent) { closes.Add(1); e.PreventDefault() })
	mygo.RunOnMain(func() { win.TestPressKeys(0x12, 0x73) }) // VK_MENU, VK_F4
	eventually(t, "Alt+F4 asks to close", func() bool { return closes.Load() > 0 })
}
