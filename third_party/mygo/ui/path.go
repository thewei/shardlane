package ui

import (
	"encoding/binary"
	"hash/maphash"
	"math"

	"github.com/egoist/mygo/internal/scene"
	"github.com/egoist/mygo/internal/vec"
)

// Path is a vector shape, in DIPs, for Painter.FillPath and StrokePath:
// icons, check marks, charts.
type Path struct {
	cmds []pathCmd
}

type pathCmd struct {
	op  uint8 // 0 move, 1 line, 2 quad, 3 cubic, 4 close
	pts [3][2]float32
}

// MoveTo starts a new subpath at (x, y).
func (p *Path) MoveTo(x, y float32) *Path {
	p.cmds = append(p.cmds, pathCmd{op: 0, pts: [3][2]float32{{x, y}}})
	return p
}

// LineTo draws a line to (x, y).
func (p *Path) LineTo(x, y float32) *Path {
	p.cmds = append(p.cmds, pathCmd{op: 1, pts: [3][2]float32{{x, y}}})
	return p
}

// QuadTo draws a quadratic Bézier curve to (x, y) with control point
// (cx, cy).
func (p *Path) QuadTo(cx, cy, x, y float32) *Path {
	p.cmds = append(p.cmds, pathCmd{op: 2, pts: [3][2]float32{{cx, cy}, {x, y}}})
	return p
}

// CubeTo draws a cubic Bézier curve to (x, y) with control points (c1x,
// c1y) and (c2x, c2y).
func (p *Path) CubeTo(c1x, c1y, c2x, c2y, x, y float32) *Path {
	p.cmds = append(p.cmds, pathCmd{op: 3, pts: [3][2]float32{{c1x, c1y}, {c2x, c2y}, {x, y}}})
	return p
}

// Close closes the subpath.
func (p *Path) Close() *Path {
	p.cmds = append(p.cmds, pathCmd{op: 4})
	return p
}

// Circle adds a circle as a subpath.
func (p *Path) Circle(cx, cy, r float32) *Path {
	const k = 0.5522847498
	p.MoveTo(cx+r, cy)
	p.CubeTo(cx+r, cy+r*k, cx+r*k, cy+r, cx, cy+r)
	p.CubeTo(cx-r*k, cy+r, cx-r, cy+r*k, cx-r, cy)
	p.CubeTo(cx-r, cy-r*k, cx-r*k, cy-r, cx, cy-r)
	p.CubeTo(cx+r*k, cy-r, cx+r, cy-r*k, cx+r, cy)
	return p.Close()
}

// flatPath is a path flattened to polylines in device pixels: polyline i
// holds pts[starts[i]:starts[i+1]], the last up to the end of pts. The
// engine keeps one for the paths it paints, reusing its memory.
type flatPath struct {
	pts    [][2]float32
	starts []int
	closed []bool
}

func (f *flatPath) polys() int { return len(f.starts) }

func (f *flatPath) poly(i int) [][2]float32 {
	end := len(f.pts)
	if i+1 < len(f.starts) {
		end = f.starts[i+1]
	}
	return f.pts[f.starts[i]:end]
}

// flatten sets f to the path's subpaths as polylines in device pixels.
func (p *Path) flatten(f *flatPath, scale float32) {
	f.pts, f.starts, f.closed = f.pts[:0], f.starts[:0], f.closed[:0]
	begin, isClosed := 0, false
	var origin, pen [2]float32
	flush := func() {
		if len(f.pts)-begin > 1 {
			f.starts = append(f.starts, begin)
			f.closed = append(f.closed, isClosed)
		} else {
			f.pts = f.pts[:begin]
		}
		begin, isClosed = len(f.pts), false
	}
	for _, c := range p.cmds {
		pts := c.pts
		for i := range pts {
			pts[i][0] *= scale
			pts[i][1] *= scale
		}
		switch c.op {
		case 0:
			flush()
			origin, pen = pts[0], pts[0]
			f.pts = append(f.pts, pen)
		case 1:
			pen = pts[0]
			f.pts = append(f.pts, pen)
		case 2:
			n := curveSteps(pen, pts[0], pts[1], pts[1])
			for i := 1; i <= n; i++ {
				t := float32(i) / float32(n)
				u := 1 - t
				f.pts = append(f.pts, [2]float32{u*u*pen[0] + 2*u*t*pts[0][0] + t*t*pts[1][0], u*u*pen[1] + 2*u*t*pts[0][1] + t*t*pts[1][1]})
			}
			pen = pts[1]
		case 3:
			n := curveSteps(pen, pts[0], pts[1], pts[2])
			for i := 1; i <= n; i++ {
				t := float32(i) / float32(n)
				u := 1 - t
				a, b, cc, d := u*u*u, 3*u*u*t, 3*u*t*t, t*t*t
				f.pts = append(f.pts, [2]float32{a*pen[0] + b*pts[0][0] + cc*pts[1][0] + d*pts[2][0], a*pen[1] + b*pts[0][1] + cc*pts[1][1] + d*pts[2][1]})
			}
			pen = pts[2]
		case 4:
			if len(f.pts) > begin {
				isClosed = true
				flush()
				pen = origin
			}
		}
	}
	flush()
}

func curveSteps(a, b, c, d [2]float32) int {
	l := dist(a, b) + dist(b, c) + dist(c, d)
	return max(2, min(int(math.Sqrt(float64(l))*2), 64))
}

func dist(a, b [2]float32) float32 {
	return float32(math.Hypot(float64(a[0]-b[0]), float64(a[1]-b[1])))
}

var pathSeed = maphash.MakeSeed()

// paths holds what the engine reuses to paint paths, frame after frame:
// the flattened path, the job of drawing its mask, and the rasterizer
// and pixels that draw it.
type paths struct {
	flat   flatPath
	job    pathJob
	raster vec.Rasterizer
	pix    []byte
}

// pathJob is the mask drawPath asks the text system for, which draws it
// with the engine's rasterizer and buffer when it is not cached.
type pathJob struct {
	f         *flatPath
	w, h      int
	hw        float32 // half the stroke's width; 0 fills
	x0, y0    float32
	rasterize func() (w, h int, pix []byte)
}

// drawPath paints a flattened path with c, from a mask cached by the shape:
// filled (non-zero winding), or with width > 0 stroked that many pixels
// wide with round joins and caps.
func (p *Painter) drawPath(f *flatPath, width float32, c Color, g *LinearGradient) {
	if f.polys() == 0 {
		return
	}
	hw := max(width, 0) / 2
	minX, minY := float32(math.MaxFloat32), float32(math.MaxFloat32)
	maxX, maxY := float32(-math.MaxFloat32), float32(-math.MaxFloat32)
	for _, pt := range f.pts {
		minX, maxX = min(minX, pt[0]), max(maxX, pt[0])
		minY, maxY = min(minY, pt[1]), max(maxY, pt[1])
	}
	x0, y0 := float32(math.Floor(float64(minX-hw))), float32(math.Floor(float64(minY-hw)))
	w, h := int(math.Ceil(float64(maxX+hw-x0)))+1, int(math.Ceil(float64(maxY+hw-y0)))+1
	if w <= 0 || h <= 0 || w > 4096 || h > 4096 {
		return
	}
	// The key covers the shape relative to the pixel grid, in quarters of
	// a pixel; a stroke's is its center line's, and its width. The points
	// move to those quarters, so that the mask of a key is the same
	// whichever path drew it first: a path looks the same wherever it is
	// on the grid, whatever drew before it.
	for i, pt := range f.pts {
		f.pts[i] = [2]float32{x0 + round((pt[0]-x0)*4)/4, y0 + round((pt[1]-y0)*4)/4}
	}
	key := uint64(1)
	if hw > 0 {
		key = 2 + uint64(math.Float32bits(hw))<<8
	}
	var buf [8]byte
	var hs maphash.Hash
	hs.SetSeed(pathSeed)
	binary.LittleEndian.PutUint64(buf[:], key)
	hs.Write(buf[:])
	for i := range f.polys() {
		for _, pt := range f.poly(i) {
			binary.LittleEndian.PutUint32(buf[:4], math.Float32bits(round((pt[0]-x0)*4)))
			binary.LittleEndian.PutUint32(buf[4:], math.Float32bits(round((pt[1]-y0)*4)))
			hs.Write(buf[:])
		}
		end := byte(0xff)
		if f.closed[i] {
			end = 0xfe
		}
		hs.WriteByte(end)
	}
	j := &p.rt.paths.job
	if j.rasterize == nil {
		j.rasterize = p.rt.rasterizePath
	}
	j.f, j.w, j.h, j.hw, j.x0, j.y0 = f, w, h, hw, x0, y0
	gi := p.rt.text.Mask(hs.Sum64(), j.rasterize)
	j.f = nil
	if !gi.OK {
		return
	}
	start := int32(len(p.s.Glyphs))
	c = c.Alpha(p.opacity)
	p.s.Glyphs = append(p.s.Glyphs, scene.Glyph{X: x0, Y: y0, W: float32(gi.W), H: float32(gi.H), U: gi.X, V: gi.Y, UW: gi.W, VH: gi.H, Color: c.scene(), Wide: p.glyphWide(c)})
	op := scene.Op{Kind: scene.OpGlyphs, Start: start, End: start + 1}
	if g != nil {
		// The gradient spans the path's bounds.
		op.Rect = scene.Rect{X: minX - hw, Y: minY - hw, W: maxX - minX + 2*hw, H: maxY - minY + 2*hw}
		op.Opacity = p.opacity
		p.gradient(&op, *g)
		op.Wide = p.wide(g.From, g.To, Color{})
	}
	p.s.Ops = append(p.s.Ops, op)
}

// rasterizePath draws the mask of the path job, into memory the engine
// reuses: the text system copies it into its atlas.
func (rt *engine) rasterizePath() (int, int, []byte) {
	j := &rt.paths.job
	z := &rt.paths.raster
	z.Reset(j.w, j.h)
	if j.hw > 0 {
		strokeInto(z, j.f, j.hw, j.x0, j.y0)
	} else {
		fillInto(z, j.f, j.x0, j.y0)
	}
	n := j.w * j.h
	if cap(rt.paths.pix) < n {
		rt.paths.pix = make([]byte, n)
	}
	pix := rt.paths.pix[:n]
	z.Mask(pix, j.w)
	return j.w, j.h, pix
}

// FillPath fills a path (non-zero winding).
func (p *Painter) FillPath(path *Path, c Color) {
	f := &p.rt.paths.flat
	path.flatten(f, p.scale)
	p.drawPath(f, 0, c, nil)
}

// FillPathGradient fills a path with a gradient across its bounds, as the
// area under a chart's line.
func (p *Painter) FillPathGradient(path *Path, g LinearGradient) {
	f := &p.rt.paths.flat
	path.flatten(f, p.scale)
	p.drawPath(f, 0, g.From, &g)
}

// StrokePath draws the outline of a path, width DIPs wide, with round
// joins and caps.
func (p *Painter) StrokePath(path *Path, width float32, c Color) {
	f := &p.rt.paths.flat
	path.flatten(f, p.scale)
	p.drawPath(f, width*p.scale, c, nil)
}

// StrokePathGradient draws the outline of a path as StrokePath does, in a
// gradient across its bounds.
func (p *Painter) StrokePathGradient(path *Path, width float32, g LinearGradient) {
	f := &p.rt.paths.flat
	path.flatten(f, p.scale)
	p.drawPath(f, width*p.scale, g.From, &g)
}

func fillInto(z *vec.Rasterizer, f *flatPath, x0, y0 float32) {
	for i := range f.polys() {
		poly := f.poly(i)
		z.MoveTo(poly[0][0]-x0, poly[0][1]-y0)
		for _, pt := range poly[1:] {
			z.LineTo(pt[0]-x0, pt[1]-y0)
		}
		z.ClosePath()
	}
}

// strokeInto draws a stroke hw pixels to each side of a flattened path: a
// quadrilateral per segment and a disc at every point, all counterclockwise
// so that where they overlap they add up instead of cancelling.
func strokeInto(z *vec.Rasterizer, f *flatPath, hw, x0, y0 float32) {
	for i := range f.polys() {
		poly := f.poly(i)
		n := len(poly)
		segs := n - 1
		if f.closed[i] {
			segs = n
		}
		for s := 0; s < segs; s++ {
			a, b := poly[s], poly[(s+1)%n]
			dx, dy := b[0]-a[0], b[1]-a[1]
			l := float32(math.Hypot(float64(dx), float64(dy)))
			if l == 0 {
				continue
			}
			nx, ny := -dy/l*hw, dx/l*hw
			polygon(z, [4][2]float32{
				{a[0] + nx - x0, a[1] + ny - y0}, {b[0] + nx - x0, b[1] + ny - y0},
				{b[0] - nx - x0, b[1] - ny - y0}, {a[0] - nx - x0, a[1] - ny - y0},
			})
		}
		for _, pt := range poly {
			disc(z, pt[0]-x0, pt[1]-y0, hw)
		}
	}
}

// polygon draws a quadrilateral counterclockwise.
func polygon(z *vec.Rasterizer, q [4][2]float32) {
	var area float32
	for i := range q {
		a, b := q[i], q[(i+1)%4]
		area += a[0]*b[1] - b[0]*a[1]
	}
	if area < 0 {
		q[0], q[1], q[2], q[3] = q[3], q[2], q[1], q[0]
	}
	z.MoveTo(q[0][0], q[0][1])
	z.LineTo(q[1][0], q[1][1])
	z.LineTo(q[2][0], q[2][1])
	z.LineTo(q[3][0], q[3][1])
	z.ClosePath()
}

// disc draws a circle of radius r as a polygon, counterclockwise.
func disc(z *vec.Rasterizer, cx, cy, r float32) {
	n := max(8, min(int(r*4), 32))
	z.MoveTo(cx+r, cy)
	for i := 1; i < n; i++ {
		a := float64(i) * 2 * math.Pi / float64(n)
		z.LineTo(cx+r*float32(math.Cos(a)), cy+r*float32(math.Sin(a)))
	}
	z.ClosePath()
}
