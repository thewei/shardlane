//! Semantic input encoding for the Host-owned shared Herdr TUI.
//!
//! [INPUT]: Host DTO `HerdrTuiInput` events or the legacy renderer-produced
//! raw `data` payload used by WTerm.
//! [OUTPUT]: bounded PTY bytes; named keys are encoded once here so Mobile and
//! other Remote viewers never handcraft terminal escape sequences.
//! [POS]: Remote TUI transport's input adapter. It owns no process, session,
//! Herdr runtime, or Conversation semantics.

use shardlane_host::{HerdrTuiInput, HerdrTuiKeyCode, HerdrTuiModifiers};
use std::fmt;

pub(crate) const MAX_INPUT_BYTES: usize = 64 * 1024;

/// `/input` accepts renderer-produced bytes for compatibility and a typed
/// event shape for native/mobile callers. The untagged wrapper keeps existing
/// Web WTerm clients working while making semantic input the preferred path.
#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
pub(crate) enum TuiInputRequest {
    Raw { data: String },
    Event(HerdrTuiInput),
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct InputBytes(pub(crate) Vec<u8>);

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct InputEncodingError(pub(crate) String);

impl fmt::Display for InputEncodingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

pub(crate) fn encode_request(request: TuiInputRequest) -> Result<InputBytes, InputEncodingError> {
    let bytes = match request {
        TuiInputRequest::Raw { data } => data.into_bytes(),
        TuiInputRequest::Event(event) => encode_event(event)?,
    };
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(InputEncodingError(format!(
            "TUI input exceeds {MAX_INPUT_BYTES} bytes"
        )));
    }
    Ok(InputBytes(bytes))
}

fn encode_event(event: HerdrTuiInput) -> Result<Vec<u8>, InputEncodingError> {
    match event {
        HerdrTuiInput::Text { text } | HerdrTuiInput::Paste { text } => Ok(text.into_bytes()),
        HerdrTuiInput::Key { code, modifiers } => encode_key(code, modifiers),
    }
}

fn encode_key(
    code: HerdrTuiKeyCode,
    modifiers: HerdrTuiModifiers,
) -> Result<Vec<u8>, InputEncodingError> {
    let modifier = modifier_parameter(modifiers);
    let bytes = match code {
        HerdrTuiKeyCode::Enter => simple_byte(b'\r', modifiers),
        HerdrTuiKeyCode::Escape => simple_byte(0x1b, modifiers),
        HerdrTuiKeyCode::Tab => {
            if modifiers.shift && !modifiers.ctrl && !modifiers.alt && !modifiers.meta {
                b"\x1b[Z".to_vec()
            } else {
                simple_byte(b'\t', modifiers)
            }
        }
        HerdrTuiKeyCode::Backspace => {
            simple_byte(if modifiers.ctrl { 0x08 } else { 0x7f }, modifiers)
        }
        HerdrTuiKeyCode::Space => simple_byte(if modifiers.ctrl { 0 } else { b' ' }, modifiers),
        HerdrTuiKeyCode::ArrowUp => modified_csi("A", modifier),
        HerdrTuiKeyCode::ArrowDown => modified_csi("B", modifier),
        HerdrTuiKeyCode::ArrowRight => modified_csi("C", modifier),
        HerdrTuiKeyCode::ArrowLeft => modified_csi("D", modifier),
        HerdrTuiKeyCode::Home => modified_csi("H", modifier),
        HerdrTuiKeyCode::End => modified_csi("F", modifier),
        HerdrTuiKeyCode::PageUp => modified_tilde(5, modifier),
        HerdrTuiKeyCode::PageDown => modified_tilde(6, modifier),
        HerdrTuiKeyCode::Insert => modified_tilde(2, modifier),
        HerdrTuiKeyCode::Delete => modified_tilde(3, modifier),
        HerdrTuiKeyCode::F1 => modified_function('P', modifier),
        HerdrTuiKeyCode::F2 => modified_function('Q', modifier),
        HerdrTuiKeyCode::F3 => modified_function('R', modifier),
        HerdrTuiKeyCode::F4 => modified_function('S', modifier),
        HerdrTuiKeyCode::F5 => modified_tilde(15, modifier),
        HerdrTuiKeyCode::F6 => modified_tilde(17, modifier),
        HerdrTuiKeyCode::F7 => modified_tilde(18, modifier),
        HerdrTuiKeyCode::F8 => modified_tilde(19, modifier),
        HerdrTuiKeyCode::F9 => modified_tilde(20, modifier),
        HerdrTuiKeyCode::F10 => modified_tilde(21, modifier),
        HerdrTuiKeyCode::F11 => modified_tilde(23, modifier),
        HerdrTuiKeyCode::F12 => modified_tilde(24, modifier),
        HerdrTuiKeyCode::KeyA
        | HerdrTuiKeyCode::KeyB
        | HerdrTuiKeyCode::KeyC
        | HerdrTuiKeyCode::KeyD
        | HerdrTuiKeyCode::KeyE
        | HerdrTuiKeyCode::KeyF
        | HerdrTuiKeyCode::KeyG
        | HerdrTuiKeyCode::KeyH
        | HerdrTuiKeyCode::KeyI
        | HerdrTuiKeyCode::KeyJ
        | HerdrTuiKeyCode::KeyK
        | HerdrTuiKeyCode::KeyL
        | HerdrTuiKeyCode::KeyM
        | HerdrTuiKeyCode::KeyN
        | HerdrTuiKeyCode::KeyO
        | HerdrTuiKeyCode::KeyP
        | HerdrTuiKeyCode::KeyQ
        | HerdrTuiKeyCode::KeyR
        | HerdrTuiKeyCode::KeyS
        | HerdrTuiKeyCode::KeyT
        | HerdrTuiKeyCode::KeyU
        | HerdrTuiKeyCode::KeyV
        | HerdrTuiKeyCode::KeyW
        | HerdrTuiKeyCode::KeyX
        | HerdrTuiKeyCode::KeyY
        | HerdrTuiKeyCode::KeyZ => letter_bytes(code, modifiers),
    };
    Ok(bytes)
}

/// Letters cover the terminal control shortcuts a native/mobile client cannot
/// synthesize any other way: Ctrl+C interrupt, Ctrl+D EOF, Ctrl+L clear,
/// Ctrl+A/E line motion, Ctrl+R search. Ctrl+letter encodes to the C0 control
/// byte (0x01..=0x1A); Shift produces the uppercase letter; Alt/Meta prefixes
/// ESC, matching `simple_byte`.
fn letter_bytes(code: HerdrTuiKeyCode, modifiers: HerdrTuiModifiers) -> Vec<u8> {
    let index = letter_index(code);
    let byte = if modifiers.ctrl {
        index
    } else if modifiers.shift {
        b'A' + index - 1
    } else {
        b'a' + index - 1
    };
    let mut bytes = Vec::with_capacity(2);
    if modifiers.alt || modifiers.meta {
        bytes.push(0x1b);
    }
    bytes.push(byte);
    bytes
}

fn letter_index(code: HerdrTuiKeyCode) -> u8 {
    match code {
        HerdrTuiKeyCode::KeyA => 1,
        HerdrTuiKeyCode::KeyB => 2,
        HerdrTuiKeyCode::KeyC => 3,
        HerdrTuiKeyCode::KeyD => 4,
        HerdrTuiKeyCode::KeyE => 5,
        HerdrTuiKeyCode::KeyF => 6,
        HerdrTuiKeyCode::KeyG => 7,
        HerdrTuiKeyCode::KeyH => 8,
        HerdrTuiKeyCode::KeyI => 9,
        HerdrTuiKeyCode::KeyJ => 10,
        HerdrTuiKeyCode::KeyK => 11,
        HerdrTuiKeyCode::KeyL => 12,
        HerdrTuiKeyCode::KeyM => 13,
        HerdrTuiKeyCode::KeyN => 14,
        HerdrTuiKeyCode::KeyO => 15,
        HerdrTuiKeyCode::KeyP => 16,
        HerdrTuiKeyCode::KeyQ => 17,
        HerdrTuiKeyCode::KeyR => 18,
        HerdrTuiKeyCode::KeyS => 19,
        HerdrTuiKeyCode::KeyT => 20,
        HerdrTuiKeyCode::KeyU => 21,
        HerdrTuiKeyCode::KeyV => 22,
        HerdrTuiKeyCode::KeyW => 23,
        HerdrTuiKeyCode::KeyX => 24,
        HerdrTuiKeyCode::KeyY => 25,
        HerdrTuiKeyCode::KeyZ => 26,
        _ => unreachable!("letter_index is only called for KeyA..=KeyZ"),
    }
}

fn modifier_parameter(modifiers: HerdrTuiModifiers) -> u8 {
    1 + u8::from(modifiers.shift)
        + 2 * u8::from(modifiers.alt)
        + 4 * u8::from(modifiers.ctrl)
        + 8 * u8::from(modifiers.meta)
}

fn simple_byte(byte: u8, modifiers: HerdrTuiModifiers) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(2);
    if modifiers.alt || modifiers.meta {
        bytes.push(0x1b);
    }
    bytes.push(byte);
    bytes
}

fn modified_csi(final_byte: &str, modifier: u8) -> Vec<u8> {
    if modifier == 1 {
        format!("\x1b[{final_byte}").into_bytes()
    } else {
        format!("\x1b[1;{modifier}{final_byte}").into_bytes()
    }
}

fn modified_tilde(code: u8, modifier: u8) -> Vec<u8> {
    if modifier == 1 {
        format!("\x1b[{code}~").into_bytes()
    } else {
        format!("\x1b[{code};{modifier}~").into_bytes()
    }
}

fn modified_function(final_byte: char, modifier: u8) -> Vec<u8> {
    if modifier == 1 {
        format!("\x1bO{final_byte}").into_bytes()
    } else {
        format!("\x1b[1;{modifier}{final_byte}").into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: HerdrTuiKeyCode, modifiers: HerdrTuiModifiers) -> TuiInputRequest {
        TuiInputRequest::Event(HerdrTuiInput::Key { code, modifiers })
    }

    #[test]
    fn text_and_paste_are_committed_utf8() {
        assert_eq!(
            encode_request(TuiInputRequest::Event(HerdrTuiInput::Text {
                text: "中文".into()
            }))
            .unwrap_or_else(|error| panic!("text encoding failed: {error}"))
            .0,
            "中文".as_bytes()
        );
        assert_eq!(
            encode_request(TuiInputRequest::Event(HerdrTuiInput::Paste {
                text: "paste".into()
            }))
            .unwrap_or_else(|error| panic!("paste encoding failed: {error}"))
            .0,
            b"paste"
        );
    }

    #[test]
    fn common_named_keys_use_standard_sequences() {
        let none = HerdrTuiModifiers::default();
        assert_eq!(
            encode_request(key(HerdrTuiKeyCode::Enter, none))
                .unwrap_or_else(|error| panic!("enter encoding failed: {error}"))
                .0,
            b"\r"
        );
        assert_eq!(
            encode_request(key(HerdrTuiKeyCode::ArrowUp, none))
                .unwrap_or_else(|error| panic!("arrow encoding failed: {error}"))
                .0,
            b"\x1b[A"
        );
        assert_eq!(
            encode_request(key(
                HerdrTuiKeyCode::Tab,
                HerdrTuiModifiers {
                    shift: true,
                    ..none
                }
            ))
            .unwrap_or_else(|error| panic!("shift-tab encoding failed: {error}"))
            .0,
            b"\x1b[Z"
        );
        assert_eq!(
            encode_request(key(HerdrTuiKeyCode::F5, none))
                .unwrap_or_else(|error| panic!("f5 encoding failed: {error}"))
                .0,
            b"\x1b[15~"
        );
    }

    #[test]
    fn modified_navigation_keeps_modifiers_on_the_host_side() {
        let modifiers = HerdrTuiModifiers {
            ctrl: true,
            alt: true,
            ..HerdrTuiModifiers::default()
        };
        assert_eq!(
            encode_request(key(HerdrTuiKeyCode::ArrowLeft, modifiers))
                .unwrap_or_else(|error| panic!("modified arrow encoding failed: {error}"))
                .0,
            b"\x1b[1;7D"
        );
    }

    #[test]
    fn letter_keys_encode_terminal_control_shortcuts() {
        let none = HerdrTuiModifiers::default();
        let ctrl = HerdrTuiModifiers {
            ctrl: true,
            ..HerdrTuiModifiers::default()
        };
        // Ctrl+C interrupt, Ctrl+D EOF, Ctrl+L clear, Ctrl+A line start.
        for (code, byte) in [
            (HerdrTuiKeyCode::KeyC, 0x03u8),
            (HerdrTuiKeyCode::KeyD, 0x04),
            (HerdrTuiKeyCode::KeyL, 0x0c),
            (HerdrTuiKeyCode::KeyA, 0x01),
        ] {
            assert_eq!(
                encode_request(key(code, ctrl))
                    .unwrap_or_else(|error| panic!("ctrl-letter encoding failed: {error}"))
                    .0,
                [byte]
            );
        }
        // Plain and Shift produce literal letters; Alt/Meta prefixes ESC.
        assert_eq!(
            encode_request(key(HerdrTuiKeyCode::KeyC, none))
                .unwrap_or_else(|error| panic!("plain letter encoding failed: {error}"))
                .0,
            b"c"
        );
        assert_eq!(
            encode_request(key(
                HerdrTuiKeyCode::KeyC,
                HerdrTuiModifiers {
                    shift: true,
                    ..none
                }
            ))
            .unwrap_or_else(|error| panic!("shift letter encoding failed: {error}"))
            .0,
            b"C"
        );
        assert_eq!(
            encode_request(key(
                HerdrTuiKeyCode::KeyC,
                HerdrTuiModifiers {
                    alt: true,
                    ctrl: true,
                    ..none
                }
            ))
            .unwrap_or_else(|error| panic!("alt-ctrl letter encoding failed: {error}"))
            .0,
            b"\x1b\x03"
        );
    }

    #[test]
    fn raw_input_remains_bounded_and_compatible() {
        let raw = TuiInputRequest::Raw { data: "abc".into() };
        assert_eq!(
            encode_request(raw)
                .unwrap_or_else(|error| panic!("raw encoding failed: {error}"))
                .0,
            b"abc"
        );
        let too_large = TuiInputRequest::Raw {
            data: "x".repeat(MAX_INPUT_BYTES + 1),
        };
        assert!(encode_request(too_large).is_err());
    }
}
