//! [INPUT]: Depends on the vendored libghostty-vt statically linked symbols (resolved by the
//! linker) and std (ptr/Arc/ffi).
//! [OUTPUT]: Exposes (within the ghostty module tree) handle type aliases, ABI constants,
//! #[repr(C)] structs and constructors, function pointer types, the extern "C" symbol block,
//! the GhosttyApi dynamic resolution table (GhosttyApi::load), and the GhosttyRuntime loading
//! entry point.
//! [POS]: The ffi slice of the ghostty module — the module's ABI foundation, consumed by
//! terminal/encode/selection/frame alike; the binary is the authority (AGENTS.md).

use super::*;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GhosttyRuntime;

pub(super) type GhosttyResult = i32;

pub(super) type GhosttyTerminalHandle = *mut c_void;

pub(super) type GhosttyRenderStateHandle = *mut c_void;

pub(super) type GhosttyRowIteratorHandle = *mut c_void;

pub(super) type GhosttyRowCellsHandle = *mut c_void;

pub(super) type GhosttyMouseEncoderHandle = *mut c_void;

pub(super) type GhosttyMouseEventHandle = *mut c_void;

pub(super) type GhosttyKeyEncoderHandle = *mut c_void;

pub(super) type GhosttyKeyEventHandle = *mut c_void;

pub(super) type GhosttyFormatterHandle = *mut c_void;

pub(super) type GhosttyCell = u64;

pub(super) const GHOSTTY_SUCCESS: GhosttyResult = 0;

pub(super) const GHOSTTY_OUT_OF_SPACE: GhosttyResult = -3;

pub(super) const GHOSTTY_INVALID_VALUE: GhosttyResult = -2;

pub(super) const GHOSTTY_NO_VALUE: GhosttyResult = -4;

/// Test-only scrollbar probe data tag: the production scrollbar chain was deleted
/// (audit B07), but the vendored ABI owns the tag value, so pin it for the regression
/// probe instead of dropping it.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) const GHOSTTY_TERMINAL_DATA_SCROLLBAR: u32 = 9;

/// Persistent terminal defaults owned by Ghostty/Herdr. These are read-only queries; Shardlane
/// never writes a host-side Terminal palette.
pub(super) const GHOSTTY_TERMINAL_DATA_COLOR_FOREGROUND_DEFAULT: u32 = 22;
pub(super) const GHOSTTY_TERMINAL_DATA_COLOR_BACKGROUND_DEFAULT: u32 = 23;

pub(super) const RENDER_STATE_DATA_ROW_ITERATOR: u32 = 4;

pub(super) const RENDER_STATE_ROW_DATA_CELLS: u32 = 3;

pub(super) const ROW_CELLS_DATA_RAW: u32 = 1;

pub(super) const ROW_CELLS_DATA_GRAPHEMES_LEN: u32 = 3;

pub(super) const ROW_CELLS_DATA_GRAPHEMES_BUF: u32 = 4;

pub(super) const ROW_CELLS_DATA_BG_COLOR: u32 = 5;

pub(super) const ROW_CELLS_DATA_FG_COLOR: u32 = 6;

pub(super) const RENDER_STATE_DATA_CURSOR_VISUAL_STYLE: u32 = 10;

pub(super) const RENDER_STATE_DATA_CURSOR_VISIBLE: u32 = 11;

pub(super) const RENDER_STATE_DATA_CURSOR_BLINKING: u32 = 12;

pub(super) const RENDER_STATE_DATA_CURSOR_VIEWPORT_HAS_VALUE: u32 = 14;

pub(super) const RENDER_STATE_DATA_CURSOR_VIEWPORT_X: u32 = 15;

pub(super) const RENDER_STATE_DATA_CURSOR_VIEWPORT_Y: u32 = 16;

pub(super) const CELL_DATA_WIDE: u32 = 3;

pub(super) const CELL_WIDE_SPACER_TAIL: u32 = 2;

pub(super) const CELL_WIDE_SPACER_HEAD: u32 = 3;

pub(super) const GHOSTTY_MOUSE_ACTION_PRESS: i32 = 0;

pub(super) const GHOSTTY_MOUSE_ACTION_RELEASE: i32 = 1;

pub(super) const GHOSTTY_MOUSE_ACTION_MOTION: i32 = 2;

pub(super) const GHOSTTY_MOUSE_BUTTON_LEFT: i32 = 1;

pub(super) const GHOSTTY_MOUSE_BUTTON_RIGHT: i32 = 2;

pub(super) const GHOSTTY_MOUSE_BUTTON_MIDDLE: i32 = 3;

pub(super) const GHOSTTY_MOUSE_BUTTON_FOUR: i32 = 4;

pub(super) const GHOSTTY_MOUSE_BUTTON_FIVE: i32 = 5;

pub(super) const GHOSTTY_MOUSE_ENCODER_OPT_SIZE: i32 = 2;

pub(super) const GHOSTTY_MOUSE_ENCODER_OPT_ANY_BUTTON_PRESSED: i32 = 3;

pub(super) const GHOSTTY_MOUSE_ENCODER_OPT_TRACK_LAST_CELL: i32 = 4;

/// `ghostty_input_action_e`: RELEASE=0, PRESS=1, REPEAT=2 (note this differs from the mouse
/// action order).
#[allow(dead_code)]
pub(super) const GHOSTTY_ACTION_PRESS: i32 = 1;

pub(super) const GHOSTTY_MODE_FOCUS_EVENT: u16 = 1004;

pub(super) const GHOSTTY_MODE_BRACKETED_PASTE: u16 = 2004;

/// The three DECSET entries for alternate screen (1049 = alt + cursor save; 1047 = alt;
/// 47 = legacy). Any one set means a TUI alternate screen.
pub(super) const GHOSTTY_MODE_ALT_SCREEN: u16 = 1049;

pub(super) const GHOSTTY_MODE_ALT_SCREEN_NO_CURSOR_CLEAR: u16 = 1047;

pub(super) const GHOSTTY_MODE_ALT_SCREEN_LEGACY: u16 = 47;

pub(super) const GHOSTTY_FOCUS_GAINED: i32 = 0;

pub(super) const GHOSTTY_FOCUS_LOST: i32 = 1;

pub(super) const GHOSTTY_MODS_SHIFT: i32 = 1 << 0;

pub(super) const GHOSTTY_MODS_CTRL: i32 = 1 << 1;

pub(super) const GHOSTTY_MODS_ALT: i32 = 1 << 2;

pub(super) const GHOSTTY_MODS_SUPER: i32 = 1 << 3;

pub(super) const GHOSTTY_POINT_TAG_VIEWPORT: i32 = 1;

pub(super) const GHOSTTY_FORMATTER_FORMAT_PLAIN: i32 = 0;

#[repr(C)]
pub(super) struct GhosttyTerminalOptions {
    pub(super) cols: u16,
    pub(super) rows: u16,
    pub(super) max_scrollback: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct GhosttyMousePosition {
    pub(super) x: f32,
    pub(super) y: f32,
}

#[repr(C)]
pub(super) struct GhosttyMouseEncoderSize {
    pub(super) size: usize,
    pub(super) screen_width: u32,
    pub(super) screen_height: u32,
    pub(super) cell_width: u32,
    pub(super) cell_height: u32,
    pub(super) padding_top: u32,
    pub(super) padding_bottom: u32,
    pub(super) padding_right: u32,
    pub(super) padding_left: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct GhosttyColorRgb {
    pub(super) r: u8,
    pub(super) g: u8,
    pub(super) b: u8,
}

/// Vendored libghostty-vt ABI layout, verified against `ghostty_type_json` and arm64
/// disassembly on 2026-08-29: size=792, background@8, foreground@11, cursor@14,
/// cursor_has_value@17, palette@18.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct GhosttyRenderStateColors {
    pub(super) size: usize,
    pub(super) background: GhosttyColorRgb,
    pub(super) foreground: GhosttyColorRgb,
    pub(super) cursor: GhosttyColorRgb,
    pub(super) cursor_has_value: bool,
    pub(super) palette: [GhosttyColorRgb; 256],
}

impl Default for GhosttyRenderStateColors {
    fn default() -> Self {
        Self {
            size: std::mem::size_of::<Self>(),
            background: GhosttyColorRgb::default(),
            foreground: GhosttyColorRgb::default(),
            cursor: GhosttyColorRgb::default(),
            cursor_has_value: false,
            palette: [GhosttyColorRgb::default(); 256],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct GhosttyPointCoordinate {
    pub(super) x: u16,
    pub(super) y: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct GhosttySurfacePosition {
    pub(super) x: f64,
    pub(super) y: f64,
}

#[repr(C)]
pub(super) union GhosttyPointValue {
    pub(super) coordinate: GhosttyPointCoordinate,
    pub(super) surface: GhosttySurfacePosition,
    pub(super) padding: [u64; 2],
}

#[repr(C)]
pub(super) struct GhosttyPoint {
    pub(super) tag: i32,
    pub(super) value: GhosttyPointValue,
}

impl GhosttyPoint {
    pub(super) fn viewport(x: u16, y: u16) -> Self {
        Self {
            tag: GHOSTTY_POINT_TAG_VIEWPORT,
            value: GhosttyPointValue {
                coordinate: GhosttyPointCoordinate { x, y: u32::from(y) },
            },
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct GhosttyGridRef {
    pub(super) size: usize,
    pub(super) node: *mut c_void,
    pub(super) x: u16,
    pub(super) y: u16,
}

impl Default for GhosttyGridRef {
    fn default() -> Self {
        Self {
            size: std::mem::size_of::<Self>(),
            node: ptr::null_mut(),
            x: 0,
            y: 0,
        }
    }
}

#[repr(C)]
pub(super) struct GhosttySelection {
    pub(super) size: usize,
    pub(super) start: GhosttyGridRef,
    pub(super) end: GhosttyGridRef,
    pub(super) rectangle: bool,
}

#[repr(C)]
pub(super) struct GhosttyTerminalSelectWordOptions {
    pub(super) size: usize,
    pub(super) grid_ref: GhosttyGridRef,
    pub(super) boundary_codepoints: *const u32,
    pub(super) boundary_codepoints_len: usize,
}

impl GhosttyTerminalSelectWordOptions {
    pub(super) fn defaults(grid_ref: GhosttyGridRef) -> Self {
        Self {
            size: std::mem::size_of::<Self>(),
            grid_ref,
            boundary_codepoints: ptr::null(),
            boundary_codepoints_len: 0,
        }
    }
}

#[repr(C)]
pub(super) struct GhosttyTerminalSelectWordBetweenOptions {
    pub(super) size: usize,
    pub(super) start: GhosttyGridRef,
    pub(super) end: GhosttyGridRef,
    pub(super) boundary_codepoints: *const u32,
    pub(super) boundary_codepoints_len: usize,
}

impl GhosttyTerminalSelectWordBetweenOptions {
    pub(super) fn defaults(start: GhosttyGridRef, end: GhosttyGridRef) -> Self {
        Self {
            size: std::mem::size_of::<Self>(),
            start,
            end,
            boundary_codepoints: ptr::null(),
            boundary_codepoints_len: 0,
        }
    }
}

#[repr(C)]
pub(super) struct GhosttyTerminalSelectLineOptions {
    pub(super) size: usize,
    pub(super) grid_ref: GhosttyGridRef,
    pub(super) whitespace: *const u32,
    pub(super) whitespace_len: usize,
    pub(super) semantic_prompt_boundary: bool,
}

impl GhosttyTerminalSelectLineOptions {
    pub(super) fn defaults(grid_ref: GhosttyGridRef) -> Self {
        Self {
            size: std::mem::size_of::<Self>(),
            grid_ref,
            whitespace: ptr::null(),
            whitespace_len: 0,
            semantic_prompt_boundary: false,
        }
    }
}

#[repr(C)]
pub(super) struct GhosttyFormatterScreenExtra {
    pub(super) size: usize,
    pub(super) cursor: bool,
    pub(super) style: bool,
    pub(super) hyperlink: bool,
    pub(super) protection: bool,
    pub(super) kitty_keyboard: bool,
    pub(super) charsets: bool,
}

impl Default for GhosttyFormatterScreenExtra {
    fn default() -> Self {
        Self {
            size: std::mem::size_of::<Self>(),
            cursor: false,
            style: false,
            hyperlink: false,
            protection: false,
            kitty_keyboard: false,
            charsets: false,
        }
    }
}

#[repr(C)]
pub(super) struct GhosttyFormatterTerminalExtra {
    pub(super) size: usize,
    pub(super) palette: bool,
    pub(super) modes: bool,
    pub(super) scrolling_region: bool,
    pub(super) tabstops: bool,
    pub(super) pwd: bool,
    pub(super) keyboard: bool,
    pub(super) screen: GhosttyFormatterScreenExtra,
}

impl Default for GhosttyFormatterTerminalExtra {
    fn default() -> Self {
        Self {
            size: std::mem::size_of::<Self>(),
            palette: false,
            modes: false,
            scrolling_region: false,
            tabstops: false,
            pwd: false,
            keyboard: false,
            screen: GhosttyFormatterScreenExtra::default(),
        }
    }
}

#[repr(C)]
pub(super) struct GhosttyFormatterTerminalOptions {
    pub(super) size: usize,
    pub(super) emit: i32,
    pub(super) unwrap: bool,
    pub(super) trim: bool,
    pub(super) extra: GhosttyFormatterTerminalExtra,
    pub(super) selection: *const GhosttySelection,
}

impl GhosttyFormatterTerminalOptions {
    pub(super) fn plain_selection(selection: &GhosttySelection) -> Self {
        Self {
            size: std::mem::size_of::<Self>(),
            emit: GHOSTTY_FORMATTER_FORMAT_PLAIN,
            unwrap: true,
            trim: true,
            extra: GhosttyFormatterTerminalExtra::default(),
            selection,
        }
    }
}

type GhosttyTerminalNew =
    unsafe extern "C" fn(*const c_void, *mut GhosttyTerminalHandle, GhosttyTerminalOptions) -> i32;

type GhosttyTerminalFree = unsafe extern "C" fn(GhosttyTerminalHandle);
type GhosttyTerminalResize = unsafe extern "C" fn(GhosttyTerminalHandle, u16, u16, u32, u32) -> i32;

type GhosttyTerminalVtWrite = unsafe extern "C" fn(GhosttyTerminalHandle, *const u8, usize);

type GhosttyTerminalScrollViewport =
    unsafe extern "C" fn(GhosttyTerminalHandle, GhosttyScrollViewport);

type GhosttyRenderStateNew =
    unsafe extern "C" fn(*const c_void, *mut GhosttyRenderStateHandle) -> i32;

type GhosttyRenderStateFree = unsafe extern "C" fn(GhosttyRenderStateHandle);

type GhosttyRenderStateUpdate =
    unsafe extern "C" fn(GhosttyRenderStateHandle, GhosttyTerminalHandle) -> i32;

type GhosttyRenderStateGet =
    unsafe extern "C" fn(GhosttyRenderStateHandle, u32, *mut c_void) -> i32;

type GhosttyRenderStateColorsGet =
    unsafe extern "C" fn(GhosttyRenderStateHandle, *mut GhosttyRenderStateColors) -> i32;

type GhosttyRowIteratorNew =
    unsafe extern "C" fn(*const c_void, *mut GhosttyRowIteratorHandle) -> i32;

type GhosttyRowIteratorFree = unsafe extern "C" fn(GhosttyRowIteratorHandle);

type GhosttyRowIteratorNext = unsafe extern "C" fn(GhosttyRowIteratorHandle) -> bool;

type GhosttyRowGet = unsafe extern "C" fn(GhosttyRowIteratorHandle, u32, *mut c_void) -> i32;

type GhosttyRowCellsNew = unsafe extern "C" fn(*const c_void, *mut GhosttyRowCellsHandle) -> i32;

type GhosttyRowCellsFree = unsafe extern "C" fn(GhosttyRowCellsHandle);

type GhosttyRowCellsNext = unsafe extern "C" fn(GhosttyRowCellsHandle) -> bool;

type GhosttyRowCellsGet = unsafe extern "C" fn(GhosttyRowCellsHandle, u32, *mut c_void) -> i32;

type GhosttyCellGet = unsafe extern "C" fn(GhosttyCell, u32, *mut c_void) -> i32;

extern "C" {
    pub(super) fn ghostty_terminal_new(
        config: *const c_void,
        terminal: *mut GhosttyTerminalHandle,
        options: GhosttyTerminalOptions,
    ) -> i32;
    pub(super) fn ghostty_terminal_free(terminal: GhosttyTerminalHandle);
    pub(super) fn ghostty_terminal_resize(
        terminal: GhosttyTerminalHandle,
        cols: u16,
        rows: u16,
        cell_width: u32,
        cell_height: u32,
    ) -> i32;
    pub(super) fn ghostty_terminal_vt_write(
        terminal: GhosttyTerminalHandle,
        data: *const u8,
        len: usize,
    );
    pub(super) fn ghostty_terminal_scroll_viewport(
        terminal: GhosttyTerminalHandle,
        behavior: GhosttyScrollViewport,
    );
    pub(super) fn ghostty_render_state_new(
        config: *const c_void,
        render_state: *mut GhosttyRenderStateHandle,
    ) -> i32;
    pub(super) fn ghostty_render_state_free(render_state: GhosttyRenderStateHandle);
    pub(super) fn ghostty_render_state_update(
        render_state: GhosttyRenderStateHandle,
        terminal: GhosttyTerminalHandle,
    ) -> i32;
    pub(super) fn ghostty_render_state_get(
        render_state: GhosttyRenderStateHandle,
        data: u32,
        out: *mut c_void,
    ) -> i32;
    pub(super) fn ghostty_render_state_colors_get(
        render_state: GhosttyRenderStateHandle,
        colors: *mut GhosttyRenderStateColors,
    ) -> i32;
    pub(super) fn ghostty_render_state_row_iterator_new(
        config: *const c_void,
        row_iterator: *mut GhosttyRowIteratorHandle,
    ) -> i32;
    pub(super) fn ghostty_render_state_row_iterator_free(row_iterator: GhosttyRowIteratorHandle);
    pub(super) fn ghostty_render_state_row_iterator_next(
        row_iterator: GhosttyRowIteratorHandle,
    ) -> bool;
    pub(super) fn ghostty_render_state_row_get(
        row_iterator: GhosttyRowIteratorHandle,
        data: u32,
        out: *mut c_void,
    ) -> i32;
    pub(super) fn ghostty_render_state_row_cells_new(
        config: *const c_void,
        row_cells: *mut GhosttyRowCellsHandle,
    ) -> i32;
    pub(super) fn ghostty_render_state_row_cells_free(row_cells: GhosttyRowCellsHandle);
    pub(super) fn ghostty_render_state_row_cells_next(row_cells: GhosttyRowCellsHandle) -> bool;
    pub(super) fn ghostty_render_state_row_cells_get(
        row_cells: GhosttyRowCellsHandle,
        data: u32,
        out: *mut c_void,
    ) -> i32;
    pub(super) fn ghostty_cell_get(cell: GhosttyCell, data: u32, out: *mut c_void) -> i32;
    pub(super) fn ghostty_terminal_get(
        terminal: GhosttyTerminalHandle,
        data: u32,
        out: *mut c_void,
    ) -> i32;
    pub(super) fn ghostty_mouse_encoder_new(
        allocator: *const c_void,
        encoder: *mut GhosttyMouseEncoderHandle,
    ) -> i32;
    pub(super) fn ghostty_mouse_encoder_free(encoder: GhosttyMouseEncoderHandle);
    pub(super) fn ghostty_mouse_encoder_setopt(
        encoder: GhosttyMouseEncoderHandle,
        option: i32,
        value: *const c_void,
    );
    pub(super) fn ghostty_mouse_encoder_setopt_from_terminal(
        encoder: GhosttyMouseEncoderHandle,
        terminal: GhosttyTerminalHandle,
    );
    pub(super) fn ghostty_mouse_encoder_encode(
        encoder: GhosttyMouseEncoderHandle,
        event: GhosttyMouseEventHandle,
        out_buf: *mut u8,
        out_buf_size: usize,
        out_len: *mut usize,
    ) -> i32;
    pub(super) fn ghostty_mouse_event_new(
        allocator: *const c_void,
        event: *mut GhosttyMouseEventHandle,
    ) -> i32;
    pub(super) fn ghostty_mouse_event_free(event: GhosttyMouseEventHandle);
    pub(super) fn ghostty_mouse_event_set_action(event: GhosttyMouseEventHandle, action: i32);
    pub(super) fn ghostty_mouse_event_set_button(event: GhosttyMouseEventHandle, button: i32);
    pub(super) fn ghostty_mouse_event_clear_button(event: GhosttyMouseEventHandle);
    pub(super) fn ghostty_mouse_event_set_mods(event: GhosttyMouseEventHandle, mods: i32);
    pub(super) fn ghostty_mouse_event_set_position(
        event: GhosttyMouseEventHandle,
        position: GhosttyMousePosition,
    );
    pub(super) fn ghostty_key_encoder_new(
        allocator: *const c_void,
        encoder: *mut GhosttyKeyEncoderHandle,
    ) -> i32;
    pub(super) fn ghostty_key_encoder_free(encoder: GhosttyKeyEncoderHandle);
    #[allow(dead_code)]
    pub(super) fn ghostty_key_encoder_setopt(
        encoder: GhosttyKeyEncoderHandle,
        option: i32,
        value: *const c_void,
    );
    #[allow(dead_code)]
    pub(super) fn ghostty_key_encoder_setopt_from_terminal(
        encoder: GhosttyKeyEncoderHandle,
        terminal: GhosttyTerminalHandle,
    );
    #[allow(dead_code)]
    pub(super) fn ghostty_key_encoder_encode(
        encoder: GhosttyKeyEncoderHandle,
        event: GhosttyKeyEventHandle,
        out_buf: *mut u8,
        out_buf_size: usize,
        out_len: *mut usize,
    ) -> i32;
    #[allow(dead_code)]
    pub(super) fn ghostty_key_event_new(
        allocator: *const c_void,
        event: *mut GhosttyKeyEventHandle,
    ) -> i32;
    #[allow(dead_code)]
    pub(super) fn ghostty_key_event_free(event: GhosttyKeyEventHandle);
    #[allow(dead_code)]
    pub(super) fn ghostty_key_event_set_action(event: GhosttyKeyEventHandle, action: i32);
    #[allow(dead_code)]
    pub(super) fn ghostty_key_event_set_key(event: GhosttyKeyEventHandle, key: i32);
    #[allow(dead_code)]
    pub(super) fn ghostty_key_event_set_mods(event: GhosttyKeyEventHandle, mods: u16);
    #[allow(dead_code)]
    pub(super) fn ghostty_key_event_set_consumed_mods(event: GhosttyKeyEventHandle, mods: u16);
    #[allow(dead_code)]
    pub(super) fn ghostty_key_event_set_composing(event: GhosttyKeyEventHandle, composing: bool);
    #[allow(dead_code)]
    pub(super) fn ghostty_key_event_set_utf8(
        event: GhosttyKeyEventHandle,
        text: *const u8,
        len: usize,
    );
    #[allow(dead_code)]
    pub(super) fn ghostty_key_event_set_unshifted_codepoint(
        event: GhosttyKeyEventHandle,
        codepoint: u32,
    );
    pub(super) fn ghostty_terminal_mode_get(
        terminal: GhosttyTerminalHandle,
        mode: u16,
        out_value: *mut bool,
    ) -> i32;
    pub(super) fn ghostty_paste_encode(
        data: *mut u8,
        data_len: usize,
        bracketed: bool,
        buf: *mut u8,
        buf_len: usize,
        out_written: *mut usize,
    ) -> i32;
    pub(super) fn ghostty_focus_encode(
        event: i32,
        buf: *mut u8,
        buf_len: usize,
        out_written: *mut usize,
    ) -> i32;
    pub(super) fn ghostty_terminal_grid_ref(
        terminal: GhosttyTerminalHandle,
        point: GhosttyPoint,
        out_ref: *mut GhosttyGridRef,
    ) -> i32;
    pub(super) fn ghostty_terminal_point_from_grid_ref(
        terminal: GhosttyTerminalHandle,
        grid_ref: *const GhosttyGridRef,
        tag: i32,
        out: *mut GhosttyPointCoordinate,
    ) -> i32;
    pub(super) fn ghostty_terminal_select_word(
        terminal: GhosttyTerminalHandle,
        options: *const GhosttyTerminalSelectWordOptions,
        out_selection: *mut GhosttySelection,
    ) -> i32;
    pub(super) fn ghostty_terminal_select_word_between(
        terminal: GhosttyTerminalHandle,
        options: *const GhosttyTerminalSelectWordBetweenOptions,
        out_selection: *mut GhosttySelection,
    ) -> i32;
    pub(super) fn ghostty_terminal_select_line(
        terminal: GhosttyTerminalHandle,
        options: *const GhosttyTerminalSelectLineOptions,
        out_selection: *mut GhosttySelection,
    ) -> i32;
    pub(super) fn ghostty_grid_ref_hyperlink_uri(
        grid_ref: *const GhosttyGridRef,
        buf: *mut u8,
        buf_len: usize,
        out_len: *mut usize,
    ) -> i32;
    pub(super) fn ghostty_formatter_terminal_new(
        allocator: *const c_void,
        formatter: *mut GhosttyFormatterHandle,
        terminal: GhosttyTerminalHandle,
        options: GhosttyFormatterTerminalOptions,
    ) -> i32;
    pub(super) fn ghostty_formatter_format_buf(
        formatter: GhosttyFormatterHandle,
        buf: *mut u8,
        buf_len: usize,
        out_written: *mut usize,
    ) -> i32;
    pub(super) fn ghostty_formatter_free(formatter: GhosttyFormatterHandle);
    #[cfg(test)]
    pub(super) fn ghostty_type_json() -> *const std::ffi::c_char;
}

pub(crate) struct GhosttyApi {
    pub(super) terminal_new: GhosttyTerminalNew,
    pub(super) terminal_free: GhosttyTerminalFree,
    pub(super) terminal_resize: GhosttyTerminalResize,
    pub(super) terminal_vt_write: GhosttyTerminalVtWrite,
    /// Test-only reader since the production viewport-scroll chain was deleted (audit B02);
    /// the vendored ABI slot stays pinned so the API table shape is unchanged.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) terminal_scroll_viewport: GhosttyTerminalScrollViewport,
    pub(super) terminal_get: unsafe extern "C" fn(GhosttyTerminalHandle, u32, *mut c_void) -> i32,
    pub(super) render_state_new: GhosttyRenderStateNew,
    pub(super) render_state_free: GhosttyRenderStateFree,
    pub(super) render_state_update: GhosttyRenderStateUpdate,
    pub(super) render_state_get: GhosttyRenderStateGet,
    pub(super) render_state_colors_get: GhosttyRenderStateColorsGet,
    pub(super) row_iterator_new: GhosttyRowIteratorNew,
    pub(super) row_iterator_free: GhosttyRowIteratorFree,
    pub(super) row_iterator_next: GhosttyRowIteratorNext,
    pub(super) row_get: GhosttyRowGet,
    pub(super) row_cells_new: GhosttyRowCellsNew,
    pub(super) row_cells_free: GhosttyRowCellsFree,
    pub(super) row_cells_next: GhosttyRowCellsNext,
    pub(super) row_cells_get: GhosttyRowCellsGet,
    pub(super) cell_get: GhosttyCellGet,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct GhosttyScrollViewportValue {
    pub(super) delta: isize,
    pub(super) padding: u64,
}

/// Test-only scrollbar probe struct (see `GHOSTTY_TERMINAL_DATA_SCROLLBAR`).
#[cfg_attr(not(test), allow(dead_code))]
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct GhosttyTerminalScrollbar {
    pub(super) total: u64,
    pub(super) offset: u64,
    pub(super) len: u64,
}

/// Test-only scroll-probe structs (see `GhosttyTerminal::scroll`).
#[cfg_attr(not(test), allow(dead_code))]
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct GhosttyScrollViewport {
    pub(super) tag: u32,
    pub(super) value: GhosttyScrollViewportValue,
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) const GHOSTTY_SCROLL_VIEWPORT_DELTA: u32 = 2;

impl GhosttyRuntime {
    pub fn detect() -> Result<Self, String> {
        Ok(Self)
    }

    pub(crate) fn load_api(&self) -> Result<Arc<GhosttyApi>, String> {
        GhosttyApi::load()
    }
}

unsafe impl Send for GhosttyApi {}

unsafe impl Sync for GhosttyApi {}

impl GhosttyApi {
    fn load() -> Result<Arc<Self>, String> {
        Ok(Arc::new(Self {
            terminal_new: ghostty_terminal_new,
            terminal_free: ghostty_terminal_free,
            terminal_resize: ghostty_terminal_resize,
            terminal_vt_write: ghostty_terminal_vt_write,
            terminal_scroll_viewport: ghostty_terminal_scroll_viewport,
            terminal_get: ghostty_terminal_get,
            render_state_new: ghostty_render_state_new,
            render_state_free: ghostty_render_state_free,
            render_state_update: ghostty_render_state_update,
            render_state_get: ghostty_render_state_get,
            render_state_colors_get: ghostty_render_state_colors_get,
            row_iterator_new: ghostty_render_state_row_iterator_new,
            row_iterator_free: ghostty_render_state_row_iterator_free,
            row_iterator_next: ghostty_render_state_row_iterator_next,
            row_get: ghostty_render_state_row_get,
            row_cells_new: ghostty_render_state_row_cells_new,
            row_cells_free: ghostty_render_state_row_cells_free,
            row_cells_next: ghostty_render_state_row_cells_next,
            row_cells_get: ghostty_render_state_row_cells_get,
            cell_get: ghostty_cell_get,
        }))
    }
}
