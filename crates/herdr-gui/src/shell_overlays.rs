//! [INPUT]: Depends on the ShardlaneApp type from the crate root (super) and existing types/imports (use super::*); no independent external dependencies.
//! [OUTPUT]: Exposes ShardlaneApp's overlay and secondary-surface toggles: help/about/picker/settings/mobile/agents/services (inherent impl shard).
//! [POS]: The `crates/herdr-gui` shell overlays responsibility domain, mechanically split out of main.rs; together with sibling shell_* modules it forms ShardlaneApp's method surface.
use super::*;

/// Audit A16: which sidebar section a collapse write targets.
enum SidebarSection {
    Projects,
    Agents,
}

impl ShardlaneApp {
    pub(super) fn open_about(
        &mut self,
        _: &OpenAbout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.about_open {
            window.close_dialog(cx);
            self.about_open = false;
            self.sync_terminal_application_focus(cx);
            cx.notify();
            return;
        }
        if window.has_active_dialog(cx) {
            return;
        }

        let herdr = cx.entity();
        let runtime_version = self
            .state
            .version
            .clone()
            .unwrap_or_else(|| "Unavailable".to_string());
        let protocol = self
            .state
            .protocol
            .map(|value| value.to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        let device = self.device.label.clone();
        self.about_open = true;
        self.clear_ime_state();
        self.sync_terminal_application_focus(cx);
        cx.notify();

        let dialog_width =
            responsive_dialog_width(window.bounds().size.width.to_f64(), 0.72, 300.0, 480.0);
        let dialog_herdr = herdr.clone();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let close_herdr = dialog_herdr.clone();
            let metadata = [
                ("Client", format!("v{}", env!("CARGO_PKG_VERSION"))),
                ("Herdr Runtime", runtime_version.clone()),
                ("Protocol", protocol.clone()),
                ("Terminal Engine", "libghostty-vt".to_string()),
                ("UI Components", "gpui-component 0.5.1".to_string()),
                ("Device", device.clone()),
            ];
            dialog
                .w(px(dialog_width))
                .margin_top(px(96.0))
                .close_button(true)
                .on_close(move |_, _, app| {
                    close_herdr.update(app, |view, cx| {
                        view.about_open = false;
                        view.sync_terminal_application_focus(cx);
                        cx.notify();
                    });
                })
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_3()
                        .py_3()
                        .child(
                            div()
                                .w(px(52.0))
                                .h(px(52.0))
                                .rounded(px(14.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(cx.theme().muted)
                                .child(Icon::new(ComponentIconName::SquareTerminal).size_8()),
                        )
                        .child(
                            div()
                                .text_size(theme::FONT_APP_TITLE)
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Shardlane"),
                        )
                        .child(
                            div()
                                .text_size(theme::FONT_DESCRIPTION)
                                .text_color(cx.theme().muted_foreground)
                                .child("Native macOS client for Herdr"),
                        )
                        .child(
                            div()
                                .w_full()
                                .mt_2()
                                .p_3()
                                .rounded(cx.theme().radius)
                                .bg(cx.theme().muted.opacity(0.45))
                                .children(metadata.into_iter().map(|(label, value)| {
                                    div()
                                        .w_full()
                                        .min_h(px(28.0))
                                        .flex()
                                        .flex_wrap()
                                        .items_center()
                                        .justify_between()
                                        .gap_2()
                                        .child(
                                            div()
                                                .text_size(theme::FONT_META)
                                                .text_color(cx.theme().muted_foreground)
                                                .child(label),
                                        )
                                        .child(
                                            div()
                                                .min_w_0()
                                                .whitespace_normal()
                                                .text_size(theme::FONT_META)
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(value),
                                        )
                                })),
                        ),
                )
        });
    }

    /// The header workspace-switch button opens the Project REGISTRY panel:
    /// every workspace (one Herdr instance each) plus adoptable foreign
    /// instances, with a filter. Switching rebinds this window or jumps to the
    /// workspace's existing window.
    /// Accept the input's ghost completion: replace the query with the completion
    /// candidate and re-run the search.
    /// Triggered by Tab/→ (ClientPicker context; see render_client_picker_overlay).
    pub(super) fn picker_accept_completion(
        &mut self,
        _: &PickerAcceptCompletion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(picker) = self.client_picker.as_ref() else {
            return;
        };
        let query = picker.input.read(cx).value().to_string();
        let Some(completion) = picker.list.read(cx).delegate().scope_completion() else {
            return;
        };
        if query.is_empty() || !completion.starts_with(&query) {
            return;
        }
        // The trailing space exits the token so the main term can keep being typed.
        let completed = format!("{completion} ");
        let input = picker.input.clone();
        let list = picker.list.clone();
        input.update(cx, |state, cx| {
            state.set_value(completed.clone(), window, cx);
        });
        let search_job = list.update(cx, |state, lcx| {
            state.delegate_mut().perform_search(&completed, window, lcx)
        });
        search_job.detach();
        cx.notify();
    }

    pub(super) fn open_client_picker(
        &mut self,
        placeholder: &'static str,
        items: Vec<ClientSearchItem>,
        history_db_path: Option<std::path::PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let herdr = cx.entity();
        let list_herdr = herdr.clone();
        // Virtual list MinContent measurement still needs fixed-width rows: card width minus container px(8)*2.
        let result_width =
            (search_view::client_picker_width(window.bounds().size.width.to_f64()) - 16.0).max(1.0);
        lag_log(format_args!(
            "picker.open win_w={:.0} result_width={result_width:.0}",
            window.bounds().size.width.to_f64()
        ));
        // No more searchable(true): the input row is drawn by the card itself (60px/15.5px
        // language); the List only handles the results area.
        let list = cx.new(|cx| {
            ListState::new(
                ClientSearchDelegate::new(list_herdr, items, history_db_path, result_width),
                window,
                cx,
            )
        });
        // Input row: self-managed InputState; Change searches, Enter confirms; the
        // subscription lives on the overlay and is replaced on each reopen.
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let input_for_events = search_input.clone();
        let list_for_events = list.clone();
        let subscription = cx.subscribe_in(
            &search_input,
            window,
            move |_, _, event, window, cx| match event {
                InputEvent::Change => {
                    let query = input_for_events.read(cx).value().to_string();
                    let search_job = list_for_events.update(cx, |state, lcx| {
                        state.delegate_mut().perform_search(&query, window, lcx)
                    });
                    search_job.detach();
                }
                InputEvent::PressEnter { .. } => {
                    list_for_events.update(cx, |state, lcx| {
                        state.delegate_mut().confirm(false, window, lcx)
                    });
                }
                _ => {}
            },
        );
        self.client_picker = Some(ClientPickerOverlay {
            list: list.clone(),
            input: search_input.clone(),
            _subscription: subscription,
        });
        self.search_open = true;
        self.clear_ime_state();
        self.sync_terminal_application_focus(cx);
        window.focus(&search_input.read(cx).focus_handle(cx));
        cx.notify();

        list.update(cx, |state, list_cx| {
            if !state.delegate().results.is_empty() {
                state.set_selected_index(Some(IndexPath::new(0)), window, list_cx);
            }
        });
    }

    pub(super) fn toggle_help(
        &mut self,
        _: &ToggleHelp,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_help = !self.show_help;
        self.sync_terminal_application_focus(cx);
        cx.notify();
    }

    /// Open the mobile control surface: switch to Settings's Mobile section.
    pub(super) fn open_mobile_surface(&mut self, cx: &mut Context<Self>) {
        self.show_settings = true;
        self.show_help = false;
        self.new_agent_open = false;
        self.leave_history_surface(cx);
        self.clear_ime_state();
        self.set_settings_section(SettingsSection::Mobile, cx);
        self.sync_terminal_application_focus(cx);
        self.notify_sidebar(cx);
        cx.notify();
    }

    pub(super) fn return_to_app_surface(&mut self, cx: &mut Context<Self>) {
        self.show_settings = false;
        self.new_agent_open = false;
        self.leave_history_surface(cx);
        self.show_help = false;
        self.clear_ime_state();
        self.sync_terminal_application_focus(cx);
        self.notify_sidebar(cx);
        // Defensive recovery: if sidebar should be visible but rendered width is stale 0,
        // force a fresh slide-in from zero so the shell always reclaims sidebar space.
        if self.shell_sidebar_visible() && self.sidebar_rendered_width <= 0.0 {
            self.sidebar_slide = Some(WidthTween::new(0.0));
        }
        cx.notify();
    }

    pub(super) fn toggle_sidebar(
        &mut self,
        _: &ToggleSidebar,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.config.ui.sidebar.collapsed = !self.config.ui.sidebar.collapsed;
        self.sidebar_collapsed = self.config.ui.sidebar.collapsed;
        // set_sidebar_visible semantics: slide from the current rendered width so a
        // mid-slide reversal never jumps.
        self.sidebar_slide = Some(WidthTween::new(self.sidebar_rendered_width));
        self.save_config();
        self.notify_sidebar(cx);
        cx.notify();
    }

    /// Audit A16: the one writer for sidebar-section collapse state — flips the config bit,
    /// mirrors the render field, optionally re-reveals the sidebar itself (reveal paths),
    /// persists, and refreshes the sidebar (plus root when requested).
    fn set_section_collapsed(
        &mut self,
        section: SidebarSection,
        collapsed: bool,
        reveal_sidebar: bool,
        notify_root: bool,
        cx: &mut Context<Self>,
    ) {
        match section {
            SidebarSection::Projects => {
                self.config.ui.sidebar.projects_collapsed = collapsed;
                self.projects_collapsed = collapsed;
            }
            SidebarSection::Agents => {
                self.config.ui.sidebar.agents_collapsed = collapsed;
                self.agents_collapsed = collapsed;
            }
        }
        if reveal_sidebar {
            // set_sidebar_visible semantics: slide from the current rendered width so a
            // mid-slide reversal never jumps.
            self.sidebar_slide = Some(WidthTween::new(self.sidebar_rendered_width));
            self.sidebar_collapsed = false;
            self.config.ui.sidebar.collapsed = false;
        }
        self.save_config();
        self.notify_sidebar(cx);
        if notify_root {
            cx.notify();
        }
    }

    pub(super) fn toggle_projects(&mut self, cx: &mut Context<Self>) {
        let collapsed = !self.config.ui.sidebar.projects_collapsed;
        self.set_section_collapsed(SidebarSection::Projects, collapsed, false, false, cx);
    }

    pub(super) fn toggle_agents(
        &mut self,
        _: &ToggleAgents,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let collapsed = !self.config.ui.sidebar.agents_collapsed;
        self.set_section_collapsed(SidebarSection::Agents, collapsed, false, false, cx);
    }

    pub(super) fn reveal_agents_section(&mut self, cx: &mut Context<Self>) {
        self.set_section_collapsed(SidebarSection::Agents, false, true, true, cx);
    }

    pub(super) fn reveal_services_section(&mut self, cx: &mut Context<Self>) {
        // The Sidebar Services section moved into the right panel (2026-09-03);
        // "reveal" now means open/activate the Services surface there.
        self.open_services_panel(cx);
    }

    pub(super) fn toggle_services(
        &mut self,
        _: &ToggleServices,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_services_panel(cx);
    }

    pub(super) fn new_script(
        &mut self,
        _: &NewScript,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_new_script_dialog(window, cx);
    }

    pub(super) fn toggle_right_panel_action(
        &mut self,
        _: &ToggleRightPanel,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_right_panel(cx);
    }
}
