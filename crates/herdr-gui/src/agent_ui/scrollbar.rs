//! Overlay scrollbar for the conversation transcript (R2 / audit CS-05).
//!
//! [INPUT]: gpui ListState/ScrollHandle (through the `Scrollable` abstraction),
//! ContentSurfaceTheme-derived colors, mouse events during the canvas paint
//! phase.
//! [OUTPUT]: ScrollbarState (cross-frame grab/hover/fade state), the `vertical`
//! overlay element, the Scrollable trait.
//! [POS]: paint-only single quad with no layout children — adding it does not
//! change content size; a drag writes the offset straight to the surface
//! (bypassing scroll handlers), which is the only way a surface that owns its
//! scroll intent can perceive a drag (`is_grabbed`). AppKit style: hidden at
//! rest, revealed on scroll, fades after a hold, reappears when the pointer
//! enters the track. Pure geometry/opacity functions carry contract tests.

use std::cell::Cell;
use std::time::{Duration, Instant};

use gpui::{
    canvas, point, px, quad, size, App, BorderStyle, Bounds, IntoElement, ListState, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollHandle, Styled, Window,
};
use std::rc::Rc;

/// Track width, plus thumb idle/hover widths.
const TRACK_WIDTH: f32 = 11.0;
const THUMB_WIDTH: f32 = 5.0;
const THUMB_WIDTH_ACTIVE: f32 = 8.0;
const THUMB_MIN_HEIGHT: f32 = 28.0;
const TRACK_INSET: f32 = 2.0;

/// Full-strength hold duration after the last scroll, and the fade duration.
const HOLD: Duration = Duration::from_millis(900);
const FADE: Duration = Duration::from_millis(350);

/// Cross-frame scrollbar state; the owner holds one per scrollable surface.
#[derive(Debug, Default)]
pub struct ScrollbarState {
    /// While dragging: the pointer's pixel offset inside the thumb.
    grab_offset: Cell<Option<f32>>,
    hovered: Cell<bool>,
    /// When the content last moved (starts the hold → fade timing).
    last_scroll: Cell<Option<Instant>>,
    /// Offset at the last paint, used to detect movement.
    last_offset: Cell<Option<Pixels>>,
    /// Whether the fade-end wake is already in flight.
    fade_wake_armed: Cell<bool>,
}

impl ScrollbarState {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// True while the thumb is held. A drag writes surface offsets directly,
    /// bypassing scroll handlers, so a surface that owns its scroll intent uses
    /// this to recognize the drag and freeze auto-follow.
    pub fn is_grabbed(&self) -> bool {
        self.grab_offset.get().is_some()
    }

    /// Observe the current offset; movement starts the hold timer. The first
    /// observation only seeds the baseline — a transcript that opens pinned to
    /// the tail must not flash the scrollbar.
    fn observe(&self, offset: Pixels, now: Instant) {
        match self.last_offset.replace(Some(offset)) {
            Some(previous) if (offset - previous).abs() > px(0.5) => {
                self.last_scroll.set(Some(now));
            }
            _ => {}
        }
    }
}

/// Overlay opacity: full strength while hovered/dragging, otherwise fades after
/// the hold. Pure function, testable.
fn opacity(since_scroll: Option<Duration>, hovered: bool, grabbed: bool) -> f32 {
    if hovered || grabbed {
        return 1.0;
    }
    let Some(elapsed) = since_scroll else {
        return 0.0;
    };
    if elapsed < HOLD {
        return 1.0;
    }
    let fading = (elapsed - HOLD).as_secs_f32() / FADE.as_secs_f32();
    (1.0 - fading).clamp(0.0, 1.0)
}

/// Resolved scrollbar geometry; None when the surface is not scrollable.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Geometry {
    /// Thumb rectangle inside the track.
    thumb: Bounds<Pixels>,
    /// Distance the thumb can travel.
    travel: Pixels,
    /// Content height beyond the viewport.
    max_offset: Pixels,
}

/// Compute the thumb rectangle for a given track. Pure function; the
/// offset↔thumb-position mapping is unit-testable.
fn geometry(
    track: Bounds<Pixels>,
    viewport_height: Pixels,
    max_offset: Pixels,
    offset: Pixels,
    thumb_width: Pixels,
) -> Option<Geometry> {
    if viewport_height <= Pixels::ZERO || max_offset <= px(0.5) || track.size.height <= Pixels::ZERO
    {
        return None;
    }
    let content_height = viewport_height + max_offset;
    let track_height = track.size.height;
    let thumb_height = (track_height * (viewport_height / content_height))
        .max(px(THUMB_MIN_HEIGHT))
        .min(track_height);
    let travel = (track_height - thumb_height).max(Pixels::ZERO);
    let progress = (offset / max_offset).clamp(0.0, 1.0);
    Some(Geometry {
        thumb: Bounds::new(
            point(
                track.right() - thumb_width - px(TRACK_INSET),
                track.top() + travel * progress,
            ),
            size(thumb_width, thumb_height),
        ),
        travel,
        max_offset,
    })
}

/// Content downward offset corresponding to a thumb top position.
fn offset_for_thumb_top(track_top: Pixels, thumb_top: Pixels, geometry: &Geometry) -> Pixels {
    if geometry.travel <= Pixels::ZERO {
        return Pixels::ZERO;
    }
    let progress = ((thumb_top - track_top) / geometry.travel).clamp(0.0, 1.0);
    geometry.max_offset * progress
}

/// Minimal interface for a scrollable surface. Both GPUI offset flavors are
/// non-positive y; implementations uniformly report downward distance.
pub trait Scrollable {
    fn viewport_height(&self) -> Pixels;
    fn max_offset(&self) -> Pixels;
    fn scrolled(&self) -> Pixels;
    fn scroll_to(&self, offset: Pixels);
}

impl Scrollable for ListState {
    fn viewport_height(&self) -> Pixels {
        self.viewport_bounds().size.height
    }

    fn max_offset(&self) -> Pixels {
        self.max_offset_for_scrollbar().height
    }

    fn scrolled(&self) -> Pixels {
        -self.scroll_px_offset_for_scrollbar().y
    }

    fn scroll_to(&self, offset: Pixels) {
        self.set_offset_from_scrollbar(point(Pixels::ZERO, -offset));
    }
}

impl Scrollable for ScrollHandle {
    fn viewport_height(&self) -> Pixels {
        self.bounds().size.height
    }

    fn max_offset(&self) -> Pixels {
        ScrollHandle::max_offset(self).height
    }

    fn scrolled(&self) -> Pixels {
        -self.offset().y
    }

    fn scroll_to(&self, offset: Pixels) {
        let x = self.offset().x;
        self.set_offset(Point::new(x, -offset));
    }
}

fn scroll_to(surface: &impl Scrollable, offset: Pixels, max_offset: Pixels) {
    surface.scroll_to(offset.clamp(Pixels::ZERO, max_offset));
}

/// A bounded number of repaint wakes during the fade (the original rode a
/// shared pulse clock; discrete wakes approximate it — fade opacity is resolved
/// from time, the wakes only trigger repaints).
fn arm_fade_wake(state: &Rc<ScrollbarState>, view: gpui::EntityId, delay: Duration, cx: &mut App) {
    if state.fade_wake_armed.replace(true) {
        return;
    }
    let state = state.clone();
    cx.spawn(async move |cx| {
        for step in 0..4 {
            cx.background_executor()
                .timer(if step == 0 {
                    delay
                } else {
                    FADE / 4 + Duration::from_millis(16)
                })
                .await;
            let _ = cx.update(|cx| cx.notify(view));
        }
        state.fade_wake_armed.set(false);
    })
    .detach();
}

/// Overlay vertical scrollbar hugging the parent's right edge. The parent must
/// be `relative()`; this element is absolutely positioned and takes no part in
/// layout.
pub fn vertical<S>(
    surface: &S,
    state: &Rc<ScrollbarState>,
    colors: ScrollbarColors,
) -> impl IntoElement + use<S>
where
    S: Scrollable + Clone + 'static,
{
    let list = surface.clone();
    let state = state.clone();
    canvas(
        |_, _, _| (),
        move |track: Bounds<Pixels>, _, window: &mut Window, cx: &mut App| {
            let viewport_height = list.viewport_height();
            let max_offset = Scrollable::max_offset(&list);
            let offset = list.scrolled();
            let now = Instant::now();
            state.observe(offset, now);

            let hovered = state.hovered.get();
            let grabbed = state.is_grabbed();
            let active = hovered || grabbed;
            let thumb_width = px(if active {
                THUMB_WIDTH_ACTIVE
            } else {
                THUMB_WIDTH
            });

            let Some(geometry) = geometry(track, viewport_height, max_offset, offset, thumb_width)
            else {
                // Not scrollable: drop any residual drag so it cannot resume
                // after a resize; paint nothing.
                state.grab_offset.set(None);
                state.hovered.set(false);
                return;
            };

            let since_scroll = state
                .last_scroll
                .get()
                .map(|last| now.saturating_duration_since(last));
            let opacity = opacity(since_scroll, hovered, grabbed);
            if opacity > 0.0 {
                window.paint_quad(quad(
                    geometry.thumb,
                    thumb_width / 2.0,
                    if active {
                        colors.thumb_active
                    } else {
                        colors.thumb_idle
                    }
                    .opacity(opacity),
                    px(0.0),
                    gpui::transparent_black(),
                    BorderStyle::default(),
                ));
                if !active {
                    match since_scroll {
                        Some(elapsed) if elapsed < HOLD => {
                            arm_fade_wake(&state, window.current_view(), HOLD - elapsed, cx);
                        }
                        _ => {
                            arm_fade_wake(&state, window.current_view(), Duration::ZERO, cx);
                        }
                    }
                }
            }

            // Hover is tracked via move events: a hidden scrollbar must be able
            // to reveal itself.
            window.on_mouse_event({
                let state = state.clone();
                move |event: &MouseMoveEvent, phase, window, _| {
                    if phase != gpui::DispatchPhase::Bubble {
                        return;
                    }
                    let hovering = track.contains(&event.position);
                    if state.hovered.replace(hovering) != hovering {
                        window.refresh();
                    }
                }
            });

            window.on_mouse_event({
                let list = list.clone();
                let state = state.clone();
                move |event: &MouseDownEvent, phase, window, _| {
                    if phase != gpui::DispatchPhase::Bubble
                        || event.button != MouseButton::Left
                        || !track.contains(&event.position)
                    {
                        return;
                    }
                    if geometry.thumb.contains(&event.position) {
                        state
                            .grab_offset
                            .set(Some(f32::from(event.position.y - geometry.thumb.top())));
                    } else {
                        // Click on empty track: center the thumb on the click
                        // and start dragging from the middle.
                        let half = geometry.thumb.size.height / 2.0;
                        state.grab_offset.set(Some(f32::from(half)));
                        scroll_to(
                            &list,
                            offset_for_thumb_top(track.top(), event.position.y - half, &geometry),
                            geometry.max_offset,
                        );
                    }
                    window.refresh();
                }
            });

            window.on_mouse_event({
                let list = list.clone();
                let state = state.clone();
                move |event: &MouseMoveEvent, phase, window, _| {
                    if phase != gpui::DispatchPhase::Bubble {
                        return;
                    }
                    let Some(grab) = state.grab_offset.get() else {
                        return;
                    };
                    scroll_to(
                        &list,
                        offset_for_thumb_top(track.top(), event.position.y - px(grab), &geometry),
                        geometry.max_offset,
                    );
                    window.refresh();
                }
            });

            window.on_mouse_event({
                let state = state.clone();
                move |_: &MouseUpEvent, phase, window, _| {
                    if phase != gpui::DispatchPhase::Bubble || state.grab_offset.get().is_none() {
                        return;
                    }
                    state.grab_offset.set(None);
                    window.refresh();
                }
            });
        },
    )
    .absolute()
    .top_0()
    .right_0()
    .h_full()
    .w(px(TRACK_WIDTH))
}

/// Scrollbar colors (derived from the surface theme; corresponds to the
/// text_ghost/text_tertiary tiers).
#[derive(Clone, Copy, Debug)]
pub struct ScrollbarColors {
    pub thumb_idle: gpui::Hsla,
    pub thumb_active: gpui::Hsla,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> Bounds<Pixels> {
        Bounds::new(point(px(500.0), px(100.0)), size(px(11.0), px(400.0)))
    }

    #[test]
    fn the_bar_rests_hidden_and_reveals_on_scroll() {
        assert_eq!(opacity(None, false, false), 0.0);
        assert_eq!(opacity(Some(Duration::ZERO), false, false), 1.0);
        assert_eq!(
            opacity(Some(HOLD - Duration::from_millis(1)), false, false),
            1.0
        );
        let midway = opacity(Some(HOLD + FADE / 2), false, false);
        assert!(
            (0.4..0.6).contains(&midway),
            "expected a half fade, got {midway}"
        );
        assert_eq!(opacity(Some(HOLD + FADE), false, false), 0.0);
        assert_eq!(opacity(Some(HOLD + FADE * 10), false, false), 0.0);
    }

    #[test]
    fn hovering_or_dragging_pins_the_bar_visible() {
        assert_eq!(opacity(Some(HOLD + FADE * 10), true, false), 1.0);
        assert_eq!(opacity(Some(HOLD + FADE * 10), false, true), 1.0);
        assert_eq!(opacity(None, false, true), 1.0);
    }

    #[test]
    fn the_first_observed_offset_only_seeds_the_baseline() {
        let state = ScrollbarState::default();
        let start = Instant::now();
        state.observe(px(4_000.0), start);
        assert_eq!(state.last_scroll.get(), None);
        state.observe(px(3_900.0), start);
        assert!(state.last_scroll.get().is_some());
        state.last_scroll.set(None);
        state.observe(px(3_900.2), start);
        assert_eq!(state.last_scroll.get(), None);
    }

    #[test]
    fn a_surface_that_does_not_scroll_has_no_thumb() {
        assert!(geometry(track(), px(400.0), Pixels::ZERO, Pixels::ZERO, px(5.0)).is_none());
        assert!(geometry(track(), Pixels::ZERO, px(900.0), Pixels::ZERO, px(5.0)).is_none());
    }

    fn geometry_or_panic(
        track: Bounds<Pixels>,
        viewport_height: f32,
        max_offset: f32,
        offset: f32,
        thumb_width: f32,
    ) -> Geometry {
        geometry(
            track,
            px(viewport_height),
            px(max_offset),
            px(offset),
            px(thumb_width),
        )
        .unwrap_or_else(|| panic!("geometry must resolve"))
    }

    #[test]
    fn thumb_height_tracks_the_visible_fraction() {
        let geometry = geometry_or_panic(track(), 400.0, 1200.0, 0.0, 5.0);
        assert_eq!(geometry.thumb.size.height, px(100.0));
        assert_eq!(geometry.thumb.top(), px(100.0));
        assert_eq!(geometry.travel, px(300.0));
    }

    #[test]
    fn a_tiny_visible_fraction_still_leaves_a_grabbable_thumb() {
        let geometry = geometry_or_panic(track(), 400.0, 100_000.0, 0.0, 5.0);
        assert_eq!(geometry.thumb.size.height, px(THUMB_MIN_HEIGHT));
    }

    #[test]
    fn thumb_position_and_offset_are_inverse() {
        let track = track();
        let geometry = geometry_or_panic(track, 400.0, 1200.0, 600.0, 5.0);
        assert_eq!(geometry.thumb.top(), track.top() + px(150.0));
        assert_eq!(
            offset_for_thumb_top(track.top(), geometry.thumb.top(), &geometry),
            px(600.0)
        );
    }

    #[test]
    fn offsets_clamp_at_both_ends() {
        let track = track();
        let geometry = geometry_or_panic(track, 400.0, 1200.0, 1200.0, 5.0);
        assert_eq!(
            geometry.thumb.top(),
            track.bottom() - geometry.thumb.size.height
        );
        assert_eq!(
            offset_for_thumb_top(track.top(), track.top() - px(9_999.0), &geometry),
            Pixels::ZERO
        );
        assert_eq!(
            offset_for_thumb_top(track.top(), track.bottom() + px(9_999.0), &geometry),
            px(1200.0)
        );
    }

    #[test]
    fn overscrolled_offsets_do_not_push_the_thumb_past_the_track() {
        let track = track();
        let geometry = geometry_or_panic(track, 400.0, 1200.0, 5000.0, 5.0);
        assert!(geometry.thumb.bottom() <= track.bottom() + px(0.001));
    }
}
