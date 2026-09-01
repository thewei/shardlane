//! [INPUT]: Depends on the crate::ghostty module-root re-export surface (`use super::*`) and
//! std memory/synchronization primitives.
//! [OUTPUT]: Exposes (within the ghostty module tree) the four encoding entry families —
//! focus/paste/mouse/key — whose output is PTY bytes.
//! [POS]: The encode slice of the ghostty module — GhosttyTerminal's input encoding, consumed
//! by terminal_stream/shell_input via method calls.

use super::*;

impl GhosttyTerminal {
    pub fn encode_focus(&mut self, focused: bool) -> Result<Vec<u8>, String> {
        let mut reporting = false;
        let result = unsafe {
            ghostty_terminal_mode_get(self.terminal, GHOSTTY_MODE_FOCUS_EVENT, &mut reporting)
        };
        if result != GHOSTTY_SUCCESS {
            return Err(format!(
                "ghostty_terminal_mode_get focus reporting failed: {result}"
            ));
        }
        if !reporting {
            return Ok(Vec::new());
        }

        let mut output = [0_u8; 8];
        let mut written = 0_usize;
        let event = if focused {
            GHOSTTY_FOCUS_GAINED
        } else {
            GHOSTTY_FOCUS_LOST
        };
        let result =
            unsafe { ghostty_focus_encode(event, output.as_mut_ptr(), output.len(), &mut written) };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty_focus_encode failed: {result}"));
        }
        Ok(output[..written].to_vec())
    }

    pub fn encode_paste(&mut self, text: &str) -> Result<Vec<u8>, String> {
        let mut bracketed = false;
        let result = unsafe {
            ghostty_terminal_mode_get(self.terminal, GHOSTTY_MODE_BRACKETED_PASTE, &mut bracketed)
        };
        if result != GHOSTTY_SUCCESS {
            return Err(format!(
                "ghostty_terminal_mode_get bracketed paste failed: {result}"
            ));
        }

        let mut input = text.as_bytes().to_vec();
        let mut output = vec![0_u8; input.len().saturating_add(16).max(16)];
        let mut written = 0_usize;
        let result = unsafe {
            ghostty_paste_encode(
                input.as_mut_ptr(),
                input.len(),
                bracketed,
                output.as_mut_ptr(),
                output.len(),
                &mut written,
            )
        };
        if result == GHOSTTY_OUT_OF_SPACE {
            output.resize(written, 0);
            let result = unsafe {
                ghostty_paste_encode(
                    input.as_mut_ptr(),
                    input.len(),
                    bracketed,
                    output.as_mut_ptr(),
                    output.len(),
                    &mut written,
                )
            };
            if result != GHOSTTY_SUCCESS {
                return Err(format!("ghostty_paste_encode retry failed: {result}"));
            }
        } else if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty_paste_encode failed: {result}"));
        }
        output.truncate(written);
        Ok(output)
    }

    pub fn encode_mouse(
        &mut self,
        action: TerminalMouseAction,
        button: Option<TerminalMouseButton>,
        modifiers: TerminalModifiers,
        position: (f32, f32),
        geometry: TerminalMouseGeometry,
        any_button_pressed: bool,
    ) -> Result<Vec<u8>, String> {
        let action = match action {
            TerminalMouseAction::Press => GHOSTTY_MOUSE_ACTION_PRESS,
            TerminalMouseAction::Release => GHOSTTY_MOUSE_ACTION_RELEASE,
            TerminalMouseAction::Motion => GHOSTTY_MOUSE_ACTION_MOTION,
        };
        let button = button.map(|button| match button {
            TerminalMouseButton::Left => GHOSTTY_MOUSE_BUTTON_LEFT,
            TerminalMouseButton::Right => GHOSTTY_MOUSE_BUTTON_RIGHT,
            TerminalMouseButton::Middle => GHOSTTY_MOUSE_BUTTON_MIDDLE,
            TerminalMouseButton::WheelUp => GHOSTTY_MOUSE_BUTTON_FOUR,
            TerminalMouseButton::WheelDown => GHOSTTY_MOUSE_BUTTON_FIVE,
        });
        let mut mods = 0_i32;
        if modifiers.shift {
            mods |= GHOSTTY_MODS_SHIFT;
        }
        if modifiers.control {
            mods |= GHOSTTY_MODS_CTRL;
        }
        if modifiers.alt {
            mods |= GHOSTTY_MODS_ALT;
        }
        if modifiers.platform {
            mods |= GHOSTTY_MODS_SUPER;
        }
        let encoder_size = GhosttyMouseEncoderSize {
            size: std::mem::size_of::<GhosttyMouseEncoderSize>(),
            screen_width: geometry.screen_width.max(1),
            screen_height: geometry.screen_height.max(1),
            cell_width: geometry.cell_width.max(1),
            cell_height: geometry.cell_height.max(1),
            padding_top: 0,
            padding_bottom: 0,
            padding_right: 0,
            padding_left: 0,
        };
        let track_last_cell = true;

        unsafe {
            ghostty_mouse_encoder_setopt_from_terminal(self.mouse_encoder, self.terminal);
            ghostty_mouse_encoder_setopt(
                self.mouse_encoder,
                GHOSTTY_MOUSE_ENCODER_OPT_SIZE,
                (&encoder_size as *const GhosttyMouseEncoderSize).cast(),
            );
            ghostty_mouse_encoder_setopt(
                self.mouse_encoder,
                GHOSTTY_MOUSE_ENCODER_OPT_ANY_BUTTON_PRESSED,
                (&any_button_pressed as *const bool).cast(),
            );
            ghostty_mouse_encoder_setopt(
                self.mouse_encoder,
                GHOSTTY_MOUSE_ENCODER_OPT_TRACK_LAST_CELL,
                (&track_last_cell as *const bool).cast(),
            );
            ghostty_mouse_event_set_action(self.mouse_event, action);
            if let Some(button) = button {
                ghostty_mouse_event_set_button(self.mouse_event, button);
            } else {
                ghostty_mouse_event_clear_button(self.mouse_event);
            }
            ghostty_mouse_event_set_mods(self.mouse_event, mods);
            ghostty_mouse_event_set_position(
                self.mouse_event,
                GhosttyMousePosition {
                    x: position.0.max(0.0),
                    y: position.1.max(0.0),
                },
            );
        }

        let mut buffer = [0_u8; 128];
        let mut out_len = 0_usize;
        let result = unsafe {
            ghostty_mouse_encoder_encode(
                self.mouse_encoder,
                self.mouse_event,
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut out_len,
            )
        };
        if result == GHOSTTY_SUCCESS {
            return Ok(buffer[..out_len].to_vec());
        }
        if result != GHOSTTY_OUT_OF_SPACE {
            return Err(format!("ghostty_mouse_encoder_encode failed: {result}"));
        }

        let mut output = vec![0_u8; out_len];
        let mut required = out_len;
        let result = unsafe {
            ghostty_mouse_encoder_encode(
                self.mouse_encoder,
                self.mouse_event,
                output.as_mut_ptr(),
                output.len(),
                &mut required,
            )
        };
        if result != GHOSTTY_SUCCESS {
            return Err(format!(
                "ghostty_mouse_encoder_encode retry failed: {result}"
            ));
        }
        output.truncate(required);
        Ok(output)
    }

    /// Key event encoding is delegated to a standalone mode-synced encoder. The terminal VT
    /// write keeps the encoder options in sync, so encoding itself does not need to re-read
    /// terminal state.
    #[allow(dead_code)]
    pub fn encode_key(
        &mut self,
        key: TerminalKey,
        mods: TerminalModifiers,
        utf8: Option<&str>,
        unshifted_codepoint: u32,
    ) -> Result<Vec<u8>, String> {
        let mut encoder = self
            .key_encoder
            .lock()
            .map_err(|error| format!("ghostty key encoder lock poisoned: {error}"))?;
        encoder.encode_key(key, mods, utf8, unshifted_codepoint)
    }
}

impl GhosttyKeyEncoderState {
    pub fn encode_key(
        &mut self,
        key: TerminalKey,
        mods: TerminalModifiers,
        utf8: Option<&str>,
        unshifted_codepoint: u32,
    ) -> Result<Vec<u8>, String> {
        let mut mods_bits = 0_u16;
        if mods.shift {
            mods_bits |= GHOSTTY_MODS_SHIFT as u16;
        }
        if mods.control {
            mods_bits |= GHOSTTY_MODS_CTRL as u16;
        }
        if mods.alt {
            mods_bits |= GHOSTTY_MODS_ALT as u16;
        }
        if mods.platform {
            mods_bits |= GHOSTTY_MODS_SUPER as u16;
        }

        unsafe {
            ghostty_key_event_set_action(self.event, GHOSTTY_ACTION_PRESS);
            ghostty_key_event_set_key(self.event, key as i32);
            ghostty_key_event_set_mods(self.event, mods_bits);
            ghostty_key_event_set_consumed_mods(self.event, 0);
            ghostty_key_event_set_composing(self.event, false);
            match utf8 {
                Some(text) => {
                    ghostty_key_event_set_utf8(self.event, text.as_ptr(), text.len());
                }
                None => ghostty_key_event_set_utf8(self.event, ptr::null(), 0),
            }
            ghostty_key_event_set_unshifted_codepoint(self.event, unshifted_codepoint);
        }

        let mut buffer = [0_u8; 128];
        let mut out_len = 0_usize;
        let result = unsafe {
            ghostty_key_encoder_encode(
                self.encoder,
                self.event,
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut out_len,
            )
        };
        if result == GHOSTTY_SUCCESS {
            return Ok(buffer[..out_len].to_vec());
        }
        if result != GHOSTTY_OUT_OF_SPACE {
            return Err(format!("ghostty_key_encoder_encode failed: {result}"));
        }

        let mut output = vec![0_u8; out_len];
        let mut required = out_len;
        let result = unsafe {
            ghostty_key_encoder_encode(
                self.encoder,
                self.event,
                output.as_mut_ptr(),
                output.len(),
                &mut required,
            )
        };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty_key_encoder_encode retry failed: {result}"));
        }
        output.truncate(required);
        Ok(output)
    }
}
