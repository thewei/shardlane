//! Global Search / picker presentation only.
//!
//! The domain model (query parsing, result items, delegate) lives in `search_model.rs`; projection
//! building and navigation actions are still orchestrated by `main.rs`.

use super::*;

/// Search input row height (SEARCH_ROW_HEIGHT).
const SEARCH_ROW_HEIGHT: f32 = 60.0;
/// Two-line result row height (CONTENT_RESULT_ROW_HEIGHT).
const RESULT_ROW_HEIGHT: f32 = 60.0;
/// Empty state height (EMPTY_RESULTS_HEIGHT).
const EMPTY_RESULTS_HEIGHT: f32 = 180.0;
/// Section header height (SECTION_HEADER_HEIGHT).
const SECTION_HEADER_HEIGHT: f32 = 30.0;
/// Results-area bottom padding (RESULTS_BOTTOM_PADDING).
const RESULTS_BOTTOM_PADDING: f32 = 8.0;

pub(super) fn client_search_result_item(
    ix: IndexPath,
    item: &ClientSearchItem,
    result_width: f32,
    cx: &Context<ListState<ClientSearchDelegate>>,
) -> ListItem {
    let kind_label = item.target.kind_label();
    // Palette row: px(11)/rounded(9), 16px icon in a 20px box, 14px foreground title,
    // 12.5px muted detail, kind badge as a shortcut chip (h22/min28/r7) aligned right.
    // Explicit pixel width on the text column prevents MinContent collapse in List virtualized measurement.
    const SEARCH_ROW_TEXT_RESERVE: f32 = 130.0;
    let text_width = (result_width - SEARCH_ROW_TEXT_RESERVE).max(160.0);
    let body = h_flex()
        .w_full()
        .min_w_0()
        .items_center()
        .justify_between()
        .child(
            h_flex()
                .items_center()
                .gap(px(10.0))
                .child(
                    div()
                        .flex_none()
                        .size(px(20.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Icon::new(item.icon.clone()).with_size(px(16.0))),
                )
                .child(
                    v_flex()
                        .flex_none()
                        .w(px(text_width))
                        .gap(px(2.0))
                        .child(
                            div()
                                .w(px(text_width))
                                .truncate()
                                .text_size(crate::theme::FONT_SECTION_TITLE)
                                .text_color(cx.theme().foreground)
                                .child(item.title.clone()),
                        )
                        .child(
                            div()
                                .w(px(text_width))
                                .truncate()
                                .text_size(crate::theme::FONT_BODY)
                                .text_color(cx.theme().muted_foreground)
                                .child(item.detail.clone()),
                        ),
                ),
        )
        .child(
            div()
                .flex_none()
                .h(px(22.0))
                .min_w(px(28.0))
                .px(px(7.0))
                .rounded(px(7.0))
                .flex()
                .items_center()
                .justify_center()
                .bg(cx.theme().secondary.opacity(0.6))
                .text_size(crate::theme::FONT_BODY)
                .text_color(cx.theme().muted_foreground)
                .child(kind_label),
        );

    ListItem::new(ix)
        .w(px(result_width))
        .min_w(px(result_width))
        .max_w(px(result_width))
        .h(px(RESULT_ROW_HEIGHT))
        .px(px(11.0))
        .py_0()
        .rounded(px(9.0))
        .child(body)
}

/// Empty state (EMPTY_RESULTS_HEIGHT=180): 18px search icon + 13px MEDIUM primary line +
/// 12.5px secondary line; fixed height rather than filling (the card hugs its content).
pub(super) fn client_search_empty_state(
    cx: &Context<ListState<ClientSearchDelegate>>,
) -> AnyElement {
    v_flex()
        .h(px(EMPTY_RESULTS_HEIGHT))
        .flex_none()
        .items_center()
        .justify_center()
        .child(
            Icon::new(ComponentIconName::Search)
                .with_size(px(18.0))
                .text_color(cx.theme().muted_foreground.opacity(0.6)),
        )
        .child(
            div()
                .mt(px(12.0))
                .text_size(crate::theme::FONT_LIST_TITLE)
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().foreground.opacity(0.8))
                .child("No matching results"),
        )
        .child(
            div()
                .mt(px(5.0))
                .text_size(crate::theme::FONT_BODY)
                .text_color(cx.theme().muted_foreground)
                .child("Try a different name, path, Agent, or message."),
        )
        // Multi-instance scoping note: search only sees the bound workspace's
        // Herdr instance, never other workspaces' data.
        .child(
            div()
                .mt(px(10.0))
                .px(px(10.0))
                .py(px(3.0))
                .rounded(px(999.0))
                .border_1()
                .border_color(cx.theme().border)
                .text_size(crate::theme::FONT_META)
                .text_color(cx.theme().muted_foreground)
                .child("Scope: current workspace"),
        )
        .into_any_element()
}

/// Card width: w_full/max_w(680), with 24 scrims on each side.
/// The "Label ×" scope chip: clicking removes that token from the query and re-runs the search.
#[allow(clippy::too_many_arguments)]
fn search_scope_chip(
    token: &str,
    input: &Entity<InputState>,
    list: &Entity<ListState<ClientSearchDelegate>>,
    query: &str,
    _window: &Window,
    cx: &Context<ShardlaneApp>,
) -> AnyElement {
    let chip_input = input.clone();
    let chip_list = list.clone();
    let click_token = token.to_string();
    let click_query = query.to_string();
    h_flex()
        .flex_none()
        .h(px(22.0))
        .pl(px(8.0))
        .pr(px(4.0))
        .gap(px(2.0))
        .rounded(px(7.0))
        .bg(cx.theme().secondary.opacity(0.6))
        .text_size(crate::theme::FONT_BODY)
        .text_color(cx.theme().muted_foreground)
        .child(div().child(search_scope_chip_label(token)))
        .child(
            div()
                .id(SharedString::from(format!("palette-scope-clear-{token}")))
                .size(px(18.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.0))
                .cursor_pointer()
                .hover(|style| style.bg(cx.theme().foreground.opacity(0.08)))
                .child(Icon::new(ComponentIconName::Close).with_size(px(11.0)))
                .on_click(move |_, window, app| {
                    let narrowed = click_query
                        .split_whitespace()
                        .filter(|part| *part != click_token.as_str())
                        .collect::<Vec<_>>()
                        .join(" ");
                    chip_input.update(app, |state, cx| {
                        state.set_value(&narrowed, window, cx);
                    });
                    let job = chip_list.update(app, |state, lcx| {
                        state.delegate_mut().perform_search(&narrowed, window, lcx)
                    });
                    job.detach();
                }),
        )
        .into_any_element()
}

/// `#scope` token → chip display name (unknown tokens display as-is).
/// Audit E11: the vocabulary is shared with the parser and completion
/// (search_model::SCOPE_TOKENS) instead of a third hand-synced table here.
fn search_scope_chip_label(token: &str) -> String {
    match crate::search_model::client_search_scope_chip_label(token) {
        Some(label) => label.to_string(),
        None => token.to_string(),
    }
}

pub(super) fn client_picker_width(window_width: f64) -> f32 {
    (window_width - 48.0).clamp(320.0, 680.0) as f32
}

/// Input row: h60/px(19)/border-b/15.5px, borderless Input (no prefix icon, no clear button).
/// Scope completion renders as ghost text overlaid after the typed text (gray, a hint only, no layout);
/// Tab (or →) accepts it (PickerAcceptCompletion, see main.rs bindings).
fn palette_input_row(
    input: &Entity<InputState>,
    list: &Entity<ListState<ClientSearchDelegate>>,
    query: &str,
    completion: Option<&'static str>,
    window: &Window,
    cx: &Context<ShardlaneApp>,
) -> AnyElement {
    // Ghost tail: the completion candidate minus the typed prefix; shown only when the candidate starts with the current input.
    let ghost_tail = completion
        .filter(|completion| {
            !query.is_empty() && completion.starts_with(query) && completion.len() > query.len()
        })
        .map(|completion| completion[query.len()..].to_string());
    // Measure the typed text's width with the text system and stick the ghost after the tail (approximating the cursor position).
    let query_width = if ghost_tail.is_some() {
        let run = gpui::TextRun {
            len: query.len(),
            font: window.text_style().font(),
            color: gpui::Hsla::default(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        window
            .text_system()
            .layout_line(query, px(15.5), &[run], None)
            .width
    } else {
        px(0.0)
    };
    div()
        .relative()
        .w_full()
        .h(px(SEARCH_ROW_HEIGHT))
        .px(px(19.0))
        .flex_none()
        .flex()
        .items_center()
        .border_b_1()
        .border_color(cx.theme().border)
        .text_size(px(15.5))
        .text_color(cx.theme().foreground)
        .child(
            div()
                .min_w_0()
                .flex_1()
                .child(Input::new(input).appearance(false).p_0()),
        )
        // notate 2026-08-29: scope token visualization — each `#scope` renders as a
        // "Label ×" chip; clicking × removes the token from the query back to global; the
        // search box itself stays generic (#all means global, no chip rendered).
        .children(
            query
                .split_whitespace()
                .filter(|token| token.starts_with('#') && *token != "#all")
                .map(str::to_string)
                .map(|token| search_scope_chip(&token, input, list, query, window, cx))
                .collect::<Vec<_>>(),
        )
        .when_some(ghost_tail, |row, tail| {
            row.child(
                div()
                    .id("palette-input-ghost")
                    .absolute()
                    .top_0()
                    .h_full()
                    .left(px(19.0) + query_width)
                    .flex()
                    .items_center()
                    .text_size(px(15.5))
                    .text_color(cx.theme().muted_foreground.opacity(0.55))
                    .cursor_default()
                    .child(tail),
            )
        })
        .into_any_element()
}

pub(super) fn render_client_picker_overlay(
    picker: &ClientPickerOverlay,
    window: &Window,
    cx: &mut Context<ShardlaneApp>,
) -> AnyElement {
    let close_herdr = cx.entity();
    let window_size = window.bounds().size;
    // Geometry: top 9% of viewport height (clamped 48-72), light/dark scrims (26%/14%), card
    // w_full/max_w(680). Height is computed with the actual formula: input row + measured
    // total of section header/result rows (the cap mirrors MAX_CARD_HEIGHT semantics) — a fixed
    // height avoids the List's Infer sizing (whose MinContent measurement used the first row's
    // section header as the width basis and once squeezed the text column into ellipses).
    let top = (window_size.height.to_f64() * 0.09).clamp(48.0, 72.0);
    let (sections, rows) = picker.list.read(cx).delegate().palette_metrics();
    let results_height = if rows == 0 {
        EMPTY_RESULTS_HEIGHT
    } else {
        sections as f32 * SECTION_HEADER_HEIGHT + rows as f32 * RESULT_ROW_HEIGHT
    } + RESULTS_BOTTOM_PADDING;
    let results_max = (window_size.height.to_f64() - top - 36.0 - SEARCH_ROW_HEIGHT as f64)
        .clamp(180.0, 420.0) as f32;
    let results_height = results_height.min(results_max);
    let query = picker.input.read(cx).value().to_string();
    let completion = picker.list.read(cx).delegate().scope_completion();
    let is_dark = cx.theme().is_dark();
    let scrim = gpui::hsla(0.0, 0.0, 0.0, if is_dark { 0.26 } else { 0.14 });
    div()
        .absolute()
        .inset_0()
        .occlude()
        .on_mouse_down(MouseButton::Left, move |_, _, app| {
            close_herdr.update(app, |this, cx| {
                this.client_picker = None;
                this.search_open = false;
                this.sync_terminal_application_focus(cx);
                cx.notify();
            });
        })
        .flex()
        .items_start()
        .justify_center()
        .pt(px(top as f32))
        .px(px(24.0))
        .bg(scrim)
        .child(
            v_flex()
                .id("shardlane-client-picker")
                .key_context("ClientPicker")
                .on_mouse_down(MouseButton::Left, |_, _, app| app.stop_propagation())
                // ↑↓/Esc use the dedicated ClientPicker key bindings (the Input's cursor bindings are
                // single-line and don't intercept them; keys land on the nearest context match — the
                // same mechanism as the gpui-component List search); Enter is handled by the
                // InputState PressEnter event subscription (main.rs open_client_picker).
                .on_action(cx.listener(|this, _: &PickerSelectUp, window, cx| {
                    if let Some(picker) = this.client_picker.as_ref() {
                        picker.list.update(cx, |state, lcx| {
                            state.delegate_mut().move_selection(-1, window, lcx)
                        });
                    }
                }))
                .on_action(cx.listener(|this, _: &PickerSelectDown, window, cx| {
                    if let Some(picker) = this.client_picker.as_ref() {
                        picker.list.update(cx, |state, lcx| {
                            state.delegate_mut().move_selection(1, window, lcx)
                        });
                    }
                }))
                .on_action(cx.listener(|this, _: &PickerCancel, _, cx| {
                    this.client_picker = None;
                    this.search_open = false;
                    this.sync_terminal_application_focus(cx);
                    cx.notify();
                }))
                .on_action(
                    cx.listener(|this, action: &PickerAcceptCompletion, window, cx| {
                        this.picker_accept_completion(action, window, cx);
                    }),
                )
                .w_full()
                .max_w(px(680.0))
                .overflow_hidden()
                .rounded(px(15.0))
                .bg(cx.theme().popover)
                .shadow_xl()
                .child(palette_input_row(
                    &picker.input,
                    &picker.list,
                    &query,
                    completion,
                    window,
                    cx,
                ))
                .child(
                    // Results container: px(8)/pb(8), fixed height (see the height formula above).
                    List::new(&picker.list)
                        .h(px(results_height))
                        .px(px(8.0))
                        .pb(px(8.0)),
                ),
        )
        .into_any_element()
}
