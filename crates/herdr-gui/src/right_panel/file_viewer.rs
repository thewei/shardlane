//! [INPUT]: The import surface and types of the right_panel module root (`use super::*`).
//! [OUTPUT]: render_right_panel_file_viewer — the read-only single-file viewer.
//! [POS]: The file_viewer responsibility slice of the right_panel directory.
use super::*;

impl ShardlaneApp {
    pub(super) fn render_right_panel_file_viewer(
        &mut self,
        path: &str,
        theme: ContentSurfaceTheme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(root) = self.active_project_path_for_right_panel() else {
            return crate::ui::empty_state::empty_state(
                "No project path",
                cx.theme().muted_foreground,
            )
            .into_any_element();
        };

        let content =
            if let Some((cached_path, cached_content)) = &self.right_panel.file_content_cache {
                if cached_path == path {
                    cached_content.clone()
                } else {
                    let read = read_file_content(&root, path).unwrap_or_else(|e| e);
                    self.right_panel.file_content_cache = Some((path.to_string(), read.clone()));
                    read
                }
            } else {
                let read = read_file_content(&root, path).unwrap_or_else(|e| e);
                self.right_panel.file_content_cache = Some((path.to_string(), read.clone()));
                read
            };

        let file_icon = file_icon_for_path(path);
        let path_label = path.to_string();

        let mut lines_div = div()
            .flex()
            .flex_col()
            .font_family("Berkeley Mono, Menlo, monospace")
            .text_size(crate::theme::FONT_BODY)
            .line_height(px(18.0));

        for (ix, line) in content.lines().enumerate() {
            let line_num = ix + 1;
            lines_div = lines_div.child(
                div()
                    .flex()
                    .items_start()
                    .child(
                        div()
                            .w(px(36.0))
                            .pr(px(8.0))
                            .text_align(gpui::TextAlign::Right)
                            .text_color(theme.muted.opacity(0.6))
                            .child(line_num.to_string()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .pl(px(8.0))
                            .text_color(theme.foreground)
                            .child(if line.is_empty() { " " } else { line }.to_string()),
                    ),
            );
        }

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(32.0))
                    .px(px(10.0))
                    .border_b_1()
                    .border_color(theme.border)
                    .flex()
                    .items_center()
                    .gap(SPACE_ICON)
                    .child(
                        Icon::empty()
                            .path(file_icon)
                            .with_size(px(14.0))
                            .text_color(theme.foreground),
                    )
                    .child(
                        div()
                            .text_size(crate::theme::FONT_BODY)
                            .text_color(theme.muted)
                            .truncate()
                            .child(path_label),
                    ),
            )
            .child(
                div()
                    .id("right-panel-file-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .p(px(8.0))
                    .child(lines_div),
            )
            .into_any_element()
    }
}
