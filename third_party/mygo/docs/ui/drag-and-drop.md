# Drag and drop

`Drag(value)` makes an element the source of a value dragged within the
window: once the pointer pressing it moves a few DIPs, a translucent copy
of the element follows the pointer, and the press is no click.
`ui.Drop[T](e)` makes `e` take values of type `T`, returning the one
dropped on it, and `ui.DragOver[T](e)` the one over it, for showing it
would take it:

```go
for _, task := range app.todo {
	ui.Row(c).Key(task.ID).Drag(task).Children(func() {
		ui.Text(c, task.Name)
	})
}

done := ui.Column(c).Grow(1).Padding(12).Radius(8).Border(1, t.Border)
if _, over := ui.DragOver[*Task](done); over {
	done.Border(2, t.Accent)
}
if task, ok := ui.Drop[*Task](done); ok {
	task.Done = true
}
```

- **Targets.** The innermost element under the pointer that takes the
  value's type gets it, so a target inside another that takes other types
  does not block it.
- **Canceling.** Escape gives up a drag.
- **Scrolling.** Scroll containers scroll as a drag nears their top or
  bottom, faster nearer to the edge.
- **Dragging several.** The copy shows how many values a drag holds, beside
  the pointer, as when the rows chosen in a list are dragged together.

`Dragging` reports that an element is dragging its value, to dim it where
it was.

## Reordering

Lists, tables and grid views reorder their rows by dragging with
`ListState.Reorder` and `GridState.Reorder`: the rows chosen move together
when the row dragged is one of them, the list shows where they would go
with a line, and the rows go where the pointer let them go.

```go
app.list.Reorder = func(rows []int, to int) {
	app.items = move(app.items, rows, to) // before row to, len for the end
}
```

See [List](list.md#reordering) and [Grid view](grid-view.md).

Files dragged from other apps are [dropped files](input.md#dropped-files).
