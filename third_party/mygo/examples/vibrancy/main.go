// Vibrancy shows a window in the style of a native macOS app: a sidebar
// made of a translucent material, under a hidden title bar whose window
// controls sit over the page, the traffic lights inset over the sidebar on
// macOS. Pick a material in the sidebar to see it behind the window.
//
//	go run ./examples/vibrancy
package main

import (
	"context"
	_ "embed"
	"log"

	"github.com/egoist/mygo"
)

// Vibrancy is called from the page.
type Vibrancy struct{}

// Set puts a material behind the calling window's page.
func (Vibrancy) Set(ctx context.Context, material mygo.Vibrancy) {
	mygo.CallerWindow(ctx).SetVibrancy(material)
}

//go:embed index.html
var page string

func main() {
	mygo.Bind(Vibrancy{})
	mygo.App.WhenReady(func() {
		win := mygo.NewWindow(mygo.WindowOptions{
			Title:     "Vibrancy",
			Width:     880,
			Height:    640,
			MinWidth:  560,
			MinHeight: 400,
			// Hide the title bar and inset the traffic lights over the
			// sidebar (macOS). Elsewhere the window controls sit in the
			// page's title bar, 52 pixels tall.
			TitleBarStyle:  mygo.TitleBarHiddenInset,
			TitleBarHeight: 52,
			// The material shows wherever the page is transparent (macOS,
			// Windows 11). Linux has none and shows the background color.
			Vibrancy:        mygo.VibrancySidebar,
			BackgroundColor: "light-dark(#ececec, #2a2a2a)",
		})
		win.Page().LoadHTML(page, "")
	})
	if err := mygo.App.Run(); err != nil {
		log.Fatal(err)
	}
}
