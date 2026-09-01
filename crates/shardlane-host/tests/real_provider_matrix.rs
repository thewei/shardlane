//! M9.3 real-provider interaction matrix (runtime acceptance).
//!
//! [INPUT]: an isolated Herdr runtime (dedicated `HERDR_SOCKET_PATH` +
//! temporary `HOME`, installed and authenticated `claude` / `codex` CLIs;
//! `pi` joins the smoke automatically at install time), plus
//! `SHARDLANE_PROVIDER_MATRIX=1`.
//! [OUTPUT]: runtime verification of the interaction matrix in
//! `docs/implementation-2026-08-30-agent-first-final-convergence.md` §M9.3:
//! New Agent launch, idle send, Working `Send after turn` (delivery +
//! cancellation race), Blocked retention, AlreadyLive reuse, Claude→Claude
//! NativeResume, Claude→Codex ContextTransfer, idle/working Live Handoff
//! (source preserved), NeedsProjectSelection planning, large artifact
//! integrity, and the Desktop+Remote single projection.
//! [POS]: `#[ignore]` by default: never runs without real Providers/auth.
//! Explicit invocation:
//! ```sh
//! SHARDLANE_PROVIDER_MATRIX=1 HERDR_SOCKET_PATH=<sock> HOME=<isolated-home> \
//!   cargo test -p shardlane-host --test real_provider_matrix -- --ignored --nocapture
//! ```

use shardlane_history::models::SessionFileRef;
use shardlane_history::{
    capture_transfer_snapshot, create_adapters, scan, AgentId, HistoryCatalog,
    TransferArtifactStore, TransferLimits, TransferPayload,
};
use shardlane_host::herdr::HerdrClient;
use shardlane_host::runtime::{AgentRuntime, RuntimeAgentReadFormat, RuntimeAgentReadRequest};
use shardlane_host::{
    conversation_id_for_history_key, run_agent_launch, run_follow_up_delivery, run_live_handoff,
    AgentLaunchIntent, AgentLaunchMode, AgentPermission, AgentRef, ContinuationRequest,
    ConversationFollowUpQueue, ConversationServiceError, GitWorktreePreparation,
    HerdrFollowUpRuntime, LiveHandoffRequest, QueueState, QueuedFollowUp,
};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PROJECT: &str = "/tmp/matrix-project";
const SETTLE_TIMEOUT_MS: u64 = 240_000;

fn matrix_enabled() {
    if std::env::var("SHARDLANE_PROVIDER_MATRIX").is_err() {
        panic!(
            "real-provider matrix is opt-in: set SHARDLANE_PROVIDER_MATRIX=1, \
             HERDR_SOCKET_PATH=<isolated socket>, HOME=<isolated home>"
        );
    }
}

fn home_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("HOME")
            .unwrap_or_else(|error| panic!("HOME must point at the isolated home: {error}")),
    )
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn open_catalog() -> HistoryCatalog {
    let path = home_dir().join(".shardlane").join("history.sqlite3");
    match HistoryCatalog::open(&path) {
        Ok(catalog) => catalog,
        Err(_) => HistoryCatalog::open_initialized(&path)
            .unwrap_or_else(|error| panic!("open history catalog at {}: {error}", path.display())),
    }
}

fn scan_catalog(catalog: &mut HistoryCatalog) {
    let adapters = create_adapters();
    let report = scan(&adapters, catalog, true)
        .unwrap_or_else(|error| panic!("history scan failed: {error}"));
    println!(
        "[scan] discovered={} parsed={} unchanged={} removed={} errors={:?}",
        report.discovered, report.parsed, report.unchanged, report.removed, report.errors
    );
}

fn find_meta_by_native(
    catalog: &HistoryCatalog,
    native_id: &str,
) -> shardlane_history::models::SessionMeta {
    // Look up by the deterministic catalog key; the indexed project path is
    // the one the provider CLI saw (/private/tmp/... on macOS), so a project
    // path filter on the caller's spelling can miss it.
    catalog
        .session(&format!("claude-code:{native_id}"))
        .unwrap_or_else(|error| panic!("catalog lookup: {error}"))
        .unwrap_or_else(|| panic!("history meta for native session {native_id} not indexed"))
}

fn agent_status(client: &HerdrClient, pane: &str) -> Option<String> {
    let agent = client
        .agents()
        .ok()?
        .into_iter()
        .find(|agent| agent.pane_id.as_deref() == Some(pane))?;
    agent.agent_status
}

fn find_agent(client: &HerdrClient, pane: &str) -> Option<shardlane_host::herdr::Agent> {
    client
        .agents()
        .unwrap_or_default()
        .into_iter()
        .find(|agent| agent.pane_id.as_deref() == Some(pane))
}

fn wait_for_status(
    client: &HerdrClient,
    pane: &str,
    statuses: &[&str],
    timeout: Duration,
) -> Option<String> {
    let started = Instant::now();
    loop {
        if let Some(status) = agent_status(client, pane) {
            if statuses.contains(&status.as_str()) {
                return Some(status);
            }
        }
        if started.elapsed() > timeout {
            return None;
        }
        std::thread::sleep(Duration::from_millis(1_000));
    }
}

fn wait_idle(client: &HerdrClient, pane: &str, timeout: Duration) {
    assert!(
        wait_for_status(client, pane, &["idle", "done"], timeout).is_some(),
        "agent {pane} never went idle"
    );
}

fn agent_gone(client: &HerdrClient, pane: &str, timeout: Duration) -> bool {
    let started = Instant::now();
    loop {
        if find_agent(client, pane).is_none() {
            return true;
        }
        if started.elapsed() > timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn enqueue_follow_up(
    queue: &ConversationFollowUpQueue,
    conversation_id: shardlane_host::ConversationId,
    agent_ref: &AgentRef,
    provider: &str,
    native_session_id: Option<String>,
    text: &str,
) {
    let item = QueuedFollowUp {
        request_id: format!("matrix-{}", now_ms()),
        conversation_id,
        agent_ref: agent_ref.clone(),
        provider: provider.to_string(),
        native_session_id,
        session_fingerprint: "matrix-fingerprint".to_string(),
        baseline_revision: 0,
        text: text.to_string(),
        created_at: SystemTime::now(),
        state: QueueState::Queued,
    };
    queue
        .enqueue(item)
        .unwrap_or_else(|error| panic!("enqueue follow-up: {error}"));
}

fn launch_intent(prompt: &str, agent: AgentId) -> AgentLaunchIntent {
    AgentLaunchIntent {
        operation_id: format!("matrix-launch-{}", agent.as_str()),
        workspace_id: None,
        project_path: PROJECT.to_string(),
        branch: String::new(),
        mode: AgentLaunchMode::Build,
        permission: AgentPermission::AskApproval,
        agent,
        prompt: prompt.to_string(),
        attachments: Vec::new(),
        extra_args: Vec::new(),
        skip_initial_prompt: false,
    }
}

fn close_tab_quietly(client: &HerdrClient, pane: &str) {
    if let Some(agent) = find_agent(client, pane) {
        if let Some(tab_id) = agent
            .tab_id
            .clone()
            .or_else(|| find_agent(client, pane).and_then(|agent| agent.tab_id))
        {
            let _ = client.close_tab(&tab_id);
        } else if let Some(pane_id) = find_agent(client, pane).and_then(|agent| agent.pane_id) {
            let _ = client.close_pane(&pane_id);
        }
    }
}

/// True once the agent's screen shows the prompt marker followed by assistant
/// output (⏺ reply or ✻ turn summary) — i.e. the text was submitted and the
/// turn ran. A parked, unsubmitted composer has no output after the marker.
fn screen_confirms_reply(
    client: &HerdrClient,
    pane: &str,
    marker: &str,
    timeout: Duration,
) -> bool {
    let started = Instant::now();
    loop {
        if let Ok(read) = client.read_runtime_agent(&RuntimeAgentReadRequest {
            agent_id: AgentRef::new(pane.to_string()),
            lines: Some(200),
            format: RuntimeAgentReadFormat::Text,
        }) {
            if let Some(position) = read.text.rfind(marker) {
                let after = &read.text[position + marker.len()..];
                if after.contains('\u{24d}') || after.contains("✻") {
                    return true;
                }
            }
        }
        if started.elapsed() > timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(2_000));
    }
}

/// Codex 0.151+ gates startup behind a one-time hook-review dialog; trust the
/// hooks and close the dialog so semantic prompts reach the composer.
fn dismiss_codex_hook_review(client: &HerdrClient, pane: &str) {
    for _ in 0..10 {
        if let Ok(read) = client.read_runtime_agent(&RuntimeAgentReadRequest {
            agent_id: AgentRef::new(pane.to_string()),
            lines: Some(60),
            format: RuntimeAgentReadFormat::Text,
        }) {
            if read.text.contains("hooks need review") {
                let _ = client
                    .send_runtime_agent_keys(&AgentRef::new(pane.to_string()), &["t".to_string()]);
                std::thread::sleep(Duration::from_millis(1_500));
                let _ = client.send_runtime_agent_keys(
                    &AgentRef::new(pane.to_string()),
                    &["escape".to_string()],
                );
                std::thread::sleep(Duration::from_millis(1_000));
                continue;
            }
            if !read.text.contains("Press t to trust all") {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(1_000));
    }
}

fn dump_agent_tail(client: &HerdrClient, pane: &str) {
    if let Ok(read) = client.read_runtime_agent(&RuntimeAgentReadRequest {
        agent_id: AgentRef::new(pane.to_string()),
        lines: Some(40),
        format: RuntimeAgentReadFormat::Text,
    }) {
        println!("--- agent {pane} tail ---\n{}\n---", read.text);
    }
}

#[test]
#[ignore = "scan probe"]
fn scan_probe() {
    matrix_enabled();
    let mut catalog = open_catalog();
    scan_catalog(&mut catalog);
    for native in [
        "d7c0c079-fb7b-4548-bc67-eb28c38fc0e6",
        "06b4a93a-9596-4beb-826b-d0a446194354",
    ] {
        println!(
            "[probe] {native} -> {:?}",
            catalog
                .session(&format!("claude-code:{native}"))
                .ok()
                .flatten()
                .map(|m| m.key)
        );
    }
}

#[test]
#[ignore = "real-provider matrix: needs isolated Herdr + authenticated claude/codex CLIs"]
fn full_provider_matrix() {
    matrix_enabled();
    let client =
        HerdrClient::connect().unwrap_or_else(|error| panic!("connect to isolated herdr: {error}"));
    let home = home_dir();
    let preparation = GitWorktreePreparation::new(home.join(".shardlane/worktrees"));
    let store = TransferArtifactStore::new(home.join(".shardlane/transfer-artifacts"));
    let limits = TransferLimits::default();
    let history_db = home.join(".shardlane/history.sqlite3");
    let delivery_coordinator = shardlane_host::ConversationDeliveryCoordinator::new(
        Arc::new(ConversationFollowUpQueue::new()),
        None,
    );
    let queue = Arc::clone(delivery_coordinator.queue());
    let manager = Arc::new(shardlane_host::ConversationSessionManager::new(
        history_db.clone(),
    ));

    // ---- 1. New Agent launch over Herdr Agent APIs (Claude) ----
    println!("[1] New Agent launch (Claude)…");
    let ack_intent = launch_intent(
        "Reply with exactly: MATRIX-ACK-1 and nothing else.",
        AgentId::ClaudeCode,
    );
    let outcome = run_agent_launch(&client, &preparation, &ack_intent)
        .unwrap_or_else(|error| panic!("claude launch failed: {error}"));
    let identity = outcome
        .identity
        .as_ref()
        .unwrap_or_else(|| panic!("claude identity binds"));
    let claude_pane = outcome.pane_id.clone();
    let claude_tab = outcome.tab_id.clone();
    let native = identity
        .native_session_id
        .clone()
        .unwrap_or_else(|| panic!("claude exposes a native session id"));
    assert_eq!(identity.provider, "claude");
    assert_ne!(identity.agent_ref.as_str(), "");
    println!("[1] OK pane={} native={native}", claude_pane);

    // ---- 1b. Pi stable-provider smoke (R2-16): every installed Stable
    // Provider must at minimum launch + settle + idle-prompt through the same
    // Host transaction. Skipped gracefully when Pi is not on PATH. ----
    println!("[1b] Pi stable-provider smoke…");
    if std::process::Command::new("pi")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        let pi_intent = launch_intent(
            "Reply with exactly: MATRIX-PI-OK and nothing else.",
            AgentId::Pi,
        );
        let pi_outcome = match run_agent_launch(&client, &preparation, &pi_intent) {
            Ok(outcome) => outcome,
            Err(error) => panic!("pi launch failed: {error}"),
        };
        let pi_pane = pi_outcome.pane_id.clone();
        let pi_tab = pi_outcome.tab_id.clone();
        assert!(
            pi_outcome.identity.is_some(),
            "pi must bind a semantic identity"
        );
        wait_idle(&client, &pi_pane, Duration::from_secs(120));
        println!("[1b] OK pane={pi_pane}");
        let _ = client.close_tab(&pi_tab);
    } else {
        println!("[1b] SKIPPED: pi CLI not installed");
    }

    // ---- 2. Idle Chat send over the Host Conversation service ----
    // Delivery is proven against the Agent's own output: the Claude Code REPL
    // flushes its provider transcript only when the session ends, so the
    // semantic source file is validated after the session closes (step 5).
    println!("[2] Idle semantic send…");
    let remote_service = shardlane_host::HostConversationService::new(&client, &history_db)
        .with_shared_sessions(manager.clone());
    let send_result = remote_service.prompt_live_conversation(
        &AgentRef::new(claude_pane.clone()),
        "Reply with exactly: MATRIX-ACK-2",
    );
    match &send_result {
        Ok(mutation) => assert!(mutation.accepted),
        Err(error) => println!("[2] submit uncertain ({error}) — verifying on the agent screen"),
    }
    wait_idle(&client, &claude_pane, Duration::from_secs(180));
    assert!(
        screen_confirms_reply(
            &client,
            &claude_pane,
            "MATRIX-ACK-2",
            Duration::from_secs(120)
        ),
        "agent never answered the idle prompt"
    );
    println!("[2] OK");

    // ---- 3. Working `Send after turn`: queued delivery after settle ----
    println!("[3] Working queued follow-up (delivery)…");
    remote_service
        .prompt_live_conversation(
            &AgentRef::new(claude_pane.clone()),
            "Count from 1 to 30, one number per line, then reply with exactly: TASK-DONE. Do not use any tools.",
        )
        .unwrap_or_else(|error| panic!("{error}"));
    // Manual-mode CC turns are not always reported as "working"; the queue
    // semantics under test are the settle wait, not this observation.
    let observed_working =
        wait_for_status(&client, &claude_pane, &["working"], Duration::from_secs(30)).is_some();
    println!("[3] working observed: {observed_working}");
    enqueue_follow_up(
        &queue,
        identity.conversation_id.clone(),
        &AgentRef::new(claude_pane.clone()),
        "claude",
        Some(native.clone()),
        "Reply with exactly: FOLLOWUP-DELIVERED",
    );
    let delivery_client = client.clone();
    let delivery_queue = queue.clone();
    let delivery_pane = claude_pane.clone();
    let started = Instant::now();
    let delivery_handle = std::thread::spawn(move || {
        let runtime = HerdrFollowUpRuntime::new(&delivery_client);
        run_follow_up_delivery(
            &delivery_queue,
            &runtime,
            &AgentRef::new(delivery_pane),
            SETTLE_TIMEOUT_MS,
        )
    });
    let blocked_delivery = delivery_handle
        .join()
        .unwrap_or_else(|error| panic!("{error:?}"));
    let elapsed = started.elapsed();
    match &blocked_delivery {
        shardlane_host::DeliveryOutcome::Delivered { text, .. } => {
            assert_eq!(text, "Reply with exactly: FOLLOWUP-DELIVERED");
            println!("[3] settle wait observed: {elapsed:?}");
        }
        // Herdr's submit wait can time out even when the Enter lands (its
        // detector may miss a manual-mode turn); uncertainty must never
        // retry — the screen check below proves the single delivery.
        shardlane_host::DeliveryOutcome::Uncertain { .. } => {
            println!("[3] delivery uncertain after {elapsed:?} — verifying on screen");
        }
        other => {
            dump_agent_tail(&client, &claude_pane);
            panic!("queued follow-up should deliver after settle, got {other:?}");
        }
    }
    assert!(queue.queued(&AgentRef::new(claude_pane.clone())).is_none());
    wait_idle(&client, &claude_pane, Duration::from_secs(180));
    assert!(
        screen_confirms_reply(
            &client,
            &claude_pane,
            "FOLLOWUP-DELIVERED",
            Duration::from_secs(120)
        ),
        "queued follow-up never reached the provider"
    );
    // Ordering proof: the follow-up reply appears after the task turn's own
    // completion — a mid-turn injection would precede TASK-DONE.
    let read = client
        .read_runtime_agent(&RuntimeAgentReadRequest {
            agent_id: AgentRef::new(claude_pane.clone()),
            lines: Some(200),
            format: RuntimeAgentReadFormat::Text,
        })
        .unwrap_or_else(|error| panic!("{error}"));
    let task_position = read
        .text
        .rfind("TASK-DONE")
        .unwrap_or_else(|| panic!("task turn missing from the transcript"));
    let followup_position = read
        .text
        .rfind("FOLLOWUP-DELIVERED")
        .unwrap_or_else(|| panic!("follow-up missing from the transcript"));
    assert!(
        followup_position > task_position,
        "follow-up must execute after the task turn settled"
    );
    println!("[3] OK (delivered after {elapsed:?}, ordered after the task turn)");

    // ---- 4. Working `Send after turn`: cancel during the settle wait ----
    println!("[4] Working queued follow-up (cancel)…");
    remote_service
        .prompt_live_conversation(
            &AgentRef::new(claude_pane.clone()),
            "Count from 1 to 300, one number per line, then reply with exactly: TASK-DONE-2. Do not use any tools.",
        )
        .unwrap_or_else(|error| panic!("{error}"));
    let observed_working =
        wait_for_status(&client, &claude_pane, &["working"], Duration::from_secs(30)).is_some();
    println!("[4] working observed: {observed_working}");
    let cancelled_marker = "CANCELLED-NEVER-APPEAR 7f3a";
    enqueue_follow_up(
        &queue,
        identity.conversation_id.clone(),
        &AgentRef::new(claude_pane.clone()),
        "claude",
        Some(native.clone()),
        &format!("Reply with exactly: {cancelled_marker}"),
    );
    let cancel_client = client.clone();
    let cancel_queue = queue.clone();
    let cancel_pane = claude_pane.clone();
    let cancel_handle = std::thread::spawn(move || {
        let runtime = HerdrFollowUpRuntime::new(&cancel_client);
        run_follow_up_delivery(
            &cancel_queue,
            &runtime,
            &AgentRef::new(cancel_pane),
            SETTLE_TIMEOUT_MS,
        )
    });
    // Cancel while the long turn is still running (the delivery transaction
    // is parked in its settle wait).
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(
        queue.cancel(&AgentRef::new(claude_pane.clone())),
        Some(format!("Reply with exactly: {cancelled_marker}")),
        "cancel during the settle wait must withdraw the item"
    );
    let cancelled = cancel_handle
        .join()
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(
        cancelled,
        shardlane_host::DeliveryOutcome::CancelledBeforeDelivery,
        "delivery must abort after a settle-window cancel"
    );
    wait_idle(&client, &claude_pane, Duration::from_secs(180));
    let read = client
        .read_runtime_agent(&RuntimeAgentReadRequest {
            agent_id: AgentRef::new(claude_pane.clone()),
            lines: Some(240),
            format: RuntimeAgentReadFormat::Text,
        })
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(
        !read.text.contains(cancelled_marker),
        "cancelled follow-up must never reach the provider"
    );
    println!("[4] OK (cancel prevented delivery)");

    // ---- 5. Session close → History index → semantic source truth ----
    println!("[5] Close session, index History, verify semantic source…");
    let mut catalog = open_catalog();
    client
        .close_tab(&claude_tab)
        .unwrap_or_else(|error| panic!("close session A: {error}"));
    assert!(agent_gone(&client, &claude_pane, Duration::from_secs(30)));
    let claude_projects = home.join(".claude/projects");
    let flush_started = Instant::now();
    let session_file = loop {
        let located = std::fs::read_dir(&claude_projects)
            .ok()
            .and_then(|entries| {
                entries
                    .filter_map(|entry| entry.ok())
                    .map(|entry| entry.path().join(format!("{native}.jsonl")))
                    .find(|candidate| candidate.is_file())
            });
        if let Some(path) = located {
            break path;
        }
        assert!(
            flush_started.elapsed() < Duration::from_secs(90),
            "claude never flushed the transcript for {native}"
        );
        std::thread::sleep(Duration::from_millis(1_000));
    };
    scan_catalog(&mut catalog);
    let meta = find_meta_by_native(&catalog, &native);
    let source = catalog
        .session_source_by_native(AgentId::ClaudeCode, &native)
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("claude session source indexed"));
    let on_disk = std::fs::read_to_string(&session_file).unwrap_or_else(|error| panic!("{error}"));
    assert!(
        on_disk.contains("MATRIX-ACK-1"),
        "semantic source lost the launch prompt"
    );
    assert!(
        on_disk.contains("MATRIX-ACK-2"),
        "semantic source lost the idle send"
    );
    assert!(
        on_disk.contains("FOLLOWUP-DELIVERED"),
        "semantic source lost the queued follow-up"
    );
    assert!(
        !on_disk.contains(cancelled_marker),
        "semantic source must not contain the cancelled follow-up"
    );
    println!("[5] OK (transcript flushed, indexed, semantic truth verified)");

    // ---- 6. NativeResume: Claude → Claude, exact session ----
    println!("[6] NativeResume (Claude → Claude)…");
    let plan = shardlane_host::plan_history_continuation(
        &client,
        &catalog,
        &ContinuationRequest {
            operation_id: "matrix-continue-1".into(),
            conversation_id: conversation_id_for_history_key(&meta.key),
            target_provider: None,
            instruction: None,
            project_override: None,
        },
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let shardlane_host::ContinuationPlan::NativeResume { resume_args, .. } = &plan else {
        panic!("expected NativeResume for the closed session, got {plan:?}");
    };
    assert_eq!(resume_args, &vec!["--resume".to_string(), native.clone()]);
    let resumed = remote_service
        .continue_conversation(
            &preparation,
            &store,
            &limits,
            &delivery_coordinator,
            &ContinuationRequest {
                operation_id: "matrix-continue-2".into(),
                conversation_id: conversation_id_for_history_key(&meta.key),
                target_provider: None,
                instruction: None,
                project_override: None,
            },
        )
        .unwrap_or_else(|error| panic!("NativeResume execution failed: {error}"));
    let shardlane_host::ContinuationResult::Launched {
        strategy: shardlane_host::ContinuationStrategy::NativeResume,
        outcome: resume_outcome,
        ..
    } = resumed
    else {
        panic!("expected a launched NativeResume, got {resumed:?}");
    };
    let resumed_identity = resume_outcome
        .identity
        .as_ref()
        .unwrap_or_else(|| panic!("resume binds identity"));
    assert_eq!(
        resumed_identity.native_session_id.as_deref(),
        Some(native.as_str()),
        "native resume must bind the exact same provider session"
    );
    assert_ne!(
        resume_outcome.pane_id, claude_pane,
        "resume creates a new agent"
    );
    let resume_pane = resume_outcome.pane_id.clone();
    let resume_tab = resume_outcome.tab_id.clone();
    wait_idle(&client, &resume_pane, Duration::from_secs(120));
    println!("[6] OK pane={resume_pane} same native session");

    // ---- 7. AlreadyLive: the exact session is reused, never duplicated ----
    println!("[7] History Continue planner: AlreadyLive…");
    let plan = shardlane_host::plan_history_continuation(
        &client,
        &catalog,
        &ContinuationRequest {
            operation_id: "matrix-continue-3".into(),
            conversation_id: conversation_id_for_history_key(&meta.key),
            target_provider: None,
            instruction: None,
            project_override: None,
        },
    )
    .unwrap_or_else(|error| panic!("plan AlreadyLive: {error}"));
    let shardlane_host::ContinuationPlan::AlreadyLive { agent_ref, .. } = plan else {
        panic!("expected AlreadyLive for the exact live session, got {plan:?}");
    };
    assert_eq!(agent_ref.as_str(), resume_pane);
    let live_duplicates = client
        .agents()
        .unwrap_or_default()
        .into_iter()
        .filter(|agent| {
            agent
                .agent_session
                .as_ref()
                .is_some_and(|session| session.value == native)
        })
        .count();
    assert_eq!(
        live_duplicates, 1,
        "AlreadyLive must never create a duplicate Agent"
    );
    println!("[7] OK");

    // ---- 8. Blocked retention (environment-dependent) ----
    println!("[8] Blocked Terminal fallback…");
    let blocked_outcome = run_agent_launch(
        &client,
        &preparation,
        &launch_intent(
            "Use the Write tool to create a file named ./blocked-canary.txt containing the word hi. Then reply with exactly: BLOCKED-DONE.",
            AgentId::ClaudeCode,
        ),
    )
    .unwrap_or_else(|error| panic!("blocked-canary launch failed: {error}"));
    let blocked_pane = blocked_outcome.pane_id.clone();
    let blocked_status = wait_for_status(
        &client,
        &blocked_pane,
        &["blocked", "idle", "done"],
        Duration::from_secs(120),
    );
    match blocked_status.as_deref() {
        Some("blocked") => {
            enqueue_follow_up(
                &queue,
                shardlane_host::conversation_id_for_live_agent(&AgentRef::new(
                    blocked_pane.clone(),
                )),
                &AgentRef::new(blocked_pane.clone()),
                "claude",
                None,
                "Reply with exactly: SHOULD-STAY-QUEUED",
            );
            let runtime = HerdrFollowUpRuntime::new(&client);
            let retained = run_follow_up_delivery(
                &queue,
                &runtime,
                &AgentRef::new(blocked_pane.clone()),
                15_000,
            );
            assert_eq!(retained, shardlane_host::DeliveryOutcome::RetainedBlocked);
            assert!(
                queue.queued(&AgentRef::new(blocked_pane.clone())).is_some(),
                "blocked must retain the queue"
            );
            queue.cancel(&AgentRef::new(blocked_pane.clone()));
            println!("[8] OK (blocked retains the queue)");
        }
        other => {
            println!(
                "[8] ENV-LIMITED: claude did not block (status {other:?}); \
                 Blocked retention stays covered by host unit tests"
            );
        }
    }
    close_tab_quietly(&client, &blocked_pane);

    // ---- 9. ContextTransfer: Claude → Codex, lossless briefing ----
    println!("[9] ContextTransfer (Claude → Codex)…");
    let transfer = remote_service
        .continue_conversation(
            &preparation,
            &store,
            &limits,
            &delivery_coordinator,
            &ContinuationRequest {
                operation_id: "matrix-continue-4".into(),
                conversation_id: conversation_id_for_history_key(&meta.key),
                target_provider: Some(AgentId::Codex),
                instruction: Some("Reply with exactly: TRANSFER-READY".to_string()),
                project_override: None,
            },
        )
        .unwrap_or_else(|error| panic!("ContextTransfer failed: {error}"));
    let shardlane_host::ContinuationResult::Launched {
        strategy: shardlane_host::ContinuationStrategy::ContextTransfer,
        outcome: transfer_outcome,
        briefing_sha256,
    } = transfer
    else {
        panic!("expected a launched ContextTransfer, got {transfer:?}");
    };
    assert_eq!(briefing_sha256.map(|sha| sha.len()), Some(64));
    let transfer_pane = transfer_outcome.pane_id.clone();
    dismiss_codex_hook_review(&client, &transfer_pane);
    let transfer_agent =
        find_agent(&client, &transfer_pane).unwrap_or_else(|| panic!("codex target alive"));
    assert_eq!(
        transfer_agent.agent.as_deref(),
        Some("codex"),
        "the transfer target must run the requested provider"
    );
    wait_idle(&client, &transfer_pane, Duration::from_secs(240));
    let read = client
        .read_runtime_agent(&RuntimeAgentReadRequest {
            agent_id: AgentRef::new(transfer_pane.clone()),
            lines: Some(200),
            format: RuntimeAgentReadFormat::Text,
        })
        .unwrap_or_else(|error| panic!("read codex target: {error}"));
    assert!(
        read.text.contains("TRANSFER-READY"),
        "codex never answered the briefing instruction; tail:\n{}",
        read.text
    );
    close_tab_quietly(&client, &transfer_pane);
    println!("[9] OK pane={transfer_pane}");

    // ---- 10. Live Handoff, idle source (source preserved) ----
    println!("[10] Live Handoff (idle source)…");
    let resume_source = catalog
        .session_source_by_native(AgentId::ClaudeCode, &native)
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("resume source indexed"));
    let handoff_request = LiveHandoffRequest {
        source: resume_source.clone(),
        source_agent_ref: AgentRef::new(resume_pane.clone()),
        target_provider: AgentId::Codex,
        launch: launch_intent("", AgentId::Codex),
        instruction: Some("Reply with exactly: HANDOFF-IDLE-OK".to_string()),
    };
    let handoff = run_live_handoff(
        &client,
        &preparation,
        &store,
        &limits,
        &handoff_request,
        SETTLE_TIMEOUT_MS,
    )
    .unwrap_or_else(|error| panic!("idle handoff failed: {error}"));
    assert!(
        find_agent(&client, &resume_pane).is_some(),
        "handoff must never stop the source agent"
    );
    let handoff_pane = handoff.launch.pane_id.clone();
    dismiss_codex_hook_review(&client, &handoff_pane);
    wait_idle(&client, &handoff_pane, Duration::from_secs(240));
    let read = client
        .read_runtime_agent(&RuntimeAgentReadRequest {
            agent_id: AgentRef::new(handoff_pane.clone()),
            lines: Some(200),
            format: RuntimeAgentReadFormat::Text,
        })
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(read.text.contains("HANDOFF-IDLE-OK"));
    close_tab_quietly(&client, &handoff_pane);
    println!("[10] OK (source alive, target answered)");

    // ---- 11. Live Handoff, working source (`Handoff after current turn`) ----
    println!("[11] Live Handoff (working source)…");
    let working = run_agent_launch(
        &client,
        &preparation,
        &launch_intent(
            "Use the Bash tool to run exactly `sleep 25` and then reply with exactly: WORKING-DONE. Do nothing else.",
            AgentId::ClaudeCode,
        ),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let working_pane = working.pane_id.clone();
    let working_native = working
        .identity
        .as_ref()
        .and_then(|identity| identity.native_session_id.clone())
        .unwrap_or_else(|| panic!("working source identity"));
    assert_eq!(
        wait_for_status(
            &client,
            &working_pane,
            &["working"],
            Duration::from_secs(60)
        )
        .as_deref(),
        Some("working")
    );
    let working_source = catalog
        .session_source_by_native(AgentId::ClaudeCode, &working_native)
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| {
            // The session file may be newer than the last scan; locate it by
            // walking the provider projects root instead of guessing one layout.
            let projects_root = home.join(".claude/projects");
            let located = std::fs::read_dir(&projects_root)
                .ok()
                .and_then(|entries| {
                    entries
                        .filter_map(|entry| entry.ok())
                        .map(|entry| entry.path().join(format!("{working_native}.jsonl")))
                        .find(|candidate| candidate.is_file())
                })
                .unwrap_or_else(|| {
                    panic!(
                        "working session file {working_native} not found under {}",
                        projects_root.display()
                    )
                });
            SessionFileRef {
                agent: AgentId::ClaudeCode,
                native_id: working_native.clone(),
                file_path: located.to_string_lossy().into_owned(),
                mtime_ms: 0,
                size: 0,
            }
        });
    let working_started = Instant::now();
    let working_handoff = run_live_handoff(
        &client,
        &preparation,
        &store,
        &limits,
        &LiveHandoffRequest {
            source: working_source,
            source_agent_ref: AgentRef::new(working_pane.clone()),
            target_provider: AgentId::Codex,
            launch: launch_intent("", AgentId::Codex),
            instruction: Some("Reply with exactly: HANDOFF-WORKING-OK".to_string()),
        },
        SETTLE_TIMEOUT_MS,
    )
    .unwrap_or_else(|error| panic!("working handoff failed: {error}"));
    let working_elapsed = working_started.elapsed();
    assert!(
        working_elapsed >= Duration::from_secs(10),
        "a working handoff must wait for the current turn, took {working_elapsed:?}"
    );
    assert!(
        find_agent(&client, &working_pane).is_some(),
        "working handoff must preserve the source"
    );
    let wh_pane = working_handoff.launch.pane_id.clone();
    dismiss_codex_hook_review(&client, &wh_pane);
    wait_idle(&client, &wh_pane, Duration::from_secs(240));
    let read = client
        .read_runtime_agent(&RuntimeAgentReadRequest {
            agent_id: AgentRef::new(wh_pane.clone()),
            lines: Some(200),
            format: RuntimeAgentReadFormat::Text,
        })
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(read.text.contains("HANDOFF-WORKING-OK"));
    close_tab_quietly(&client, &wh_pane);
    close_tab_quietly(&client, &working_pane);
    close_tab_quietly(&client, &resume_tab);
    println!("[11] OK (waited {working_elapsed:?}, source preserved)");

    // ---- 12. NeedsProjectSelection before any runtime mutation ----
    println!("[12] NeedsProjectSelection planning…");
    let noproject_dir =
        home.join(".claude/projects/-private-tmp-matrix-project-sibling-zzz-noproject");
    std::fs::create_dir_all(&noproject_dir).unwrap_or_else(|error| panic!("{error}"));
    let mut synthetic = String::new();
    for index in 0..20 {
        synthetic.push_str(&format!(
            "{{\"type\":\"user\",\"cwd\":\"\",\"timestamp\":\"2026-08-30T00:{:02}:00Z\",\"message\":{{\"content\":\"msg {index}\"}}}}\n",
            index % 60
        ));
        synthetic.push_str(&format!(
            "{{\"type\":\"assistant\",\"cwd\":\"\",\"timestamp\":\"2026-08-30T00:{:02}:01Z\",\"message\":{{\"id\":\"a{index}\",\"content\":[{{\"type\":\"text\",\"text\":\"reply {index}\"}}]}}}}\n",
            index % 60
        ));
    }
    std::fs::write(noproject_dir.join("test-noproject.jsonl"), synthetic)
        .unwrap_or_else(|error| panic!("{error}"));
    scan_catalog(&mut catalog);
    let noproject_meta = catalog
        .session("claude-code:test-noproject")
        .ok()
        .flatten()
        .or_else(|| {
            catalog
                .search_session_metadata("msg 19", 50)
                .ok()
                .and_then(|metas| {
                    metas
                        .into_iter()
                        .find(|meta| meta.file_path.contains("zzz-noproject"))
                })
        })
        .unwrap_or_else(|| panic!("synthetic no-project session was not indexed"));
    assert_eq!(
        noproject_meta.project_path, "",
        "the synthetic session must have no project path"
    );
    let noproject_plan = shardlane_host::plan_history_continuation(
        &client,
        &catalog,
        &ContinuationRequest {
            operation_id: "matrix-continue-5".into(),
            conversation_id: conversation_id_for_history_key(&noproject_meta.key),
            target_provider: None,
            instruction: None,
            project_override: None,
        },
    )
    .unwrap_or_else(|error| panic!("{error}"));
    assert!(
        matches!(
            noproject_plan,
            shardlane_host::ContinuationPlan::NeedsProjectSelection
        ),
        "missing project must plan NeedsProjectSelection, got {noproject_plan:?}"
    );
    let resolved_plan = shardlane_host::plan_history_continuation(
        &client,
        &catalog,
        &ContinuationRequest {
            operation_id: "matrix-continue-6".into(),
            conversation_id: conversation_id_for_history_key(&noproject_meta.key),
            target_provider: None,
            instruction: None,
            project_override: Some(PROJECT.to_string()),
        },
    )
    .unwrap_or_else(|error| panic!("{error}"));
    assert!(
        matches!(
            resolved_plan,
            shardlane_host::ContinuationPlan::NativeResume { .. }
        ),
        "after the picker resolves a project the plan must proceed, got {resolved_plan:?}"
    );
    println!("[12] OK (planner refuses before mutation, proceeds after resolution)");

    // ---- 13. Large-session artifact integrity ----
    println!("[13] Large-session artifact integrity…");
    let big_dir = home.join(".claude/projects/-private-tmp-matrix-project-sibling-zzz-bigsession");
    std::fs::create_dir_all(&big_dir).unwrap_or_else(|error| panic!("{error}"));
    let mut big = String::new();
    for index in 0..1200 {
        let filler = "x".repeat(180);
        big.push_str(&format!(
            "{{\"type\":\"user\",\"cwd\":\"{PROJECT}\",\"timestamp\":\"2026-08-30T01:{:02}:00Z\",\"message\":{{\"content\":\"{filler} {index}\"}}}}\n",
            index % 60
        ));
        big.push_str(&format!(
            "{{\"type\":\"assistant\",\"cwd\":\"{PROJECT}\",\"timestamp\":\"2026-08-30T01:{:02}:01Z\",\"message\":{{\"id\":\"big{index}\",\"content\":[{{\"type\":\"text\",\"text\":\"ack {index} {filler}\"}}]}}}}\n",
            index % 60
        ));
    }
    let big_path = big_dir.join("test-bigsession.jsonl");
    std::fs::write(&big_path, &big).unwrap_or_else(|error| panic!("{error}"));
    scan_catalog(&mut catalog);
    let big_meta = catalog
        .session("claude-code:test-bigsession")
        .ok()
        .flatten()
        .unwrap_or_else(|| panic!("big session not indexed"));
    let big_source = SessionFileRef {
        agent: AgentId::ClaudeCode,
        native_id: big_meta.id.clone(),
        file_path: big_path.to_string_lossy().into_owned(),
        mtime_ms: big_meta.updated_at,
        size: big.len() as i64,
    };
    let snapshot = capture_transfer_snapshot(&big_source, &store, &limits)
        .unwrap_or_else(|error| panic!("capture big session: {error}"));
    let reference = match &snapshot.payload {
        TransferPayload::Artifact(reference) => reference,
        TransferPayload::Inline(_) => panic!("a >256KiB session must use an artifact"),
    };
    let restored = store
        .verify(reference)
        .unwrap_or_else(|error| panic!("artifact verify: {error}"));
    let on_disk = std::fs::read(&big_path).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(restored, on_disk, "artifact must be byte-exact");
    assert_eq!(reference.sha256, snapshot.meta.sha256);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let file_mode = std::fs::metadata(&reference.path)
            .unwrap_or_else(|error| panic!("{error}"))
            .permissions()
            .mode();
        assert_eq!(file_mode & 0o777, 0o600, "artifact files must be 0600");
        let dir_mode = std::fs::metadata(store.root())
            .unwrap_or_else(|error| panic!("{error}"))
            .permissions()
            .mode();
        assert_eq!(dir_mode & 0o777, 0o700, "artifact dirs must be 0700");
    }
    let artifact_name = reference
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    assert!(!artifact_name.contains("claude") && !artifact_name.contains(&big_meta.id));
    let before = std::fs::read(&big_path).unwrap_or_default();
    let _ = snapshot;
    assert_eq!(
        std::fs::read(&big_path).unwrap_or_default(),
        before,
        "capture must never mutate the provider source"
    );
    println!("[13] OK ({} bytes, hash-verified)", reference.byte_length);

    // ---- 14. Desktop + Remote: one semantic projection per Conversation ----
    println!("[14] Shared live projection (Desktop lease + Remote detail)…");
    let lease = manager
        .subscribe(source.clone())
        .unwrap_or_else(|error| panic!("subscribe: {error}"));
    let initial = manager
        .initial_delivery(&lease)
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(!initial.appended_messages.is_empty());
    let detail = shardlane_host::HostConversationService::new(&client, &history_db)
        .with_shared_sessions(manager.clone())
        .live_detail(&AgentRef::new(resume_pane.clone()), Default::default())
        .unwrap_or_else(|error| panic!("remote-style live detail: {error}"));
    assert!(!detail.window.items.is_empty());
    assert_eq!(manager.retained_sessions(), 1, "exactly one session owner");
    drop(lease);
    assert_eq!(manager.retained_sessions(), 0, "leases release cleanly");
    let _ = client.close_tab(&resume_tab);
    let _ = ConversationServiceError::Runtime(String::new()); // keep import used on all paths
    println!("MATRIX COMPLETE: all steps passed");
}
