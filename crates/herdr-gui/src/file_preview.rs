//! [INPUT]: right_panel::files (LoadedFileContent/load_file_content, bounded
//! background loading), ui::syntax (lightweight highlighting), shell drag-strip
//! helper, FocusIntent-adjacent full-page action wiring (PickerCancel + Window
//! menu actions, same as the other full-content secondary surfaces).
//! [OUTPUT]: FilePreviewState, ShardlaneApp::open_file_preview/close_file_preview,
//! and file_preview_page — the full-content preview surface.
//! [POS]: The file-preview responsibility slice of the shell. Clicking a file
//! in the right-panel Files tree previews it HERE (content area), covering the
//! hosted TUI without tearing it down — a sanctioned native secondary surface.
use super::*;
use crate::right_panel::files::{
    file_icon_for_path, load_file_content, LoadedFileContent, FILE_PREVIEW_MAX_BYTES,
};
use ::gpui::img;

/// Visible render window (same policy as the former right-panel viewer):
/// presentation scales with the visible window, not the file size.
const PREVIEW_MAX_LINES: usize = 2000;

#[derive(Clone, Debug)]
pub(crate) struct FilePreviewState {
    pub(crate) relative_path: String,
    pub(crate) content: Option<Result<LoadedFileContent, String>>,
    pub(crate) loading: bool,
}

impl ShardlaneApp {
    pub(crate) fn open_file_preview(&mut self, relative_path: String, cx: &mut Context<Self>) {
        let Some(root) = self.active_project_path_for_right_panel() else {
            return;
        };
        self.file_preview = Some(FilePreviewState {
            relative_path: relative_path.clone(),
            content: None,
            loading: true,
        });
        cx.spawn(async move |this, cx| {
            let load_path = relative_path.clone();
            let loaded = cx
                .background_executor()
                .spawn(async move { load_file_content(&root, &load_path) })
                .await;
            let _ = this.update(cx, |view, cx| {
                if let Some(preview) = view.file_preview.as_mut() {
                    if preview.relative_path == relative_path {
                        preview.loading = false;
                        preview.content = Some(loaded);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn close_file_preview(&mut self, cx: &mut Context<Self>) {
        if self.file_preview.is_some() {
            self.file_preview = None;
            cx.notify();
        }
    }

    pub(super) fn file_preview_page(
        &mut self,
        _theme: UiTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(preview) = self.file_preview.clone() else {
            return div().into_any_element();
        };
        let content_theme = self.content_surface_theme(window);
        let close_herdr = cx.entity();

        let body: AnyElement = if preview.loading {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .items_center()
                .justify_center()
                .child(Spinner::new().small())
                .into_any_element()
        } else {
            match preview.content.as_ref() {
                Some(Ok(LoadedFileContent::Text(text))) => {
                    Self::render_preview_text(&preview.relative_path, text, &content_theme)
                }
                Some(Ok(LoadedFileContent::Image { absolute_path })) => div()
                    .id("file-preview-image-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(16.0))
                    .flex()
                    .justify_center()
                    .child(
                        img(absolute_path.clone())
                            .max_w_full()
                            .flex_shrink_0()
                            .rounded(px(8.0)),
                    )
                    .into_any_element(),
                Some(Ok(LoadedFileContent::Binary)) => crate::ui::empty_state::empty_state(
                    "Binary file — preview is text and image only",
                    content_theme.muted,
                )
                .into_any_element(),
                Some(Ok(LoadedFileContent::TooLarge { size_bytes })) => {
                    crate::ui::empty_state::empty_state(
                        format!(
                            "File too large to preview ({:.1} MB > {:.0} MB limit)",
                            *size_bytes as f64 / 1_048_576.0,
                            FILE_PREVIEW_MAX_BYTES as f64 / 1_048_576.0
                        ),
                        content_theme.muted,
                    )
                    .into_any_element()
                }
                Some(Err(error)) => {
                    crate::ui::empty_state::empty_state(error.clone(), content_theme.muted)
                        .into_any_element()
                }
                None => crate::ui::empty_state::empty_state(
                    "Preview not loaded yet",
                    content_theme.muted,
                )
                .into_any_element(),
            }
        };

        div()
            .id("file-preview-page")
            .size_full()
            .key_context("ShardlaneApp")
            .on_action(cx.listener(|this, _: &PickerCancel, _, cx| {
                this.close_file_preview(cx);
            }))
            // Early render return skips the shell's action registrations;
            // keep the Window menu alive on full-page surfaces.
            .on_action(cx.listener(ShardlaneApp::new_window))
            .on_action(cx.listener(ShardlaneApp::merge_all_windows))
            .bg(content_theme.background)
            .flex()
            .flex_col()
            .child(self.titlebar_drag_strip("file-preview-titlebar", cx))
            .child(
                div()
                    .h(px(40.0))
                    .flex_none()
                    .px(px(12.0))
                    .border_b_1()
                    .border_color(content_theme.border)
                    .flex()
                    .items_center()
                    .gap(SPACE_ICON)
                    .child(
                        Icon::empty()
                            .path(file_icon_for_path(&preview.relative_path))
                            .with_size(px(14.0))
                            .text_color(content_theme.foreground),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(crate::theme::FONT_BODY)
                            .text_color(content_theme.muted)
                            .child(preview.relative_path.clone()),
                    )
                    .child(
                        div()
                            .id("file-preview-close")
                            .size(px(24.0))
                            .rounded(px(5.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|s| {
                                s.bg(content_theme.foreground.opacity(crate::theme::WASH_HOVER))
                            })
                            .on_click(move |_, _, app| {
                                close_herdr.update(app, |this, cx| this.close_file_preview(cx));
                            })
                            .tooltip(crate::ui::tooltip::tooltip_fn("Close preview (Esc)"))
                            .child(
                                Icon::empty()
                                    .path("icons/x.svg")
                                    .with_size(px(13.0))
                                    .text_color(content_theme.muted),
                            ),
                    ),
            )
            .child(body)
            .into_any_element()
    }

    fn render_preview_text(
        relative_path: &str,
        text: &str,
        theme: &ContentSurfaceTheme,
    ) -> AnyElement {
        let lang = crate::ui::syntax::lang_for_path(
            std::path::Path::new(relative_path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(relative_path),
        );
        let mut highlighter = crate::ui::syntax::Highlighter::new(lang);
        let total_lines = text.lines().count();
        let shown_lines = total_lines.min(PREVIEW_MAX_LINES);
        let mut list = div().flex().flex_col();
        for (index, line) in text.lines().take(shown_lines).enumerate() {
            let spans = highlighter.line(line);
            let row = div()
                .flex()
                .items_start()
                .child(
                    div()
                        .w(px(44.0))
                        .pr(px(8.0))
                        .flex_none()
                        .text_align(gpui::TextAlign::Right)
                        .font_family("Berkeley Mono, Menlo, monospace")
                        .text_size(crate::theme::FONT_META)
                        .line_height(px(18.0))
                        .text_color(theme.muted.opacity(0.55))
                        .child((index + 1).to_string()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .font_family("Berkeley Mono, Menlo, monospace")
                        .text_size(crate::theme::FONT_BODY)
                        .line_height(px(18.0))
                        .text_color(theme.foreground),
                );
            let code_row = row.children(spans.into_iter().map(|span| {
                let color = match span.kind {
                    crate::ui::syntax::SyntaxKind::Keyword => theme.primary,
                    crate::ui::syntax::SyntaxKind::String => theme.success,
                    crate::ui::syntax::SyntaxKind::Comment => theme.muted.opacity(0.75),
                    crate::ui::syntax::SyntaxKind::Number => theme.primary,
                    crate::ui::syntax::SyntaxKind::Plain => theme.foreground,
                };
                div()
                    .text_color(color)
                    .child(span.text.replace(' ', "\u{00a0}"))
            }));
            list = list.child(code_row);
        }
        if total_lines > shown_lines {
            list = list.child(
                div()
                    .px(px(10.0))
                    .py(px(8.0))
                    .text_size(crate::theme::FONT_META)
                    .text_color(theme.muted)
                    .child(format!(
                        "… first {shown_lines} of {total_lines} lines — open the file in an editor for the full content"
                    )),
            );
        }
        div()
            .id("file-preview-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(10.0))
            .py(px(8.0))
            .child(list)
            .into_any_element()
    }
}
