# Views

The view is a function from your app's state to its interface. MyGo calls
it on the main thread to build every frame: after input, after you change
the state (see [below](#change-the-state-from-other-goroutines)), and while
something animates. Elements live for one frame: the state that lasts is
yours, in your own types, plus what MyGo keeps for each element from frame
to frame (focus, hover, scrolling, the text being edited, animations).

```go
// todoList is the app's state, which lasts.
type todoList struct {
	todos []Todo
	draft string
}

// view builds the interface from it.
func (app *todoList) view(c *ui.Context) {
	ui.Column(c).Fill().Padding(16).Gap(8).Children(func() {
		for i := range app.todos {
			ui.Checkbox(c, &app.todos[i].Done, app.todos[i].Title)
		}
		if ui.TextInput(c, &app.draft).Placeholder("New to-do").Submitted() {
			app.todos = append(app.todos, Todo{Title: app.draft})
			app.draft = ""
		}
	})
}
```

Make the state once, as the app starts, and give its view to the window:

```go
app := &todoList{}
mygo.NewWindow(mygo.WindowOptions{Title: "To-dos", Content: ui.View(app.view)})
```

`ui.View(app.view)` is the window's content (`mygo.WindowOptions.Content`);
a `Content` can serve several windows, each with its own element state.

The examples in these guides are parts of such a view: `c` is the view's
`*ui.Context`, and `app` its receiver, the value of your own type that
holds the state, as `todoList` here. A field such as `app.volume` or a
method such as `app.save()` is one you declare on that type. Values that
last, such as a [router](navigation.md), are fields made with the rest of
the state, never in the view, which runs for every frame.

## Events are questions

Events are questions you ask while building: `Clicked` reports whether the
element was clicked since the last frame, so the code that handles a click
sits where the button is built:

```go
if ui.Button(c, "Save").Clicked() {
	app.save()
}
```

When a handler changes the state while the view builds, MyGo builds the
frame again, so it always shows the outcome.

The rest of the frame builds first, though, after the handler: a change
to what the view is building pulls it from under the code building it,
as deleting an item from the slice a loop builds rows from, from a button
in one of the rows. The loop goes on over the items as they were, and
reads past the end of the slice. Note the item instead, and change the
slice once the loop is done:

```go
deleted := -1
for i := range app.items {
	item := app.items[i]
	ui.Row(c).Key(item.ID).Children(func() {
		ui.Text(c, item.Title).Grow(1)
		if ui.Button(c, "Delete").Clicked() {
			deleted = i
		}
	})
}
if deleted >= 0 {
	app.items = slices.Delete(app.items, deleted, deleted+1)
}
```

Widgets that change a value take a pointer to it, so they need no handler:
`ui.Checkbox(c, &app.settings.Sync, "Sync")` changes the field the moment
the user clicks. `Changed` reports that they did, for work that follows:

```go
if ui.TextInput(c, &app.query).Placeholder("Search").Changed() {
	app.results = search(app.query)
}
```

## Keys

MyGo tells elements apart by their position among their siblings. When the
siblings before an element can change, as in a list whose items you
insert, delete or reorder, give each item a `Key` so its state follows it:

```go
for i := range app.todos {
	todo := &app.todos[i]
	ui.Row(c).Key(todo.ID).Children(func() {
		ui.Checkbox(c, &todo.Done, todo.Title)
	})
}
```

Widgets that handle their input as they are created, such as `Checkbox`
here, take the key from an element around them: `Key` panics on them, as
their state would be lost.

## Change the state from other goroutines

The view reads your state on the main thread. Change it from other
goroutines with `Window.Update`, which runs a function on the main thread
and then draws a new frame:

```go
win := mygo.NewWindow(mygo.WindowOptions{Content: ui.View(app.view)})
go func() {
	items, err := fetchItems()
	win.Update(func() { app.items, app.err = items, err })
}()
```

`Window.Invalidate` only draws a new frame, for state you guard yourself.
Within the view, `c.Invalidate()` asks for another frame and `c.After(d)` for
one after a delay, such as a clock's next second.

## State of an element

An element keeps state of its own from frame to frame with `ui.Local`, for
widgets you build yourself; see [custom widgets](custom-widgets.md).
