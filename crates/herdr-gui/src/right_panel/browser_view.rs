//! [INPUT]: The import surface and types of the right_panel module root (`use super::*`).
//! [OUTPUT]: render_right_panel_browser_view / ensure_browser_address / navigate_browser
//! [POS]: The browser_view responsibility slice of the right_panel directory.
use super::*;
use crate::browser_profile::BrowserSessionId;

/// BROWSER-02: URL → session (default profile). Compatible with legacy URL
/// addressing.
fn session_for_url(url: &str) -> BrowserSessionId {
    BrowserSessionId::default_session(url)
}

/// BROWSER-02: build the session from the configured default profile — the
/// profile id participates in the isolation key (after switching profiles the
/// new key differs and the old profile's webview is never reused).
impl ShardlaneApp {
    pub(super) fn ensure_browser_address(
        &mut self,
        url: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(input) = self.browser_addresses.get(url) {
            return input.clone();
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search or enter address"));
        let submit_herdr = cx.entity();
        let from_url = url.to_string();
        let input_for_submit = input.clone();
        let subscription = cx.subscribe_in(&input, window, move |_, _, event, window, cx| {
            if let InputEvent::PressEnter { .. } = event {
                let text = input_for_submit.read(cx).value().to_string();
                let submit_herdr = submit_herdr.clone();
                let from_url = from_url.clone();
                window.defer(cx, move |window, cx| {
                    submit_herdr.update(cx, |this, cx| {
                        this.navigate_browser(&from_url, &text, window, cx);
                    });
                });
            }
        });
        self.browser_address_subscriptions
            .insert(url.to_string(), subscription);
        self.browser_addresses
            .insert(url.to_string(), input.clone());
        input
    }

    pub(super) fn navigate_browser(
        &mut self,
        from_url: &str,
        raw: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = resolve_address(raw) else {
            return;
        };
        let new_url = match target {
            AddressTarget::Url(url) => url,
            AddressTarget::Search(query) => search_url(&query),
        };
        // P1-6: the surface-frozen profile is the identity; navigation only
        // changes the URL.
        let active_browser = self
            .right_panel
            .active_surface
            .and_then(|i| self.right_panel.surfaces.get(i))
            .and_then(|s| match s {
                RightPanelSurface::Browser { url, profile_id } if url == from_url => {
                    Some(profile_id.clone())
                }
                _ => None,
            });
        let frozen_profile_id = active_browser
            .clone()
            .unwrap_or_else(|| self.config.browser.default_profile().id.clone());
        if let (Some(index), Some(profile_id)) =
            (self.right_panel.active_surface, active_browser.as_ref())
        {
            self.right_panel.surfaces[index] = RightPanelSurface::Browser {
                url: new_url.clone(),
                profile_id: profile_id.clone(),
            };
        }
        let profile = self
            .config
            .browser
            .profiles
            .iter()
            .find(|candidate| candidate.id == frozen_profile_id)
            .cloned()
            .unwrap_or_else(|| self.config.browser.default_profile().clone());
        let new_session =
            crate::browser_profile::BrowserSessionId::new(profile.id.as_str(), &new_url);
        if let std::collections::hash_map::Entry::Vacant(e) =
            self.browser_webviews.entry(new_session.clone())
        {
            // BROWSER-02: the configured default profile takes effect (ephemeral/
            // UUID-isolated store).
            if let Some(webview) = webview::BrowserWebview::ensure_with_config(
                &new_url,
                profile.ephemeral,
                Some(&profile.id),
                window,
            ) {
                e.insert(webview);
            }
        }
        // BROWSER-05 + P1-6: after swapping the URL in place, retire the old
        // session's WKWebView; the old key is computed from the frozen profile,
        // and default/legacy keys are kept as fallback retirements.
        if from_url != new_url {
            let old_key =
                crate::browser_profile::BrowserSessionId::new(&frozen_profile_id, from_url);
            if let Some(webview) = self.browser_webviews.remove(&old_key) {
                webview.remove();
            }
            let default_key = crate::browser_profile::BrowserSessionId::new(
                self.config.browser.default_profile().id,
                from_url,
            );
            if default_key != old_key {
                if let Some(webview) = self.browser_webviews.remove(&default_key) {
                    webview.remove();
                }
            }
            let legacy_key = session_for_url(from_url);
            if legacy_key != old_key && legacy_key != default_key {
                if let Some(webview) = self.browser_webviews.remove(&legacy_key) {
                    webview.remove();
                }
            }
        }
        if let Some(webview) = self.browser_webviews.get_mut(&new_session) {
            webview.load(&new_url);
        }
        if let Some(input) = self.browser_addresses.remove(from_url) {
            input.update(cx, |state, cx| {
                state.set_value(new_url.clone(), window, cx);
            });
            self.browser_addresses.insert(new_url.clone(), input);
        }
        if from_url != new_url {
            self.browser_load_failure = None;
        }
        self.right_panel.address_synced_url = Some(new_url);
        cx.notify();
    }

    pub(super) fn render_right_panel_browser_view(
        &mut self,
        url: &str,
        profile_id: &str,
        theme: ContentSurfaceTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let herdr = cx.entity();
        let target_url = if url.is_empty() {
            crate::right_panel::BROWSER_DEFAULT_URL.to_string()
        } else {
            url.to_string()
        };
        let ext_url = target_url.clone();

        // P1-6: the render key uses the surface-frozen profile (never reads back
        // the default).
        let profile = self
            .config
            .browser
            .profiles
            .iter()
            .find(|candidate| candidate.id == profile_id)
            .cloned()
            .unwrap_or_else(|| self.config.browser.default_profile().clone());
        let active_session =
            crate::browser_profile::BrowserSessionId::new(profile.id.as_str(), &target_url);
        // UX (walkthrough #12): take this session's navigation-failure event and
        // show a GPUI error card instead of a white screen.
        if let Some(webview) = self.browser_webviews.get(&active_session) {
            if let Some(failed) = webview.take_failure() {
                self.browser_load_failure = Some(failed);
            }
        }
        let show_failure = self
            .browser_load_failure
            .clone()
            .filter(|failed| *failed == target_url)
            .map(|f| (f, herdr.clone(), active_session.clone()));
        if let std::collections::hash_map::Entry::Vacant(e) =
            self.browser_webviews.entry(active_session.clone())
        {
            // BROWSER-02: the render path likewise builds the webview from the
            // configured profile.
            if let Some(webview) = webview::BrowserWebview::ensure_with_config(
                &target_url,
                profile.ephemeral,
                Some(&profile.id),
                window,
            ) {
                e.insert(webview);
            }
        }
        if let Some(webview) = self.browser_webviews.get_mut(&active_session) {
            webview.load(&target_url);
        }
        for (key, webview) in &self.browser_webviews {
            if *key != active_session {
                webview.set_visible(false);
            }
        }

        let address_input = self.ensure_browser_address(&target_url, window, cx);
        let address_synced = self
            .right_panel
            .address_synced_url
            .as_deref()
            .is_some_and(|synced| synced == target_url);
        if !address_synced {
            address_input.update(cx, |state, cx| {
                state.set_value(target_url.clone(), window, cx);
            });
            self.right_panel.address_synced_url = Some(target_url.clone());
        }

        let back_herdr = herdr.clone();
        let forward_herdr = herdr.clone();
        let reload_herdr = herdr.clone();
        let browser_key = target_url.clone();
        let forward_key = browser_key.clone();
        let reload_key = browser_key.clone();

        let toolbar = div()
            .h(px(36.0))
            .px(px(8.0))
            .border_b_1()
            .border_color(theme.border)
            .flex()
            .items_center()
            .gap(SPACE_ICON)
            .child(
                div()
                    .id("browser-back")
                    .size(px(22.0))
                    .rounded(px(4.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|e| e.bg(theme.foreground.opacity(crate::theme::WASH_HOVER)))
                    .child(
                        Icon::empty()
                            .path("icons/arrow-left.svg")
                            .with_size(px(12.0))
                            .text_color(theme.muted),
                    )
                    .on_mouse_down(MouseButton::Left, move |_, _, app| {
                        app.stop_propagation();
                        back_herdr.update(app, |this, _| {
                            if let Some(webview) =
                                this.browser_webviews.get(&session_for_url(&browser_key))
                            {
                                webview.go_back();
                            }
                        });
                    }),
            )
            .child(
                div()
                    .id("browser-forward")
                    .size(px(22.0))
                    .rounded(px(4.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|e| e.bg(theme.foreground.opacity(crate::theme::WASH_HOVER)))
                    .child(
                        Icon::empty()
                            .path("icons/arrow-right.svg")
                            .with_size(px(12.0))
                            .text_color(theme.muted),
                    )
                    .on_mouse_down(MouseButton::Left, move |_, _, app| {
                        app.stop_propagation();
                        forward_herdr.update(app, |this, _| {
                            if let Some(webview) =
                                this.browser_webviews.get(&session_for_url(&forward_key))
                            {
                                webview.go_forward();
                            }
                        });
                    }),
            )
            .child(
                div()
                    .id("browser-reload")
                    .size(px(22.0))
                    .rounded(px(4.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|e| e.bg(theme.foreground.opacity(crate::theme::WASH_HOVER)))
                    .child(
                        Icon::empty()
                            .path("icons/rotate-cw.svg")
                            .with_size(px(11.0))
                            .text_color(theme.muted),
                    )
                    .on_mouse_down(MouseButton::Left, move |_, _, app| {
                        app.stop_propagation();
                        reload_herdr.update(app, |this, _| {
                            if let Some(webview) =
                                this.browser_webviews.get(&session_for_url(&reload_key))
                            {
                                webview.reload();
                            }
                        });
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .h(px(24.0))
                    .px(px(8.0))
                    .rounded(px(5.0))
                    .bg(theme.foreground.opacity(0.04))
                    .border_1()
                    .border_color(theme.border)
                    .flex()
                    .items_center()
                    .gap(SPACE_ICON)
                    .child(
                        Icon::empty()
                            .path(if is_secure_url(&target_url) {
                                "icons/lock.svg"
                            } else {
                                "icons/globe.svg"
                            })
                            .with_size(px(11.0))
                            .text_color(theme.muted),
                    )
                    .child(
                        div().flex_1().min_w_0().flex().items_center().child(
                            Input::new(&address_input)
                                .small()
                                .appearance(false)
                                .p_0()
                                .text_size(crate::theme::FONT_META),
                        ),
                    ),
            )
            .child(
                div()
                    .size(px(22.0))
                    .rounded(px(4.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|e| e.bg(theme.foreground.opacity(crate::theme::WASH_HOVER)))
                    .child(
                        Icon::empty()
                            .path("icons/external-link.svg")
                            .with_size(px(12.0))
                            .text_color(theme.muted),
                    )
                    .on_mouse_down(MouseButton::Left, move |_, _, _| {
                        let _ = std::process::Command::new("open").arg(&ext_url).spawn();
                    }),
            );

        let sync_url = target_url.clone();
        let webview_body = div()
            .id("browser-webview-viewport")
            .flex_1()
            .min_h_0()
            .relative()
            .child(
                canvas(
                    move |bounds, window, app| {
                        herdr.update(app, |this, _| {
                            if let Some(webview) =
                                this.browser_webviews.get(&session_for_url(&sync_url))
                            {
                                webview.sync_frame(bounds, true, window);
                            }
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .when_some(show_failure, |body, (failed_url, failure_herdr, failure_session)| {
                body.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(rgb(0x1a1a1e))
                        .child(
                            v_flex()
                                .max_w(px(420.0))
                                .px(px(28.0))
                                .py(px(22.0))
                                .rounded(px(12.0))
                                .border_1()
                                .border_color(theme.border)
                                .bg(rgb(0x222228))
                                .items_center()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .text_size(crate::theme::FONT_SECTION_TITLE)
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.foreground)
                                        .child("Can't reach this page"),
                                )
                                .child(
                                    div()
                                        .text_size(crate::theme::FONT_BODY)
                                        .line_height(px(18.0))
                                        .text_color(theme.muted)
                                        .text_center()
                                        .child(
                                            "The local server may not be running, or the address is unreachable.",
                                        ),
                                )
                                .child(
                                    div()
                                        .max_w_full()
                                        .truncate()
                                        .text_size(crate::theme::FONT_META)
                                        .text_color(theme.muted)
                                        .child(failed_url),
                                )
                                .child(
                                    div()
                                        .id("browser-failure-reload")
                                        .mt(px(6.0))
                                        .px(px(14.0))
                                        .py(px(6.0))
                                        .rounded(px(8.0))
                                        .border_1()
                                        .border_color(theme.primary.opacity(0.5))
                                        .text_size(crate::theme::FONT_BODY)
                                        .text_color(theme.primary)
                                        .cursor_pointer()
                                        .hover(|h2| h2.bg(theme.primary.opacity(0.15)))
                                        .on_mouse_down(MouseButton::Left, move |_, _, app| {
                                            failure_herdr.update(app, |this, cx| {
                                                this.browser_load_failure = None;
                                                if let Some(webview) =
                                                    this.browser_webviews.get(&failure_session)
                                                {
                                                    webview.reload();
                                                }
                                                cx.notify();
                                            });
                                        })
                                        .child("Reload"),
                                ),
                        ),
                )
            });

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .child(webview_body)
            .into_any_element()
    }
}
