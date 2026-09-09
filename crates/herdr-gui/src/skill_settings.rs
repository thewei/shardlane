//! Settings -> Skill panel: installs/uninstalls the bundled `shardlane`
//! skill into each Agent's skill root.
//!
//! [INPUT]: depends on the ShardlaneApp state in super (main.rs), the
//! settings_view card vocabulary, ui::controls ControlSurface, the
//! gpui-component Button, and the include_str! embedded
//! skill/shardlane/SKILL.md asset
//! [OUTPUT]: exposes ShardlaneApp::skill_settings_content (the Settings ->
//! Skill content column), the bundled skill asset constant BUNDLED_SKILL_MD,
//! and the testable install engine (skill_roots / read_skill_state /
//! install_skill / uninstall_skill)
//! [POS]: one split of the settings presentation layer (sibling of
//! mobile_view.rs); the install engine is pure fs logic and is tested
//! directly against temp roots; the CLI query surface lives in cli.rs and
//! SKILL.md is the semantic contract of both surfaces
//! [PROTOCOL]: Update this header on change, then check CLAUDE.md.

use std::path::{Path, PathBuf};

use super::*;
use crate::settings_view::{settings_card, settings_card_row};
use crate::ui::controls::ControlSurface;

// ============================================================
// Install engine: pure fs with no UI dependency; symlinked targets always
// refuse direct writes.
// ============================================================

/// The skill directory name equals the skill name (matching the herdr skill
/// directory convention).
pub(crate) const SKILL_DIR_NAME: &str = "shardlane";

/// The bundled skill asset: the only content the install panel writes to
/// disk; cli.rs's query contract shares this source.
pub(crate) const BUNDLED_SKILL_MD: &str = include_str!("../skill/shardlane/SKILL.md");

/// Canonical install roots: the agents/skillshare ecosystem, Codex, Claude.
/// All are shown; existence is never guessed.
pub(crate) fn skill_roots() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    [".agents/skills", ".codex/skills", ".claude/skills"]
        .iter()
        .map(|relative| home.join(relative))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SkillInstallState {
    /// Byte-identical to the bundled asset.
    Installed,
    /// Present with different content (older version or hand-edited);
    /// Install refreshes it to the bundled version.
    Outdated,
    Missing,
    /// Symlink: may point at an externally managed source such as a
    /// skillshare hub; never written directly.
    External,
}

fn skill_dir(root: &Path) -> PathBuf {
    root.join(SKILL_DIR_NAME)
}

fn skill_file(root: &Path) -> PathBuf {
    skill_dir(root).join("SKILL.md")
}

pub(crate) fn read_skill_state(root: &Path) -> SkillInstallState {
    if skill_dir(root).is_symlink() || skill_file(root).is_symlink() {
        return SkillInstallState::External;
    }
    match std::fs::read_to_string(skill_file(root)) {
        Ok(content) if content == BUNDLED_SKILL_MD => SkillInstallState::Installed,
        Ok(_) => SkillInstallState::Outdated,
        Err(_) => SkillInstallState::Missing,
    }
}

fn guard_externally_managed(root: &Path) -> Result<(), String> {
    if skill_dir(root).is_symlink() || skill_file(root).is_symlink() {
        return Err(format!(
            "{} is a symlink and is managed externally; not touched",
            skill_dir(root).display()
        ));
    }
    Ok(())
}

/// Writes (or refreshes) the bundled skill. On success the on-disk state is
/// re-read and returned so the UI projects facts directly.
pub(crate) fn install_skill(root: &Path) -> Result<SkillInstallState, String> {
    guard_externally_managed(root)?;
    let dir = skill_dir(root);
    std::fs::create_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let file = skill_file(root);
    std::fs::write(&file, BUNDLED_SKILL_MD)
        .map_err(|error| format!("{}: {error}", file.display()))?;
    Ok(read_skill_state(root))
}

/// Removes only this skill's own SKILL.md; the directory is removed only
/// when it becomes empty, sibling files are never touched. Returns whether
/// a file was actually removed.
pub(crate) fn uninstall_skill(root: &Path) -> Result<bool, String> {
    guard_externally_managed(root)?;
    let file = skill_file(root);
    if !file.exists() {
        return Ok(false);
    }
    std::fs::remove_file(&file).map_err(|error| format!("{}: {error}", file.display()))?;
    let dir = skill_dir(root);
    let is_empty = std::fs::read_dir(&dir)
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(false);
    if is_empty {
        let _ = std::fs::remove_dir(&dir);
    }
    Ok(true)
}

// ============================================================
// Panel UI: an overview card plus per-root status cards (immediate
// Install/Remove, no intermediate states).
// ============================================================

fn display_root(root: &Path) -> String {
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        if let Ok(relative) = root.strip_prefix(&home) {
            return format!("~/{}", relative.display());
        }
    }
    root.display().to_string()
}

fn skill_state_label(state: SkillInstallState) -> SharedString {
    match state {
        SkillInstallState::Installed => crate::i18n::t("settings.skill.state_installed"),
        SkillInstallState::Outdated => crate::i18n::t("settings.skill.state_outdated"),
        SkillInstallState::Missing => crate::i18n::t("settings.skill.state_missing"),
        SkillInstallState::External => crate::i18n::t("settings.skill.state_external"),
    }
}

impl ShardlaneApp {
    pub(super) fn skill_settings_content(
        &self,
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

        let overview_card = settings_card(
            surface,
            vec![settings_card_row(
                &crate::i18n::t("settings.skill.overview_title"),
                &crate::i18n::t("settings.skill.overview_body"),
                div().into_any_element(),
            )],
        );

        let mut location_rows: Vec<AnyElement> = Vec::new();
        for (index, root) in skill_roots().into_iter().enumerate() {
            let state = read_skill_state(&root);
            let detail = skill_state_label(state);
            let control: AnyElement = match state {
                SkillInstallState::External => div()
                    .text_size(crate::theme::FONT_META)
                    .text_color(content_theme.muted)
                    .child(crate::i18n::t("settings.skill.state_external"))
                    .into_any_element(),
                SkillInstallState::Installed => {
                    let click = herdr.clone();
                    let root = root.clone();
                    Button::new(("skill-location-action", index))
                        .custom(content_button)
                        .xsmall()
                        .label(crate::i18n::t("settings.skill.remove_action"))
                        .on_click(move |_, _, cx| {
                            click.update(cx, |this, cx| {
                                this.skill_notice = uninstall_skill(&root).err();
                                cx.notify();
                            });
                        })
                        .into_any_element()
                }
                SkillInstallState::Outdated | SkillInstallState::Missing => {
                    let click = herdr.clone();
                    let root = root.clone();
                    let label = match state {
                        SkillInstallState::Outdated => {
                            crate::i18n::t("settings.skill.refresh_action")
                        }
                        _ => crate::i18n::t("settings.skill.install_action"),
                    };
                    Button::new(("skill-location-action", index))
                        .custom(content_button)
                        .xsmall()
                        .label(label)
                        .on_click(move |_, _, cx| {
                            click.update(cx, |this, cx| {
                                this.skill_notice = install_skill(&root).err();
                                cx.notify();
                            });
                        })
                        .into_any_element()
                }
            };
            location_rows.push(settings_card_row(&display_root(&root), &detail, control));
        }
        let locations_card = settings_card(surface, location_rows);

        // Visible feedback for the most recent install/uninstall failure;
        // the next successful operation clears it naturally.
        let error_note = self.skill_notice.clone().map(|message| {
            div()
                .mt(px(10.0))
                .text_size(crate::theme::FONT_META)
                .text_color(foreground.opacity(0.75))
                .child(format!("⚠ {message}"))
        });

        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .child(overview_card)
            .child(locations_card)
            .children(error_note)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let root = dir.path().join("home");
        std::fs::create_dir_all(&root).unwrap_or_else(|error| panic!("mkdir: {error}"));
        (dir, root)
    }

    #[test]
    fn bundled_asset_keeps_frontmatter_and_targeting_contract() {
        assert!(BUNDLED_SKILL_MD.starts_with("---\nname: shardlane"));
        assert!(BUNDLED_SKILL_MD.contains("HERDR_SESSION=default"));
        assert!(BUNDLED_SKILL_MD.contains("shardlane workspace list"));
    }

    #[test]
    fn install_refresh_and_uninstall_round_trip() {
        let (_guard, root) = temp_root();
        assert_eq!(read_skill_state(&root), SkillInstallState::Missing);

        let installed = install_skill(&root)
            .ok()
            .unwrap_or_else(|| panic!("install failed"));
        assert_eq!(installed, SkillInstallState::Installed);
        assert_eq!(read_skill_state(&root), SkillInstallState::Installed);

        // Content drift -> Outdated; Install semantics = refresh to the
        // bundled version.
        let file = skill_file(&root);
        std::fs::write(&file, "stale").unwrap_or_else(|error| panic!("write: {error}"));
        assert_eq!(read_skill_state(&root), SkillInstallState::Outdated);
        let refreshed = install_skill(&root)
            .ok()
            .unwrap_or_else(|| panic!("refresh failed"));
        assert_eq!(refreshed, SkillInstallState::Installed);

        assert_eq!(uninstall_skill(&root), Ok(true));
        assert_eq!(read_skill_state(&root), SkillInstallState::Missing);
        // The directory was removed with its last file; repeated uninstalls
        // are an idempotent no-op.
        assert!(!skill_dir(&root).exists());
        assert_eq!(uninstall_skill(&root), Ok(false));
    }

    #[test]
    fn uninstall_keeps_sibling_files_and_non_empty_dir() {
        let (_guard, root) = temp_root();
        install_skill(&root)
            .ok()
            .unwrap_or_else(|| panic!("install failed"));
        let sibling = skill_dir(&root).join("notes.txt");
        std::fs::write(&sibling, "keep me").unwrap_or_else(|error| panic!("write: {error}"));

        assert_eq!(uninstall_skill(&root), Ok(true));
        assert!(sibling.exists(), "sibling files must survive");
        assert!(skill_dir(&root).exists(), "non-empty dir must survive");
    }

    #[cfg(unix)]
    #[test]
    fn externally_managed_symlink_is_never_written() {
        let (guard, root) = temp_root();
        let hub = guard.path().join("hub");
        std::fs::create_dir_all(&hub).unwrap_or_else(|error| panic!("mkdir: {error}"));
        std::os::unix::fs::symlink(&hub, skill_dir(&root))
            .unwrap_or_else(|error| panic!("symlink: {error}"));

        assert_eq!(read_skill_state(&root), SkillInstallState::External);
        let install_error = install_skill(&root).err().unwrap_or_default();
        assert!(
            install_error.contains("symlink"),
            "unexpected: {install_error}"
        );
        let uninstall_error = uninstall_skill(&root).err().unwrap_or_default();
        assert!(
            uninstall_error.contains("symlink"),
            "unexpected: {uninstall_error}"
        );
        assert!(hub.exists());
    }
}
