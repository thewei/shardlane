//! Settings page presentation layer: settings group cards, control rows, and theme preset cards.
//!
//! Theme page semantics (2026-08-29): first pick light/dark (Light/Dark/Auto segment), then pick
//! that appearance's official Herdr theme card; Auto shows both Light/Dark choice groups. The rest
//! of the Herdr page merged into the Terminal page's Herdr TUI card (user-wise it all lives in
//! Terminal); the Herdr section was deleted.
//!
//! i18n (2026-09-01): the Appearance page's copy is served through `crate::i18n::t()` and the
//! Language card switches `settings::Language`; other pages migrate to `t()` surface by surface.
//!
//! [INPUT]: Depends on `super` (main.rs)'s ShardlaneApp state, the settings/config models,
//! theme presets, font_catalog (monospace font enumeration), gpui-component controls,
//! and the ui::controls form control family
//! [OUTPUT]: Exposes `ShardlaneApp::settings_page` (with the private settings_card/settings_card_row/
//! settings_section_title helpers)
//! [POS]: One of main.rs's presentation-layer splits (sibling of search_view.rs/header_view.rs);
//! config read/write paths and actions remain owned by main.rs

use super::*;
use crate::font_catalog;
use crate::ui::controls::{ControlMenu, ControlSurface, Segmented, Toggle};

impl ShardlaneApp {
    pub(super) fn settings_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let herdr = cx.entity();
        let current_font = self.config.terminal.font_family.clone();
        let herdr_config = self.herdr_user_config.clone();
        let herdr_config_path = herdr_tui::herdr_user_config_path().display().to_string();
        let agent_notifications = self.config.behavior.agent_notifications;
        let cursor_style_preference = self.config.terminal.cursor_style;
        let cursor_blink_preference = self.config.terminal.cursor_blink;
        let tab_bar_placement = self.config.terminal.tab_bar_placement;
        let font_size = self.config.terminal.font_size.clamp(10.0, 24.0);
        let line_height = self.config.terminal.line_height.clamp(14.0, 34.0);
        let terminal_padding = self.terminal_content_padding();
        let lazygit_config = self.config.lazygit.clone();
        let opacity = self.window_opacity();
        let content_theme = self.content_surface_theme(window);
        let foreground = content_theme.foreground;
        let background = content_theme.background;
        // Control-family injected colors: the Settings content column's surface follows the current
        // content surface; the Hosted Terminal's named theme itself is controlled only by the official Herdr theme.
        let surface = ControlSurface {
            foreground,
            background,
        };
        let compact_layout = sidebar_should_auto_collapse(window.bounds().size.width.to_f64());
        let selected_section = self.settings_section;
        let selected_provider = self.settings_provider_detail;
        let current_language = self.config.ui.language;
        let content_button = content_theme.button_variant(cx);

        // Theme page semantics: pick light/dark first (appearance mode), then the theme. Manual mode
        // shows only that appearance's official theme cards; Auto gives both Light/Dark groups (both need a choice).
        let theme_appearance_mode =
            herdr_tui::theme_appearance_mode(&herdr_config, &self.config.ui.appearance);
        let (theme_light_selection, theme_dark_selection) =
            herdr_tui::effective_theme_selections(&herdr_config);
        let theme_card_categories = match theme_appearance_mode {
            herdr_tui::ThemeAppearanceMode::Light => vec![theme::ThemePresetCategory::Light],
            herdr_tui::ThemeAppearanceMode::Dark => vec![theme::ThemePresetCategory::Dark],
            herdr_tui::ThemeAppearanceMode::Auto => theme::ThemePresetCategory::ALL.to_vec(),
        };
        let theme_preset_groups = theme_card_categories
            .into_iter()
            .map(|category| {
                let selected = match category {
                    theme::ThemePresetCategory::Light => theme_light_selection.clone(),
                    theme::ThemePresetCategory::Dark => theme_dark_selection.clone(),
                };
                // In manual mode the active card is just the currently effective `theme.name`; Auto highlights each appearance's selection.
                let active_name = match theme_appearance_mode {
                    herdr_tui::ThemeAppearanceMode::Auto => selected,
                    _ => herdr_config.theme_name.clone(),
                };
                let cards = theme::THEME_PRESETS
                    .iter()
                    .copied()
                    .enumerate()
                    .filter(|(_, preset)| preset.category == category)
                    .map(|(ix, preset)| {
                        theme_preset_card(
                            ix,
                            preset,
                            active_name == preset.herdr_theme,
                            content_theme,
                            herdr.clone(),
                        )
                    })
                    .collect::<Vec<_>>();
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        div()
                            .text_size(theme::FONT_META)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(content_theme.muted)
                            .child(category.label()),
                    )
                    .child(
                        div()
                            .w_full()
                            .grid()
                            .grid_cols(if compact_layout { 1 } else { 3 })
                            .gap_2()
                            .children(cards),
                    )
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        // —— Control family construction (dumb components + side-effect callbacks) ——

        // Font family: the system monospace font enumeration (font_catalog, cached in-process) supplies
        // the options; fonts injected via hand-edited config also join the menu to avoid a no-selection state.
        let font_herdr = herdr.clone();
        let mut font_menu = ControlMenu::new("settings-font", surface).label(current_font.clone());
        for font in font_catalog::monospace_font_families() {
            font_menu = font_menu.option(font.clone(), font.clone());
        }
        if !font_catalog::monospace_font_families()
            .iter()
            .any(|f| f == &current_font)
        {
            font_menu = font_menu.option(current_font.clone(), current_font.clone());
        }
        let font_control = font_menu
            .value(current_font.clone())
            .on_change(move |font, _, app| {
                font_herdr.update(app, |this, cx| {
                    this.config.terminal.font_family = font.clone();
                    this.apply_terminal_render_settings(cx);
                    this.save_config();
                    cx.notify();
                });
            })
            .menu_below_right()
            .into_any_element();

        let padding_herdr = herdr.clone();
        let mut padding_control = Segmented::new("settings-terminal-padding", surface)
            .option(0.0_f32, "0")
            .option(4.0, "4")
            .option(8.0, "8")
            .option(12.0, "12")
            .option(16.0, "16");
        // Tolerance: a non-preset value from hand-edited config is appended exactly as a custom segment (honest presentation, no snapping to the nearest preset).
        let padding_presets = [0.0_f32, 4.0, 8.0, 12.0, 16.0];
        if !padding_presets
            .iter()
            .any(|preset| (terminal_padding - preset).abs() < f32::EPSILON)
        {
            let custom_label = if terminal_padding.fract() == 0.0 {
                format!("{terminal_padding:.0}")
            } else {
                format!("{terminal_padding:.1}")
            };
            padding_control = padding_control.option(terminal_padding, custom_label);
        }
        let padding_control = padding_control
            .value(terminal_padding)
            .on_change(move |value, _, app| {
                padding_herdr.update(app, |this, cx| {
                    this.config.terminal.padding = *value;
                    this.apply_terminal_render_settings(cx);
                    this.save_config();
                    cx.notify();
                });
            })
            .into_any_element();

        // Cursor style/blink: let the user force cursor shape and blinking; Auto follows the terminal program
        // (DECSCUSR/default style and ?12 blink, including hollow when unfocused), sharing the same render
        // settings dispatch path as padding.
        let cursor_style_herdr = herdr.clone();
        let cursor_style_control = Segmented::new("settings-terminal-cursor-style", surface)
            .option(
                crate::settings::TerminalCursorStylePreference::FollowTerminal,
                "Auto",
            )
            .option(
                crate::settings::TerminalCursorStylePreference::Block,
                "Block",
            )
            .option(crate::settings::TerminalCursorStylePreference::Bar, "Bar")
            .option(
                crate::settings::TerminalCursorStylePreference::Underline,
                "Underline",
            )
            .value(cursor_style_preference)
            .on_change(move |preference, _, app| {
                cursor_style_herdr.update(app, |this, cx| {
                    this.config.terminal.cursor_style = *preference;
                    this.apply_terminal_render_settings(cx);
                    this.save_config();
                    cx.notify();
                });
            })
            .into_any_element();

        let cursor_blink_herdr = herdr.clone();
        let cursor_blink_control = Segmented::new("settings-terminal-cursor-blink", surface)
            .option(
                crate::settings::TerminalCursorBlinkPreference::FollowTerminal,
                "Auto",
            )
            .option(crate::settings::TerminalCursorBlinkPreference::On, "Blink")
            .option(
                crate::settings::TerminalCursorBlinkPreference::Off,
                "Steady",
            )
            .value(cursor_blink_preference)
            .on_change(move |preference, _, app| {
                cursor_blink_herdr.update(app, |this, cx| {
                    this.config.terminal.cursor_blink = *preference;
                    this.apply_terminal_render_settings(cx);
                    this.save_config();
                    cx.notify();
                });
            })
            .into_any_element();

        // Tab bar placement: where the active Project's Tab list is presented. Both sides keep
        // Herdr's Tab order/lifecycle authority; this only swaps the presentation owner
        // (Sidebar Tab rows vs the native content-area Tab strip).
        let tab_bar_herdr = herdr.clone();
        let tab_bar_placement_control =
            Segmented::new("settings-terminal-tab-bar-placement", surface)
                .option(crate::settings::TabBarPlacement::Sidebar, "Sidebar")
                .option(crate::settings::TabBarPlacement::Native, "Native Tabs")
                .value(tab_bar_placement)
                .on_change(move |placement, _, app| {
                    tab_bar_herdr.update(app, |this, cx| {
                        this.config.terminal.tab_bar_placement = *placement;
                        this.save_config();
                        this.notify_sidebar(cx);
                        cx.notify();
                    });
                })
                .into_any_element();

        let notifications_herdr = herdr.clone();
        let notifications_control = Toggle::new("settings-agent-notifications", surface)
            .checked(agent_notifications)
            .on_change(move |checked, _, app| {
                notifications_herdr.update(app, |this, cx| {
                    this.config.behavior.agent_notifications = checked;
                    if checked {
                        notifications::request_authorization();
                    }
                    this.save_config();
                    cx.notify();
                });
            })
            .into_any_element();

        let reload_herdr = herdr.clone();
        let herdr_reload_control = Button::new("settings-herdr-reload-config")
            .custom(content_button)
            .xsmall()
            .label("Reload")
            .on_click(move |_, window, app| {
                reload_herdr.update(app, |this, cx| {
                    this.reload_herdr_config(&ReloadHerdrConfig, window, cx);
                });
            })
            .into_any_element();

        let herdr_copy_herdr = herdr.clone();
        let herdr_copy_control = Toggle::new("settings-herdr-copy-on-select", surface)
            .checked(herdr_config.copy_on_select)
            .on_change(move |enabled, window, app| {
                herdr_copy_herdr.update(app, |this, cx| {
                    this.apply_herdr_user_config_update_and_restart(
                        herdr_tui::HerdrUserConfigUpdate::CopyOnSelect(enabled),
                        window,
                        cx,
                    );
                });
            })
            .into_any_element();

        let mouse_capture_herdr = herdr.clone();
        let mouse_capture_control = Toggle::new("settings-herdr-mouse-capture", surface)
            .checked(herdr_config.mouse_capture)
            .on_change(move |enabled, window, app| {
                mouse_capture_herdr.update(app, |this, cx| {
                    this.apply_herdr_user_config_update_and_restart(
                        herdr_tui::HerdrUserConfigUpdate::MouseCapture(enabled),
                        window,
                        cx,
                    );
                });
            })
            .into_any_element();

        let sidebar_start_herdr = herdr.clone();
        let sidebar_start_control = Toggle::new("settings-herdr-sidebar-start-collapsed", surface)
            .checked(herdr_config.sidebar_start_collapsed)
            .on_change(move |enabled, window, app| {
                sidebar_start_herdr.update(app, |this, cx| {
                    this.apply_herdr_user_config_update_and_restart(
                        herdr_tui::HerdrUserConfigUpdate::SidebarStartCollapsed(enabled),
                        window,
                        cx,
                    );
                });
            })
            .into_any_element();

        let sidebar_mode_herdr = herdr.clone();
        let sidebar_mode_control = Segmented::new("settings-herdr-sidebar-collapsed-mode", surface)
            .option("compact".to_string(), "Compact")
            .option("hidden".to_string(), "Hidden")
            .value(herdr_config.sidebar_collapsed_mode.clone())
            .on_change(move |mode, window, app| {
                sidebar_mode_herdr.update(app, |this, cx| {
                    this.apply_herdr_user_config_update_and_restart(
                        herdr_tui::HerdrUserConfigUpdate::SidebarCollapsedMode(mode.clone()),
                        window,
                        cx,
                    );
                });
            })
            .into_any_element();

        let pane_borders_herdr = herdr.clone();
        let pane_borders_control = Toggle::new("settings-herdr-pane-borders", surface)
            .checked(herdr_config.pane_borders)
            .on_change(move |enabled, window, app| {
                pane_borders_herdr.update(app, |this, cx| {
                    this.apply_herdr_user_config_update_and_restart(
                        herdr_tui::HerdrUserConfigUpdate::PaneBorders(enabled),
                        window,
                        cx,
                    );
                });
            })
            .into_any_element();

        let outer_borders_herdr = herdr.clone();
        let pane_outer_borders_control = Toggle::new("settings-herdr-pane-outer-borders", surface)
            .checked(herdr_config.pane_outer_borders)
            .on_change(move |enabled, window, app| {
                outer_borders_herdr.update(app, |this, cx| {
                    this.apply_herdr_user_config_update_and_restart(
                        herdr_tui::HerdrUserConfigUpdate::PaneOuterBorders(enabled),
                        window,
                        cx,
                    );
                });
            })
            .into_any_element();

        let pane_scrollbars_herdr = herdr.clone();
        let pane_scrollbars_control = Toggle::new("settings-herdr-pane-scrollbars", surface)
            .checked(herdr_config.pane_scrollbars)
            .on_change(move |enabled, window, app| {
                pane_scrollbars_herdr.update(app, |this, cx| {
                    this.apply_herdr_user_config_update_and_restart(
                        herdr_tui::HerdrUserConfigUpdate::PaneScrollbars(enabled),
                        window,
                        cx,
                    );
                });
            })
            .into_any_element();

        let pane_gaps_herdr = herdr.clone();
        let pane_gaps_control = Toggle::new("settings-herdr-pane-gaps", surface)
            .checked(herdr_config.pane_gaps)
            .on_change(move |enabled, window, app| {
                pane_gaps_herdr.update(app, |this, cx| {
                    this.apply_herdr_user_config_update_and_restart(
                        herdr_tui::HerdrUserConfigUpdate::PaneGaps(enabled),
                        window,
                        cx,
                    );
                });
            })
            .into_any_element();

        let single_tab_herdr = herdr.clone();
        let hide_single_tab_control = Toggle::new("settings-herdr-hide-single-tab", surface)
            .checked(herdr_config.hide_tab_bar_when_single_tab)
            .on_change(move |enabled, window, app| {
                single_tab_herdr.update(app, |this, cx| {
                    this.apply_herdr_user_config_update_and_restart(
                        herdr_tui::HerdrUserConfigUpdate::HideTabBarWhenSingleTab(enabled),
                        window,
                        cx,
                    );
                });
            })
            .into_any_element();

        let tab_position_herdr = herdr.clone();
        let tab_position_control = Segmented::new("settings-herdr-tab-position", surface)
            .option("top".to_string(), "Top")
            .option("bottom".to_string(), "Bottom")
            .value(herdr_config.tab_bar_position.clone())
            .on_change(move |position, window, app| {
                tab_position_herdr.update(app, |this, cx| {
                    this.apply_herdr_user_config_update_and_restart(
                        herdr_tui::HerdrUserConfigUpdate::TabBarPosition(position.clone()),
                        window,
                        cx,
                    );
                });
            })
            .into_any_element();

        let indicators_herdr = herdr.clone();
        let indicators_control = Segmented::new("settings-herdr-status-indicators", surface)
            .option("dots".to_string(), "Dots")
            .option("symbols".to_string(), "Symbols")
            .value(herdr_config.status_indicators.clone())
            .on_change(move |style, window, app| {
                indicators_herdr.update(app, |this, cx| {
                    this.apply_herdr_user_config_update_and_restart(
                        herdr_tui::HerdrUserConfigUpdate::StatusIndicators(style.clone()),
                        window,
                        cx,
                    );
                });
            })
            .into_any_element();

        let scroll_lines_herdr = herdr.clone();
        let mut scroll_lines_control = Segmented::new("settings-herdr-scroll-lines", surface)
            .option(1_i64, "1")
            .option(3_i64, "3")
            .option(5_i64, "5")
            .option(8_i64, "8");
        if ![1_i64, 3, 5, 8].contains(&herdr_config.mouse_scroll_lines) {
            scroll_lines_control = scroll_lines_control.option(
                herdr_config.mouse_scroll_lines,
                herdr_config.mouse_scroll_lines.to_string(),
            );
        }
        let herdr_scroll_lines_control = scroll_lines_control
            .value(herdr_config.mouse_scroll_lines)
            .on_change(move |lines, window, app| {
                scroll_lines_herdr.update(app, |this, cx| {
                    this.apply_herdr_user_config_update_and_restart(
                        herdr_tui::HerdrUserConfigUpdate::MouseScrollLines(*lines),
                        window,
                        cx,
                    );
                });
            })
            .into_any_element();

        let compact_nav = h_flex().w_full().flex_wrap().gap_1().children(
            SettingsSection::ALL
                .into_iter()
                .enumerate()
                .map(|(ix, section)| {
                    let section_herdr = herdr.clone();
                    Button::new(("settings-compact-section", ix))
                        .custom(content_button)
                        .xsmall()
                        .selected(section == selected_section)
                        .label(section.label())
                        .on_click(move |_, _, app| {
                            section_herdr
                                .update(app, |this, cx| this.set_settings_section(section, cx));
                        })
                }),
        );

        // Mode selection: three selectable small cards (same interaction primitive as the theme cards). Selected gets an outline + check.
        let mode_cards = [
            (
                herdr_tui::ThemeAppearanceMode::Light,
                i18n::t("settings.appearance.mode_light"),
                i18n::t("settings.appearance.mode_light_detail"),
            ),
            (
                herdr_tui::ThemeAppearanceMode::Dark,
                i18n::t("settings.appearance.mode_dark"),
                i18n::t("settings.appearance.mode_dark_detail"),
            ),
            (
                herdr_tui::ThemeAppearanceMode::Auto,
                i18n::t("settings.appearance.mode_auto"),
                i18n::t("settings.appearance.mode_auto_detail"),
            ),
        ]
        .into_iter()
        .enumerate()
        .map(|(ix, (mode, label, detail))| {
            let selected = theme_appearance_mode == mode;
            let mode_herdr = herdr.clone();
            div()
                .id(("settings-theme-mode", ix))
                .flex_1()
                .min_w_0()
                .px_3()
                .py_2()
                .rounded(px(9.0))
                .border_1()
                .border_color(if selected {
                    content_theme.primary.opacity(0.95)
                } else {
                    content_theme.border
                })
                .bg(if selected {
                    content_theme.primary.opacity(0.08)
                } else {
                    content_theme.active
                })
                .cursor_pointer()
                .hover(move |style| style.border_color(content_theme.primary.opacity(0.55)))
                .shardlane_interactive(content_theme.primary.opacity(0.12), move |window, app| {
                    mode_herdr.update(app, |this, cx| {
                        this.apply_theme_appearance_mode(mode, window, cx);
                    });
                })
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            h_flex()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(theme::FONT_BODY)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(foreground)
                                        .child(label.to_string()),
                                )
                                .when(selected, |row| {
                                    row.child(
                                        div()
                                            .size(px(16.0))
                                            .rounded(px(8.0))
                                            .bg(content_theme.primary.opacity(0.15))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_color(content_theme.primary)
                                            .child(
                                                Icon::new(ComponentIconName::CircleCheck).xsmall(),
                                            ),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .text_size(theme::FONT_META)
                                .text_color(content_theme.muted)
                                .whitespace_normal()
                                .child(detail.to_string()),
                        ),
                )
                .into_any_element()
        })
        .collect::<Vec<_>>();

        // —— SettingsCard: the Theme card returns to a single custom container (isomorphic to the original
        // appearance_card, whose rendering is verified; the settings_card_row control slot does not render
        // within this card's context — do not backfill).
        let theme_card = settings_card(
            surface,
            vec![v_flex()
                .w_full()
                .min_w_0()
                .overflow_hidden()
                .gap_4()
                .px(px(20.0))
                .py(px(14.0))
                .child(settings_section_title(
                    &i18n::t("settings.appearance.title"),
                    &i18n::t("settings.appearance.subtitle"),
                ))
                .child(h_flex().w_full().gap_2().children(mode_cards))
                .when(
                    theme_appearance_mode == herdr_tui::ThemeAppearanceMode::Auto
                        && theme::preset_for_herdr_theme(&herdr_config.theme_name).is_none(),
                    |section| {
                        section.child(
                            div()
                                .w_full()
                                .px_3()
                                .py_2()
                                .rounded(px(6.0))
                                .bg(content_theme.active)
                                .text_size(theme::FONT_META)
                                .text_color(content_theme.muted)
                                .child(i18n::t("settings.appearance.notice_unofficial_theme")),
                        )
                    },
                )
                .when(
                    theme_appearance_mode == herdr_tui::ThemeAppearanceMode::Auto,
                    |section| {
                        section.child(
                            div()
                                .w_full()
                                .px_3()
                                .py_2()
                                .rounded(px(6.0))
                                .bg(content_theme.active)
                                .text_size(theme::FONT_META)
                                .text_color(content_theme.muted)
                                .child(i18n::t("settings.appearance.notice_auto_both")),
                        )
                    },
                )
                .children(theme_preset_groups)
                .into_any_element()],
        );

        // Language: interface language of the native shell (i18n). English is the
        // only catalog today; the menu is the seam additional locales plug into
        // (a `settings::Language` variant + `locales/<code>.yml`). Switching
        // applies the process-global locale and re-renders every surface.
        let language_herdr = herdr.clone();
        let mut language_menu =
            ControlMenu::new("settings-language", surface).label(current_language.display_name());
        for language in settings::Language::ALL {
            language_menu = language_menu.option(language, language.display_name());
        }
        let language_control = language_menu
            .value(current_language)
            .on_change(move |language, _, app| {
                language_herdr.update(app, |this, cx| {
                    this.config.ui.language = *language;
                    this.save_config();
                    i18n::apply_language(*language);
                    cx.notify();
                });
            })
            .menu_below_right()
            .into_any_element();
        let language_card = settings_card(
            surface,
            vec![settings_card_row(
                &i18n::t("settings.appearance.language_title"),
                &i18n::t("settings.appearance.language_subtitle"),
                language_control,
            )],
        );

        let herdr_tui_card = settings_card(
            surface,
            vec![
                settings_card_row(
                    "Herdr TUI",
                    "Preferences of the hosted Herdr terminal UI. They live in the real Herdr config and apply to every Herdr client. Changing them restarts the hosted terminal session.",
                    h_flex().into_any_element(),
                ),
                settings_card_row(
                    "Config file",
                    &herdr_config_path,
                    herdr_reload_control,
                ),
                settings_card_row(
                    "Copy on select",
                    "Herdr ui.copy_on_select. This affects native Herdr mouse selection behavior in every Herdr client.",
                    herdr_copy_control,
                ),
                settings_card_row(
                    "Mouse capture",
                    "Herdr ui.mouse_capture: enable Herdr's mouse-aware UI in every client.",
                    mouse_capture_control,
                ),
                settings_card_row(
                    "Mouse scroll lines",
                    "Herdr ui.mouse_scroll_lines: rows moved per wheel notch.",
                    herdr_scroll_lines_control,
                ),
                settings_card_row(
                    "Start sidebar collapsed",
                    "Herdr ui.sidebar_start_collapsed. This is a Herdr launch preference, not a Shardlane-only override.",
                    sidebar_start_control,
                ),
                settings_card_row(
                    "Collapsed sidebar mode",
                    "Herdr ui.sidebar_collapsed_mode. Hidden is the cleanest fit inside Shardlane; Compact remains available for normal Herdr clients too.",
                    sidebar_mode_control,
                ),
                settings_card_row(
                    "Pane borders",
                    "Herdr ui.pane_borders.",
                    pane_borders_control,
                ),
                settings_card_row(
                    "Outer pane borders",
                    "Herdr ui.pane_outer_borders.",
                    pane_outer_borders_control,
                ),
                settings_card_row(
                    "Pane scrollbars",
                    "Herdr ui.pane_scrollbars.",
                    pane_scrollbars_control,
                ),
                settings_card_row(
                    "Pane gaps",
                    "Herdr ui.pane_gaps.",
                    pane_gaps_control,
                ),
                settings_card_row(
                    "Hide single-tab bar",
                    "Herdr ui.hide_tab_bar_when_single_tab.",
                    hide_single_tab_control,
                ),
                settings_card_row(
                    "Tab bar position",
                    "Herdr ui.tab_bar_position.",
                    tab_position_control,
                ),
                settings_card_row(
                    "Status indicators",
                    "Herdr ui.status_indicators.",
                    indicators_control,
                ),
            ],
        );

        // TUI-only cutover: the normal work surface is always the single hosted Herdr TUI, with no
        // Embedded/TUI mode selection; the Terminal card keeps appearance preferences + host restore
        // state/actions (plan §18).
        let mut terminal_rows = vec![settings_card_row(
            "Terminal",
            "The work surface hosts one Herdr TUI. Herdr's normal user config is authoritative; Shardlane does not maintain a second hosted-TUI config.",
            h_flex().into_any_element(),
        )];
        {
            let tui_status = self.tui_host.status;
            let tui_status_label = tui_status.label();
            let tui_status_detail = self
                .tui_host
                .last_error
                .as_deref()
                .unwrap_or(match tui_status {
                    crate::herdr_tui::HerdrTuiHostStatus::Stopped => {
                        "TUI host is not running. Click Restart to launch."
                    }
                    crate::herdr_tui::HerdrTuiHostStatus::Starting => "TUI host is starting up…",
                    crate::herdr_tui::HerdrTuiHostStatus::Running => {
                        "Herdr TUI process is active and receiving navigation commands."
                    }
                    crate::herdr_tui::HerdrTuiHostStatus::Failed => {
                        "TUI host encountered an error."
                    }
                })
                .to_string();
            let glyph_level = match tui_status {
                crate::herdr_tui::HerdrTuiHostStatus::Stopped => {
                    crate::status::AttentionLevel::Idle
                }
                crate::herdr_tui::HerdrTuiHostStatus::Starting => {
                    crate::status::AttentionLevel::Working
                }
                crate::herdr_tui::HerdrTuiHostStatus::Running => {
                    crate::status::AttentionLevel::ReadyForReview
                }
                crate::herdr_tui::HerdrTuiHostStatus::Failed => {
                    crate::status::AttentionLevel::NeedsAttention
                }
            };
            let status_glyph =
                crate::status::status_glyph_container("tui-status-glyph", glyph_level, cx);
            terminal_rows.push(settings_card_row(
                "Herdr host status",
                &format!("{tui_status_label} — {tui_status_detail}"),
                status_glyph,
            ));

            let restart_herdr = herdr.clone();
            let tui_actions = h_flex()
                .gap(SPACE_ICON)
                .child(
                    Button::new("tui-restart")
                        .custom(content_button)
                        .xsmall()
                        .label("Restart")
                        .on_click(move |_, window, app| {
                            restart_herdr.update(app, |this, cx| {
                                this.restart_tui_surface(window, cx);
                            });
                        }),
                )
                .into_any_element();
            terminal_rows.push(settings_card_row(
                "Host actions",
                "Restart the hosted Herdr terminal process.",
                tui_actions,
            ));
        }
        terminal_rows.extend([
            settings_card_row(
                "Font family",
                "Used by every visible terminal pane.",
                font_control,
            ),
                settings_card_row(
                    "Font size",
                    // Audit E23: surface the default and the hidden ⌘0 reset next to the value.
                    &format!(
                        "{font_size:.0} pt · default {:.0} pt · ⌘0 resets",
                        settings::TerminalConfig::default().font_size
                    ),
                    Slider::new(&self.font_size_slider)
                        .ml(px(8.0))
                        .mr(px(8.0))
                        .bg(content_theme.foreground.opacity(0.18))
                        .text_color(content_theme.primary)
                        .w(px(240.0))
                        .max_w_full()
                        .into_any_element(),
                ),
                settings_card_row(
                    "Line height",
                    &format!(
                        "{line_height:.0} px · default {:.0} px",
                        settings::TerminalConfig::default().line_height
                    ),
                    Slider::new(&self.line_height_slider)
                        .ml(px(8.0))
                        .mr(px(8.0))
                        .bg(content_theme.foreground.opacity(0.18))
                        .text_color(content_theme.primary)
                        .w(px(240.0))
                        .max_w_full()
                        .into_any_element(),
                ),
                settings_card_row(
                    "Padding",
                    &format!(
                        "{terminal_padding:.0} px around the character grid · default {:.0}",
                        settings::TerminalConfig::default().padding
                    ),
                    padding_control,
                ),
                settings_card_row(
                    "Tab bar",
                    "Show Project tabs in the Sidebar tree or as native tabs above the terminal content.",
                    tab_bar_placement_control,
                ),
                settings_card_row(
                    "Cursor style",
                    "Auto follows the running program (including the hollow caret when the window loses focus). Default: Auto.",
                    cursor_style_control,
                ),
                settings_card_row(
                    "Cursor blinking",
                    "Auto follows the program's blink mode. Blink forces it on; Steady keeps the caret solid. Default: Auto.",
                    cursor_blink_control,
                ),
            ]);

        let terminal_card = settings_card(surface, terminal_rows);

        let behavior_card = settings_card(
            surface,
            vec![settings_card_row(
                "Agent notifications",
                "Use macOS notifications only for meaningful Agent state transitions.",
                notifications_control,
            )],
        );

        // Audit E09: always_on_top is a persisted preference that previously only existed
        // in the native Window menu — surface it here through the same toggle path.
        let always_on_top_herdr = herdr.clone();
        let always_on_top_control = Toggle::new("settings-window-always-on-top", surface)
            .checked(self.config.ui.window.always_on_top)
            .on_change(move |_, window, app| {
                always_on_top_herdr.update(app, |this, cx| {
                    this.toggle_window_always_on_top(window, cx);
                });
            })
            .into_any_element();
        let window_card =
            settings_card(
                surface,
                vec![settings_card_row(
                "Opacity",
                // Audit E23: annotate the default next to the current value.
                &format!(
                    "{:.0}% · default {:.0}%",
                    opacity * 100.0,
                    settings::WindowConfig::default().opacity * 100.0
                ),
                Slider::new(&self.opacity_slider)
                    .ml(px(8.0))
                    .mr(px(8.0))
                    .bg(content_theme.foreground.opacity(0.18))
                    .text_color(content_theme.primary)
                    .w(px(240.0))
                    .max_w_full()
                    .into_any_element(),
            ),
            settings_card_row(
                "Always on Top",
                "Keep the Shardlane window above all others. Same toggle as the Window menu.",
                always_on_top_control,
            )],
            );

        // Providers list/detail and source policy live in their own view module;
        // both surfaces consume the roster generation held by HistoryUiState.
        let providers_content = self.providers_settings_content(surface, window, cx);

        // Lazygit discovery is cached and runs off the render path. The right-panel
        // session has its own readiness state; this card only exposes CLI/config facts.
        if selected_section == SettingsSection::Lazygit {
            self.ensure_lazygit_detection(cx);
            self.ensure_lazygit_executable_input(window, cx);
        }
        let lazygit_status = self
            .lazygit_detection
            .as_ref()
            .map(|detection| match detection.compatibility {
                crate::right_panel::lazygit::LazygitCompatibility::Supported => {
                    format!("Supported — {}", detection.detail)
                }
                crate::right_panel::lazygit::LazygitCompatibility::Outdated => {
                    format!(
                        "Available without overlay — update recommended — {}",
                        detection.detail
                    )
                }
                crate::right_panel::lazygit::LazygitCompatibility::Missing => {
                    detection.detail.clone()
                }
            })
            .unwrap_or_else(|| "Detecting Lazygit…".to_string());
        let lazygit_refresh_herdr = herdr.clone();
        let lazygit_refresh_control = Button::new("settings-lazygit-refresh")
            .custom(content_button)
            .xsmall()
            .label("Refresh")
            .on_click(move |_, _, app| {
                lazygit_refresh_herdr.update(app, |this, cx| {
                    this.refresh_lazygit_detection(cx);
                });
            })
            .into_any_element();
        let lazygit_panel_herdr = herdr.clone();
        let lazygit_panel_control = Segmented::new("settings-lazygit-startup-panel", surface)
            .option(crate::settings::LazygitStartupPanel::Status, "Status")
            .option(crate::settings::LazygitStartupPanel::Branch, "Branch")
            .option(crate::settings::LazygitStartupPanel::Log, "Log")
            .option(crate::settings::LazygitStartupPanel::Stash, "Stash")
            .value(lazygit_config.startup_panel)
            .on_change(move |value, _, app| {
                lazygit_panel_herdr.update(app, |this, cx| {
                    this.config.lazygit.startup_panel = *value;
                    this.schedule_config_save(cx);
                    if this.is_lazygit_surface_active() {
                        this.restart_lazygit_session(cx);
                    }
                    cx.notify();
                });
            })
            .into_any_element();
        let lazygit_screen_herdr = herdr.clone();
        let lazygit_screen_control = Segmented::new("settings-lazygit-screen-mode", surface)
            .option(crate::settings::LazygitScreenMode::Normal, "Normal")
            .option(crate::settings::LazygitScreenMode::Half, "Half")
            .option(crate::settings::LazygitScreenMode::Full, "Full")
            .value(lazygit_config.screen_mode)
            .on_change(move |value, _, app| {
                lazygit_screen_herdr.update(app, |this, cx| {
                    this.config.lazygit.screen_mode = *value;
                    this.schedule_config_save(cx);
                    if this.is_lazygit_surface_active() {
                        this.restart_lazygit_session(cx);
                    }
                    cx.notify();
                });
            })
            .into_any_element();
        let lazygit_integration_herdr = herdr.clone();
        let lazygit_integration_control = Toggle::new("settings-lazygit-integration", surface)
            .checked(lazygit_config.integration_enabled)
            .on_change(move |enabled, _, app| {
                lazygit_integration_herdr.update(app, |this, cx| {
                    this.config.lazygit.integration_enabled = enabled;
                    this.schedule_config_save(cx);
                    if this.is_lazygit_surface_active() {
                        this.restart_lazygit_session(cx);
                    }
                    cx.notify();
                });
            })
            .into_any_element();
        let lazygit_mouse_herdr = herdr.clone();
        let lazygit_mouse_control = Toggle::new("settings-lazygit-mouse", surface)
            .checked(lazygit_config.mouse_events)
            .on_change(move |enabled, _, app| {
                lazygit_mouse_herdr.update(app, |this, cx| {
                    this.config.lazygit.mouse_events = enabled;
                    this.schedule_config_save(cx);
                    if this.is_lazygit_surface_active() {
                        this.restart_lazygit_session(cx);
                    }
                    cx.notify();
                });
            })
            .into_any_element();
        let lazygit_refresh_mode_herdr = herdr.clone();
        let lazygit_auto_refresh_control = Toggle::new("settings-lazygit-auto-refresh", surface)
            .checked(lazygit_config.auto_refresh)
            .on_change(move |enabled, _, app| {
                lazygit_refresh_mode_herdr.update(app, |this, cx| {
                    this.config.lazygit.auto_refresh = enabled;
                    this.schedule_config_save(cx);
                    if this.is_lazygit_surface_active() {
                        this.restart_lazygit_session(cx);
                    }
                    cx.notify();
                });
            })
            .into_any_element();
        let lazygit_width_herdr = herdr.clone();
        let lazygit_width_control = Segmented::new("settings-lazygit-side-panel-width", surface)
            .option(20_u8, "20%")
            .option(25_u8, "25%")
            .option(33_u8, "33%")
            .option(40_u8, "40%")
            .value(lazygit_config.side_panel_width)
            .on_change(move |value, _, app| {
                lazygit_width_herdr.update(app, |this, cx| {
                    this.config.lazygit.side_panel_width = *value;
                    this.schedule_config_save(cx);
                    if this.is_lazygit_surface_active() {
                        this.restart_lazygit_session(cx);
                    }
                    cx.notify();
                });
            })
            .into_any_element();
        let lazygit_status_control = h_flex()
            .min_w_0()
            .gap(SPACE_ICON)
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .text_size(theme::FONT_META)
                    .text_color(content_theme.muted)
                    .truncate()
                    .child(lazygit_status),
            )
            .child(lazygit_refresh_control)
            .into_any_element();
        let lazygit_executable_control = self
            .lazygit_executable_input
            .as_ref()
            .map(|input| {
                Input::new(input)
                    .small()
                    .appearance(false)
                    .w_full()
                    .text_size(crate::theme::FONT_META)
                    .into_any_element()
            })
            .unwrap_or_else(|| div().into_any_element());
        let lazygit_status_card = settings_card(
            surface,
            vec![
                settings_card_row(
                    "Lazygit CLI",
                    "Shardlane resolves one executable and hosts it only while the Lazygit surface is visible.",
                    lazygit_status_control,
                ),
                settings_card_row(
                    "Executable",
                    "Leave empty to use PATH, or enter an absolute executable path. Press Enter or leave the field to apply.",
                    lazygit_executable_control,
                ),
            ],
        );
        let lazygit_startup_card = settings_card(
            surface,
            vec![
                settings_card_row(
                    "Startup panel",
                    "The first Lazygit panel shown for a new Project session.",
                    lazygit_panel_control,
                ),
                settings_card_row(
                    "Screen mode",
                    "Initial focused-panel size passed to Lazygit.",
                    lazygit_screen_control,
                ),
            ],
        );
        let lazygit_interface_card = settings_card(
            surface,
            vec![
                settings_card_row(
                    "Mouse events",
                    "Forward mouse input through the hosted PTY when Lazygit enables it.",
                    lazygit_mouse_control,
                ),
                settings_card_row(
                    "Side panel width",
                    "Small overlay preference for narrow right-panel layouts.",
                    lazygit_width_control,
                ),
            ],
        );
        let lazygit_git_card = settings_card(
            surface,
            vec![settings_card_row(
                "Auto refresh",
                "Keep Lazygit status updates enabled while the surface is visible.",
                lazygit_auto_refresh_control,
            )],
        );
        let lazygit_config_card = settings_card(
            surface,
            vec![
                settings_card_row(
                    "Shardlane overlay",
                    "Write a disposable runtime overlay for hosted presentation only; user and repository config files remain untouched.",
                    lazygit_integration_control,
                ),
                settings_card_row(
                    "Configuration ownership",
                    "Lazygit keeps owning user and repository configuration. Shardlane only supplies the small runtime overlay when integration is enabled.",
                    div()
                        .min_w_0()
                        .text_size(theme::FONT_META)
                        .text_color(content_theme.muted)
                        .child("User + repository config precedence preserved")
                        .into_any_element(),
                ),
            ],
        );

        div()
            .size_full()
            .overflow_y_scrollbar()
            .bg(background)
            .text_color(foreground)
            .child(
                // Settings content column: max_w 760 centered, px32, page title 18 MEDIUM pt2;
                // one raised card per section (settings_card) with hairline separators between rows.
                // min_w_0 + overflow_hidden keep the 760 cap from being broken by the min-content width
                // of long copy (flex auto min-size becomes zero here) — the Terminal card once overflowed
                // the window horizontally for this reason.
                div()
                    .max_w(px(760.0))
                    .w_full()
                    .min_w_0()
                    .overflow_hidden()
                    .mx_auto()
                    .px(px(32.0))
                    .pb(px(48.0))
                    .flex()
                    .flex_col()
                    .when(selected_provider.is_none(), |page| {
                        page.child(
                            div()
                                .pt(px(2.0))
                                .flex_none()
                                .text_size(crate::theme::FONT_APP_TITLE)
                                .font_weight(FontWeight::MEDIUM)
                                .child(settings_section_title_label(selected_section)),
                        )
                    })
                    .when(compact_layout && selected_provider.is_none(), |page| {
                        page.child(compact_nav)
                    })
                    .child(
                        theme_card.when(selected_section != SettingsSection::Appearance, |card| {
                            card.hidden()
                        }),
                    )
                    .child(
                        language_card
                            .when(selected_section != SettingsSection::Appearance, |card| {
                                card.hidden()
                            }),
                    )
                    .child(
                        herdr_tui_card
                            .when(selected_section != SettingsSection::Terminal, |card| {
                                card.hidden()
                            }),
                    )
                    .child(
                        terminal_card.when(selected_section != SettingsSection::Terminal, |card| {
                            card.hidden()
                        }),
                    )
                    .child(
                        lazygit_status_card
                            .when(selected_section != SettingsSection::Lazygit, |card| {
                                card.hidden()
                            }),
                    )
                    .child(
                        lazygit_startup_card
                            .when(selected_section != SettingsSection::Lazygit, |card| {
                                card.hidden()
                            }),
                    )
                    .child(
                        lazygit_interface_card
                            .when(selected_section != SettingsSection::Lazygit, |card| {
                                card.hidden()
                            }),
                    )
                    .child(
                        lazygit_git_card
                            .when(selected_section != SettingsSection::Lazygit, |card| {
                                card.hidden()
                            }),
                    )
                    .child(
                        lazygit_config_card
                            .when(selected_section != SettingsSection::Lazygit, |card| {
                                card.hidden()
                            }),
                    )
                    .child(
                        behavior_card.when(selected_section != SettingsSection::Behavior, |card| {
                            card.hidden()
                        }),
                    )
                    .child(
                        window_card.when(selected_section != SettingsSection::Window, |card| {
                            card.hidden()
                        }),
                    )
                    .child(
                        providers_content
                            .when(selected_section != SettingsSection::Providers, |card| {
                                card.hidden()
                            }),
                    )
                    .when(selected_section == SettingsSection::Shortcuts, |page| {
                        page.child(self.render_shortcuts_settings(window, cx))
                    })
                    .when(selected_section == SettingsSection::Mobile, |page| {
                        page.child(self.mobile_settings_content(window, cx))
                    })
                    .when(selected_section == SettingsSection::Browser, |page| {
                        page.child(self.render_browser_settings(window, cx))
                    })
                    .when(selected_section == SettingsSection::Skill, |page| {
                        page.child(self.skill_settings_content(window, cx))
                    }),
            )
            .into_any_element()
    }
}

/// Page title: the current settings section name (18 MEDIUM).
fn settings_section_title_label(section: SettingsSection) -> SharedString {
    section.label()
}

/// Card header: 13.5 MEDIUM primary title + 12.5 muted subtitle (lh18).
pub(crate) fn settings_section_title(title: &str, subtitle: &str) -> impl IntoElement {
    div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(5.0))
        .child(
            div()
                .text_size(crate::theme::FONT_SECTION_TITLE)
                .font_weight(FontWeight::MEDIUM)
                .child(title.to_string()),
        )
        .child(
            div()
                .w_full()
                .min_w_0()
                .text_size(crate::theme::FONT_BODY)
                .line_height(px(18.0))
                .opacity(0.72)
                .whitespace_normal()
                .child(subtitle.to_string()),
        )
}

/// SettingsCard: mt15, r13, raised background (7% foreground wash) group container;
/// rows come from [`settings_card_row`] with an mx20 1px hairline between rows (mirroring mx-5 border-t).
pub(crate) fn settings_card(surface: ControlSurface, rows: Vec<AnyElement>) -> gpui::Div {
    let mut card = div()
        .mt(px(15.0))
        .w_full()
        .min_w_0()
        .rounded(px(13.0))
        .bg(surface.accent())
        .overflow_hidden()
        .flex()
        .flex_col();
    for (ix, row) in rows.into_iter().enumerate() {
        if ix > 0 {
            card = card.child(
                div()
                    .mx(px(20.0))
                    .border_t(px(1.0))
                    .border_color(surface.hairline()),
            );
        }
        card = card.child(row);
    }
    card
}

/// Settings card row: px20/py12; title + wrappable description fill the card width, and the control
/// gets its own right-aligned row so long copy (like Herdr's) can't push a Segmented/Toggle out of
/// the card (notate 2026-08-29).
pub(crate) fn settings_card_row(title: &str, detail: &str, control: AnyElement) -> AnyElement {
    div()
        .w_full()
        .min_w_0()
        // Skill-panel audit (2026-09-06): percentage max/width constraints fail to resolve
        // across this card row chain (a 250-char detail still measured its full single-line
        // width and pushed the control row past the card), so the row carries an absolute
        // cap equal to the 760px column content width (760 - 2*32 page padding - 2*20 row
        // padding = 656). It only caps over-long intrinsic text; short rows are unaffected.
        .max_w(px(656.0))
        .min_h(px(60.0))
        .px(px(20.0))
        .py(px(12.0))
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(5.0))
                .child(
                    div()
                        .text_size(crate::theme::FONT_SECTION_TITLE)
                        .font_weight(FontWeight::MEDIUM)
                        .whitespace_normal()
                        .child(title.to_string()),
                )
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .max_w_full()
                        // Audit (2026-09-06, Skill panel): a text leaf's intrinsic width still
                        // contributes its full single-line width to the flex min-content chain,
                        // so a long detail (>~110 chars) could push the whole 760px settings
                        // column past the window and clip the right-aligned controls.
                        // overflow_hidden zeroes the automatic min size (Taffy) so the text
                        // wraps inside the column instead of stretching it.
                        .overflow_hidden()
                        .text_size(crate::theme::FONT_BODY)
                        .line_height(px(18.0))
                        .opacity(0.72)
                        .whitespace_normal()
                        .child(detail.to_string()),
                ),
        )
        .child(
            // notate 08-29 second round: `justify_end` doesn't push a bare control to the right edge
            // on this container (observed on the Mobile page: a Toggle stuck at 60% width while
            // justify_between rows of the same card hugged right correctly) — using an empty lead +
            // justify_between is the proven right-hugging approach.
            div()
                .w_full()
                .min_w_0()
                .flex()
                .justify_between()
                .overflow_hidden()
                .child(div())
                .child(control),
        )
        .into_any_element()
}
