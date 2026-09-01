//! Key-name mapping layer for terminal key input: GPUI Keystroke → terminal-encoded input.
//!
//! [INPUT]: Depends on crepuscularity_gpui's Keystroke and `crate::ghostty`'s TerminalKey
//! [OUTPUT]: Exposes `key_name` (Script hotkeys/debug naming),
//!           `should_swallow_gui_keystroke_with_config` (the sole key-swallowing decision — live
//!             resolved Shortcut Registry; dropped overrides / disabled keys pass through to the host terminal),
//!           `ghostty_terminal_key` (GPUI key → Ghostty physical key + base codepoint),
//!           `legacy_alt_fallback_byte` (Option-as-Alt fallback byte for pure VT mode),
//!           platform modifier/chord helpers (serialized with GPUI's actual platform key names)
//! [POS]: The mapping layer ahead of `main.rs` handle_keystroke; keys with no mapping fall back to Herdr

use crate::ghostty::TerminalKey;
use crate::shortcuts::{chord_is_bound, ShortcutConfig};
use crepuscularity_gpui::Keystroke;

/// GPUI's `modifiers.platform` is Cmd on macOS, Super on Linux, and Win on
/// Windows. The shortcut registry's *default* profile is intentionally
/// separate: Linux/Windows use Ctrl, which arrives as `modifiers.control`.
pub(crate) fn platform_modifier_name() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "cmd"
    }

    #[cfg(target_os = "windows")]
    {
        "win"
    }

    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        "super"
    }
}

#[cfg(test)]
fn shortcut_primary_modifier_name() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "cmd"
    }

    #[cfg(not(target_os = "macos"))]
    {
        "ctrl"
    }
}

pub fn key_name(key: &Keystroke) -> String {
    let mut name = String::new();
    if key.modifiers.control {
        name.push_str("ctrl+");
    }
    if key.modifiers.alt {
        name.push_str("alt+");
    }
    if key.modifiers.shift {
        name.push_str("shift+");
    }
    if key.modifiers.platform {
        name.push_str(platform_modifier_name());
        name.push('+');
    }
    name.push_str(match key.key.as_str() {
        "enter" => "enter",
        "backspace" => "backspace",
        "tab" => "tab",
        "escape" => "escape",
        "up" => "up",
        "down" => "down",
        "right" => "right",
        "left" => "left",
        "delete" => "delete",
        "home" => "home",
        "end" => "end",
        "pageup" => "pageup",
        "pagedown" => "pagedown",
        "insert" => "insert",
        other => other,
    });
    name
}

/// GPUI's macOS parser folds a shifted number/punctuation key into the `key`
/// string and clears `modifiers.shift` (the symbol itself carries the shift).
/// The terminal encoder restores that consumed modifier; GUI shortcut/script
/// names intentionally keep the raw GPUI key so existing bindings remain stable.
pub(crate) fn terminal_key_uses_implicit_shift(key: &Keystroke) -> bool {
    !key.modifiers.shift
        && matches!(
            key.key.as_str(),
            "!" | "@"
                | "#"
                | "$"
                | "%"
                | "^"
                | "&"
                | "*"
                | "("
                | ")"
                | "_"
                | "+"
                | "{"
                | "}"
                | "|"
                | ":"
                | "\""
                | "<"
                | ">"
                | "?"
                | "~"
        )
}

/// The vendored Ghostty encoder intentionally leaves Option/Alt printable
/// chords empty in legacy mode because the macOS "option as alt" preference
/// belongs to the host application.  Shardlane still owns the hosted PTY, so
/// preserve the terminal convention explicitly as `ESC` + the displayed ASCII
/// byte instead of silently swallowing the key.
pub(crate) fn legacy_alt_fallback_byte(key: &Keystroke, unshifted_codepoint: u32) -> Option<u8> {
    if !key.modifiers.alt || key.modifiers.control || key.modifiers.platform {
        return None;
    }
    let key_bytes = key.key.as_bytes();
    let mut byte = if key_bytes.len() == 1 && key_bytes[0].is_ascii_graphic() {
        key_bytes[0]
    } else {
        u8::try_from(unshifted_codepoint).ok()?
    };
    if key.modifiers.shift && byte.is_ascii_lowercase() {
        byte = byte.to_ascii_uppercase();
    }
    (byte == b' ' || byte.is_ascii_graphic()).then_some(byte)
}

/// GPUI key event → Ghostty physical key + unmodified base codepoint (consumed by the key encoder).
/// Named keys have codepoint 0; printable keys use the lowercase base character. utf8 is always
/// decided by the encoding layer; dead-key text is not passed here (alt's meta semantics are the
/// encoder/fallback layer's responsibility).
/// Unmapped keys (non-ASCII layout keys, unknown names) return None and are left to the
/// caller for input-method/host handling.
pub fn ghostty_terminal_key(key: &Keystroke) -> Option<(TerminalKey, u32)> {
    let named = match key.key.as_str() {
        "enter" => Some(TerminalKey::Enter),
        "backspace" => Some(TerminalKey::Backspace),
        "tab" => Some(TerminalKey::Tab),
        "escape" => Some(TerminalKey::Escape),
        "delete" => Some(TerminalKey::Delete),
        "home" => Some(TerminalKey::Home),
        "end" => Some(TerminalKey::End),
        "pageup" => Some(TerminalKey::PageUp),
        "pagedown" => Some(TerminalKey::PageDown),
        "insert" => Some(TerminalKey::Insert),
        "space" => Some(TerminalKey::Space),
        "up" => Some(TerminalKey::ArrowUp),
        "down" => Some(TerminalKey::ArrowDown),
        "left" => Some(TerminalKey::ArrowLeft),
        "right" => Some(TerminalKey::ArrowRight),
        "f1" => Some(TerminalKey::F1),
        "f2" => Some(TerminalKey::F2),
        "f3" => Some(TerminalKey::F3),
        "f4" => Some(TerminalKey::F4),
        "f5" => Some(TerminalKey::F5),
        "f6" => Some(TerminalKey::F6),
        "f7" => Some(TerminalKey::F7),
        "f8" => Some(TerminalKey::F8),
        "f9" => Some(TerminalKey::F9),
        "f10" => Some(TerminalKey::F10),
        "f11" => Some(TerminalKey::F11),
        "f12" => Some(TerminalKey::F12),
        "f13" => Some(TerminalKey::F13),
        "f14" => Some(TerminalKey::F14),
        "f15" => Some(TerminalKey::F15),
        "f16" => Some(TerminalKey::F16),
        "f17" => Some(TerminalKey::F17),
        "f18" => Some(TerminalKey::F18),
        "f19" => Some(TerminalKey::F19),
        "f20" => Some(TerminalKey::F20),
        "f21" => Some(TerminalKey::F21),
        "f22" => Some(TerminalKey::F22),
        "f23" => Some(TerminalKey::F23),
        "f24" => Some(TerminalKey::F24),
        _ => None,
    };
    if let Some(key) = named {
        return Some((key, 0));
    }

    // Single printable ASCII character (the base key of a modifier chord); multi-character/non-ASCII returns None.
    // On macOS, GPUI normalizes "shift + digit/punctuation" into the shifted character name,
    // e.g. Cmd+Shift+7 → `&`, Ctrl+Shift+/ → `?`. Ghostty needs the physical
    // base key + unshifted codepoint to produce correct CSI/Kitty chords, so this
    // accepts the full set of US-keyboard shifted symbols, not just unshifted characters.
    let mut chars = key.key.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if !c.is_ascii() {
            return None;
        }
        let lowered = c.to_ascii_lowercase();
        let mapped = match lowered {
            'a'..='z' => Some(LETTER_KEYS[usize::from(lowered as u8 - b'a')]),
            '0'..='9' => Some(DIGIT_KEYS[usize::from(lowered as u8 - b'0')]),
            ',' => Some(TerminalKey::Comma),
            '.' => Some(TerminalKey::Period),
            '/' => Some(TerminalKey::Slash),
            ';' => Some(TerminalKey::Semicolon),
            '\'' => Some(TerminalKey::Quote),
            '-' => Some(TerminalKey::Minus),
            '=' => Some(TerminalKey::Equal),
            '[' => Some(TerminalKey::BracketLeft),
            ']' => Some(TerminalKey::BracketRight),
            '\\' => Some(TerminalKey::Backslash),
            '`' => Some(TerminalKey::Backquote),
            ' ' => Some(TerminalKey::Space),
            '!' => Some(TerminalKey::Digit1),
            '@' => Some(TerminalKey::Digit2),
            '#' => Some(TerminalKey::Digit3),
            '$' => Some(TerminalKey::Digit4),
            '%' => Some(TerminalKey::Digit5),
            '^' => Some(TerminalKey::Digit6),
            '&' => Some(TerminalKey::Digit7),
            '*' => Some(TerminalKey::Digit8),
            '(' => Some(TerminalKey::Digit9),
            ')' => Some(TerminalKey::Digit0),
            '_' => Some(TerminalKey::Minus),
            '+' => Some(TerminalKey::Equal),
            '{' => Some(TerminalKey::BracketLeft),
            '}' => Some(TerminalKey::BracketRight),
            '|' => Some(TerminalKey::Backslash),
            ':' => Some(TerminalKey::Semicolon),
            '"' => Some(TerminalKey::Quote),
            '<' => Some(TerminalKey::Comma),
            '>' => Some(TerminalKey::Period),
            '?' => Some(TerminalKey::Slash),
            '~' => Some(TerminalKey::Backquote),
            _ => None,
        }?;
        let unshifted = match lowered {
            '!' => b'1',
            '@' => b'2',
            '#' => b'3',
            '$' => b'4',
            '%' => b'5',
            '^' => b'6',
            '&' => b'7',
            '*' => b'8',
            '(' => b'9',
            ')' => b'0',
            '_' => b'-',
            '+' => b'=',
            '{' => b'[',
            '}' => b']',
            '|' => b'\\',
            ':' => b';',
            '"' => b'\'',
            '<' => b',',
            '>' => b'.',
            '?' => b'/',
            '~' => b'`',
            other => other as u8,
        };
        return Some((mapped, u32::from(unshifted)));
    }
    None
}

const LETTER_KEYS: [TerminalKey; 26] = [
    TerminalKey::A,
    TerminalKey::B,
    TerminalKey::C,
    TerminalKey::D,
    TerminalKey::E,
    TerminalKey::F,
    TerminalKey::G,
    TerminalKey::H,
    TerminalKey::I,
    TerminalKey::J,
    TerminalKey::K,
    TerminalKey::L,
    TerminalKey::M,
    TerminalKey::N,
    TerminalKey::O,
    TerminalKey::P,
    TerminalKey::Q,
    TerminalKey::R,
    TerminalKey::S,
    TerminalKey::T,
    TerminalKey::U,
    TerminalKey::V,
    TerminalKey::W,
    TerminalKey::X,
    TerminalKey::Y,
    TerminalKey::Z,
];

const DIGIT_KEYS: [TerminalKey; 10] = [
    TerminalKey::Digit0,
    TerminalKey::Digit1,
    TerminalKey::Digit2,
    TerminalKey::Digit3,
    TerminalKey::Digit4,
    TerminalKey::Digit5,
    TerminalKey::Digit6,
    TerminalKey::Digit7,
    TerminalKey::Digit8,
    TerminalKey::Digit9,
];

/// SCT-02 / P1-4 (audit 2026-08-27 revalidation): the swallow decision derives from the
/// LIVE user shortcut configuration, not compile-time defaults. Consequences: an
/// overridden chord fires its GPUI action exactly once (the old default chord now falls
/// through to the terminal), and a disabled shortcut neither fires nor is swallowed.
pub fn should_swallow_gui_keystroke_with_config(key: &Keystroke, config: &ShortcutConfig) -> bool {
    // F1 is the one unmodified application chord.  Resolve it through the same
    // live registry as every modified binding so disabling/overriding Help does
    // not leave an invisible hard-coded swallow in front of the hosted TUI.
    if !key.modifiers.platform
        && !key.modifiers.control
        && !key.modifiers.alt
        && !key.modifiers.function
        && key.key != "f1"
    {
        return false;
    }
    chord_is_bound(&keystroke_chord(key), config)
}

/// Keystroke → registry chord string ("cmd-shift-a" form).
pub(crate) fn keystroke_chord(key: &Keystroke) -> String {
    let mut chord = String::new();
    if key.modifiers.platform {
        chord.push_str(platform_modifier_name());
        chord.push('-');
    }
    if key.modifiers.control {
        chord.push_str("ctrl-");
    }
    if key.modifiers.alt {
        chord.push_str("alt-");
    }
    if key.modifiers.shift {
        chord.push_str("shift-");
    }
    chord.push_str(&key.key);
    chord
}

#[cfg(test)]
mod tests {
    use super::*;
    use crepuscularity_gpui::Keystroke;

    fn ks(key: &str, platform: bool, shift: bool) -> Keystroke {
        Keystroke {
            key: key.to_string(),
            key_char: None,
            modifiers: crepuscularity_gpui::Modifiers {
                control: false,
                alt: false,
                shift,
                platform,
                function: false,
            },
        }
    }

    fn primary_ks(key: &str, shift: bool) -> Keystroke {
        let mut keystroke = ks(key, cfg!(target_os = "macos"), shift);
        if !cfg!(target_os = "macos") {
            keystroke.modifiers.control = true;
        }
        keystroke
    }

    #[test]
    fn cmd_backspace_encodes_platform_modifier() {
        let expected = format!("{}+backspace", platform_modifier_name());
        assert_eq!(key_name(&ks("backspace", true, false)), expected);
    }

    #[test]
    fn named_space_maps_to_terminal_space() {
        assert_eq!(
            ghostty_terminal_key(&ks("space", false, false)),
            Some((TerminalKey::Space, 0))
        );
    }

    #[test]
    fn swallow_derives_from_resolved_registry_not_a_second_table() {
        // Sole key-swallowing decision: swallow only on a registry hit; unregistered/unmodified keys pass through to the host terminal.
        let config = ShortcutConfig::default();
        assert!(should_swallow_gui_keystroke_with_config(
            &primary_ks("v", false),
            &config
        ));
        assert!(!should_swallow_gui_keystroke_with_config(
            &primary_ks("backspace", false),
            &config
        ));
        // Disabled shortcuts are no longer swallowed (the key passes through to the host terminal and triggers no action).
        let mut disabled = ShortcutConfig::default();
        disabled.disabled.insert("terminal.paste".to_string());
        assert!(!should_swallow_gui_keystroke_with_config(
            &primary_ks("v", false),
            &disabled
        ));
        // User override to a new chord: the new chord is swallowed, the old default chord passes through (P1-4 semantics).
        let mut overridden = ShortcutConfig::default();
        let primary = crate::input::shortcut_primary_modifier_name();
        overridden.overrides.insert(
            "terminal.paste".to_string(),
            vec![format!("{primary}-shift-v")],
        );
        assert!(!should_swallow_gui_keystroke_with_config(
            &primary_ks("v", false),
            &overridden
        ));
        assert!(should_swallow_gui_keystroke_with_config(
            &{ primary_ks("v", true) },
            &overridden
        ));

        let mut f1_disabled = ShortcutConfig::default();
        f1_disabled.disabled.insert("app.help".to_string());
        assert!(!should_swallow_gui_keystroke_with_config(
            &ks("f1", false, false),
            &f1_disabled
        ));
    }

    #[test]
    fn ghostty_terminal_key_maps_named_and_printable_keys() {
        assert_eq!(
            ghostty_terminal_key(&ks("enter", false, false)),
            Some((TerminalKey::Enter, 0))
        );
        assert_eq!(
            ghostty_terminal_key(&ks("pageup", false, false)),
            Some((TerminalKey::PageUp, 0))
        );
        assert_eq!(
            ghostty_terminal_key(&ks("f5", false, false)),
            Some((TerminalKey::F5, 0))
        );
        assert_eq!(
            ghostty_terminal_key(&ks("b", false, false)),
            Some((TerminalKey::B, u32::from(b'b')))
        );
        assert_eq!(
            ghostty_terminal_key(&ks("B", false, true)),
            Some((TerminalKey::B, u32::from(b'b')))
        );
        assert_eq!(
            ghostty_terminal_key(&ks("5", false, false)),
            Some((TerminalKey::Digit5, u32::from(b'5')))
        );
        assert_eq!(
            ghostty_terminal_key(&ks("-", false, false)),
            Some((TerminalKey::Minus, u32::from(b'-')))
        );
    }

    #[test]
    fn ghostty_terminal_key_maps_shifted_us_symbols_to_physical_keys() {
        // GPUI's macOS parser reports shifted number/punctuation symbols as the
        // key name for modified chords (e.g. Cmd+Shift+7 becomes `&`). Keep the
        // physical key and unshifted codepoint so Ghostty can encode the chord.
        for (symbol, key, base) in [
            ("!", TerminalKey::Digit1, b'1'),
            ("@", TerminalKey::Digit2, b'2'),
            ("&", TerminalKey::Digit7, b'7'),
            ("_", TerminalKey::Minus, b'-'),
            ("+", TerminalKey::Equal, b'='),
            ("{", TerminalKey::BracketLeft, b'['),
            ("|", TerminalKey::Backslash, b'\\'),
            (":", TerminalKey::Semicolon, b';'),
            ("\"", TerminalKey::Quote, b'\''),
            ("<", TerminalKey::Comma, b','),
            (">", TerminalKey::Period, b'.'),
            ("?", TerminalKey::Slash, b'/'),
            ("~", TerminalKey::Backquote, b'`'),
        ] {
            assert_eq!(
                ghostty_terminal_key(&ks(symbol, true, false)),
                Some((key, u32::from(base))),
                "shifted symbol {symbol} must retain its physical key"
            );
        }
    }

    #[test]
    fn ghostty_terminal_key_rejects_unmapped_keys() {
        // Multi-character and non-ASCII layout keys fall back to the Herdr send_keys path.
        assert_eq!(ghostty_terminal_key(&ks("f25", false, false)), None);
        assert_eq!(ghostty_terminal_key(&ks("ä", false, false)), None);
    }

    #[test]
    fn platform_modifier_serializes_to_registry_profile() {
        let platform_key = ks("k", true, false);
        assert_eq!(
            keystroke_chord(&platform_key),
            format!("{}-k", platform_modifier_name())
        );
        let primary_key = primary_ks("k", false);
        assert_eq!(
            keystroke_chord(&primary_key),
            format!("{}-k", shortcut_primary_modifier_name())
        );
    }

    #[test]
    fn shifted_symbol_shortcut_chords_preserve_gpui_key_names() {
        let key = ks("}", true, false);
        assert_eq!(
            keystroke_chord(&key),
            format!("{}-}}", platform_modifier_name())
        );
        assert_eq!(key_name(&key), format!("{}+}}", platform_modifier_name()));
    }

    #[test]
    fn legacy_alt_fallback_keeps_printable_option_chords_lossless() {
        let alt_key = |key: &str, shift: bool| {
            let mut key = ks(key, false, shift);
            key.modifiers.alt = true;
            key
        };
        assert_eq!(
            legacy_alt_fallback_byte(&alt_key("b", false), b'b'.into()),
            Some(b'b')
        );
        assert_eq!(
            legacy_alt_fallback_byte(&alt_key("b", true), b'b'.into()),
            Some(b'B')
        );
        assert_eq!(
            legacy_alt_fallback_byte(&alt_key("?", true), b'/'.into()),
            Some(b'?')
        );
        assert_eq!(legacy_alt_fallback_byte(&alt_key("enter", false), 0), None);
        let mut ctrl_alt = alt_key("b", false);
        ctrl_alt.modifiers.control = true;
        assert_eq!(legacy_alt_fallback_byte(&ctrl_alt, b'b'.into()), None);
    }
}
