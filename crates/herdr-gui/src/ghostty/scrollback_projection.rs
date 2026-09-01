//! [INPUT]: Depends on the crate::ghostty module-root re-export surface (`use super::*`) and
//! std memory/synchronization primitives.
//! [OUTPUT]: Exposes (within the ghostty module tree) scroll-viewport projection regressions
//! (pinned viewport; the scrollbar offset remains the test-only observable).
//! [POS]: Scroll-viewport projection regressions (pinned viewport, scrollbar semantics) of the
//! ghostty module.

use super::*;

fn setup(api: Arc<GhosttyApi>) -> GhosttyTerminal {
    GhosttyTerminal::new(api, 20, 4).unwrap_or_else(|error| panic!("{error}"))
}

fn write_table(t: &mut GhosttyTerminal) {
    t.write(b"\x1b[36m+-----+\x1b[0m\r\n");
    t.write(b"\x1b[36m|\x1b[0m dat \x1b[36m|\x1b[0m\r\n");
    t.write(b"\x1b[36m+-----+\x1b[0m\r\n");
    for _ in 0..8 {
        t.write(b"\r\n");
    }
    t.write(b"bottom");
}

/// Contract: scrollback rows must retain styles and border characters while the viewport is
/// pinned; resize reflow must not lose content; new output while pinned must not yank the
/// viewport back to the bottom. Seed window depth is set by the bootstrap constants in main.rs.
#[test]
fn scrollback_retains_styles_borders_and_pin_across_resize_and_output() {
    let runtime = GhosttyRuntime::detect().unwrap_or_else(|error| panic!("{error}"));
    let api = runtime.load_api().unwrap_or_else(|error| panic!("{error}"));

    let mut t = setup(api.clone());
    write_table(&mut t);
    let bottom = t.frame().unwrap_or_else(|error| panic!("{error}"));
    let _ = bottom;
    assert_eq!(
        t.scrollbar()
            .unwrap_or_else(|error| panic!("{error}"))
            .offset,
        8,
        "seeded table sits above viewport"
    );

    // Scrolled to top: table row styles and borders must be intact.
    t.scroll(-8isize);
    let top = t.frame().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        t.scrollbar()
            .unwrap_or_else(|error| panic!("{error}"))
            .offset,
        0
    );
    let row_text = |i: usize| top.lines[i].cells.concat();
    assert_eq!(row_text(0).trim_end(), "+-----+");
    assert_eq!(row_text(1).trim_end(), "| dat |");
    assert_eq!(row_text(2).trim_end(), "+-----+");
    let border_run = top.lines[0]
        .runs
        .iter()
        .find(|run| run.text.contains('+'))
        .unwrap_or_else(|| panic!("border run missing"));
    assert_ne!(
        border_run.fg, 0x00c5_ceda,
        "scrollback rows must keep SGR color"
    );

    // Resize reflow: content still present, not cleared.
    t.resize(16, 4, 320, 96)
        .unwrap_or_else(|error| panic!("{error}"));
    let after_resize = t.frame().unwrap_or_else(|error| panic!("{error}"));
    let joined = after_resize
        .lines
        .iter()
        .map(|l| l.cells.concat())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains("+-----+"),
        "resize must not erase scrollback"
    );

    // New output arrives while the viewport is pinned: the viewport must not jump to the bottom.
    let mut t2 = setup(api);
    write_table(&mut t2);
    t2.scroll(-8isize);
    t2.write(b" new-output");
    let pinned = t2.frame().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        t2.scrollbar()
            .unwrap_or_else(|error| panic!("{error}"))
            .offset,
        0,
        "pinned viewport must hold on new output"
    );
    assert!(pinned.lines[0].cells.concat().contains('+'));
}
