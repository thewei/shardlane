//! [INPUT]: Depends on the ShardlaneApp type from the crate root (super) and existing types/imports (use super::*); no independent external dependencies.
//! [OUTPUT]: Exposes ShardlaneApp's unified App + Herdr theme switching, hosted terminal
//! emulator dynamic color application (`apply_hosted_terminal_theme_background`/
//! `hosted_terminal_colors_for_window`), and terminal rendering settings (inherent impl shard).
//! [POS]: The `crates/herdr-gui` shell theme responsibility domain, mechanically split out of main.rs; together with sibling shell_* modules it forms ShardlaneApp's method surface.
use super::*;

impl ShardlaneApp {
    pub(super) fn sync_app_theme_from_herdr(&mut self, cx: &mut Context<Self>) {
        // Audit A18: the single resolution rule shared with bootstrap and theme(); `true`
        // anchors the scheme on the dark slot under auto switch (this helper's original rule).
        let Some(preset) = resolved_theme_preset(&self.herdr_user_config, true) else {
            lag_log(format_args!(
                "herdr theme {} has no Shardlane chrome preset; preserving current app scheme",
                self.herdr_user_config.theme_name
            ));
            return;
        };
        self.config.ui.appearance = if self.herdr_user_config.theme_auto_switch {
            "system".to_string()
        } else {
            preset.appearance.to_string()
        };
        self.config.ui.color_scheme = preset.scheme.to_string();
        self.theme_mode = theme_mode_from_config(&self.config.ui.appearance);
        self.apply_native_window_preferences(cx);
    }

    /// Theme change landing point for the hosted work surface. Shardlane is the hosted
    /// terminal emulator: seed the theme-derived dynamic colors into the live model
    /// (frames re-resolve immediately) and hold the pane fill until the restarted
    /// Herdr TUI's first authoritative frame arrives.
    pub(super) fn apply_hosted_terminal_theme_background(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let theme = self.theme(window);
        let colors = self.hosted_terminal_colors_for_window(window);
        self.hosted_terminal_colors = Some(colors);
        if let Some(terminal) = self.terminal.clone() {
            if let Ok(mut managed) = terminal.try_lock() {
                managed.set_dynamic_colors(colors.0, colors.1);
            }
        }
        if let Some(terminal) = self.lazygit_terminal() {
            if let Ok(mut managed) = terminal.try_lock() {
                managed.set_dynamic_colors(colors.0, colors.1);
            }
        }
        self.terminal_pane.update(cx, |pane, cx| {
            pane.set_placeholder_background(theme.terminal, cx);
        });
        self.lazygit_session.pane.update(cx, |pane, cx| {
            pane.set_placeholder_background(theme.terminal, cx)
        });
        cx.notify();
    }

    /// Dynamic default colors (foreground, background) reported to the hosted Herdr
    /// TUI child and seeded into the terminal model. Both derive from the active
    /// Herdr official theme — the single Terminal color authority.
    pub(super) fn hosted_terminal_colors_for_window(&self, window: &Window) -> (u32, u32) {
        let theme = self.theme(window);
        (theme.text, theme.terminal)
    }

    pub(super) fn hosted_terminal_background_rgb_from_theme(&self, theme: UiTheme) -> u32 {
        self.terminal_frame
            .surface_background
            .or(self.terminal_frame.default_background)
            .unwrap_or(theme.terminal)
    }

    pub(super) fn apply_terminal_render_settings(&mut self, cx: &mut Context<Self>) {
        let geometry = TerminalGeometry::resolve(
            &self.config.terminal.font_family,
            self.config.terminal.font_size,
            self.config.terminal.line_height,
            cx,
        );
        if self.terminal_geometry != geometry {
            lag_log(format_args!(
                "terminal.geometry font={} size={:.1} cell={:.3}x{:.1}",
                geometry.font_family, geometry.font_size, geometry.cell_width, geometry.cell_height,
            ));
        }
        let padding = self.terminal_content_padding();
        let cursor_style =
            terminal_view::terminal_cursor_style_override(self.config.terminal.cursor_style);
        let cursor_blink =
            terminal_view::terminal_cursor_blink_override(self.config.terminal.cursor_blink);
        self.terminal_geometry = geometry.clone();
        self.terminal_surface_size = None;
        self.terminal_pane.update(cx, |pane, cx| {
            pane.set_render_settings(geometry.clone(), padding, 1.0, cx);
            pane.set_cursor_preferences(cursor_style, cursor_blink, cx);
        });
        self.lazygit_session.pane.update(cx, |pane, cx| {
            pane.set_render_settings(geometry, padding, 1.0, cx);
            pane.set_cursor_preferences(cursor_style, cursor_blink, cx);
        });
    }

    pub(super) fn set_theme(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(preset) = theme::THEME_PRESETS
            .iter()
            .find(|preset| preset.herdr_theme == name)
            .copied()
        else {
            lag_log(format_args!(
                "theme action {name} has no unified App + Herdr preset"
            ));
            return;
        };
        self.apply_herdr_user_config_update_and_restart(
            herdr_tui::HerdrUserConfigUpdate::ThemeName(preset.herdr_theme.to_string()),
            window,
            cx,
        );
    }

    set_theme!(theme_catppuccin, ThemeCatppuccin, "catppuccin");
    set_theme!(
        theme_catppuccin_latte,
        ThemeCatppuccinLatte,
        "catppuccin-latte"
    );
    set_theme!(theme_tokyo_night, ThemeTokyoNight, "tokyo-night");
    set_theme!(theme_tokyo_night_day, ThemeTokyoNightDay, "tokyo-night-day");
    set_theme!(theme_dracula, ThemeDracula, "dracula");
    set_theme!(theme_nord, ThemeNord, "nord");
    set_theme!(theme_gruvbox, ThemeGruvbox, "gruvbox");
    set_theme!(theme_gruvbox_light, ThemeGruvboxLight, "gruvbox-light");
    set_theme!(theme_one_dark, ThemeOneDark, "one-dark");
    set_theme!(theme_one_light, ThemeOneLight, "one-light");
    set_theme!(theme_solarized, ThemeSolarized, "solarized");
    set_theme!(
        theme_solarized_light,
        ThemeSolarizedLight,
        "solarized-light"
    );
    set_theme!(theme_kanagawa, ThemeKanagawa, "kanagawa");
    set_theme!(theme_kanagawa_lotus, ThemeKanagawaLotus, "kanagawa-lotus");
    set_theme!(theme_rose_pine, ThemeRosePine, "rose-pine");
    set_theme!(theme_rose_pine_dawn, ThemeRosePineDawn, "rose-pine-dawn");
    set_theme!(theme_vesper, ThemeVesper, "vesper");

    /// Theme page: switch the appearance mode (Light / Dark / Auto), keeping the
    /// effective per-appearance selections.
    pub(super) fn apply_theme_appearance_mode(
        &mut self,
        mode: herdr_tui::ThemeAppearanceMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (light, dark) = herdr_tui::effective_theme_selections(&self.herdr_user_config);
        self.apply_theme_scheme(mode, light, dark, window, cx);
    }

    /// Theme page: one theme card picked. Light cards set the light built-in and
    /// dark cards the dark one; the card never flips the appearance mode, so Auto
    /// keeps needing both selections.
    pub(super) fn apply_theme_card_selection(
        &mut self,
        preset: theme::ThemePreset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mode =
            herdr_tui::theme_appearance_mode(&self.herdr_user_config, &self.config.ui.appearance);
        let (mut light, mut dark) = herdr_tui::effective_theme_selections(&self.herdr_user_config);
        match preset.category {
            theme::ThemePresetCategory::Light => light = preset.herdr_theme.to_string(),
            theme::ThemePresetCategory::Dark => dark = preset.herdr_theme.to_string(),
        }
        self.apply_theme_scheme(mode, light, dark, window, cx);
    }

    fn apply_theme_scheme(
        &mut self,
        mode: herdr_tui::ThemeAppearanceMode,
        light: String,
        dark: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let update =
            herdr_tui::theme_scheme_update(mode, &light, &dark, &self.herdr_user_config.theme_name);
        self.apply_herdr_user_config_update_and_restart(update, window, cx);
    }

    pub(super) fn is_dark_appearance(&self, window: &Window) -> bool {
        match self.theme_mode {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => matches!(
                window.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
        }
    }

    pub(super) fn theme(&self, window: &Window) -> UiTheme {
        let dark = self.is_dark_appearance(window);
        // Audit A18: the same single resolution as bootstrap/sync_app_theme_from_herdr; the
        // live `dark` flag selects the per-appearance Herdr theme slot.
        if let Some(preset) = resolved_theme_preset(&self.herdr_user_config, dark) {
            return theme::theme_for_scheme(preset.scheme, dark);
        }
        theme::theme_for_scheme(&self.config.ui.color_scheme, dark)
    }
}
