//! [INPUT]: Main-crate imports and sibling-module shared items passed through the scripts module root (super) via the `use super::*` chain
//! [OUTPUT]: Provides the script observation layer (authoritative pane recheck, the observation tri-state, the apply_script_observation state machine, and port-projection reuse judgment)
//! [POS]: The observation slice of the scripts module, mechanically split out of scripts.rs
#[cfg(test)]
use super::model::script_record;
use super::ports::listening_ports_for_pids;
use super::*;

#[derive(Clone, Debug)]
pub(super) struct ScriptObservation {
    pub(super) script_id: String,
    /// `None` means process inspection failed transiently, so existing runtime correlation is
    /// preserved until Herdr provides authoritative evidence that the Script Pane is gone.
    pane_exists: Option<bool>,
    foreground_pid: Option<u32>,
    ports: Vec<u16>,
    error: Option<String>,
}

/// Authoritative recheck: when the local tab snapshot is missing or
/// process_info fails, use workspace pane.list to distinguish "the pane is
/// really gone" from "transiently undecidable".
/// Some(true) = present; Some(false) = authoritatively confirmed gone by Herdr;
/// None = transient (keep the current state).
fn pane_alive_after_recheck(
    script: &ScriptRecord,
    result: Result<Vec<Pane>, shardlane_host::mux::MuxError>,
) -> Option<bool> {
    let pane_id = script.pane_id.as_deref()?;
    match result {
        Ok(panes) => Some(panes.iter().any(|pane| pane.pane_id == pane_id)),
        // An explicit API negation from Herdr (e.g. unknown workspace/pane) is
        // an authoritative death.
        Err(shardlane_host::mux::MuxError::Api(_)) => Some(false),
        // Service unreachable / protocol / codec problems are transient: the
        // correlation must never be cleared based on them.
        Err(_) => None,
    }
}

fn dead_script_observation(script: &ScriptRecord) -> ScriptObservation {
    ScriptObservation {
        script_id: script.id.clone(),
        pane_exists: Some(false),
        foreground_pid: None,
        ports: Vec::new(),
        error: None,
    }
}

fn transient_script_observation(script: &ScriptRecord, error: Option<String>) -> ScriptObservation {
    ScriptObservation {
        script_id: script.id.clone(),
        pane_exists: None,
        foreground_pid: None,
        ports: Vec::new(),
        error,
    }
}

fn probe_script_pane(
    client: &dyn shardlane_host::mux::MultiplexerConnection,
    script: &ScriptRecord,
    pane_id: &str,
) -> ScriptObservation {
    match client.pane_process_info(pane_id) {
        Ok(info) => {
            let pids = script_foreground_pids(&info);
            let foreground_pid = pids.first().copied();
            let ports = if script_port_projection_is_reusable(script, foreground_pid) {
                script.runtime.ports.clone()
            } else {
                listening_ports_for_pids(&pids)
            };
            ScriptObservation {
                script_id: script.id.clone(),
                pane_exists: Some(true),
                foreground_pid,
                ports,
                error: None,
            }
        }
        Err(error) => {
            // The pane may have been closed on its own while the Tab still lives
            // (split-pane scenario): recheck before declaring death, otherwise
            // stay transient to avoid a fake-alive "forever Running".
            match pane_alive_after_recheck(script, client.workspace_panes(&script.workspace_id)) {
                Some(false) => dead_script_observation(script),
                _ => transient_script_observation(script, Some(error.to_string())),
            }
        }
    }
}

pub(super) fn observe_script(
    client: &dyn shardlane_host::mux::MultiplexerConnection,
    script: &ScriptRecord,
    known_tabs: &HashSet<String>,
) -> ScriptObservation {
    let (Some(tab_id), Some(pane_id)) = (script.tab_id.as_deref(), script.pane_id.as_deref())
    else {
        return dead_script_observation(script);
    };
    // The local tab snapshot is only a hint: during a start race the refresh has
    // not been applied yet, so the snapshot may lack the new Tab; a real death
    // must be determined by the Herdr authoritative recheck, otherwise a just-
    // started script would be misjudged as Stopped.
    if !known_tabs.contains(tab_id) {
        return match pane_alive_after_recheck(script, client.workspace_panes(&script.workspace_id))
        {
            Some(true) => probe_script_pane(client, script, pane_id),
            Some(false) => dead_script_observation(script),
            None => transient_script_observation(script, None),
        };
    }
    probe_script_pane(client, script, pane_id)
}

pub(super) fn script_foreground_pids(info: &PaneProcessInfo) -> Vec<u32> {
    info.foreground_processes
        .iter()
        .filter(|process| Some(process.pid) != info.shell_pid)
        .map(|process| process.pid)
        .collect()
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct ScriptObservationApply {
    pub(super) changed: bool,
    pub(super) persistence_changed: bool,
    pub(super) close_pane_id: Option<String>,
}

pub(super) fn apply_script_observation(
    script: &mut ScriptRecord,
    observation: ScriptObservation,
    now: u64,
) -> ScriptObservationApply {
    let previous_status = script.runtime.status;
    let next_status = match observation.pane_exists {
        Some(true) if observation.foreground_pid.is_some() => ScriptStatus::Running,
        Some(true)
            if script.runtime.status == ScriptStatus::Starting
                && script
                    .runtime
                    .started_at_ms
                    .is_some_and(|started| now.saturating_sub(started) < 3_000) =>
        {
            ScriptStatus::Starting
        }
        Some(true) | Some(false) => ScriptStatus::Stopped,
        None => script.runtime.status,
    };
    let next_pid = if observation.pane_exists.is_none() {
        script.runtime.pid
    } else {
        observation.foreground_pid
    };
    let next_ports = if observation.pane_exists.is_none() {
        script.runtime.ports.clone()
    } else {
        observation.ports
    };
    let next_error = observation.error;
    let next_started_at_ms = if next_status == ScriptStatus::Stopped {
        None
    } else {
        script.runtime.started_at_ms
    };
    let runtime_missing =
        next_status == ScriptStatus::Stopped && observation.pane_exists == Some(false);
    let completed_in_existing_pane = matches!(
        previous_status,
        ScriptStatus::Starting | ScriptStatus::Running
    ) && next_status == ScriptStatus::Stopped
        && observation.pane_exists == Some(true);
    let close_pane_id =
        (script.close_on_complete && script.is_one_shot() && completed_in_existing_pane)
            .then(|| script.pane_id.clone())
            .flatten();
    // When Shardlane intentionally closes a completed one-shot BackgroundJob Pane, release the persisted
    // Herdr correlation in the same observation. Otherwise the containing Tab can remain alive
    // and a later process-info error would preserve a ghost pane_id indefinitely.
    let runtime_released = runtime_missing || close_pane_id.is_some();
    let next_tab_id = if runtime_released {
        None
    } else {
        script.tab_id.clone()
    };
    let next_pane_id = if runtime_released {
        None
    } else {
        script.pane_id.clone()
    };
    let persistence_changed = script.tab_id != next_tab_id || script.pane_id != next_pane_id;
    let changed = script.runtime.status != next_status
        || script.runtime.pid != next_pid
        || script.runtime.ports != next_ports
        || script.runtime.last_error != next_error
        || script.runtime.started_at_ms != next_started_at_ms
        || persistence_changed;
    if !changed {
        return ScriptObservationApply::default();
    }

    script.runtime.status = next_status;
    script.runtime.pid = next_pid;
    script.runtime.ports = next_ports;
    script.runtime.last_error = next_error;
    script.runtime.started_at_ms = next_started_at_ms;
    script.tab_id = next_tab_id;
    script.pane_id = next_pane_id;
    ScriptObservationApply {
        changed: true,
        persistence_changed,
        close_pane_id,
    }
}

fn script_port_projection_is_reusable(script: &ScriptRecord, foreground_pid: Option<u32>) -> bool {
    script.runtime.pid == foreground_pid
        && foreground_pid.is_some()
        && !script.runtime.ports.is_empty()
}

pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pane_recheck_confirms_alive_when_pane_listed() {
        let script = ScriptRecord {
            pane_id: Some("w1:p1".to_string()),
            ..ScriptRecord::default()
        };
        let panes = vec![serde_json::from_value::<Pane>(serde_json::json!({
            "pane_id": "w1:p1"
        }))
        .ok()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        assert_eq!(pane_alive_after_recheck(&script, Ok(panes)), Some(true));
    }

    #[test]
    fn pane_recheck_confirms_dead_when_pane_absent() {
        let script = ScriptRecord {
            pane_id: Some("w1:p1".to_string()),
            ..ScriptRecord::default()
        };
        let panes = vec![serde_json::from_value::<Pane>(serde_json::json!({
            "pane_id": "w1:p2"
        }))
        .ok()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        assert_eq!(pane_alive_after_recheck(&script, Ok(panes)), Some(false));
    }

    #[test]
    fn pane_recheck_treats_api_error_as_authoritative_death() {
        let script = ScriptRecord {
            pane_id: Some("w1:p1".to_string()),
            ..ScriptRecord::default()
        };
        assert_eq!(
            pane_alive_after_recheck(
                &script,
                Err(shardlane_host::mux::MuxError::Api(
                    "unknown workspace".to_string()
                ))
            ),
            Some(false)
        );
    }

    #[test]
    fn pane_recheck_treats_socket_failure_as_transient() {
        let script = ScriptRecord {
            pane_id: Some("w1:p1".to_string()),
            ..ScriptRecord::default()
        };
        assert_eq!(
            pane_alive_after_recheck(
                &script,
                Err(shardlane_host::mux::MuxError::SocketUnavailable(
                    "/tmp/herdr.sock".to_string(),
                    "down".to_string(),
                ))
            ),
            None
        );
    }
    #[test]
    fn one_shot_script_requests_runtime_pane_close_after_foreground_completion() {
        let mut script = ScriptRecord {
            definition: ScriptDefinition {
                kind: ScriptKind::Service,
                one_shot: true,
                close_on_complete: true,
                ..ScriptDefinition::default()
            },
            tab_id: Some("t1".into()),
            pane_id: Some("p1".into()),
            runtime: ScriptRuntimeProjection {
                status: ScriptStatus::Running,
                started_at_ms: Some(1),
                ..ScriptRuntimeProjection::default()
            },
            ..ScriptRecord::default()
        };
        let applied = apply_script_observation(
            &mut script,
            ScriptObservation {
                script_id: "script".into(),
                pane_exists: Some(true),
                foreground_pid: None,
                ports: Vec::new(),
                error: None,
            },
            10_000,
        );
        assert_eq!(applied.close_pane_id.as_deref(), Some("p1"));
        assert!(applied.persistence_changed);
        assert_eq!(script.runtime.status, ScriptStatus::Stopped);
        assert_eq!(script.tab_id, None);
        assert_eq!(script.pane_id, None);
    }

    #[test]
    fn script_observation_reconciles_running_transient_and_missing_states() {
        let mut script = script_record("script-1");
        script.runtime.status = ScriptStatus::Starting;
        script.runtime.started_at_ms = Some(1_000);
        script.tab_id = Some("tab-1".into());
        script.pane_id = Some("pane-1".into());
        let running = apply_script_observation(
            &mut script,
            ScriptObservation {
                script_id: "script-1".into(),
                pane_exists: Some(true),
                foreground_pid: Some(42),
                ports: vec![5173],
                error: None,
            },
            1_500,
        );
        assert!(running.changed);
        assert!(!running.persistence_changed);
        assert_eq!(script.runtime.status, ScriptStatus::Running);
        assert_eq!(script.runtime.pid, Some(42));
        assert_eq!(script.runtime.ports, vec![5173]);

        let transient = apply_script_observation(
            &mut script,
            ScriptObservation {
                script_id: "script-1".into(),
                pane_exists: None,
                foreground_pid: None,
                ports: Vec::new(),
                error: Some("temporary socket error".into()),
            },
            2_000,
        );
        assert!(transient.changed);
        assert!(!transient.persistence_changed);
        assert_eq!(script.runtime.status, ScriptStatus::Running);
        assert_eq!(script.runtime.pid, Some(42));
        assert_eq!(script.runtime.ports, vec![5173]);
        assert_eq!(script.tab_id.as_deref(), Some("tab-1"));

        let missing = apply_script_observation(
            &mut script,
            ScriptObservation {
                script_id: "script-1".into(),
                pane_exists: Some(false),
                foreground_pid: None,
                ports: Vec::new(),
                error: None,
            },
            3_000,
        );
        assert!(missing.changed);
        assert!(missing.persistence_changed);
        assert_eq!(script.runtime.status, ScriptStatus::Stopped);
        assert!(script.tab_id.is_none());
        assert!(script.pane_id.is_none());
        assert!(script.runtime.started_at_ms.is_none());
    }

    #[test]
    fn starting_script_without_foreground_process_eventually_becomes_stopped() {
        let mut script = script_record("script-2");
        script.runtime.status = ScriptStatus::Starting;
        script.runtime.started_at_ms = Some(1_000);
        script.tab_id = Some("tab-2".into());
        script.pane_id = Some("pane-2".into());
        let observation = || ScriptObservation {
            script_id: "script-2".into(),
            pane_exists: Some(true),
            foreground_pid: None,
            ports: Vec::new(),
            error: None,
        };
        let still_starting = apply_script_observation(&mut script, observation(), 2_000);
        assert!(!still_starting.changed);
        assert!(!still_starting.persistence_changed);
        assert_eq!(script.runtime.status, ScriptStatus::Starting);
        let stopped = apply_script_observation(&mut script, observation(), 4_500);
        assert!(stopped.changed);
        assert!(!stopped.persistence_changed);
        assert_eq!(script.runtime.status, ScriptStatus::Stopped);
    }

    #[test]
    fn stable_script_pid_and_ports_reuse_projection_without_expensive_rescan() {
        let script = ScriptRecord {
            runtime: ScriptRuntimeProjection {
                pid: Some(42),
                ports: vec![3000, 5173],
                ..ScriptRuntimeProjection::default()
            },
            ..ScriptRecord::default()
        };
        assert!(script_port_projection_is_reusable(&script, Some(42)));
        assert!(!script_port_projection_is_reusable(&script, Some(43)));
        assert!(!script_port_projection_is_reusable(&script, None));

        let script_without_ports = ScriptRecord {
            runtime: ScriptRuntimeProjection {
                pid: Some(42),
                ..ScriptRuntimeProjection::default()
            },
            ..ScriptRecord::default()
        };
        assert!(!script_port_projection_is_reusable(
            &script_without_ports,
            Some(42)
        ));
    }

    #[test]
    fn unchanged_script_observation_is_a_noop_for_persistence_and_ui() {
        let mut script = script_record("script-1");
        script.runtime = ScriptRuntimeProjection {
            status: ScriptStatus::Running,
            pid: Some(42),
            ports: vec![5173],
            started_at_ms: Some(1_000),
            last_error: None,
        };
        script.tab_id = Some("tab-1".into());
        script.pane_id = Some("pane-1".into());
        let applied = apply_script_observation(
            &mut script,
            ScriptObservation {
                script_id: "script-1".into(),
                pane_exists: Some(true),
                foreground_pid: Some(42),
                ports: vec![5173],
                error: None,
            },
            2_000,
        );
        assert!(!applied.changed);
        assert!(!applied.persistence_changed);
        assert_eq!(script.runtime.started_at_ms, Some(1_000));
    }
}
