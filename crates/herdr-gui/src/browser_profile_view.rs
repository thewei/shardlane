//! Browser Profile settings section: list, create, and manage browser profiles.
//!
//! [INPUT]: BrowserConfig (from ApplicationConfig)
//! [OUTPUT]: render_browser_settings() + profile management actions (BROWSER-03)
//! [POS]: Settings UI sub-view; mutates config via save_config() with immediate effect

use super::*;
use crate::browser_profile::BrowserProfileConfig;
use crate::settings_view::{settings_card, settings_card_row};
use crate::ui::controls::ControlSurface;
use crate::ui_metrics::SPACE_ICON;
use crepuscularity_gpui::{
    div, px, AnyElement, Context, ElementId, IntoElement, MouseButton, Window,
};
use gpui_component::h_flex;

impl ShardlaneApp {
    /// How long a destructive profile action stays armed before the confirmation lapses
    /// (audit E03 two-click arm, same interaction as the workspace dialog's
    /// "Delete Workspace…" → "Click again to delete").
    const BROWSER_ARM_WINDOW: Duration = Duration::from_secs(5);

    /// BROWSER-03 + audit E03: shared two-click arm for the profile row's destructive
    /// actions. The first click arms (the chip copy becomes "Click again to …"); the
    /// second click within the arm window executes. Arming another action or profile,
    /// another profile action, or letting the arm lapse disarms without side effects.
    fn browser_profile_confirmed(
        &mut self,
        action: crate::browser_profile::BrowserProfileConfirmAction,
        id: &str,
        cx: &mut Context<Self>,
    ) {
        if let Some((armed_action, armed_id, armed_at)) = &self.browser_confirm {
            if *armed_action == action
                && armed_id == id
                && armed_at.elapsed() <= Self::BROWSER_ARM_WINDOW
            {
                self.browser_confirm = None;
                match action {
                    crate::browser_profile::BrowserProfileConfirmAction::ClearData => {
                        self.browser_profile_clear_data(id, cx);
                    }
                    crate::browser_profile::BrowserProfileConfirmAction::Delete => {
                        self.browser_profile_delete(id, cx);
                    }
                }
                return;
            }
        }
        self.browser_confirm = Some((action, id.to_string(), std::time::Instant::now()));
        cx.notify();
    }

    /// BROWSER-03: create a new profile (UUID + a random available name).
    pub(crate) fn browser_profile_create(&mut self, cx: &mut Context<Self>) {
        self.browser_confirm = None;
        let uuid = objc2_foundation::NSUUID::UUID().UUIDString().to_string();
        let name = format!("Profile {}", self.config.browser.profiles.len() + 1);
        self.config.browser.profiles.push(BrowserProfileConfig {
            id: uuid,
            name,
            ephemeral: false,
        });
        self.save_config();
        cx.notify();
    }

    /// BROWSER-03: set the default profile (takes effect immediately for later browser surfaces).
    pub(crate) fn browser_profile_set_default(&mut self, id: &str, cx: &mut Context<Self>) {
        self.browser_confirm = None;
        self.config.browser.default_profile_id = Some(id.to_string());
        self.save_config();
        cx.notify();
    }

    /// BROWSER-03: delete a profile. Destructive (closes the profile's surfaces, evicts
    /// its WebView sessions, and rewrites config) — the UI invokes it only through the
    /// two-click arm in [`ShardlaneApp::browser_profile_confirmed`].
    pub(crate) fn browser_profile_delete(&mut self, id: &str, cx: &mut Context<Self>) {
        self.config.browser.profiles.retain(|p| p.id != id);
        if self.config.browser.default_profile_id.as_deref() == Some(id) {
            self.config.browser.default_profile_id = None;
        }
        // P1-7 (audit 2026-08-27): deleting a profile must also evict its runtime traces —
        // all Browser surfaces of that profile (including WKWebView/address book/subscriptions
        // via the unified close path) plus orphaned sessions in the pool no longer owned by any surface.
        let surface_indices: Vec<usize> = self
            .right_panel
            .surfaces
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                matches!(s, crate::right_panel::RightPanelSurface::Browser { profile_id, .. }
                    if profile_id == id)
            })
            .map(|(index, _)| index)
            .collect();
        for index in surface_indices.into_iter().rev() {
            self.close_right_panel_surface(index, cx);
        }
        self.evict_browser_sessions_for_profile(id);
        self.save_config();
        cx.notify();
    }

    /// P1-7: evict all pooled WKWebView sessions of this profile.
    pub(crate) fn evict_browser_sessions_for_profile(&mut self, id: &str) {
        let doomed: Vec<crate::browser_profile::BrowserSessionId> = self
            .browser_webviews
            .keys()
            .filter(|key| key.profile_id == id)
            .cloned()
            .collect();
        for key in doomed {
            if let Some(webview) = self.browser_webviews.remove(&key) {
                webview.remove();
            }
            // The session purpose is the URL: address book/subscriptions share the address key's lifetime.
            let url = key.purpose.clone();
            self.browser_addresses.remove(&url);
            self.browser_address_subscriptions.remove(&url);
        }
    }

    /// BROWSER-03: clear all website data of a profile (WKWebsiteDataStore removeData).
    pub(crate) fn browser_profile_clear_data(&mut self, id: &str, cx: &mut Context<Self>) {
        let ok = crate::right_panel::webview::clear_profile_data(Some(id));
        crate::lag_log(format_args!(
            "browser: clear data for profile {id} => {}",
            if ok { "ok" } else { "failed" }
        ));
        // P1-7: after clearing data, rebuild (evict) this profile's active sessions — the next
        // render recreates them under the same session keys with a clean WKWebsiteDataStore,
        // avoiding stale session state.
        self.evict_browser_sessions_for_profile(id);
        cx.notify();
    }

    pub(crate) fn render_browser_settings(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let content_theme = self.content_surface_theme(window);
        let surface = ControlSurface {
            foreground: content_theme.foreground,
            background: content_theme.background,
        };
        let muted = content_theme.muted;
        let foreground = content_theme.foreground;
        let border = content_theme.border;
        let danger = content_theme.danger;
        let accent = content_theme.primary;
        let content_button = content_theme.button_variant(cx);

        let browser_config = &self.config.browser;
        let profiles = &browser_config.profiles;

        let mut rows: Vec<AnyElement> = Vec::with_capacity(profiles.len() + 2);

        // notate 2026-08-29 G1: Enabled is a real toggle (it gates the right panel's Browser
        // entry) instead of a static badge with no control.
        rows.push(settings_card_row(
            "Built-in browser",
            if browser_config.enabled {
                "Enabled — available from the right panel and surface chooser."
            } else {
                "Disabled — browser surfaces stay hidden until re-enabled."
            },
            crate::ui::controls::Toggle::new("browser-enabled", surface)
                .checked(browser_config.enabled)
                .on_change({
                    let herdr = cx.entity();
                    move |checked, _, app| {
                        herdr.update(app, |this, cx| {
                            this.config.browser.enabled = checked;
                            this.save_config();
                            cx.notify();
                        });
                    }
                })
                .into_any_element(),
        ));

        // notate 2026-08-29 G3: the synthesized Default row has no actionable items and is
        // no longer rendered; Default always exists (the right panel silently uses it when
        // there are no profiles).
        for profile in profiles {
            let is_default = browser_config
                .default_profile_id
                .as_deref()
                .map_or(profile.id == "default", |d| d == profile.id);
            rows.push(self.render_profile_row(
                profile, is_default, foreground, muted, border, danger, accent, cx,
            ));
        }

        rows.push(settings_card_row(
            "New profile",
            "Create an isolated browser profile with its own website data store.",
            Button::new("browser-profile-new")
                .custom(content_button)
                .xsmall()
                .label("+ New Profile")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.browser_profile_create(cx);
                }))
                .into_any_element(),
        ));

        settings_card(surface, rows).into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_profile_row(
        &self,
        profile: &BrowserProfileConfig,
        is_default: bool,
        foreground: gpui::Hsla,
        muted: gpui::Hsla,
        border: gpui::Hsla,
        danger: gpui::Hsla,
        accent: gpui::Hsla,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let profile_id = profile.id.clone();
        let is_synthesized_default = profile.id == "default" && is_default;
        let detail = if profile.ephemeral {
            "Ephemeral (private browsing)"
        } else {
            "Persistent isolated data store"
        };
        let title = if is_default {
            format!("{} (default)", profile.name)
        } else {
            profile.name.clone()
        };

        let mut actions = h_flex().gap(SPACE_ICON).flex_wrap();

        if !is_synthesized_default {
            if !is_default {
                let id = profile_id.clone();
                actions = actions.child(
                    div()
                        .id(ElementId::Name(format!("bp-default-{}", profile_id).into()))
                        .px(px(8.0))
                        .py(px(3.0))
                        .rounded(px(6.0))
                        .text_size(crate::theme::FONT_META)
                        .text_color(accent)
                        .cursor_pointer()
                        .hover(|s| s.bg(accent.opacity(0.1)))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _ev, _win, cx| {
                                this.browser_profile_set_default(&id, cx);
                            }),
                        )
                        .child("Set Default"),
                );
            }
            // Audit E03: destructive chips are two-click armed — the first click arms the
            // action (armed copy + filled treatment, same as the workspace dialog); the
            // second click executes. An expired arm renders as idle and re-arms on click.
            let arm_matches = |action: crate::browser_profile::BrowserProfileConfirmAction| {
                self.browser_confirm
                    .as_ref()
                    .is_some_and(|(armed_action, armed_id, armed_at)| {
                        *armed_action == action
                            && *armed_id == profile_id
                            && armed_at.elapsed() <= Self::BROWSER_ARM_WINDOW
                    })
            };
            let clear_armed =
                arm_matches(crate::browser_profile::BrowserProfileConfirmAction::ClearData);
            let delete_armed =
                arm_matches(crate::browser_profile::BrowserProfileConfirmAction::Delete);
            let id = profile_id.clone();
            actions = actions.child(
                div()
                    .id(ElementId::Name(format!("bp-clear-{}", profile_id).into()))
                    .px(px(8.0))
                    .py(px(3.0))
                    .rounded(px(6.0))
                    .border_1()
                    .border_color(if clear_armed {
                        foreground.opacity(0.7)
                    } else {
                        border
                    })
                    .when(clear_armed, |chip| chip.bg(muted.opacity(0.2)))
                    .text_size(crate::theme::FONT_META)
                    .text_color(foreground.opacity(0.8))
                    .cursor_pointer()
                    .hover(|s| s.bg(muted.opacity(0.15)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev, _win, cx| {
                            this.browser_profile_confirmed(
                                crate::browser_profile::BrowserProfileConfirmAction::ClearData,
                                &id,
                                cx,
                            );
                        }),
                    )
                    .child(if clear_armed {
                        "Click again to clear"
                    } else {
                        "Clear Data"
                    }),
            );
            let id = profile_id.clone();
            actions = actions.child(
                div()
                    .id(ElementId::Name(format!("bp-delete-{}", profile_id).into()))
                    .px(px(8.0))
                    .py(px(3.0))
                    .rounded(px(6.0))
                    .border_1()
                    .border_color(if delete_armed {
                        danger
                    } else {
                        danger.opacity(0.4)
                    })
                    .when(delete_armed, |chip| chip.bg(danger.opacity(0.18)))
                    .text_size(crate::theme::FONT_META)
                    .text_color(danger)
                    .cursor_pointer()
                    .hover(|s| s.bg(danger.opacity(0.15)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev, _win, cx| {
                            this.browser_profile_confirmed(
                                crate::browser_profile::BrowserProfileConfirmAction::Delete,
                                &id,
                                cx,
                            );
                        }),
                    )
                    .child(if delete_armed {
                        "Click again to delete"
                    } else {
                        "Delete"
                    }),
            );
        }

        settings_card_row(&title, detail, actions.into_any_element())
    }
}
