package nativeui

/**
 * [INPUT]: Git CommitInfo full parent hashes, ordered by the existing history reader
 * [OUTPUT]: Bounded native commit graph lanes/edges for the history table, with wire-continuous lane colors
 * [POS]: Pure Git history visual projection; no Git invocation in the render loop
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

// gitGraphRow is one commit's slice of the graph. Before/After are the
// active lanes around the row; the color slices parallel them by position
// but index lane identity (the hash), so a lane keeps its color while it
// compacts toward lane 0 and its wire reads as one continuous stroke.
type gitGraphRow struct {
	Before       []string
	After        []string
	BeforeColors []int
	AfterColors  []int
	Lane         int
	// Color is the commit's own lane color index.
	Color int
}

// gitHistoryGraph follows actual parent IDs, not branches inferred from
// subjects. A merge makes a secondary lane until both paths reconverge.
// Up to 200 commits are already bounded by the existing reader.
func gitHistoryGraph(commits []gitworkbench.CommitInfo) []gitGraphRow {
	rows := make([]gitGraphRow, len(commits))
	active := []string{}
	colors := map[string]int{}
	next := 0
	// freshColor hands a new lane its round-robin color.
	freshColor := func(hash string) {
		if _, ok := colors[hash]; !ok {
			colors[hash] = next % len(gitLaneColors)
			next++
		}
	}
	for i, cm := range commits {
		lane := slices.Index(active, cm.Hash)
		if lane < 0 {
			lane = len(active)
			active = append(active, cm.Hash)
		}
		freshColor(cm.Hash)
		before := append([]string(nil), active...)
		beforeColors := make([]int, len(before))
		for j, hash := range before {
			freshColor(hash)
			beforeColors[j] = colors[hash]
		}
		after := append([]string(nil), active...)
		if len(cm.Parents) == 0 {
			after = append(after[:lane], after[lane+1:]...)
		} else {
			after[lane] = cm.Parents[0]
			// The first parent continues this lane and inherits its wire
			// color; only an additional parent opens a genuinely new lane
			// and earns a fresh color. A parent already colored belongs to
			// an earlier lane (a rejoin) and keeps that color.
			for j, parent := range cm.Parents {
				if _, ok := colors[parent]; ok {
					continue
				}
				if j == 0 {
					colors[parent] = colors[cm.Hash]
				} else {
					colors[parent] = next % len(gitLaneColors)
					next++
				}
			}
			for j := 1; j < len(cm.Parents); j++ {
				parent := cm.Parents[j]
				if !slices.Contains(after, parent) {
					after = append(after, parent)
				}
			}
		}
		// Rejoining parents leave one lane, not duplicate wires.
		seen := map[string]bool{}
		unique := after[:0]
		for _, name := range after {
			if !seen[name] {
				unique = append(unique, name)
				seen[name] = true
			}
		}
		afterColors := make([]int, len(unique))
		for j, hash := range unique {
			freshColor(hash)
			afterColors[j] = colors[hash]
		}
		rows[i] = gitGraphRow{
			Before:       before,
			After:        append([]string(nil), unique...),
			BeforeColors: beforeColors,
			AfterColors:  afterColors,
			Lane:         lane,
			Color:        colors[cm.Hash],
		}
		active = append([]string(nil), unique...)
	}
	return rows
}

var gitLaneColors = []ui.Color{
	ui.Hex("#e9a134"), ui.Hex("#5da6ec"), ui.Hex("#65ba85"),
	ui.Hex("#c68ae5"), ui.Hex("#e7858f"), ui.Hex("#e5ca68"),
}

// gitGraphRowH is the graph glyph's height and, by being the tallest child
// of a padding-free row, the All Commits row pitch: adjacent glyphs tile
// edge to edge, so lane wires run unbroken across rows.
const gitGraphRowH float32 = 26

func gitGraphGlyph(c *ui.Context, graph gitGraphRow) {
	ui.Box(c).Width(76).Height(gitGraphRowH).Shrink(0).Draw(func(p *ui.Painter, r ui.Rect) {
		const step float32 = 11
		center := r.Y + r.H/2
		laneColor := func(idx int) ui.Color { return gitLaneColors[idx%len(gitLaneColors)] }
		for i, hash := range graph.Before {
			if i >= 6 {
				break
			}
			x := r.X + 9 + float32(i)*step
			color := laneColor(graph.BeforeColors[i])
			if i != graph.Lane {
				p.Line(x, r.Y, x, center, 1.4, color.Alpha(0.8))
			}
			target := slices.Index(graph.After, hash)
			if i != graph.Lane && target >= 0 && target < 6 {
				nx := r.X + 9 + float32(target)*step
				p.Line(x, center, nx, r.Y+r.H, 1.4, color.Alpha(0.8))
			}
		}
		for i := range graph.After {
			if i >= 6 {
				break
			}
			x := r.X + 9 + float32(i)*step
			p.Line(x, center, x, r.Y+r.H, 1.4, laneColor(graph.AfterColors[i]).Alpha(0.85))
		}
		if graph.Lane < 6 {
			x := r.X + 9 + float32(graph.Lane)*step
			color := laneColor(graph.Color)
			p.Line(x, r.Y, x, center, 1.5, color)
			p.Fill(ui.Rect{X: x - 4, Y: center - 4, W: 8, H: 8}, color, 4)
			p.Stroke(ui.Rect{X: x - 4, Y: center - 4, W: 8, H: 8}, color.Alpha(0.85), 4, 0.75)
			for j := 1; j < len(graph.After); j++ {
				if !slices.Contains(graph.Before, graph.After[j]) && j < 6 {
					to := r.X + 9 + float32(j)*step
					p.Line(x, center, to, r.Y+r.H, 1.4, laneColor(graph.AfterColors[j]))
				}
			}
		}
	})
}
