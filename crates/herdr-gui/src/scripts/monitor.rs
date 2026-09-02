//! [INPUT]: Main-crate imports and sibling-module shared items passed through the scripts module root (super) via the `use super::*` chain
//! [OUTPUT]: Provides the script monitoring loop (the ScriptMonitorTrigger selector and the start_script_monitor main loop)
//! [POS]: The monitoring-loop slice of the scripts module, mechanically split out of scripts.rs
use super::discovery::{
    next_discovery_interval, observe_unmanaged_services, service_discovery_wait,
    SERVICE_DISCOVERY_INTERVAL, SERVICE_DISCOVERY_SWEEP_BUDGET,
};
use super::observe::{apply_script_observation, now_ms, observe_script};
use super::*;

pub(super) const TASK_MONITOR_INTERVAL: Duration = Duration::from_millis(1_500);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScriptMonitorTrigger {
    Wake,
    Timer,
    Closed,
}

async fn wait_for_script_monitor<F>(
    wake: &async_channel::Receiver<()>,
    timer: F,
) -> ScriptMonitorTrigger
where
    F: std::future::Future<Output = ()>,
{
    let wake_wait = wake.recv();
    futures::pin_mut!(wake_wait, timer);
    match futures::future::select(wake_wait, timer).await {
        futures::future::Either::Left((Ok(()), _)) => ScriptMonitorTrigger::Wake,
        futures::future::Either::Left((Err(_), _)) => ScriptMonitorTrigger::Closed,
        futures::future::Either::Right(((), _)) => ScriptMonitorTrigger::Timer,
    }
}

impl ShardlaneApp {
    pub(crate) fn start_script_monitor(
        &mut self,
        wake: async_channel::Receiver<()>,
        cx: &mut Context<Self>,
    ) -> BackgroundJob<()> {
        cx.spawn(async move |this, cx| {
            let mut wait_before_probe: Option<Duration> = None;
            let mut last_service_discovery: Option<Instant> = None;
            let mut discovery_interval = SERVICE_DISCOVERY_INTERVAL;
            loop {
                if let Some(wait) = wait_before_probe.take() {
                    match wait_for_script_monitor(&wake, cx.background_executor().timer(wait)).await {
                        ScriptMonitorTrigger::Wake => {
                            last_service_discovery = None;
                            discovery_interval = SERVICE_DISCOVERY_INTERVAL;
                        }
                        ScriptMonitorTrigger::Timer => {}
                        ScriptMonitorTrigger::Closed => break,
                    }
                }

                let snapshot = match this.update(cx, |view, _| {
                    let tabs = view
                        .state
                        .tabs
                        .iter()
                        .map(|tab| tab.tab_id.clone())
                        .collect::<HashSet<_>>();
                    let scripts = view.scripts.runtime_probe_scripts();
                    // Service discovery covers every runtime workspace of the bound
                    // instance (no client-side grouping filter).
                    let mut discovery_workspace_ids = view
                        .state
                        .workspaces
                        .iter()
                        .map(|workspace| workspace.workspace_id.clone())
                        .collect::<Vec<_>>();
                    discovery_workspace_ids.sort();
                    let discovery_enabled = !discovery_workspace_ids.is_empty();
                    let managed_pane_ids = view
                        .scripts
                        .scripts
                        .iter()
                        .filter_map(|script| script.pane_id.clone())
                        .collect::<HashSet<_>>();
                    (
                        view.client.clone(),
                        scripts,
                        tabs,
                        discovery_enabled,
                        discovery_workspace_ids,
                        managed_pane_ids,
                    )
                }) {
                    Ok(snapshot) => snapshot,
                    Err(_) => break,
                };
                let (
                    client,
                    scripts,
                    known_tabs,
                    discovery_enabled,
                    discovery_workspace_ids,
                    managed_pane_ids,
                ) = snapshot;
                let should_discover_services = discovery_enabled
                    && last_service_discovery
                        .is_none_or(|last| last.elapsed() >= discovery_interval);

                if scripts.is_empty() && !should_discover_services {
                    if discovery_enabled {
                        wait_before_probe = Some(
                            last_service_discovery
                                .map(|last| {
                                    service_discovery_wait(last.elapsed(), discovery_interval)
                                })
                                .unwrap_or(discovery_interval),
                        );
                        continue;
                    }
                    if wake.recv().await.is_err() {
                        break;
                    }
                    continue;
                }
                let Some(client) = client else {
                    wait_before_probe = Some(TASK_MONITOR_INTERVAL);
                    continue;
                };

                let has_runtime_scripts = !scripts.is_empty();
                wait_before_probe = Some(if has_runtime_scripts {
                    TASK_MONITOR_INTERVAL
                } else {
                    discovery_interval
                });
                let probe_count = scripts.len();
                let discovery_project_count = discovery_workspace_ids.len();
                let probe_started = Instant::now();
                let sweep_deadline = probe_started + SERVICE_DISCOVERY_SWEEP_BUDGET;
                let close_client = client.clone();
                let probe_client = client;
                let (observations, observed_services) = cx
                    .background_executor()
                    .spawn(async move {
                        let observations = scripts
                            .iter()
                            .map(|script| observe_script(probe_client.as_ref(), script, &known_tabs))
                            .collect::<Vec<_>>();
                        let observed_services = should_discover_services.then(|| {
                            observe_unmanaged_services(
                                probe_client.as_ref(),
                                &discovery_workspace_ids,
                                &managed_pane_ids,
                                sweep_deadline,
                            )
                        });
                        (observations, observed_services)
                    })
                    .await;
                if should_discover_services {
                    last_service_discovery = Some(Instant::now());
                }
                let probe_elapsed = probe_started.elapsed();
                if probe_elapsed >= Duration::from_millis(100) {
                    lag_log(format_args!(
                        "script.monitor probes={probe_count} service_projects={discovery_project_count} elapsed={:.2}ms",
                        probe_elapsed.as_secs_f64() * 1_000.0
                    ));
                }
                let (close_panes, any_change) = match this.update(cx, |view, cx| {
                    let mut changed = false;
                    let mut persistence_changed = false;
                    let mut close_panes = Vec::new();
                    for observation in observations {
                        let Some(script) = view.scripts.get_mut(&observation.script_id) else {
                            continue;
                        };
                        let applied = apply_script_observation(script, observation, now_ms());
                        changed |= applied.changed;
                        persistence_changed |= applied.persistence_changed;
                        if let Some(pane_id) = applied.close_pane_id {
                            close_panes.push(pane_id);
                        }
                    }
                    if let Some(observed_services) = observed_services {
                        if view.observed_services != observed_services {
                            view.observed_services = observed_services;
                            changed = true;
                        }
                    }
                    if persistence_changed {
                        view.scripts.save();
                    }
                    if changed {
                        view.notify_sidebar(cx);
                        cx.notify();
                    }
                    (close_panes, changed)
                }) {
                    Ok(result) => result,
                    Err(_) => break,
                };
                if should_discover_services {
                    last_service_discovery = Some(Instant::now());
                    // Adaptive interval: back off by doubling when this round had
                    // zero changes; any change (service set / script status /
                    // pending-close panes) resets to the floor immediately. The
                    // Wake path already resets in sync.
                    discovery_interval = next_discovery_interval(any_change, discovery_interval);
                }
                for pane_id in close_panes {
                    let client = close_client.clone();
                    let _ = cx
                        .background_executor()
                        .spawn(async move { client.close_pane(&pane_id) })
                        .await;
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;
    use futures::future::{pending, ready};
    #[test]
    fn script_monitor_wake_interrupts_idle_timer_and_timer_remains_fallback() {
        let (tx, rx) = async_channel::bounded(1);
        tx.try_send(()).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            block_on(wait_for_script_monitor(&rx, pending())),
            ScriptMonitorTrigger::Wake
        );
        assert_eq!(
            block_on(wait_for_script_monitor(&rx, ready(()))),
            ScriptMonitorTrigger::Timer
        );
        drop(tx);
        assert_eq!(
            block_on(wait_for_script_monitor(&rx, pending())),
            ScriptMonitorTrigger::Closed
        );
    }
}
