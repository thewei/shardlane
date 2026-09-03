//! [INPUT]: The import surface and types of the right_panel module root (`use super::*`).
//! [OUTPUT]: render_right_panel_files — the file-tree browsing view; clicking a file opens the
//! full-content preview surface (ShardlaneApp::open_file_preview), not a right-panel surface.
//! [POS]: The files_view responsibility slice of the right_panel directory.
use super::*;

impl ShardlaneApp {
    pub(super) fn render_right_panel_files(
        &self,
        theme: ContentSurfaceTheme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let herdr = cx.entity();
        let entries = self.right_panel.working_tree.clone();

        let mut list = div().flex().flex_col().py(px(4.0));

        if entries.is_empty() {
            list = list.child(crate::ui::empty_state::empty_state(
                "No files found in project workspace",
                theme.muted,
            ));
        }

        for entry in entries {
            let rel = entry.relative_path.clone();
            let abs = entry.absolute_path.clone();
            let is_dir = entry.is_dir;
            let file_icon = entry.file_icon;
            let depth = entry.depth;
            let name = entry.name.clone();
            let expanded = entry.expanded;
            let click_herdr = herdr.clone();

            let row = div()
                .h(px(26.0))
                .mx(px(4.0))
                .pl(px(6.0 + depth as f32 * 14.0))
                .pr(px(6.0))
                .rounded(px(5.0))
                .flex()
                .items_center()
                .gap(SPACE_ICON)
                .cursor_pointer()
                .hover(|e| e.bg(theme.foreground.opacity(crate::theme::WASH_HOVER)))
                .child(if is_dir {
                    Icon::empty()
                        .path(if expanded {
                            "icons/chevron-down.svg"
                        } else {
                            "icons/chevron-right.svg"
                        })
                        .with_size(px(10.0))
                        .text_color(theme.muted)
                        .into_any_element()
                } else {
                    div().size(px(10.0)).flex_none().into_any_element()
                })
                .child(
                    Icon::empty()
                        .path(if is_dir {
                            "icons/folder.svg"
                        } else {
                            file_icon
                        })
                        .with_size(px(14.0))
                        .text_color(theme.foreground),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .text_size(crate::theme::FONT_BODY)
                        .text_color(theme.foreground)
                        .child(name),
                );

            list = list.child(row.on_mouse_down(MouseButton::Left, move |_, _, app| {
                let abs_clone = abs.clone();
                let rel_clone = rel.clone();
                click_herdr.update(app, |this, cx| {
                    if is_dir {
                        if !this.right_panel.files_expanded_paths.remove(&abs_clone) {
                            this.right_panel.files_expanded_paths.insert(abs_clone);
                        }
                        this.refresh_right_panel_working_tree(cx);
                    } else {
                        // 2026-09-03: file preview moved to the full-content
                        // surface (covers the hosted TUI, keeps it alive).
                        this.open_file_preview(rel_clone, cx);
                    }
                });
            }));
        }

        div()
            .id("right-panel-files-tree")
            .size_full()
            .overflow_y_scroll()
            .child(list)
            .into_any_element()
    }
}
