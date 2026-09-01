//! [INPUT]: Depends on objc2, objc2-app-kit, objc2-foundation, and status data from main.rs
//! [OUTPUT]: Exposes StatusBarController, StatusBarSnapshot, StatusBarProjectItem, StatusBarAgentItem, StatusBarAction
//! [POS]: crates/herdr-gui/src/status_bar.rs — native integration layer for the macOS status bar (Menu Bar Extra)

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
        last_snapshot: Mutex<Option<StatusBarSnapshot>>,
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
            if let Ok(mut guard) = self.last_snapshot.lock() {
                if guard.as_ref() == Some(snapshot) {
                    return;
                }
                *guard = Some(snapshot.clone());
            }
            if let Ok(mut guard) = CURRENT_SNAPSHOT.lock() {
                *guard = Some(snapshot.clone());
            }

            // Update status bar button title, icon, and tooltip
            if let Some(button) = self.status_item.button(mtm) {
                let title = snapshot.status_title();
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
