//! [INPUT]: ShardlaneApp (crate root), gpui-component Popover/Input/Icon, theme tokens; device +
//! instance data read through `ShardlaneApp`'s pub(crate) surface.
//! [OUTPUT]: The workspace switcher panel (one device's workspaces: hover gears → workspace/device
//! settings, running dots, click = jump/rebind) and the Tab switcher panel (same interaction, rows
//! switch tabs) — both as Popover panels around any `Selectable` trigger; shared by the sidebar
//! footer chip and the content header breadcrumbs.
//! [POS]: `crates/herdr-gui`'s switcher presentation; consumed by sidebar/shell.rs (footer) and
//! header_view.rs (breadcrumbs).
use super::*;
use ::gpui::Corner;
use gpui_component::popover::Popover;
use gpui_component::Selectable;

/// One workspace-switcher device section: (device id, display name, is-local,
/// its instances). Each instance row is (jump key, label, running, bound).
pub(crate) type PickerMachine = (String, String, bool, Vec<(String, String, bool, bool)>);

/// Builds the switcher's device sections: the local machine first, then every
/// live SSH bridge's sessions. Labels come from the session metadata (raw
/// session name when there is none).
pub(crate) fn build_picker_machines(app: &ShardlaneApp) -> Vec<PickerMachine> {
    let bound_key = app
        .bound_project()
        .map(|binding| binding.project_id.clone());
    let local_machine_name = crate::remote_display_host_name();
    let bridged_devices: HashSet<String> = app
        .shared
        .ssh_bridges
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .map(|bridge| bridge.device_id.clone())
        .collect();
    let mut machines: Vec<PickerMachine> = Vec::new();
    let local_instances = app
        .shared
        .instance_list()
        .iter()
        .map(|instance| {
            let label = app.shared.display_name(&instance.name);
            let bound = bound_key.as_deref() == Some(instance.name.as_str());
            (instance.name.clone(), label, instance.running, bound)
        })
        .collect::<Vec<_>>();
    machines.push((
        "local".to_string(),
        local_machine_name,
        true,
        local_instances,
    ));
    for device in &app.config.devices {
        if device.ssh_target.is_none() || !bridged_devices.contains(&device.id) {
            continue; // disconnected machines are managed on the devices page
        }
        let instances = app
            .shared
            .remote_sessions_for(&device.id)
            .iter()
            .map(|session| {
                let key = format!("ssh:{}:{}", device.id, session.name);
                let label = app.shared.display_name(&key);
                let bound = bound_key.as_deref() == Some(key.as_str());
                (key, label, session.running, bound)
            })
            .collect::<Vec<_>>();
        machines.push((device.id.clone(), device.name.clone(), false, instances));
    }
    machines
}

/// The device whose workspaces the panel lists: the selected device, else the
/// bound one, else this Mac.
pub(crate) fn selected_panel_device(app: &ShardlaneApp) -> String {
    let bound_device_id = app
        .bound_project()
        .map(|binding| binding.project_id.clone())
        .and_then(|project_id| {
            project_id
                .strip_prefix("ssh:")
                .and_then(|rest| rest.split_once(':'))
                .map(|(device_id, _)| device_id.to_string())
        });
    app.panel_device
        .clone()
        .or(bound_device_id)
        .unwrap_or_else(|| "local".to_string())
}

/// Wraps any `Selectable` trigger (footer chip, header breadcrumb button) in
/// the workspace switcher panel: device header (hover gear → device settings)
/// over that device's workspaces (hover gear → workspace settings; click =
/// jump/rebind), with a front-end filter row at the bottom.
pub(crate) fn workspace_switcher_panel(
    herdr: Entity<ShardlaneApp>,
    machines: Vec<PickerMachine>,
    selected_device: String,
    anchor: Corner,
    popover_id: &'static str,
    filter_key: &'static str,
    mut trigger: impl Selectable + Styled + IntoElement + 'static,
) -> Popover {
    let trigger_style = trigger.style().clone();
    Popover::new(SharedString::from(popover_id))
        .anchor(anchor)
        .trigger(trigger)
        .trigger_style(trigger_style)
        .content(move |_, window, cx| {
            let popover = cx.entity();
            let picker_herdr = herdr.clone();
            let picker_theme = cx.theme().clone();
            // Front-end filter for the rows above. Lives in the popover's
            // keyed state (entities must not be created during ShardlaneApp's
            // own render pass); keystrokes repaint the tree, so the closure
            // reads the live value every frame.
            let filter_holder =
                window.use_keyed_state(SharedString::from(filter_key), cx, |window, cx| {
                    cx.new(|cx| {
                        InputState::new(window, cx).placeholder(crate::i18n::t("workspace.filter"))
                    })
                });
            let filter_input = filter_holder.read(cx).clone();
            // Autofocus the filter on open (idempotent): the panel is
            // keyboard-first — open and type.
            if !filter_input.read(cx).focus_handle(cx).is_focused(window) {
                filter_input.update(cx, |state, cx| {
                    state.focus(window, cx);
                });
            }
            let filter = filter_input.read(cx).value().trim().to_lowercase();
            let mut sections = v_flex().w_full().gap(px(2.0));
            for (device_id, machine_name, is_local, instances) in &machines {
                // The panel lists ONE device's workspaces.
                if *device_id != selected_device {
                    continue;
                }
                let matches: Vec<_> = instances
                    .iter()
                    .filter(|(_, label, _, _)| {
                        filter.is_empty() || label.to_lowercase().contains(&filter)
                    })
                    .collect();
                if !filter.is_empty() && matches.is_empty() {
                    continue;
                }
                let device_gear_herdr = picker_herdr.clone();
                sections = sections.child(
                    h_flex()
                        .group("ws-device-header")
                        .w_full()
                        .px(px(10.0))
                        .pt(px(6.0))
                        .pb(px(2.0))
                        .gap(px(6.0))
                        .items_center()
                        .child(
                            div()
                                .size(px(7.0))
                                .rounded_full()
                                .flex_shrink_0()
                                .bg(picker_theme.success),
                        )
                        .child(
                            div()
                                .text_size(px(13.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(picker_theme.foreground)
                                .min_w_0()
                                .truncate()
                                .child(SharedString::from(if *is_local {
                                    format!(
                                        "{} {}",
                                        machine_name,
                                        crate::i18n::t("workspace.local")
                                    )
                                } else {
                                    machine_name.clone()
                                })),
                        )
                        .child(
                            div()
                                .id("ws-device-settings-gear")
                                .size(px(18.0))
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(4.0))
                                .cursor_pointer()
                                .opacity(0.0)
                                .group_hover("ws-device-header", |s| s.opacity(1.0))
                                .hover(|s| {
                                    s.bg(picker_theme.foreground.opacity(crate::theme::WASH_HOVER))
                                })
                                .on_click(move |_, window, app| {
                                    app.stop_propagation();
                                    device_gear_herdr.update(app, |this, cx| {
                                        this.open_device_settings(window, cx)
                                    });
                                })
                                .child(
                                    Icon::empty()
                                        .path("icons/settings.svg")
                                        .with_size(px(12.0))
                                        .text_color(picker_theme.muted_foreground),
                                ),
                        ),
                );
                for (key, label, running, bound) in matches {
                    let key = key.clone();
                    let row_herdr = picker_herdr.clone();
                    let row_popover = popover.clone();
                    let row_gear_herdr = picker_herdr.clone();
                    let row_gear_popover = popover.clone();
                    let gear_key = key.clone();
                    sections = sections.child(
                        h_flex()
                            .group("ws-row")
                            .id(SharedString::from(format!("ws-picker-{key}")))
                            .w_full()
                            .h(px(30.0))
                            .px(px(10.0))
                            .rounded(px(6.0))
                            .gap(px(8.0))
                            .items_center()
                            .cursor_pointer()
                            .hover(|s| {
                                s.bg(picker_theme.foreground.opacity(crate::theme::WASH_HOVER))
                            })
                            .on_click(move |_, window, app| {
                                row_popover.update(app, |state, cx| state.dismiss(window, cx));
                                row_herdr.update(app, |this, cx| {
                                    this.open_or_jump_project(&key, window, cx)
                                });
                            })
                            .child(div().size(px(7.0)).rounded_full().flex_shrink_0().bg(
                                if *running {
                                    picker_theme.success
                                } else {
                                    picker_theme.muted_foreground.opacity(0.45)
                                },
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(13.0))
                                    .text_color(if *bound {
                                        picker_theme.foreground
                                    } else {
                                        picker_theme.muted_foreground
                                    })
                                    .child(label.clone()),
                            )
                            .child(if *bound {
                                Icon::empty()
                                    .path("icons/check.svg")
                                    .with_size(px(12.0))
                                    .text_color(picker_theme.success)
                                    .flex_shrink_0()
                                    .into_any_element()
                            } else {
                                div().into_any_element()
                            })
                            .child(
                                div()
                                    .id(SharedString::from(format!("ws-row-gear-{gear_key}")))
                                    .size(px(18.0))
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(4.0))
                                    .cursor_pointer()
                                    .opacity(0.0)
                                    .group_hover("ws-row", |s| s.opacity(1.0))
                                    .hover(|s| {
                                        s.bg(picker_theme
                                            .foreground
                                            .opacity(crate::theme::WASH_HOVER))
                                    })
                                    .on_click(move |_, window, app| {
                                        app.stop_propagation();
                                        row_gear_popover
                                            .update(app, |state, cx| state.dismiss(window, cx));
                                        row_gear_herdr.update(app, |this, cx| {
                                            this.open_workspace_settings(
                                                gear_key.clone(),
                                                window,
                                                cx,
                                            )
                                        });
                                    })
                                    .child(
                                        Icon::empty()
                                            .path("icons/settings.svg")
                                            .with_size(px(12.0))
                                            .text_color(picker_theme.muted_foreground),
                                    ),
                            ),
                    );
                }
            }
            let filter_element = Input::new(&filter_input)
                .small()
                .appearance(false)
                .w_full()
                .text_size(px(12.0));
            v_flex()
                .w(px(320.0))
                .py(px(4.0))
                .child(
                    div()
                        .id(SharedString::from(format!("{popover_id}-scroll")))
                        .w_full()
                        .max_h(px(420.0))
                        .overflow_y_scroll()
                        .child(sections),
                )
                .child(
                    h_flex()
                        .w_full()
                        .h(px(30.0))
                        .mt(px(4.0))
                        .mx(px(6.0))
                        .px(px(6.0))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(picker_theme.border)
                        .gap(px(6.0))
                        .items_center()
                        .child(
                            Icon::empty()
                                .path("icons/list-filter.svg")
                                .with_size(px(12.0))
                                .text_color(picker_theme.muted_foreground)
                                .flex_shrink_0(),
                        )
                        .child(filter_element),
                )
        })
}

/// One row of the Tab switcher panel: (tab id, title, focused).
pub(crate) type TabRow = (String, String, bool);

/// The Tab switcher panel — the same interaction and styling as the workspace
/// switcher, listing the focused workspace's Tabs; click focuses the Tab.
pub(crate) fn tab_switcher_panel(
    herdr: Entity<ShardlaneApp>,
    tabs: Vec<TabRow>,
    anchor: Corner,
    popover_id: &'static str,
    filter_key: &'static str,
    mut trigger: impl Selectable + Styled + IntoElement + 'static,
) -> Popover {
    let trigger_style = trigger.style().clone();
    Popover::new(SharedString::from(popover_id))
        .anchor(anchor)
        .trigger(trigger)
        .trigger_style(trigger_style)
        .content(move |_, window, cx| {
            let popover = cx.entity();
            let picker_herdr = herdr.clone();
            let picker_theme = cx.theme().clone();
            let filter_holder =
                window.use_keyed_state(SharedString::from(filter_key), cx, |window, cx| {
                    cx.new(|cx| {
                        InputState::new(window, cx).placeholder(crate::i18n::t("tab.filter"))
                    })
                });
            let filter_input = filter_holder.read(cx).clone();
            if !filter_input.read(cx).focus_handle(cx).is_focused(window) {
                filter_input.update(cx, |state, cx| {
                    state.focus(window, cx);
                });
            }
            let filter = filter_input.read(cx).value().trim().to_lowercase();
            let mut rows = v_flex().w_full().gap(px(2.0));
            let mut shown = 0usize;
            for (tab_id, label, focused) in &tabs {
                shown += 1;
                if !filter.is_empty() && !label.to_lowercase().contains(&filter) {
                    continue;
                }
                let row_herdr = picker_herdr.clone();
                let row_popover = popover.clone();
                let tab_id = tab_id.clone();
                rows =
                    rows.child(
                        h_flex()
                            .group("tab-row")
                            .id(SharedString::from(format!("tab-picker-{tab_id}")))
                            .w_full()
                            .h(px(30.0))
                            .px(px(10.0))
                            .rounded(px(6.0))
                            .gap(px(8.0))
                            .items_center()
                            .cursor_pointer()
                            .hover(|s| {
                                s.bg(picker_theme.foreground.opacity(crate::theme::WASH_HOVER))
                            })
                            .on_click(move |_, window, app| {
                                row_popover.update(app, |state, cx| state.dismiss(window, cx));
                                row_herdr.update(app, |this, cx| {
                                    this.focus_tab_id(tab_id.clone(), window, cx)
                                });
                            })
                            .child(div().size(px(7.0)).rounded_full().flex_shrink_0().bg(
                                if *focused {
                                    picker_theme.success
                                } else {
                                    picker_theme.muted_foreground.opacity(0.45)
                                },
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(13.0))
                                    .text_color(if *focused {
                                        picker_theme.foreground
                                    } else {
                                        picker_theme.muted_foreground
                                    })
                                    .child(label.clone()),
                            )
                            .child(if *focused {
                                Icon::empty()
                                    .path("icons/check.svg")
                                    .with_size(px(12.0))
                                    .text_color(picker_theme.success)
                                    .flex_shrink_0()
                                    .into_any_element()
                            } else {
                                div().into_any_element()
                            }),
                    );
            }
            if shown == 0 {
                rows = rows.child(
                    div()
                        .w_full()
                        .px(px(10.0))
                        .py(px(8.0))
                        .text_size(px(12.0))
                        .text_color(picker_theme.muted_foreground)
                        .child(crate::i18n::t("tab.none")),
                );
            }
            let filter_element = Input::new(&filter_input)
                .small()
                .appearance(false)
                .w_full()
                .text_size(px(12.0));
            v_flex()
                .w(px(320.0))
                .py(px(4.0))
                .child(
                    div()
                        .id(SharedString::from(format!("{popover_id}-scroll")))
                        .w_full()
                        .max_h(px(420.0))
                        .overflow_y_scroll()
                        .child(rows),
                )
                .child(
                    h_flex()
                        .w_full()
                        .h(px(30.0))
                        .mt(px(4.0))
                        .mx(px(6.0))
                        .px(px(6.0))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(picker_theme.border)
                        .gap(px(6.0))
                        .items_center()
                        .child(
                            Icon::empty()
                                .path("icons/list-filter.svg")
                                .with_size(px(12.0))
                                .text_color(picker_theme.muted_foreground)
                                .flex_shrink_0(),
                        )
                        .child(filter_element),
                )
        })
}
