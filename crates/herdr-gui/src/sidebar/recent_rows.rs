//! [INPUT]: Constants, types, and root-level imports from the sidebar module root (`super`); crate::workspace_model's recent_targets (pure live HerdrState derivation) and shardlane_host's ProjectIndex; the shell_navigation FocusIntent project seam.
//! [OUTPUT]: Provides RECENT_TARGET_LIMIT and ShardlaneApp::sidebar_recent_section — the Sidebar "Recent" section (plain header + at most N jump rows), rendered only when live targets exist.
//! [POS]: Sidebar section layer of crates/herdr-gui::sidebar, slotted between Agents and Projects by shell.rs; rows navigate strictly through the existing FocusIntent seam (same channel as Search/switcher project targets) and add no action, persistence, settings, or collapse state.
use super::*;

/// Sidebar Recent cap (spec #6: the N most recently active Projects, N = 5).
pub(crate) const RECENT_TARGET_LIMIT: usize = 5;

impl ShardlaneApp {
    /// Live-derived quick-jump section over Projects. Empty runtime in, None
    /// out: the section vanishes entirely instead of rendering empty chrome.
    pub(super) fn sidebar_recent_section(
        &self,
        project_index: &shardlane_host::project_index::ProjectIndex,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let targets =
            crate::workspace_model::recent_targets(&self.state, project_index, RECENT_TARGET_LIMIT);
        if targets.is_empty() {
            return None;
        }
        let theme = cx.theme().clone();
        let mut section = v_flex().w_full().child(
            // Plain header on purpose: Recent owns no collapse state (the
            // other sections' collapse flags persist via settings, and
            // Recent must never touch settings).
            div()
                .w_full()
                .flex_shrink_0()
                .pl(SIDEBAR_EDGE + LEAD_INSET)
                .pr(SIDEBAR_EDGE)
                .pt(SPACE_MD)
                .pb(SPACE_XS)
                .text_size(FONT_BODY)
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.muted_foreground)
                .child("Recent"),
        );
        for target in targets {
            let row_id = format!("shardlane-recent-{}", target.runtime_workspace_id);
            let workspace_id = target.runtime_workspace_id.clone();
            let cwd_tail = target.cwd.to_string_lossy().into_owned();
            let herdr = cx.entity();
            section = section.child(
                div()
                    .id(ElementId::Name(row_id.into()))
                    .w_full()
                    .h(ROW_HEIGHT_SUB)
                    .flex_shrink_0()
                    .pl(SIDEBAR_EDGE + LEAD_INSET + SUB_INDENT)
                    .pr(SIDEBAR_EDGE)
                    .flex()
                    .items_center()
                    .gap(SPACE_XS)
                    .text_size(FONT_LABEL)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.foreground.opacity(crate::theme::WASH_HOVER)))
                    .active(|s| s.bg(theme.foreground.opacity(crate::theme::WASH_ACTIVE)))
                    .on_click(move |_, window, app| {
                        app.stop_propagation();
                        herdr.update(app, |this, cx| {
                            this.apply_focus_intent(
                                FocusIntent::project(workspace_id.clone()),
                                window,
                                cx,
                            )
                        });
                    })
                    .child(
                        icon("icons/rotate-cw.svg")
                            .with_size(px(12.0))
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(120.0))
                            .truncate()
                            .text_color(theme.foreground)
                            .child(target.title),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(theme.muted_foreground.opacity(0.8))
                            .child(cwd_tail),
                    ),
            );
        }
        Some(section.into_any_element())
    }
}
