# Windows with native UI

A window with `Content` takes the [window options](../windows.md#options)
of any window, such as its size, `StateKey`, `Frameless` and `Parent`, as
well as menus, dialogs and the other [desktop APIs](../native.md). It has no
page: `Page()` is nil, and it ignores `URL` and `WindowOptions.Page`. Its Go
code needs no bindings: the view calls it directly. `CapturePage` returns a
PNG of what it shows.

```go
mygo.NewWindow(mygo.WindowOptions{
	Title:    "Notes",
	Width:    900,
	Height:   600,
	StateKey: "main",
	Content:  ui.View(app.view),
})
```

## Title bars

With `TitleBarStyle: mygo.TitleBarHidden`, the view draws the title bar
under the window controls, as a page does with the `--mygo-titlebar-*` CSS
variables: `c.TitleBar()` returns the room the controls take, zero in full
screen, and `DragWindow` makes elements drag the window, which
double-clicking them zooms or minimizes as a title bar would:

```go
bar := c.TitleBar()
ui.Row(c).Height(max(bar.Height, 32)).Padding(0, bar.Right+12, 0, bar.Left+12).DragWindow().Children(func() {
	ui.Text(c, "Inbox").Bold()
})
```

In a `Frameless` window, `DragWindow` makes an element move the window, and
a double click on it maximizes the window.

## Vibrancy

On macOS, a window's `Vibrancy` shows wherever its native UI draws no
background. The root draws the theme's by default: make it transparent and
give backgrounds to the parts that need one, as a sidebar beside opaque
content does:

```go
c.Root().Background(ui.Transparent)
ui.Row(c).Fill().AlignItems(ui.Stretch).Children(func() {
	app.sidebar(c) // over the material
	ui.Column(c).Grow(1).Background(c.Theme().Background).Children(func() { app.content(c) })
})
```

## No webview

An app whose windows all show native UI needs no webview: on Linux it needs
GTK 3 alone, not WebKitGTK, and on Windows no WebView2 Runtime.
