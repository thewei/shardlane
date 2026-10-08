//go:build linux && (amd64 || arm64)

package linux

import "github.com/egoist/mygo/internal/gpu/gl"

// testLazyGL makes the GtkGLAreas of surfaces wait for UseGPU with
// MYGO_GPU=1 too, and surfaces have one without a GPU too (TestLazyGL).
var testLazyGL bool

// TestLazyGL makes the GtkGLArea of the windows created from now on make
// no context until their content asks for the GPU, with MYGO_GPU=1 too,
// which otherwise draws with OpenGL from the first frame. Windows have a
// GtkGLArea without a GPU too, where it makes none.
func TestLazyGL(on bool) { testLazyGL = on }

// TestUseGPU asks the surface of a window for the GPU, as its content does
// once drawing in memory costs too much, and reports whether it gave it.
func TestUseGPU(handle uintptr) bool {
	s := surfaceByHandle(handle)
	return s != nil && s.UseGPU()
}

// TestSurfaceInputLowest reports whether the input window of a window's
// GtkGLArea is below the other windows in its window, as those of a hidden
// title bar's controls, which take the pointer over the area.
func TestSurfaceInputLowest(handle uintptr) bool {
	s := surfaceByHandle(handle)
	if s == nil || !s.gl {
		return true
	}
	parent, last := gtkWidgetGetWindow(s.area), ptr(0)
	// GList, from the top: data 0, next 8.
	for l := gdkWindowPeekChildren(parent); l != 0; l = field[ptr](l, 8) {
		last = field[ptr](l, 0)
	}
	return last == s.eventWindow()
}

// TestSurfaceGL tells how the native UI of a window draws: "opengl" when
// its renderer draws in the GtkGLArea, "memory" when the GtkGLArea shows
// frames drawn on the CPU, "cairo" when a drawing area, or a GtkGLArea
// without a context, paints them with cairo, and "" before the first frame.
// For a GtkGLArea it returns its framebuffer's pixels too, premultiplied
// BGRA rows from the top, and their size.
func TestSurfaceGL(handle uintptr) (how string, pix []byte, width, height int) {
	s := surfaceByHandle(handle)
	switch {
	case s == nil:
		return "", nil, 0, 0
	case !s.gl || gtkGLAreaGetError(s.area) != 0:
		return "cairo", nil, 0, 0
	case !s.rendered:
		return "", nil, 0, 0
	}
	how = "opengl"
	if s.inMemory {
		how = "memory"
	}
	var makeCurrent, attachBuffers func(area ptr)
	if !bind(libGTK, &makeCurrent, "gtk_gl_area_make_current") || !bind(libGTK, &attachBuffers, "gtk_gl_area_attach_buffers") {
		return how, nil, 0, 0
	}
	// The area's framebuffer keeps the last frame it rendered.
	makeCurrent(s.area)
	attachBuffers(s.area)
	scale := int(max(gtkWidgetGetScaleFactor(s.area), 1))
	width, height = int(gtkWidgetGetAllocatedWidth(s.area))*scale, int(gtkWidgetGetAllocatedHeight(s.area))*scale
	pix, err := gl.ReadFramebuffer(width, height)
	if err != nil {
		return how, nil, 0, 0
	}
	return how, pix, width, height
}

// TestSurfaceOnScreen returns, as a PNG, what the display shows of the
// surface of a window: GTK repaints only what frames drawn in memory
// changed, and keeps the rest.
func TestSurfaceOnScreen(handle uintptr) []byte {
	s := surfaceByHandle(handle)
	var getFromWindow func(win ptr, x, y, width, height int32) ptr
	if s == nil || !bind(libGDK, &getFromWindow, "gdk_pixbuf_get_from_window") {
		return nil
	}
	// The area's GdkWindow is its parent's when it has none of its own.
	var at gdkRectangle
	if !gtkWidgetGetHasWindow(s.area) {
		gtkWidgetGetAllocation(s.area, &at)
	}
	pix := getFromWindow(gtkWidgetGetWindow(s.area), at.X, at.Y, gtkWidgetGetAllocatedWidth(s.area), gtkWidgetGetAllocatedHeight(s.area))
	if pix == 0 {
		return nil
	}
	defer gObjectUnref(pix)
	return pngFromPixbuf(pix)
}
