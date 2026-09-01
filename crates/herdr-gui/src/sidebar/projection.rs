//! [INPUT]: Constants, types, and root-level imports from the sidebar module root (`super`); full inheritance via `use super::*`.
//! [OUTPUT]: Provides sidebar_panes_for_tab (the pane projection in the layout's authoritative order), with regression tests.
//! [POS]: Pure projection/pure-function layer of `crates/herdr-gui::sidebar` (no rendering dependencies); consumed by tree_rows/pane_rows/shell; mechanically split out of sidebar.rs and sharing the module-root namespace with its sibling submodules.
use super::*;

pub(super) fn sidebar_panes_for_tab(
    panes: &[Pane],
    tab_id: &str,
    layout: Option<&PaneLayout>,
) -> Vec<Pane> {
    let mut projected = panes
        .iter()
        .filter(|pane| pane.tab_id.as_deref() == Some(tab_id))
        .cloned()
        .collect::<Vec<_>>();
    projected.sort_by(|left, right| {
        let position = |pane_id: &str| {
            layout
                .and_then(|layout| layout.panes.iter().find(|pane| pane.pane_id == pane_id))
                .map(|pane| (pane.rect.y, pane.rect.x))
                .unwrap_or((u32::MAX, u32::MAX))
        };
        position(&left.pane_id)
            .cmp(&position(&right.pane_id))
            .then_with(|| left.pane_id.cmp(&right.pane_id))
    });
    projected
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::herdr::LayoutPane;

    #[test]
    fn pane_projection_follows_authoritative_layout_position() {
        let pane = |pane_id: &str| Pane {
            pane_id: pane_id.to_string(),
            terminal_id: None,
            workspace_id: Some("w1".to_string()),
            tab_id: Some("t1".to_string()),
            label: None,
            title: None,
            terminal_title: None,
            cwd: None,
            agent_status: None,
            agent: None,
            focused: false,
            scroll: None,
        };
        let panes = vec![pane("p3"), pane("p1"), pane("p2")];
        let layout = PaneLayout {
            tab_id: "t1".to_string(),
            workspace_id: Some("w1".to_string()),
            area: LayoutRect {
                x: 0,
                y: 0,
                width: 120,
                height: 40,
            },
            panes: vec![
                LayoutPane {
                    pane_id: "p1".to_string(),
                    rect: LayoutRect {
                        x: 60,
                        y: 0,
                        width: 60,
                        height: 20,
                    },
                    focused: false,
                },
                LayoutPane {
                    pane_id: "p2".to_string(),
                    rect: LayoutRect {
                        x: 0,
                        y: 20,
                        width: 60,
                        height: 20,
                    },
                    focused: false,
                },
                LayoutPane {
                    pane_id: "p3".to_string(),
                    rect: LayoutRect {
                        x: 0,
                        y: 0,
                        width: 60,
                        height: 20,
                    },
                    focused: true,
                },
            ],
            splits: Vec::new(),
            focused_pane_id: Some("p3".to_string()),
            zoomed: false,
        };
        let ids = sidebar_panes_for_tab(&panes, "t1", Some(&layout))
            .into_iter()
            .map(|pane| pane.pane_id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["p3", "p1", "p2"]);
    }
}
