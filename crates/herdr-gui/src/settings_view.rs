//! Settings page presentation layer: settings group cards, control rows, and theme preset cards.
//!
//! Theme page semantics (2026-08-29): first pick light/dark (Light/Dark/Auto segment), then pick
//! that appearance's official Herdr theme card; Auto shows both Light/Dark choice groups. The rest
//! of the Herdr page merged into the Terminal page's Herdr TUI card (user-wise it all lives in
//! Terminal); the Herdr section was deleted.
//!
//! i18n (2026-09-12): every Settings page's copy is served through `crate::i18n::t()`/
//! `t_with()` (five locales: en/zh-CN/zh-TW/ja/ko) and the Language card switches
//! `settings::Language`.
//!
//! [INPUT]: Depends on `super` (main.rs)'s ShardlaneApp state, the settings/config models,
//! theme presets, font_catalog (monospace font enumeration), gpui-component controls,
//! and the ui::controls form control family
//! [OUTPUT]: Exposes `ShardlaneApp::settings_page` (with the private settings_card/settings_card_row/
//! settings_section_title helpers) and the Herdr TUI card's inline red apply-failure
//! notice (`herdr_config_notice`; the card's controls keep their disk-truth values so
//! a rejected write never strands user input)
//! [POS]: One of main.rs's presentation-layer splits (sibling of search_view.rs/header_view.rs);
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
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
        let danger = cx.theme().danger;

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
                i18n::t("settings.option.auto"),
            )
            .option(
                crate::settings::TerminalCursorStylePreference::Block,
                i18n::t("settings.terminal.cursor_block"),
            )
            .option(
                crate::settings::TerminalCursorStylePreference::Bar,
                i18n::t("settings.terminal.cursor_bar"),
            )
            .option(
                crate::settings::TerminalCursorStylePreference::Underline,
                i18n::t("settings.terminal.cursor_underline"),
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
                i18n::t("settings.option.auto"),
            )
            .option(
                crate::settings::TerminalCursorBlinkPreference::On,
                i18n::t("settings.terminal.cursor_blink_on"),
            )
            .option(
                crate::settings::TerminalCursorBlinkPreference::Off,
                i18n::t("settings.terminal.cursor_blink_steady"),
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
            .label(i18n::t("settings.herdr_tui.reload"))
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
            .option("compact".to_string(), i18n::t("settings.option.compact"))
            .option("hidden".to_string(), i18n::t("settings.option.hidden"))
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

        let tab_position_herdr = herdr.clone();
        let tab_position_control = Segmented::new("settings-herdr-tab-position", surface)
            .option("top".to_string(), i18n::t("settings.option.top"))
            .option("bottom".to_string(), i18n::t("settings.option.bottom"))
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
            .option("dots".to_string(), i18n::t("settings.option.dots"))
            .option("symbols".to_string(), i18n::t("settings.option.symbols"))
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
            SettingsSection::visible_sections()
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
                    &i18n::t("settings.herdr_tui.title"),
                    &i18n::t("settings.herdr_tui.subtitle"),
                    h_flex().into_any_element(),
                ),
                settings_card_row(
                    &i18n::t("settings.herdr_tui.config_file"),
                    &herdr_config_path,
                    herdr_reload_control,
                ),
                settings_card_row(
                    &i18n::t("settings.herdr_tui.copy_on_select"),
                    &i18n::t("settings.herdr_tui.copy_on_select_detail"),
                    herdr_copy_control,
                ),
                settings_card_row(
                    &i18n::t("settings.herdr_tui.mouse_capture"),
                    &i18n::t("settings.herdr_tui.mouse_capture_detail"),
                    mouse_capture_control,
                ),
                settings_card_row(
                    &i18n::t("settings.herdr_tui.scroll_lines"),
                    &i18n::t("settings.herdr_tui.scroll_lines_detail"),
                    herdr_scroll_lines_control,
                ),
                settings_card_row(
                    &i18n::t("settings.herdr_tui.sidebar_start_collapsed"),
                    &i18n::t("settings.herdr_tui.sidebar_start_collapsed_detail"),
                    sidebar_start_control,
                ),
                settings_card_row(
                    &i18n::t("settings.herdr_tui.sidebar_collapsed_mode"),
                    &i18n::t("settings.herdr_tui.sidebar_collapsed_mode_detail"),
                    sidebar_mode_control,
                ),
                settings_card_row(
                    &i18n::t("settings.herdr_tui.pane_borders"),
                    &i18n::t("settings.herdr_tui.pane_borders_detail"),
                    pane_borders_control,
                ),
                settings_card_row(
                    &i18n::t("settings.herdr_tui.pane_outer_borders"),
                    &i18n::t("settings.herdr_tui.pane_outer_borders_detail"),
                    pane_outer_borders_control,
                ),
                settings_card_row(
                    &i18n::t("settings.herdr_tui.pane_scrollbars"),
                    &i18n::t("settings.herdr_tui.pane_scrollbars_detail"),
                    pane_scrollbars_control,
                ),
                settings_card_row(
                    &i18n::t("settings.herdr_tui.pane_gaps"),
                    &i18n::t("settings.herdr_tui.pane_gaps_detail"),
                    pane_gaps_control,
                ),
                settings_card_row(
                    &i18n::t("settings.herdr_tui.tab_position"),
                    &i18n::t("settings.herdr_tui.tab_position_detail"),
                    tab_position_control,
                ),
                settings_card_row(
                    &i18n::t("settings.herdr_tui.status_indicators"),
                    &i18n::t("settings.herdr_tui.status_indicators_detail"),
                    indicators_control,
                ),
            ],
        );
        // Spec #1 (2026-09-19): the most recent Herdr config apply/reload failure,
        // inline and red under the card. The controls above always render the
        // on-disk truth (`herdr_user_config` only changes on success), so a
        // rejected write leaves the user's original settings editable in place.
        let herdr_config_notice = self.herdr_config_notice.clone().map(|message| {
            div()
                .mt(px(10.0))
                .text_size(crate::theme::FONT_META)
                .text_color(danger)
                .child(format!("⚠ {message}"))
        });

        // TUI-only cutover: the normal work surface is always the single hosted Herdr TUI, with no
        // Embedded/TUI mode selection; the Terminal card keeps appearance preferences + host restore
        // state/actions (plan §18).
        let mut terminal_rows = vec![settings_card_row(
            &i18n::t("settings.terminal.title"),
            &i18n::t("settings.terminal.subtitle"),
            h_flex().into_any_element(),
        )];
        {
            let tui_status = self.tui_host.status;
            let tui_status_label = tui_status.label();
            let tui_status_detail =
                self.tui_host
                    .last_error
                    .clone()
                    .unwrap_or_else(|| match tui_status {
                        crate::herdr_tui::HerdrTuiHostStatus::Stopped => {
                            i18n::t("settings.terminal.status_stopped_detail").to_string()
                        }
                        crate::herdr_tui::HerdrTuiHostStatus::Starting => {
                            i18n::t("settings.terminal.status_starting_detail").to_string()
                        }
                        crate::herdr_tui::HerdrTuiHostStatus::Running => {
                            i18n::t("settings.terminal.status_running_detail").to_string()
                        }
                        crate::herdr_tui::HerdrTuiHostStatus::Failed => {
                            i18n::t("settings.terminal.status_failed_detail").to_string()
                        }
                    });
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
                &i18n::t("settings.terminal.host_status"),
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
                        .label(i18n::t("settings.terminal.restart"))
                        .on_click(move |_, window, app| {
                            restart_herdr.update(app, |this, cx| {
                                this.restart_tui_surface(window, cx);
                            });
                        }),
                )
                .into_any_element();
            terminal_rows.push(settings_card_row(
                &i18n::t("settings.terminal.host_actions"),
                &i18n::t("settings.terminal.host_actions_detail"),
                tui_actions,
            ));
        }
        terminal_rows.extend([
            settings_card_row(
                &i18n::t("settings.terminal.font_family"),
                &i18n::t("settings.terminal.font_family_detail"),
                font_control,
            ),
            settings_card_row(
                &i18n::t("settings.terminal.font_size"),
                // Audit E23: surface the default and the hidden ⌘0 reset next to the value.
                &i18n::t_with(
                    "settings.terminal.font_size_detail",
                    &[
                        ("size", format!("{font_size:.0}")),
                        (
                            "default",
                            format!("{:.0}", settings::TerminalConfig::default().font_size),
                        ),
                    ],
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
                &i18n::t("settings.terminal.line_height"),
                &i18n::t_with(
                    "settings.terminal.line_height_detail",
                    &[
                        ("height", format!("{line_height:.0}")),
                        (
                            "default",
                            format!("{:.0}", settings::TerminalConfig::default().line_height),
                        ),
                    ],
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
                &i18n::t("settings.terminal.padding"),
                &i18n::t_with(
                    "settings.terminal.padding_detail",
                    &[
                        ("padding", format!("{terminal_padding:.0}")),
                        (
                            "default",
                            format!("{:.0}", settings::TerminalConfig::default().padding),
                        ),
                    ],
                ),
                padding_control,
            ),
            settings_card_row(
                &i18n::t("settings.terminal.cursor_style"),
                &i18n::t("settings.terminal.cursor_style_detail"),
                cursor_style_control,
            ),
            settings_card_row(
                &i18n::t("settings.terminal.cursor_blink"),
                &i18n::t("settings.terminal.cursor_blink_detail"),
                cursor_blink_control,
            ),
        ]);

        let terminal_card = settings_card(surface, terminal_rows);

        let behavior_card = settings_card(
            surface,
            vec![settings_card_row(
                &i18n::t("settings.terminal.agent_notifications"),
                &i18n::t("settings.terminal.agent_notifications_detail"),
                notifications_control,
            )],
        );

        // Auto updates: the toggles are the user-facing switches (persisted
        // to the on-disk config the checker loop re-reads); the status row
        // projects the install state machine — the check finds a release,
        // the installer downloads, verifies and stages it, and the action
        // button swaps the bundle and relaunches. Dev builds (no .app
        // ancestor) degrade to the old open-the-releases-page behavior.
        let updates_check = self.config.updates.check_enabled;
        let updates_toggle_herdr = herdr.clone();
        let updates_toggle = Toggle::new("settings-updates-check", surface)
            .checked(updates_check)
            .on_change(move |checked, _, app| {
                updates_toggle_herdr.update(app, |this, cx| {
                    this.config.updates.check_enabled = checked;
                    this.save_config();
                    cx.notify();
                });
            })
            .into_any_element();

        let updates_auto_herdr = herdr.clone();
        let updates_auto_toggle = Toggle::new("settings-updates-auto-download", surface)
            .checked(self.config.updates.auto_download)
            .on_change(move |checked, _, app| {
                updates_auto_herdr.update(app, |this, cx| {
                    this.config.updates.auto_download = checked;
                    this.save_config();
                    cx.notify();
                });
            })
            .into_any_element();

        let up_to_date = i18n::t_with(
            "settings.behavior.updates.up_to_date",
            &[
                ("version", env!("CARGO_PKG_VERSION").to_string()),
                ("hours", self.config.updates.interval_hours.to_string()),
            ],
        )
        .to_string();
        // One factory for both the first download and a retry after failure;
        // both read the same recorded manifest.
        let download_action = |id: &'static str, label: gpui::SharedString| {
            let manifest = crate::update_check::last_newer()?;
            let herdr = herdr.downgrade();
            Some(
                Button::new(id)
                    .custom(content_button)
                    .xsmall()
                    .label(label)
                    .on_click(move |_, _, app| {
                        if let Err(error) = crate::update_install::begin_staged_download(
                            manifest.clone(),
                            Some(herdr.clone()),
                            app,
                        ) {
                            crate::notifications::show(
                                &i18n::t("settings.behavior.updates.update_failed"),
                                &error,
                            );
                        }
                    })
                    .into_any_element(),
            )
        };

        let (updates_status_detail, updates_action): (String, AnyElement) =
            if !crate::update_install::can_install() {
                match crate::update_check::last_newer() {
                    Some(latest) => (
                        i18n::t_with(
                            "settings.behavior.updates.available_dev",
                            &[("version", latest.version.clone())],
                        )
                        .to_string(),
                        {
                            let url = latest.asset_for_current_platform().url;
                            Button::new("settings-updates-releases")
                                .custom(content_button)
                                .xsmall()
                                .label(i18n::t("settings.behavior.updates.releases"))
                                .on_click(move |_, _, cx| {
                                    cx.open_url(&url);
                                })
                                .into_any_element()
                        },
                    ),
                    None => (up_to_date, div().into_any_element()),
                }
            } else {
                match crate::update_install::install_state() {
                    crate::update_install::InstallState::Downloading {
                        version,
                        downloaded_bytes,
                        total_bytes,
                    } => (
                        i18n::t_with(
                            "settings.behavior.updates.downloading",
                            &[
                                ("version", version.clone()),
                                (
                                    "progress",
                                    crate::update_install::format_progress(
                                        downloaded_bytes,
                                        total_bytes,
                                    ),
                                ),
                            ],
                        )
                        .to_string(),
                        div().into_any_element(),
                    ),
                    crate::update_install::InstallState::Ready { version } => (
                        i18n::t_with(
                            "settings.behavior.updates.ready",
                            &[("version", version.clone())],
                        )
                        .to_string(),
                        {
                            let herdr = herdr.downgrade();
                            Button::new("settings-updates-restart")
                                .custom(content_button)
                                .xsmall()
                                .label(i18n::t("settings.behavior.updates.update_restart"))
                                .on_click(move |_, _, app| {
                                    if let Err(error) = crate::update_install::install_and_restart()
                                    {
                                        crate::notifications::show(
                                            &i18n::t("settings.behavior.updates.update_failed"),
                                            &error,
                                        );
                                        if let Some(herdr) = herdr.upgrade() {
                                            herdr.update(app, |_, cx| cx.notify());
                                        }
                                    }
                                })
                                .into_any_element()
                        },
                    ),
                    crate::update_install::InstallState::Failed { version, error } => (
                        i18n::t_with(
                            "settings.behavior.updates.failed",
                            &[("version", version.clone()), ("error", error.clone())],
                        )
                        .to_string(),
                        download_action(
                            "settings-updates-retry",
                            i18n::t("settings.behavior.updates.retry_download"),
                        )
                        .unwrap_or_else(|| div().into_any_element()),
                    ),
                    crate::update_install::InstallState::Idle => {
                        match crate::update_check::last_newer() {
                            Some(latest) => (
                                i18n::t_with(
                                    "settings.behavior.updates.available",
                                    &[("version", latest.version.clone())],
                                )
                                .to_string(),
                                download_action(
                                    "settings-updates-download",
                                    i18n::t("settings.behavior.updates.download_update"),
                                )
                                .unwrap_or_else(|| div().into_any_element()),
                            ),
                            None => (up_to_date, div().into_any_element()),
                        }
                    }
                }
            };

        let updates_card = settings_card(
            surface,
            vec![
                settings_card_row(
                    &i18n::t("settings.behavior.updates.check"),
                    &i18n::t("settings.behavior.updates.check_detail"),
                    updates_toggle,
                ),
                settings_card_row(
                    &i18n::t("settings.behavior.updates.auto_download"),
                    &i18n::t("settings.behavior.updates.auto_download_detail"),
                    updates_auto_toggle,
                ),
                settings_card_row(
                    &i18n::t("settings.behavior.updates.latest"),
                    &updates_status_detail,
                    updates_action,
                ),
            ],
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
        let window_card = settings_card(
            surface,
            vec![
                settings_card_row(
                    &i18n::t("settings.window.opacity"),
                    // Audit E23: annotate the default next to the current value.
                    &i18n::t_with(
                        "settings.window.opacity_detail",
                        &[
                            ("value", format!("{:.0}", opacity * 100.0)),
                            (
                                "default",
                                format!("{:.0}", settings::WindowConfig::default().opacity * 100.0),
                            ),
                        ],
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
                    &i18n::t("settings.window.always_on_top"),
                    &i18n::t("settings.window.always_on_top_detail"),
                    always_on_top_control,
                ),
            ],
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
                crate::right_panel::lazygit::LazygitCompatibility::Supported => i18n::t_with(
                    "settings.lazygit.supported",
                    &[("detail", detection.detail.clone())],
                )
                .to_string(),
                crate::right_panel::lazygit::LazygitCompatibility::Outdated => i18n::t_with(
                    "settings.lazygit.outdated",
                    &[("detail", detection.detail.clone())],
                )
                .to_string(),
                crate::right_panel::lazygit::LazygitCompatibility::Missing => {
                    detection.detail.clone()
                }
            })
            .unwrap_or_else(|| i18n::t("settings.lazygit.detecting").to_string());
        let lazygit_refresh_herdr = herdr.clone();
        let lazygit_refresh_control = Button::new("settings-lazygit-refresh")
            .custom(content_button)
            .xsmall()
            .label(i18n::t("settings.lazygit.refresh"))
            .on_click(move |_, _, app| {
                lazygit_refresh_herdr.update(app, |this, cx| {
                    this.refresh_lazygit_detection(cx);
                });
            })
            .into_any_element();
        let lazygit_panel_herdr = herdr.clone();
        let lazygit_panel_control = Segmented::new("settings-lazygit-startup-panel", surface)
            .option(
                crate::settings::LazygitStartupPanel::Status,
                i18n::t("settings.lazygit.panel_status"),
            )
            .option(
                crate::settings::LazygitStartupPanel::Branch,
                i18n::t("settings.lazygit.panel_branch"),
            )
            .option(
                crate::settings::LazygitStartupPanel::Log,
                i18n::t("settings.lazygit.panel_log"),
            )
            .option(
                crate::settings::LazygitStartupPanel::Stash,
                i18n::t("settings.lazygit.panel_stash"),
            )
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
            .option(
                crate::settings::LazygitScreenMode::Normal,
                i18n::t("settings.lazygit.screen_normal"),
            )
            .option(
                crate::settings::LazygitScreenMode::Half,
                i18n::t("settings.lazygit.screen_half"),
            )
            .option(
                crate::settings::LazygitScreenMode::Full,
                i18n::t("settings.lazygit.screen_full"),
            )
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
                    &i18n::t("settings.lazygit.cli"),
                    &i18n::t("settings.lazygit.cli_detail"),
                    lazygit_status_control,
                ),
                settings_card_row(
                    &i18n::t("settings.lazygit.executable"),
                    &i18n::t("settings.lazygit.executable_detail"),
                    lazygit_executable_control,
                ),
            ],
        );
        let lazygit_startup_card = settings_card(
            surface,
            vec![
                settings_card_row(
                    &i18n::t("settings.lazygit.startup_panel"),
                    &i18n::t("settings.lazygit.startup_panel_detail"),
                    lazygit_panel_control,
                ),
                settings_card_row(
                    &i18n::t("settings.lazygit.screen_mode"),
                    &i18n::t("settings.lazygit.screen_mode_detail"),
                    lazygit_screen_control,
                ),
            ],
        );
        let lazygit_interface_card = settings_card(
            surface,
            vec![
                settings_card_row(
                    &i18n::t("settings.lazygit.mouse_events"),
                    &i18n::t("settings.lazygit.mouse_events_detail"),
                    lazygit_mouse_control,
                ),
                settings_card_row(
                    &i18n::t("settings.lazygit.side_width"),
                    &i18n::t("settings.lazygit.side_width_detail"),
                    lazygit_width_control,
                ),
            ],
        );
        let lazygit_git_card = settings_card(
            surface,
            vec![settings_card_row(
                &i18n::t("settings.lazygit.auto_refresh"),
                &i18n::t("settings.lazygit.auto_refresh_detail"),
                lazygit_auto_refresh_control,
            )],
        );
        let lazygit_config_card = settings_card(
            surface,
            vec![
                settings_card_row(
                    &i18n::t("settings.lazygit.overlay"),
                    &i18n::t("settings.lazygit.overlay_detail"),
                    lazygit_integration_control,
                ),
                settings_card_row(
                    &i18n::t("settings.lazygit.config_ownership"),
                    &i18n::t("settings.lazygit.config_ownership_detail"),
                    div()
                        .min_w_0()
                        .text_size(theme::FONT_META)
                        .text_color(content_theme.muted)
                        .child(i18n::t("settings.lazygit.precedence_note"))
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
                    .children(herdr_config_notice.map(|notice| {
                        notice.when(selected_section != SettingsSection::Terminal, |note| {
                            note.hidden()
                        })
                    }))
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
                        updates_card.when(selected_section != SettingsSection::Behavior, |card| {
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
                    .when(
                        selected_section == SettingsSection::Mobile
                            && crate::mobile_view::mobile_surface_enabled(),
                        |page| page.child(self.mobile_settings_content(window, cx)),
                    )
                    .when(selected_section == SettingsSection::Browser, |page| {
                        page.child(self.render_browser_settings(window, cx))
                    })
                    .when(selected_section == SettingsSection::AgentHooks, |page| {
                        page.child(self.agent_hooks_settings_content(window, cx))
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
