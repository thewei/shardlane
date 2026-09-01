//! [INPUT]: Depends on the crate::ghostty module-root re-export surface (`use super::*`) and
//! std memory/synchronization primitives.
//! [OUTPUT]: Exposes (within the ghostty module tree) the TerminalKey/TerminalModifiers/
//! TerminalMouse* client projection enums for encode and shell_input to consume.
//! [POS]: The input slice of the ghostty module — key semantics shared by encode encoding and
//! the shell_input/terminal interaction layers.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalMouseAction {
    Press,
    Release,
    Motion,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalMouseButton {
    Left,
    Right,
    Middle,
    WheelUp,
    WheelDown,
}

/// Client projection of Ghostty's `GhosttyMods`: mouse and key encoding share the same bit
/// layout (`ghostty_input_mods_e`: shift=1, ctrl=2, alt=4, super=8).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TerminalModifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub platform: bool,
}

/// Client subset of Ghostty's `input.Key`: GPUI key names map onto the vendored ABI's
/// physical keys. Discriminant values come from upstream ghostty.h `ghostty_input_key_e`
/// (identical in v1.3.1 and main, W3C UI Events key order) and are pinned against the
/// vendored binary by the `key_encoder_*` behavioral regressions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub enum TerminalKey {
    Backquote = 1,
    Backslash = 2,
    BracketLeft = 3,
    BracketRight = 4,
    Comma = 5,
    Digit0 = 6,
    Digit1 = 7,
    Digit2 = 8,
    Digit3 = 9,
    Digit4 = 10,
    Digit5 = 11,
    Digit6 = 12,
    Digit7 = 13,
    Digit8 = 14,
    Digit9 = 15,
    Equal = 16,
    A = 20,
    B = 21,
    C = 22,
    D = 23,
    E = 24,
    F = 25,
    G = 26,
    H = 27,
    I = 28,
    J = 29,
    K = 30,
    L = 31,
    M = 32,
    N = 33,
    O = 34,
    P = 35,
    Q = 36,
    R = 37,
    S = 38,
    T = 39,
    U = 40,
    V = 41,
    W = 42,
    X = 43,
    Y = 44,
    Z = 45,
    Minus = 46,
    Period = 47,
    Quote = 48,
    Semicolon = 49,
    Slash = 50,
    Backspace = 53,
    Enter = 58,
    Space = 63,
    Tab = 64,
    Delete = 68,
    End = 69,
    Home = 71,
    Insert = 72,
    PageDown = 73,
    PageUp = 74,
    ArrowDown = 75,
    ArrowLeft = 76,
    ArrowRight = 77,
    ArrowUp = 78,
    Escape = 120,
    F1 = 121,
    F2 = 122,
    F3 = 123,
    F4 = 124,
    F5 = 125,
    F6 = 126,
    F7 = 127,
    F8 = 128,
    F9 = 129,
    F10 = 130,
    F11 = 131,
    F12 = 132,
    F13 = 133,
    F14 = 134,
    F15 = 135,
    F16 = 136,
    F17 = 137,
    F18 = 138,
    F19 = 139,
    F20 = 140,
    F21 = 141,
    F22 = 142,
    F23 = 143,
    F24 = 144,
}

#[derive(Clone, Copy, Debug)]
pub struct TerminalMouseGeometry {
    pub screen_width: u32,
    pub screen_height: u32,
    pub cell_width: u32,
    pub cell_height: u32,
}
