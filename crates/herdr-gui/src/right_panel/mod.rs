//! [INPUT]: The browser, files, lazygit, and services_view submodules, gpui, gpui_component, theme
//! [OUTPUT]: Provides RightPanelSurface (Files/Services/Lazygit/Browser{url,profile_id}: P1-6 freezes the profile as the surface identity at creation),
//!           RightPanelState, LazygitSession, and ShardlaneApp's right-panel rendering and interaction implementations
//! [POS]: Root module of the right-panel directory family in crates/herdr-gui. Since 2026-09-03 the
//! panel hosts the Services surface (moved from the Sidebar section); the File preview surface was
//! replaced by the full-content file preview (file_preview.rs).

pub(crate) mod browser;
mod browser_view;
mod chooser;
pub(crate) mod files;
mod files_view;
mod header;
pub(crate) mod lazygit;
mod lazygit_view;
mod services_view;
#[cfg(target_os = "macos")]
pub(crate) mod webview;

use std::collections::HashSet;
use std::path::PathBuf;

use super::*;
use crate::ContentSurfaceTheme;
use browser::{display_url, is_secure_url, resolve_address, search_url, AddressTarget};
use files::{collect_working_tree, WorkingTreeEntry};
use gpui_component::menu::DropdownMenu as _;

/// SBX-11: single definition of the built-in browser's default URL (previously
/// hardcoded in three places).
pub(crate) const BROWSER_DEFAULT_URL: &str = "http://localhost:3000";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RightPanelSurface {
    Files,
    /// Resident service scripts + observed listening processes for the bound
    /// instance. The Sidebar Services section moved here (2026-09-03).
    Services,
    Lazygit,
    /// P1-6 (audit 2026-08-27): the profile belongs to the surface identity and
    /// is frozen at creation — render/close always use this id and never read
    /// back the global default (the root cause of "opened with Profile A, then
    /// default changed to B, and A's surface session/close keys all mismatched").
    Browser {
        url: String,
        profile_id: String,
    },
}

impl RightPanelSurface {
    pub fn label(&self) -> String {
        match self {
            Self::Files => "Files".to_string(),
            Self::Services => "Services".to_string(),
            Self::Lazygit => "Lazygit".to_string(),
            Self::Browser { url, .. } => {
                if url.is_empty() {
                    "Browser".to_string()
                } else {
                    display_url(url).to_string()
                }
            }
        }
    }

    pub fn icon_path(&self) -> &'static str {
        match self {
            Self::Files => "icons/folder.svg",
            Self::Services => "icons/square-terminal.svg",
            Self::Lazygit => "icons/git-branch.svg",
            Self::Browser { .. } => "icons/globe.svg",
        }
    }
}

#[derive(Clone, Debug)]
pub struct RightPanelState {
    pub open: bool,
    pub width: f32,
    pub surfaces: Vec<RightPanelSurface>,
    pub active_surface: Option<usize>,
    pub file_tree_width: f32,
    pub files_selected_path: Option<String>,
    pub files_expanded_paths: HashSet<PathBuf>,
    pub working_tree: Vec<WorkingTreeEntry>,
    /// The URL the address bar last synced to (echo_page_url/address_dirty
    /// semantics: writes back only once after the surface URL changes, never
    /// overwriting user input frame by frame).
    pub address_synced_url: Option<String>,
    pub browser_url: String,
    pub browser_title: Option<String>,
    pub browser_history: Vec<String>,
    pub browser_history_idx: usize,
}

impl Default for RightPanelState {
    fn default() -> Self {
        Self {
            open: false,
            width: 440.0,
            surfaces: Vec::new(),
            active_surface: None,
            file_tree_width: 200.0,
            files_selected_path: None,
            files_expanded_paths: HashSet::new(),
            working_tree: Vec::new(),
            address_synced_url: None,
            browser_url: "http://localhost:3000".to_string(),
            browser_title: None,
            browser_history: vec!["http://localhost:3000".to_string()],
            browser_history_idx: 0,
        }
    }
}

/// Right-panel content bound to the Herdr runtime workspace (Product Project);
/// `open`/`width` are global chrome.
#[derive(Clone, Debug, Default)]
pub(crate) struct RightPanelProjectContent {
    pub surfaces: Vec<RightPanelSurface>,
    pub active_surface: Option<usize>,
    pub file_tree_width: f32,
    pub files_selected_path: Option<String>,
    pub files_expanded_paths: HashSet<PathBuf>,
    pub working_tree: Vec<WorkingTreeEntry>,
    pub address_synced_url: Option<String>,
    pub browser_url: String,
    pub browser_title: Option<String>,
    pub browser_history: Vec<String>,
    pub browser_history_idx: usize,
}

impl RightPanelProjectContent {
    fn from_panel(panel: &RightPanelState) -> Self {
        Self {
            surfaces: panel.surfaces.clone(),
            active_surface: panel.active_surface,
            file_tree_width: panel.file_tree_width,
            files_selected_path: panel.files_selected_path.clone(),
            files_expanded_paths: panel.files_expanded_paths.clone(),
            working_tree: panel.working_tree.clone(),
            address_synced_url: panel.address_synced_url.clone(),
            browser_url: panel.browser_url.clone(),
            browser_title: panel.browser_title.clone(),
            browser_history: panel.browser_history.clone(),
            browser_history_idx: panel.browser_history_idx,
        }
    }

    fn apply_to(&self, panel: &mut RightPanelState) {
        panel.surfaces = self.surfaces.clone();
        panel.active_surface = self.active_surface;
        panel.file_tree_width = self.file_tree_width;
        panel.files_selected_path = self.files_selected_path.clone();
        panel.files_expanded_paths = self.files_expanded_paths.clone();
        panel.working_tree = self.working_tree.clone();
        panel.address_synced_url = self.address_synced_url.clone();
        panel.browser_url = self.browser_url.clone();
        panel.browser_title = self.browser_title.clone();
        panel.browser_history = self.browser_history.clone();
        panel.browser_history_idx = self.browser_history_idx;
    }
}

impl ShardlaneApp {
    fn active_right_panel_context_runtime_id(&self) -> Option<String> {
        self.new_agent_context_workspace_id
            .clone()
            .or_else(|| self.state.focused_workspace_id.clone())
    }

    /// When the sidebar's Project context or runtime focus changes, switch the
    /// right-panel tab snapshot (independent per Project).
    pub(crate) fn sync_right_panel_for_project_context(&mut self, cx: &mut Context<Self>) {
        let target = self.active_right_panel_context_runtime_id();
        if self.right_panel_runtime_id == target {
            return;
        }

        if let Some(current) = self.right_panel_runtime_id.clone() {
            let snapshot = RightPanelProjectContent::from_panel(&self.right_panel);
            self.right_panel_projects.insert(current, snapshot);
            webview::hide_all(&self.browser_webviews);
        }

        // Project identity is the Lazygit session boundary. Never keep an auxiliary
        // child alive while the active Project changes; the next visible surface will
        // resolve the new Git root and launch a fresh child.
        self.stop_lazygit_session(cx);

        self.right_panel_runtime_id = target.clone();
        let content = target
            .and_then(|id| self.right_panel_projects.remove(&id))
            .unwrap_or_default();
        content.apply_to(&mut self.right_panel);

        if self.right_panel.open {
            self.refresh_right_panel_state(cx);
            self.ensure_lazygit_session(cx);
        } else {
            cx.notify();
        }
    }

    pub(crate) fn toggle_right_panel(&mut self, cx: &mut Context<Self>) {
        self.right_panel.open = !self.right_panel.open;
        // Same as before: slide out/in from the current occupied width. When
        // collapsed, hide the webviews immediately — they float above the GPUI
        // compositing layer and do not leave with the slide-out animation.
        if !self.right_panel.open {
            webview::hide_all(&self.browser_webviews);
            self.stop_lazygit_session(cx);
        }
        self.right_panel_slide = Some(WidthTween::new(self.right_panel_rendered_width));
        if self.right_panel.open {
            self.sync_right_panel_for_project_context(cx);
            self.refresh_right_panel_state(cx);
            // sync_right_panel_for_project_context may return early because the
            // Project is unchanged; but the auxiliary child was explicitly
            // stopped while the panel was closed, so reopening must rebind.
            if self.is_lazygit_surface_active() {
                self.ensure_lazygit_session(cx);
            }
        }
        cx.notify();
    }

    pub(crate) fn refresh_right_panel_state(&mut self, cx: &mut Context<Self>) {
        self.refresh_right_panel_working_tree(cx);
    }

    pub(crate) fn active_project_path_for_right_panel(&self) -> Option<PathBuf> {
        // Multi-instance cwd model (2026-09): a Project has NO working
        // directory of its own — Herdr gives every Tab its own cwd. Files (and
        // the Lazygit tool) follow the focused Tab's cwd, nothing else.
        self.active_tab_cwd_for_right_panel()
    }

    /// The focused Tab's working directory: its focused pane's cwd, else any pane
    /// cwd of that tab.
    fn active_tab_cwd_for_right_panel(&self) -> Option<PathBuf> {
        let tab_id = self.state.focused_tab_id.as_deref()?;
        let panes: Vec<&crate::herdr::Pane> = self
            .state
            .panes
            .iter()
            .filter(|pane| pane.tab_id.as_deref() == Some(tab_id))
            .collect();
        panes
            .iter()
            .find(|pane| pane.focused)
            .or_else(|| panes.first())
            .and_then(|pane| pane.cwd.as_deref())
            .map(PathBuf::from)
    }

    pub(crate) fn refresh_right_panel_working_tree(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.active_project_path_for_right_panel() else {
            lag_log(format_args!(
                "right_panel.files: no active project path (context={:?}, focused={:?})",
                self.new_agent_context_workspace_id, self.state.focused_workspace_id
            ));
            self.right_panel.working_tree = Vec::new();
            cx.notify();
            return;
        };
        if !path.is_dir() {
            lag_log(format_args!(
                "right_panel.files: project path is not a directory: {}",
                path.display()
            ));
            self.right_panel.working_tree = Vec::new();
            cx.notify();
            return;
        }
        let expanded = self.right_panel.files_expanded_paths.clone();
        lag_log(format_args!(
            "right_panel.files: collecting {} (expanded={})",
            path.display(),
            expanded.len()
        ));

        cx.spawn(async move |this, cx| {
            let tree = cx
                .background_executor()
                .spawn(async move { collect_working_tree(&path, &expanded) })
                .await;
            let _ = this.update(cx, |view, cx| {
                lag_log(format_args!(
                    "right_panel.files: collected {} entries",
                    tree.len()
                ));
                view.right_panel.working_tree = tree;
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn open_right_panel_surface(
        &mut self,
        surface: RightPanelSurface,
        cx: &mut Context<Self>,
    ) {
        self.right_panel.open = true;
        let mut found = None;
        for (idx, s) in self.right_panel.surfaces.iter().enumerate() {
            if std::mem::discriminant(s) == std::mem::discriminant(&surface) {
                found = Some(idx);
                break;
            }
        }

        if let Some(idx) = found {
            self.right_panel.surfaces[idx] = surface;
            self.right_panel.active_surface = Some(idx);
        } else {
            self.right_panel.surfaces.push(surface);
            self.right_panel.active_surface = Some(self.right_panel.surfaces.len() - 1);
        }
        if matches!(
            self.right_panel.surfaces[self.right_panel.active_surface.unwrap_or(0)],
            RightPanelSurface::Lazygit
        ) {
            self.ensure_lazygit_session(cx);
        } else {
            self.stop_lazygit_session(cx);
        }
        self.refresh_right_panel_state(cx);
        cx.notify();
    }

    /// Single seam for "show Services" (Header summary button, Window menu,
    /// the former Sidebar section header): opens the panel if closed and
    /// activates the Services surface.
    pub(crate) fn open_services_panel(&mut self, cx: &mut Context<Self>) {
        self.open_right_panel_surface(RightPanelSurface::Services, cx);
    }

    pub(crate) fn activate_right_panel_surface(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.right_panel.surfaces.len() {
            return;
        }
        self.right_panel.active_surface = Some(index);
        if matches!(self.right_panel.surfaces[index], RightPanelSurface::Lazygit) {
            self.ensure_lazygit_session(cx);
        } else {
            self.stop_lazygit_session(cx);
        }
        cx.notify();
    }
    pub(crate) fn close_right_panel_surface(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.right_panel.surfaces.len() {
            let closing_lazygit = matches!(
                self.right_panel.surfaces.get(index),
                Some(RightPanelSurface::Lazygit)
            );
            // Browser surface closed: the corresponding native WKWebView is
            // removed from the view tree and released; otherwise the leftover
            // floats above the GPUI compositing layer (the root cause of "only
            // the header hides while the content never leaves").
            if let Some(RightPanelSurface::Browser { url, profile_id }) =
                self.right_panel.surfaces.get(index)
            {
                // P1-6: the close key uses the profile frozen at surface creation;
                // the default key is kept as a fallback retirement for legacy
                // persistence/legacy sessions.
                let key = crate::browser_profile::BrowserSessionId::new(
                    profile_id.as_str(),
                    url.as_str(),
                );
                if let Some(webview) = self.browser_webviews.remove(&key) {
                    webview.remove();
                }
                let default_key = crate::browser_profile::BrowserSessionId::new(
                    self.config.browser.default_profile().id,
                    url.as_str(),
                );
                if default_key != key {
                    if let Some(webview) = self.browser_webviews.remove(&default_key) {
                        webview.remove();
                    }
                }
                let legacy =
                    crate::browser_profile::BrowserSessionId::default_session(url.as_str());
                if legacy != key && legacy != default_key {
                    if let Some(webview) = self.browser_webviews.remove(&legacy) {
                        webview.remove();
                    }
                }
                self.browser_addresses.remove(url);
                // BROWSER-05: the address subscription and the address table are
                // lifecycle-paired; otherwise the subscription leaks.
                self.browser_address_subscriptions.remove(url);
            }
            self.right_panel.surfaces.remove(index);
            if closing_lazygit {
                self.stop_lazygit_session(cx);
            }
            if self.right_panel.surfaces.is_empty() {
                self.right_panel.active_surface = None;
            } else if let Some(active) = self.right_panel.active_surface {
                if active >= index && active > 0 {
                    self.right_panel.active_surface = Some(active - 1);
                } else if active >= self.right_panel.surfaces.len() {
                    self.right_panel.active_surface = Some(self.right_panel.surfaces.len() - 1);
                }
            }
            if !self
                .right_panel
                .active_surface
                .and_then(|i| self.right_panel.surfaces.get(i))
                .is_some_and(|s| matches!(s, RightPanelSurface::Lazygit))
            {
                self.stop_lazygit_session(cx);
            }
        }
        cx.notify();
    }

    /// Counterpart of the original render_right_panel_toggle: a 26×26 r6 icon
    /// button, hover 5%, active 9% (the same three-state language as the seams),
    /// panel-right 14px muted; mouse_down stops propagation (the header drag
    /// zone sits below).
    pub(crate) fn render_right_panel_toggle_button(
        &self,
        herdr: Entity<Self>,
        hover: gpui::Hsla,
        active: gpui::Hsla,
        icon_color: gpui::Hsla,
    ) -> impl IntoElement {
        div()
            .id("titlebar-toggle-right-panel")
            .w(px(26.0))
            .h(px(26.0))
            .flex_none()
            .rounded(px(6.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .hover(move |style| style.bg(hover))
            .active(move |style| style.bg(active))
            .tooltip(crate::ui::tooltip::tooltip_fn("Toggle Right Panel"))
            .child(
                Icon::empty()
                    .path("icons/panel-right.svg")
                    .with_size(px(14.0))
                    .text_color(icon_color),
            )
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .on_click(move |_, _, app| {
                app.stop_propagation();
                herdr.update(app, |this, cx| {
                    this.toggle_right_panel(cx);
                });
            })
    }

    /// Whether the Browser surface is currently the active surface (drives the
    /// native webview visibility guard).
    fn right_panel_browser_active(&self) -> bool {
        self.right_panel
            .active_surface
            .and_then(|i| self.right_panel.surfaces.get(i))
            .is_some_and(|s| matches!(s, RightPanelSurface::Browser { .. }))
    }

    pub(crate) fn render_right_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let content_theme = self.content_surface_theme(window);
        let width = self.right_panel.width.clamp(280.0, 1000.0);

        // Visibility guard (webviews float above GPUI and do not leave with the
        // element tree): hide all webviews when the active surface is not
        // Browser — covering every path such as tab switches, tab closes, and
        // the chooser's empty state, so browser content never lingers.
        if !self.right_panel_browser_active() {
            webview::hide_all(&self.browser_webviews);
        }
        let body = match self
            .right_panel
            .active_surface
            .and_then(|i| self.right_panel.surfaces.get(i))
            .cloned()
        {
            None => self.render_right_panel_chooser(content_theme, cx),
            Some(RightPanelSurface::Files) => self.render_right_panel_files(content_theme, cx),
            Some(RightPanelSurface::Services) => {
                self.render_right_panel_services(content_theme, cx)
            }
            Some(RightPanelSurface::Lazygit) => {
                self.render_right_panel_lazygit_view(content_theme, window, cx)
            }
            Some(RightPanelSurface::Browser { url, profile_id }) => {
                self.render_right_panel_browser_view(&url, &profile_id, content_theme, window, cx)
            }
        };

        div()
            .id("right-panel")
            .w(px(width))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .min_w_0()
            .relative()
            .bg(content_theme.background)
            .child(self.render_right_panel_header(content_theme, cx))
            .child(body)
            .child(self.render_right_panel_resize_handle(content_theme, cx))
    }

    /// Geometry of the panel resize handle (the RightPanel branch): the hot-zone
    /// strip sits entirely to the left of the panel's left edge (-7..+1), with
    /// the visible 2px line at -2..0 — the hot zone never covers panel content;
    /// the line color uses our established three-state seam language
    /// (always-on 8% / hover 18% / dragging 30%), the same semantics as the left
    /// sidebar divider. mouse_down starts a RightPanel drag and stops
    /// propagation (terminal/panel content sits below, same stop as before).
    fn render_right_panel_resize_handle(
        &self,
        theme: ContentSurfaceTheme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let foreground = theme.foreground;
        let is_resizing = matches!(self.sidebar_drag, Some(SidebarDrag::RightPanel(..)));
        let divider_down = cx.listener(|this, event: &MouseDownEvent, _, cx| {
            // Dragging tracks the pointer directly; an unfinished slide-out yields.
            this.right_panel_slide = None;
            this.sidebar_drag = Some(SidebarDrag::RightPanel(
                event.position.x.to_f64(),
                this.right_panel.width as f64,
            ));
            cx.stop_propagation();
            cx.notify();
        });
        div()
            .id("right-panel-resize-handle")
            .group("right-panel-resize-handle")
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(-7.0))
            .w(px(8.0))
            .cursor(gpui::CursorStyle::ResizeLeftRight)
            .on_mouse_down(MouseButton::Left, divider_down)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(5.0))
                    .w(px(2.0))
                    .bg(foreground.opacity(if is_resizing { 0.30 } else { 0.08 }))
                    .group_hover("right-panel-resize-handle", move |s| {
                        s.bg(foreground.opacity(if is_resizing { 0.30 } else { 0.18 }))
                    }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn right_panel_surface_labels_and_icons() {
        let files = RightPanelSurface::Files;
        assert_eq!(files.label(), "Files");
        assert_eq!(files.icon_path(), "icons/folder.svg");

        let lazygit = RightPanelSurface::Lazygit;
        assert_eq!(lazygit.label(), "Lazygit");
        assert_eq!(lazygit.icon_path(), "icons/git-branch.svg");

        let browser_empty = RightPanelSurface::Browser {
            url: "".into(),
            profile_id: "p".into(),
        };
        assert_eq!(browser_empty.label(), "Browser");
        assert_eq!(browser_empty.icon_path(), "icons/globe.svg");

        let browser_url = RightPanelSurface::Browser {
            url: "https://example.com".into(),
            profile_id: "p".into(),
        };
        assert_eq!(browser_url.label(), "example.com");

        let services = RightPanelSurface::Services;
        assert_eq!(services.label(), "Services");
        assert_eq!(services.icon_path(), "icons/square-terminal.svg");
    }

    #[test]
    fn right_panel_default_state() {
        let state = RightPanelState::default();
        assert!(!state.open);
        assert!(state.surfaces.is_empty());
        assert_eq!(state.active_surface, None);
    }

    #[test]
    fn right_panel_project_content_round_trip() {
        let panel = RightPanelState {
            surfaces: vec![RightPanelSurface::Files, RightPanelSurface::Lazygit],
            active_surface: Some(1),
            working_tree: vec![WorkingTreeEntry {
                relative_path: "src".into(),
                absolute_path: PathBuf::from("/tmp/src"),
                name: "src".into(),
                is_dir: true,
                file_icon: "icons/folder.svg",
                expanded: false,
                depth: 0,
            }],
            ..RightPanelState::default()
        };

        let snapshot = RightPanelProjectContent::from_panel(&panel);
        let mut restored = RightPanelState::default();
        snapshot.apply_to(&mut restored);
        assert_eq!(restored.surfaces, panel.surfaces);
        assert_eq!(restored.active_surface, Some(1));
        assert_eq!(restored.working_tree.len(), 1);
        assert_eq!(panel.open, restored.open);
        assert_eq!(panel.width, restored.width);
    }
}
