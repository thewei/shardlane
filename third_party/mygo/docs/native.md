# Desktop APIs

MyGo wraps the parts of the operating system desktop apps use most, for
apps whose windows show web pages, native UI or both. Like the rest of
MyGo, each can be called from any goroutine.

## Dialogs

`mygo.Dialog` shows the system's file and message dialogs. Its methods
block until the user answers: call them from bound methods, which run on
goroutines of their own, or start a goroutine for them in event listeners
and menu items, which should return quickly. `Parent` attaches a dialog to a
window, as a sheet on macOS.

```go
// Pick files.
paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{
	Parent:   win,
	Title:    "Import",
	Filters:  []mygo.FileFilter{{Name: "Images", Extensions: []string{"png", "jpg"}}},
	Multiple: true,
}) // nil when canceled

// Choose where to save.
path, err := mygo.Dialog.Save(mygo.SaveDialogOptions{
	Parent:      win,
	DefaultPath: "notes.json",
}) // "" when canceled

// Ask a question.
res, err := mygo.Dialog.Message(mygo.MessageOptions{
	Parent:        win,
	Type:          mygo.MessageWarning,
	Message:       "Delete 3 notes?",
	Detail:        "You cannot undo this.",
	Buttons:       []string{"Delete", "Cancel"},
	CheckboxLabel: "Don't ask again",
})
if res.Button == 0 { /* Delete */ }

// Report an error.
mygo.Dialog.Error("Import failed", err.Error())
```

`OpenDialogOptions` also pick directories (`Directory`), show hidden files
and, on macOS, a message and a New Folder button. In message dialogs Enter
activates `DefaultButton` and Escape `CancelButton`, by default the first
button labeled Cancel or No.

## Notifications

```go
n := mygo.NewNotification(mygo.NotificationOptions{
	Title: "Export finished",
	Body:  "notes.json was saved to Documents.",
})
n.OnClick(func() { mainWindow().Focus() })
if err := n.Show(); err != nil {
	log.Println(err)
}
```

`Close` removes a notification. macOS shows notifications of packaged apps
only, which `mygo dev` and `mygo build` make; `mygo.NotificationsSupported()`
reports whether the app can show them. Linux shows them through the
desktop's notification service, Windows as notification-area balloons.
`Show` returns once the system has the notification. On macOS the first
one asks the user whether to allow notifications, and `Show` waits for the
answer; it returns `mygo.ErrNotificationsDenied` when they are not allowed,
by that answer or later in System Settings.

macOS keeps a notification in Notification Center after it is clicked,
until the app removes it: `Close` it in `OnClick` to take it away, or call
`mygo.ClearNotifications()`, which removes all of the app's notifications,
those of earlier runs too, as the app comes to the front:

```go
n.OnClick(func() {
	mainWindow().Focus()
	n.Close()
})

// What the user has seen in the app does not wait in Notification Center.
mygo.App.OnDidBecomeActive(func() { mygo.ClearNotifications() })
```

Linux asks the desktop's notification service to close them, which may
have closed them already, and Windows hides its balloon.

A notification can outlive the run that showed it, in Notification Center
on macOS, where a click launches the app again. Give it an `ID` of the
app's own, which `App.OnNotificationClick` receives for every click, that
one included when it is registered before `Run`; showing a notification
with the `ID` of one still shown replaces it. `Group` gathers notifications
in one stack of Notification Center, such as the messages of a
conversation (macOS):

```go
// IDs are "<conversation>/<message>".
mygo.App.OnNotificationClick(func(id string) {
	conversation, _, _ := strings.Cut(id, "/")
	openConversation(conversation)
})

mygo.NewNotification(mygo.NotificationOptions{
	ID:    "ada/" + msg.ID,
	Group: "ada",
	Title: "Ada",
	Body:  msg.Text,
}).Show()
```

## Clipboard

```go
mygo.Clipboard.WriteText("hello")
text := mygo.Clipboard.ReadText()

mygo.Clipboard.WriteHTML("<b>hello</b>")
mygo.Clipboard.WriteImage(png) // a PNG
img := mygo.Clipboard.ReadImage()

formats := mygo.Clipboard.AvailableFormats()
mygo.Clipboard.Clear()
```

Pages can use the web's `navigator.clipboard` too.

`Clipboard.Write(transfer.Data)` and `Clipboard.Read(formats...)` share the
native drag/drop model: ordered items, alternative representations, file
lists, custom MIME formats, and lazy providers. `Formats` discovers portable
names without requesting bytes; `ReadFormat`, `ReadFiles`, `WriteFiles`, and
`Flush` provide explicit negotiation, file transfer, and persistence. See
[Clipboard and drag data](data-transfer.md) for provider lifetimes and native
format mappings. The [clipboard example](../examples/clipboard) demonstrates
these operations.

## The shell

```go
mygo.Shell.OpenExternal("https://example.com") // the default browser
mygo.Shell.OpenPath("/Users/ada/report.pdf")   // the default app for the file
mygo.Shell.ShowItemInFolder(path)              // Finder, Explorer, the file manager
mygo.Shell.TrashItem(path)                     // to the trash
mygo.Shell.Beep()
```

`FileIcon` returns the icon the file manager shows for a file, a folder
or an application, as a PNG of the size asked for in pixels: pass the size
in DIPs times the display's scale factor.

```go
png, err := mygo.Shell.FileIcon("/Applications/Safari.app", 64)
```

On macOS it is Finder's icon (`NSWorkspace`), drawn from the
representation that fits the size best; a symbolic link has the icon of
what it points to, without Finder's arrow. On Windows it is Explorer's
(`IShellItemImageFactory`, icons only, no thumbnails), and the path may
also name a shell item, such as an app of the Start menu by
`shell:AppsFolder\<AppUserModelID>`. On Linux it is the icon of the file's
content type from the icon theme, and for a desktop entry (`.desktop`), the
icon of its application, or where the theme has none, that of desktop
entries.

`InstalledApps` lists the applications installed on the computer, sorted
by name, as a launcher shows them, and `LaunchApp` starts one, or brings
it to the front:

```go
go func() {
	apps, err := mygo.Shell.InstalledApps() // reads the disk: off the main thread
	if err != nil {
		return
	}
	for _, app := range apps {
		fmt.Println(app.Name, app.ID)
	}
	icon, _ := mygo.Shell.FileIcon(apps[0].Path, 64)
	_ = mygo.Shell.LaunchApp(apps[0])
}()
```

Each app has the `Name` the system shows, in the user's language, an
`ID` and the `Path` whose icon `FileIcon` draws:

| | the apps | `ID` | `Path` |
|---|---|---|---|
| macOS | the `.app` bundles of `/Applications`, `/System/Applications`, `~/Applications` and CoreServices' `Applications`, and of their folders (Utilities), plus Finder; an app behind symbolic links once, at the link, as `/Applications/Safari.app` | the bundle identifier, or the path for a bundle without one | the bundle |
| Windows | the Start menu's All apps, desktop and packaged apps alike (the shell's Applications folder) | the AppUserModelID, or for a desktop app without one, its path under a known folder | `shell:AppsFolder\<ID>` |
| Linux | the desktop entries menus show (not `NoDisplay`, `Hidden`, or meant for another desktop) | the desktop file ID, such as `org.gnome.Nautilus.desktop` | the desktop entry |

`InstalledApps` does not block the main thread when called from another
goroutine, and returns in tens of milliseconds. Drawing icons takes a few
milliseconds each, on the main thread: draw those that show, and keep
them.

## Displays

`mygo.Screen` describes the connected displays, in device-independent
pixels with the origin at the top-left corner of the primary display:

```go
primary := mygo.Screen.PrimaryDisplay()
fmt.Println(primary.Bounds, primary.WorkArea, primary.ScaleFactor)

for _, d := range mygo.Screen.Displays() { /* ... */ }

cursor := mygo.Screen.CursorScreenPoint()
d := mygo.Screen.DisplayNearestPoint(cursor)

mygo.Screen.OnDisplaysChanged(func() { /* rearrange windows */ })
```

`WorkArea` leaves out the menu bar, the Dock and taskbars.
`DisplayMatching(rect)` returns the display a rectangle overlaps most, the
one a window is on.

## Dark mode

`mygo.Theme` is the light or dark appearance, which pages follow through
the `prefers-color-scheme` media query:

```go
dark := mygo.Theme.IsDark()
mygo.Theme.OnUpdated(func() { log.Println("dark:", mygo.Theme.IsDark()) })

mygo.Theme.SetSource(mygo.ThemeDark) // or ThemeLight, or ThemeSystem to follow the system
```

`SetSource` may be called before `App.Run`, for example with an appearance
the user saved in the app's preferences: the app then starts in it.

## Accent color and accessibility

`mygo.Theme.Preferences()` returns the accent color of the desktop's own
controls and the accessibility settings they follow, all read at once:

```go
p := mygo.Theme.Preferences()
p.Accent             // a color.NRGBA (image/color), A 0 where the desktop has none
p.ReduceMotion       // the desktop asks for less motion
p.ReduceTransparency // the desktop asks for opaque surfaces, e.g. no vibrancy
p.HighContrast       // the desktop asks for more contrast
p.TextScale          // how many times larger text should be; 1 is the usual size
```

The accent is 8-bit sRGB with straight alpha; `ui.RGB(p.Accent.R, p.Accent.G, p.Accent.B)`
makes it a `ui.Color` for drawing of your own.

`ReduceMotion` is macOS's Reduce Motion, Windows's animation effects and
GNOME's animations turned off; `ReduceTransparency` macOS's Reduce
Transparency and Windows's transparency effects turned off, which Linux
desktops do not have, so it is false there; `HighContrast` macOS's
Increase Contrast, Windows's contrast themes and the higher contrast of
the settings portal (or a HighContrast GTK theme) on Linux. macOS has no
text scale, so `TextScale` is 1 there. The accent is missing on Linux
desktops whose settings portal has none, and in browsers.

`Theme.OnUpdated` fires when the desktop changes any of them, as
`SetSource` does. Reading them can call out to the desktop (the settings
portal on Linux), so MyGo reads them once and keeps them until the next
change. Package `ui`'s default theme already follows the accent, the
contrast and the text scale, and `Animate` reduced motion. Like the other
calls that need the running app, `Preferences` panics in `main` before
`App.Run`, and another goroutine calling it waits for the app to start.

Pages get them from document start on, as `preferences()` of
`mygo-runtime` and the `--mygo-accent` and `--mygo-text-scale` CSS
variables, see [the frontend guide](frontend.md#accent-color-and-accessibility);
`prefers-reduced-motion` and `prefers-contrast` already follow the desktop.
A `ThemePreferences` sent through an event or returned by a bound method
is that same JSON, and the generated client types it with mygo-runtime's
`ThemePreferences`.

## Power

```go
// Keep the computer awake during a long task, and the display on with true.
release := mygo.Power.KeepAwake("Exporting the video", false)
defer release()

mygo.Power.OnSuspend(func() { pauseSync() })
mygo.Power.OnResume(func() { resumeSync() })
mygo.Power.OnLockScreen(func() { hideSecrets() })
mygo.Power.OnUnlockScreen(func() { /* ... */ })

onBattery := mygo.Power.IsOnBattery()
idle := mygo.Power.IdleTime() // since the last keyboard or mouse input
```

The reason given to `KeepAwake` shows where the system lists what keeps it
awake. On Linux, `OnLockScreen` is called when the screen saver starts,
which usually locks the screen, and `IdleTime` is 0 on desktops that do not
report it.

## Network

`mygo.Network` tells whether the computer is online and whether the
connection is metered — a phone hotspot or a plan that counts the data,
where an app should hold off on large downloads:

```go
status := mygo.Network.Status()
if status.Online && !status.Metered {
	syncNow()
}

mygo.Network.OnChanged(func(s mygo.NetworkStatus) {
	log.Printf("online: %v, metered: %v", s.Online, s.Metered)
})
```

`Online` means the system has a connection to a network beyond the
computer, as the browsers' `navigator.onLine` does, which does not prove
that a server answers: a network behind a captive portal, or one whose
way to the internet is down, counts. macOS and Linux count a route out
(a satisfied path, GLib's `network-available`), Windows a connectivity
level of local access or more. `Metered` is an expensive path (cellular, a
phone's hotspot) or Low Data Mode on macOS, a metered connection on Linux,
and on Windows a connection whose cost is fixed or variable, or that
roams, is over its data limit or approaches it. In browsers `Online` is
`navigator.onLine`, and `Metered` is false.

`Status` is safe from any goroutine; on macOS the first call waits for
the system's first answer, which comes within milliseconds. Changes are
watched from the first `OnChanged` on, whose listener runs on the main
thread, only when the status changed; it returns a function that removes
the listener.

Pages already have `navigator.onLine`. To show the native status in the
UI, broadcast a typed event from the listener, so the generated client
receives it:

```go
// Connectivity is pushed to every window when the connection changes, as
// {online, metered}.
var Connectivity = mygo.NewEvent[mygo.NetworkStatus]("connectivity")

func init() {
	mygo.Network.OnChanged(func(s mygo.NetworkStatus) { Connectivity.Broadcast(s) })
}
```

## Secrets

`mygo.Secrets` keeps small secrets, such as access tokens and API keys, in
the system's credential store, which encrypts them for the user, unlike a
file in the user data directory: the login Keychain on macOS, the Secret
Service on Linux (GNOME Keyring, KWallet, KeePassXC…, through libsecret)
and the Credential Manager on Windows.

```go
if err := mygo.Secrets.Set("github-token", []byte(token)); err != nil {
	log.Println(err)
}

token, err := mygo.Secrets.Get("github-token")
if errors.Is(err, mygo.ErrSecretNotFound) {
	// Ask the user to sign in.
}

mygo.Secrets.Delete("github-token") // on sign out; nil when there is none
```

A secret may be at most `mygo.MaxSecretSize` (2560) bytes, the most the
Credential Manager keeps, and a key at most 256 bytes. To keep more,
encrypt it into a file in the user data directory with a key kept in
`Secrets`. The methods are safe from any goroutine, also before `Run`, and
may block while the system asks the user something, e.g. to unlock the
keyring; on the main thread the app keeps handling events meanwhile. To
use them from the page, bind a backend method that calls them, so that
only what the page needs crosses into it.

A store keeps a secret under a service and an account: `Secrets` takes the
key for the account and the app's identifier (`identifier` of
`mygo.config.ts`) for the service. The development app of `mygo dev` is
`<identifier>.dev`, so it keeps its secrets apart from the installed app's;
an app run without an identifier, as with `go run`, uses its name.
`Secrets.Service` chooses another service, to share secrets with another
program of the app, such as its command line tool, or to read those an
Electron app stored with keytar, which keeps them as `Secrets` does under
the service the app passed it:

```go
// keytar.getPassword("MyApp", "refresh-token") in the Electron app
token, err := mygo.Secrets.Service("MyApp").Get("refresh-token")
```

Secrets protect against other users and stolen disks, not against other
apps of the same user: macOS asks the user before another app reads them,
and before a rebuilt app does when it is not signed with the same
identity, while Linux and Windows let them read.

Where there is no credential store, the methods fail with an error for
which `errors.Is(err, errors.ErrUnsupported)` holds: in browsers, and on
Linux without libsecret (the `libsecret-1-0` package on Debian and Ubuntu),
a session bus or a Secret Service on it, as on servers and minimal
desktops. An app can then fall back, e.g. ask for the secret each time:

```go
token, err := mygo.Secrets.Get("github-token")
switch {
case errors.Is(err, errors.ErrUnsupported):
	// No credential store: ask the user, keep it in memory.
case errors.Is(err, mygo.ErrSecretNotFound):
	// Ask the user to sign in.
}
```

## Watching files

`mygo.WatchFiles` watches a file or a directory, and with `Recursive` the
directories in it, for changes. It returns once everything is watched, and
`Next` waits for the next changes and returns them in a batch; use it on a
goroutine of its own:

```go
w, err := mygo.WatchFiles(ctx, dir, mygo.FileWatchOptions{
	Recursive: true,
	Ignore: func(p string, isDir bool) bool {
		return isDir && (path.Base(p) == "node_modules" || path.Base(p) == ".git")
	},
})
if err != nil {
	return err
}
go func() {
	defer w.Close()
	for {
		events, err := w.Next(ctx)
		if err != nil {
			return // io.EOF once closed, or once dir went away
		}
		for _, e := range events {
			log.Println(e.Op, e.Path)
		}
	}
}()
```

An event's `Path` is relative to what is watched (`w.Root()`, its path
with symlinks resolved) and separated by slashes, `.` for itself, and `Op`
is `mygo.FileCreate`, `FileWrite`, `FileRemove` or `FileRename`, with
`OldPath` where a rename moved from. What is already there is not
reported. Changes wait for 50 ms of quiet (`Debounce`, right away when
negative) before they are reported, and never more than 500 ms
(`MaxDelay`). A watch compares what it sees with what it saw, so events say
what is different, not every step that made it so: treat them as hints to
read the files again. A write is a file whose size or modification time
changed, or that the system says was written: permissions and other
metadata, which indexers and antivirus touch, are not reported, nor are
directories written, whose changes are their entries'.

A watch doesn't look again until `Next` has taken the last batch: when the
app is busy, what changed meanwhile comes in the next batch, so nothing is
lost and nothing piles up. What the watch can't look at, such as a
directory without permission, is left out rather than ending it.
`Ignore` leaves out files and directories by
their path: a directory it ignores is never opened, and doesn't count in
`MaxEntries`.

A file is watched through its directory, as [fsnotify](https://github.com/fsnotify/fsnotify)
recommends: a file replaced, as editors save, is a write of `.`, one
removed a removal, and one that appears again a creation. When the
directory itself, or the directory of the file, is removed, renamed or
replaced, the last batch has a `FileRemove` of `.`, and `Next` then returns
`io.EOF`.

`Close`, or canceling the context given to `WatchFiles`, stops the watch:
`Next` returns `io.EOF`, and `Done` is closed once it has released its
handles. The app stops its watches when it quits. A watch stops with
`mygo.ErrWatchLimit` when its directory has more than `MaxEntries` files
and directories: 65536 on macOS, where one FSEvents stream covers the
whole tree, and 8192 elsewhere, where a backend may take a handle per
entry. The [watch plugin](plugins/watch.md) gives
pages watches of the directories and files an app chooses, and says how
each platform watches.

`HintsOnly` trades all of that precision for cost. The watch keeps no
snapshot and stats no file: it reports only `mygo.FileHint` events, each
naming a directory in or under which the system said something changed,
and `.` after the system lost changes. Which file changed, how, and whether
it was renamed are never known. It suits a dev reloader, which rebuilds on
any change:

```go
w, err := mygo.WatchFiles(ctx, dir, mygo.FileWatchOptions{
	Recursive: true,
	HintsOnly: true, // only "something under this directory changed"
	Ignore: func(p string, isDir bool) bool {
		return isDir && path.Base(p) == "node_modules"
	},
})
```

Never use it where the file matters: following a document, syncing, or
anything that would act on a path it can't know. With it, `Ignore` is asked
about directories only (a change to a file it would ignore still hints at
the file's directory), and `MaxEntries` counts the directories enrolled. On
macOS and Windows a recursive hints-only watch enrolls only its directory,
which the system watches whole; on Linux it enrolls every directory,
reading their entries' types without stat'ing them. Symlinks are still not
followed, nothing outside the directory is watched, and the watched
directory going away still ends the watch with a `FileRemove` of `.`.

## Global shortcuts

Global shortcuts work while the app is in the background, for example to
bring it to the front:

```go
err := mygo.GlobalShortcut.Register("CmdOrCtrl+Shift+Space", func() {
	win.Show()
	win.Focus()
})
```

The function runs on the main thread. Shortcuts use the syntax of
[accelerators](menus.md#accelerators); `Unregister`, `UnregisterAll` and
`IsRegistered` manage them. `Register` fails when the shortcut is taken,
by the app or, where the system tells, by another app.

On Linux under X11 the app grabs the keys itself. Under Wayland, where apps
cannot, the desktop binds shortcuts for the app through the XDG desktop
portal, which KDE Plasma 6 and GNOME 48 and later provide: the desktop may
ask the user to confirm new shortcuts and lets them pick other keys, and it
needs the app installed, since it identifies apps by their desktop entry.
The shortcut works shortly after `Register` returns. Windows the function
shows or focuses take the focus with the key press's activation token where
the desktop provides one, as GNOME does.
