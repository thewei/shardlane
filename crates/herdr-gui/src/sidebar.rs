// Shardlane native sidebar: one canonical navigation surface for Workspace-scoped Projects, Tabs, Agents, Scripts, and History.
//! [INPUT]: Depends on the full ShardlaneApp/Pane/theme namespace from the crate root (super) and on the rows/shell/tree_rows/pane_rows/service_rows/projection/section_layout submodules.
//! [OUTPUT]: Exposes (crate-internally) SidebarProjection, agent_identity, secondary_sidebar_back_row, SIDEBAR_DEFAULT_WIDTH, plus shared constants and imports supplied to submodules via the module root namespace.
//! [POS]: The sole Sidebar module root for `crates/herdr-gui` — declares submodules, owns the root-level public surface and shared constants; all rendering/projection implementations live in the sidebar/ submodules.
use super::*;
use crate::assets::agent_brand_icon;
use crate::interaction::{InteractiveSurfaceExt as _, RovingList};
use crate::scripts::{ObservedService, ScriptKind, ScriptRecord, ScriptStatus};
use crate::theme::{
    FONT_BODY as FONT_CAPTION, FONT_LIST_TITLE as FONT_BODY, FONT_META as FONT_LABEL,
};
use crate::ui_metrics::{
    INTERACTIVE_FOCUS_OPACITY, INTERACTIVE_HOVER_OPACITY, INTERACTIVE_PRESSED_OPACITY,
    ROW_HEIGHT_PRIMARY as ROW_HEIGHT, ROW_HEIGHT_SUB, SIDEBAR_EDGE_INSET as SIDEBAR_EDGE,
    SPACE_ICON, SPACE_MD, SPACE_SM, SPACE_XS,
};
use crate::workspace_model::{build_project_index, visible_sidebar_projects_with_index};
use ::gpui::{img, ClickEvent, Div, ElementId, Hsla, Point, SharedString, Stateful, Styled};
use gpui_component::{h_flex, tooltip::Tooltip, v_flex};
use std::collections::HashSet;

pub(crate) const SIDEBAR_DEFAULT_WIDTH: f64 = 260.0;
const LEAD_BOX: Pixels = px(18.0);
const LEAD_INSET: Pixels = px(7.75);
const SUB_INDENT: Pixels = px(12.0);

mod pane_rows;
mod projection;
mod rows;
mod section_layout;
mod service_rows;
mod shell;
mod tree_rows;

// Mechanical split plumbing: re-aggregate the pub(super) surfaces of submodules with
// named items back into the module root namespace so submodules can reference each
// other via `use super::*` (equivalent to the pre-split single-file name scope).
// impl-only submodules (shell/tree_rows/pane_rows/service_rows) need no re-exports:
// method calls resolve through types, not the name scope.
use self::projection::*;
use self::rows::*;

pub(super) struct SidebarProjection<'a> {
    pub expanded_projects: &'a HashSet<String>,
    pub panes_by_project: &'a HashMap<String, Vec<Pane>>,
    pub project_pane_loads_in_flight: &'a HashSet<String>,
    /// Audit A11: last workspace_panes error per Project (rendered as a click-to-retry row).
    pub project_pane_errors: &'a HashMap<String, String>,
}

pub(crate) fn agent_identity(agent: &Agent) -> Option<&str> {
    agent
        .display_agent
        .as_deref()
        .or(agent.agent.as_deref())
        .or(agent.name.as_deref())
}

pub(super) fn secondary_sidebar_back_row(
    id: &'static str,
    on_activate: impl Fn(&mut Window, &mut App) + 'static,
    cx: &Context<ShardlaneApp>,
) -> Stateful<Div> {
    let theme = cx.theme();
    div()
        .id(id)
        .w_full()
        .min_w_0()
        .h(ROW_HEIGHT)
        .px(SPACE_SM)
        .rounded(theme.radius)
        .cursor_pointer()
        .flex()
        .items_center()
        .gap(SPACE_XS)
        .text_size(FONT_CAPTION)
        .text_color(theme.muted_foreground)
        .hover(|style| style.bg(theme.secondary_hover).text_color(theme.foreground))
        .active(|style| style.opacity(INTERACTIVE_PRESSED_OPACITY))
        .shardlane_interactive(
            theme.primary.opacity(INTERACTIVE_FOCUS_OPACITY),
            on_activate,
        )
        .child(Icon::new(ComponentIconName::ChevronLeft).xsmall())
        .child("Back to app")
}
