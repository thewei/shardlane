package nativeui

/**
 * [INPUT]: Git CommitInfo full parent hashes, ordered by the existing history reader
 * [OUTPUT]: Bounded native commit graph lanes/edges for the history table
 * [POS]: Pure Git history visual projection; no Git invocation in the render loop
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

type gitGraphRow struct {
	Before []string
	After  []string
	Lane   int
}

// gitHistoryGraph follows actual parent IDs, not branches inferred from
// subjects. A merge makes a secondary lane until both paths reconverge.
// Up to 200 commits are already bounded by the existing reader.
func gitHistoryGraph(commits []gitworkbench.CommitInfo) []gitGraphRow {
	rows := make([]gitGraphRow, len(commits))
	active := []string{}
	for i, cm := range commits {
		lane := slices.Index(active, cm.Hash)
		if lane < 0 {
			lane = len(active)
			active = append(active, cm.Hash)
		}
		before := append([]string(nil), active...)
		after := append([]string(nil), active...)
		if len(cm.Parents) == 0 {
			after = append(after[:lane], after[lane+1:]...)
		} else {
			after[lane] = cm.Parents[0]
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
		rows[i] = gitGraphRow{Before: before, After: append([]string(nil), unique...), Lane: lane}
		active = append([]string(nil), unique...)
	}
	return rows
}

var gitLaneColors = []ui.Color{
	ui.Hex("#e9a134"), ui.Hex("#5da6ec"), ui.Hex("#65ba85"),
	ui.Hex("#c68ae5"), ui.Hex("#e7858f"), ui.Hex("#e5ca68"),
}

func gitGraphGlyph(c *ui.Context, graph gitGraphRow) {
	ui.Box(c).Width(76).Height(24).Shrink(0).Draw(func(p *ui.Painter, r ui.Rect) {
		const step float32 = 11
		center := r.Y + r.H/2
		for i, hash := range graph.Before {
			if i >= 6 {
				break
			}
			x := r.X + 9 + float32(i)*step
			color := gitLaneColors[i%len(gitLaneColors)]
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
			color := gitLaneColors[i%len(gitLaneColors)]
			p.Line(x, center, x, r.Y+r.H, 1.4, color.Alpha(0.85))
		}
		if graph.Lane < 6 {
			x := r.X + 9 + float32(graph.Lane)*step
			color := gitLaneColors[graph.Lane%len(gitLaneColors)]
			p.Line(x, r.Y, x, center, 1.5, color)
			p.Fill(ui.Rect{X: x - 4, Y: center - 4, W: 8, H: 8}, color, 4)
			p.Stroke(ui.Rect{X: x - 4, Y: center - 4, W: 8, H: 8}, color.Alpha(0.85), 4, 0.75)
			for j := 1; j < len(graph.After); j++ {
				if !slices.Contains(graph.Before, graph.After[j]) && j < 6 {
					to := r.X + 9 + float32(j)*step
					p.Line(x, center, to, r.Y+r.H, 1.4, gitLaneColors[j%len(gitLaneColors)])
				}
			}
		}
	})
}
