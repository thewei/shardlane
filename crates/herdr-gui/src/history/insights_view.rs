//! [INPUT]: InsightsSnapshot from shardlane-history, ContentSurfaceTheme.
//! [OUTPUT]: insights_panel() — read-only statistics view for the History Insights entry point.
//! [POS]: herdr-gui History Insights; consumes local derived data only, no telemetry.
//!
//! Visual language: the earlier Insights design —
//! large-number overview row, GitHub-style weekly heatmap, bar charts, usage boards
//! with brand icons; centered at max-width 720 px.
use super::*;
use shardlane_history::InsightsSnapshot;

/// Render the Insights statistics panel.
pub(super) fn insights_panel(
    snapshot: Option<&InsightsSnapshot>,
    theme: &ContentSurfaceTheme,
    component_theme: &gpui_component::Theme,
) -> AnyElement {
    let muted = component_theme.muted_foreground;

    let Some(snap) = snapshot else {
        return v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap(px(12.0))
            .text_color(muted)
            .child(Spinner::new().small())
            .child(
                div()
                    .text_size(crate::theme::FONT_META)
                    .child("Computing statistics…"),
            )
            .into_any_element();
    };

    if snap.session_count == 0 {
        return v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .child(
                v_flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(Icon::new(ComponentIconName::BookOpen).with_size(px(28.0)))
                    .child(
                        div()
                            .text_size(px(14.0))
                            .font_weight(FontWeight::MEDIUM)
                            .child("No activity yet"),
                    )
                    .child(
                        div()
                            .text_size(crate::theme::FONT_META)
                            .text_color(muted)
                            .child("Start using agents to see your activity here."),
                    ),
            )
            .into_any_element();
    }

    let token_label = if snap.total_tokens > 0 {
        fmt_tokens(snap.total_tokens as u64)
    } else {
        "—".to_string()
    };

    // ── Overview row ────────────────────────────────────────────────────
    let overview = h_flex()
        .flex_wrap()
        .gap(px(40.0))
        .child(stat_big(
            thousands(snap.session_count),
            "Sessions",
            component_theme,
        ))
        .when(snap.total_tokens > 0, |row| {
            row.child(stat_big(token_label.clone(), "Tokens", component_theme))
        })
        .child(stat_big(
            thousands(snap.prompt_count),
            "Prompts",
            component_theme,
        ))
        .child(stat_big(
            thousands(snap.active_days),
            "Active days",
            component_theme,
        ))
        .when(snap.current_streak > 0, |row| {
            row.child(stat_big(
                format!("{} day", snap.current_streak),
                "Streak",
                component_theme,
            ))
        });

    // ── Activity section (heatmap + streak callouts) ─────────────────────
    let heatmap_block = if !snap.daily_activity.is_empty() {
        let heatmap = render_heatmap_weekly(&snap.daily_activity, component_theme);
        // streak / busiest callout row beneath heatmap
        let streak_row = h_flex()
            .gap(px(24.0))
            .when(snap.longest_streak > 0, |r| {
                r.child(streak_pill(
                    &format!("{} day best", snap.longest_streak),
                    component_theme,
                ))
            })
            .when(snap.busiest_day_prompts > 0, |r| {
                r.child(streak_pill(
                    &format!("{} prompts on busiest day", snap.busiest_day_prompts),
                    component_theme,
                ))
            });
        Some(section_block(
            "ACTIVITY",
            v_flex().gap(px(8.0)).child(heatmap).child(streak_row),
            component_theme,
        ))
    } else {
        None
    };

    // ── Distribution charts ───────────────────────────────────────────────
    let hourly = render_bar_section("HOURLY", &snap.hourly, &HOUR_LABELS, component_theme);
    let weekday = render_bar_section("BY DAY", &snap.weekday, &WEEKDAY_LABELS, component_theme);
    let dist_row = h_flex()
        .gap(px(40.0))
        .items_start()
        .child(div().flex_1().child(hourly))
        .child(div().w(px(140.0)).child(weekday));

    // ── Leaderboards ─────────────────────────────────────────────────────
    let dark = component_theme.mode.is_dark();
    let agents_section = (!snap.agent_leaderboard.is_empty())
        .then(|| render_agent_leaderboard(&snap.agent_leaderboard, dark, component_theme));
    let projects_section = (!snap.project_leaderboard.is_empty()).then(|| {
        render_leaderboard(
            "TOP PROJECTS",
            snap.project_leaderboard
                .iter()
                .take(8)
                .map(|t| {
                    let name = if !t.project_name.is_empty() {
                        t.project_name.as_str()
                    } else {
                        t.project_path.as_str()
                    };
                    (name, None, t.sessions, 0u64)
                })
                .collect::<Vec<_>>(),
            component_theme,
        )
    });
    let models_section = (!snap.model_leaderboard.is_empty()).then(|| {
        render_leaderboard(
            "TOP MODELS",
            snap.model_leaderboard
                .iter()
                .take(8)
                .map(|t| (t.model.as_str(), None, t.sessions, 0u64))
                .collect::<Vec<_>>(),
            component_theme,
        )
    });

    let coverage_note = if snap.token_coverage_sessions > 0 && snap.session_count > 0 {
        let pct = (snap.token_coverage_sessions * 100) / snap.session_count;
        Some(format!("Token data covers {pct}% of sessions."))
    } else {
        None
    };

    div()
        .id("insights-scroll")
        .size_full()
        .overflow_y_scrollbar()
        .bg(theme.background)
        .child(
            div().w_full().flex().justify_center().px(px(24.0)).child(
                v_flex()
                    .w_full()
                    .max_w(px(720.0))
                    .pt(px(24.0))
                    .pb(px(40.0))
                    .gap(px(32.0))
                    // Header
                    .child(
                        v_flex()
                            .gap(px(4.0))
                            .child(
                                div()
                                    .text_size(px(18.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(component_theme.foreground)
                                    .child("Insights"),
                            )
                            .child(
                                div()
                                    .text_size(crate::theme::FONT_META)
                                    .text_color(muted)
                                    .child("Your coding agent activity"),
                            ),
                    )
                    .child(overview)
                    .when_some(heatmap_block, |el, block| el.child(block))
                    .child(section_block("DISTRIBUTION", dist_row, component_theme))
                    .when_some(agents_section, |el, s| el.child(s))
                    .when_some(projects_section, |el, s| el.child(s))
                    .when_some(models_section, |el, s| el.child(s))
                    .when_some(coverage_note, |el, note| {
                        el.child(
                            div()
                                .text_size(crate::theme::FONT_META)
                                .text_color(muted)
                                .child(note),
                        )
                    }),
            ),
        )
        .into_any_element()
}

// ── label tables ─────────────────────────────────────────────────────────────

const HOUR_LABELS: [&str; 24] = [
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
    "17", "18", "19", "20", "21", "22", "23",
];
const WEEKDAY_LABELS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

// ── helpers ───────────────────────────────────────────────────────────────────

/// Large-number stat cell: value on top, small muted label below.
fn stat_big(
    value: impl Into<SharedString>,
    label: &str,
    t: &gpui_component::Theme,
) -> impl IntoElement {
    v_flex()
        .gap(px(2.0))
        .child(
            div()
                .text_size(px(24.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.foreground)
                .child(value.into()),
        )
        .child(
            div()
                .text_size(crate::theme::FONT_META)
                .text_color(t.muted_foreground)
                .child(label.to_string()),
        )
}

/// Tiny pill showing streak / activity callout.
fn streak_pill(text: &str, t: &gpui_component::Theme) -> impl IntoElement {
    div()
        .text_size(crate::theme::FONT_META)
        .text_color(t.muted_foreground)
        .child(text.to_string())
}

/// Section wrapper: small muted uppercase label + content.
fn section_block(
    title: &str,
    content: impl IntoElement,
    t: &gpui_component::Theme,
) -> impl IntoElement {
    v_flex()
        .gap(px(12.0))
        .child(
            div()
                .text_size(crate::theme::FONT_META)
                .font_weight(FontWeight::MEDIUM)
                .text_color(t.muted_foreground)
                .child(title.to_string()),
        )
        .child(content)
}

/// GitHub-style weekly heatmap: columns = weeks, rows = day-of-week (Mon top).
/// The most recent week is the rightmost column; each cell is 10×10 px.
fn render_heatmap_weekly(
    daily: &[shardlane_history::DayActivity],
    t: &gpui_component::Theme,
) -> impl IntoElement {
    let max = daily
        .iter()
        .map(|d| d.prompt_count)
        .max()
        .unwrap_or(1)
        .max(1) as f32;
    let accent = t.primary;
    let empty_bg = t.muted_foreground.opacity(0.1);

    // Align to Monday boundary: use weekday_index() which is exposed by DayActivity.
    let mut grid_entries: Vec<Option<u32>> = Vec::new();
    if let Some(first) = daily.first() {
        // Mon=0 .. Sun=6; pad leading empty cells
        for _ in 0..first.weekday_index() {
            grid_entries.push(None);
        }
    }
    for d in daily {
        grid_entries.push(Some(d.prompt_count));
    }
    while !grid_entries.len().is_multiple_of(7) {
        grid_entries.push(None);
    }

    let col_count = (grid_entries.len() / 7).max(1);
    let columns: Vec<gpui::AnyElement> = (0..col_count)
        .map(|col| {
            let cells: Vec<gpui::AnyElement> = (0..7)
                .map(|row| {
                    let entry = grid_entries[row * col_count + col];
                    let bg = match entry {
                        None => gpui::transparent_black(),
                        Some(0) => empty_bg,
                        Some(n) => {
                            let intensity = n as f32 / max;
                            accent.opacity(0.2 + intensity * 0.8)
                        }
                    };
                    div()
                        .w(px(10.0))
                        .h(px(10.0))
                        .rounded_sm()
                        .bg(bg)
                        .into_any_element()
                })
                .collect();
            v_flex().gap(px(2.0)).children(cells).into_any_element()
        })
        .collect();

    h_flex().gap(px(2.0)).children(columns)
}

/// Vertical bar chart section (N bars).
fn render_bar_section<const N: usize>(
    title: &str,
    values: &[u32; N],
    labels: &[&str; N],
    t: &gpui_component::Theme,
) -> impl IntoElement {
    let max = values.iter().copied().max().unwrap_or(1).max(1) as f32;
    let accent = t.primary;
    let empty_bg = t.muted_foreground.opacity(0.12);
    let bars: Vec<gpui::AnyElement> = (0..N)
        .map(|i| {
            let count = values[i];
            let intensity = count as f32 / max;
            let bar_h = 4.0_f32.max(intensity * 40.0);
            let bg = if count == 0 {
                empty_bg
            } else {
                accent.opacity(0.25 + intensity * 0.75)
            };
            v_flex()
                .items_center()
                .gap(px(3.0))
                .child(
                    div()
                        .h(px(40.0))
                        .flex()
                        .items_end()
                        .child(div().w(px(10.0)).h(px(bar_h)).rounded_sm().bg(bg)),
                )
                .child(
                    div()
                        .w(px(14.0))
                        .text_size(px(7.5))
                        .text_color(t.muted_foreground)
                        .child(labels[i].to_string()),
                )
                .into_any_element()
        })
        .collect();
    section_block(title, h_flex().gap(px(2.0)).children(bars), t)
}

/// Agent leaderboard with brand icons where available.
fn render_agent_leaderboard(
    agents: &[shardlane_history::AgentTally],
    dark: bool,
    t: &gpui_component::Theme,
) -> impl IntoElement {
    let rows: Vec<gpui::AnyElement> = agents
        .iter()
        .take(8)
        .enumerate()
        .map(|(i, tally)| {
            let icon_el: gpui::AnyElement = match agent_brand_icon(&tally.agent, dark) {
                Some(path) => gpui::img(path).w(px(14.0)).h(px(14.0)).into_any_element(),
                None => div().w(px(14.0)).h(px(14.0)).into_any_element(),
            };
            let count_str = if tally.prompts > 0 {
                format!("{} sessions · {} prompts", tally.sessions, tally.prompts)
            } else {
                format!("{} sessions", tally.sessions)
            };
            h_flex()
                .py(px(6.0))
                .border_b_1()
                .border_color(t.border.opacity(0.4))
                .gap(px(8.0))
                .child(
                    div()
                        .w(px(16.0))
                        .flex_shrink_0()
                        .text_size(crate::theme::FONT_META)
                        .text_color(t.muted_foreground)
                        .child(format!("{}", i + 1)),
                )
                .child(div().flex_shrink_0().child(icon_el))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(13.0))
                        .text_color(t.foreground)
                        .child(tally.agent.clone()),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(crate::theme::FONT_META)
                        .text_color(t.muted_foreground)
                        .child(count_str),
                )
                .into_any_element()
        })
        .collect();
    section_block("TOP AGENTS", v_flex().children(rows), t)
}

/// Generic leaderboard section (no icon column).
fn render_leaderboard(
    title: &str,
    items: Vec<(&str, Option<gpui::AnyElement>, u64, u64)>,
    t: &gpui_component::Theme,
) -> impl IntoElement {
    let rows: Vec<gpui::AnyElement> = items
        .into_iter()
        .enumerate()
        .map(|(i, (name, icon, sessions, prompts))| {
            let count_str = if prompts > 0 {
                format!("{sessions} sessions · {prompts} prompts")
            } else {
                format!("{sessions} sessions")
            };
            let mut row = h_flex()
                .py(px(6.0))
                .border_b_1()
                .border_color(t.border.opacity(0.4))
                .gap(px(8.0))
                .child(
                    div()
                        .w(px(16.0))
                        .flex_shrink_0()
                        .text_size(crate::theme::FONT_META)
                        .text_color(t.muted_foreground)
                        .child(format!("{}", i + 1)),
                );
            if let Some(icon_el) = icon {
                row = row.child(div().flex_shrink_0().child(icon_el));
            }
            row.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.0))
                    .text_color(t.foreground)
                    .child(name.to_string()),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(crate::theme::FONT_META)
                    .text_color(t.muted_foreground)
                    .child(count_str),
            )
            .into_any_element()
        })
        .collect();
    section_block(title, v_flex().children(rows), t)
}

// ── number formatting ─────────────────────────────────────────────────────────

fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    result.chars().rev().collect()
}

fn fmt_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
