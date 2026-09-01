//! Global Search / picker domain model: query parsing, result items, scopes, and the ListDelegate.
//!
//! [INPUT]: Depends on `super` (main.rs)'s ShardlaneApp navigation/History orchestration methods (via entity
//!          update callbacks), `shardlane_history`'s session/search types, and the gpui-component List
//! [OUTPUT]: Exposes the ClientSearchTarget/ClientSearchItem/ClientSearchScope family,
//!           ClientPickerOverlay, ClientSearchDelegate, parse_client_search_query,
//!           client_search_scope_completion, and the history_*_search_item adapters
//! [POS]: The model layer of the search domain's three layers; overlay presentation belongs to search_view.rs, while
//!        projection building (client_search_items) and navigation actions remain in main.rs

use super::*;
use crate::search_view::client_search_empty_state;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ClientSearchTarget {
    Project {
        workspace_id: String,
    },
    Tab {
        tab_id: String,
    },
    Pane {
        workspace_id: Option<String>,
        tab_id: Option<String>,
        pane_id: String,
    },
    Agent {
        workspace_id: Option<String>,
        tab_id: Option<String>,
        pane_id: Option<String>,
        /// Stable primary identity (audit P2-6): allows a direct agent.focus even when the pane is missing.
        terminal_id: Option<String>,
    },
    Script {
        script_id: String,
    },
    DetectedService {
        workspace_id: String,
        tab_id: String,
        pane_id: String,
    },
    Conversation {
        session: Box<ConversationMeta>,
        target_seq: Option<i64>,
    },
}

impl ClientSearchTarget {
    pub(crate) fn kind_label(&self) -> &'static str {
        match self {
            Self::Project { .. } => "Project",
            Self::Tab { .. } => "Tab",
            Self::Pane { .. } => "Pane",
            Self::Agent { .. } => "Agent",
            Self::Script { .. } | Self::DetectedService { .. } => "Script",
            Self::Conversation { .. } => "History",
        }
    }
}

#[derive(Clone)]
pub(crate) struct ClientSearchItem {
    pub(crate) title: String,
    pub(crate) detail: String,
    haystack: String,
    pub(crate) icon: ComponentIconName,
    pub(crate) target: ClientSearchTarget,
    history_project_paths: Vec<String>,
}

impl ClientSearchItem {
    pub(crate) fn new(
        title: String,
        detail: String,
        id: &str,
        icon: ComponentIconName,
        target: ClientSearchTarget,
    ) -> Self {
        let haystack = format!("{title} {detail} {id}").to_lowercase();
        Self {
            title,
            detail,
            haystack,
            icon,
            target,
            history_project_paths: Vec::new(),
        }
    }

    pub(crate) fn with_history_project_paths(mut self, project_paths: Vec<String>) -> Self {
        self.history_project_paths = project_paths;
        self
    }

    pub(crate) fn matches(&self, query: &str) -> bool {
        query
            .split_whitespace()
            .map(str::to_lowercase)
            .all(|token| self.haystack.contains(&token))
    }
}

pub(crate) struct ClientPickerOverlay {
    pub(crate) list: Entity<ListState<ClientSearchDelegate>>,
    pub(crate) input: Entity<InputState>,
    /// Input event subscription lives and dies with the overlay: reopening creates a fresh one and old subscriptions drop automatically.
    pub(crate) _subscription: Subscription,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ClientProjectSearchScope {
    pub(crate) haystack: String,
    pub(crate) project_path: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClientSearchScope {
    Project,
    Tab,
    Pane,
    Agent,
    Script,
    History,
}

impl ClientSearchScope {
    /// Chip/section display label.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Project => "Project",
            Self::Tab => "Tab",
            Self::Pane => "Pane",
            Self::Agent => "Agent",
            Self::Script => "Script",
            Self::History => "History",
        }
    }
}

/// Single source of truth for the `#scope` token vocabulary (audit E11): the parser's
/// scope arms, the picker's completion candidates, and the query chip labels all derive
/// from this table. The first token of each row is the canonical form proposed by
/// completion; the remaining entries are accepted aliases (audit E12: `#histories`,
/// `#proj` now parse the way the chip labels always claimed).
const SCOPE_TOKENS: &[(&[&str], ClientSearchScope)] = &[
    (
        &["#project", "#projects", "#proj"],
        ClientSearchScope::Project,
    ),
    (&["#tab", "#tabs"], ClientSearchScope::Tab),
    (&["#pane", "#panes"], ClientSearchScope::Pane),
    (&["#agent", "#agents"], ClientSearchScope::Agent),
    (&["#script", "#scripts"], ClientSearchScope::Script),
    (&["#history", "#histories"], ClientSearchScope::History),
];

/// Look up an exact `#scope` token (canonical or alias).
fn find_scope_token(token: &str) -> Option<ClientSearchScope> {
    SCOPE_TOKENS
        .iter()
        .find(|(tokens, _)| tokens.contains(&token))
        .map(|(_, scope)| *scope)
}

/// Chip display name for a `#scope` token (`None` → the view shows the token as-is).
pub(crate) fn client_search_scope_chip_label(token: &str) -> Option<&'static str> {
    let normalized = token.to_ascii_lowercase();
    find_scope_token(&normalized).map(|scope| scope.label())
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ParsedClientSearchQuery {
    pub(crate) in_memory_query: String,
    pub(crate) history_query: String,
    pub(crate) project_filter: Option<String>,
    pub(crate) history_project_paths: Vec<String>,
    pub(crate) agent_filter: Option<String>,
    pub(crate) agents: Vec<AgentId>,
    pub(crate) scopes: Vec<ClientSearchScope>,
    pub(crate) scope_completion: Option<&'static str>,
}

impl ParsedClientSearchQuery {
    pub(crate) fn allows_target(&self, target: &ClientSearchTarget) -> bool {
        self.scopes.is_empty()
            || self.scopes.iter().any(|scope| {
                matches!(
                    (scope, target),
                    (
                        ClientSearchScope::Project,
                        ClientSearchTarget::Project { .. }
                    ) | (ClientSearchScope::Tab, ClientSearchTarget::Tab { .. })
                        | (ClientSearchScope::Pane, ClientSearchTarget::Pane { .. })
                        | (ClientSearchScope::Agent, ClientSearchTarget::Agent { .. })
                        | (ClientSearchScope::Script, ClientSearchTarget::Script { .. })
                        | (
                            ClientSearchScope::Script,
                            ClientSearchTarget::DetectedService { .. }
                        )
                        | (
                            ClientSearchScope::History,
                            ClientSearchTarget::Conversation { .. }
                        )
                )
            })
    }

    pub(crate) fn history_allowed(&self) -> bool {
        self.scopes.is_empty() || self.scopes.contains(&ClientSearchScope::History)
    }
}

pub(crate) fn client_search_scope_completion(
    token: &str,
) -> Option<(&'static str, ClientSearchScope)> {
    let normalized = token.to_ascii_lowercase();
    // Completion proposes canonical forms only (first token per SCOPE_TOKENS row) so a
    // shared prefix like "#p" stays unambiguous.
    let candidates = SCOPE_TOKENS
        .iter()
        .map(|(tokens, scope)| (tokens[0], *scope));
    let mut matches = candidates.filter(|(syntax, _)| syntax.starts_with(normalized.as_str()));
    let candidate = matches.next()?;
    matches.next().is_none().then_some(candidate)
}

pub(crate) struct ClientSearchDelegate {
    app: Entity<ShardlaneApp>,
    base_items: Vec<ClientSearchItem>,
    pub(crate) results: Vec<ClientSearchItem>,
    selected_ix: Option<usize>,
    search_generation: u64,
    history_db_path: Option<std::path::PathBuf>,
    project_scopes: Vec<ClientProjectSearchScope>,
    scope_completion: Option<&'static str>,
    result_width: f32,
}

pub(crate) fn parse_client_search_query(
    query: &str,
    project_scopes: &[ClientProjectSearchScope],
) -> ParsedClientSearchQuery {
    let mut parsed = ParsedClientSearchQuery::default();
    let mut in_memory_tokens = Vec::new();
    let mut history_tokens = Vec::new();

    for token in query.split_whitespace() {
        let normalized = token.to_ascii_lowercase();
        let scope = match normalized.as_str() {
            "#all" => {
                parsed.scopes.clear();
                continue;
            }
            token if token.starts_with('#') => {
                // Audit E11: exact tokens (canonical + aliases) come from the shared
                // SCOPE_TOKENS vocabulary; other `#` prefixes fall through to the
                // unique-prefix completion resolver.
                match find_scope_token(token) {
                    Some(scope) => Some(scope),
                    None => client_search_scope_completion(&normalized).map(|(syntax, scope)| {
                        if normalized != syntax {
                            parsed.scope_completion = Some(syntax);
                        }
                        scope
                    }),
                }
            }
            _ => None,
        };
        if let Some(scope) = scope {
            if !parsed.scopes.contains(&scope) {
                parsed.scopes.push(scope);
            }
            continue;
        }
        if let Some(value) = token
            .strip_prefix("project:")
            .or_else(|| token.strip_prefix("proj:"))
            .filter(|value| !value.is_empty())
        {
            let value = value.to_lowercase();
            in_memory_tokens.push(value.clone());
            if parsed.project_filter.is_none() {
                parsed.project_filter = Some(value.clone());
                for scope in project_scopes {
                    if scope.haystack.contains(&value)
                        && !parsed.history_project_paths.contains(&scope.project_path)
                    {
                        parsed
                            .history_project_paths
                            .push(scope.project_path.clone());
                    }
                }
            }
            continue;
        }
        if let Some(value) = token
            .strip_prefix("agent:")
            .filter(|value| !value.is_empty())
        {
            let value = value.to_lowercase();
            in_memory_tokens.push(value.clone());
            if parsed.agent_filter.is_none() {
                parsed.agent_filter = Some(value.clone());
                parsed.agents = AgentId::ALL
                    .into_iter()
                    .filter(|agent| {
                        agent.as_str().contains(&value)
                            || agent.display_name().to_lowercase().contains(&value)
                    })
                    .collect();
            }
            continue;
        }
        in_memory_tokens.push(token.to_string());
        history_tokens.push(token.to_string());
    }

    parsed.in_memory_query = in_memory_tokens.join(" ");
    parsed.history_query = history_tokens.join(" ");
    parsed
}

impl ClientSearchDelegate {
    pub(crate) fn new(
        app: Entity<ShardlaneApp>,
        items: Vec<ClientSearchItem>,
        history_db_path: Option<std::path::PathBuf>,
        result_width: f32,
    ) -> Self {
        let project_scopes = items
            .iter()
            .filter(|item| matches!(item.target, ClientSearchTarget::Project { .. }))
            .filter_map(|item| {
                item.history_project_paths
                    .first()
                    .map(|project_path| ClientProjectSearchScope {
                        haystack: item.haystack.clone(),
                        project_path: project_path.clone(),
                    })
            })
            .collect();
        Self {
            app,
            results: items.clone(),
            base_items: items,
            selected_ix: None,
            search_generation: 0,
            history_db_path,
            project_scopes,
            scope_completion: None,
            result_width,
        }
    }

    /// Current scope completion candidate (e.g. "#project"): consumed by the input row's inline ghost completion.
    pub(super) fn scope_completion(&self) -> Option<&'static str> {
        self.scope_completion
    }

    /// Total result rows and kind-run count: inputs to the card height formula (card_height).
    pub(super) fn palette_metrics(&self) -> (usize, usize) {
        (self.kind_runs().len(), self.results.len())
    }

    fn selected_item(&self) -> Option<ClientSearchItem> {
        self.selected_ix
            .and_then(|ix| self.results.get(ix))
            .cloned()
    }

    /// Kind runs (start row, run length, kind label): one List section.
    /// When results cluster by kind, section headers appear as standalone 30px rows.
    fn kind_runs(&self) -> Vec<(usize, usize, &'static str)> {
        let mut runs: Vec<(usize, usize, &'static str)> = Vec::new();
        for (ix, item) in self.results.iter().enumerate() {
            let kind = item.target.kind_label();
            match runs.last_mut() {
                Some(last) if last.2 == kind => last.1 += 1,
                _ => runs.push((ix, 1, kind)),
            }
        }
        runs
    }

    /// IndexPath(section,row) → flat row number; invalid sections clamp to MAX (get returns None at render time).
    fn flat_index(&self, ix: IndexPath) -> usize {
        let Some((start, len, _)) = self.kind_runs().get(ix.section).copied() else {
            return usize::MAX;
        };
        (start + ix.row).min(start + len.saturating_sub(1))
    }

    /// Flat row number → IndexPath (used by wraparound navigation).
    fn index_path_for_flat(&self, flat: usize) -> IndexPath {
        for (section, (start, len, _)) in self.kind_runs().iter().enumerate() {
            if flat < start + len {
                return IndexPath::new(flat - start).section(section);
            }
        }
        IndexPath::new(0)
    }

    /// Card-level ↑/↓: wrap the selection around; go through defer into ListState::set_selected_index so scrolling stays linked.
    pub(super) fn move_selection(
        &mut self,
        delta: isize,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        let len = self.results.len();
        if len == 0 {
            return;
        }
        let current = self.selected_ix.unwrap_or(0) as isize;
        let next = (current + delta).rem_euclid(len as isize) as usize;
        self.selected_ix = Some(next);
        let ix = self.index_path_for_flat(next);
        cx.defer_in(window, move |state, window, cx| {
            state.set_selected_index(Some(ix), window, cx);
        });
    }

    fn close_search(&self, _window: &mut Window, cx: &mut Context<ListState<Self>>) {
        self.app.update(cx, |view, cx| {
            view.client_picker = None;
            view.search_open = false;
            view.sync_terminal_application_focus(cx);
            cx.notify();
        });
    }
}

impl ListDelegate for ClientSearchDelegate {
    type Item = ListItem;

    fn perform_search(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> BackgroundJob<()> {
        let query = query.trim().to_string();
        let parsed_query = parse_client_search_query(&query, &self.project_scopes);
        self.scope_completion = parsed_query.scope_completion;
        self.search_generation = self.search_generation.wrapping_add(1);
        let generation = self.search_generation;
        self.results = self
            .base_items
            .iter()
            .filter(|item| parsed_query.allows_target(&item.target))
            .filter(|item| item.matches(&parsed_query.in_memory_query))
            .take(80)
            .cloned()
            .collect();
        self.selected_ix = (!self.results.is_empty()).then_some(0);
        let has_results = !self.results.is_empty();
        cx.defer_in(window, move |state, window, cx| {
            state.set_selected_index(has_results.then_some(IndexPath::new(0)), window, cx);
        });
        cx.notify();

        let Some(db_path) = self.history_db_path.clone() else {
            return BackgroundJob::ready(());
        };
        if query.is_empty() || !parsed_query.history_allowed() {
            return BackgroundJob::ready(());
        }

        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(180))
                .await;
            let current = this
                .update(cx, |state, _| {
                    state.delegate().search_generation == generation
                })
                .unwrap_or(false);
            if !current {
                return;
            }
            let parsed_query = parsed_query.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let catalog =
                        HistoryCatalog::open(&db_path).map_err(|error| error.to_string())?;
                    let mut project_paths = parsed_query.history_project_paths.clone();
                    if parsed_query.project_filter.is_some() && project_paths.is_empty() {
                        let project_filter =
                            parsed_query.project_filter.as_deref().unwrap_or_default();
                        let candidates = catalog
                            .search_session_metadata(project_filter, 30)
                            .map_err(|error| error.to_string())?;
                        for session in candidates {
                            if !session.project_path.is_empty()
                                && !project_paths.contains(&session.project_path)
                            {
                                project_paths.push(session.project_path);
                            }
                        }
                    }
                    if parsed_query.agent_filter.is_some() && parsed_query.agents.is_empty() {
                        return Ok::<_, String>((Vec::new(), Vec::new()));
                    }
                    let metadata = catalog
                        .search_session_metadata_scoped(
                            &parsed_query.history_query,
                            &project_paths,
                            &parsed_query.agents,
                            30,
                        )
                        .map_err(|error| error.to_string())?;
                    let messages = catalog
                        .search_scoped(
                            &parsed_query.history_query,
                            &project_paths,
                            &parsed_query.agents,
                            60,
                        )
                        .map_err(|error| error.to_string())?;
                    Ok::<_, String>((metadata, messages))
                })
                .await;
            let _ = this.update_in(cx, |state, window, cx| {
                if state.delegate().search_generation != generation {
                    return;
                }
                let Ok((metadata, messages)) = result else {
                    return;
                };
                let delegate = state.delegate_mut();
                let mut body_sessions = HashSet::new();
                let mut history_items = Vec::new();
                for hit in messages {
                    body_sessions.insert(hit.session.key.clone());
                    history_items.push(history_message_search_item(hit));
                }
                for session in metadata {
                    if !body_sessions.contains(&session.key) {
                        history_items.push(history_metadata_search_item(session));
                    }
                }
                delegate.results.extend(
                    history_items
                        .into_iter()
                        .take(80usize.saturating_sub(delegate.results.len())),
                );
                if delegate.selected_ix.is_none() && !delegate.results.is_empty() {
                    delegate.selected_ix = Some(0);
                    state.set_selected_index(Some(IndexPath::new(0)), window, cx);
                }
                cx.notify();
            });
        })
    }

    fn sections_count(&self, _: &App) -> usize {
        self.kind_runs().len()
    }

    fn items_count(&self, section: usize, _: &App) -> usize {
        self.kind_runs().get(section).map_or(0, |(_, len, _)| *len)
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let flat = self.flat_index(ix);
        let item = self.results.get(flat)?;
        Some(client_search_result_item(ix, item, self.result_width, cx))
    }

    /// SECTION_HEADER: standalone 30px row (pt10, 12.5 MEDIUM tertiary);
    /// the first section carries the scope auto-completion hint.
    #[allow(refining_impl_trait)]
    /// SECTION_HEADER: standalone 30px row (pt10, 12.5 MEDIUM tertiary).
    /// Pure kind label; scope auto-completion belongs to the input row's inline ghost, not here.
    fn render_section_header(
        &mut self,
        section: usize,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<AnyElement> {
        let (_, _, kind) = self.kind_runs().get(section).copied()?;
        Some(
            div()
                .h(px(30.0))
                .flex_shrink_0()
                .px(px(9.0))
                .pt(px(10.0))
                .flex()
                .items_start()
                .text_size(crate::theme::FONT_BODY)
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().muted_foreground)
                .child(kind)
                .into_any_element(),
        )
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        client_search_empty_state(cx)
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        let flat = ix.map(|ix| self.flat_index(ix));
        self.selected_ix = flat;
        cx.notify();
    }

    fn confirm(
        &mut self,
        _secondary: bool,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        let Some(item) = self.selected_item() else {
            return;
        };
        self.close_search(window, cx);
        // FocusIntent seam: Search results and Sidebar/History share the same navigation landing;
        // Agent projections without a pane degrade to the most specific known target.
        self.app.update(cx, |view, view_cx| {
            let intent = match item.target {
                ClientSearchTarget::Project { workspace_id } => FocusIntent::project(workspace_id),
                ClientSearchTarget::Tab { tab_id } => FocusIntent::tab(tab_id),
                ClientSearchTarget::Pane {
                    workspace_id,
                    tab_id,
                    pane_id,
                }
                | ClientSearchTarget::Agent {
                    workspace_id,
                    tab_id,
                    pane_id: Some(pane_id),
                    ..
                } => FocusIntent::pane(workspace_id, tab_id, pane_id),
                // audit P2-6: an Agent without a pane but with a stable terminal_id → direct agent.focus.
                ClientSearchTarget::Agent {
                    terminal_id: Some(terminal_id),
                    workspace_id,
                    tab_id,
                    ..
                } => FocusIntent::agent(terminal_id, workspace_id, tab_id, None),
                ClientSearchTarget::Agent { tab_id, .. } => match tab_id {
                    Some(tab_id) => FocusIntent::tab(tab_id),
                    None => return,
                },
                ClientSearchTarget::Script { script_id } => {
                    view.focus_script_id(script_id, window, view_cx);
                    return;
                }
                ClientSearchTarget::DetectedService {
                    workspace_id,
                    tab_id,
                    pane_id,
                } => FocusIntent::pane(Some(workspace_id), Some(tab_id), pane_id),
                ClientSearchTarget::Conversation {
                    session,
                    target_seq,
                } => {
                    view.open_history_session_from_search(*session, target_seq, view_cx);
                    return;
                }
            };
            view.apply_focus_intent(intent, window, view_cx);
        });
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        self.close_search(window, cx);
    }
}

pub(crate) fn history_metadata_search_item(session: ConversationMeta) -> ClientSearchItem {
    let title = if session.title.trim().is_empty() {
        format!(
            "{} · {}",
            session.agent.display_name(),
            session.project_name
        )
    } else {
        session.title.clone()
    };
    let detail = format!(
        "Conversation · {} · {}",
        session.agent.display_name(),
        session.project_path
    );
    let ids = format!("{} {} {}", session.key, session.id, session.file_path);
    ClientSearchItem::new(
        title,
        detail,
        &ids,
        ComponentIconName::BookOpen,
        ClientSearchTarget::Conversation {
            session: Box::new(session),
            target_seq: None,
        },
    )
}

pub(crate) fn history_message_search_item(hit: SearchHit) -> ClientSearchItem {
    let session = hit.session;
    let title = if session.title.trim().is_empty() {
        format!(
            "{} · {}",
            session.agent.display_name(),
            session.project_name
        )
    } else {
        session.title.clone()
    };
    let snippet = hit.snippet.replace('\n', " ");
    let detail = format!(
        "{} · {} · {}",
        session.agent.display_name(),
        session.project_path,
        snippet
    );
    let ids = format!(
        "{} {} {} {}",
        session.key, session.id, session.file_path, snippet
    );
    ClientSearchItem::new(
        title,
        detail,
        &ids,
        ComponentIconName::Search,
        ClientSearchTarget::Conversation {
            session: Box::new(session),
            target_seq: Some(hit.seq),
        },
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn scope_vocabulary_drives_parser_completion_and_labels() {
        for (tokens, scope) in SCOPE_TOKENS {
            for token in *tokens {
                let parsed = parse_client_search_query(token, &[]);
                assert_eq!(
                    parsed.scopes,
                    vec![*scope],
                    "{token} must parse to its scope"
                );
                assert_eq!(parsed.in_memory_query, "");
                assert_eq!(
                    client_search_scope_chip_label(token),
                    Some(scope.label()),
                    "{token} chip label must derive from the same table"
                );
            }
            // The canonical form completes to itself; aliases don't propose ghosts.
            assert_eq!(
                client_search_scope_completion(tokens[0]),
                Some((tokens[0], *scope))
            );
        }
    }

    #[test]
    fn scope_alias_histories_is_honored() {
        // Audit E12: the chip vocabulary already claimed "#histories"; the parser now
        // honors it instead of silently degrading to a literal-text search.
        let parsed = parse_client_search_query("#histories crash", &[]);
        assert_eq!(parsed.scopes, vec![ClientSearchScope::History]);
        assert_eq!(parsed.history_query, "crash");
        assert!(parsed.history_allowed());
        assert!(!parsed.allows_target(&ClientSearchTarget::Project {
            workspace_id: "w1".to_string(),
        }));
    }

    #[test]
    fn ambiguous_hash_prefixes_stay_literal() {
        let parsed = parse_client_search_query("#p shell", &[]);
        assert!(parsed.scopes.is_empty());
        assert_eq!(parsed.scope_completion, None);
        assert_eq!(parsed.in_memory_query, "#p shell");
    }
}
