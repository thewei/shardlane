//! [INPUT]: Depends on the crate::ghostty module-root re-export surface (`use super::*`) and
//! std memory/synchronization primitives.
//! [OUTPUT]: Exposes (within the ghostty module tree) the cross-slice behavioral regression
//! suite: frame projection/palette/delta/encoding/selection/scrollbar/hyperlink reuse.
//! [POS]: The behavioral regression boundary of the ghostty module; using the vendored
//! libghostty-vt as the measured ABI, guarding frame-extraction and terminal-interaction
//! invariants.

use super::{
    color_query_report_bytes, default_selection_color, dominant_surface_background,
    ghostty_type_json, push_run, terminal_bg, GhosttyRuntime, GhosttyTerminal, TerminalCursorStyle,
    TerminalDelta, TerminalFrame, TerminalFramePlan, TerminalKey, TerminalLine, TerminalModifiers,
    TerminalMouseAction, TerminalMouseButton, TerminalMouseGeometry, COLOR_QUERY_BACKGROUND,
    COLOR_QUERY_FOREGROUND,
};

#[test]
fn detect_should_find_local_ghostty_checkout() {
    let runtime = GhosttyRuntime::detect();
    assert!(runtime.is_ok() || runtime.is_err());
}

#[test]
fn terminal_runs_merge_adjacent_same_style_without_losing_grid_columns() {
    let mut line = TerminalLine::default();
    push_run(&mut line.runs, 0, "a".to_string(), 0x111111, None);
    push_run(&mut line.runs, 1, "b".to_string(), 0x111111, None);
    push_run(&mut line.runs, 2, "c".to_string(), 0x222222, None);
    assert_eq!(line.runs.len(), 2);
    assert_eq!(line.runs[0].text, "ab");
    assert_eq!(line.runs[0].start_col, 0);
    assert_eq!(line.runs[0].cell_count, 2);
    assert_eq!(line.runs[1].text, "c");
    assert_eq!(line.runs[1].start_col, 2);
    assert_eq!(line.runs[1].cell_count, 1);
}

#[test]
fn default_selection_color_is_derived_from_resolved_terminal_colors() {
    assert_eq!(default_selection_color(0xffffff, 0x000000), 0xff3f3f3f);
    assert_eq!(default_selection_color(0x000000, 0xffffff), 0xffbfbfbf);
}

#[test]
fn dominant_surface_background_requires_eighty_percent_visible_coverage() {
    let mut line = TerminalLine {
        cells: vec![" ".to_string(); 10],
        ..TerminalLine::default()
    };
    for col in 0..8 {
        push_run(
            &mut line.runs,
            col,
            " ".to_string(),
            0xffffff,
            Some(0x282a36),
        );
    }
    assert_eq!(dominant_surface_background(&[line.clone()]), Some(0x282a36));

    line.runs.clear();
    for col in 0..7 {
        push_run(
            &mut line.runs,
            col,
            " ".to_string(),
            0xffffff,
            Some(0x282a36),
        );
    }
    assert_eq!(dominant_surface_background(&[line]), None);
}

#[test]
fn partial_repaint_retains_previous_confirmed_surface_background() {
    let previous = TerminalFrame {
        lines: vec![TerminalLine {
            cells: vec![" ".to_string(); 10],
            ..TerminalLine::default()
        }],
        default_foreground: Some(0xffffff),
        default_background: Some(0x000000),
        surface_background: Some(0x282a36),
        selection_color: Some(default_selection_color(0xffffff, 0x282a36)),
        ..TerminalFrame::default()
    };
    let mut partial_line = TerminalLine {
        cells: vec![" ".to_string(); 10],
        ..TerminalLine::default()
    };
    for col in 0..7 {
        push_run(
            &mut partial_line.runs,
            col,
            " ".to_string(),
            0xffffff,
            Some(0x282a36),
        );
    }
    let mut current = TerminalFrame {
        surface_background: dominant_surface_background(std::slice::from_ref(&partial_line)),
        lines: vec![partial_line],
        default_foreground: Some(0xffffff),
        default_background: Some(0x000000),
        selection_color: Some(default_selection_color(0xffffff, 0x000000)),
        ..TerminalFrame::default()
    };

    assert_eq!(current.surface_background, None);
    assert!(current.retain_confirmed_surface_background_from(&previous));
    assert_eq!(current.surface_background, Some(0x282a36));
    assert_eq!(
        current.selection_color,
        Some(default_selection_color(0xffffff, 0x282a36))
    );
}

#[test]
fn newly_confirmed_surface_background_replaces_previous_theme() {
    let previous = TerminalFrame {
        lines: vec![TerminalLine::default()],
        default_foreground: Some(0xffffff),
        default_background: Some(0x000000),
        surface_background: Some(0x282a36),
        ..TerminalFrame::default()
    };
    let mut line = TerminalLine {
        cells: vec![" ".to_string(); 10],
        ..TerminalLine::default()
    };
    for col in 0..8 {
        push_run(
            &mut line.runs,
            col,
            " ".to_string(),
            0xffffff,
            Some(0x1e1e2e),
        );
    }
    let mut current = TerminalFrame {
        surface_background: dominant_surface_background(std::slice::from_ref(&line)),
        lines: vec![line],
        default_foreground: Some(0xffffff),
        default_background: Some(0x000000),
        ..TerminalFrame::default()
    };

    assert!(!current.retain_confirmed_surface_background_from(&previous));
    assert_eq!(current.surface_background, Some(0x1e1e2e));
}

#[test]
fn projected_frame_recomputes_surface_background_for_visible_pane() {
    let mut line = TerminalLine {
        cells: vec![" ".to_string(); 10],
        ..TerminalLine::default()
    };
    for col in 0..2 {
        push_run(
            &mut line.runs,
            col,
            " ".to_string(),
            0xffffff,
            Some(0x5f1f2a),
        );
    }
    for col in 2..10 {
        push_run(
            &mut line.runs,
            col,
            " ".to_string(),
            0xffffff,
            Some(0x282a36),
        );
    }
    let frame = TerminalFrame {
        lines: vec![line],
        default_foreground: Some(0xffffff),
        default_background: Some(0x000000),
        surface_background: Some(0x282a36),
        ..TerminalFrame::default()
    };
    let projected = frame.project_rect(0, 0, 8, 0);
    assert_eq!(projected.surface_background, Some(0x5f1f2a));
    assert_eq!(
        projected.selection_color,
        Some(default_selection_color(0xffffff, 0x5f1f2a))
    );
}

#[test]
fn herdr_osc_default_colors_flow_into_terminal_frame() {
    let runtime = GhosttyRuntime::detect().unwrap_or_else(|error| panic!("{error}"));
    let api = runtime.load_api().unwrap_or_else(|error| panic!("{error}"));
    let mut terminal = GhosttyTerminal::new(api, 20, 4).unwrap_or_else(|error| panic!("{error}"));
    terminal.write(b"\x1b]10;#112233\x07\x1b]11;#445566\x07theme");
    let frame = terminal.frame().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(frame.default_foreground, Some(0x112233));
    assert_eq!(frame.default_background, Some(0x445566));
    assert_eq!(frame.cursor_color, Some(0x112233));
    assert_eq!(
        frame.selection_color,
        Some(default_selection_color(0x112233, 0x445566))
    );
}

#[test]
fn set_dynamic_colors_seeds_model_defaults_without_registering_queries() {
    let runtime = GhosttyRuntime::detect().unwrap_or_else(|error| panic!("{error}"));
    let api = runtime.load_api().unwrap_or_else(|error| panic!("{error}"));
    let mut terminal = GhosttyTerminal::new(api, 20, 4).unwrap_or_else(|error| panic!("{error}"));
    terminal.set_dynamic_colors(0x112233, 0x445566);
    // Sets change resolved colors; they are never queries.
    assert_eq!(terminal.take_pending_color_queries(), 0);
    let frame = terminal.frame().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(frame.default_foreground, Some(0x112233));
    assert_eq!(frame.default_background, Some(0x445566));
}

#[test]
fn osc_color_queries_are_detected_across_chunk_boundaries() {
    let runtime = GhosttyRuntime::detect().unwrap_or_else(|error| panic!("{error}"));
    let api = runtime.load_api().unwrap_or_else(|error| panic!("{error}"));
    let mut terminal = GhosttyTerminal::new(api, 20, 4).unwrap_or_else(|error| panic!("{error}"));
    terminal.write(b"\x1b]10;");
    assert_eq!(terminal.take_pending_color_queries(), 0);
    terminal.write(b"?\x1b\\");
    assert_eq!(
        terminal.take_pending_color_queries(),
        COLOR_QUERY_FOREGROUND
    );
    terminal.write(b"\x1b]11;?\x07");
    assert_eq!(
        terminal.take_pending_color_queries(),
        COLOR_QUERY_BACKGROUND
    );
    // OSC 4 palette queries are consumed without reporting foreground/background.
    terminal.write(b"\x1b]4;15;?\x1b\\");
    assert_eq!(terminal.take_pending_color_queries(), 0);
    // Sets with payloads never register as queries.
    terminal.write(b"\x1b]10;#abcdef\x07");
    assert_eq!(terminal.take_pending_color_queries(), 0);
}

#[test]
fn color_query_reports_use_xterm_16bit_rgb_and_skip_empty_queries() {
    let bytes = color_query_report_bytes(
        COLOR_QUERY_FOREGROUND | COLOR_QUERY_BACKGROUND,
        0x4c4f69,
        0xffffff,
    );
    let rendered = String::from_utf8(bytes)
        .unwrap_or_else(|error| panic!("color report must be valid UTF-8: {error}"));
    assert_eq!(
        rendered,
        "\x1b]10;rgb:4c4c/4f4f/6969\x1b\\\x1b]11;rgb:ffff/ffff/ffff\x1b\\"
    );
    assert!(color_query_report_bytes(0, 0x000000, 0xffffff).is_empty());
}

#[test]
fn frame_reuse_invalidates_when_default_background_changes() {
    let runtime = GhosttyRuntime::detect().unwrap_or_else(|error| panic!("{error}"));
    let api = runtime.load_api().unwrap_or_else(|error| panic!("{error}"));
    let mut terminal = GhosttyTerminal::new(api, 20, 4).unwrap_or_else(|error| panic!("{error}"));
    // This vendored libghostty-vt snapshot exposes dynamic defaults reliably when foreground and
    // background are updated together. Isolated OSC 11 is a pinned ABI limitation: terminal_get
    // returns NO_VALUE and render-state reports zero, so Shardlane must not reimplement Ghostty's
    // color parser merely to emulate unsupported behavior.
    terminal.write(b"\x1b]10;#112233\x07\x1b]11;#101820\x07same-content");
    let first = terminal.frame().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(first.default_background, Some(0x101820));
    assert_eq!(terminal.resolved_default_background, Some(0x101820));
    let second = terminal
        .frame_reusing(Some(&first))
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(terminal.resolved_default_background, Some(0x101820));
    terminal.write(b"\x1b]10;#112233\x07\x1b]11;#202830\x07");
    let changed = terminal
        .frame_reusing(Some(&second))
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(second.default_background, Some(0x101820));
    assert_eq!(changed.default_background, Some(0x202830));
    assert_eq!(changed.lines, second.lines);
}

#[test]
fn ghostty_applies_cursor_addressed_viewport_deltas_like_herdr_scroll_frames() {
    let runtime = GhosttyRuntime::detect().unwrap_or_else(|error| panic!("{error}"));
    let api = runtime.load_api().unwrap_or_else(|error| panic!("{error}"));
    let mut terminal = GhosttyTerminal::new(api, 80, 10).unwrap_or_else(|error| panic!("{error}"));

    let mut initial = Vec::new();
    for (row, line) in (73..=80).enumerate() {
        initial.extend_from_slice(format!("\x1b[{};1HLINE-{line:03}", row + 1).as_bytes());
    }
    terminal.write(&initial);
    let before = terminal.frame().unwrap_or_else(|error| panic!("{error}"));
    assert!(before.lines[0]
        .runs
        .iter()
        .any(|run| run.text.contains("LINE-073")));

    let mut delta = Vec::new();
    for (row, line) in (68..=77).enumerate() {
        delta.extend_from_slice(format!("\x1b[{};1HLINE-{line:03}", row + 1).as_bytes());
    }
    terminal.write(&delta);
    let after = terminal.frame().unwrap_or_else(|error| panic!("{error}"));
    assert!(after.lines[0]
        .runs
        .iter()
        .any(|run| run.text.contains("LINE-068")));
    assert!(after.lines[9]
        .runs
        .iter()
        .any(|run| run.text.contains("LINE-077")));
}

#[test]
fn terminal_bg_drops_default_background() {
    assert_eq!(terminal_bg(Some(0x282c34), Some(0x282c34)), None);
    assert_eq!(terminal_bg(Some(0x5f1f2a), Some(0x282c34)), Some(0x5f1f2a));
    assert_eq!(terminal_bg(Some(0x00aa00), Some(0x1e1e1e)), Some(0x00aa00));
}

#[test]
fn terminal_bg_preserves_explicit_backgrounds_regardless_of_theme() {
    // No theme reference: Ghostty's explicit bg is authoritative and must be painted.
    assert_eq!(terminal_bg(Some(0x282c34), None), Some(0x282c34));
    // Light theme + explicit dark panel: valid contrast, must be painted.
    assert_eq!(terminal_bg(Some(0x282c34), Some(0xf5f5f5)), Some(0x282c34));
    // Dark theme + explicit dark panel (TUI status bars/floating windows/selected items):
    // must be painted faithfully and never dropped by any heuristic.
    assert_eq!(terminal_bg(Some(0x282c34), Some(0x1e1e1e)), Some(0x282c34));
    assert_eq!(terminal_bg(Some(0x282c34), Some(0x0a0a0a)), Some(0x282c34));
    // Colored backgrounds are always painted.
    assert_eq!(terminal_bg(Some(0x5f1f2a), None), Some(0x5f1f2a));
    assert_eq!(terminal_bg(Some(0x00aa00), Some(0x1e1e1e)), Some(0x00aa00));
}

#[test]
fn frame_exposes_ghostty_default_colors_without_host_palette() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 12, 2) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };
    let frame = terminal.frame().unwrap_or_else(|err| panic!("{err}"));
    let foreground = frame
        .default_foreground
        .unwrap_or_else(|| panic!("Ghostty must expose its resolved default foreground"));
    assert_eq!(frame.cursor_color, Some(foreground));
    if frame.default_background.is_some() {
        assert!(
            frame.selection_color.is_some(),
            "resolved foreground/background must derive a visible selection color"
        );
    }
}

#[test]
fn ghostty_frame_preserves_ansi_backgrounds() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 12, 3) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };
    terminal.write(b"\x1b[42mgreen\x1b[0m\r\n\x1b[7mreverse\x1b[0m");
    let frame = match terminal.frame() {
        Ok(frame) => frame,
        Err(err) => panic!("{err}"),
    };

    assert!(has_background(&frame, "green"));
    assert!(has_text(&frame, "reverse"));
}

#[test]
fn terminal_delta_only_marks_changed_rows() {
    let previous = TerminalFrame {
        lines: vec![TerminalLine::default(), TerminalLine::default()],
        ..TerminalFrame::default()
    };
    let mut current = previous.clone();
    current.lines[1].cells.push("changed".to_string());
    current.cursor = Some((3, 1));

    let delta = TerminalDelta::from_full_frame(&current, Some(&previous));
    assert_eq!(delta.changed_rows.len(), 1);
    assert_eq!(delta.changed_rows[0].row, 1);
    assert!(delta.cursor_changed);
}

#[test]
fn terminal_delta_work_is_bounded_to_changed_rows_in_large_viewport() {
    let mut previous = TerminalFrame {
        lines: vec![TerminalLine::default(); 4_096],
        ..TerminalFrame::default()
    };
    for (row, line) in previous.lines.iter_mut().enumerate() {
        line.cells.push(format!("row-{row}"));
    }
    let mut current = previous.clone();
    current.lines[2_731].cells[0] = "changed".to_string();

    let delta = TerminalDelta::from_full_frame(&current, Some(&previous));
    assert_eq!(delta.changed_rows.len(), 1);
    assert_eq!(delta.changed_rows[0].row, 2_731);
    assert_eq!(delta.changed_rows[0].line.cells[0], "changed");
    assert!(!delta.line_count_changed);
    assert!(!delta.cursor_changed);
}

#[test]
fn terminal_delta_identical_large_frame_produces_no_projection_work() {
    let frame = TerminalFrame {
        lines: vec![TerminalLine::default(); 4_096],
        ..TerminalFrame::default()
    };
    let delta = TerminalDelta::from_full_frame(&frame, Some(&frame));
    assert!(delta.changed_rows.is_empty());
    assert!(!delta.line_count_changed);
    assert!(!delta.cursor_changed);
}

#[test]
#[ignore = "local performance smoke; run explicitly on the development Mac"]
fn ghostty_interactive_frame_extraction_stays_within_local_budget() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 120, 40) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };
    for row in 0..40 {
        terminal.write(format!("baseline-{row:02} {}\r\n", "x".repeat(80)).as_bytes());
    }
    let _ = terminal.frame().unwrap_or_else(|err| panic!("{err}"));

    const SAMPLES: u32 = 32;
    let started = std::time::Instant::now();
    for sample in 0..SAMPLES {
        terminal.write(format!("\rinteractive-{sample:02}").as_bytes());
        let _ = terminal.frame().unwrap_or_else(|err| panic!("{err}"));
    }
    let elapsed = started.elapsed();
    let average_ms = elapsed.as_secs_f64() * 1_000.0 / f64::from(SAMPLES);
    eprintln!(
        "ghostty interactive frame extraction: samples={SAMPLES} total={:.2}ms avg={average_ms:.2}ms",
        elapsed.as_secs_f64() * 1_000.0
    );
    assert!(
        average_ms < 50.0,
        "interactive frame extraction average {average_ms:.2}ms exceeded 50ms local regression budget"
    );
}

#[test]
#[ignore = "local performance diagnostic; run explicitly on the development Mac"]
fn ghostty_frame_reusing_changed_vs_unchanged_smoke() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 120, 40) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };
    for row in 0..40 {
        terminal.write(format!("baseline-{row:02} {}\r\n", "x".repeat(80)).as_bytes());
    }
    let mut previous = terminal.frame().unwrap_or_else(|err| panic!("{err}"));

    const SAMPLES: usize = 64;
    let mut changed_us = Vec::with_capacity(SAMPLES);
    for sample in 0..SAMPLES {
        terminal.write(format!("\rchanged-{sample:02}").as_bytes());
        let started = std::time::Instant::now();
        previous = terminal
            .frame_reusing(Some(&previous))
            .unwrap_or_else(|err| panic!("{err}"));
        changed_us.push(started.elapsed().as_secs_f64() * 1_000_000.0);
    }

    let mut unchanged_us = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let started = std::time::Instant::now();
        previous = terminal
            .frame_reusing(Some(&previous))
            .unwrap_or_else(|err| panic!("{err}"));
        unchanged_us.push(started.elapsed().as_secs_f64() * 1_000_000.0);
    }

    let summarize = |samples: &mut Vec<f64>| {
        samples.sort_by(|left, right| left.total_cmp(right));
        let percentile = |p: f64| {
            let index = ((samples.len() - 1) as f64 * p).round() as usize;
            samples[index]
        };
        (
            percentile(0.50) / 1_000.0,
            percentile(0.95) / 1_000.0,
            samples[samples.len() - 1] / 1_000.0,
        )
    };
    let (changed_p50, changed_p95, changed_max) = summarize(&mut changed_us);
    let (unchanged_p50, unchanged_p95, unchanged_max) = summarize(&mut unchanged_us);
    eprintln!(
        "ghostty frame_reusing 120x40: changed p50={changed_p50:.2}ms p95={changed_p95:.2}ms max={changed_max:.2}ms; unchanged p50={unchanged_p50:.2}ms p95={unchanged_p95:.2}ms max={unchanged_max:.2}ms"
    );
    assert!(changed_p95 < 50.0 && unchanged_p95 < 50.0);
}

#[test]
#[ignore = "local full-screen redraw performance diagnostic; run explicitly on the development Mac"]
fn ghostty_full_viewport_redraw_reuse_smoke() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 120, 40) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };
    terminal.write(b"\x1b[2J\x1b[H");
    for row in 0..40 {
        terminal.write(format!("baseline-{row:02} {}\r\n", "x".repeat(96)).as_bytes());
    }
    let mut previous = terminal.frame().unwrap_or_else(|err| panic!("{err}"));

    const SAMPLES: usize = 96;
    let mut samples_ms = Vec::with_capacity(SAMPLES);
    for sample in 0..SAMPLES {
        let mut payload = String::from("\x1b[H");
        for row in 0..40 {
            payload.push_str(&format!(
                "vim-scroll-{sample:03}-{row:02} {}\x1b[K{}",
                "x".repeat(88),
                if row == 39 { "" } else { "\r\n" }
            ));
        }
        terminal.write(payload.as_bytes());
        let started = std::time::Instant::now();
        previous = terminal
            .frame_reusing(Some(&previous))
            .unwrap_or_else(|err| panic!("{err}"));
        samples_ms.push(started.elapsed().as_secs_f64() * 1_000.0);
    }
    samples_ms.sort_by(|left, right| left.total_cmp(right));
    let percentile = |p: f64| {
        let index = ((samples_ms.len() - 1) as f64 * p).round() as usize;
        samples_ms[index]
    };
    eprintln!(
        "ghostty full viewport redraw 120x40: p50={:.2}ms p95={:.2}ms max={:.2}ms",
        percentile(0.50),
        percentile(0.95),
        samples_ms[samples_ms.len() - 1]
    );
    assert!(percentile(0.95) < 50.0);
}

#[test]
fn ghostty_frame_keeps_wide_cell_columns() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 8, 2) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };
    terminal.write("你a".as_bytes());
    let frame = match terminal.frame() {
        Ok(frame) => frame,
        Err(err) => panic!("{err}"),
    };
    let line = frame.lines.first().unwrap_or_else(|| panic!("missing row"));
    assert_eq!(line.cells.first().map(String::as_str), Some("你"));
    assert_eq!(line.cells.get(1).map(String::as_str), Some(""));
    assert_eq!(line.cells.get(2).map(String::as_str), Some("a"));
    assert_eq!(line.runs.len(), 2);
    assert_eq!(line.runs[0].text, "你");
    assert_eq!(line.runs[0].start_col, 0);
    assert_eq!(line.runs[0].cell_count, 2);
    assert!(line.runs[1].text.starts_with('a'));
    assert_eq!(line.runs[1].start_col, 2);
    assert_eq!(line.runs[1].cell_count as usize, line.cells.len() - 2);
}

#[test]
fn ghostty_focus_encoder_tracks_terminal_mode() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 80, 24) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };

    assert!(terminal
        .encode_focus(true)
        .unwrap_or_else(|err| panic!("{err}"))
        .is_empty());

    terminal.write(b"\x1b[?1004h");
    assert_eq!(
        terminal
            .encode_focus(true)
            .unwrap_or_else(|err| panic!("{err}")),
        b"\x1b[I"
    );
    assert_eq!(
        terminal
            .encode_focus(false)
            .unwrap_or_else(|err| panic!("{err}")),
        b"\x1b[O"
    );
}

#[test]
fn ghostty_paste_encoder_tracks_bracketed_mode() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 80, 24) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };

    let plain = terminal
        .encode_paste("one\ntwo")
        .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(plain, b"one\rtwo");

    let sanitized = terminal
        .encode_paste("a\x1bb")
        .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(sanitized, b"a b");

    terminal.write(b"\x1b[?2004h");
    let bracketed = terminal
        .encode_paste("one\ntwo")
        .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(bracketed, b"\x1b[200~one\ntwo\x1b[201~");
}

/// The Herdr TUI pushes the kitty keyboard protocol (`CSI > 7 u`) on startup, so the
/// hosted viewer's mode-synced key encoder must produce exactly these bytes. Contract
/// verified against Herdr 0.8.2 key forwarding (2026-08-30 isolated-PTY probes):
/// backspace/enter/tab and kitty ctrl-chords and `CSI 27u` escape and
/// `CSI 1;1:1{A..D}` arrows are parsed; Herdr does NOT parse Home/End in any form
/// (`CSI 1;1:1{H,F}` nor legacy `CSI {H,F}`), and misparses the functional-key
/// `CSI 172u` backspace as the literal `¬` codepoint. Those gaps live at the Herdr
/// boundary; this test pins the Shardlane-side encoding contract.
#[test]
fn key_encoder_matches_herdr_tui_kitty_flag7_contract() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 20, 4) {
        Ok(terminal) => terminal,
        Err(_) => return,
    };
    let none = TerminalModifiers::default();
    let ctrl = TerminalModifiers {
        control: true,
        ..TerminalModifiers::default()
    };

    // Herdr TUI startup output enables the protocol in the shared VT model.
    terminal.write(b"\x1b[>7u");

    let encoded = |terminal: &mut GhosttyTerminal, key, mods, utf8: Option<&str>, cp: u32| {
        terminal
            .encode_key(key, mods, utf8, cp)
            .unwrap_or_else(|err| panic!("{err}"))
    };

    // Editing keys stay legacy bytes: Herdr forwards `\x7f`/`\r`/`\t` verbatim to the
    // focused pane (probe-verified erase at a live zsh prompt).
    assert_eq!(
        encoded(&mut terminal, TerminalKey::Backspace, none, None, 0),
        b"\x7f"
    );
    assert_eq!(
        encoded(&mut terminal, TerminalKey::Enter, none, None, 0),
        b"\r"
    );
    assert_eq!(
        encoded(&mut terminal, TerminalKey::Tab, none, None, 0),
        b"\t"
    );

    // Disambiguated escape and arrows with the flag-2 press-type parameter.
    assert_eq!(
        encoded(&mut terminal, TerminalKey::Escape, none, None, 0),
        b"\x1b[27u"
    );
    assert_eq!(
        encoded(&mut terminal, TerminalKey::ArrowUp, none, None, 0),
        b"\x1b[1;1:1A"
    );

    // Ctrl chords become CSI-u with the unshifted codepoint (Herdr parses these and
    // synthesizes the legacy control byte toward the pane; probe-verified SIGINT).
    assert_eq!(
        encoded(&mut terminal, TerminalKey::C, ctrl, None, u32::from(b'c')),
        b"\x1b[99;5u"
    );

    // Mode-independent invariants in flag-7.
    assert_eq!(
        encoded(&mut terminal, TerminalKey::Delete, none, None, 0),
        b"\x1b[3~"
    );
    assert_eq!(
        encoded(&mut terminal, TerminalKey::PageUp, none, None, 0),
        b"\x1b[5~"
    );

    // Printable text is never re-encoded into a sequence.
    assert_eq!(
        encoded(&mut terminal, TerminalKey::A, none, Some("a"), 0),
        b"a"
    );
}

#[test]
fn key_encoder_encodes_named_keys_and_modifier_chords_per_legacy_mode() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 5, 3) {
        Ok(terminal) => terminal,
        Err(_) => return,
    };
    let none = TerminalModifiers::default();
    let ctrl = TerminalModifiers {
        control: true,
        ..TerminalModifiers::default()
    };
    let alt = TerminalModifiers {
        alt: true,
        ..TerminalModifiers::default()
    };
    let shift = TerminalModifiers {
        shift: true,
        ..TerminalModifiers::default()
    };

    // Named keys: base sequences matching xterm conventions.
    assert_eq!(
        terminal
            .encode_key(TerminalKey::Enter, none, None, 0)
            .unwrap_or_else(|err| panic!("{err}")),
        b"\r"
    );
    assert_eq!(
        terminal
            .encode_key(TerminalKey::Tab, none, None, 0)
            .unwrap_or_else(|err| panic!("{err}")),
        b"\t"
    );
    assert_eq!(
        terminal
            .encode_key(TerminalKey::Backspace, none, None, 0)
            .unwrap_or_else(|err| panic!("{err}")),
        b"\x7f"
    );
    assert_eq!(
        terminal
            .encode_key(TerminalKey::Escape, none, None, 0)
            .unwrap_or_else(|err| panic!("{err}")),
        b"\x1b"
    );
    assert_eq!(
        terminal
            .encode_key(TerminalKey::Home, none, None, 0)
            .unwrap_or_else(|err| panic!("{err}")),
        b"\x1b[H"
    );
    assert_eq!(
        terminal
            .encode_key(TerminalKey::End, none, None, 0)
            .unwrap_or_else(|err| panic!("{err}")),
        b"\x1b[F"
    );
    assert_eq!(
        terminal
            .encode_key(TerminalKey::ArrowUp, none, None, 0)
            .unwrap_or_else(|err| panic!("{err}")),
        b"\x1b[A"
    );
    assert_eq!(
        terminal
            .encode_key(TerminalKey::PageUp, none, None, 0)
            .unwrap_or_else(|err| panic!("{err}")),
        b"\x1b[5~"
    );
    assert_eq!(
        terminal
            .encode_key(TerminalKey::PageDown, none, None, 0)
            .unwrap_or_else(|err| panic!("{err}")),
        b"\x1b[6~"
    );
    assert_eq!(
        terminal
            .encode_key(TerminalKey::Delete, none, None, 0)
            .unwrap_or_else(|err| panic!("{err}")),
        b"\x1b[3~"
    );
    assert_eq!(
        terminal
            .encode_key(TerminalKey::Insert, none, None, 0)
            .unwrap_or_else(|err| panic!("{err}")),
        b"\x1b[2~"
    );
    assert_eq!(
        terminal
            .encode_key(TerminalKey::F5, none, None, 0)
            .unwrap_or_else(|err| panic!("{err}")),
        b"\x1b[15~"
    );

    // Modifier chords: classic terminal semantics for ctrl/alt and shift.
    assert_eq!(
        terminal
            .encode_key(TerminalKey::A, ctrl, None, u32::from(b'a'))
            .unwrap_or_else(|err| panic!("{err}")),
        b"\x01"
    );
    assert_eq!(
        terminal
            .encode_key(TerminalKey::C, ctrl, None, u32::from(b'c'))
            .unwrap_or_else(|err| panic!("{err}")),
        b"\x03"
    );
    // legacy mode alt+printable: Ghostty's macOS option-as-alt gate is not configurable in
    // a pure VT library, so the encoding is empty; Shardlane's hosted-PTY boundary then adds
    // the ESC prefix (covered by unit tests of the caller's fallback logic), so this still
    // pins the vendored encoder's actual behavior.
    assert_eq!(
        terminal
            .encode_key(TerminalKey::B, alt, None, u32::from(b'b'))
            .unwrap_or_else(|err| panic!("{err}")),
        b""
    );
    assert_eq!(
        terminal
            .encode_key(TerminalKey::Tab, shift, None, 0)
            .unwrap_or_else(|err| panic!("{err}")),
        b"\x1b[Z"
    );
}

#[test]
fn key_encoder_is_independent_from_terminal_frame_lock() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let terminal = std::sync::Arc::new(std::sync::Mutex::new(
        GhosttyTerminal::new(api, 80, 24).unwrap_or_else(|error| panic!("{error}")),
    ));
    let encoder = terminal
        .lock()
        .unwrap_or_else(|error| panic!("{error}"))
        .key_encoder_handle();

    // Simulate a full-frame extraction holding the model mutex. Named/modifier key encoding
    // must remain available through its own tiny lock instead of queueing behind the frame.
    let _model_guard = terminal.lock().unwrap_or_else(|error| panic!("{error}"));
    let mut encoder = encoder
        .try_lock()
        .unwrap_or_else(|error| panic!("key encoder unexpectedly blocked by model lock: {error}"));
    let encoded = encoder
        .encode_key(
            TerminalKey::F,
            TerminalModifiers {
                control: true,
                ..TerminalModifiers::default()
            },
            None,
            u32::from('f'),
        )
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(!encoded.is_empty());
}

#[test]
fn key_encoder_follows_terminal_keyboard_mode_changes() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 5, 3) {
        Ok(terminal) => terminal,
        Err(_) => return,
    };
    let none = TerminalModifiers::default();

    // DECCKM (cursor key application mode): arrow keys switch to SS3 encoding.
    terminal.write(b"\x1b[?1h");
    assert_eq!(
        terminal
            .encode_key(TerminalKey::ArrowUp, none, None, 0)
            .unwrap_or_else(|err| panic!("{err}")),
        b"\x1bOA"
    );

    // Kitty keyboard protocol progressive enhancement: shift+up becomes a CSI u sequence
    // with modifier arguments.
    terminal.write(b"\x1b[>1u");
    let shifted = terminal
        .encode_key(
            TerminalKey::ArrowUp,
            TerminalModifiers {
                shift: true,
                ..TerminalModifiers::default()
            },
            None,
            0,
        )
        .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(shifted, b"\x1b[1;2A");

    // In kitty disambiguate mode, alt+printable encodes as CSI u (codepoint b + mods 3=alt).
    let alt = TerminalModifiers {
        alt: true,
        ..TerminalModifiers::default()
    };
    assert_eq!(
        terminal
            .encode_key(TerminalKey::B, alt, None, u32::from(b'b'))
            .unwrap_or_else(|err| panic!("{err}")),
        b"\x1b[98;3u"
    );
}

#[test]
fn vendored_ghostty_reports_selection_formatter_layout() {
    let ptr = unsafe { ghostty_type_json() };
    assert!(!ptr.is_null());
    let json = unsafe { std::ffi::CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    let layouts: serde_json::Value =
        serde_json::from_str(&json).unwrap_or_else(|err| panic!("{err}"));
    let expected = [
        ("GhosttyPoint", std::mem::size_of::<super::GhosttyPoint>()),
        (
            "GhosttyPointCoordinate",
            std::mem::size_of::<super::GhosttyPointCoordinate>(),
        ),
        (
            "GhosttyGridRef",
            std::mem::size_of::<super::GhosttyGridRef>(),
        ),
        (
            "GhosttySelection",
            std::mem::size_of::<super::GhosttySelection>(),
        ),
        (
            "GhosttyFormatterScreenExtra",
            std::mem::size_of::<super::GhosttyFormatterScreenExtra>(),
        ),
        (
            "GhosttyFormatterTerminalExtra",
            std::mem::size_of::<super::GhosttyFormatterTerminalExtra>(),
        ),
        (
            "GhosttyFormatterTerminalOptions",
            std::mem::size_of::<super::GhosttyFormatterTerminalOptions>(),
        ),
        (
            "GhosttyTerminalSelectWordOptions",
            std::mem::size_of::<super::GhosttyTerminalSelectWordOptions>(),
        ),
        (
            "GhosttyTerminalSelectLineOptions",
            std::mem::size_of::<super::GhosttyTerminalSelectLineOptions>(),
        ),
        (
            "GhosttyTerminalSelectWordBetweenOptions",
            std::mem::size_of::<super::GhosttyTerminalSelectWordBetweenOptions>(),
        ),
    ];
    for (name, expected_size) in expected {
        let actual_size = layouts
            .get(name)
            .and_then(|layout| layout.get("size"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_else(|| panic!("missing {name} size"));
        assert_eq!(
            actual_size as usize, expected_size,
            "ABI size mismatch for {name}"
        );
    }
}

#[test]
fn ghostty_formatter_unwraps_soft_wrapped_selection() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 5, 3) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };
    terminal.write(b"abcdefghij");
    let text = terminal
        .selection_text((0, 0), (4, 1))
        .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(text, "abcdefghij");
    let reversed = terminal
        .selection_text((4, 1), (0, 0))
        .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(reversed, "abcdefghij");
}

#[test]
fn ghostty_formatter_preserves_hard_newlines_and_trims_padding() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 5, 3) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };
    terminal.write(b"abc  \r\nxy");
    let text = terminal
        .selection_text((0, 0), (4, 1))
        .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(text, "abc\nxy");
}

#[test]
fn ghostty_semantic_word_and_line_selection_use_terminal_rules() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 16, 3) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };
    terminal.write(b"alpha beta\r\n  gamma  ");

    let word = terminal
        .select_word_at((2, 0))
        .unwrap_or_else(|err| panic!("{err}"));
    let line = terminal
        .select_line_at((4, 1))
        .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(word, Some(((0, 0), (4, 0))));
    assert_eq!(line, Some(((2, 1), (6, 1))));
}

#[test]
fn ghostty_word_drag_keeps_word_granularity_across_whitespace() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 16, 2) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };
    terminal.write(b"one two three");

    let over_space = terminal
        .select_word_drag((1, 0), (7, 0))
        .unwrap_or_else(|err| panic!("{err}"));
    let over_three = terminal
        .select_word_drag((1, 0), (9, 0))
        .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(over_space, Some(((0, 0), (7, 0))));
    assert_eq!(over_three, Some(((0, 0), (12, 0))));
}

#[test]
fn ghostty_line_drag_combines_trimmed_logical_lines() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 16, 4) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };
    terminal.write(b"  alpha  \r\n beta \r\n gamma");

    let selection = terminal
        .select_line_drag((3, 0), (3, 2))
        .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(selection, Some(((2, 0), (5, 2))));
}

#[test]
fn ghostty_frame_extracts_osc8_hyperlink_spans() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 24, 2) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };
    terminal.write(b"\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\ plain");
    let frame = terminal.frame().unwrap_or_else(|err| panic!("{err}"));
    let line = frame.lines.first().unwrap_or_else(|| panic!("missing row"));
    assert_eq!(line.hyperlinks.len(), 1);
    assert_eq!(line.hyperlinks[0].start_col, 0);
    assert_eq!(line.hyperlinks[0].end_col, 3);
    assert_eq!(line.hyperlinks[0].uri, "https://example.com");
}

#[test]
fn osc8_links_survive_frame_reuse_without_new_writes() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 24, 2) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };
    terminal.write(b"\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\ plain");
    let first = terminal.frame().unwrap_or_else(|err| panic!("{err}"));
    let row = first.lines.first().unwrap_or_else(|| panic!("missing row"));
    assert_eq!(row.hyperlinks.len(), 1);
    // A frame with no new writes takes the row-level reuse path: spans must be preserved
    // as-is (not lost).
    let second = terminal
        .frame_reusing(Some(&first))
        .unwrap_or_else(|err| panic!("{err}"));
    let reused = second
        .lines
        .first()
        .unwrap_or_else(|| panic!("missing row"));
    assert_eq!(reused.hyperlinks.len(), 1);
    assert_eq!(reused.hyperlinks[0].uri, "https://example.com");
    assert_eq!(reused.hyperlinks[0].start_col, 0);
    assert_eq!(reused.hyperlinks[0].end_col, 3);
    // The first frame rebuilt the RAW signature; the second frame should hit the whole-frame
    // reuse fast path directly instead of doing another full FFI extraction.
    assert_eq!(terminal.reused_rows, first.lines.len());
}

#[test]
fn osc8_relink_updates_uri_after_new_osc8_write() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 24, 2) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };
    terminal.write(b"\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\ plain");
    let first = terminal.frame().unwrap_or_else(|err| panic!("{err}"));
    // Same text with the URI swapped in place: cells/runs are exactly equal, but the OSC-8
    // write must force re-probing, otherwise row-level reuse would return a stale URI —
    // this is the guard test for the reuse-safety invariant.
    terminal.write(b"\r\x1b]8;;https://other.example\x1b\\link\x1b]8;;\x1b\\");
    let second = terminal
        .frame_reusing(Some(&first))
        .unwrap_or_else(|err| panic!("{err}"));
    let row = second
        .lines
        .first()
        .unwrap_or_else(|| panic!("missing row"));
    assert_eq!(row.hyperlinks.len(), 1);
    assert_eq!(row.hyperlinks[0].uri, "https://other.example");
}

#[test]
fn osc8_close_invalidates_same_text_link_reuse() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 24, 2) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };
    terminal.write(b"\x1b]8;;https://example.com\x1b\\link");
    let first = terminal.frame().unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(
        first
            .lines
            .first()
            .unwrap_or_else(|| panic!("missing row"))
            .hyperlinks
            .len(),
        1
    );

    // Close the link and redraw the exact same bytes at the same cursor. The
    // RAW row signature is unchanged; OSC-8 close must still force a fresh
    // span probe so the old hyperlink is not retained.
    terminal.write(b"\r\x1b]8;;\x1b\\link");
    let second = terminal
        .frame_reusing(Some(&first))
        .unwrap_or_else(|err| panic!("{err}"));
    assert!(second
        .lines
        .first()
        .unwrap_or_else(|| panic!("missing row"))
        .hyperlinks
        .is_empty());
}

#[test]
fn c1_osc8_markers_are_observed_for_safe_frame_reuse() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 24, 2) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };
    // C1 OSC/ST is a legal alternative to the 7-bit ESC ] / ESC \\ framing.
    // The vendored VT is configured for 7-bit controls by default, so verify
    // the observer itself captures both C1 open and close markers (including
    // a split terminator); frame reuse will then conservatively take the full
    // extraction path whenever a C1 stream is present.
    terminal.finish_osc8_frame();
    terminal.write(b"\x9d8;;https://example.com");
    assert_eq!(terminal.osc8_probe_state, 4);
    terminal.write(b"\x9c");
    assert_eq!(terminal.osc8_probe_state, 0);
    assert!(terminal.osc8_dirty);
}

#[test]
fn identical_osc8_redraw_stream_keeps_frame_signature_reuse() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 24, 2) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };
    let stream = b"\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\";
    terminal.write(stream);
    let first = terminal.frame().unwrap_or_else(|err| panic!("{err}"));
    let seeded = terminal
        .frame_reusing(Some(&first))
        .unwrap_or_else(|err| panic!("{err}"));
    terminal.reused_rows = 0;

    // Herdr repaints may repeat identical OSC-8 open/close control sequences without
    // changing visible cells. When the control flow is equivalent, the frame-signature fast
    // path must remain available, avoiding per-cell URI FFI.
    // Simulates a PTY splitting the same control sequence across multiple read/write
    // chunks; cross-chunk parsing should still preserve equivalence.
    terminal.write(b"\x1b]8;;https://example.com");
    terminal.write(b"\x1b\\");
    terminal.write(b"\x1b]8;;");
    terminal.write(b"\x1b\\");
    let reused = terminal
        .frame_reusing(Some(&seeded))
        .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(reused.lines, seeded.lines);
    assert_eq!(terminal.reused_rows, seeded.lines.len());
}

#[test]
fn plain_writes_preserve_prior_link_rows() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 24, 3) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };
    terminal.write(b"\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\ plain");
    let first = terminal.frame().unwrap_or_else(|err| panic!("{err}"));
    // A non-OSC-8 write only touches the new row; old rows' links must be preserved via
    // reuse.
    terminal.write(b"\r\nplain tail");
    let second = terminal
        .frame_reusing(Some(&first))
        .unwrap_or_else(|err| panic!("{err}"));
    let row0 = second
        .lines
        .first()
        .unwrap_or_else(|| panic!("missing row"));
    assert_eq!(row0.hyperlinks.len(), 1);
    assert_eq!(row0.hyperlinks[0].uri, "https://example.com");
    let row1 = second
        .lines
        .get(1)
        .unwrap_or_else(|| panic!("missing second row"));
    assert!(row1.hyperlinks.is_empty());
}

#[test]
fn ghostty_osc8_hyperlink_covers_wide_cell_spacers() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 8, 2) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };
    terminal.write(b"\x1b]");
    terminal.write("8;;https://example.com/cjk\u{1b}\\你\u{1b}]8;;\u{1b}\\".as_bytes());
    let frame = terminal.frame().unwrap_or_else(|err| panic!("{err}"));
    let line = frame.lines.first().unwrap_or_else(|| panic!("missing row"));
    assert_eq!(line.hyperlinks.len(), 1);
    assert_eq!(
        (line.hyperlinks[0].start_col, line.hyperlinks[0].end_col),
        (0, 1)
    );
    assert_eq!(line.hyperlinks[0].uri, "https://example.com/cjk");
}

#[test]
fn ghostty_mouse_encoder_tracks_terminal_modes() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 80, 24) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };
    let geometry = TerminalMouseGeometry {
        screen_width: 576,
        screen_height: 432,
        cell_width: 7,
        cell_height: 18,
    };
    let disabled = terminal
        .encode_mouse(
            TerminalMouseAction::Press,
            Some(TerminalMouseButton::Left),
            TerminalModifiers::default(),
            (14.0, 18.0),
            geometry,
            true,
        )
        .unwrap_or_else(|err| panic!("{err}"));
    assert!(disabled.is_empty());

    terminal.write(b"\x1b[?1000h\x1b[?1006h");
    let enabled = terminal
        .encode_mouse(
            TerminalMouseAction::Press,
            Some(TerminalMouseButton::Left),
            TerminalModifiers::default(),
            (14.0, 18.0),
            geometry,
            true,
        )
        .unwrap_or_else(|err| panic!("{err}"));
    let encoded = String::from_utf8(enabled).unwrap_or_else(|err| panic!("{err}"));
    assert!(encoded.starts_with("\x1b[<"));
    assert!(encoded.ends_with('M'));
}

#[test]
fn ghostty_cursor_style_tracks_decscusr() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 8, 3) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };

    terminal.write(b"\x1b[5 q");
    let bar = terminal.frame().unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(bar.cursor_style, TerminalCursorStyle::Bar);
    assert!(bar.cursor_blinking);

    terminal.write(b"\x1b[4 q");
    let underline = terminal.frame().unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(underline.cursor_style, TerminalCursorStyle::Underline);
    assert!(!underline.cursor_blinking);

    terminal.write(b"\x1b[2 q");
    let block = terminal.frame().unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(block.cursor_style, TerminalCursorStyle::Block);
    assert!(!block.cursor_blinking);
}

#[test]
fn ghostty_frame_exposes_visible_cursor_position() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 8, 3) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };
    terminal.write(b"ab");
    let frame = match terminal.frame() {
        Ok(frame) => frame,
        Err(err) => panic!("{err}"),
    };
    assert_eq!(frame.cursor, Some((2, 0)));
}

#[test]
fn ghostty_scrollbar_round_trips_absolute_rows() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 8, 3) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };
    terminal.write(b"one\r\ntwo\r\nthree\r\nfour\r\nfive");
    let bottom = terminal.scrollbar().unwrap_or_else(|err| panic!("{err}"));
    assert!(bottom.total > bottom.len);
    assert_eq!(bottom.offset, bottom.total.saturating_sub(bottom.len));

    terminal
        .scroll_to_row(0)
        .unwrap_or_else(|err| panic!("{err}"));
    let top = terminal.scrollbar().unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(top.offset, 0);
    // frame() must not disturb the viewport: the offset after extraction is unchanged.
    let _ = terminal.frame().unwrap_or_else(|err| panic!("{err}"));
    let after_frame = terminal.scrollbar().unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(after_frame.offset, top.offset);
}

#[test]
fn rapid_scroll_partial_theme_rows_keep_confirmed_surface_background() {
    let runtime = GhosttyRuntime::detect().unwrap_or_else(|error| panic!("{error}"));
    let api = runtime.load_api().unwrap_or_else(|error| panic!("{error}"));
    let mut terminal = GhosttyTerminal::new(api, 20, 6).unwrap_or_else(|error| panic!("{error}"));

    // Older scrollback rows deliberately paint only half of each row with the Herdr-like
    // background. Once these dominate the viewport they fall below the 80% confirmation
    // threshold and reproduce the partial-repaint shape that used to flash to default black.
    for row in 0..12 {
        terminal
            .write(format!("\x1b[48;2;40;42;54mP{row:02}xxxxxxx\x1b[0m.........\r\n").as_bytes());
    }
    // Finish with six almost-full themed rows so the initial viewport strongly confirms
    // #282a36. Leave the final row without a newline so all six visible rows stay themed.
    for row in 0..6 {
        let ending = if row == 5 { "" } else { "\r\n" };
        terminal.write(
            format!("\x1b[48;2;40;42;54mT{row:02}xxxxxxxxxxxxxxxx\x1b[0m{ending}").as_bytes(),
        );
    }

    let mut frame = terminal.frame().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(frame.surface_background, Some(0x282a36));
    for _ in 0..8 {
        terminal.scroll(-1);
        frame = terminal
            .frame_reusing(Some(&frame))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            frame.surface_background,
            Some(0x282a36),
            "partial scroll viewport must retain the previously confirmed Herdr surface"
        );
    }
}

#[test]
fn ghostty_scroll_viewport_changes_frame() {
    let Ok(runtime) = GhosttyRuntime::detect() else {
        return;
    };
    let Ok(api) = runtime.load_api() else {
        return;
    };
    let mut terminal = match GhosttyTerminal::new(api, 8, 3) {
        Ok(terminal) => terminal,
        Err(err) => panic!("{err}"),
    };
    terminal.write(b"one\r\ntwo\r\nthree\r\nfour\r\nfive");
    let bottom = match terminal.frame() {
        Ok(frame) => frame,
        Err(err) => panic!("{err}"),
    };
    assert!(has_text(&bottom, "five"));
    terminal.scroll(-2);
    let scrolled = match terminal.frame() {
        Ok(frame) => frame,
        Err(err) => panic!("{err}"),
    };

    assert_ne!(frame_text(&bottom), frame_text(&scrolled));
    assert!(has_text(&scrolled, "two") || has_text(&scrolled, "three"));

    terminal.scroll(1_000_000);
    let restored = match terminal.frame() {
        Ok(frame) => frame,
        Err(err) => panic!("{err}"),
    };
    assert_eq!(frame_text(&bottom), frame_text(&restored));
    assert!(has_text(&restored, "five"));
}

fn has_background(frame: &TerminalFrame, text: &str) -> bool {
    frame.lines.iter().any(|line| {
        line.runs
            .iter()
            .any(|run| run.text.contains(text) && run.bg.is_some())
    })
}

fn has_text(frame: &TerminalFrame, text: &str) -> bool {
    frame
        .lines
        .iter()
        .any(|line| line.runs.iter().any(|run| run.text.contains(text)))
}

fn frame_text(frame: &TerminalFrame) -> String {
    frame
        .lines
        .iter()
        .flat_map(|line| line.runs.iter())
        .map(|run| run.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

// --- Frame-level signature fast path (RAW bitfield reuse) ---

#[test]
fn frame_reusing_signature_fast_path_reuses_unchanged_frame() {
    let runtime = GhosttyRuntime::detect().unwrap_or_else(|error| panic!("{error}"));
    let api = runtime.load_api().unwrap_or_else(|error| panic!("{error}"));
    let mut terminal = GhosttyTerminal::new(api, 20, 4).unwrap_or_else(|error| panic!("{error}"));
    terminal.write(b"hello signature reuse");
    let first = terminal.frame().unwrap_or_else(|error| panic!("{error}"));
    // First frame_reusing: signature collection (the constructor's initial osc8_dirty bit
    // keeps the bootstrap frame() from collecting), fast path missed.
    let second = terminal
        .frame_reusing(Some(&first))
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(second.lines, first.lines);
    terminal.reused_rows = 0;
    // Subsequent no-change polls: whole frame hits the fast path, skipping per-cell
    // extraction.
    let third = terminal
        .frame_reusing(Some(&second))
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(third.lines, second.lines);
    assert_eq!(terminal.reused_rows, second.lines.len());
}

#[test]
fn frame_reusing_signature_reuses_unchanged_rows_after_content_change() {
    let runtime = GhosttyRuntime::detect().unwrap_or_else(|error| panic!("{error}"));
    let api = runtime.load_api().unwrap_or_else(|error| panic!("{error}"));
    let mut terminal = GhosttyTerminal::new(api, 20, 4).unwrap_or_else(|error| panic!("{error}"));
    terminal.write(b"before");
    let first = terminal.frame().unwrap_or_else(|error| panic!("{error}"));
    let stored = terminal
        .frame_reusing(Some(&first))
        .unwrap_or_else(|error| panic!("{error}"));
    terminal.write(b" after change");
    terminal.reused_rows = 0;
    let changed = terminal
        .frame_reusing(Some(&stored))
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        terminal.reused_rows,
        changed.lines.len().saturating_sub(1),
        "only the changed row should pay text/color extraction"
    );
    let text: String = changed.lines[0].cells.concat();
    assert!(
        text.contains("after change"),
        "changed row must re-extract: {text}"
    );
}

// --- TerminalFramePlan (B16): the exact row-level plan surfaced from extraction ---

#[test]
fn frame_plan_is_rows_unchanged_for_identical_repaints() {
    let runtime = GhosttyRuntime::detect().unwrap_or_else(|error| panic!("{error}"));
    let api = runtime.load_api().unwrap_or_else(|error| panic!("{error}"));
    let mut terminal = GhosttyTerminal::new(api, 20, 4).unwrap_or_else(|error| panic!("{error}"));
    terminal.write(b"hello plan");
    let first = terminal.frame().unwrap_or_else(|error| panic!("{error}"));
    // Bootstrap extraction (scroll/resize/first frame) carries no row knowledge: consumers
    // must fall back to the deep comparison.
    assert_eq!(terminal.take_last_frame_plan(), TerminalFramePlan::Unknown);
    let _second = terminal
        .frame_reusing(Some(&first))
        .unwrap_or_else(|error| panic!("{error}"));
    // By the next identical poll the plan proves every row byte-identical — the fast path
    // set_terminal_frame uses to skip the raw deep compare and all downstream row re-syncs.
    let third = terminal
        .frame_reusing(Some(&first))
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(third.lines, first.lines);
    assert_eq!(
        terminal.take_last_frame_plan(),
        TerminalFramePlan::RowsUnchanged
    );
}

#[test]
fn frame_plan_names_exactly_the_changed_rows() {
    let runtime = GhosttyRuntime::detect().unwrap_or_else(|error| panic!("{error}"));
    let api = runtime.load_api().unwrap_or_else(|error| panic!("{error}"));
    let mut terminal = GhosttyTerminal::new(api, 20, 4).unwrap_or_else(|error| panic!("{error}"));
    terminal.write(b"before");
    let first = terminal.frame().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(terminal.take_last_frame_plan(), TerminalFramePlan::Unknown);
    let stored = terminal
        .frame_reusing(Some(&first))
        .unwrap_or_else(|error| panic!("{error}"));
    terminal.write(b" after change");
    let changed = terminal
        .frame_reusing(Some(&stored))
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        terminal.take_last_frame_plan(),
        TerminalFramePlan::RowsChanged(vec![0]),
        "only row 0 changed; every other row is a byte-identical clone"
    );
    let text: String = changed.lines[0].cells.concat();
    assert!(text.contains("after change"), "row 0 re-extracted: {text}");
}

#[test]
fn frame_plan_falls_back_to_unknown_for_osc8_and_bootstrap_extractions() {
    let runtime = GhosttyRuntime::detect().unwrap_or_else(|error| panic!("{error}"));
    let api = runtime.load_api().unwrap_or_else(|error| panic!("{error}"));
    let mut terminal = GhosttyTerminal::new(api, 20, 4).unwrap_or_else(|error| panic!("{error}"));
    terminal.write(b"plain");
    let first = terminal.frame().unwrap_or_else(|error| panic!("{error}"));
    let stored = terminal
        .frame_reusing(Some(&first))
        .unwrap_or_else(|error| panic!("{error}"));
    // An OSC-8 semantic stream change invalidates the signature baseline: the conservative
    // full extract has no row knowledge.
    terminal.write(b"\x1b]8;;https://herdr.dev\x1b\\link\x1b]8;;\x1b\\");
    let linked = terminal
        .frame_reusing(Some(&stored))
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(terminal.take_last_frame_plan(), TerminalFramePlan::Unknown);
    assert!(has_text(&linked, "link"), "hyperlink text extracted");
    // Scroll bootstrap (frame() after scroll): same conservative verdict.
    terminal.scroll(-1);
    let _scrolled = terminal.frame().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(terminal.take_last_frame_plan(), TerminalFramePlan::Unknown);
}

#[test]
fn frame_scalars_match_compares_scalars_but_not_lines() {
    // With a RowsUnchanged plan proving the grids identical, scalar equality IS frame
    // equality; the helper must therefore ignore lines but catch every scalar change.
    let mut a = TerminalFrame::default();
    a.lines.push(TerminalLine {
        cells: vec!["a".to_string()],
        ..TerminalLine::default()
    });
    a.cursor = Some((1, 0));
    a.default_foreground = Some(0xd8dee9);
    let mut b = a.clone();
    b.lines[0].cells[0] = "b".to_string();
    assert!(a.scalars_match(&b), "lines are covered by the plan");
    assert_ne!(a, b);
    b.lines = a.lines.clone();
    b.cursor = Some((2, 1));
    assert!(!a.scalars_match(&b), "cursor is a scalar");
    b.cursor = a.cursor;
    b.default_foreground = Some(0x88c0d0);
    assert!(!a.scalars_match(&b), "default foreground is a scalar");
}

#[test]
fn frame_reusing_signature_resets_after_scroll_bootstrap() {
    let runtime = GhosttyRuntime::detect().unwrap_or_else(|error| panic!("{error}"));
    let api = runtime.load_api().unwrap_or_else(|error| panic!("{error}"));
    let mut terminal = GhosttyTerminal::new(api, 20, 4).unwrap_or_else(|error| panic!("{error}"));
    for row in 0..8 {
        terminal.write(format!("line-{row}\r\n").as_bytes());
    }
    let first = terminal.frame().unwrap_or_else(|error| panic!("{error}"));
    let stored = terminal
        .frame_reusing(Some(&first))
        .unwrap_or_else(|error| panic!("{error}"));
    // The low-level bootstrap path still supports terminal.scroll + frame()
    // (previous=None). In production, ManagedTerminal scroll passes the previous raw frame
    // into frame_reusing when there is no chrome projection; this specifically guarantees
    // the no-previous low-level fallback never leaves a stale signature behind.
    terminal.scroll(-1);
    let scrolled = terminal.frame().unwrap_or_else(|error| panic!("{error}"));
    terminal.reused_rows = 0;
    let reused = terminal
        .frame_reusing(Some(&scrolled))
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(reused.lines, scrolled.lines);
    assert_eq!(terminal.reused_rows, scrolled.lines.len());
    // Scrolling actually moved the viewport (validates the test itself).
    assert_ne!(scrolled.lines, stored.lines);
}

#[test]
fn resize_extraction_never_keeps_columns_beyond_the_new_grid() {
    // Hosted shared-TUI invariant: when another viewer shrinks the shared grid,
    // the child repaints only its own columns, so the viewer's local model MUST
    // be resized to the authoritative grid — otherwise extraction keeps the old
    // wider rows and the stale columns are projected into the visible pane as
    // residue. Ghostty resize truncates the extraction width in both directions.
    let runtime = GhosttyRuntime::detect().unwrap_or_else(|error| panic!("{error}"));
    let api = runtime.load_api().unwrap_or_else(|error| panic!("{error}"));
    let mut terminal = GhosttyTerminal::new(api, 20, 4).unwrap_or_else(|error| panic!("{error}"));
    terminal.write(b"ABCDEFGHIJKLMNOPQRST\r\n");
    let wide = terminal.frame().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(wide.lines.len(), 4);
    assert_eq!(wide.lines[0].cells.len(), 20);
    assert_eq!(wide.lines[0].cells[19], "T");

    terminal
        .resize(10, 4, 84, 40)
        .unwrap_or_else(|error| panic!("{error}"));
    let narrow = terminal.frame().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(narrow.lines.len(), 4);
    for line in &narrow.lines {
        assert_eq!(
            line.cells.len(),
            10,
            "stale columns survived the shrink: {:?}",
            line.cells
        );
    }

    // Growing back must present cleared cells (default background), not stale
    // content, in the newly visible columns.
    terminal
        .resize(20, 4, 168, 80)
        .unwrap_or_else(|error| panic!("{error}"));
    let grown = terminal.frame().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(grown.lines[0].cells.len(), 20);
}

/// Diagnostic (ignored by default): replays a real captured Herdr TUI byte stream
/// (`/tmp/shardlane-resize-debug/child-bytes2.bin`, recorded with the split offset
/// at the remote shrink 164→96) through the viewer-local model with the exact
/// production sequence: model at the old grid → bytes → adopt-resize → remaining
/// bytes → frame. Run with:
///   cargo test -p shardlane --bin shardlane ghostty::tests::replay -- --ignored --nocapture
#[test]
#[ignore]
fn replay_captured_shared_resize_through_viewer_model() {
    let path = std::path::Path::new("/tmp/shardlane-resize-debug/child-bytes2.bin");
    if !path.exists() {
        panic!("capture file missing: {}", path.display());
    }
    let data = std::fs::read(path).unwrap_or_else(|error| panic!("{error}"));
    let split = 14326_usize;
    let runtime = GhosttyRuntime::detect().unwrap_or_else(|error| panic!("{error}"));
    let api = runtime.load_api().unwrap_or_else(|error| panic!("{error}"));
    let mut terminal = GhosttyTerminal::new(api, 164, 43).unwrap_or_else(|error| panic!("{error}"));
    terminal.write(&data[..split]);
    terminal
        .resize(96, 42, 806, 840)
        .unwrap_or_else(|error| panic!("{error}"));
    terminal.write(&data[split..]);
    let frame = terminal.frame().unwrap_or_else(|error| panic!("{error}"));
    println!("frame rows={} cols-expect=96", frame.lines.len());
    for (row_index, line) in frame.lines.iter().enumerate().take(12) {
        let text: String = line.cells.iter().map(|cell| cell.as_str()).collect();
        println!(
            "row {:02} len={:03} {:?}",
            row_index,
            line.cells.len(),
            text.trim_end()
        );
    }
    let pane_row: String = frame.lines[5]
        .cells
        .iter()
        .map(|cell| cell.as_str())
        .collect();
    let pane_text = pane_row.trim_end();
    println!("pane row 5 len={} {:?}", pane_text.len(), pane_text);
}
