//! Mobile pairing surface: pairing onboarding (QR/manual) + online client list + credential management.
//!
//! [INPUT]: Depends on `super` (main.rs)'s ShardlaneApp state and remote handle,
//! settings_view's settings_card/settings_card_row card vocabulary, the qrcode crate
//! (matrix generation), and gpui-component Input/Toggle/Button controls
//! [OUTPUT]: Exposes `ShardlaneApp::mobile_settings_content` (the content column of the mobile-control secondary surface),
//! `ensure_mobile_port_input`/`commit_mobile_port` (lazy port input creation and commit;
//! committing calls apply_remote_settings to restart the listener)
//! [POS]: One of main.rs's presentation-layer splits (sibling of settings_view.rs);
//! connection/token facts belong to shardlane-remote (RemoteServerHandle); this file only does presentation and action entry points

use super::*;
use crate::settings_view::{settings_card, settings_card_row};
use crate::ui::controls::{ControlSurface, Segmented, Toggle};

/// QR quiet zone (in modules).
const QR_QUIET_ZONE: usize = 2;
/// QR rendered side length (logical pixels).
const QR_RENDER_SIZE: f32 = 176.0;

impl ShardlaneApp {
    pub(super) fn mobile_settings_content(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let herdr = cx.entity();
        let content_theme = self.content_surface_theme(window);
        let foreground = content_theme.foreground;
        let background = content_theme.background;
        let surface = ControlSurface {
            foreground,
            background,
        };
        let remote = &self.config.remote;
        let remote_enabled = remote.enabled
            && matches!(
                remote.listener_mode,
                shardlane_remote::ListenerMode::Loopback
                    | shardlane_remote::ListenerMode::LocalNetwork
            );
        let remote_running = self
            .shared
            .remote_server
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some();
        let remote_port = remote.port;
        let token = remote.access_token.clone().unwrap_or_default();

        // ---- Card 1: Remote access status + Listener ----
        let toggle_herdr = herdr.clone();
        let remote_toggle = Toggle::new("mobile-remote-enabled", surface)
            .checked(remote_enabled)
            .on_change(move |checked, _, app| {
                toggle_herdr.update(app, |this, cx| {
                    this.config.remote.enabled = checked;
                    if !checked {
                        this.config.remote.listener_mode = shardlane_remote::ListenerMode::Off;
                    } else if !matches!(
                        this.config.remote.listener_mode,
                        shardlane_remote::ListenerMode::Loopback
                            | shardlane_remote::ListenerMode::LocalNetwork
                    ) {
                        this.config.remote.listener_mode =
                            shardlane_remote::ListenerMode::LocalNetwork;
                    }
                    this.apply_remote_settings(cx);
                    this.save_config();
                    cx.notify();
                });
            })
            .into_any_element();

        // Audit D02: Loopback is honored end-to-end, so the listener scope is an explicit
        // three-way choice instead of an implied LocalNetwork behind the master toggle.
        let listener_scope_herdr = herdr.clone();
        let listener_scope_control = Segmented::new("mobile-listener-scope", surface)
            .option(shardlane_remote::ListenerMode::Off, "Off")
            .option(shardlane_remote::ListenerMode::Loopback, "Loopback")
            .option(
                shardlane_remote::ListenerMode::LocalNetwork,
                "All interfaces",
            )
            .value(remote.listener_mode)
            .on_change(move |mode, _, app| {
                listener_scope_herdr.update(app, |this, cx| {
                    this.config.remote.listener_mode = *mode;
                    this.config.remote.enabled =
                        !matches!(*mode, shardlane_remote::ListenerMode::Off);
                    this.apply_remote_settings(cx);
                    this.save_config();
                    cx.notify();
                });
            })
            .into_any_element();

        let is_lan_mode = remote.listener_mode == shardlane_remote::ListenerMode::LocalNetwork;
        let listener_detail = if remote_enabled {
            if remote_running {
                if is_lan_mode {
                    format!(
                        "Listening on all interfaces (0.0.0.0:{remote_port}). \
                         Mobile devices on the same network can connect."
                    )
                } else {
                    format!("Listening at 127.0.0.1:{remote_port}. Mobile clients can connect.")
                }
            } else {
                format!("Enabled, but the listener failed to start (port {remote_port} busy?).")
            }
        } else {
            "Off — no listener thread, no idle cost. Enable to accept mobile connections."
                .to_string()
        };
        let all_addrs: Vec<String> = if is_lan_mode {
            let mut addrs = Vec::new();
            if let Ok(interfaces) = local_ip_address::list_afinet_netifas() {
                for (_name, ip) in &interfaces {
                    if ip.is_ipv4() && !ip.is_loopback() {
                        addrs.push(format!("{}:{}", ip, remote_port));
                    }
                }
            }
            if addrs.is_empty() {
                addrs.push(format!("127.0.0.1:{remote_port}"));
            }
            addrs
        } else {
            vec![format!("127.0.0.1:{remote_port}")]
        };
        let connect_addr = all_addrs[0].clone();

        let listener_status = div()
            .flex()
            .items_center()
            .gap(SPACE_ICON)
            .child(
                Icon::new(if remote_running {
                    ComponentIconName::CircleCheck
                } else {
                    ComponentIconName::CircleX
                })
                .with_size(px(14.0))
                .text_color(if remote_running {
                    content_theme.primary
                } else {
                    content_theme.muted
                }),
            )
            .child(
                div()
                    .text_size(crate::theme::FONT_BODY)
                    .line_height(px(18.0))
                    .opacity(0.82)
                    .child(if remote_running { "Running" } else { "Stopped" }),
            )
            .into_any_element();

        // Mobile Web address rows: list every available IP; each opens on click
        let mut url_rows: Vec<AnyElement> = Vec::new();
        url_rows.push(settings_card_row(
            "Remote Access",
            "Serve the Remote API on this Mac. Mobile clients connect with the access token.",
            remote_toggle,
        ));
        url_rows.push(settings_card_row(
            "Listener scope",
            "Off starts no listener. Loopback binds 127.0.0.1 (this Mac only). All interfaces binds 0.0.0.0 so phones on the network can connect.",
            listener_scope_control,
        ));
        url_rows.push(settings_card_row(
            "Listener",
            &listener_detail,
            listener_status,
        ));
        // Port input: the user can pick a free port when other apps conflict (committing restarts the listener).
        // notate 2026-08-29 F4: the input shows on the left (the reading start), no longer right-hung.
        let port_control = match &self.mobile_port_input {
            Some(input) => div()
                .w_full()
                .flex()
                .justify_start()
                .child(Input::new(input).small().w(px(96.0)))
                .into_any_element(),
            None => div().into_any_element(),
        };
        url_rows.push(settings_card_row(
            "Port",
            "Remote API + Mobile Web listen port. Change it when another app occupies the default; press Enter to apply (the listener restarts immediately).",
            port_control,
        ));
        // notate 2026-08-29 F3: all available addresses merge into a single Mobile Web row,
        // addresses left-aligned + Open right-aligned, removing duplicated titles and misalignment.
        if remote_running {
            let mut address_column = v_flex().w_full().min_w_0().gap(px(6.0));
            for (idx, addr) in all_addrs.iter().enumerate() {
                let url = format!("http://{addr}");
                let url_for_open = url.clone();
                address_column = address_column.child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .justify_between()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(crate::theme::FONT_BODY)
                                .text_color(content_theme.foreground.opacity(0.82))
                                .child(url),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("mobile-open-url-{idx}")))
                                .flex_none()
                                .flex()
                                .items_center()
                                .gap(SPACE_ICON)
                                .cursor_pointer()
                                .text_size(crate::theme::FONT_BODY)
                                .shardlane_interactive(
                                    content_theme.foreground.opacity(0.10),
                                    move |_window, app| {
                                        app.open_url(&url_for_open);
                                    },
                                )
                                .child(Icon::new(ComponentIconName::Globe).with_size(px(14.0)))
                                .child("Open"),
                        ),
                );
            }
            url_rows.push(settings_card_row(
                "Mobile Web",
                "Addresses reachable from this network. Click Open, or type one into a phone browser and pair with the QR below.",
                address_column.into_any_element(),
            ));
        } else {
            url_rows.push(settings_card_row(
                "Mobile Web",
                "Start Remote Access to get a URL.",
                div().into_any_element(),
            ));
        }
        let status_card = settings_card(surface, url_rows);

        // ---- Card 2: Pairing onboarding (QR + connection info) ----
        let content_button = content_theme.button_variant(cx);
        // UX security fix (2026-08-27 review): when Remote Access is off/not running, no
        // address+token combination may be shown — the old implementation rendered the full
        // credential QR and a plaintext token summary merely because the token was non-empty,
        // giving users the illusion of "ready to connect" even in the Off state.
        let qr_payload = if remote_running && !token.is_empty() {
            Some(format!("{connect_addr}#pair={token}"))
        } else {
            None
        };
        let qr_element = match &qr_payload {
            Some(payload) => render_qr(payload, foreground, background),
            None => div()
                .w(px(QR_RENDER_SIZE))
                .h(px(QR_RENDER_SIZE))
                .rounded(px(8.0))
                .border_1()
                .border_color(content_theme.border)
                .flex()
                .items_center()
                .justify_center()
                .px(px(14.0))
                .child(
                    div()
                        .text_size(crate::theme::FONT_META)
                        .line_height(px(16.0))
                        .opacity(0.7)
                        .child("Enable Remote Access to generate a pairing QR code."),
                )
                .into_any_element(),
        };

        let connect_label = if remote_running && !token.is_empty() {
            format!(
                "{connect_addr} · token: {}",
                mask_token(&token, "…").unwrap_or_else(|| token.clone())
            )
        } else {
            "Remote Access is off — enable it above to get a pairing code.".to_string()
        };

        let pairing_card = settings_card(
            surface,
            vec![settings_card_row(
                "Pair by QR",
                "Scan with a phone camera. The QR carries the connection address and credential; the mobile app reads it and connects automatically.",
                div().into_any_element(),
            )],
        )
        .child(
            div()
                .w_full()
                .px(px(20.0))
                .pb(px(16.0))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(8.0))
                .child(qr_element)
                .child(
                    div()
                        .text_size(crate::theme::FONT_META)
                        .line_height(px(16.0))
                        .opacity(0.55)
                        .child(connect_label),
                ),
        );

        // ---- Card 3: Online clients ----
        let connections = self.remote_web_connections();
        let mut client_rows = Vec::new();
        if connections.is_empty() {
            client_rows.push(settings_card_row(
                "No connected clients",
                "This list is live: connect from the mobile web app and the client appears here.",
                div().into_any_element(),
            ));
        } else {
            for connection in &connections {
                let conn_id = connection.id;
                let kick_herdr = herdr.clone();
                let disconnect_button = div()
                    .id(SharedString::from(format!("mobile-kick-{conn_id}")))
                    .flex()
                    .items_center()
                    .gap(SPACE_ICON)
                    .cursor_pointer()
                    .text_size(crate::theme::FONT_BODY)
                    .shardlane_interactive(
                        content_theme.foreground.opacity(0.10),
                        move |_window, app| {
                            kick_herdr.update(app, |this, cx| {
                                let remote_slot = this
                                    .shared
                                    .remote_server
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                                if let Some(handle) = remote_slot.as_ref() {
                                    handle.kick_web_connection(conn_id);
                                }
                                cx.notify();
                            });
                        },
                    )
                    .child(Icon::new(ComponentIconName::Close).with_size(px(14.0)))
                    .child("Disconnect")
                    .into_any_element();

                client_rows.push(settings_card_row(
                    &format!("Web client · {}", connection.addr),
                    &format!(
                        "connected {}s ago · last seen {}s ago",
                        connection.connected_at.elapsed().as_secs(),
                        connection.last_seen.elapsed().as_secs(),
                    ),
                    disconnect_button,
                ));
            }
        }
        let clients_card = settings_card(surface, client_rows);

        // ---- Card 4: Credential management ----
        let token_masked = mask_token(&token, "••••").unwrap_or_else(|| "—".to_string());
        let copy_herdr = herdr.clone();
        let copy_token = div()
            .id("mobile-copy-token")
            .flex()
            .items_center()
            .gap(SPACE_ICON)
            .cursor_pointer()
            .text_size(crate::theme::FONT_BODY)
            .shardlane_interactive(content_theme.foreground.opacity(0.10), move |_, app| {
                copy_herdr.update(app, |this, cx| {
                    if let Some(token) = this.config.remote.access_token.clone() {
                        cx.write_to_clipboard(crepuscularity_gpui::ClipboardItem::new_string(
                            token,
                        ));
                    }
                });
            })
            .child(Icon::new(ComponentIconName::Copy).with_size(px(14.0)))
            .child("Copy")
            .into_any_element();
        let rotate_herdr = herdr.clone();
        let rotate_button = Button::new("mobile-token-rotate")
            .custom(content_button)
            .xsmall()
            .label("Reset token")
            .on_click(move |_, _, cx| {
                rotate_herdr.update(cx, |this, cx| {
                    this.config.remote.rotate_access_token();
                    this.apply_remote_settings(cx);
                    this.save_config();
                    cx.notify();
                });
            });

        let credential_card = settings_card(
            surface,
            vec![
                settings_card_row(
                    "Access token",
                    &format!("Bearer token for clients. {token_masked}"),
                    copy_token,
                ),
                settings_card_row(
                    "Reset token",
                    "Revokes every paired client immediately (they re-pair via the new QR). Host identity stays stable.",
                    rotate_button.into_any_element(),
                ),
            ],
        );

        div()
            .child(status_card)
            .child(pairing_card)
            .child(clients_card)
            .child(credential_card)
            .into_any_element()
    }

    /// Snapshot of connected remote clients (empty when the remote service isn't running).
    fn remote_web_connections(&self) -> Vec<shardlane_remote::state::WebConnection> {
        self.shared
            .remote_server
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(|handle| handle.web_connections())
            .unwrap_or_default()
    }

    /// Lazily create the Mobile port input (InputState needs a window; render ensures every frame,
    /// same pattern as settings_sidebar_search). Initial value = the currently effective port.
    pub(super) fn ensure_mobile_port_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mobile_port_input.is_some() {
            return;
        }
        let initial = self.config.remote.port.to_string();
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("8757"));
        input.update(cx, |state, cx| {
            state.set_value(initial, window, cx);
        });
        let herdr = cx.entity();
        let input_for_commit = input.clone();
        let subscription = cx.subscribe_in(
            &input,
            window,
            move |_, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    let raw = input_for_commit.read(cx).value().to_string();
                    let herdr = herdr.clone();
                    window.defer(cx, move |window, cx| {
                        herdr.update(cx, |this, cx| this.commit_mobile_port(&raw, window, cx));
                    });
                }
                _ => {}
            },
        );
        self.mobile_port_input = Some(input);
        self.mobile_port_subscription = Some(subscription);
    }

    /// Commit the port: valid (1024..=65535) and changed → write config + restart the listener;
    /// invalid → notify and echo the currently effective port. The SHARDLANE_REMOTE_PORT env
    /// override only applies to the spawned copy (overlay_env inside apply_remote_settings), so
    /// test isolation is unaffected.
    pub(super) fn commit_mobile_port(
        &mut self,
        raw: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = self.config.remote.port;
        match raw.trim().parse::<u16>() {
            Ok(port) if (1024..=65535).contains(&port) => {
                if port != current {
                    self.config.remote.port = port;
                    self.apply_remote_settings(cx);
                    self.save_config();
                    cx.notify();
                }
            }
            _ => {
                window.push_notification("Port must be a number between 1024 and 65535", cx);
                if let Some(input) = &self.mobile_port_input {
                    let restored = current.to_string();
                    input.update(cx, |state, cx| {
                        state.set_value(restored, window, cx);
                    });
                }
            }
        }
    }
}

/// Mask a credential token for display: keep the first and last 4 characters separated
/// by `separator`; `None` when the token is too short to mask (audit E19: the
/// first4/last4 slicing existed twice in this module).
fn mask_token(token: &str, separator: &str) -> Option<String> {
    (token.len() > 8).then(|| format!("{}{separator}{}", &token[..4], &token[token.len() - 4..]))
}

/// QR grid rendering: module matrix → a div grid of dark cells on white (including the quiet zone).
fn render_qr(payload: &str, foreground: gpui::Hsla, background: gpui::Hsla) -> AnyElement {
    let Ok(code) =
        qrcode::QrCode::with_error_correction_level(payload.as_bytes(), qrcode::EcLevel::M)
    else {
        return div().child("QR generation failed").into_any_element();
    };
    let modules = code.width();
    // to_colors(): a row-major module matrix (Dark = dark module).
    let cells = code.to_colors();
    let total = modules + QR_QUIET_ZONE * 2;
    let cell = QR_RENDER_SIZE / total as f32;
    let mut grid = v_flex();
    for row in 0..total {
        let mut row_el = h_flex();
        for col in 0..total {
            let dark = row >= QR_QUIET_ZONE
                && col >= QR_QUIET_ZONE
                && row < total - QR_QUIET_ZONE
                && col < total - QR_QUIET_ZONE
                && matches!(
                    cells[(row - QR_QUIET_ZONE) * modules + (col - QR_QUIET_ZONE)],
                    qrcode::Color::Dark
                );
            row_el = row_el.child(
                div()
                    .w(px(cell))
                    .h(px(cell))
                    .when(dark, |cell| cell.bg(foreground)),
            );
        }
        grid = grid.child(row_el);
    }
    div()
        .p(px(8.0))
        .rounded(px(8.0))
        .bg(background)
        .child(grid)
        .into_any_element()
}
