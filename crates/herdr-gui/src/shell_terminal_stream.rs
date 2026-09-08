//! [INPUT]: Depends on the ShardlaneApp type from the crate root (super) and existing types/imports (use super::*); no independent external dependencies.
//! [OUTPUT]: Exposes ShardlaneApp's terminal stream lifecycle: frame ingestion (including the B16 TerminalFramePlan fast path)/geometry sync/deep-history/visible pane terminal management (inherent impl shard).
//! [POS]: The `crates/herdr-gui` shell terminal_stream responsibility domain, mechanically split out of main.rs; together with sibling shell_* modules it forms ShardlaneApp's method surface.
use super::*;

/// B16 (pure, deterministic): frame equality where an extraction plan may replace the
/// full-grid deep comparison. With `RowsUnchanged`, extraction proved the grids
/// byte-identical (the RAW bitfield is the cell's complete storage), so equality reduces to
/// the scalar fields; every other case — including `RowsChanged`, whose verdict this helper
/// deliberately does not short-circuit — keeps the exact deep/path comparison that ran
/// before. No frame can flip from equal to unequal or the reverse.
pub(super) fn terminal_frames_equal_with_plan(
    previous: &TerminalFrame,
    frame: &TerminalFrame,
    plan: Option<&TerminalFramePlan>,
) -> bool {
    match plan {
        Some(TerminalFramePlan::RowsUnchanged) => previous.scalars_match(frame),
        _ => std::ptr::eq(previous, frame) || previous == frame,
    }
}

impl ShardlaneApp {
    pub(super) fn terminal_frame_for_target(&self) -> &TerminalFrame {
        if self.is_lazygit_surface_active() {
            self.lazygit_frame()
        } else {
            self.terminal_frame.as_ref()
        }
    }

    /// F47: automatic reconnect after the event stream drops. Exponential backoff (reconnect_backoff);
    /// on success it rebuilds the event + pane subscriptions and restores the projection. A successful
    /// manual Refresh in the meantime sets Connected, so the retry chain exits itself on its next wake
    /// instead of stacking a second event stream. The handle is stored in a field to prevent double chains.
    pub(super) fn schedule_events_reconnect(&mut self, cx: &mut Context<Self>) {
        let delay = reconnect_backoff(self.events_reconnect_attempts);
        self.events_reconnect_attempts = self.events_reconnect_attempts.saturating_add(1);
        let existing_client = self.client.clone();
        let registry = self.shared.mux_registry.clone();
        // Reconnect re-adopts THIS window's bound session; a Project rebind in the
        // meantime bumps the generation and voids the whole retry.
        let generation = self.binding_generation;
        let bound = self
            .binding
            .as_ref()
            .map(|binding| (binding.backend, binding.session_name().to_string()));
        self._events_reconnect_script = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let result = cx
                .background_executor()
                .spawn(async move {
                    let client = match existing_client {
                        Some(client) if client.ping().is_ok() => client,
                        _ => match bound.as_ref() {
                            Some((backend, session)) => {
                                let reference = if *backend == "tmux" {
                                    shardlane_host::mux::InstanceRef::default_instance("tmux")
                                } else if *backend == "uuyc" {
                                    shardlane_host::mux::InstanceRef::named("uuyc", session)
                                } else {
                                    shardlane_host::mux::InstanceRef::named("herdr", session)
                                };
                                registry.open_instance(&reference)?
                            }
                            None => {
                                return Err(shardlane_host::mux::MuxError::SocketUnavailable(
                                    "unbound window".to_string(),
                                    "no session to reconnect".to_string(),
                                ))
                            }
                        },
                    };
                    let state = client.visible_state()?;
                    let events = client.subscribe_events()?;
                    Ok::<_, shardlane_host::mux::MuxError>((client, state, events))
                })
                .await;
            let mut attach_target = None;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok((client, state, events)) => {
                        view.events_reconnect_attempts = 0;
                        view.client = Some(client.clone());
                        view.state = state;
                        derive_selection_flags(&mut view.state);
                        view.prune_steering_drafts();
                        view.status = ConnectionStatus::Connected;
                        view.notify_sidebar(cx);
                        if view.binding_generation != generation {
                            return;
                        }
                        Self::start_event_subscription(client, events, generation, cx);
                        // After the event stream is rebuilt, the pane-scoped subscription is rebuilt too (same cause as the refresh path).
                        view.sync_pane_event_subscription(cx);
                        // A failed first bind re-enters the runtime through this chain,
                        // so the attach the normal bind path performs must happen here
                        // too — otherwise the window turns Connected with no terminal
                        // surface (same tail as the manual Refresh path).
                        attach_target = view.window_handle;
                        cx.notify();
                    }
                    Err(err) => {
                        // Still on the backoff chain: unless someone (manual Refresh) already restored the connection.
                        if !view.status.is_connected() {
                            view.status = ConnectionStatus::Offline(err.to_string());
                            view.notify_sidebar(cx);
                            cx.notify();
                            view.schedule_events_reconnect(cx);
                        }
                    }
                }
            });
            if let Some(handle) = attach_target {
                let _ = cx.update_window(handle, |_, window, cx| {
                    let _ = this.update(cx, |view, cx| view.attach_focused_terminal(window, cx));
                });
            }
        });
    }

    /// F19: bounded retries after a pane-scoped subscription failure. Same backoff accumulator;
    /// the success path (subscription_started) resets the counter; after the cap it gives up,
    /// leaving the next surface application or manual Refresh as the backstop so unbounded
    /// retries never occupy the handle.
    pub(super) fn schedule_pane_subscription_retry(&mut self, cx: &mut Context<Self>) {
        if self.pane_subscription_retry_attempts >= 5 {
            return;
        }
        let delay = reconnect_backoff(self.pane_subscription_retry_attempts);
        self.pane_subscription_retry_attempts =
            self.pane_subscription_retry_attempts.saturating_add(1);
        self._pane_subscription_retry_script = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update(cx, |view, cx| {
                // Precondition still holds: the pane set was not reset (a reset is handled separately by sync).
                if !view.pane_event_subscription_ids.is_empty() {
                    return;
                }
                view.sync_pane_event_subscription(cx);
            });
        });
    }

    /// The one hosted-TUI teardown reset list (audit B04): every site that tears the hosted
    /// surface down goes through this so the field lists cannot drift apart again (the attach
    /// failure arm used to forget `terminal_surface_size`/`terminal_pending_frame`).
    /// `reset_surface` additionally clears the retained presentation (surface size + frame)
    /// for sites that clear the surface outright; sites that keep the last frame visible
    /// behind a placeholder/error state pass `false`. Site-specific extras (token bumps,
    /// attach-target flags, cooldowns, host status) stay at the call sites.
    pub(super) fn reset_hosted_tui_state(&mut self, reset_surface: bool, cx: &mut Context<Self>) {
        self.terminal = None;
        self.terminal_key_encoder = None;
        self.terminal_input = None;
        self.terminal_target = None;
        self.terminal_size = None;
        self.terminal_pending_frame = false;
        self.tui_adopted_grid = None;
        self.terminal_poll_wake = None;
        if reset_surface {
            self.terminal_surface_size = None;
            self.set_terminal_frame(Arc::new(TerminalFrame::default()), None, cx);
        }
    }

    pub(super) fn clear_terminal_surface(&mut self, cx: &mut Context<Self>) {
        self.terminal_token = self.terminal_token.wrapping_add(1);
        self.terminal_attach_target = None;
        self.state.focused_pane_id = None;
        self.state.panes.clear();
        self.state.layouts.clear();
        self.sync_pane_event_subscription(cx);
        self.reset_terminal_scroll_state();
        self.clear_ime_state();
        self.reset_hosted_tui_state(true, cx);
        self.sync_terminal_application_focus(cx);
    }

    pub(super) fn set_terminal_frame(
        &mut self,
        frame: Arc<TerminalFrame>,
        plan: Option<TerminalFramePlan>,
        cx: &mut Context<Self>,
    ) {
        let trace = crate::terminal_trace::enabled();
        let apply_started = trace.then(Instant::now);
        let projection_started = trace.then(Instant::now);
        let projection = if self.terminal_target.as_deref() == Some(herdr_tui::TUI_TARGET) {
            self.tui_chrome_projection
        } else {
            Default::default()
        };
        // B16: `plan` carries the extraction's exact row-level verdict (RAW bitfield
        // signatures are the cell's complete storage). It is only ever supplied together with
        // a frame extracted via `frame_reusing(Some(self.terminal_raw_frame))`, so the plan's
        // baseline IS `terminal_raw_frame` and the raw-layer deep compare reduces to the
        // scalar fields. Without a plan: byte-for-byte today's deep comparison.
        let raw_unchanged = terminal_frames_equal_with_plan(
            &self.terminal_raw_frame,
            frame.as_ref(),
            plan.as_ref(),
        );
        // The pane may only exploit the plan when the visible frame IS the raw grid this
        // round AND was last round (equal, empty projections): a chrome projection crops and
        // re-indexes rows, so raw row indices would not describe the painted grid. Anything
        // else keeps the plan out of the pane path (correctness over speed).
        let applied_with_plan = plan.is_some();
        let pane_plan = if projection.is_empty() && self.terminal_frame_projection == projection {
            plan
        } else {
            None
        };
        if raw_unchanged && self.terminal_frame_projection == projection {
            if trace {
                crate::terminal_trace::event(format_args!(
                    "stage=frame.apply unchanged=true planned={} projected={} rows={} projection_us=0 compare_us=0 total_us={}",
                    applied_with_plan,
                    !projection.is_empty(),
                    self.terminal_frame.lines.len(),
                    apply_started
                        .map(crate::terminal_trace::elapsed_us)
                        .unwrap_or(0),
                ));
            }
            self.terminal_raw_frame = frame;
            return;
        }
        // Keep the raw Ghostty grid separate from the visible projection. The Hosted Herdr TUI
        // includes navigation chrome beyond the pane, so the projected frame has a different
        // shape and cannot be reused as the signature baseline for the next polling round.
        self.terminal_raw_frame = frame.clone();
        let projected = !projection.is_empty();
        let mut frame = if projected {
            Arc::new(projection.project_frame(&frame))
        } else {
            frame
        };
        let retained_surface_background = if self.terminal_target.as_deref()
            == Some(herdr_tui::TUI_TARGET)
            && frame.surface_background.is_none()
            && !frame.lines.is_empty()
            && self.terminal_frame.surface_background.is_some()
        {
            Arc::make_mut(&mut frame).retain_confirmed_surface_background_from(&self.terminal_frame)
        } else {
            false
        };
        let projection_us = projection_started
            .map(crate::terminal_trace::elapsed_us)
            .unwrap_or(0);
        let previous_surface_background = self
            .terminal_frame
            .surface_background
            .or(self.terminal_frame.default_background);
        let next_surface_background = frame.surface_background.or(frame.default_background);
        let surface_background_changed = previous_surface_background != next_surface_background;
        let compare_started = trace.then(Instant::now);
        // B16: under `pane_plan` the stored visible frame is the plan's baseline projected by
        // the same (empty) projection, so the deep compare's verdict is already decided —
        // RowsChanged means a raw row (hence a visible row, no crop) differs, and RowsUnchanged
        // reaching this point means the scalars differ (the equal-scalar case early-returned
        // above). Without the plan: today's deep comparison, unchanged.
        let unchanged = if pane_plan.is_some() {
            false
        } else {
            Arc::ptr_eq(&self.terminal_frame, &frame)
                || self.terminal_frame.as_ref() == frame.as_ref()
        };
        let compare_us = compare_started
            .map(crate::terminal_trace::elapsed_us)
            .unwrap_or(0);
        if unchanged {
            self.terminal_frame_projection = projection;
            if trace {
                crate::terminal_trace::event(format_args!(
                    "stage=frame.apply unchanged=true projected={projected} rows={} projection_us={projection_us} compare_us={compare_us} total_us={}",
                    frame.lines.len(),
                    apply_started
                        .map(crate::terminal_trace::elapsed_us)
                        .unwrap_or(0),
                ));
            }
            return;
        }
        if trace {
            let total_cells = frame
                .lines
                .iter()
                .map(|line| line.cells.len())
                .sum::<usize>();
            let mut explicit_cells = 0_usize;
            let mut by_background = std::collections::HashMap::<u32, usize>::new();
            for run in frame.lines.iter().flat_map(|line| line.runs.iter()) {
                let Some(background) = run.bg else {
                    continue;
                };
                let cells = usize::from(run.cell_count);
                explicit_cells = explicit_cells.saturating_add(cells);
                let count = by_background.entry(background).or_default();
                *count = count.saturating_add(cells);
            }
            let dominant = by_background.into_iter().max_by_key(|(_, cells)| *cells);
            let (dominant_bg, dominant_cells) = dominant.unwrap_or((0, 0));
            let dominant_permille = dominant_cells
                .saturating_mul(1_000)
                .checked_div(total_cells)
                .unwrap_or(0);
            crate::terminal_trace::event(format_args!(
                "stage=frame.bg_stats rows={} default_fg={:06x} default_bg={:06x} surface_bg={:06x} surface_retained={retained_surface_background} total_cells={total_cells} explicit_cells={explicit_cells} dominant_bg={dominant_bg:06x} dominant_cells={dominant_cells} dominant_permille={dominant_permille}",
                frame.lines.len(),
                frame.default_foreground.unwrap_or(0),
                frame.default_background.unwrap_or(0),
                frame.surface_background.unwrap_or(0),
            ));
        }
        self.terminal_frame = frame.clone();
        self.terminal_frame_projection = projection;
        self.last_terminal_frame_at = Some(Instant::now());
        self.terminal_pending_frame = false;
        // Only notify TerminalPane — never root/sidebar (cached siblings stay cold). The pane
        // receives the extraction plan only when it provably describes the painted grid
        // (see pane_plan above); otherwise Unknown keeps its full deep re-sync.
        let pane_started = trace.then(Instant::now);
        let pane_plan = pane_plan.unwrap_or(TerminalFramePlan::Unknown);
        self.terminal_pane
            .update(cx, |pane, cx| pane.set_frame(frame, pane_plan, cx));
        if surface_background_changed {
            // Content/Header surfaces consume the same Herdr-derived background. Repaint Root only
            // when that effective color actually changes (first confirmation/theme switch), never
            // for ordinary Terminal frames or partial scroll repaints.
            cx.notify();
        }
        if trace {
            crate::terminal_trace::event(format_args!(
                "stage=frame.apply unchanged=false projected={projected} rows={} projection_us={projection_us} compare_us={compare_us} pane_update_us={} total_us={}",
                self.terminal_frame.lines.len(),
                pane_started
                    .map(crate::terminal_trace::elapsed_us)
                    .unwrap_or(0),
                apply_started
                    .map(crate::terminal_trace::elapsed_us)
                    .unwrap_or(0),
            ));
        }
    }

    pub(super) fn terminal_frame_min_interval(&self, output_pending: bool) -> Duration {
        let recent_user_input = self
            .last_terminal_input_at
            .is_some_and(|at| at.elapsed() < Duration::from_millis(300));
        terminal_frame_min_interval_for_activity(recent_user_input, output_pending)
    }

    pub(super) fn maybe_refresh_terminal_frame(
        &mut self,
        cx: &mut Context<Self>,
        force: bool,
    ) -> bool {
        let trace = crate::terminal_trace::enabled();
        let refresh_started = trace.then(Instant::now);
        let Some(terminal) = self.terminal.clone() else {
            return false;
        };
        let frame_min_interval = self.terminal_frame_min_interval(self.terminal_pending_frame);
        let due = force
            || self
                .last_terminal_frame_at
                .is_none_or(|at| at.elapsed() >= frame_min_interval);
        let controller_frame_pending = self.terminal_pending_frame;
        let vt_drain = std::mem::take(&mut self.pending_vt);
        let has_authoritative_work = force || controller_frame_pending || !vt_drain.is_empty();
        if !has_authoritative_work {
            if trace {
                crate::terminal_trace::event(format_args!(
                    "stage=frame.refresh result=no_work elapsed_us={}",
                    refresh_started
                        .map(crate::terminal_trace::elapsed_us)
                        .unwrap_or(0),
                ));
            }
            return false;
        }
        let want_frame = force || due;
        if !want_frame {
            if !vt_drain.is_empty() {
                self.pending_vt.extend_from_slice(&vt_drain);
            }
            self.terminal_pending_frame = true;
            if trace {
                crate::terminal_trace::event(format_args!(
                    "stage=frame.refresh result=defer force={force} due={due} min_interval_us={} elapsed_us={}",
                    frame_min_interval.as_micros(),
                    refresh_started
                        .map(crate::terminal_trace::elapsed_us)
                        .unwrap_or(0),
                ));
            }
            return false;
        }

        let mut session = match terminal.try_lock() {
            Ok(session) => session,
            Err(_) => {
                if !vt_drain.is_empty() {
                    self.pending_vt.extend_from_slice(&vt_drain);
                }
                self.terminal_pending_frame = true;
                if trace {
                    crate::terminal_trace::event(format_args!(
                        "stage=frame.refresh result=lock_busy force={force} elapsed_us={}",
                        refresh_started
                            .map(crate::terminal_trace::elapsed_us)
                            .unwrap_or(0),
                    ));
                }
                return false;
            }
        };
        self.terminal_pending_frame = false;
        if !vt_drain.is_empty() {
            session.write_bytes(&vt_drain);
            let bells = session.take_terminal_bells();
            if bells > 0 {
                if let Some(target) = self.terminal_target.clone() {
                    self.handle_terminal_bells(&target, bells);
                }
            }
        }
        let extract_started = Instant::now();
        let frame = session.frame_reusing(Some(self.terminal_raw_frame.as_ref()));
        let extract_elapsed = extract_started.elapsed();
        if extract_elapsed >= Duration::from_millis(8) {
            lag_log(format_args!(
                "terminal.frame extract_slow {:.1}ms",
                extract_elapsed.as_secs_f64() * 1_000.0,
            ));
        }
        // B16: the extraction's row plan travels with the frame so set_terminal_frame and the
        // pane can skip deep grid comparisons. Taken before the lock is dropped; `Unknown`
        // (the conservative full-comparison answer) unless extraction proved better.
        let frame_plan = session.take_last_frame_plan();
        let io_trace = session.io_trace_snapshot();
        drop(session);

        match frame {
            Ok(frame) => {
                if trace {
                    let now_us = crate::terminal_trace::now_us();
                    crate::terminal_trace::event(format_args!(
                        "stage=frame.extract mode=frame rows={} extract_us={} input_id={} read_id={} since_write_us={} since_read_us={} refresh_elapsed_us={}",
                        frame.lines.len(),
                        extract_elapsed.as_micros(),
                        io_trace.last_input_id,
                        io_trace.last_read_id,
                        now_us.saturating_sub(io_trace.last_write_us),
                        now_us.saturating_sub(io_trace.last_read_us),
                        refresh_started
                            .map(crate::terminal_trace::elapsed_us)
                            .unwrap_or(0),
                    ));
                }
                self.last_terminal_frame_at = Some(Instant::now());
                let frame = Arc::new(frame);
                if trace {
                    crate::terminal_trace::set_last_frame_source(
                        io_trace.last_input_id,
                        io_trace.last_read_id,
                    );
                }
                self.set_terminal_frame(frame, Some(frame_plan), cx);
                if trace {
                    crate::terminal_trace::event(format_args!(
                        "stage=frame.present_request input_id={} read_id={} total_refresh_us={}",
                        io_trace.last_input_id,
                        io_trace.last_read_id,
                        refresh_started
                            .map(crate::terminal_trace::elapsed_us)
                            .unwrap_or(0),
                    ));
                }
                if scroll_debug_enabled() {
                    lag_log(format_args!(
                        "scroll_debug frame t={}",
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis())
                            .unwrap_or(0),
                    ));
                }
                true
            }
            Err(error) => {
                self.terminal_pending_frame = true;
                lag_log(format_args!("terminal.frame failed error={error}"));
                false
            }
        }
    }

    pub(super) fn sync_terminal_geometry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let size = self
            .terminal_surface_size
            .unwrap_or_else(|| self.terminal_size(window));
        self.resize_main_terminal_to_size(size, cx);
    }

    /// Hosted TUI resize: the local Ghostty model reflows immediately and the compensated
    /// raw grid goes to the shared session's resize seam; the Herdr TUI repaints naturally
    /// via its own PTY output.
    pub(super) fn resize_main_terminal_to_size(
        &mut self,
        size: TerminalSize,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_size == Some(size) {
            return;
        }
        self.terminal_size = Some(size);
        let transport_size = self.tui_raw_terminal_size(size);
        let Some(local_terminal) = self.terminal.clone() else {
            cx.notify();
            return;
        };
        let token = self.terminal_token;

        // Local model reflow (async lock); once done, project the reflow frame immediately; within the
        // same lock, write the compensated raw grid to the host PTY (TIOCSWINSZ/SIGWINCH) — the Herdr
        // TUI then repaints naturally.
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    local_terminal
                        .lock()
                        .map_err(|err| err.to_string())
                        .and_then(|mut session| {
                            let frame = session.resize_local(
                                transport_size.0,
                                transport_size.1,
                                transport_size.2,
                                transport_size.3,
                            )?;
                            session
                                .input_handle()
                                .resize(transport_size.0, transport_size.1)?;
                            Ok(frame)
                        })
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                if view.terminal_token != token {
                    return;
                }
                match result {
                    Ok(frame) => {
                        // The model now holds exactly the compensated transport grid.
                        view.tui_adopted_grid = Some((transport_size.0, transport_size.1));
                        // Root re-render so the "Restore Width" fallback tracks the grid.
                        cx.notify();
                        // Resize reflows the whole grid: no row plan exists.
                        view.set_terminal_frame(Arc::new(frame), None, cx);
                    }
                    Err(err) => lag_log(format_args!("terminal resize failed: {err}")),
                }
            });
        })
        .detach();
    }

    pub(super) fn sync_main_terminal_surface_size(
        &mut self,
        width: f64,
        height: f64,
        cx: &mut Context<Self>,
    ) {
        let padding = f64::from(self.terminal_content_padding()) * 2.0;
        let content_width = (width - padding).max(1.0);
        let content_height = (height - padding - TERMINAL_PAINT_FUDGE).max(1.0);
        // Single grid-size helper + pixel convention (audit B19): identical to the
        // window-derived attach fallback, so the same visible surface produces exactly
        // one size (and therefore one SIGWINCH), never two.
        let size = grid_size_for(
            content_width,
            content_height,
            self.terminal_cell_width(),
            self.terminal_cell_height(),
        );
        if self.terminal_surface_size == Some(size) {
            return;
        }
        self.terminal_surface_size = Some(size);
        self.resize_main_terminal_to_size(size, cx);
    }

    pub(super) fn attach_focused_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // TUI-only: the primary terminal completion path — keep the primary host TUI alive. The
        // per-pane controller attach/takeover/backoff/deep-history reseeding paths were all deleted.
        self.ensure_tui_surface(window, cx, false);
    }

    pub(super) fn schedule_recovery_refresh(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { client.visible_state() })
                .await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(state) => {
                        view.state = state;
                        // F34: after the whole-domain replacement, derive the selection flags uniformly.
                        derive_selection_flags(&mut view.state);
                        view.prune_steering_drafts();
                        view.status = ConnectionStatus::Connected;
                        view.sync_pane_event_subscription(cx);
                    }
                    Err(err) => view.status = ConnectionStatus::Offline(err.to_string()),
                }
                view.notify_sidebar(cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn focused_pane(&self) -> Option<&Pane> {
        if let Some(focused_id) = self.state.focused_pane_id.as_deref() {
            if let Some(pane) = self
                .state
                .panes
                .iter()
                .find(|pane| pane.pane_id == focused_id)
            {
                return Some(pane);
            }
        }
        self.state
            .panes
            .iter()
            .find(|pane| pane.focused)
            .or_else(|| self.state.panes.first())
    }

    pub(super) fn terminal_surface_blocked(&self) -> bool {
        // While a secondary surface is open, keyboard input must not leak into the host TUI.
        self.show_settings
            || self.show_help
            || self.history.open
            || self.new_agent_open
            || self.history.search_open
            || self.search_open
            || self.about_open
            || self.rename_open
            || self.script_dialog_open
            || self.navigation_loading
    }
}
