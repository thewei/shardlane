//! Shortcuts settings section: list all registered shortcuts with scope, chord, and reset.
//!
//! [INPUT]: ShortcutRegistry (REGISTRY), ShortcutConfig (from ApplicationConfig)
//! [OUTPUT]: render_shortcuts_settings() returns an AnyElement for the Shortcuts settings page
//! [POS]: Settings UI sub-view; pure render, mutates config via save_config()

use crate::shortcuts::{self, ShortcutScope, REGISTRY};
use crate::ShardlaneApp;
use crepuscularity_gpui::prelude::*;
use crepuscularity_gpui::{
    div, px, AnyElement, Context, ElementId, IntoElement, MouseButton, Window,
};
use gpui_component::{h_flex, v_flex, ActiveTheme as _};

impl ShardlaneApp {
    /// SCT-04: remove a shortcut's override/disable, restore the default, and rebind immediately.
    pub(crate) fn reset_shortcut_override(&mut self, id: &str, cx: &mut Context<Self>) {
        self.config.shortcuts.overrides.remove(id);
        self.config.shortcuts.disabled.remove(id);
        self.save_config();
        self.rebind_shortcuts(cx);
        cx.notify();
    }

    /// SCT-04: toggle a shortcut's disabled state and rebind immediately.
    pub(crate) fn toggle_shortcut_disabled(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.config.shortcuts.disabled.remove(id) {
            self.config.shortcuts.disabled.insert(id.to_string());
        }
        self.save_config();
        self.rebind_shortcuts(cx);
        cx.notify();
    }

    /// UX: enter "key recording". First suppress all currently effective chords with
    /// trailing NoAction bindings so recording cannot accidentally trigger real actions;
    /// the observer-side capture branch then takes over.
    pub(crate) fn begin_shortcut_recording(
        &mut self,
        id: shortcuts::ShortcutId,
        cx: &mut Context<Self>,
    ) {
        self.shortcut_recording = Some(id);
        let masks = shortcuts::resolved_bindings(&self.config.shortcuts)
            .into_iter()
            .map(|resolved| {
                gpui::KeyBinding::new(
                    resolved.chord.as_str(),
                    gpui::NoAction,
                    resolved.scope.gpui_context(),
                )
            });
        cx.bind_keys(masks);
        cx.notify();
    }

    pub(crate) fn finish_shortcut_recording(
        &mut self,
        key: &crepuscularity_gpui::Keystroke,
        id: shortcuts::ShortcutId,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.shortcut_recording = None;
        let bare = !(key.modifiers.control
            || key.modifiers.alt
            || key.modifiers.platform
            || key.modifiers.function
            || key.modifiers.shift);
        if key.key == "escape" && bare {
            // Cancel: don't write an override, just restore the bindings.
            self.rebind_shortcuts(cx);
            cx.notify();
            return;
        }
        if matches!(
            key.key.as_str(),
            "shift" | "control" | "alt" | "platform" | "function"
        ) {
            // Pressing a modifier alone doesn't count as done — stay in recording state waiting for the main key.
            self.shortcut_recording = Some(id);
            return;
        }
        let mut chord = crate::input::keystroke_chord(key).to_lowercase();
        if key.modifiers.shift && !chord.contains("shift") {
            chord = format!("shift-{chord}");
        }
        self.config
            .shortcuts
            .overrides
            .insert(id.to_string(), vec![chord]);
        self.config.shortcuts.disabled.remove(id);
        self.save_config();
        self.rebind_shortcuts(cx);
        cx.notify();
    }

    /// SCT-01 + P1-3 (audit 2026-08-27): runtime rebinding.
    ///
    /// The GPUI keymap is append-only (`add_bindings`, later entries win); `bind_keys`
    /// never replaces a previous generation's bindings. Shadowing just the default chord
    /// is not enough — a previous generation's override chords also linger. This
    /// implementation records each generation's complete dynamic set (active chords +
    /// masks); when rebinding, it appends NoAction tombstones for every (chord, scope)
    /// that "this generation no longer uses but a historical generation installed", so
    /// in the override chain X→J→K, J truly stops firing; the stale mask set converges
    /// across further generations.
    pub(crate) fn rebind_shortcuts(&mut self, cx: &mut Context<Self>) {
        let (bindings, mut installed_pairs) = crate::dyn_shortcut_install(&self.config);
        let previous = std::mem::take(&mut self.dyn_keymap_generation);
        let stale: Vec<(String, shortcuts::ShortcutScope)> = previous
            .into_iter()
            .filter(|pair| !installed_pairs.contains(pair))
            .collect();
        if !stale.is_empty() {
            crate::lag_log(format_args!(
                "shortcuts.rebind tombstoning {} retired chord(s)",
                stale.len()
            ));
            cx.bind_keys(stale.iter().map(|(chord, scope)| {
                gpui::KeyBinding::new(chord.as_str(), gpui::NoAction, scope.gpui_context())
            }));
        }
        // This generation's pair set includes the newly installed tombstones: the set converges monotonically across rebinding generations.
        installed_pairs.extend(stale);
        self.dyn_keymap_generation = installed_pairs;
        cx.bind_keys(bindings);
    }

    pub(crate) fn render_shortcuts_settings(
        &self,
        _window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let foreground = theme.foreground;
        let border = theme.border;
        let surface = theme.background;
        let config = &self.config.shortcuts;

        let conflicts = shortcuts::detect_conflicts(config);

        let mut rows: Vec<AnyElement> = Vec::with_capacity(REGISTRY.len());

        for entry in REGISTRY.iter() {
            let effective = shortcuts::effective_chord(entry.id, config);
            let display = effective
                .map(shortcuts::format_chord_display)
                .unwrap_or_else(|| "—".to_string());
            let is_disabled = config.disabled.contains(entry.id);
            let is_overridden = config.overrides.contains_key(entry.id);
            let has_conflict = conflicts.iter().any(|c| c.ids.contains(&entry.id));

            let recording_this = self.shortcut_recording == Some(entry.id);
            // Audit E10: name what the chord conflicts with — detect_conflicts already
            // computed the colliding ids, so the tooltip says "Conflicts with …" instead
            // of only coloring the badge.
            let conflict_note = conflicts
                .iter()
                .filter(|conflict| conflict.ids.contains(&entry.id))
                .flat_map(|conflict| conflict.ids.iter().copied())
                .filter(|other| *other != entry.id)
                .map(|other| {
                    let label = shortcuts::find_entry(other)
                        .map(|other_entry| other_entry.label)
                        .unwrap_or(other);
                    let chord = shortcuts::effective_chord(other, config)
                        .map(shortcuts::format_chord_display)
                        .unwrap_or_else(|| "—".to_string());
                    format!("{label} ({chord})")
                })
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let tooltip = if conflict_note.is_empty() {
                "Click to rebind · Esc cancels".to_string()
            } else {
                format!(
                    "Click to rebind · Esc cancels · Conflicts with {}",
                    conflict_note.join(", ")
                )
            };
            let scope_label = match entry.scope {
                ShortcutScope::Global => "Global",
                ShortcutScope::App => "App",
                ShortcutScope::AgentSwitcher => "Agent Switcher",
            };

            let row = h_flex()
                .w_full()
                .px(px(16.0))
                .py(px(8.0))
                .gap(px(12.0))
                .items_center()
                .border_b_1()
                .border_color(border.opacity(0.5))
                .child(
                    div()
                        .w(px(200.0))
                        .truncate()
                        .text_size(crate::theme::FONT_BODY)
                        .text_color(if is_disabled { muted } else { foreground })
                        .child(entry.label),
                )
                .child(
                    div()
                        .w(px(60.0))
                        .text_size(crate::theme::FONT_META)
                        .text_color(muted)
                        .child(scope_label),
                )
                // UX: the badge itself is the rebind entry — a recording row shows a guided placeholder.
                .child(
                    div().flex_1().child(
                        div()
                            .id(ElementId::Name(
                                format!("shortcut-rebind-{}", entry.id).into(),
                            ))
                            .flex()
                            .flex_none()
                            .px(px(8.0))
                            .py(px(3.0))
                            .rounded(px(6.0))
                            .border_1()
                            .border_color(if recording_this {
                                theme.primary.opacity(0.9)
                            } else if has_conflict {
                                theme.danger.opacity(0.8)
                            } else {
                                border.opacity(0.6)
                            })
                            .bg(theme
                                .primary
                                .opacity(if recording_this { 0.14 } else { 0.05 }))
                            .text_size(crate::theme::FONT_BODY)
                            .text_color(if recording_this {
                                theme.primary
                            } else if is_disabled {
                                muted
                            } else {
                                foreground
                            })
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.primary.opacity(0.12)))
                            .tooltip(crate::ui::tooltip::tooltip_fn(tooltip))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _ev, window, cx| {
                                    this.begin_shortcut_recording(entry.id, cx);
                                    let _ = window;
                                }),
                            )
                            .child(if recording_this {
                                "Press keys… (Esc cancels)".to_string()
                            } else if is_disabled {
                                format!("{display} · disabled")
                            } else {
                                display
                            }),
                    ),
                )
                .when(is_overridden || is_disabled, |row| {
                    // SCT-04: Reset is a real action (remove override/disable + save + rebind).
                    row.child(
                        div()
                            .id(ElementId::Name(
                                format!("shortcut-reset-{}", entry.id).into(),
                            ))
                            .px(px(8.0))
                            .py(px(2.0))
                            .rounded(px(6.0))
                            .text_size(crate::theme::FONT_META)
                            .text_color(theme.primary.opacity(0.85))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.primary.opacity(0.15)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _ev, _win, cx| {
                                    this.reset_shortcut_override(entry.id, cx);
                                }),
                            )
                            .child("Reset"),
                    )
                })
                .when(!is_disabled, |row| {
                    // SCT-04: Disable toggle (once disabled, clicking again enables).
                    row.child(
                        div()
                            .id(ElementId::Name(
                                format!("shortcut-disable-{}", entry.id).into(),
                            ))
                            .px(px(8.0))
                            .py(px(2.0))
                            .rounded(px(6.0))
                            .text_size(crate::theme::FONT_META)
                            .text_color(theme.primary.opacity(0.85))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.primary.opacity(0.15)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _ev, _win, cx| {
                                    this.toggle_shortcut_disabled(entry.id, cx);
                                }),
                            )
                            .child("Disable"),
                    )
                })
                .into_any_element();

            rows.push(row);
        }

        let header = h_flex()
            .w_full()
            .px(px(16.0))
            .py(px(6.0))
            .gap(px(12.0))
            .items_center()
            .border_b_1()
            .border_color(border)
            .child(
                div()
                    .w(px(200.0))
                    .text_size(crate::theme::FONT_META)
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(muted)
                    .child("Command"),
            )
            .child(
                div()
                    .w(px(60.0))
                    .text_size(crate::theme::FONT_META)
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(muted)
                    .child("Scope"),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(crate::theme::FONT_META)
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(muted)
                    .child("Shortcut"),
            );

        v_flex()
            .w_full()
            // Same rhythm as settings_card's mt(15): title-to-content spacing aligned with the
            // other Settings pages (notate 08-29 second round).
            .mt(px(15.0))
            .rounded_lg()
            .border_1()
            .border_color(border)
            .bg(surface)
            .overflow_hidden()
            .child(header)
            .children(rows)
            .into_any_element()
    }
}
