//! Split-pane rectangle layout math (after the TUI-only convergence only directional
//! navigation neighbor computation remains).
//!
//! Pure function layer; holds no app/UI state. The native multi-pane render tree was
//! deleted along with the Embedded per-Pane Terminal; directional navigation (⌘⌥arrows)
//! still needs neighbor lookups in the local projection.
//!
//! [INPUT]: Depends on `crate::herdr`'s `LayoutRect`/`PaneLayout`
//! [OUTPUT]: Exposes `neighbor_pane_in_direction` (directional pane neighbor lookup)
//! [POS]: Local neighbor evaluation layer for directional navigation

#[cfg(test)]
use crate::herdr::LayoutRect;
use crate::herdr::PaneLayout;

/// Local directional navigation: find the pane adjacent to pane_id in the projected layout.
///
/// Navigation selection is client-local state (no focus is sent to Herdr), so neighbor
/// computation must happen on the client. The rules mirror tmux choose-pane: candidates
/// must lie entirely on the target side, the largest cross-axis overlap wins, and the
/// smallest gap breaks ties. A zoomed layout is a single full-screen pane with no neighbors.
pub(crate) fn neighbor_pane_in_direction(
    layout: &PaneLayout,
    pane_id: &str,
    direction: &str,
) -> Option<String> {
    if layout.zoomed {
        return None;
    }
    let horizontal = matches!(direction, "left" | "right");
    let forward = matches!(direction, "right" | "down");
    let source = layout
        .panes
        .iter()
        .find(|pane| pane.pane_id == pane_id)?
        .rect;
    let (source_start, source_end) = if horizontal {
        (source.x, source.x.saturating_add(source.width))
    } else {
        (source.y, source.y.saturating_add(source.height))
    };
    let (cross_start, cross_end) = if horizontal {
        (source.y, source.y.saturating_add(source.height))
    } else {
        (source.x, source.x.saturating_add(source.width))
    };

    layout
        .panes
        .iter()
        .filter(|pane| pane.pane_id != pane_id)
        .filter_map(|pane| {
            let rect = pane.rect;
            let (start, end) = if horizontal {
                (rect.x, rect.x.saturating_add(rect.width))
            } else {
                (rect.y, rect.y.saturating_add(rect.height))
            };
            let (cross, cross_far) = if horizontal {
                (rect.y, rect.y.saturating_add(rect.height))
            } else {
                (rect.x, rect.x.saturating_add(rect.width))
            };
            let (gap, in_direction) = if forward {
                (start.saturating_sub(source_end), start >= source_end)
            } else {
                (source_start.saturating_sub(end), end <= source_start)
            };
            if !in_direction {
                return None;
            }
            let overlap = cross_far
                .min(cross_end)
                .saturating_sub(cross.max(cross_start));
            Some((overlap, gap, pane.pane_id.clone()))
        })
        .max_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)))
        .map(|(_, _, pane_id)| pane_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::herdr::LayoutPane;

    fn rect(x: u32, y: u32, width: u32, height: u32) -> LayoutRect {
        LayoutRect {
            x,
            y,
            width,
            height,
        }
    }

    fn pane(id: &str, r: LayoutRect) -> LayoutPane {
        LayoutPane {
            pane_id: id.to_string(),
            rect: r,
            focused: false,
        }
    }

    fn layout(panes: Vec<LayoutPane>, zoomed: bool) -> PaneLayout {
        PaneLayout {
            tab_id: "w:t".to_string(),
            workspace_id: Some("w".to_string()),
            area: rect(0, 0, 100, 50),
            panes,
            splits: Vec::new(),
            focused_pane_id: None,
            zoomed,
        }
    }

    #[test]
    fn direction_neighbor_picks_largest_overlap_then_smallest_gap() {
        // Left column a (top) / b (bottom); right column c (top, overlapping a by 20 rows) / d (bottom).
        let grid = layout(
            vec![
                pane("a", rect(0, 0, 50, 40)),
                pane("b", rect(0, 40, 50, 10)),
                pane("c", rect(50, 0, 50, 20)),
                pane("d", rect(50, 20, 50, 30)),
            ],
            false,
        );
        assert_eq!(
            neighbor_pane_in_direction(&grid, "a", "right").as_deref(),
            Some("d")
        );
        assert_eq!(
            neighbor_pane_in_direction(&grid, "a", "down").as_deref(),
            Some("b")
        );
        assert_eq!(
            neighbor_pane_in_direction(&grid, "d", "up").as_deref(),
            Some("c")
        );
        assert_eq!(
            neighbor_pane_in_direction(&grid, "d", "left").as_deref(),
            Some("a")
        );
        // No neighbor beyond the edge.
        assert_eq!(neighbor_pane_in_direction(&grid, "a", "left"), None);
        assert_eq!(neighbor_pane_in_direction(&grid, "a", "up"), None);
    }

    #[test]
    fn direction_neighbor_ties_break_on_smaller_gap() {
        // To a's right there are equal-height e (near) and f (far): with equal overlap, the nearer e wins.
        let grid = layout(
            vec![
                pane("a", rect(0, 0, 20, 20)),
                pane("e", rect(30, 0, 20, 20)),
                pane("f", rect(60, 0, 20, 20)),
            ],
            false,
        );
        assert_eq!(
            neighbor_pane_in_direction(&grid, "a", "right").as_deref(),
            Some("e")
        );
    }

    #[test]
    fn direction_neighbor_returns_none_for_zoomed_or_unknown_pane() {
        let grid = layout(vec![pane("a", rect(0, 0, 100, 50))], true);
        assert_eq!(neighbor_pane_in_direction(&grid, "a", "right"), None);
        let grid = layout(vec![pane("a", rect(0, 0, 50, 50))], false);
        assert_eq!(neighbor_pane_in_direction(&grid, "missing", "right"), None);
    }
}
