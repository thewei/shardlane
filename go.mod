module github.com/wh-studio/herdr-client

go 1.27.1

tool github.com/egoist/mygo/cmd/mygo

// LOCAL FRAMEWORK PATCH (F146/F147 + WIX-021, user-approved 2026-10-06):
// the vendored fork under third_party/mygo is v0.3.7 plus two local deltas
// — terminal Options.LocalDragSelect (scrollback panes select text and get
// a right-click menu while the wheel keeps reporting) and field focus
// delegation (AutoFocus on a SearchField/Combobox reaches its inner
// input). Remove this replace, bump the pin, and delete third_party/mygo
// once upstream ships both — Shardlane code needs no changes. See
// third_party/mygo/LOCAL-PATCH.md.
replace github.com/egoist/mygo => ./third_party/mygo

require (
	github.com/alecthomas/chroma/v2 v2.27.0
	github.com/egoist/mygo v0.4.5
)

require (
	github.com/dlclark/regexp2/v2 v2.2.1 // indirect
	github.com/ebitengine/purego v0.11.1 // indirect
	github.com/go-text/typesetting v0.3.5 // indirect
	golang.org/x/image v0.46.0 // indirect
)
