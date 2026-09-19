//! Hook Adapter Layer: strict identity normalization + Shardlane Hook Journal.
//!
//! [INPUT]: AgentHookReport from the Tier-1 IPC/OSC paths, AgentId alias
//! authority from shardlane-history resolve_agent_alias, Antigravity live
//! turns from shardlane-history adapters::antigravity_live, and the
//! Shardlane-owned hook journal directory.
//! [OUTPUT]: NormalizedHookEvent, classify_hook_event, journal enrichment
//! (Antigravity live turns backfill), HookEventJournal (bounded JSONL
//! append + rotation), normalize_agent_identity (fail-closed),
//! journal_path_for/journal_root helpers.
//! [POS]: agent_hooks 的语义适配层：把各 CLI 原生 hook 的原始事件归一为
//! provider 中立事件并写入 Shardlane 自有 journal；身份判定只信
//! capability registry 的别名表（单一权威），未知来源一律拒绝——这同时
//! 是"其他 Server 被误识别成 Agent"的治理点。journal 是
//! LiveCapability::HookJournal 的数据源，不是第二运行时：进程与状态
//! 权威仍在 Herdr。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use crate::agent_hooks::ipc::AgentHookReport;
use shardlane_history::adapters::antigravity_live::{antigravity_live_turns, AgLiveTurn};
use shardlane_history::{resolve_agent_alias, AgentId};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Normalized hook event kinds (journal wire vocabulary, v1).
///
/// 命名即契约：journal 解码器（shardlane-history live::journal）按
/// kind 字符串投影消息；新增 kind 必须两侧同步并升版本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookEventKind {
    SessionStart,
    UserPrompt,
    AssistantMessage,
    TurnComplete,
    Status,
    SessionEnd,
    /// Provider 上报了事件但语义未知：进 journal 计数，不产生消息。
    Unrecognized,
}

impl HookEventKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::SessionStart => "session_start",
            Self::UserPrompt => "user_prompt",
            Self::AssistantMessage => "assistant_message",
            Self::TurnComplete => "turn_complete",
            Self::Status => "status",
            Self::SessionEnd => "session_end",
            Self::Unrecognized => "unrecognized",
        }
    }
}

/// Provider-identity-verified, pane-bound semantic event ready for the journal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedHookEvent {
    pub agent: AgentId,
    pub kind: HookEventKind,
    pub session_id: String,
    pub text: String,
    pub ts_ms: u64,
    pub pane: Option<String>,
    pub cwd: Option<String>,
    pub model: Option<String>,
}

/// Strict provider identity: the capability registry alias table is the sole
/// authority. Unknown/misreported agents (e.g. MCP servers, shells, or any
/// non-agent process that happens to emit a report) fail closed to None and
/// must never enter the journal or Chat identity paths.
pub fn normalize_agent_identity(raw: &str, secondary: Option<&str>) -> Option<AgentId> {
    resolve_agent_alias(raw, secondary)
}

/// Map a raw provider hook event name onto the journal vocabulary.
/// Names follow the union observed across native CLI hook systems
/// (Claude/Codex/Qwen/Gemini/Cursor/Grok: SessionStart/UserPromptSubmit/Stop…;
/// Antigravity: PreInvocation/Stop).
pub fn classify_hook_event(raw: &str) -> HookEventKind {
    match raw {
        "SessionStart" | "session_start" | "PreInvocation" => HookEventKind::SessionStart,
        "UserPromptSubmit" | "user_prompt" | "before_submit_prompt" => HookEventKind::UserPrompt,
        "AssistantMessage" | "assistant_message" | "lastAssistantMessage" => {
            HookEventKind::AssistantMessage
        }
        "Stop" | "stop" | "TurnComplete" | "turn_complete" | "agent_turn_complete" => {
            HookEventKind::TurnComplete
        }
        "SessionEnd" | "session_end" => HookEventKind::SessionEnd,
        "Notification" | "notification" | "status" => HookEventKind::Status,
        _ => HookEventKind::Unrecognized,
    }
}

/// Normalize one IPC/OSC report into a journal event. Returns None unless
/// BOTH the provider identity resolves through the registry AND a stable
/// session key exists — a report without session identity cannot anchor a
/// conversation and must not be guessed into one.
pub fn normalize_report(report: &AgentHookReport) -> Option<NormalizedHookEvent> {
    let agent = normalize_agent_identity(&report.agent, None)?;
    // 写入/读取对称（2026-09-19 审计修复）：只为 HookJournal 档位的
    // provider 落 journal——AppendLog provider 的语义源是原生会话文件，
    // 复制 prompt 只扩大隐私面且永远没有读端。
    if !shardlane_history::provider_capabilities(agent)?
        .live
        .is_hook_journal()
    {
        return None;
    }
    // 无 provider session id 的语义事件拒绝：读端只按 Herdr
    // AgentSessionInfo 的 native id 寻址，pane 锚定键永远无法被寻址，
    // 且 pane 复用会跨会话拼接两个对话。
    // 空串同样拒绝（hook 脚本对缺失 session 的载荷会发 ""，历史上
    // 它曾绕过回退把所有会话写进同一个 session.jsonl——审计 adv2-F1）。
    let session_id = truncate_chars(
        report
            .session_id
            .as_deref()
            .filter(|value| !value.is_empty())?,
        JOURNAL_SESSION_MAX_CHARS,
    );
    let kind = report
        .event
        .as_deref()
        .map(classify_hook_event)
        .unwrap_or(HookEventKind::Status);
    Some(NormalizedHookEvent {
        agent,
        kind,
        session_id,
        text: truncate_chars(report.text.as_deref().unwrap_or(""), JOURNAL_TEXT_MAX_CHARS),
        // 时间戳契约（审计 adv1-F3）：IPC 载荷不携带 timestamp（serde
        // 默认 0），OSC 路径给秒。0 = 未知 → 在 ingest 侧盖接收时刻；
        // 非 0 按秒转毫秒。
        ts_ms: if report.timestamp == 0 {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_millis() as u64)
                .unwrap_or(0)
        } else {
            report.timestamp.saturating_mul(1000)
        },
        pane: report.resolved_pane_id().map(str::to_string),
        cwd: report.cwd.clone(),
        model: None,
    })
}

/// Bounded per-session JSONL journal writer. Shardlane-owned storage only;
/// provider session files are never written.
pub struct HookEventJournal;

/// Journal 单文件上限：超限后轮转（path → path.1），transport 侧
/// Shrunk→Replace 语义天然触发整体重建。轮转语义（2026-09-19 审计
/// 记录）：HookJournal 档位的 journal 是该会话唯一在线 transcript，
/// 轮转会重置 Chat 可见历史；durable 全量 transcript 由规划中的
/// AntigravityAdapter v2（会话库直读）承担，不在此层拼接双代。
const JOURNAL_MAX_BYTES: u64 = 8 * 1024 * 1024;
/// 轮转保留一代：活动文件 + 至多一个历史文件。
const JOURNAL_ROTATIONS: usize = 1;
/// 单 agent 目录 journal 文件数上限（追加时按 mtime 淘汰最旧）。
const JOURNAL_MAX_FILES_PER_AGENT: usize = 64;
/// 单条事件 text 的持久化上限（超出截断——防 IPC 毒化与内存放大）。
const JOURNAL_TEXT_MAX_CHARS: usize = 64 * 1024;
/// session id 的持久化上限（与文件名 sanitize 上限一致）。
const JOURNAL_SESSION_MAX_CHARS: usize = 128;

/// 截断到字符上限（UTF-8 边界安全）。
fn truncate_chars(raw: &str, max_chars: usize) -> String {
    if raw.chars().count() <= max_chars {
        return raw.to_string();
    }
    raw.chars().take(max_chars).collect()
}

/// Host 侧统一 ingest 接缝（2026-09-19 审计修复）：归一化 + 落 journal
/// 的所有权在 Host，不挂在 GUI 视图控制器上。IPC 服务器线程与 GUI 的
/// OSC 管道都调用这里；失败静默（journal 是尽力而为的语义覆盖层）。
pub fn ingest_report(report: &AgentHookReport) {
    let Some(root) = HookEventJournal::default_root() else {
        return;
    };
    if let Some(event) = normalize_report(report) {
        let outcome = HookEventJournal::append(&root, &event);
        crate::op_log(
            if outcome.is_ok() { "INFO" } else { "WARN" },
            format_args!(
                "hook journal: agent={} kind={} session={} outcome={}",
                event.agent.as_str(),
                event.kind.as_str(),
                event.session_id,
                match &outcome {
                    Ok(()) => "ok".to_string(),
                    Err(error) => format!("err ({error})"),
                },
            ),
        );
        // Antigravity 正文 enrichment（AntigravityAdapter v2）：agy 的钩子
        // 载荷不带任何文本，journal 里只有生命周期骨架；回合正文权威在
        // agy 自己的会话库。会话事件到达时做一次增量回填，journal 仍是
        // Chat 的唯一 live 源（解码器不变，第二运行时边界不破）。
        if outcome.is_ok()
            && event.agent == AgentId::Antigravity
            && matches!(
                event.kind,
                HookEventKind::SessionStart | HookEventKind::TurnComplete
            )
        {
            let turns = antigravity_live_turns(&event.session_id);
            if !turns.is_empty() {
                let appended = enrich_append_missing(&root, &event, &turns);
                crate::op_log(
                    "INFO",
                    format_args!(
                        "hook enrich: agent=antigravity session={} source_turns={} appended={}",
                        event.session_id,
                        turns.len(),
                        appended,
                    ),
                );
            }
        }
    } else {
        crate::op_log(
            "INFO",
            format_args!(
                "hook report rejected: agent={} event={} session={} (identity/capability/anchor gate)",
                report.agent,
                report.event.as_deref().unwrap_or("-"),
                report.session_id.as_deref().unwrap_or("-"),
            ),
        );
    }
}

/// Enrichment 写入核：只追加 journal 里还没有的回合（按 ts + 文本指纹
/// 去重），锚定事件携带 pane/cwd 上下文。测试可直接喂合成 turns。
fn enrich_append_missing(root: &Path, anchor: &NormalizedHookEvent, turns: &[AgLiveTurn]) -> usize {
    // 进程内串行化："读 journal 去重集 -> 追加"的窗口不允许并发交错
    //（IPC 线程与 OSC 管道都可能到达 ingest）。跨进程无锁与 journal
    // 写入器的既有记录在案限制一致。
    static ENRICH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _serial = ENRICH_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut seen = journaled_assistant_keys(root, anchor.agent, &anchor.session_id);
    let mut appended = 0usize;
    for turn in turns {
        // 截断在写入点收口：journal 的 append 不截断，enrichment 是
        // 绕过 normalize_report 的第二个写入者；指纹必须对截断后的
        // 落盘文本计算，否则下个事件的去重集永远对不上（全量重追加）。
        let text = truncate_chars(&turn.text, JOURNAL_TEXT_MAX_CHARS);
        let key = (turn.ts_ms, text_fingerprint(&text));
        if seen.contains(&key) {
            continue;
        }
        let event = NormalizedHookEvent {
            agent: anchor.agent,
            kind: HookEventKind::AssistantMessage,
            session_id: anchor.session_id.clone(),
            text,
            ts_ms: u64::try_from(turn.ts_ms.max(0)).unwrap_or(0),
            pane: anchor.pane.clone(),
            cwd: anchor.cwd.clone(),
            model: None,
        };
        if HookEventJournal::append(root, &event).is_err() {
            break;
        }
        // 单批次内的重复行同样要互相去重（同 ts 同文本只落一条）。
        seen.insert(key);
        appended += 1;
    }
    appended
}

/// 读取 journal 现有 assistant_message 记录的去重键集
///（(ts_ms, 文本指纹)）。journal 是有界小文件，整读可接受。
fn journaled_assistant_keys(root: &Path, agent: AgentId, session_id: &str) -> HashSet<(i64, u64)> {
    let mut keys = HashSet::new();
    // 字节读取 + lossy 解码：非 UTF-8（跨进程写入撕裂的残留）只降级
    // 单行，不能让整个去重集丢失——那会导致窗口内全量重追加。
    let Ok(bytes) = fs::read(HookEventJournal::journal_path_for(root, agent, session_id)) else {
        return keys;
    };
    let content = String::from_utf8_lossy(&bytes);
    for line in content.lines() {
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if record.get("kind").and_then(|kind| kind.as_str()) != Some("assistant_message") {
            continue;
        }
        let ts = record.get("ts").and_then(|ts| ts.as_i64()).unwrap_or(0);
        let text = record
            .get("text")
            .and_then(|text| text.as_str())
            .unwrap_or("");
        keys.insert((ts, text_fingerprint(text)));
    }
    keys
}

fn text_fingerprint(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

impl HookEventJournal {
    /// Default journal root: ~/.shardlane/hook-journal.
    pub fn default_root() -> Option<PathBuf> {
        std::env::var_os("HOME")
            .map(|home| Path::new(&home).join(".shardlane").join("hook-journal"))
    }

    /// Journal file for one (agent, session). Session ids are sanitized to a
    /// flat filename charset so provider-controlled ids can never traverse.
    pub fn journal_path_for(root: &Path, agent: AgentId, session_id: &str) -> PathBuf {
        root.join(agent.as_str())
            .join(format!("{}.jsonl", sanitize(session_id)))
    }

    /// Append one normalized event as a v1 JSONL record. Best-effort bounded
    /// write: journal failures never propagate into caller control flow.
    pub fn append(root: &Path, event: &NormalizedHookEvent) -> std::io::Result<()> {
        let path = Self::journal_path_for(root, event.agent, &event.session_id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
            // prompt 正文落盘：目录收紧到 0700（与 IPC socket 目录先例
            // 一致），文件在 open 时指定 0600。
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
        let _ = fs::set_permissions(root, fs::Permissions::from_mode(0o700));
        Self::prune_excess(root, event.agent)?;
        // 符号链接/非普通文件拒绝：journal 命名空间必须是普通文件，
        // 预置符号链接会把追加内容写到任意用户文件（fail-closed）。
        if let Ok(meta) = fs::symlink_metadata(&path) {
            if !meta.is_file() {
                return Err(std::io::Error::other("journal path is not a regular file"));
            }
        }
        if matches!(fs::metadata(&path), Ok(meta) if meta.len() >= JOURNAL_MAX_BYTES) {
            Self::rotate(&path)?;
        }
        let record = serde_json::json!({
            "v": 1,
            "kind": event.kind.as_str(),
            "text": event.text,
            "ts": event.ts_ms,
            "session": event.session_id,
            "cwd": event.cwd,
            "model": event.model,
        });
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&path)?;
        // 单次 write_all（含换行）：多写者场景下把行撕裂窗口压到最小
        //（审计 adv2-F4；无跨进程锁是记录在案的限制）。
        let line = format!("{record}\n");
        file.write_all(line.as_bytes())
    }

    /// 每 agent 目录文件数上限：超限时按 mtime 淘汰最旧（只删本层
    /// 目录里匹配 journal 命名模式的普通文件，绝不递归）。
    fn prune_excess(root: &Path, agent: AgentId) -> std::io::Result<()> {
        let dir = root.join(agent.as_str());
        let mut entries: Vec<(PathBuf, std::time::SystemTime)> = fs::read_dir(&dir)?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                if !name.ends_with(".jsonl") && !name.ends_with(".jsonl.1") {
                    return None;
                }
                let meta = entry.metadata().ok()?;
                if !meta.is_file() {
                    return None;
                }
                let mtime = meta.modified().ok()?;
                Some((entry.path(), mtime))
            })
            .collect();
        if entries.len() <= JOURNAL_MAX_FILES_PER_AGENT {
            return Ok(());
        }
        entries.sort_by_key(|(_, mtime)| *mtime);
        let excess = entries.len().saturating_sub(JOURNAL_MAX_FILES_PER_AGENT);
        for (path, _) in entries.into_iter().take(excess) {
            let _ = fs::remove_file(&path);
        }
        Ok(())
    }

    fn rotate(path: &Path) -> std::io::Result<()> {
        for index in (1..JOURNAL_ROTATIONS).rev() {
            let from = rotation_path(path, index);
            let to = rotation_path(path, index + 1);
            if from.exists() {
                fs::rename(&from, &to)?;
            }
        }
        fs::rename(path, rotation_path(path, 1))
    }
}

fn rotation_path(path: &Path, index: usize) -> PathBuf {
    let mut name = path.file_name().map_or_else(
        || "journal.jsonl".to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    name.push_str(&format!(".{index}"));
    path.with_file_name(name)
}

/// Flat filename charset: [A-Za-z0-9._-]，其余一律折叠为 '_'。
fn sanitize(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(128));
    for ch in raw.chars().take(128) {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("session");
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn report(agent: &str, event: Option<&str>, session: Option<&str>) -> AgentHookReport {
        AgentHookReport {
            agent: agent.to_string(),
            status: "working".to_string(),
            pane_id: Some("%3".to_string()),
            tmux_pane: None,
            herdr_pane: None,
            tty: None,
            session_id: session.map(str::to_string),
            cwd: Some("/tmp/proj".to_string()),
            event: event.map(str::to_string),
            text: Some("hello".to_string()),
            timestamp: 42,
        }
    }

    #[test]
    fn unknown_agent_identities_fail_closed() {
        assert_eq!(normalize_agent_identity("mcp-server-gemini", None), None);
        assert_eq!(normalize_agent_identity("shell", None), None);
        assert_eq!(normalize_agent_identity("some-http-server", None), None);
        assert_eq!(
            normalize_agent_identity("agy", None),
            Some(AgentId::Antigravity)
        );
        assert_eq!(
            normalize_agent_identity("claude", None),
            Some(AgentId::ClaudeCode)
        );
    }

    #[test]
    fn normalize_requires_registry_identity_and_session_anchor() {
        // 未知 agent：拒绝（误识别治理的核心断言）。
        assert!(normalize_report(&report("postgres-server", Some("Stop"), Some("s1"))).is_none());
        // 非 HookJournal 档位（claude）不落 journal：写入/读取对称。
        assert!(
            normalize_report(&report("claude", Some("UserPromptSubmit"), Some("s1"))).is_none()
        );
        // HookJournal 档位但无 session id / 空串 session id：拒绝
        //（读端永远无法寻址；空串曾把所有会话写进同一个文件）。
        assert!(normalize_report(&report("agy", Some("PreInvocation"), None)).is_none());
        assert!(normalize_report(&report("agy", Some("PreInvocation"), Some(""))).is_none());
        // 合法锚定。
        let event =
            normalize_report(&report("agy", Some("PreInvocation"), Some("conv-1"))).unwrap();
        assert_eq!(event.session_id, "conv-1");
        assert_eq!(event.kind, HookEventKind::SessionStart);
    }

    #[test]
    fn oversized_text_is_truncated_at_normalization() {
        let mut poison = report("agy", Some("lastAssistantMessage"), Some("conv-1"));
        poison.text = Some("x".repeat(JOURNAL_TEXT_MAX_CHARS + 10));
        let event = normalize_report(&poison).unwrap();
        assert_eq!(event.text.chars().count(), JOURNAL_TEXT_MAX_CHARS);
    }

    #[test]
    fn event_classification_covers_native_hook_vocabularies() {
        assert_eq!(
            classify_hook_event("PreInvocation"),
            HookEventKind::SessionStart
        );
        assert_eq!(
            classify_hook_event("SessionStart"),
            HookEventKind::SessionStart
        );
        assert_eq!(
            classify_hook_event("UserPromptSubmit"),
            HookEventKind::UserPrompt
        );
        assert_eq!(
            classify_hook_event("before_submit_prompt"),
            HookEventKind::UserPrompt
        );
        assert_eq!(
            classify_hook_event("lastAssistantMessage"),
            HookEventKind::AssistantMessage
        );
        assert_eq!(classify_hook_event("Stop"), HookEventKind::TurnComplete);
        assert_eq!(
            classify_hook_event("agent_turn_complete"),
            HookEventKind::TurnComplete
        );
        assert_eq!(
            classify_hook_event("anything-else"),
            HookEventKind::Unrecognized
        );
    }

    /// 跨 crate 契约（审计 arch-5）：host 写入的 journal 必须能被
    /// shardlane-history 的 LiveSession 真实解码——此前两侧只对各自
    /// 手造的字符串断言，wire 漂移会双双绿灯。
    #[test]
    fn journal_round_trips_through_live_session() {
        let dir = tempfile::tempdir().unwrap();
        let event = NormalizedHookEvent {
            agent: AgentId::Antigravity,
            kind: HookEventKind::UserPrompt,
            session_id: "conv-123".to_string(),
            text: "帮我修个 bug".to_string(),
            ts_ms: 1_695_000_000_000,
            pane: Some("%3".to_string()),
            cwd: Some("/tmp/p".to_string()),
            model: None,
        };
        HookEventJournal::append(dir.path(), &event).unwrap();
        HookEventJournal::append(
            dir.path(),
            &NormalizedHookEvent {
                kind: HookEventKind::AssistantMessage,
                text: "看到了".to_string(),
                ..event
            },
        )
        .unwrap();
        let path = HookEventJournal::journal_path_for(dir.path(), AgentId::Antigravity, "conv-123");
        let meta = fs::metadata(&path).unwrap();
        let source = shardlane_history::models::SessionFileRef {
            agent: AgentId::Antigravity,
            native_id: "conv-123".to_string(),
            file_path: path.to_string_lossy().into_owned(),
            mtime_ms: 0,
            size: i64::try_from(meta.len()).unwrap_or(i64::MAX),
        };
        let session = shardlane_history::LiveSession::open(source).unwrap();
        let snapshot = session.snapshot();
        assert_eq!(snapshot.messages.len(), 2);
        assert_eq!(snapshot.messages[0].text, "帮我修个 bug");
        assert_eq!(snapshot.messages[1].text, "看到了");
        assert_eq!(snapshot.facts.session_id.as_deref(), Some("conv-123"));
    }

    #[test]
    fn session_ids_are_sanitized_to_flat_filenames() {
        let path = HookEventJournal::journal_path_for(
            Path::new("/tmp/j"),
            AgentId::Antigravity,
            "../../etc/passwd",
        );
        assert!(path.ends_with(".._.._etc_passwd.jsonl"));
        assert_eq!(path.parent().unwrap(), Path::new("/tmp/j/antigravity"));
    }

    fn anchor(session: &str) -> NormalizedHookEvent {
        NormalizedHookEvent {
            agent: AgentId::Antigravity,
            kind: HookEventKind::TurnComplete,
            session_id: session.to_string(),
            text: String::new(),
            ts_ms: 1_700_000_000_000,
            pane: Some("%3".to_string()),
            cwd: Some("/tmp/proj".to_string()),
            model: None,
        }
    }

    #[test]
    fn enrichment_appends_missing_turns_then_dedupes() {
        let dir = tempfile::tempdir().unwrap();
        let anchor = anchor("conv-enrich");
        let turns = vec![
            AgLiveTurn {
                ts_ms: 1_700_000_010_000,
                text: "第一条回复".to_string(),
            },
            AgLiveTurn {
                ts_ms: 1_700_000_020_000,
                text: "第二条回复".to_string(),
            },
        ];
        assert_eq!(enrich_append_missing(dir.path(), &anchor, &turns), 2);
        // 同一批 turns 重放：全被去重，零追加。
        assert_eq!(enrich_append_missing(dir.path(), &anchor, &turns), 0);
        // 增量：只追加新的一条。
        let turns_new = vec![
            turns[0].clone(),
            AgLiveTurn {
                ts_ms: 1_700_000_030_000,
                text: "第三条回复".to_string(),
            },
        ];
        assert_eq!(enrich_append_missing(dir.path(), &anchor, &turns_new), 1);

        let path =
            HookEventJournal::journal_path_for(dir.path(), AgentId::Antigravity, "conv-enrich");
        let content = fs::read_to_string(path).unwrap();
        let assistant_lines = content
            .lines()
            .filter(|line| line.contains("\"assistant_message\""))
            .count();
        assert_eq!(assistant_lines, 3);
        // 追加顺序 = ts 顺序（Chat 的消息序即文件序）。
        assert!(content.find("第一条回复").unwrap() < content.find("第三条回复").unwrap());
    }

    #[test]
    fn enrichment_dedupe_ignores_lifecycle_records() {
        // 生命周期事件与回填记录同 ts 同文本也不互相去重：
        // kind 域不同，语义上就不是同一条消息。
        let dir = tempfile::tempdir().unwrap();
        let anchor = anchor("conv-kind");
        let lifecycle = NormalizedHookEvent {
            kind: HookEventKind::TurnComplete,
            text: "老杨，ok".to_string(),
            ts_ms: 1_700_000_010_000,
            ..anchor.clone()
        };
        HookEventJournal::append(dir.path(), &lifecycle).unwrap();
        let turns = vec![AgLiveTurn {
            ts_ms: 1_700_000_010_000,
            text: "老杨，ok".to_string(),
        }];
        assert_eq!(enrich_append_missing(dir.path(), &anchor, &turns), 1);
    }
}
