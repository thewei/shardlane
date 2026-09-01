//! [INPUT]: Depends on the crate::ghostty module-root re-export surface (`use super::*`) and
//! std memory/synchronization primitives.
//! [OUTPUT]: Exposes (within the ghostty module tree) alt-screen detection and BEL counting
//! regressions plus #[ignore] perf_* micro-benchmarks (magnitudes of the presentation hot
//! paths).
//! [POS]: Alt-screen detection and BEL counting regressions plus #[ignore] perf_*
//! micro-benchmarks (magnitudes of the presentation hot paths) of the ghostty module.

use std::time::Instant;

use super::*;

fn setup(api: Arc<GhosttyApi>) -> GhosttyTerminal {
    GhosttyTerminal::new(api, 20, 4).unwrap_or_else(|error| panic!("{error}"))
}

fn runtime() -> Arc<GhosttyApi> {
    let runtime = GhosttyRuntime::detect().unwrap_or_else(|error| panic!("{error}"));
    runtime.load_api().unwrap_or_else(|error| panic!("{error}"))
}

/// Performance micro-benchmarks (run manually via `cargo test -- --ignored perf_`; not part
/// of the default gate): pin the magnitudes of the three major terminal presentation hot
/// paths, providing reproducible numbers for optimization decisions.
#[test]
#[ignore]
fn perf_frame_extraction_equality_and_search_hotpaths() {
    let api = runtime();
    let cols = 120_u16;
    let rows = 40_u16;
    let mut t = GhosttyTerminal::new(api, cols, rows).unwrap_or_else(|e| panic!("{e}"));
    // Typical TUI scenario: multi-color styled lines with a newline at the end of each.
    let line = b"\x1b[32mok\x1b[0m \x1b[1;34msrc/main.rs\x1b[0m:\x1b[33m128\x1b[0m: warning: unused variable `x` \x1b[90m[--Wunused]\x1b[0m";
    for _ in 0..rows {
        t.write(line);
        t.write(b"\r\n");
    }
    let baseline = t.frame().unwrap_or_else(|e| panic!("{e}"));

    // 1) Frame extraction (pure extraction cost with no new output; happens on every poll
    // while output is active).
    let iterations = 500_u32;
    let started = Instant::now();
    for _ in 0..iterations {
        let _ = t
            .frame_reusing(Some(&baseline))
            .unwrap_or_else(|e| panic!("{e}"));
    }
    let extract_us = started.elapsed().as_micros() as f64 / f64::from(iterations);

    // 2) set_frame deep equality (full PartialEq comparison after Arc::ptr_eq fails).
    let candidate = t.frame().unwrap_or_else(|e| panic!("{e}"));
    let started = Instant::now();
    let mut equal_count = 0_u32;
    for _ in 0..iterations {
        if baseline == candidate {
            equal_count += 1;
        }
    }
    let equality_us = started.elapsed().as_micros() as f64 / f64::from(iterations);
    assert_eq!(equal_count, iterations);

    // 3) Search: a hydration window of 10k rows x 80 cells per row. (The terminal_search
    //    module tests cannot access GhosttyRuntime directly, so this only measures the concat
    //    cost of the line_text-isomorphic shape.)
    let long_line: Vec<String> = (0..80).map(|i| i.to_string()).collect();
    let refs: Vec<&str> = long_line.iter().map(String::as_str).collect();
    let lines: Vec<TerminalLine> = (0..10_000)
        .map(|_| TerminalLine {
            cells: long_line.clone(),
            ..TerminalLine::default()
        })
        .collect();
    let started = Instant::now();
    let mut total_chars = 0_usize;
    for line in &lines {
        total_chars += line.cells.concat().len();
        total_chars += refs.len(); // silence the unused warning
    }
    let concat_ms = started.elapsed().as_secs_f64() * 1000.0;
    assert!(total_chars > 0);

    // 4) VT ingest throughput: parse time on the UI thread for one drain budget (256KB).
    let chunk: Vec<u8> = line.repeat(8); // ~4KB per write, simulating a log-line stream
    let batch: Vec<u8> = chunk.repeat(64); // ~256KB
    let started = Instant::now();
    let mut rounds = 0_u32;
    while started.elapsed().as_secs_f64() < 0.5 {
        t.write(&batch);
        rounds += 1;
    }
    let parsed_mb = f64::from(rounds) * batch.len() as f64 / 1e6;
    let parse_ms_per_256k = 0.5 / f64::from(rounds) * 1000.0;

    println!(
        "perf: viewport {cols}x{rows} cells={} runs={}\n  frame extraction: {extract_us:.1}us/frame ({:.1}% of 16.7ms budget @60fps)\n  deep equality:    {equality_us:.1}us/frame\n  10k-row concat:   {concat_ms:.1}ms\n  VT ingest:        {parsed_mb:.0}MB/0.5s -> {parse_ms_per_256k:.2}ms per 256KB drain budget\n",
        baseline.lines.iter().map(|l| l.cells.len()).sum::<usize>(),
        baseline.lines.iter().map(|l| l.runs.len()).sum::<usize>(),
        extract_us / 1000.0 / 16.7 * 100.0,
    );
}

#[test]
fn alt_screen_tracks_all_three_decset_entries_and_exit() {
    let api = runtime();
    for enter in [
        b"\x1b[?1049h".as_slice(),
        b"\x1b[?1047h".as_slice(),
        b"\x1b[?47h".as_slice(),
    ] {
        let mut t = setup(api.clone());
        assert!(!t.is_alternate_screen(), "primary screen before {enter:?}");
        t.write(enter);
        assert!(t.is_alternate_screen(), "alt screen after {enter:?}");
        // Corresponding exit sequence returns to the primary screen.
        let exit: &[u8] = match enter {
            b"\x1b[?1049h" => b"\x1b[?1049l",
            b"\x1b[?1047h" => b"\x1b[?1047l",
            _ => b"\x1b[?47l",
        };
        t.write(exit);
        assert!(!t.is_alternate_screen(), "primary screen after {exit:?}");
    }
}

#[test]
fn ground_bell_rings_and_is_counted_once() {
    let api = runtime();
    let mut t = setup(api);
    t.write(b"done\x07 waiting\x07\x07");
    assert_eq!(t.take_pending_bells(), 3);
    assert_eq!(t.take_pending_bells(), 0, "cleared to zero after take");
}

#[test]
fn osc_terminator_bell_does_not_ring_but_st_bell_after_osc_also_silent() {
    let api = runtime();
    let mut t = setup(api);
    // BEL-terminated OSC (title): no ring.
    t.write(b"\x1b]0;title\x07");
    assert_eq!(t.take_pending_bells(), 0);
    // Ground BEL following an ST-terminated OSC: rings.
    t.write(b"\x1b]8;;http://e\x1b\\ ping\x07");
    assert_eq!(t.take_pending_bells(), 1);
}

#[test]
fn dcs_string_bell_is_ignored_and_st_ends_string() {
    let api = runtime();
    let mut t = setup(api);
    t.write(b"\x1bPq payload \x07 stays\x1b\\ then\x07");
    assert_eq!(
        t.take_pending_bells(),
        1,
        "only a Ground BEL after ST rings"
    );
}

#[test]
fn bell_scan_survives_arbitrary_chunk_splitting() {
    let api = runtime();
    let mut t = setup(api);
    let stream = b"\x1b]0;ti\x07tle\x07 \x1b[?1049h\x07ring";
    let mut total = 0;
    for chunk in stream.chunks(3) {
        t.write(chunk);
        total += t.take_pending_bells();
    }
    assert_eq!(
        total, 2,
        "OSC terminators do not ring; the BEL after alt screen does"
    );
}
