//! [INPUT]: Constants, types, and root-level imports from the sidebar module root (`super`); full inheritance via `use super::*`.
//! [OUTPUT]: Row-level primitives: the TextColored coloring trait, `icon`, `sidebar_action_row`, the RowLevel/RowLead row models, project/tab drag ghosts (with Render), `sidebar_hint_row`, `group_header`, and the general-purpose `sidebar_row` renderer.
//! [POS]: Row-primitive layer of `crates/herdr-gui::sidebar`, consumed by shell/tree_rows/pane_rows/service_rows; mechanically split out of sidebar.rs and sharing the module-root namespace with its sibling submodules.
use super::*;

trait TextColored: Styled + Sized {
    fn text_colored(self, color: Hsla, size: Pixels) -> Self {
        self.text_color(color).text_size(size)
    }
}

impl<T: Styled + Sized> TextColored for T {}

pub(super) fn icon(path: &'static str) -> Icon {
    Icon::empty().path(path)
}

/// Full-width action row, same as before: 32 tall, px(4) horizontal padding,
/// rounded(7), gap(10); 16px icon inside a 20px box, 13px label, both
/// text_secondary (muted_foreground here); hover 6%, active 9% — alpha
/// semantics neutral enough to sit on any sidebar background. One-shot jump
/// buttons get no persistent selected state, only hover/active.
pub(super) fn sidebar_action_row(
    id: &'static str,
    icon: Icon,
    label: &'static str,
    theme: &gpui_component::Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .w_full()
        .h(ROW_HEIGHT)
        .flex_none()
        .px(SPACE_XS)
        .rounded(px(7.0))
        .flex()
        .items_center()
        .gap(px(10.0))
        .cursor_pointer()
        .hover(|style| style.bg(theme.foreground.opacity(crate::theme::WASH_HOVER)))
        .active(|style| style.bg(theme.foreground.opacity(crate::theme::WASH_ACTIVE)))
        .child(
            div()
                .size(px(20.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(icon.with_size(px(16.0)).text_color(theme.muted_foreground)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(FONT_BODY)
                .text_color(theme.muted_foreground)
                .child(label),
        )
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum RowLevel {
    Primary,
    Sub,
    Pane,
}

pub(super) enum RowLead {
    Icon(&'static str),
    Brand(&'static str),
    Project { expanded: bool },
}

#[derive(Clone)]
pub(super) struct SidebarProjectDrag {
    pub(super) workspace_id: String,
    pub(super) label: String,
    pub(super) position: Point<Pixels>,
}

impl SidebarProjectDrag {
    pub(super) fn position(mut self, position: Point<Pixels>) -> Self {
        self.position = position;
        self
    }
}

fn sidebar_drag_ghost(position: Point<Pixels>, label: &str, cx: &App) -> Div {
    crate::ui::drag::drag_ghost_row(
        position,
        label,
        crate::ui::drag::DragGhostStyle {
            padding_x: SPACE_SM,
            height: ROW_HEIGHT_SUB,
            radius: cx.theme().radius,
            font_size: FONT_CAPTION,
            gap: None,
        },
        cx,
    )
}

impl Render for SidebarProjectDrag {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        sidebar_drag_ghost(self.position, &self.label, cx)
    }
}

#[derive(Clone)]
pub(super) struct SidebarTabDrag {
    pub(super) tab_id: String,
    pub(super) workspace_id: String,
    pub(super) label: String,
    pub(super) position: Point<Pixels>,
}

impl SidebarTabDrag {
    pub(super) fn position(mut self, position: Point<Pixels>) -> Self {
        self.position = position;
        self
    }
}

impl Render for SidebarTabDrag {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        sidebar_drag_ghost(self.position, &self.label, cx)
    }
}

pub(super) fn sidebar_hint_row(
    text: impl Into<SharedString>,
    cx: &Context<ShardlaneApp>,
) -> impl IntoElement {
    div()
        .h(ROW_HEIGHT_SUB)
        .flex_shrink_0()
        .pl(LEAD_INSET + SUB_INDENT)
        .pr(SIDEBAR_EDGE)
        .flex()
        .items_center()
        .text_size(FONT_LABEL)
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

pub(super) fn sidebar_meta_pill(
    text: impl Into<SharedString>,
    background: Hsla,
    foreground: Hsla,
) -> impl IntoElement {
    div()
        .flex_none()
        .px(px(5.0))
        .py(px(1.0))
        .rounded(px(4.0))
        .bg(background)
        .text_size(FONT_LABEL)
        .text_color(foreground)
        .child(text.into())
}

/// SBX-07: collapsed-section summary glyph — same state language (dot shape
/// plus color) as the Activity/bell rows, replacing the text count pill.
pub(super) fn group_header_glyph(
    id: impl Into<crepuscularity_gpui::ElementId>,
    level: crate::status::AttentionLevel,
    count: usize,
    color: Hsla,
    cx: &App,
) -> AnyElement {
    h_flex()
        .flex_none()
        .gap(px(3.0))
        .text_size(FONT_LABEL)
        .text_color(color)
        .child(crate::status::status_glyph_container(id, level, cx))
        .when(count > 1, |row| row.child(format!("{count}")))
        .into_any_element()
}

pub(super) fn group_header(
    id: &'static str,
    text: &'static str,
    collapsed: bool,
    trailing: Option<AnyElement>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &Context<ShardlaneApp>,
) -> Stateful<Div> {
    let theme = cx.theme();
    div()
        .id(id)
        .group("section-header")
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .pl(SIDEBAR_EDGE + LEAD_INSET)
        .pr(SIDEBAR_EDGE)
        .pt(SPACE_MD)
        .pb(SPACE_XS)
        .cursor_pointer()
        .active(|s| s.opacity(INTERACTIVE_PRESSED_OPACITY))
        .on_click(on_click)
        .child(
            h_flex()
                .w_full()
                .gap(SPACE_XS)
                .text_size(FONT_BODY)
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.muted_foreground)
                .hover(|s| s.text_colored(theme.foreground, FONT_BODY))
                .child(text)
                .child(
                    div()
                        .flex_none()
                        .opacity(0.0)
                        .group_hover("section-header", |s| s.opacity(1.0))
                        .child(
                            icon("icons/chevron-right.svg")
                                .with_size(px(10.0))
                                .when(!collapsed, |ic| {
                                    ic.rotate(gpui::Radians(std::f32::consts::FRAC_PI_2))
                                }),
                        ),
                )
                .child(div().flex_1())
                .when_some(trailing, |header, action| {
                    header.child(div().flex_none().child(action))
                }),
        )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sidebar_row(
    list: &Rc<RovingList>,
    id: impl Into<SharedString>,
    lead: RowLead,
    label: impl Into<SharedString>,
    role_badge: Option<&'static str>,
    meta_text: Option<SharedString>,
    count: Option<i64>,
    loading: bool,
    status: Option<crate::status::AttentionLevel>,
    active: bool,
    level: RowLevel,
    on_activate: impl Fn(&mut Window, &mut App) + 'static,
    cx: &Context<ShardlaneApp>,
) -> Stateful<Div> {
    let theme = cx.theme();
    let focus_key = id.into();
    let focus_handle = list.row_handle(focus_key.as_ref(), cx);
    let sub = level != RowLevel::Primary;
    let pane = level == RowLevel::Pane;
    let lead_width = LEAD_BOX;
    let indicator_color = theme.accent;
    div()
        .id(ElementId::Name(focus_key.clone()))
        .group("sidebar-item-row")
        .h(if sub { ROW_HEIGHT_SUB } else { ROW_HEIGHT })
        .flex_shrink_0()
        .pl(if pane {
            LEAD_INSET + SUB_INDENT * 2.0
        } else if sub {
            LEAD_INSET + SUB_INDENT
        } else {
            LEAD_INSET
        })
        .pr(SIDEBAR_EDGE)
        .rounded(theme.radius)
        .cursor_pointer()
        .relative()
        .flex()
        .items_center()
        .child(
            div()
                .absolute()
                .left_0()
                .top(px(6.0))
                .bottom(px(6.0))
                .w(px(2.5))
                .rounded(px(1.5))
                .when(active, |bar| bar.bg(indicator_color))
                .when(!active, |bar| {
                    bar.invisible().group_hover("sidebar-item-row", |s| {
                        s.visible().bg(indicator_color.opacity(0.5))
                    })
                }),
        )
        .when(active, |s| {
            s.bg(theme.sidebar_accent)
                .text_color(theme.sidebar_accent_foreground)
        })
        .when(!active, |s| {
            s.text_color(theme.sidebar_foreground)
                .hover(|s| s.bg(theme.sidebar_accent.opacity(INTERACTIVE_HOVER_OPACITY)))
                .active(|s| s.bg(theme.sidebar_accent))
        })
        .shardlane_roving_row(
            list,
            focus_handle,
            focus_key,
            theme.primary.opacity(INTERACTIVE_FOCUS_OPACITY),
            on_activate,
        )
        .child(
            h_flex()
                .w_full()
                .gap(SPACE_SM)
                .child(
                    div()
                        .w(lead_width)
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(match lead {
                            RowLead::Icon(path) => icon(path)
                                .with_size(if sub { px(14.0) } else { px(15.0) })
                                .text_color(if active {
                                    theme.sidebar_accent_foreground
                                } else {
                                    theme.muted_foreground
                                })
                                .into_any_element(),
                            RowLead::Brand(path) => img(path).size(LEAD_BOX).into_any_element(),
                            RowLead::Project { expanded } => icon(if expanded {
                                "icons/folder-open.svg"
                            } else {
                                "icons/folder.svg"
                            })
                            .with_size(px(15.0))
                            .text_color(if active {
                                theme.sidebar_accent_foreground
                            } else {
                                theme.muted_foreground
                            })
                            .into_any_element(),
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(if sub { FONT_CAPTION } else { FONT_BODY })
                        .truncate()
                        .child(label.into()),
                )
                .when_some(role_badge, |this, badge| {
                    this.child(sidebar_meta_pill(
                        badge,
                        if active {
                            theme.sidebar_accent_foreground.opacity(0.14)
                        } else {
                            theme.sidebar_foreground.opacity(0.08)
                        },
                        if active {
                            theme.sidebar_accent_foreground.opacity(0.86)
                        } else {
                            theme.muted_foreground
                        },
                    ))
                })
                .when_some(meta_text, |this, text| {
                    this.child(
                        div()
                            .flex_shrink_0()
                            .text_size(FONT_LABEL)
                            .text_color(if active {
                                theme.sidebar_accent_foreground.opacity(0.82)
                            } else {
                                theme.muted_foreground
                            })
                            .child(text),
                    )
                })
                .when_some(status, |this, level| {
                    this.child(crate::status::status_glyph_container(
                        "sidebar-row-status",
                        level,
                        cx,
                    ))
                })
                .when(loading, |this| this.child(Spinner::new().xsmall()))
                .when_some(count, |this, n| {
                    this.child(
                        div()
                            .flex_shrink_0()
                            .text_size(FONT_LABEL)
                            .text_color(theme.muted_foreground)
                            .child(n.to_string()),
                    )
                }),
        )
}
