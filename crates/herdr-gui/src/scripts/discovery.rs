//! [INPUT]: Main-crate imports and sibling-module shared items passed through the scripts module root (super) via the `use super::*` chain
//! [OUTPUT]: Provides the service discovery cadence (adaptive backoff / sweep budget constants, unmanaged service observation)
//! [POS]: The discovery-cadence slice of the scripts module, mechanically split out of scripts.rs
#[cfg(test)]
use super::model::script_record;
#[cfg(test)]
use super::monitor::TASK_MONITOR_INTERVAL;
use super::ports::{all_listening_ports, find_listening_for_pane, ProcessTreeSnapshot};
use super::*;

pub(super) const SERVICE_DISCOVERY_INTERVAL: Duration = Duration::from_secs(10);

/// Backoff ceiling while discovery results stay unchanged: an idle session backs
/// off exponentially from 10s/round to 80s/round, reducing probes against herdr
/// (observed pane.process_info p95 > 100ms in bad periods).
pub(super) const SERVICE_DISCOVERY_MAX_INTERVAL: Duration = Duration::from_secs(80);

/// Wall-clock budget for a single discovery sweep: in bad periods 60+ serial
/// RPCs can stretch to 6-11s; 8s allows complete multi-workspace inspection.
pub(super) const SERVICE_DISCOVERY_SWEEP_BUDGET: Duration = Duration::from_millis(8_000);

pub(super) const SERVICE_DISCOVERY_MAX_PROJECTS: usize = 32;

pub(super) const SERVICE_DISCOVERY_MAX_PANES: usize = 64;

pub(super) fn service_discovery_wait(elapsed: Duration, interval: Duration) -> Duration {
    interval.saturating_sub(elapsed)
}

/// Adaptive discovery interval: exponential backoff when nothing changes
/// (doubling up to the ceiling), immediate reset to the floor on any change.
pub(super) fn next_discovery_interval(any_change: bool, current: Duration) -> Duration {
    if any_change {
        SERVICE_DISCOVERY_INTERVAL
    } else {
        current
            .saturating_mul(2)
            .min(SERVICE_DISCOVERY_MAX_INTERVAL)
    }
}

pub(super) fn observe_unmanaged_services(
    client: &dyn shardlane_host::mux::MultiplexerConnection,
    workspace_ids: &[String],
    managed_pane_ids: &HashSet<String>,
    sweep_deadline: Instant,
) -> Vec<ObservedService> {
    let listening_ports = all_listening_ports();
    if listening_ports.is_empty() {
        return Vec::new();
    }
    let process_tree = ProcessTreeSnapshot::capture();

    let mut observed = Vec::new();
    let mut inspected_panes = 0usize;

    for workspace_id in workspace_ids.iter().take(SERVICE_DISCOVERY_MAX_PROJECTS) {
        if Instant::now() >= sweep_deadline {
            break;
        }
        let Ok(panes) = client.workspace_panes(workspace_id) else {
            continue;
        };
        for pane in panes {
            if inspected_panes >= SERVICE_DISCOVERY_MAX_PANES {
                break;
            }
            inspected_panes += 1;
            if managed_pane_ids.contains(&pane.pane_id) || pane.tab_id.is_none() {
                continue;
            }
            if Instant::now() >= sweep_deadline {
                break;
            }
            let Ok(info) = client.pane_process_info(&pane.pane_id) else {
                continue;
            };

            let mut pane_roots = HashSet::new();
            if let Some(shell_pid) = info.shell_pid {
                pane_roots.insert(shell_pid);
            }
            if let Some(pgid) = info.foreground_process_group_id {
                pane_roots.insert(pgid);
            }
            for process in &info.foreground_processes {
                pane_roots.insert(process.pid);
            }
            if pane_roots.is_empty() {
                continue;
            }

            let matches = find_listening_for_pane(&pane_roots, &listening_ports, &process_tree);
            if matches.is_empty() {
                continue;
            }

            let all_ports = matches
                .iter()
                .flat_map(|(_, ports)| ports.iter().copied())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();

            // Determine best descriptive command and PID:
            // 1) foreground process matching a listening pid
            // 2) any foreground process other than shell
            // 3) command from process tree of listening pid
            let (service_pid, command, fallback_name) = if let Some(fp) = matches
                .iter()
                .find_map(|(pid, _)| info.foreground_processes.iter().find(|fp| fp.pid == *pid))
            {
                let cmd = fp
                    .cmdline
                    .clone()
                    .or_else(|| {
                        fp.argv
                            .as_ref()
                            .filter(|argv| !argv.is_empty())
                            .map(|argv| argv.join(" "))
                    })
                    .or_else(|| fp.argv0.clone())
                    .unwrap_or_else(|| fp.name.clone());
                (fp.pid, cmd, fp.name.clone())
            } else if let Some(fp) = info
                .foreground_processes
                .iter()
                .find(|fp| Some(fp.pid) != info.shell_pid)
            {
                let cmd = fp
                    .cmdline
                    .clone()
                    .or_else(|| {
                        fp.argv
                            .as_ref()
                            .filter(|argv| !argv.is_empty())
                            .map(|argv| argv.join(" "))
                    })
                    .or_else(|| fp.argv0.clone())
                    .unwrap_or_else(|| fp.name.clone());
                (fp.pid, cmd, fp.name.clone())
            } else {
                let first_match_pid = matches[0].0;
                let cmd = process_tree
                    .commands
                    .get(&first_match_pid)
                    .cloned()
                    .unwrap_or_else(|| format!("Process {first_match_pid}"));
                let name = cmd
                    .split_whitespace()
                    .next()
                    .map(|p| {
                        std::path::Path::new(p)
                            .file_name()
                            .and_then(|f| f.to_str())
                            .unwrap_or(p)
                            .to_string()
                    })
                    .unwrap_or_else(|| format!("{first_match_pid}"));
                (first_match_pid, cmd, name)
            };

            let pane_name = pane
                .label
                .as_deref()
                .or(pane.title.as_deref())
                .or(pane.terminal_title.as_deref())
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(&fallback_name)
                .to_string();

            let Some(tab_id) = pane.tab_id else {
                continue;
            };

            observed.push(ObservedService {
                workspace_id: workspace_id.clone(),
                tab_id,
                pane_id: pane.pane_id,
                pane_name,
                command,
                pid: service_pid,
                ports: all_ports,
            });
        }
        if inspected_panes >= SERVICE_DISCOVERY_MAX_PANES {
            break;
        }
    }

    observed.sort_by(|left, right| {
        left.workspace_id
            .cmp(&right.workspace_id)
            .then_with(|| left.tab_id.cmp(&right.tab_id))
            .then_with(|| left.pane_id.cmp(&right.pane_id))
    });
    observed
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn service_discovery_idle_wait_uses_service_cadence_not_script_health_cadence() {
        let wait = service_discovery_wait(Duration::from_secs(2), SERVICE_DISCOVERY_INTERVAL);
        assert_eq!(wait, Duration::from_secs(8));
        assert!(wait > TASK_MONITOR_INTERVAL);
        assert_eq!(
            service_discovery_wait(SERVICE_DISCOVERY_INTERVAL, SERVICE_DISCOVERY_INTERVAL),
            Duration::ZERO
        );
    }

    #[test]
    fn discovery_interval_backs_off_when_quiet_and_resets_on_any_change() {
        // No change: 10s → 20s → 40s → 80s (capped).
        let mut interval = SERVICE_DISCOVERY_INTERVAL;
        for expected in [20, 40, 80, 80] {
            interval = next_discovery_interval(false, interval);
            assert_eq!(interval, Duration::from_secs(expected));
        }
        // Any change (service set / script status / pending-close panes) resets
        // to the floor immediately.
        assert_eq!(
            next_discovery_interval(true, SERVICE_DISCOVERY_MAX_INTERVAL),
            SERVICE_DISCOVERY_INTERVAL
        );
    }

    #[test]
    fn observed_service_label_surfaces_ports_without_persisted_script_identity() {
        let service = ObservedService {
            workspace_id: "w1".into(),
            tab_id: "t1".into(),
            pane_id: "p1".into(),
            pane_name: "vite".into(),
            command: "pnpm dev".into(),
            pid: 42,
            ports: vec![3000, 5173],
        };
        assert_eq!(service.ports_label(), ":3000 +1");
    }

    #[test]
    fn script_monitor_probe_set_ignores_unmaterialized_stopped_history() {
        let mut scripts = (0..2_000)
            .map(|index| script_record(&format!("stopped-{index}")))
            .collect::<Vec<_>>();
        let mut running = script_record("running");
        running.runtime.status = ScriptStatus::Running;
        running.tab_id = Some("tab-1".into());
        running.pane_id = Some("pane-1".into());
        scripts.push(running);
        let registry = ScriptRegistry { scripts };
        let probes = registry.runtime_probe_scripts();
        assert_eq!(probes.len(), 1);
        assert_eq!(probes[0].id, "running");
    }
}
