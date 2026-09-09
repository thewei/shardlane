//! Settings -> Agent Hooks panel: manages per-Coding-Agent lifecycle hook
//! status sniffing and integrations.
//!
//! [INPUT]: depends on the ShardlaneApp state in super (main.rs), the
//! settings_view card vocabulary, ui::controls ControlSurface, the
//! gpui-component Button, and shardlane_host::agent_hooks::*
//! [OUTPUT]: exposes ShardlaneApp::agent_hooks_settings_content (the
//! Settings -> Agent Hooks content column)
//! [POS]: one split of the Settings panel; manages the status hook
//! install/uninstall for agents such as Claude Code, Codex, OpenCode, Pi,
//! and Command Code, with tmux / Herdr / remote tmux cross-backend sniffing
//! and a 4-level graceful degradation.

use std::path::PathBuf;

use super::*;
use crate::settings_view::{settings_card, settings_card_row};
use crate::ui::controls::ControlSurface;
use shardlane_host::agent_hooks::{AgentHookMeta, AgentHookRegistry, HookInstallStatus};

impl ShardlaneApp {
    pub(crate) fn agent_hooks_settings_content(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let herdr = cx.entity();
        let content_theme = self.content_surface_theme(window);
        let foreground = content_theme.foreground;
        let surface = ControlSurface {
            foreground,
            background: content_theme.background,
        };
        let content_button = content_theme.button_variant(cx);

        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));

        // 1. Overview Card: Description & Degradation System
        let ipc_status_text = shardlane_host::AgentHookIpcServer::default_socket_path()
            .map(|p| format!("Active: {}", p.display()))
            .unwrap_or_else(|| crate::i18n::t("settings.agent_hooks.ipc_active").to_string());

        let overview_card = settings_card(
            surface,
            vec![
                settings_card_row(
                    &crate::i18n::t("settings.agent_hooks.overview_title"),
                    &crate::i18n::t("settings.agent_hooks.overview_body"),
                    div().into_any_element(),
                ),
                settings_card_row(
                    &crate::i18n::t("settings.agent_hooks.ipc_title"),
                    &ipc_status_text,
                    div().into_any_element(),
                ),
            ],
        );

        // 2. Agent Hooks List Card
        let items: Vec<AgentHookMeta> = AgentHookRegistry::audit_all(&home);
        let mut hook_rows: Vec<AnyElement> = Vec::new();

        for (index, item) in items.into_iter().enumerate() {
            let agent = item.agent;
            let status = item.status;
            let display_name = item.name;

            let status_badge_text = match status {
                HookInstallStatus::Installed => {
                    crate::i18n::t("settings.agent_hooks.state_installed")
                }
                HookInstallStatus::Outdated => {
                    crate::i18n::t("settings.agent_hooks.state_outdated")
                }
                HookInstallStatus::NotInstalled => {
                    crate::i18n::t("settings.agent_hooks.state_not_installed")
                }
                HookInstallStatus::HerdrManaged => {
                    crate::i18n::t("settings.agent_hooks.state_herdr_managed")
                }
                HookInstallStatus::Unsupported => {
                    crate::i18n::t("settings.agent_hooks.state_unsupported")
                }
            };

            let path_info = item
                .hook_path
                .as_ref()
                .or(item.config_path.as_ref())
                .map(|p| p.display().to_string())
                .unwrap_or_default();

            let row_detail = if path_info.is_empty() {
                status_badge_text.to_string()
            } else {
                format!("{status_badge_text} · {path_info}")
            };

            let control: AnyElement = match status {
                HookInstallStatus::Installed => {
                    let click = herdr.clone();
                    let home = home.clone();
                    Button::new(("agent-hook-action", index))
                        .custom(content_button)
                        .xsmall()
                        .label(crate::i18n::t("settings.agent_hooks.action_uninstall"))
                        .on_click(move |_, _, cx| {
                            click.update(cx, |this, cx| {
                                match AgentHookRegistry::uninstall(agent, &home) {
                                    Ok(_) => this.agent_hook_notice = None,
                                    Err(e) => this.agent_hook_notice = Some(e.to_string()),
                                }
                                cx.notify();
                            });
                        })
                        .into_any_element()
                }
                HookInstallStatus::Outdated => {
                    let click = herdr.clone();
                    let home = home.clone();
                    Button::new(("agent-hook-action", index))
                        .custom(content_button)
                        .xsmall()
                        .label(crate::i18n::t("settings.agent_hooks.action_refresh"))
                        .on_click(move |_, _, cx| {
                            click.update(cx, |this, cx| {
                                match AgentHookRegistry::install(agent, &home) {
                                    Ok(_) => this.agent_hook_notice = None,
                                    Err(e) => this.agent_hook_notice = Some(e.to_string()),
                                }
                                cx.notify();
                            });
                        })
                        .into_any_element()
                }
                HookInstallStatus::NotInstalled | HookInstallStatus::HerdrManaged => {
                    let click = herdr.clone();
                    let home = home.clone();
                    Button::new(("agent-hook-action", index))
                        .custom(content_button)
                        .xsmall()
                        .label(crate::i18n::t("settings.agent_hooks.action_install"))
                        .on_click(move |_, _, cx| {
                            click.update(cx, |this, cx| {
                                match AgentHookRegistry::install(agent, &home) {
                                    Ok(_) => this.agent_hook_notice = None,
                                    Err(e) => this.agent_hook_notice = Some(e.to_string()),
                                }
                                cx.notify();
                            });
                        })
                        .into_any_element()
                }
                HookInstallStatus::Unsupported => div()
                    .text_size(crate::theme::FONT_META)
                    .text_color(content_theme.muted)
                    .child(crate::i18n::t("settings.agent_hooks.state_unsupported"))
                    .into_any_element(),
            };

            hook_rows.push(settings_card_row(display_name, &row_detail, control));
        }

        let hooks_card = settings_card(surface, hook_rows);

        let notice_element = self.agent_hook_notice.clone().map(|msg| {
            div()
                .mt(px(10.0))
                .text_size(crate::theme::FONT_META)
                .text_color(foreground.opacity(0.8))
                .child(format!("⚠ {msg}"))
        });

        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .child(overview_card)
            .child(hooks_card)
            .children(notice_element)
            .into_any_element()
    }
}
