# Table

<!-- preview table 310 -->

`ui.Table` puts a [list](list.md)'s rows in columns, under a header of
`ui.TableColumn`s, each with a title, a width (or 0 to share the room the
others leave) and an alignment; `cell` builds the content of a row's
column, usually a `Text`. It builds only the rows in view, as a list does,
and rows are as high as their tallest cell, so text that wraps makes its
row taller.

```go
cols := []ui.TableColumn{{Title: "Name"}, {Title: "Size", Width: 90, Align: ui.End}}
app.files.Selected = &app.file
if ui.Table(c, &app.files, cols, len(files), func(row, col int) {
	switch col {
	case 0:
		ui.Text(c, files[row].Name).SingleLine()
	case 1:
		ui.Text(c, files[row].Size())
	}
}).Grow(1).Submitted() {
	app.open(files[app.file])
}
```

## Choosing rows

The table takes the same `ListState` as a list, or `nil`, and takes the
keyboard focus for it: with `Selected`, a click or the keys choose a row,
with `Selection` several, and `Submitted` reports a double click or Enter;
`Key`, `Label`, `Reorder` and the rest work as for a list. A row `Header`
names spans every column, which `cell` builds as column 0, and stays at the
top while its section's rows scroll under it.

Chosen and hovered rows are square from edge to edge, as in Finder's list
view; `RoundRows` rounds their corners at the theme's `Radius`, as a list's
rows are, for an app whose other lists are rounded.

## Arranging columns

The user arranges the columns, as in Finder:

- **Resizing.** Dragging the right edge of a column's header resizes the
  column, between its `MinWidth` and `MaxWidth`, and a double click there
  fits it to its cells. The columns before it that share the room left keep
  their widths, for the edge to follow the pointer.
- **Moving.** Dragging a header moves its column among the others.
- **Scrolling sideways.** Columns wider than the table scroll sideways, the
  header with them.
- **Fixed columns.** A `Fixed` column stays where it is, as wide as it is.

`ListState.Columns` keeps the order and the widths, by the columns' `ID`s
(their titles by default): set it from saved settings to restore them, and
save it as it changes.

## Sorting

With `ListState.Sort`, a click on the header of a `Sortable` column sorts
the rows by it, ascending, and a second click reverses the order: the header
shows an arrow, assistive technology reads the order, and the table's
`Changed` reports it. The table shows the rows in the order `cell` gets
them, so sort them as it says, keyed by `Key` for the choice to follow its
rows:

```go
cols := []ui.TableColumn{{Title: "Name", Sortable: true}, {Title: "Size", Width: 90, Align: ui.End, Sortable: true}}
app.files.Sort = &app.sort
sorted := sortFiles(app.files, app.sort) // by app.sort.Column, Descending or not
app.files.Key = func(i int) any { return sorted[i].ID }
```

To sort by several columns, set `ListState.SortBy`. A click still sorts by
one column, and reverses it when it was first; a Cmd-click (Ctrl-click on
Linux and Windows) or a Shift-click, as in web data grids, adds the column
after those sorted by, ascending; another makes it descending, and a third
takes it out. The headers show their direction and, when there are several,
their place. `SortBy` gets a new slice as it changes, so one the app kept
stays as it was. With `Sort` set too, the table keeps it the first of
`SortBy`, the zero `SortOrder` when it sorts by none, so code that reads the
column sorting first goes on reading `Sort`:

```go
app.files.SortBy = &app.sortBy // []ui.SortOrder, the first first
sorted := sortFiles(app.files, app.sortBy...)
```

## Header menu

`ListState.HeaderMenu` gives the header a context menu, built as
[ContextMenu](context-menu.md)'s is. `col` is the index of the column
clicked in the columns given to `Table`, whatever order the user moved them
into, or -1 for the part of the header past the columns, as AppKit's
`clickedColumn`. A menu built without items does not open:

```go
app.files.HeaderMenu = func(m *ui.Menu, col int) {
	if col >= 0 && m.Item("Hide Column").Chosen() {
		app.hide(cols[col].ID)
	}
	if m.Item("Show All Columns").Chosen() {
		app.showAll()
	}
}
```

## Renaming in place

An [editable text](editable-text.md) in a cell is renamed in place, as
Finder's file names: Return on macOS and F2 elsewhere edit the chosen row's
text, as does a click on it once the row is chosen.

```go
ui.Table(c, &app.files, cols, len(files), func(row, col int) {
	if col == 0 && ui.EditableText(c, &files[row].Name).Changed() {
		app.rename(files[row])
	}
})
```

## Accessibility

Assistive technology sees a table, named by its `Label`, of rows named by
the text inside them, each saying which of all the rows it is, with column
headers that say the order the rows are sorted in.

For a tree of rows in columns, see [Outline](outline.md#in-columns).
