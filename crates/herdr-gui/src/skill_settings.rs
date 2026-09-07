//! Settings → Skill 面板：把内置 `shardlane` skill 安装/卸载到各 Agent 技能根目录。
//!
//! [INPUT]: 依赖 super（main.rs）的 ShardlaneApp 状态、settings_view 的卡片词汇、ui::controls 的 ControlSurface、gpui-component Button，以及 include_str! 内嵌的 skill/shardlane/SKILL.md 资产
//! [OUTPUT]: 对外提供 ShardlaneApp::skill_settings_content（Settings → Skill 内容列）、内置 skill 资产常量 BUNDLED_SKILL_MD、可测的安装引擎（skill_roots / read_skill_state / install_skill / uninstall_skill）
//! [POS]: settings 演示层拆分之一（mobile_view.rs 的兄弟）；安装引擎是纯 fs 逻辑并直接对临时根目录测试；CLI 查询面在 cli.rs，SKILL.md 是两面的语义契约
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use std::path::{Path, PathBuf};

use super::*;
use crate::settings_view::{settings_card, settings_card_row};
use crate::ui::controls::ControlSurface;

// ============================================================
// 安装引擎：纯 fs、无 UI 依赖；symlink 托管的目标永远拒绝直写
// ============================================================

/// skill 目录名 = skill 名（与 herdr skill 的目录惯例一致）。
pub(crate) const SKILL_DIR_NAME: &str = "shardlane";

/// 内置 skill 资产：安装面板写入磁盘的唯一内容；cli.rs 的查询契约与它同源。
pub(crate) const BUNDLED_SKILL_MD: &str = include_str!("../skill/shardlane/SKILL.md");

/// 规范安装根：agents/skillshare 生态、Codex、Claude。全部展示，不猜测存在性。
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
    /// 与内置资产逐字节一致。
    Installed,
    /// 存在但内容不同（旧版本或被手改）；Install 会刷新为内置版本。
    Outdated,
    Missing,
    /// symlink：可能指向 skillshare hub 等外部托管源，永不直写。
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

/// 写入（或刷新）内置 skill。成功后重读磁盘状态返回，UI 直接投影事实。
pub(crate) fn install_skill(root: &Path) -> Result<SkillInstallState, String> {
    guard_externally_managed(root)?;
    let dir = skill_dir(root);
    std::fs::create_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let file = skill_file(root);
    std::fs::write(&file, BUNDLED_SKILL_MD)
        .map_err(|error| format!("{}: {error}", file.display()))?;
    Ok(read_skill_state(root))
}

/// 只删本 skill 自己的 SKILL.md；目录仅在其变空时移除，绝不触碰兄弟文件。
/// 返回是否真的删除了文件。
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
// 面板 UI：概览卡 + 逐根目录状态卡（即时 Install/Remove，无中间态）
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

        // 最近一次安装/卸载失败的可见反馈；下一次成功操作自然清除。
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

        // 内容漂移 → Outdated；Install 语义 = 刷新为内置版本。
        let file = skill_file(&root);
        std::fs::write(&file, "stale").unwrap_or_else(|error| panic!("write: {error}"));
        assert_eq!(read_skill_state(&root), SkillInstallState::Outdated);
        let refreshed = install_skill(&root)
            .ok()
            .unwrap_or_else(|| panic!("refresh failed"));
        assert_eq!(refreshed, SkillInstallState::Installed);

        assert_eq!(uninstall_skill(&root), Ok(true));
        assert_eq!(read_skill_state(&root), SkillInstallState::Missing);
        // 目录已随最后一份文件清空而移除；重复卸载是幂等 no-op。
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
