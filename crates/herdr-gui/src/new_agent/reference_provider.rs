//! Composer reference-completion provider: hooks the @ file / slash-command
//! catalog into gpui-component Input's native completion popup (CompletionMenu).
//! Two consumers share one set of trigger/rank/replace logic: NewAgent (the
//! launch composer, @ files + / commands) and Steering (appending instructions
//! mid-run, @ files only).
//!
//! [INPUT]: gpui-component input's CompletionProvider/InputState/Rope,
//! lsp-types' CompletionItem/TextEdit (text_edit range replacement), reference.rs'
//! trigger detection/ranking/UTF-16 positions, and ShardlaneApp's new_agent_ui/steering
//! catalog snapshots.
//! [OUTPUT]: Provides ComposerReferenceProvider (a CompletionProvider implementation:
//! any insertion re-queries; an empty list on trigger-detector miss → the menu
//! collapses automatically) and ComposerReferenceSource (the NewAgent/Steering source choice).
//! [POS]: The popup adaptation layer of new_agent, consumed by surface.rs (NewAgent) and
//! steering.rs (Steering); the catalog belongs to reference_index.rs and pure logic to reference.rs.

use super::reference::{
    byte_to_utf16_position, detect_reference_trigger, rank_reference_rows, ReferenceKind,
    ReferenceRow,
};
use crate::ShardlaneApp;
use gpui::{App, Context, Task, WeakEntity, Window};
use gpui_component::input::{CompletionProvider, InputState, Rope};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse, CompletionTextEdit,
    Position, Range, TextEdit,
};
use std::rc::Rc;

/// Completion catalog source: NewAgent reads new_agent_ui (files + commands);
/// Steering reads the steering state (files only — slash commands are a launch
/// concept and do not apply to mid-session additions).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ComposerReferenceSource {
    NewAgent,
    Steering,
}

/// Reference completion source attached to the composer prompt: @ files /
/// slash commands.
pub(crate) struct ComposerReferenceProvider {
    app: WeakEntity<ShardlaneApp>,
    source: ComposerReferenceSource,
}

impl ComposerReferenceProvider {
    pub(crate) fn new(app: WeakEntity<ShardlaneApp>, source: ComposerReferenceSource) -> Rc<Self> {
        Rc::new(Self { app, source })
    }

    /// Build the completion response synchronously: no trigger / no catalog →
    /// empty list (the menu collapses).
    fn reference_response(&self, text: &str, offset: usize, cx: &App) -> CompletionResponse {
        let Some(trigger) = detect_reference_trigger(text, offset) else {
            return CompletionResponse::Array(Vec::new());
        };
        let files = match self.app.upgrade() {
            Some(app) => {
                let state = app.read(cx);
                match self.source {
                    ComposerReferenceSource::NewAgent => state
                        .new_agent_ui
                        .as_ref()
                        .and_then(|ui| ui.file_index.clone()),
                    ComposerReferenceSource::Steering => state
                        .steering
                        .as_ref()
                        .and_then(|steering| steering.file_index.clone()),
                }
            }
            None => None,
        };
        let rows = match (self.source, trigger.kind) {
            // Steering only offers file references (v1 scope).
            (_, ReferenceKind::File) => files
                .map(|index| {
                    index
                        .files
                        .iter()
                        .map(|path| ReferenceRow {
                            kind: ReferenceKind::File,
                            label: path.clone(),
                            detail: None,
                            insert: format!("@{path} "),
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            (ComposerReferenceSource::NewAgent, ReferenceKind::Command) => match self.app.upgrade()
            {
                Some(app) => {
                    let state = app.read(cx);
                    state
                        .new_agent_ui
                        .as_ref()
                        .and_then(|ui| {
                            ui.command_catalog
                                .as_ref()
                                .filter(|catalog| catalog.agent == ui.agent)
                                .map(|catalog| {
                                    catalog
                                        .commands
                                        .iter()
                                        .map(|command| ReferenceRow {
                                            kind: ReferenceKind::Command,
                                            label: format!("/{}", command.name),
                                            detail: Some(if command.description.is_empty() {
                                                command.scope.label().to_string()
                                            } else {
                                                format!(
                                                    "{} · {}",
                                                    command.scope.label(),
                                                    command.description
                                                )
                                            }),
                                            insert: format!("/{} ", command.name),
                                        })
                                        .collect::<Vec<_>>()
                                })
                        })
                        .unwrap_or_default()
                }
                None => Vec::new(),
            },
            (ComposerReferenceSource::Steering, ReferenceKind::Command) => Vec::new(),
        };
        let ranked = rank_reference_rows(&trigger.query, rows);
        if ranked.is_empty() {
            return CompletionResponse::Array(Vec::new());
        }
        // Trigger range (bytes) → LSP UTF-16 Position; on acceptance the whole
        // range is replaced by the insert.
        let (start_line, start_character) = byte_to_utf16_position(text, trigger.start);
        let (end_line, end_character) = byte_to_utf16_position(text, trigger.end);
        let range = Range {
            start: Position {
                line: start_line,
                character: start_character,
            },
            end: Position {
                line: end_line,
                character: end_character,
            },
        };
        let items = ranked
            .into_iter()
            .map(|row| CompletionItem {
                label: row.label,
                detail: row.detail,
                kind: Some(match row.kind {
                    ReferenceKind::File => CompletionItemKind::FILE,
                    ReferenceKind::Command => CompletionItemKind::KEYWORD,
                }),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range,
                    new_text: row.insert,
                })),
                ..Default::default()
            })
            .collect();
        CompletionResponse::Array(items)
    }
}

impl CompletionProvider for ComposerReferenceProvider {
    /// Every non-deletion insert re-queries: the provider performs trigger
    /// detection internally; no trigger returns an empty list so the menu
    /// collapses (including the case of typing whitespace after the trigger
    /// token, which ends the query).
    fn is_completion_trigger(
        &self,
        _offset: usize,
        new_text: &str,
        _cx: &mut Context<InputState>,
    ) -> bool {
        !new_text.is_empty()
    }

    fn completions(
        &self,
        rope: &Rope,
        offset: usize,
        _trigger: CompletionContext,
        _window: &mut Window,
        cx: &mut Context<InputState>,
    ) -> Task<anyhow::Result<CompletionResponse>> {
        let response = self.reference_response(&rope.to_string(), offset, cx);
        Task::ready(Ok(response))
    }
}
