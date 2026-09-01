//! ConversationSurface — the full conversation presentation layer shared by
//! Live and History (R1/R2, audit CS-02/03/04/05).
//!
//! [INPUT]: gpui list/ListState, this module's scrollbar, the markdown cache
//! and selection, conversation row projection; zero Herdr/catalog/live worker
//! dependencies.
//! [OUTPUT]: ConversationViewportState (shared viewport state: virtual list +
//! row-signature diff + Markdown cache + selection + tool expansion + overlay
//! scrollbar + scroll intent), ConversationScrollIntent (reader intent
//! derivation), the ConversationColumn geometry SSOT, and the
//! `conversation_surface` render function (content under one screen
//! automatically flips to Top alignment pinned to the top; beyond one screen
//! uses Bottom anchoring to follow the tail; notate 2026-08-29 A1).
//! [POS]: the sole conversation layout owner for Live Chat and History Detail:
//! one virtual list, one row measurement, one scrollbar, one Markdown
//! selection, one content column width, one Composer zone, and one bottom gap.
//! Only lifecycle/controllers may differ (History paging/catalog and the Live
//! worker stay in their controllers, not in this module). Scroll intent is
//! derived from geometry + grab state (Dragging/Following/Reading);
//! auto-follow freezes during a drag. The follow-the-tail geometry anchor
//! comes from `ListAlignment::Bottom` (logical anchor None while at tail);
//! this layer no longer maintains a manual tail-following flag.

use std::collections::HashSet;
use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::IntoElement as _;
use gpui::{
    div, list, px, AnyElement, App, ListAlignment, ListState, ParentElement as _, Styled as _,
    Window,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{v_flex, Sizable as _};

use shardlane_history::TranscriptMessage;

use crate::agent_ui::conversation::ConversationRow;
use crate::agent_ui::markdown::cache::MarkdownViewCache;
use crate::agent_ui::markdown::render::TranscriptSelection;
use crate::agent_ui::scrollbar::{self, Scrollable as _, ScrollbarColors, ScrollbarState};
use crate::ContentSurfaceTheme;

/// Conversation content column geometry SSOT (R2 / audit CS-03/04): the
/// transcript's visible blocks and the Composer's visible block must share the
/// same left/right edges; declaring max width / inset in two places is
/// forbidden.
pub const CONVERSATION_MAX_WIDTH: f32 = 720.0;
/// The conversation column's uniform horizontal gutter (shared by the header,
/// row wrappers, and the Composer zone).
pub const CONVERSATION_GUTTER: f32 = 16.0;
/// External bottom gap of the Composer zone (outside-the-surface spacing, not
/// the Composer's internal padding).
pub const CONVERSATION_COMPOSER_BOTTOM_GAP: f32 = 14.0;

/// Virtual list overdraw.
pub const CHAT_LIST_OVERDRAW: f32 = 500.0;

/// Reader scroll intent (audit CS-05 §9.5). Derived from "scrollbar grab state
/// + tail geometry":
/// - scrollbar held → DraggingScrollbar (auto-follow frozen, appends never
///   steal position);
/// - viewport resting at the true tail → FollowingTail (append geometry pins
///   to the bottom);
/// - otherwise → ReadingHistory (append keeps the item anchored, never yanks
///   back to the tail).
///
/// Why it is not stored as separate state: geometry (ListState offset/max
/// offset) and grab state (ScrollbarState::is_grabbed) are already the source
/// of truth; the derivation is pure, testable, and naturally satisfies "drag
/// ends at tail → resume following; drag ends off tail → keep reading; append
/// during drag → do not follow".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConversationScrollIntent {
    FollowingTail,
    ReadingHistory,
    DraggingScrollbar,
}

/// Derive the current intent. `pixels_from_tail` = pixel distance from the
/// viewport to the true tail; `scrollbar_grabbed` comes from the overlay
/// scrollbar. `<= 1px` counts as at-tail.
pub fn classify_scroll_intent(
    scrollbar_grabbed: bool,
    pixels_from_tail: f32,
) -> ConversationScrollIntent {
    if scrollbar_grabbed {
        return ConversationScrollIntent::DraggingScrollbar;
    }
    if pixels_from_tail <= 1.0 {
        ConversationScrollIntent::FollowingTail
    } else {
        ConversationScrollIntent::ReadingHistory
    }
}

/// Viewport state shared by Live/History (audit CS-02 §6.3): each controller
/// instance holds its own, but the implementation is unique.
pub struct ConversationViewportState {
    pub list: ListState,
    /// Whether the list is currently Bottom aligned. GPUI's
    /// `ListAlignment::Bottom` presses content to the viewport bottom when it
    /// is shorter than one screen (a large blank area at top); the product rule
    /// (notate 2026-08-29 A1) is that short content must pin to the top, and
    /// only content beyond one screen uses Bottom anchoring to follow the tail.
    /// Alignment is fixed at ListState construction; flipping rebuilds it
    /// (signatures cleared → full splice in the same frame).
    bottom_aligned: bool,
    /// Last frame's row signatures: the frame path diffs them into a minimal
    /// splice/reset, avoiding full-list remeasurement.
    pub row_signatures: Vec<u64>,
    pub selection: TranscriptSelection,
    pub markdown: MarkdownViewCache,
    /// Expanded tool details ("seq-toolIndex" keys).
    pub expanded_tools: HashSet<String>,
    pub scrollbar: Rc<ScrollbarState>,
}

impl Default for ConversationViewportState {
    fn default() -> Self {
        Self::new()
    }
}

impl ConversationViewportState {
    pub fn new() -> Self {
        Self {
            list: ListState::new(0, ListAlignment::Bottom, px(CHAT_LIST_OVERDRAW)),
            bottom_aligned: true,
            row_signatures: Vec::new(),
            selection: TranscriptSelection::default(),
            markdown: MarkdownViewCache::default(),
            expanded_tools: HashSet::new(),
            scrollbar: ScrollbarState::new(),
        }
    }

    /// Full presentation rebuild (shared by rebind/conversation switch/generation
    /// reset): clear caches + regenerate the list (Bottom anchored = first
    /// frame pinned to tail).
    pub fn reset(&mut self) {
        self.markdown.clear();
        self.expanded_tools.clear();
        self.row_signatures.clear();
        self.bottom_aligned = true;
        self.list = ListState::new(0, ListAlignment::Bottom, px(CHAT_LIST_OVERDRAW));
    }

    /// Alignment reconciliation: only flips to Top when "already painted,
    /// content under one screen, still Bottom aligned" (fixes the blank top
    /// area when content is short). The flip rebuilds the ListState and clears
    /// signatures so the same frame's diff takes the full-Replace path.
    /// One-way: Top never flips back (append tail-following is compensated by
    /// the caller's following_tail derivation; see the chat surface).
    fn reconcile_alignment(&mut self) {
        if !self.bottom_aligned || self.row_signatures.is_empty() {
            return;
        }
        let painted = self.list.viewport_bounds().size.height > px(0.0);
        if !painted {
            return;
        }
        let adapter = ScrollListOwned(self.list.clone());
        if adapter.max_offset() > px(1.0) {
            return;
        }
        self.bottom_aligned = false;
        self.list = ListState::new(0, ListAlignment::Top, px(CHAT_LIST_OVERDRAW));
        self.row_signatures.clear();
    }

    /// Whether content is presented Top aligned (short content pinned to top).
    pub fn is_top_aligned(&self) -> bool {
        !self.bottom_aligned
    }

    /// Frame-path row-signature diff → minimal splice/reset (pure appends
    /// cause no reset; mid-list changes replace only the window between the
    /// shared prefix and suffix; Reset semantics are triggered explicitly by
    /// the caller via `reset()`).
    pub fn sync_rows(&mut self, signatures: Vec<u64>) {
        self.reconcile_alignment();
        let old = std::mem::replace(&mut self.row_signatures, signatures);
        let new = &self.row_signatures;
        match crate::chat::plan_row_splice(&old, new) {
            crate::chat::RowSplice::None => {}
            crate::chat::RowSplice::Replace { old_range, count } => {
                self.list.splice(old_range, count);
            }
        }
    }

    /// Pixel distance to the true tail (≥0).
    pub fn pixels_from_tail(&self) -> f32 {
        let adapter = ScrollListOwned(self.list.clone());
        f32::from((adapter.max_offset() - adapter.scrolled()).max(gpui::px(0.0)))
    }

    /// Current scroll intent (derived from geometry + grab state).
    pub fn scroll_intent(&self) -> ConversationScrollIntent {
        classify_scroll_intent(self.scrollbar.is_grabbed(), self.pixels_from_tail())
    }

    /// Whether we are following the tail (not dragging and geometrically at
    /// tail). Live appends pin to bottom only when this is true (guaranteed
    /// automatically by Bottom-anchored geometry; this query exists for UI
    /// hints/tests).
    pub fn following_tail(&self) -> bool {
        self.scroll_intent() == ConversationScrollIntent::FollowingTail
    }
}

#[derive(Clone)]
struct ScrollListOwned(ListState);

impl scrollbar::Scrollable for ScrollListOwned {
    fn viewport_height(&self) -> gpui::Pixels {
        self.0.viewport_bounds().size.height
    }
    fn max_offset(&self) -> gpui::Pixels {
        self.0.max_offset_for_scrollbar().height
    }
    fn scrolled(&self) -> gpui::Pixels {
        -self.0.scroll_px_offset_for_scrollbar().y
    }
    fn scroll_to(&self, offset: gpui::Pixels) {
        self.0
            .set_offset_from_scrollbar(gpui::point(gpui::Pixels::ZERO, -offset));
    }
}

/// Conversation row signature (discriminant + message content-length digest;
/// strings are not cloned). Streaming text growth → text_len changes → tail
/// row signature changes → minimal splice remeasure. Shared by Live/History.
pub fn conversation_row_signature(messages: &[TranscriptMessage], row: &ConversationRow) -> u64 {
    use std::hash::{Hash, Hasher as _};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::mem::discriminant(row).hash(&mut hasher);
    match *row {
        ConversationRow::UserPrompt(index)
        | ConversationRow::ContextBoundary(index)
        | ConversationRow::Reasoning(index)
        | ConversationRow::Answer(index) => {
            index.hash(&mut hasher);
            hash_message_content(messages, index, &mut hasher);
        }
        ConversationRow::ToolActivity { message, tool } => {
            message.hash(&mut hasher);
            tool.hash(&mut hasher);
            hash_message_content(messages, message, &mut hasher);
        }
        ConversationRow::TurnFold(turn) | ConversationRow::ResponseFooter(turn) => {
            turn.hash(&mut hasher);
        }
        ConversationRow::WorkingIndicator => {}
    }
    hasher.finish()
}

fn hash_message_content(
    messages: &[TranscriptMessage],
    index: usize,
    hasher: &mut std::collections::hash_map::DefaultHasher,
) {
    use std::hash::Hash as _;
    let Some(message) = messages.get(index) else {
        return;
    };
    message.text.len().hash(hasher);
    message.thinking.as_deref().map_or(0, str::len).hash(hasher);
    message.tool_calls.len().hash(hasher);
    for tool in &message.tool_calls {
        tool.output.as_deref().map_or(0, str::len).hash(hasher);
    }
}

/// Fixed signature for the local pending row (the row slot's existence
/// participates in the diff).
pub const PENDING_ROW_SIGNATURE: u64 = 0x70656e64_696e6731; // "pending1"

/// Row render closure (called during list layout; re-enters the caller through
/// the entity to read controller state).
pub type RowRenderer = Box<dyn FnMut(usize, &mut Window, &mut App) -> AnyElement + 'static>;

/// Inputs for one complete ConversationSurface (presentation only; controller
/// data stays out of this type).
pub struct ConversationSurfaceProps<'a> {
    pub viewport: &'a mut ConversationViewportState,
    pub theme: &'a ContentSurfaceTheme,
    /// Top header (geometry goes through the shared `conversation_header_bar`;
    /// content is mode-specific).
    pub header: AnyElement,
    /// Virtual row count (model rows + mode-specific virtual row slots).
    pub row_count: usize,
    /// Row render closure (re-enters the caller through the entity during list
    /// layout; handles only visible ± overdraw).
    pub render_row: RowRenderer,
    /// Unhydrated/connecting overlay (when present the list is not rendered).
    pub overlay: Option<AnyElement>,
    /// In-conversation find bar (⌘F; floats at the conversation area's top
    /// right, notate 08-29 five rounds).
    pub find_bar: Option<AnyElement>,
    /// Composer card (None before History's R3).
    pub composer: Option<AnyElement>,
    /// Fixed strip below the transcript (History's pager).
    pub pager: Option<AnyElement>,
}

/// How many pixels away from the tail before the "Return to Latest" button
/// shows.
const RETURN_TO_LATEST_THRESHOLD: f32 = 100.0;

/// Render the shared ConversationSurface: header / [overlay | virtual list +
/// overlay scrollbar + Return to Latest] / pager / Composer zone (with the
/// bottom gap).
pub fn conversation_surface(props: ConversationSurfaceProps<'_>) -> AnyElement {
    let ConversationSurfaceProps {
        viewport,
        theme,
        header,
        row_count,
        mut render_row,
        overlay,
        find_bar,
        composer,
        pager,
    } = props;

    let show_list = overlay.is_none();
    let far_from_tail =
        show_list && viewport.pixels_from_tail() > RETURN_TO_LATEST_THRESHOLD && row_count > 0;
    let list_state = viewport.list.clone();
    let list_state_for_bar = viewport.list.clone();
    let list_state_for_jump = viewport.list.clone();
    let scrollbar_colors = ScrollbarColors {
        thumb_idle: theme.muted.opacity(0.45),
        thumb_active: theme.foreground.opacity(0.55),
    };
    let list_scrollbar = viewport.scrollbar.clone();
    let selection = viewport.selection.clone();
    let jump_row_count = row_count;

    let content = div().w_full().h_full().flex().justify_center().child(
        list(list_state, move |ix, window, app| {
            render_row(ix, window, app)
        })
        .size_full()
        .max_w(px(CONVERSATION_MAX_WIDTH)),
    );

    let return_to_latest = far_from_tail.then(|| {
        use gpui_component::button::Button;
        use gpui_component::IconName as ComponentIconName;
        div()
            .absolute()
            .bottom(px(12.0))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                Button::new("return-to-latest")
                    .ghost()
                    .xsmall()
                    .icon(ComponentIconName::ArrowDown)
                    .label("Latest")
                    .on_click(move |_, _window, _cx| {
                        if jump_row_count > 0 {
                            list_state_for_jump.scroll_to_reveal_item(jump_row_count - 1);
                        }
                    }),
            )
    });

    let transcript = div()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .relative()
        .overflow_hidden()
        .child(crate::agent_ui::markdown::render::frame_reset(selection))
        .children(overlay)
        .children(find_bar)
        .when(show_list, |container| {
            container.child(content).child(scrollbar::vertical(
                &ScrollListOwned(list_state_for_bar.clone()),
                &list_scrollbar,
                scrollbar_colors,
            ))
        })
        .children(return_to_latest);

    // Composer zone: coaxial with the conversation column (same column
    // geometry); this layer owns the external bottom gap.
    let composer_zone = composer.map(|composer| {
        div()
            .w_full()
            .flex()
            .justify_center()
            .px(px(CONVERSATION_GUTTER))
            .pb(px(CONVERSATION_COMPOSER_BOTTOM_GAP))
            .child(
                div()
                    .w_full()
                    .max_w(px(CONVERSATION_MAX_WIDTH))
                    .child(composer),
            )
    });

    v_flex()
        .flex_1()
        .min_w_0()
        .h_full()
        .bg(theme.background)
        .child(header)
        .child(transcript)
        .children(pager)
        .children(composer_zone)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_intent_classifies_by_grab_and_geometry() {
        // Drag wins: while grabbed it is Dragging regardless of geometry.
        assert_eq!(
            classify_scroll_intent(true, 0.0),
            ConversationScrollIntent::DraggingScrollbar
        );
        assert_eq!(
            classify_scroll_intent(true, 500.0),
            ConversationScrollIntent::DraggingScrollbar
        );
        // At tail (≤1px) → Following.
        assert_eq!(
            classify_scroll_intent(false, 0.0),
            ConversationScrollIntent::FollowingTail
        );
        assert_eq!(
            classify_scroll_intent(false, 1.0),
            ConversationScrollIntent::FollowingTail
        );
        // Away from tail → Reading.
        assert_eq!(
            classify_scroll_intent(false, 1.5),
            ConversationScrollIntent::ReadingHistory
        );
        assert_eq!(
            classify_scroll_intent(false, 900.0),
            ConversationScrollIntent::ReadingHistory
        );
    }

    /// Intent contract (audit §9.6): off-tail + append → no follow; at-tail +
    /// append → follow; dragging + append → no follow. Intent derives from
    /// geometry, so these scenarios are the derivation's truth table.
    #[test]
    fn append_follow_contract_derives_from_intent() {
        let follow_on_append = |grabbed: bool, from_tail: f32| {
            classify_scroll_intent(grabbed, from_tail) == ConversationScrollIntent::FollowingTail
        };
        // Append while reading off-tail: no follow.
        assert!(!follow_on_append(false, 300.0));
        // Append while dragging: no follow.
        assert!(!follow_on_append(true, 0.0));
        // Append while following: keep following.
        assert!(follow_on_append(false, 0.0));
    }

    /// Column geometry SSOT: the constants are the contract (single source;
    /// callers may not keep their own max width/inset).
    #[test]
    fn conversation_column_constants_are_the_contract() {
        assert_eq!(CONVERSATION_MAX_WIDTH, 720.0);
        assert_eq!(CONVERSATION_GUTTER, 16.0);
        assert!(
            (12.0..=16.0).contains(&CONVERSATION_COMPOSER_BOTTOM_GAP),
            "bottom gap must stay in the 12–16px range"
        );
    }

    #[test]
    fn return_to_latest_threshold_is_reasonable() {
        let threshold = RETURN_TO_LATEST_THRESHOLD;
        assert!(
            (50.0..=200.0).contains(&threshold),
            "threshold must be 50–200px to avoid flicker or invisibility, got {threshold}"
        );
    }

    /// The viewport starts tail-anchored (Bottom alignment); pure appends in
    /// sync_rows never trigger a reset.
    #[test]
    fn viewport_state_starts_tail_anchored_and_splices_minimally() {
        let mut viewport = ConversationViewportState::new();
        assert!(viewport.following_tail() || viewport.pixels_from_tail() == 0.0);

        viewport.sync_rows(vec![1, 2, 3]);
        assert_eq!(viewport.row_signatures, vec![1, 2, 3]);
        viewport.sync_rows(vec![1, 2, 3]);
        assert_eq!(viewport.row_signatures, vec![1, 2, 3]);

        // The pure-append path does not error and advances the bookkeeping.
        viewport.sync_rows(vec![1, 2, 3, 4]);
        assert_eq!(viewport.row_signatures, vec![1, 2, 3, 4]);
        viewport.reset();
        assert!(viewport.row_signatures.is_empty());
        assert!(viewport.markdown.is_empty());
    }
}
