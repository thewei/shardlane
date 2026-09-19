//! [INPUT]: Depends on objc2, objc2-app-kit, objc2-foundation, status data from
//!          main.rs, shardlane_host::UsageAggregator（当日 token 聚合 + 窗口事实）,
//!          and crate::i18n for the usage badge text
//! [OUTPUT]: Exposes StatusBarController, StatusBarSnapshot, StatusBarProjectItem, StatusBarAgentItem, StatusBarAction
//! [POS]: crates/herdr-gui/src/status_bar.rs — native integration layer for the macOS
//!        status bar (Menu Bar Extra)；usage_badge 是 #3 用量徽标的唯一展示位：
//!        有 provider 可证窗口时显示 "5h 51% · 7d 8%"，仅聚合时显示本地化
//!        "今日 3.8M tok"；刷新走后台单飞线程（host 侧 60s 节流 + mtime 失效），
//!        展示线程零 I/O，不重复实现任何聚合逻辑
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod usage_badge {
    use crate::i18n;
    use shardlane_host::{UsageAggregator, UsageSnapshot};
    use std::sync::Mutex;

    // ------------------------------------------------------------------
    // 后台刷新状态：徽标文本 + 聚合器 + 单飞标记。展示路径只读 badge。
    // ------------------------------------------------------------------
    struct State {
        aggregator: Option<UsageAggregator>,
        badge: Option<String>,
        attempt_ms: u64,
        running: bool,
    }

    static STATE: Mutex<Option<State>> = Mutex::new(None);
    /// 线程 spawn 频率上限；真正的重算节流（60s）在 host 聚合器里。
    const RETRY_MS: u64 = 5_000;

    fn system_now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as u64)
            .unwrap_or(0)
    }

    /// 徽标文本（规格 #3）：有窗口 → provider 原文标签 "5h 51% · 7d 8%"；
    /// 仅聚合 → 本地化 "今日 3.8M tok"；无事实 → None（不显示）。
    fn badge_text(snapshot: &UsageSnapshot) -> Option<String> {
        let windows = snapshot.badge_windows();
        if !windows.is_empty() {
            return Some(
                windows
                    .iter()
                    .map(|window| format!("{} {}%", window.label, window.used_percentage))
                    .collect::<Vec<_>>()
                    .join(" · "),
            );
        }
        snapshot
            .badge_tokens_compact()
            .map(|tokens| i18n::t_with("statusbar.usage_today", &[("tokens", tokens)]).to_string())
    }

    /// 当前已就绪的徽标文本；刷新在后台线程完成，由下一次 update() 取用。
    pub fn current() -> Option<String> {
        STATE
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().and_then(|state| state.badge.clone()))
    }

    /// 节流触发的后台单飞刷新：绝不阻塞调用线程（菜单栏更新走主线程，
    /// Hosted TUI 对主线程停顿敏感）。
    pub fn request_refresh_if_due() {
        let now_ms = system_now_ms();
        let mut guard = match STATE.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        let state = guard.get_or_insert_with(|| State {
            aggregator: Some(UsageAggregator::new(shardlane_host::default_cache_path())),
            badge: None,
            attempt_ms: 0,
            running: false,
        });
        if state.running || now_ms.saturating_sub(state.attempt_ms) < RETRY_MS {
            return;
        }
        state.attempt_ms = now_ms;
        state.running = true;
        let Some(aggregator) = state.aggregator.take() else {
            state.running = false;
            return;
        };
        let spawned = std::thread::Builder::new()
            .name("usage-badge".into())
            .spawn(move || {
                let mut aggregator = aggregator;
                let snapshot = aggregator.refresh_if_due();
                let badge = snapshot.as_ref().and_then(badge_text);
                if let Ok(mut guard) = STATE.lock() {
                    if let Some(state) = guard.as_mut() {
                        state.aggregator = Some(aggregator);
                        state.badge = badge;
                        state.running = false;
                    }
                }
            });
        if spawned.is_err() {
            // 聚合器随闭包丢弃；下次调用经 get_or_insert_with 重建（磁盘缓存仍在）。
            if let Some(state) = guard.as_mut() {
                state.running = false;
            }
        }
    }

    #[cfg(test)]
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    mod tests {
        use super::*;
        use chrono::NaiveDate;
        use shardlane_host::{AccountUsage, UsageWindow};

        fn snapshot_with(windows: Vec<UsageWindow>, tokens: i64) -> UsageSnapshot {
            UsageSnapshot {
                accounts: vec![AccountUsage {
                    provider: shardlane_history::AgentId::Codex,
                    account_id: None,
                    label_masked: "codex:1111…".into(),
                    day: NaiveDate::from_ymd_opt(2026, 9, 19).unwrap(),
                    tokens_used: tokens,
                    windows,
                }],
                ..UsageSnapshot::default()
            }
        }

        #[test]
        fn badge_text_prefers_provider_windows() {
            let snapshot = snapshot_with(
                vec![
                    UsageWindow {
                        label: "5h".into(),
                        used_percentage: 51,
                        resets_at: None,
                    },
                    UsageWindow {
                        label: "7d".into(),
                        used_percentage: 8,
                        resets_at: None,
                    },
                ],
                3_800_000,
            );
            assert_eq!(badge_text(&snapshot).as_deref(), Some("5h 51% · 7d 8%"));
        }

        #[test]
        fn badge_text_falls_back_to_localized_today_tokens() {
            let snapshot = snapshot_with(Vec::new(), 3_800_000);
            let text = badge_text(&snapshot).expect("tokens badge");
            assert!(text.contains("3.8M"), "unexpected badge: {text}");
        }

        #[test]
        fn badge_text_is_none_without_facts() {
            assert_eq!(badge_text(&UsageSnapshot::default()), None);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusBarProjectItem {
    pub workspace_id: String,
    pub name: String,
    pub is_active: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusBarAgentItem {
    pub name: String,
    pub project_name: String,
    pub status: String,
    pub is_blocked: bool,
    pub is_working: bool,
    /// The stable agent identity (audit A12): menu jumps degrade to the agent-focused
    /// FocusIntent when the pane id is absent instead of no-oping.
    pub terminal_id: String,
    pub workspace_id: Option<String>,
    pub tab_id: Option<String>,
    pub pane_id: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusBarSnapshot {
    pub connected: bool,
    pub working_agents_count: usize,
    pub blocked_agents_count: usize,
    pub active_scripts_count: usize,
    pub failed_scripts_count: usize,
    pub projects: Vec<StatusBarProjectItem>,
    pub agents: Vec<StatusBarAgentItem>,
}

impl StatusBarSnapshot {
    pub fn status_title(&self) -> String {
        if self.blocked_agents_count > 0 && self.working_agents_count > 0 {
            format!(
                " ⚠️ {} ⚡ {}",
                self.blocked_agents_count, self.working_agents_count
            )
        } else if self.blocked_agents_count > 0 {
            format!(" ⚠️ {}", self.blocked_agents_count)
        } else if self.working_agents_count > 0 {
            format!(" ⚡ {}", self.working_agents_count)
        } else {
            String::new()
        }
    }

    pub fn tooltip(&self) -> String {
        if !self.connected {
            return "Shardlane — Disconnected from Herdr".to_string();
        }
        let parts = self.summary_parts();
        if parts.is_empty() {
            "Shardlane — Ready".to_string()
        } else {
            format!("Shardlane — {}", parts.join(", "))
        }
    }

    /// E20: the one working/blocked/scripts summary shared by the status-item
    /// tooltip and the menu header, so the wording cannot drift apart.
    fn summary_parts(&self) -> Vec<String> {
        let mut parts = Vec::new();
        if self.working_agents_count > 0 {
            parts.push(format!("{} working", self.working_agents_count));
        }
        if self.blocked_agents_count > 0 {
            parts.push(format!("{} blocked", self.blocked_agents_count));
        }
        if self.active_scripts_count > 0 {
            parts.push(format!("{} scripts active", self.active_scripts_count));
        }
        parts
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatusBarAction {
    FocusTarget {
        workspace_id: Option<String>,
        tab_id: Option<String>,
        pane_id: Option<String>,
    },
    /// Agent jump (audit A12): always carries the stable terminal_id, so a missing pane id
    /// degrades to the agent-focused FocusIntent semantics instead of a silent no-op click.
    FocusAgent {
        terminal_id: String,
        workspace_id: Option<String>,
        tab_id: Option<String>,
        pane_id: Option<String>,
    },
    OpenNewAgent,
    OpenHistory,
    OpenSettings,
    OpenSearch,
    ShowMainWindow,
    Quit,
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use async_channel::Sender;
    use objc2::rc::Retained;
    use objc2::{define_class, msg_send, sel, ClassType, MainThreadMarker};
    use objc2_app_kit::{NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem};
    use objc2_foundation::{NSObject, NSString};
    use std::sync::Mutex;

    // Action tag constants
    pub const TAG_SHOW_WINDOW: isize = 1000;
    pub const TAG_NEW_AGENT: isize = 1001;
    pub const TAG_HISTORY: isize = 1002;
    pub const TAG_SETTINGS: isize = 1003;
    pub const TAG_SEARCH: isize = 1004;
    pub const TAG_QUIT: isize = 1005;
    pub const TAG_QUICK_BLOCKED_AGENT: isize = 1010;

    pub const TAG_PROJECT_BASE: isize = 3000;
    pub const TAG_AGENT_BASE: isize = 4000;

    static ACTION_SENDER: Mutex<Option<Sender<StatusBarAction>>> = Mutex::new(None);
    static CURRENT_SNAPSHOT: Mutex<Option<StatusBarSnapshot>> = Mutex::new(None);

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "ShardlaneStatusBarTarget"]
        pub struct StatusBarTarget;

        impl StatusBarTarget {
            #[unsafe(method(handleAction:))]
            fn handle_action(&self, sender: &NSMenuItem) {
                let tag = sender.tag();
                let snapshot = CURRENT_SNAPSHOT.lock().ok().and_then(|g| g.clone());
                let action = match tag {
                    TAG_SHOW_WINDOW => Some(StatusBarAction::ShowMainWindow),
                    TAG_NEW_AGENT => Some(StatusBarAction::OpenNewAgent),
                    TAG_HISTORY => Some(StatusBarAction::OpenHistory),
                    TAG_SETTINGS => Some(StatusBarAction::OpenSettings),
                    TAG_SEARCH => Some(StatusBarAction::OpenSearch),
                    TAG_QUIT => Some(StatusBarAction::Quit),
                    TAG_QUICK_BLOCKED_AGENT => {
                        snapshot.as_ref().and_then(|s| {
                            s.agents.iter().find(|a| a.is_blocked).map(|agent| {
                                StatusBarAction::FocusAgent {
                                    terminal_id: agent.terminal_id.clone(),
                                    workspace_id: agent.workspace_id.clone(),
                                    tab_id: agent.tab_id.clone(),
                                    pane_id: agent.pane_id.clone(),
                                }
                            })
                        })
                    }
                    _ if (TAG_PROJECT_BASE..TAG_AGENT_BASE).contains(&tag) => {
                        let idx = (tag - TAG_PROJECT_BASE) as usize;
                        snapshot.as_ref().and_then(|s| s.projects.get(idx)).map(|project| {
                            StatusBarAction::FocusTarget {
                                workspace_id: Some(project.workspace_id.clone()),
                                tab_id: None,
                                pane_id: None,
                            }
                        })
                    }
                    _ if tag >= TAG_AGENT_BASE => {
                        let idx = (tag - TAG_AGENT_BASE) as usize;
                        snapshot.as_ref().and_then(|s| s.agents.get(idx)).map(|agent| {
                            StatusBarAction::FocusAgent {
                                terminal_id: agent.terminal_id.clone(),
                                workspace_id: agent.workspace_id.clone(),
                                tab_id: agent.tab_id.clone(),
                                pane_id: agent.pane_id.clone(),
                            }
                        })
                    }
                    _ => None,
                };

                if let Some(action) = action {
                    if let Ok(guard) = ACTION_SENDER.lock() {
                        if let Some(sender) = guard.as_ref() {
                            let _ = sender.try_send(action);
                        }
                    }
                }
            }
        }
    );

    fn create_clickable_menu_item(
        title: &str,
        tag: isize,
        key_equivalent: Option<&str>,
        target: &StatusBarTarget,
        mtm: MainThreadMarker,
    ) -> Retained<NSMenuItem> {
        let item = NSMenuItem::new(mtm);
        item.setTitle(&NSString::from_str(title));
        item.setTag(tag);
        if let Some(key) = key_equivalent {
            item.setKeyEquivalent(&NSString::from_str(key));
        }
        unsafe {
            item.setTarget(Some(target));
            item.setAction(Some(sel!(handleAction:)));
        }
        item.setEnabled(true);
        item
    }

    fn create_header_menu_item(title: &str, mtm: MainThreadMarker) -> Retained<NSMenuItem> {
        let item = NSMenuItem::new(mtm);
        item.setTitle(&NSString::from_str(title));
        item.setEnabled(false);
        item
    }

    pub struct StatusBarController {
        status_item: Retained<NSStatusItem>,
        target: Retained<StatusBarTarget>,
        /// Audit A10: the last snapshot rendered into the NSMenu. `notify_status_bar` fires on
        /// every navigation/status/theme/drag event; when the snapshot is unchanged (it derives
        /// PartialEq) the whole teardown/rebuild of dozens of ObjC menu objects is skipped.
        /// 元组第二位是用量徽标文本：徽标由后台线程异步就绪，快照未变但徽标
        /// 变化时也要触发一次重建，否则徽标永远停留在旧值。
        last_snapshot: Mutex<Option<(StatusBarSnapshot, Option<String>)>>,
    }

    impl StatusBarController {
        pub fn new(sender: Sender<StatusBarAction>) -> Option<Self> {
            let _mtm = MainThreadMarker::new()?;
            if let Ok(mut guard) = ACTION_SENDER.lock() {
                *guard = Some(sender);
            }

            let status_bar = NSStatusBar::systemStatusBar();
            // Variable length = -1.0
            let status_item = status_bar.statusItemWithLength(-1.0);
            let target: Retained<StatusBarTarget> =
                unsafe { msg_send![StatusBarTarget::class(), new] };

            let controller = Self {
                status_item,
                target,
                last_snapshot: Mutex::new(None),
            };

            controller.update(&StatusBarSnapshot::default());
            Some(controller)
        }

        pub fn update(&self, snapshot: &StatusBarSnapshot) {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            // #3 用量徽标：节流触发后台刷新（永不阻塞主线程），本次渲染取用
            // 已就绪的徽标文本；后台完成后由下一次 update() 自然拾取。
            usage_badge::request_refresh_if_due();
            let usage_badge = usage_badge::current();
            if let Ok(mut guard) = self.last_snapshot.lock() {
                if guard.as_ref().is_some_and(|(last, last_badge)| {
                    last == snapshot && *last_badge == usage_badge
                }) {
                    return;
                }
                *guard = Some((snapshot.clone(), usage_badge.clone()));
            }
            if let Ok(mut guard) = CURRENT_SNAPSHOT.lock() {
                *guard = Some(snapshot.clone());
            }

            // Update status bar button title, icon, and tooltip
            if let Some(button) = self.status_item.button(mtm) {
                let mut title = snapshot.status_title();
                if let Some(badge_text) = &usage_badge {
                    if !title.is_empty() {
                        title.push_str("  ");
                    }
                    title.push_str(badge_text);
                }
                let ns_title = NSString::from_str(&title);
                button.setTitle(&ns_title);

                let tooltip = snapshot.tooltip();
                let ns_tooltip = NSString::from_str(&tooltip);
                button.setToolTip(Some(&ns_tooltip));

                // Set system icon template
                let symbol_name = NSString::from_str("terminal.fill");
                if let Some(image) =
                    NSImage::imageWithSystemSymbolName_accessibilityDescription(&symbol_name, None)
                {
                    image.setTemplate(true);
                    button.setImage(Some(&image));
                }
            }

            // Root menu
            let menu = NSMenu::new(mtm);
            menu.setAutoenablesItems(false);

            // 1. Header: Status Summary
            let header_title = if snapshot.connected {
                let parts = snapshot.summary_parts();
                if parts.is_empty() {
                    "Shardlane · Connected (Idle)".to_string()
                } else {
                    format!("Shardlane · {}", parts.join(" · "))
                }
            } else {
                "Shardlane · Disconnected".to_string()
            };

            menu.addItem(&create_header_menu_item(&header_title, mtm));
            menu.addItem(&NSMenuItem::separatorItem(mtm));

            // 2. High-urgency Quick Jump if an agent is blocked
            if let Some(first_blocked) = snapshot.agents.iter().find(|a| a.is_blocked) {
                let quick_title = format!("⚠️  Jump to {} (Needs Response)", first_blocked.name);
                menu.addItem(&create_clickable_menu_item(
                    &quick_title,
                    TAG_QUICK_BLOCKED_AGENT,
                    None,
                    &self.target,
                    mtm,
                ));
                menu.addItem(&NSMenuItem::separatorItem(mtm));
            }

            // 3. Submenu: Projects (project submenu)
            if !snapshot.projects.is_empty() {
                let proj_parent = NSMenuItem::new(mtm);
                let parent_title = format!("📁 Projects ({})", snapshot.projects.len());
                proj_parent.setTitle(&NSString::from_str(&parent_title));

                let proj_submenu = NSMenu::new(mtm);
                proj_submenu.setAutoenablesItems(false);
                proj_submenu.addItem(&create_header_menu_item("Projects", mtm));
                proj_submenu.addItem(&NSMenuItem::separatorItem(mtm));

                for (idx, project) in snapshot.projects.iter().enumerate() {
                    let prefix = if project.is_active { "✓ " } else { "  " };
                    let title = format!("{prefix}📁 {}", project.name);
                    let item = create_clickable_menu_item(
                        &title,
                        TAG_PROJECT_BASE + idx as isize,
                        None,
                        &self.target,
                        mtm,
                    );
                    proj_submenu.addItem(&item);
                }

                proj_parent.setSubmenu(Some(&proj_submenu));
                menu.addItem(&proj_parent);
            }

            // 4. Submenu: Agents (agent submenu)
            let agent_parent = NSMenuItem::new(mtm);
            let agent_parent_title = if snapshot.blocked_agents_count > 0 {
                format!(
                    "🤖 Agents (⚠️ {}, ⚡ {})",
                    snapshot.blocked_agents_count, snapshot.working_agents_count
                )
            } else if snapshot.working_agents_count > 0 {
                format!("🤖 Agents (⚡ {})", snapshot.working_agents_count)
            } else if !snapshot.agents.is_empty() {
                format!("🤖 Agents ({})", snapshot.agents.len())
            } else {
                "🤖 Agents (None)".to_string()
            };
            agent_parent.setTitle(&NSString::from_str(&agent_parent_title));

            let agent_submenu = NSMenu::new(mtm);
            agent_submenu.setAutoenablesItems(false);

            if snapshot.agents.is_empty() {
                agent_submenu.addItem(&create_header_menu_item("No Active Agents", mtm));
            } else {
                let blocked_agents: Vec<(usize, &StatusBarAgentItem)> = snapshot
                    .agents
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| a.is_blocked)
                    .collect();

                if !blocked_agents.is_empty() {
                    agent_submenu.addItem(&create_header_menu_item("Needs Attention", mtm));
                    for (idx, agent) in blocked_agents {
                        let title =
                            format!("  ⚠️ {} · {} (Needs Input)", agent.name, agent.project_name);
                        let item = create_clickable_menu_item(
                            &title,
                            TAG_AGENT_BASE + idx as isize,
                            None,
                            &self.target,
                            mtm,
                        );
                        agent_submenu.addItem(&item);
                    }
                    agent_submenu.addItem(&NSMenuItem::separatorItem(mtm));
                }

                let working_agents: Vec<(usize, &StatusBarAgentItem)> = snapshot
                    .agents
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| a.is_working)
                    .collect();

                if !working_agents.is_empty() {
                    agent_submenu.addItem(&create_header_menu_item("Working", mtm));
                    for (idx, agent) in working_agents {
                        let title = format!("  ⚡ {} · {}", agent.name, agent.project_name);
                        let item = create_clickable_menu_item(
                            &title,
                            TAG_AGENT_BASE + idx as isize,
                            None,
                            &self.target,
                            mtm,
                        );
                        agent_submenu.addItem(&item);
                    }
                    agent_submenu.addItem(&NSMenuItem::separatorItem(mtm));
                }

                let other_agents: Vec<(usize, &StatusBarAgentItem)> = snapshot
                    .agents
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| !a.is_working && !a.is_blocked)
                    .collect();

                if !other_agents.is_empty() {
                    agent_submenu.addItem(&create_header_menu_item("Idle / Other", mtm));
                    for (idx, agent) in other_agents {
                        let title = format!(
                            "  • {} · {} ({})",
                            agent.name, agent.project_name, agent.status
                        );
                        let item = create_clickable_menu_item(
                            &title,
                            TAG_AGENT_BASE + idx as isize,
                            None,
                            &self.target,
                            mtm,
                        );
                        agent_submenu.addItem(&item);
                    }
                }
            }

            agent_parent.setSubmenu(Some(&agent_submenu));
            menu.addItem(&agent_parent);

            menu.addItem(&NSMenuItem::separatorItem(mtm));

            // 5. Quick Actions / Jump targets
            menu.addItem(&create_header_menu_item("Quick Actions", mtm));

            menu.addItem(&create_clickable_menu_item(
                "New Task…",
                TAG_NEW_AGENT,
                Some("n"),
                &self.target,
                mtm,
            ));

            menu.addItem(&create_clickable_menu_item(
                "Quick Open / Search...",
                TAG_SEARCH,
                Some("p"),
                &self.target,
                mtm,
            ));

            menu.addItem(&create_clickable_menu_item(
                "Conversation History...",
                TAG_HISTORY,
                Some("y"),
                &self.target,
                mtm,
            ));

            menu.addItem(&create_clickable_menu_item(
                "Settings...",
                TAG_SETTINGS,
                Some(","),
                &self.target,
                mtm,
            ));

            menu.addItem(&NSMenuItem::separatorItem(mtm));

            // 6. Window & Process control
            menu.addItem(&create_clickable_menu_item(
                "Open Shardlane Window",
                TAG_SHOW_WINDOW,
                None,
                &self.target,
                mtm,
            ));

            menu.addItem(&create_clickable_menu_item(
                "Quit Shardlane",
                TAG_QUIT,
                Some("q"),
                &self.target,
                mtm,
            ));

            self.status_item.setMenu(Some(&menu));
        }
    }
}

#[cfg(target_os = "macos")]
pub use macos::StatusBarController;

#[cfg(not(target_os = "macos"))]
pub struct StatusBarController;

#[cfg(not(target_os = "macos"))]
impl StatusBarController {
    pub fn new(_sender: async_channel::Sender<StatusBarAction>) -> Option<Self> {
        None
    }
    pub fn update(&self, _snapshot: &StatusBarSnapshot) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_bar_snapshot_title_and_tooltip() {
        let mut snapshot = StatusBarSnapshot::default();
        assert_eq!(snapshot.status_title(), "");
        assert_eq!(snapshot.tooltip(), "Shardlane — Disconnected from Herdr");

        snapshot.connected = true;
        assert_eq!(snapshot.tooltip(), "Shardlane — Ready");

        snapshot.working_agents_count = 2;
        assert_eq!(snapshot.status_title(), " ⚡ 2");
        assert_eq!(snapshot.tooltip(), "Shardlane — 2 working");

        snapshot.blocked_agents_count = 1;
        assert_eq!(snapshot.status_title(), " ⚠️ 1 ⚡ 2");
        assert_eq!(snapshot.tooltip(), "Shardlane — 2 working, 1 blocked");

        snapshot.working_agents_count = 0;
        assert_eq!(snapshot.status_title(), " ⚠️ 1");
        assert_eq!(snapshot.tooltip(), "Shardlane — 1 blocked");
    }

    #[test]
    fn test_status_bar_items_creation() {
        let agent = StatusBarAgentItem {
            name: "pi".to_string(),
            project_name: "herdr-client".to_string(),
            status: "Needs Attention".to_string(),
            is_blocked: true,
            is_working: false,
            terminal_id: "t1".to_string(),
            workspace_id: Some("w1".to_string()),
            tab_id: Some("t1".to_string()),
            pane_id: Some("p1".to_string()),
        };
        assert!(agent.is_blocked);
        assert!(!agent.is_working);

        let project = StatusBarProjectItem {
            workspace_id: "w1".to_string(),
            name: "herdr-client".to_string(),
            is_active: true,
        };
        assert!(project.is_active);
    }
}
