//! AccountUsage / UsageWindow：本地 provider 会话文件的当日 token 聚合与
//! 可证明的用量窗口（v1：聚合先行，窗口仅在 provider 本地状态可证时填充）。
//!
//! [INPUT]: 依赖 shardlane-history 的 adapter roster（list_session_files /
//!          quick_meta / parse_session —— 唯一解析来源，禁止第二套解析）、
//!          chrono 的本地日换算、serde_json 的 JSON 快照缓存。
//! [OUTPUT]: 对外提供 UsageWindow、AccountUsage、UsageSnapshot（徽标事实段）、
//!           UsageAggregator（JSON 快照缓存 + mtime 失效 + 1 分钟节流）、
//!           mask_account_label、format_tokens_compact；agent_insight 的
//!           with_allowance 填充路径由此供数。
//! [POS]: shardlane-host 事实层的聚合器，与 attention 同层；展示层只读
//!        快照文本、零 I/O。单账号维度是 provider 本地可证的账号身份，
//!        取不到即 None——Unknown stays None; never guessed。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{Local, NaiveDate, TimeZone};
use serde::{Deserialize, Serialize};
use shardlane_history::models::SessionFileRef;
use shardlane_history::AgentHistoryAdapter;
use shardlane_history::{create_adapters, AgentId};

// ----------------------------------------------------------------------------
// 事实模型
// ----------------------------------------------------------------------------

/// provider 报告的限额窗口。label 是 provider 原文（如 "5h" / "7d"），
/// 永不本地编造。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageWindow {
    pub label: String,
    pub used_percentage: u8,
    /// epoch 秒；provider 未给即 None。
    pub resets_at: Option<i64>,
}

/// 一个 (provider, 账号, 本地日) 聚合行。tokens 是 provider 会话文件的
/// 累计事实（shardlane-history 已解析），按会话最后活动的本地日记账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountUsage {
    pub provider: AgentId,
    /// 可证明的本地账号身份（如 Codex auth.json 的 tokens.account_id）；
    /// 证明不了即 None，绝不猜测。
    pub account_id: Option<String>,
    /// 展示层打码标签；原始账号 id 永不进入日志或 UI。
    pub label_masked: String,
    /// 聚合归属的本地日。
    pub day: NaiveDate,
    /// provider 报告的当日会话 token 总量（>0 才成行）。
    pub tokens_used: i64,
    /// 空 = provider 本地状态未暴露窗口事实（不发明）。
    pub windows: Vec<UsageWindow>,
}

/// 一次聚合的完整快照：徽标文本的唯一事实源。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsageSnapshot {
    pub day: NaiveDate,
    pub accounts: Vec<AccountUsage>,
    pub generated_at_ms: u64,
    /// 失效信号：来源文件的最大 mtime 与数量；两者都不变则跳过重算。
    pub max_mtime_ms: Option<i64>,
    pub source_files: u64,
}

impl UsageSnapshot {
    /// 当日 provider 报告的 token 总量。
    pub fn tokens_today(&self) -> i64 {
        self.accounts
            .iter()
            .map(|account| account.tokens_used)
            .sum()
    }

    /// 窗口徽标事实：首个带窗口的账号行（v1 单账号现实）。
    pub fn badge_windows(&self) -> Vec<UsageWindow> {
        self.accounts
            .iter()
            .find(|account| !account.windows.is_empty())
            .map(|account| account.windows.clone())
            .unwrap_or_default()
    }

    /// 仅聚合时的徽标事实："3.8M" 形态的当日 token 压缩串。
    pub fn badge_tokens_compact(&self) -> Option<String> {
        let tokens = self.tokens_today();
        (tokens > 0).then(|| format_tokens_compact(tokens))
    }
}

// ----------------------------------------------------------------------------
// 纯展示函数
// ----------------------------------------------------------------------------

/// 账号展示标签打码：保留 provider 名 + 账号前 4 字符 + 省略号；
/// 无账号身份时退回 provider 展示名。原始账号 id 永不出现在返回值里。
pub fn mask_account_label(provider: AgentId, account_id: Option<&str>) -> String {
    match account_id.map(str::trim).filter(|id| !id.is_empty()) {
        Some(id) => {
            let keep = id.chars().take(4).collect::<String>();
            format!("{}:{}…", provider.as_str(), keep)
        }
        None => provider.display_name().to_string(),
    }
}

/// token 数压缩展示：999 → "999"，850_000 → "850K"，3_800_000 → "3.8M"。
pub fn format_tokens_compact(tokens: i64) -> String {
    if tokens < 1_000 {
        return tokens.to_string();
    }
    if tokens < 1_000_000 {
        return format!("{}K", tokens / 1_000);
    }
    let millions = tokens as f64 / 1_000_000.0;
    if millions >= 100.0 {
        format!("{:.0}M", millions)
    } else {
        format!("{:.1}M", millions)
    }
}

// ----------------------------------------------------------------------------
// provider 本地事实源
// ----------------------------------------------------------------------------

/// provider 用量窗口的本地可证来源。v1 实测（2026-09-19）Codex
/// state_5.sqlite 无任何 rate-limit 窗口表，可证来源出现前恒空——
/// Unknown stays None; never guessed。
fn provider_windows(_provider: AgentId) -> Vec<UsageWindow> {
    Vec::new()
}

fn codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(|home| Path::new(&home).join(".codex"))
                .unwrap_or_default()
        })
}

#[derive(Deserialize)]
struct CodexAuthFile {
    #[serde(default)]
    tokens: Option<CodexAuthTokens>,
}

#[derive(Deserialize)]
struct CodexAuthTokens {
    #[serde(default)]
    account_id: Option<String>,
}

/// Codex 的本地账号身份：auth.json 的 tokens.account_id（唯一反序列化
/// 字段，token 密钥永不读取/落日志）。
fn codex_local_account_id(home: &Path) -> Option<String> {
    let raw = fs::read_to_string(home.join("auth.json")).ok()?;
    let auth: CodexAuthFile = serde_json::from_str(&raw).ok()?;
    auth.tokens?
        .account_id
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
}

/// 生产账号身份解析：Codex 读 auth.json；其余 provider v1 无本地可证
/// 身份，返回 None。
fn default_account_of(provider: AgentId) -> Option<String> {
    match provider {
        AgentId::Codex => codex_local_account_id(&codex_home()),
        _ => None,
    }
}

/// epoch 毫秒 → 本地日；越界/无效时间返回 None（该会话不参与记账）。
fn local_day_of(ms: i64) -> Option<NaiveDate> {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|datetime| datetime.date_naive())
}

fn local_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

// ----------------------------------------------------------------------------
// 聚合
// ----------------------------------------------------------------------------

/// 单轮聚合的解析预算：无 quick_meta 的 provider 每轮至多完整解析这么多
/// 会话文件（有界 I/O 不变量），其余按 mtime 记入文件计数等待下轮。
const MAX_PARSE_PER_REFRESH: usize = 64;

fn aggregate_roster(
    adapters: &[Box<dyn AgentHistoryAdapter>],
    today: NaiveDate,
    day_of: &dyn Fn(i64) -> Option<NaiveDate>,
    account_of: &dyn Fn(AgentId) -> Option<String>,
    now_ms: u64,
) -> UsageSnapshot {
    // (provider, account) → 当日 token 累计。
    let mut tokens_by_account: BTreeMap<(String, String), i64> = BTreeMap::new();
    let mut max_mtime_ms: Option<i64> = None;
    let mut source_files: u64 = 0;
    let mut parse_budget = MAX_PARSE_PER_REFRESH;

    for adapter in adapters {
        if !adapter.detect() {
            continue;
        }
        let references: Vec<SessionFileRef> = match adapter.list_session_files() {
            Ok(references) => references,
            Err(_) => continue,
        };
        source_files += references.len() as u64;
        let quick = adapter.quick_meta(&references);
        for reference in &references {
            max_mtime_ms = Some(match max_mtime_ms {
                Some(current) => current.max(reference.mtime_ms),
                None => reference.mtime_ms,
            });
            // 廉价事实先行：quick_meta（如 Codex state db）给的
            // updated_at 判日，未命中的退回文件 mtime 判日。
            let quick_meta = quick
                .as_ref()
                .and_then(|table| table.get(reference.file_path.as_str()));
            let candidate_day = day_of(match quick_meta {
                Some(meta) => meta.updated_at,
                None => reference.mtime_ms,
            });
            if candidate_day != Some(today) {
                continue;
            }
            let meta = match quick_meta {
                Some(meta) => meta.clone(),
                None => {
                    if parse_budget == 0 {
                        continue;
                    }
                    parse_budget -= 1;
                    match adapter.parse_session(reference) {
                        Ok(parsed) => parsed.meta,
                        Err(_) => continue,
                    }
                }
            };
            // quick_meta 命中后仍以解析出的权威 updated_at 复核记账日。
            if day_of(meta.updated_at) != Some(today) {
                continue;
            }
            let tokens = meta.tokens_used.unwrap_or(0);
            if tokens <= 0 {
                continue;
            }
            let account = account_of(reference.agent)
                .map(|id| mask_account_label(reference.agent, Some(&id)))
                .unwrap_or_else(|| mask_account_label(reference.agent, None));
            *tokens_by_account
                .entry((reference.agent.as_str().to_string(), account))
                .or_insert(0) += tokens;
        }
    }

    let accounts = tokens_by_account
        .into_iter()
        .filter_map(|((provider_slug, account), tokens)| {
            let provider = AgentId::from_slug(&provider_slug)?;
            Some(AccountUsage {
                provider,
                account_id: account_of(provider),
                label_masked: account,
                day: today,
                tokens_used: tokens,
                windows: provider_windows(provider),
            })
        })
        .collect();

    UsageSnapshot {
        day: today,
        accounts,
        generated_at_ms: now_ms,
        max_mtime_ms,
        source_files,
    }
}

// ----------------------------------------------------------------------------
// JSON 快照缓存
// ----------------------------------------------------------------------------

const SNAPSHOT_FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct SnapshotDto {
    version: u32,
    day: String,
    generated_at_ms: u64,
    max_mtime_ms: Option<i64>,
    source_files: u64,
    accounts: Vec<AccountDto>,
}

#[derive(Serialize, Deserialize)]
struct AccountDto {
    provider: String,
    account_id: Option<String>,
    label_masked: String,
    day: String,
    tokens_used: i64,
    windows: Vec<UsageWindow>,
}

fn store_snapshot(path: &Path, snapshot: &UsageSnapshot) {
    // 缓存只是加速：任何失败都静默降级为"无缓存"，内存事实不受影响。
    let result = (|| -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let dto = SnapshotDto {
            version: SNAPSHOT_FORMAT_VERSION,
            day: snapshot.day.to_string(),
            generated_at_ms: snapshot.generated_at_ms,
            max_mtime_ms: snapshot.max_mtime_ms,
            source_files: snapshot.source_files,
            accounts: snapshot
                .accounts
                .iter()
                .map(|account| AccountDto {
                    provider: account.provider.as_str().to_string(),
                    account_id: account.account_id.clone(),
                    label_masked: account.label_masked.clone(),
                    day: account.day.to_string(),
                    tokens_used: account.tokens_used,
                    windows: account.windows.clone(),
                })
                .collect(),
        };
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string(&dto).unwrap_or_default())?;
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    let _ = result;
}

fn load_snapshot(path: &Path) -> Option<UsageSnapshot> {
    let raw = fs::read_to_string(path).ok()?;
    let dto: SnapshotDto = serde_json::from_str(&raw).ok()?;
    if dto.version != SNAPSHOT_FORMAT_VERSION {
        return None;
    }
    let day = NaiveDate::parse_from_str(&dto.day, "%Y-%m-%d").ok()?;
    let accounts = dto
        .accounts
        .into_iter()
        .filter_map(|account| {
            Some(AccountUsage {
                provider: AgentId::from_slug(&account.provider)?,
                account_id: account.account_id,
                label_masked: account.label_masked,
                day: NaiveDate::parse_from_str(&account.day, "%Y-%m-%d").ok()?,
                tokens_used: account.tokens_used,
                windows: account.windows,
            })
        })
        .collect();
    Some(UsageSnapshot {
        day,
        accounts,
        generated_at_ms: dto.generated_at_ms,
        max_mtime_ms: dto.max_mtime_ms,
        source_files: dto.source_files,
    })
}

/// 默认缓存位置：~/.shardlane/usage/snapshot.json（Shardlane 自有根，
/// 与 settings/hook-journal 同范式）；无 HOME 时退回 /tmp。
pub fn default_cache_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(|home| {
            Path::new(&home)
                .join(".shardlane")
                .join("usage")
                .join("snapshot.json")
        })
        .unwrap_or_else(|| PathBuf::from("/tmp/shardlane-usage-snapshot.json"))
}

// ----------------------------------------------------------------------------
// 聚合器
// ----------------------------------------------------------------------------

/// 重算节流：两次完整聚合至少间隔 60 秒（对齐 Moshi 节奏，全本地）。
pub const REFRESH_THROTTLE_MS: u64 = 60_000;

/// 节流决策（纯函数）：无缓存 / 跨日 → 必须刷新；60 秒内 → 拒绝。
fn should_refresh(
    now_ms: u64,
    last_refresh_ms: Option<u64>,
    cached_day: Option<NaiveDate>,
    today: NaiveDate,
) -> bool {
    let Some(cached_day) = cached_day else {
        return true;
    };
    if cached_day != today {
        return true;
    }
    let Some(last_refresh_ms) = last_refresh_ms else {
        return true;
    };
    now_ms.saturating_sub(last_refresh_ms) >= REFRESH_THROTTLE_MS
}

/// 当日 token 聚合器：JSON 快照缓存 + mtime/数量失效 + 1 分钟节流。
pub struct UsageAggregator {
    cache_path: PathBuf,
    adapters: Vec<Box<dyn AgentHistoryAdapter>>,
    cache_loaded: bool,
    cached: Option<UsageSnapshot>,
    last_refresh_ms: Option<u64>,
}

impl UsageAggregator {
    /// 生产构造：默认 adapter roster（roster 由调用方级配置统一管理）。
    pub fn new(cache_path: PathBuf) -> Self {
        Self::with_roster(cache_path, create_adapters())
    }

    /// 测试/定制构造：注入 roster。
    pub fn with_roster(cache_path: PathBuf, adapters: Vec<Box<dyn AgentHistoryAdapter>>) -> Self {
        Self {
            cache_path,
            adapters,
            cache_loaded: false,
            cached: None,
            last_refresh_ms: None,
        }
    }

    pub fn cached(&self) -> Option<&UsageSnapshot> {
        self.cached.as_ref()
    }

    /// 生产入口：内部取当前时刻与本地日。
    pub fn refresh_if_due(&mut self) -> Option<UsageSnapshot> {
        let now_ms = local_now_ms();
        let today = Local::now().date_naive();
        self.refresh_at(now_ms, today, &local_day_of, &default_account_of)
    }

    /// 可注入时钟的核心刷新（测试用）。返回值：可供展示的最新快照。
    pub fn refresh_at(
        &mut self,
        now_ms: u64,
        today: NaiveDate,
        day_of: &dyn Fn(i64) -> Option<NaiveDate>,
        account_of: &dyn Fn(AgentId) -> Option<String>,
    ) -> Option<UsageSnapshot> {
        if !self.cache_loaded {
            self.cache_loaded = true;
            if self.cached.is_none() {
                self.cached = load_snapshot(&self.cache_path);
                self.last_refresh_ms = self.cached.as_ref().map(|snap| snap.generated_at_ms);
            }
        }
        if !should_refresh(
            now_ms,
            self.last_refresh_ms,
            self.cached.as_ref().map(|snap| snap.day),
            today,
        ) {
            return self.cached.clone();
        }
        // 节流已过：先做廉价 listing。mtime 与文件数都未变且未跨日 →
        // 顺延节流窗口，直接回缓存。
        let sources_changed = self.sources_changed_since_cache();
        if !sources_changed {
            self.last_refresh_ms = Some(now_ms);
            return self.cached.clone();
        }
        let snapshot = aggregate_roster(&self.adapters, today, day_of, account_of, now_ms);
        self.cached = Some(snapshot.clone());
        self.last_refresh_ms = Some(now_ms);
        store_snapshot(&self.cache_path, &snapshot);
        Some(snapshot)
    }

    /// 廉价失效探针：重新 listing（无解析），比较 max mtime 与文件数。
    fn sources_changed_since_cache(&self) -> bool {
        let Some(cache) = self.cached.as_ref() else {
            return true;
        };
        let mut max_mtime_ms: Option<i64> = None;
        let mut source_files: u64 = 0;
        for adapter in &self.adapters {
            if !adapter.detect() {
                continue;
            }
            let Ok(references) = adapter.list_session_files() else {
                continue;
            };
            source_files += references.len() as u64;
            for reference in references {
                max_mtime_ms = Some(match max_mtime_ms {
                    Some(current) => current.max(reference.mtime_ms),
                    None => reference.mtime_ms,
                });
            }
        }
        cache.max_mtime_ms != max_mtime_ms || cache.source_files != source_files
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use shardlane_history::adapters::codex::CodexAdapter;

    fn fixed_offset_day_of(ms: i64) -> Option<NaiveDate> {
        // 固定 +08:00：与机器时区无关的确定性判日。
        let offset = chrono::FixedOffset::east_opt(8 * 3600)?;
        offset
            .timestamp_millis_opt(ms)
            .single()
            .map(|datetime| datetime.date_naive())
    }

    fn account_all_codex(provider: AgentId) -> Option<String> {
        (provider == AgentId::Codex).then(|| "11112222-3333-4444-5555-666677778888".to_string())
    }

    fn rollout_line(timestamp_ms: i64, tokens: i64) -> String {
        serde_json::json!({
            "timestamp": timestamp_ms,
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {"total_token_usage": {"total_tokens": tokens}}
            }
        })
        .to_string()
    }

    fn fixture_adapter(dir: &Path) -> Box<dyn AgentHistoryAdapter> {
        CodexAdapter::new().with_custom_root(dir.to_path_buf())
    }

    fn write_rollout(dir: &Path, name: &str, timestamp_ms: i64, tokens: i64) {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, rollout_line(timestamp_ms, tokens)).unwrap();
        // mtime 与内容时间戳对齐：聚合的 mtime 预过滤必须与机器真实
        // 时钟/时区无关，fixture 才是确定性的。
        let stamp = filetime::FileTime::from_unix_time(timestamp_ms / 1_000, 0);
        filetime::set_file_times(&path, stamp, stamp).unwrap();
    }

    #[test]
    fn mask_account_label_masks_and_falls_back() {
        assert_eq!(
            mask_account_label(AgentId::Codex, Some("11112222-3333-4444")),
            "codex:1111…"
        );
        assert_eq!(mask_account_label(AgentId::Codex, None), "Codex");
        assert_eq!(mask_account_label(AgentId::Codex, Some("  ")), "Codex");
        // 短 id 不越界。
        assert_eq!(mask_account_label(AgentId::Pi, Some("ab")), "pi:ab…");
    }

    #[test]
    fn format_tokens_compact_table() {
        assert_eq!(format_tokens_compact(0), "0");
        assert_eq!(format_tokens_compact(999), "999");
        assert_eq!(format_tokens_compact(1_000), "1K");
        assert_eq!(format_tokens_compact(850_000), "850K");
        assert_eq!(format_tokens_compact(999_999), "999K");
        assert_eq!(format_tokens_compact(3_800_000), "3.8M");
        assert_eq!(format_tokens_compact(123_000_000), "123M");
    }

    #[test]
    fn badge_prefers_windows_then_falls_back_to_tokens() {
        let day = NaiveDate::from_ymd_opt(2026, 9, 19).unwrap();
        let window = UsageWindow {
            label: "5h".into(),
            used_percentage: 51,
            resets_at: None,
        };
        let with_windows = UsageSnapshot {
            day,
            accounts: vec![AccountUsage {
                provider: AgentId::Codex,
                account_id: None,
                label_masked: "codex:1111…".into(),
                day,
                tokens_used: 3_800_000,
                windows: vec![window.clone()],
            }],
            ..UsageSnapshot::default()
        };
        assert_eq!(with_windows.badge_windows(), vec![window]);
        let tokens_only = UsageSnapshot {
            day,
            accounts: vec![AccountUsage {
                provider: AgentId::Codex,
                account_id: None,
                label_masked: "codex:1111…".into(),
                day,
                tokens_used: 3_800_000,
                windows: Vec::new(),
            }],
            ..UsageSnapshot::default()
        };
        // 窗口缺省路径：回退 token 聚合，不报错、不编窗口。
        assert!(tokens_only.badge_windows().is_empty());
        assert_eq!(tokens_only.badge_tokens_compact().as_deref(), Some("3.8M"));
        let empty = UsageSnapshot::default();
        assert!(empty.badge_tokens_compact().is_none());
    }

    #[test]
    fn aggregates_fixture_rollouts_by_local_day() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        // 今日（+08:00 的 2026-09-19 10:00 = 02:00Z）与昨日各一个会话。
        let today_ms = 1_758_240_000_000i64; // 2026-09-19T02:00:00Z
        let yesterday_ms = 1_758_153_600_000i64; // 2026-09-18T02:00:00Z
        write_rollout(
            root,
            "rollout-2026-09-19T10-00-00-aabbccdd-1111-2222-3333-444455556666.jsonl",
            today_ms,
            1_200,
        );
        write_rollout(
            root,
            "rollout-2026-09-18T10-00-00-aabbccdd-1111-2222-3333-444455556667.jsonl",
            yesterday_ms,
            9_999,
        );
        let cache = temp.path().join("cache").join("snapshot.json");
        let mut aggregator = UsageAggregator::with_roster(cache, vec![fixture_adapter(root)]);
        let today = fixed_offset_day_of(today_ms).unwrap();
        let snapshot = aggregator
            .refresh_at(1_000, today, &fixed_offset_day_of, &account_all_codex)
            .expect("snapshot must exist");
        assert_eq!(snapshot.accounts.len(), 1);
        let account = &snapshot.accounts[0];
        assert_eq!(account.provider, AgentId::Codex);
        // 多日分离：昨日会话不计入今日。
        assert_eq!(account.tokens_used, 1_200);
        assert_eq!(
            account.account_id.as_deref(),
            Some("11112222-3333-4444-5555-666677778888")
        );
        assert_eq!(account.label_masked, "codex:1111…");
        // 窗口缺省路径：无本地可证窗口事实 → 空表。
        assert!(account.windows.is_empty());
    }

    #[test]
    fn cache_roundtrip_serves_next_instance_without_recompute() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("sessions-root");
        let today_ms = 1_758_240_000_000i64;
        write_rollout(
            &root,
            "rollout-2026-09-19T10-00-00-aabbccdd-1111-2222-3333-444455556666.jsonl",
            today_ms,
            700,
        );
        let cache = temp.path().join("snapshot.json");
        // today 必须与 fixture 内容/mtime 的固定偏移判日一致。
        let today = fixed_offset_day_of(today_ms).unwrap();
        {
            let mut aggregator =
                UsageAggregator::with_roster(cache.clone(), vec![fixture_adapter(&root)]);
            let snapshot = aggregator
                .refresh_at(1_000, today, &fixed_offset_day_of, &account_all_codex)
                .unwrap();
            assert_eq!(snapshot.tokens_today(), 700);
        }
        // 新实例：磁盘缓存命中（同 mtime/数量），generated_at 不变。
        let mut second = UsageAggregator::with_roster(cache, vec![fixture_adapter(&root)]);
        let served = second
            .refresh_at(999_999, today, &fixed_offset_day_of, &account_all_codex)
            .unwrap();
        assert_eq!(served.generated_at_ms, 1_000);
        assert_eq!(served.tokens_today(), 700);
    }

    #[test]
    fn throttle_and_mtime_invalidation() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("sessions-root");
        let rollout =
            root.join("rollout-2026-09-19T10-00-00-aabbccdd-1111-2222-3333-444455556666.jsonl");
        let today_ms = 1_758_240_000_000i64;
        let later_ms = 1_758_240_600_000i64;
        let write_with_mtime = |tokens: i64, mtime_ms: i64| {
            fs::create_dir_all(&root).unwrap();
            fs::write(&rollout, rollout_line(1_758_240_000_000, tokens)).unwrap();
            let stamp = filetime::FileTime::from_unix_time(mtime_ms / 1_000, 0);
            filetime::set_file_times(&rollout, stamp, stamp).unwrap();
        };
        write_with_mtime(10, today_ms);
        let cache = temp.path().join("snapshot.json");
        let today = fixed_offset_day_of(today_ms).unwrap();
        let mut aggregator = UsageAggregator::with_roster(cache, vec![fixture_adapter(&root)]);
        let first = aggregator
            .refresh_at(1_000, today, &fixed_offset_day_of, &account_all_codex)
            .unwrap();
        assert_eq!(first.generated_at_ms, 1_000);
        // 节流窗口内：即使 mtime 变了也回缓存。
        write_with_mtime(20, later_ms);
        let within = aggregator
            .refresh_at(
                1_000 + 1_000,
                today,
                &fixed_offset_day_of,
                &account_all_codex,
            )
            .unwrap();
        assert_eq!(within.generated_at_ms, 1_000);
        // 节流已过 + mtime 回到已记录值 → 顺延节流，仍回缓存。
        write_with_mtime(20, today_ms);
        let deferred = aggregator
            .refresh_at(
                1_000 + REFRESH_THROTTLE_MS,
                today,
                &fixed_offset_day_of,
                &account_all_codex,
            )
            .unwrap();
        assert_eq!(deferred.generated_at_ms, 1_000);
        // 节流已过 + mtime 变化 → 重算。
        write_with_mtime(30, later_ms);
        let recomputed = aggregator
            .refresh_at(
                1_000 + 2 * REFRESH_THROTTLE_MS,
                today,
                &fixed_offset_day_of,
                &account_all_codex,
            )
            .unwrap();
        assert_eq!(recomputed.generated_at_ms, 1_000 + 2 * REFRESH_THROTTLE_MS);
        assert_eq!(recomputed.tokens_today(), 30);
    }

    #[test]
    fn should_refresh_truth_table() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 19).unwrap();
        let yesterday = NaiveDate::from_ymd_opt(2026, 9, 18).unwrap();
        // 无缓存 → 刷新。
        assert!(should_refresh(1_000, None, None, today));
        // 跨日 → 刷新（哪怕刚刷过）。
        assert!(should_refresh(1_000, Some(1_000), Some(yesterday), today));
        // 节流窗口内 → 拒绝。
        assert!(!should_refresh(
            1_000 + 1_000,
            Some(1_000),
            Some(today),
            today
        ));
        // 节流已过 → 刷新。
        assert!(should_refresh(
            1_000 + REFRESH_THROTTLE_MS,
            Some(1_000),
            Some(today),
            today
        ));
    }
}
