# Terminal

The terminal plugin is a terminal for apps of [native UI](../ui/README.md): a view
that runs the user's shell, or any program, in a pseudo-terminal, with the
terminal emulator of [Ghostty](https://ghostty.org), libghostty-vt. It is
all Go, with no npm package: the emulator is a native library that the
app loads at run time, with no cgo, and that `mygo build` and `mygo dev`
put into the app.

## Set up

Make a terminal, which starts its program, and show it in a window:

```go
import (
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/plugins/terminal"
	"github.com/egoist/mygo/ui"
)

func main() {
	mygo.App.WhenReady(func() {
		term, err := terminal.New(terminal.Options{})
		if err != nil {
			log.Fatal(err)
		}
		win := mygo.NewWindow(mygo.WindowOptions{
			Title: "Terminal",
			Content: ui.View(func(c *ui.Context) {
				terminal.View(c, term).Fill().AutoFocus()
			}),
		})
		win.OnClosed(func() { term.Close() })
	})
	if err := mygo.App.Run(); err != nil {
		log.Fatal(err)
	}
}
```

The [terminal example](../../examples/terminal) shows this app. Its window
uses the shell's title and closes when the shell exits.

`terminal.View` is an element like any other: size it with `Fill` or
`Grow`, and put it next to other elements, as in a pane of an editor. Its
size sets the terminal's, in cells of its font, and a click gives it the
keyboard focus. A terminal shows in one view at a time. Its program starts
once a view first shows it, so that it draws its first screen at the
view's size, or half a second after `New` at 80×24 when no view does.

The terminal does what terminal apps do:

- colors (16, 256 and 24-bit), bold, italic, faint, inverse, strikethrough,
  overlines and underlines (single, double, curly, dotted, dashed, in their
  own colors);
- Unicode: wide characters, emoji and grapheme clusters take the cells of
  their width, as in Ghostty, and the system's fonts stand in for what the
  terminal's font lacks;
- box drawing, blocks and Powerline's separators drawn rather than taken
  from the font, so that the lines of full-screen programs join;
- a scrollback that reflows when the terminal resizes, and the alternate
  screen of full-screen programs;
- the keyboard as programs ask: legacy sequences, xterm's modifyOtherKeys
  or the Kitty keyboard protocol, and input methods, which compose at the
  cursor;
- mouse reports, focus reports, bracketed paste, synchronized output,
  hyperlinks (OSC 8), the title (OSC 0 and 2), the working directory
  (OSC 7), copying to and reading the clipboard (OSC 52, OSC 5522) as the
  app allows (see `ClipboardRead`), and paste events of the Kitty
  clipboard protocol (OSC 5522, mode 5522), which let programs take
  pasted images.

## Using it

- **Selecting** with the pointer works as in Ghostty: a drag selects
  cells, a double click words and a triple click lines, and dragging past
  the top or the bottom scrolls. Option (Alt elsewhere) drags a rectangle.
  While a program takes the mouse, as editors do, Shift selects.
- **Copy and paste** are Command+C and Command+V on macOS, which the Edit
  menu's roles also send, and Control+Shift+C and Control+Shift+V
  elsewhere, with Control+Insert and Shift+Insert, and on Windows
  Control+C and Control+V as in Windows Terminal (see [Keys](#keys)).
  Pastes are bracketed when the program asks, so that pasted lines do not
  run as commands. The context menu has Copy, Paste, Select All and Clear
  Scrollback. Options add copying on select and other secondary clicks
  (see [Options](#options)).
- **Pasting images**: a program that reads the clipboard itself, as it
  asks with mode 5522 (as coding agents that accept images do), is told
  what a paste holds, every format of the clipboard as MIME types (the
  text, an image as PNG, files as `text/uri-list`, …), and reads what it
  wants, even over SSH. Only the terminal that pasted answers its read,
  with what the clipboard held at the paste; other terminals showing the
  same program, as other windows on a session, stay silent. Other
  programs get the text alone. An image alone pastes an empty bracketed
  paste to a program that asks for bracketed pastes, as VS Code's
  terminal does, which agents such as Claude Code take as their cue to
  read the image from the clipboard themselves; to any other, nothing.
- **Reading the clipboard**: a program asks with OSC 52 (`?`) or OSC 5522
  and is refused by default, with an empty clipboard for OSC 52 and an
  EPERM status for OSC 5522; `ClipboardRead` allows it or asks the app
  first (see [Options](#options)). The clipboard is read, and the answer
  written, on the main thread as the view builds a frame, so what the
  program reads is what the clipboard held then, not when it asked.
- **Scrolling**: the wheel and the touchpad scroll the scrollback, as do
  Shift+Page Up and Shift+Page Down. Full-screen programs get the wheel as
  arrow keys, or as mouse reports when they take the mouse. Typing scrolls
  back to the bottom.
- **Zooming**: the wheel with Control held (Command on macOS), and
  Control (Command) with =, - and 0 make the text larger, smaller, and its
  size again, a DIP a step, as in Ghostty; `OnZoom` takes zooms instead.
- **Links**: Command+click (Control+click elsewhere) opens a hyperlink a
  program printed (OSC 8), or a URL printed as text, even while a program
  takes the mouse; the pointer is a hand over them while Command is held.
- Shortcuts of the app come first: Command shortcuts on macOS are never
  sent to the program, and keys that the window or an element around the
  terminal handles with `Shortcut`, or that a menu of the app has as its
  key equivalent on macOS, go there.

## Options

```go
term, err := terminal.New(terminal.Options{
	Command:  []string{"htop"},          // the user's login shell when empty
	Dir:      "/tmp",                     // the home directory when empty
	Env:      []string{"EDITOR=vim"},     // added to the app's environment
	Font:     terminal.Font{Family: "JetBrains Mono", Size: 14},
	Theme:    terminal.DarkTheme(),        // follows the window when nil
	Cursor:   terminal.CursorBar,          // a block when zero
	OnTitle:  func(title string) { win.SetTitle(title) },
	OnExit:   func(code int) { win.Close() },
})
```

- `Command` runs a program instead of the shell. The shell runs as a login
  shell (`-zsh`), as terminal apps start it, so that it reads the user's
  profile: `$SHELL`, else `/bin/zsh` on macOS and `/bin/sh` elsewhere, and
  PowerShell on Windows.
- `Env` adds to the environment, which has `TERM=xterm-256color`,
  `COLORTERM=truecolor`, and `LANG=en_US.UTF-8` when no locale is set, as
  for apps started from the Finder.
- `Font` sets the font: the system's monospaced font at 13 DIPs by
  default (see [Fonts](#fonts)).
- `Theme` sets the colors: the foreground, the background, the cursor's
  and the text's under it, the selection's and the selected text's, and
  the palette of 16 colors; `DarkTheme`, when set too, sets those of dark
  windows (see [Themes](#themes)). `terminal.DarkTheme()` and
  `terminal.LightTheme()` are the window's background and text with Visual
  Studio Code's palette, and without a theme the terminal takes the one of
  the window's appearance as it changes. Programs may change the colors
  (OSC 4, 10, 11, 12).
- `Cursor` and `NoBlink` set the cursor, which programs may change.
- `Scrollback` is about how many bytes of output to keep above the
  screen: 10 MB by default, none when negative.
- `Transparent` leaves the background undrawn, as Ghostty's
  `background-opacity`, so that what is under the view shows through: the
  window's material, or an element's translucent background. Cells with a
  background color of their own still draw it.
- `OptionAsAlt` makes Option on macOS the Alt key of programs (Meta),
  instead of the key typing accented letters.
- `CopyOnSelect: terminal.CopyOnSelectClipboard` copies the selection to
  the clipboard as the pointer ends it, and as Select All makes it, as
  Ghostty's `copy-on-select = clipboard`. Ghostty's `primary`, Linux's
  selection clipboard, is not there, as native UI has none.
  `SetCopyOnSelect` changes it in a running terminal.
- `Keybinds` bind keys to actions, over the defaults (see [Keys](#keys)).
- `RightClick` is what a secondary click does, as Ghostty's
  `right-click-action`: the context menu by default,
  `RightClickPaste`, `RightClickCopy`, `RightClickCopyOrPaste` (copy the
  selection, or paste when there is none, as Windows Terminal) or
  `RightClickIgnore`. Copying lets go of the selection. A program that
  takes the mouse still gets the click, unless Shift is held.
- `NoClipboardWrite` keeps programs from writing to the clipboard (OSC 52
  and OSC 5522), as Ghostty's `clipboard-write = deny`; reads are governed
  by `ClipboardRead`, and what a paste offers a program is still answered.
- `ClipboardRead` is what programs get when they read the clipboard (OSC
  52's "?", and the Kitty clipboard protocol's OSC 5522):
  `ClipboardDeny` by default, as Ghostty's `clipboard-read = deny`
  (Ghostty asks by default, but a terminal that asks needs a view), or
  `ClipboardAllow` to answer with what the clipboard holds, or
  `ClipboardAsk` to ask the app first. The clipboard is read and the
  answer written on the main thread, as the view builds its next frame,
  which is also the frame the terminal's own prompt shows in: a read of a
  terminal no view shows waits, and one of a closed terminal is never
  answered.
- `OnClipboardRequest` asks the app before a program reads the clipboard,
  when `ClipboardRead` is `ClipboardAsk`. It runs on the main thread with
  a `ClipboardRequest`: the MIME types the program asked for (`Mimes`),
  whether it asked for the types available (`List`, the Kitty protocol's
  targets listing, which is answered without asking), the selection it
  wants (`Primary`) and what the clipboard held as text (`Text`), for a
  prompt about it. `Allow` and `Deny` answer the request, in the hook or
  later and from any goroutine, and the first of them wins; a request the
  hook leaves unanswered waits. Without the hook the terminal draws its
  own prompt in the view, one read at a time, with a one-line preview of
  what the program would read and Allow and Deny buttons (Enter allows,
  Escape denies):

  ```go
  OnClipboardRequest: func(req terminal.ClipboardRequest) {
  	if req.Text == "" && !req.List {
  		req.Deny() // nothing to leak
  		return
  	}
  	askTheUser(func(ok bool) {
  		if ok {
  			req.Allow()
  		} else {
  			req.Deny()
  		}
  	})
  },
  ```
- `OnTitle`, `OnExit`, `OnBell` and `OnNotify` run on a goroutine of the
  terminal when the program sets the title, ends, rings the bell or asks
  for a desktop notification. `Done` is closed once the program exited,
  and `ExitCode` returns its code (-1 for a program that could not
  start); the terminal then shows "[Process exited]".
- `OpenLink` takes the links Command+click opens, on the main thread,
  and with it file paths printed as text too (`/abs/path`, `~/notes.md`,
  `./a.go`, `src/app.go:12:3`, `main.go:250`), as printed: a relative
  path is the program's, as of `Dir`. It reports whether it opened the
  link; a hyperlink or a URL it leaves opens with the system, as every one
  does without `OpenLink`. An app opens a remote shell's paths in a
  preview of its own this way, rather than the system opening them
  locally.
- `OnPaste` sees a paste of the clipboard, by the keys, a Paste menu item
  or a secondary click, on the main thread, with every format the
  clipboard holds as a `transfer.Data`, and takes it by returning true:
  nothing is pasted. An app driving a remote shell uploads the
  clipboard's files (`d.Files()`) or image (`d.Read(transfer.PNG)`) this
  way, and pastes their remote paths with `Paste` instead:

  ```go
  OnPaste: func(d transfer.Data) bool {
  	files, _ := d.Files()
  	if len(files) == 0 {
  		return false // text pastes as usual
  	}
  	go func() { term.Paste(strings.Join(upload(files), " ")) }()
  	return true
  },
  ```

- `OnPasteStatus` tells how a paste to a program that reads the clipboard
  itself goes, on a goroutine of the terminal, with a `PasteProgress`:
  `PasteOffered`, then `PasteSending` with `Sent` of `Total` bytes as the
  program reads (bytes written to it, encoded as the protocol sends them),
  then `PasteSent`; or `PasteNotTaken` when the program has not read it 5
  seconds after the offer was written (it may still, until another paste
  comes), or `PasteNoText` when a paste with no text, as only an image,
  goes to a program that reads neither the clipboard nor bracketed
  pastes, which pastes nothing. An app shows a large image's progress over a slow link this
  way, and says why nothing pasted.
- `OnKey` sees a key press on the main thread before the keybinds and
  before the terminal sends it, and takes it by returning true: neither the key nor the text it
  types reaches the program. Unlike a `Shortcut` around the view, which
  runs with the next frame, it keeps its place among the keys typed after
  it, as does what it sends with `Send`. Keys that only type text may come
  as their text alone, as on Linux, and `OnKey` does not see those.
- `OnScroll` sees the wheel scroll the view by whole rows (negative up)
  on the main thread before the terminal scrolls its scrollback, and
  takes it by returning true: the scrollback stays. It is not asked while
  a program gets the wheel: as mouse reports when it takes the mouse
  (unless Shift is held), or as arrow keys on a full-screen program's
  screen (alternate scroll, which `\x1b[?1007l` turns off). A `Conn`
  whose history lives at the other end, as a remote multiplexer streaming
  only its screen, sends the wheel there this way.
- `OnZoom` sees a zoom on the main thread: the wheel with Control held
  (Command on macOS), which zooms before `OnScroll` and a program that
  takes the mouse see it, or a key bound to a font size action. Its
  `Zoom` has how many `Steps`, rolling up positive (a wheel's event is a
  notch; a trackpad's moves add up to them), or `Reset`. It takes the zoom
  by returning true, as an app that sizes all its text at once does;
  without it, or for a zoom it leaves, the terminal changes its font's
  size itself, a DIP a step, down to 1, and back to `Font.Size` on a reset.
  macOS's pinch, which native UI does not report, does not zoom.

## Keys

The terminal binds keys to actions, as Ghostty's `keybind` option does,
with Ghostty's defaults for the platform, which `terminal.DefaultKeybinds`
returns:

| Action | macOS | Linux and Windows | Windows also |
| --- | --- | --- | --- |
| `ActionCopy` | Cmd+C | Ctrl+Shift+C, Ctrl+Insert | Ctrl+C (performable) |
| `ActionPaste` | Cmd+V | Ctrl+Shift+V, Shift+Insert | Ctrl+V (performable) |
| `ActionSelectAll` | Cmd+A | Ctrl+Shift+A | |
| `ActionClearScreen` | Cmd+K | | Ctrl+Shift+K |
| `ActionIncreaseFontSize` | Cmd+=, Cmd++ | Ctrl+=, Ctrl++ | |
| `ActionDecreaseFontSize` | Cmd+- | Ctrl+- | |
| `ActionResetFontSize` | Cmd+0 | Ctrl+0 | |
| `ActionScrollPageUp`, `…Down` | Shift+ or Cmd+Page Up/Down | Shift+Page Up/Down | Ctrl+Shift+Page Up/Down |
| `ActionScrollToTop`, `…Bottom` | Cmd+Home/End | Shift+Home/End (Linux) | Ctrl+Shift+Home/End |

The keys that clear the screen and scroll are performable too, so that a
full-screen program, as an editor, gets them. `Options.Keybinds` bind more,
over the defaults: a binding replaces the defaults of its key and
modifiers, and one to `ActionNone` unbinds them, so that the program gets
the key; `NoDefaultKeybinds` starts from none, as Ghostty's
`keybind = clear`:

```go
term, err := terminal.New(terminal.Options{Keybinds: []terminal.Keybind{
	// Windows Terminal's Control+C and Control+V on Linux too.
	{Mods: ui.Ctrl, Key: ui.KeyC, Action: terminal.ActionCopy, Performable: true},
	{Mods: ui.Ctrl, Key: ui.KeyV, Action: terminal.ActionPaste, Performable: true},
	// Control+Shift+A reaches the program.
	{Mods: ui.Ctrl | ui.Shift, Key: ui.KeyA, Action: terminal.ActionNone},
}})
```

A `Performable` binding, as Ghostty's `performable:` prefix, takes the key
only when its action does something: a copy only with a selection, a
paste only when the clipboard holds something, clearing the screen and
scrolling only off a full-screen program's screen; otherwise the key goes
on to the program, as if unbound: Control+C interrupts when nothing is
selected. A performable copy also lets go of the selection, so that the
key's next press interrupts. `ActionClearScreen` clears the scrollback and
the screen above the cursor's line, which moves to the top with the
shell's prompt, as Kitty's `clear_terminal to_cursor`; `ActionIgnore`
takes the key and does nothing.

A key goes first to the app's shortcuts, then to `OnKey`, then to the
keybinds, then to the program. The context menu shows the keys bound to
its items.

## Fonts

`terminal.Font` holds what Ghostty's font options set:

```go
term, err := terminal.New(terminal.Options{Font: terminal.Font{
	Family:     "JetBrains Mono, Menlo, monospace", // tried in order
	Size:       14,                                  // DIPs, 13 when zero
	Weight:     500,                                 // 400 when zero
	Features:   []string{"-calt", "+ss01"},          // as Ghostty's font-feature
	LineHeight: 1.2,                                 // as adjust-cell-height = 20%
	Thicken:    true,                                // as font-thicken, on macOS
}})
```

- `Family` is a list of families, as `ui.Element.Font` takes it: the
  system's monospaced font when empty. Characters the fonts lack come
  from the system's fallback fonts. A font the app carries, rather than
  one the user installed, is added with `ui.RegisterFont`, its styles
  under one family so that bold and italic text take them:

  ```go
  //go:embed fonts
  var fonts embed.FS

  for _, name := range []string{"Regular", "Bold", "Italic", "BoldItalic"} {
  	data, _ := fonts.ReadFile("fonts/JetBrainsMono-" + name + ".ttf")
  	ui.RegisterFont(data, "JetBrains Mono")
  }
  ```

- `Weight` sets how heavy text is, from 100 to 900, of the weights the
  family has; bold text is 300 heavier, at least 700.
- `Features` turn OpenType features on and off: `"ss01"` or `"+ss01"` on,
  `"calt=0"` or `"-calt"` off, `"cv05=2"` an alternate. Coding fonts make
  most of their ligatures with `calt`, the others with `liga`; a ligature
  spans the cells of its characters.
- `LineHeight` makes rows taller or shorter than the font's line height,
  with the text centered in them; box drawing still joins.
- `Thicken` draws text with a thicker stroke, with Core Text's font
  smoothing at its strongest whatever the colors and the system's
  setting, as Ghostty does. Without it, text is as AppKit draws it:
  smoothed more the lighter it is, while the user leaves smoothing on. It
  changes nothing on Linux and Windows, as in Ghostty.

`SetFont` changes the font of a running terminal; the grid then takes the
new size of its cells.

## Themes

The plugin carries the hundreds of themes Ghostty ships (from
[iTerm2-Color-Schemes](https://github.com/mbadolato/iTerm2-Color-Schemes)),
which `terminal.GhosttyTheme` returns by name, as Ghostty's `theme` option
takes it:

```go
mocha, err := terminal.GhosttyTheme("Catppuccin Mocha")
latte, err := terminal.GhosttyTheme("Catppuccin Latte")
term, err := terminal.New(terminal.Options{
	Theme:     latte, // in light windows
	DarkTheme: mocha, // in dark windows, as theme = light:…,dark:… in Ghostty
})
```

As in Ghostty, a theme file of the user's in Ghostty's themes directory
(`$XDG_CONFIG_HOME/ghostty/themes`, `~/.config/ghostty/themes` by default)
comes first, and an absolute path names a theme file.
`terminal.GhosttyThemes()` lists the names, the user's themes with
Ghostty's, for a picker, and `terminal.ParseGhosttyTheme` reads a theme in
Ghostty's format from bytes:

```
palette = 0=#45475a
palette = 1=#f38ba8
background = #1e1e2e
foreground = #cdd6f4
cursor-color = #f5e0dc
cursor-text = #1e1e2e
selection-background = #f5e0dc
selection-foreground = #1e1e2e
```

`SetTheme(theme, dark)` changes the themes of a running terminal.

## Methods

The terminal's methods are safe from any goroutine:

- `Send` sends bytes to the program as if typed, and `Paste` pastes text.
  `PasteData` pastes a `transfer.Data`, as the paste keys paste the
  clipboard: a program that reads the clipboard itself is offered every
  format, as MIME types, and reads those it wants, when it asks (a
  `transfer.Lazy` representation is made only then, on a goroutine of
  the terminal); any other gets the text. `Paste` is `PasteData` with
  text, which such a program reads as `text/plain`.
- `Feed` shows bytes as if the program wrote them, text and escape
  sequences.
- `Title`, `Dir` (the directory the shell reported), `Size` (in cells) and
  `Text` (the screen and its scrollback) read the terminal.
- `SetFont` and `SetTheme` change the font and the colors; `SetFont` also
  sets the size a zoom resets to, and drops the zoom.
- `Resize` sets the size of a terminal no view shows, and `Snapshot`
  returns what a terminal shows as escape sequences: its scrollback and
  screen with their styles, the cursor and the modes programs set. Fed to
  a new terminal of the same size, it shows the same, as when a server
  keeps a session's screen for the windows that attach to it.
- `Close` hangs up the program, which gets SIGHUP, and frees the terminal.

## Without a program

`Conn` connects the terminal to anything that reads and writes, such as an
SSH session or a serial port, instead of a program: what the terminal reads
from it shows, and what is typed is written to it. A `Conn` with a method
`Resize(cols, rows int) error` is told the size of the terminal:

```go
term, err := terminal.New(terminal.Options{Conn: session})
```

A terminal whose `Conn` never sends anything shows what the app writes
with `Feed`, as a log with colors would.

## The library

libghostty-vt is a native library of Ghostty, built for each platform from
the version of Ghostty the plugin binds, and published with MyGo. The
plugin's package names those builds and their SHA-256 in
`mygo-plugin.json`, and the CLI puts the one of each platform into the
apps it builds, among their resources, as
`libghostty-vt.dylib`, `libghostty-vt.so` or `ghostty-vt.dll`: a macOS app
signs it with the app, and a universal app gets both architectures in one
file.

A program not built by the CLI, as under `go run` and `go test`, downloads
the library once into the user's cache (`<cache>/mygo/natives/`), where
the CLI keeps those it downloads, and checks its SHA-256. Packaged apps
never download it. `terminal.LibraryPath` returns the library a program
loads, and `$MYGO_GHOSTTY_VT` names another library.

<!-- repository-only:start -->

To build libghostty-vt from Ghostty's sources:

```sh
git clone https://github.com/ghostty-org/ghostty && cd ghostty
zig build -Demit-lib-vt -Doptimize=ReleaseFast
MYGO_GHOSTTY_VT=$PWD/zig-out/lib/libghostty-vt.dylib go run ./examples/terminal
```

<!-- repository-only:end -->

`terminal.Load` loads the library, which `New` does too: call it first to
report a missing library before showing a window.

The library runs on macOS 13 or later, as Ghostty does, so apps with a
terminal set `minimumSystemVersion` to 13.0 or later; on Linux with glibc
2.28 or later (Debian 10, Ubuntu 20.04), on x64 and arm64; and on Windows
10 1809 or later, whose ConPTY runs the program, on x64 and arm64.

## Testing

`ui.NewTester` runs a terminal view without a window: type into it with
`Type` and `Key`, and read what the program printed with `Text`:

```go
func TestShell(t *testing.T) {
	term, err := terminal.New(terminal.Options{Command: []string{"/bin/sh"}})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	tt := ui.NewTester(func(c *ui.Context) { terminal.View(c, term).Fill().AutoFocus() }, 640, 400)
	tt.Type("echo hello")
	tt.Key(0, ui.KeyEnter)
	for !strings.Contains(term.Text(), "\nhello") {
		time.Sleep(10 * time.Millisecond)
		tt.Frame()
	}
}
```
