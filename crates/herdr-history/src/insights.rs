//! Plan 063: local read-only History statistics snapshot.
//!
//! `InsightsSnapshot` is an immutable statistics result computed once in the
//! background; the render path only reads it and never runs SQL on the UI
//! thread.
//! Token statistics only aggregate values explicitly reported by the provider;
//! when coverage is insufficient, comparisons are not fabricated.
//!
//! Excludes Activity / live agent status / telemetry / uploads.

use anyhow::Result;
use chrono::{DateTime, Datelike, Local, NaiveDate, Timelike, Utc};
use rusqlite::{params, Connection, OpenFlags};
use std::path::Path;

/// A complete History statistics snapshot (computed in the background, read
/// only by the UI thread).
#[derive(Clone, Debug, Default)]
pub struct InsightsSnapshot {
    /// Total number of non-archived sessions.
    pub session_count: u64,
    /// Total number of mainline user messages (prompt definition).
    pub prompt_count: u64,
    /// Number of sessions with token reporting.
    pub token_coverage_sessions: u64,
    /// Total valid tokens (only sessions within coverage).
    pub total_tokens: i64,
    /// Number of dates (local timezone) with at least 1 user message among
    /// non-archived sessions.
    pub active_days: u64,
    /// Current consecutive active-day streak (relative to the `as_of` date,
    /// counting backwards over consecutive prompt days).
    pub current_streak: u64,
    /// Longest consecutive active-day streak in history.
    pub longest_streak: u64,
    /// Prompt count of the busiest day (date not included; KPI only).
    pub busiest_day_prompts: u64,
    /// Daily prompt counts over the past 365 days (indexed by YYYY-MM-DD).
    pub daily_activity: Vec<DayActivity>,
    /// Hourly distribution (0–23).
    pub hourly: [u32; 24],
    /// Weekday distribution (0=Mon..6=Sun).
    pub weekday: [u32; 7],
    /// Monthly distribution (0=Jan..11=Dec).
    pub monthly: [u32; 12],
    /// Agent leaderboard (descending by session count).
    pub agent_leaderboard: Vec<AgentTally>,
    /// Project leaderboard (descending by session count).
    pub project_leaderboard: Vec<ProjectTally>,
    /// Model leaderboard (descending by session count; empty/unknown models
    /// excluded).
    pub model_leaderboard: Vec<ModelTally>,
    /// Base date of the snapshot (local timezone).
    pub as_of: NaiveDate,
}

#[derive(Clone, Debug)]
pub struct DayActivity {
    pub date: NaiveDate,
    pub prompt_count: u32,
}

impl DayActivity {
    /// Mon=0 .. Sun=6, computed without exposing chrono's private API.
    pub fn weekday_index(&self) -> usize {
        // 2001-01-01 is a Monday. Use days-since-epoch arithmetic:
        // NaiveDate::from_num_days_from_ce is public.
        // Days from CE of our anchor (Mon) = days_from_ce(2001-01-01)
        // We compute the anchor once here.
        const EPOCH: i32 = 730_485; // days from year 0001-01-01 to 2001-01-01
                                    // NaiveDate's internal: from_num_days_from_ce uses the same CE epoch.
                                    // However from_num_days_from_ce is public, from_num_days_from_ce(730_485) == 2001-01-01.
                                    // Use signed_duration_since with a known-Monday anchor date.
        if let Some(anchor) = NaiveDate::from_num_days_from_ce_opt(EPOCH) {
            let delta = self.date.signed_duration_since(anchor).num_days();
            ((delta % 7 + 7) % 7) as usize
        } else {
            0
        }
    }
}

#[derive(Clone, Debug)]
pub struct AgentTally {
    pub agent: String,
    pub sessions: u64,
    pub prompts: u64,
}

#[derive(Clone, Debug)]
pub struct ProjectTally {
    pub project_path: String,
    pub project_name: String,
    pub sessions: u64,
}

#[derive(Clone, Debug)]
pub struct ModelTally {
    pub model: String,
    pub sessions: u64,
}

/// Open a read-only connection to the catalog DB file and compute insights
/// (background use). `as_of` defaults to today (local timezone).
pub fn compute_insights(db_path: &Path) -> Result<InsightsSnapshot> {
    let as_of = Local::now().naive_local().date();
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    compute_insights_with_conn(&conn, as_of)
}

/// Compute using an existing read-only connection (test friendly).
pub fn compute_insights_with_conn(conn: &Connection, as_of: NaiveDate) -> Result<InsightsSnapshot> {
    let now_ts = as_of
        .and_hms_opt(23, 59, 59)
        .map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc).timestamp())
        .unwrap_or(i64::MAX);

    // --- sessions KPI ---
    let session_count: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sessions WHERE archived = 0",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0) as u64;

    let (token_coverage_sessions, total_tokens): (u64, i64) = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(tokens_used),0)
             FROM sessions WHERE archived = 0 AND tokens_used IS NOT NULL AND tokens_used > 0",
            [],
            |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)?)),
        )
        .unwrap_or((0, 0));

    // --- prompt count from message_fts ---
    // message_fts is a virtual table (FTS5) with a role column
    let prompt_count: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM message_fts WHERE role = 'user'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0) as u64;

    // --- daily activity (per local timezone) ---
    // timestamp is unix seconds (UTC)
    let mut stmt = conn.prepare(
        "SELECT timestamp FROM message_fts
         WHERE role = 'user' AND timestamp IS NOT NULL AND CAST(timestamp AS INTEGER) <= ?1",
    )?;
    let timestamps: Vec<i64> = stmt
        .query_map(params![now_ts], |r| r.get::<_, Option<String>>(0))?
        .filter_map(|r| {
            r.ok()
                .flatten()
                .and_then(|s| s.parse::<f64>().ok())
                .map(|f| f as i64)
        })
        .collect();

    let mut daily_map: std::collections::HashMap<NaiveDate, u32> = std::collections::HashMap::new();
    let mut hourly = [0u32; 24];
    let mut weekday = [0u32; 7];
    let mut monthly = [0u32; 12];

    for ts in &timestamps {
        let dt = DateTime::<Utc>::from_timestamp(*ts, 0)
            .map(|d| d.with_timezone(&Local))
            .filter(|d| d.date_naive() <= as_of);
        if let Some(dt) = dt {
            *daily_map.entry(dt.date_naive()).or_insert(0) += 1;
            hourly[dt.hour() as usize] = hourly[dt.hour() as usize].saturating_add(1);
            // chrono: weekday() Mon=0..Sun=6
            let wd = dt.weekday().num_days_from_monday() as usize;
            weekday[wd] = weekday[wd].saturating_add(1);
            let mo = dt.month0() as usize;
            monthly[mo] = monthly[mo].saturating_add(1);
        }
    }

    // 365 days rolling window
    let window_start = as_of
        .checked_sub_days(chrono::Days::new(364))
        .unwrap_or(as_of);
    let mut daily_activity: Vec<DayActivity> = {
        let mut d = window_start;
        let mut v = Vec::with_capacity(365);
        while d <= as_of {
            let count = daily_map.get(&d).copied().unwrap_or(0);
            v.push(DayActivity {
                date: d,
                prompt_count: count,
            });
            d = d.checked_add_days(chrono::Days::new(1)).unwrap_or(d);
            if d == window_start {
                break;
            }
        }
        v
    };
    daily_activity.sort_by_key(|a| a.date);

    let active_days = daily_map.len() as u64;

    // --- streak ---
    let current_streak = compute_current_streak(&daily_map, as_of);
    let longest_streak = compute_longest_streak(&daily_map);
    let busiest_day_prompts = daily_map.values().copied().max().unwrap_or(0) as u64;

    // --- leaderboards ---
    let agent_leaderboard = compute_agent_leaderboard(conn)?;
    let project_leaderboard = compute_project_leaderboard(conn)?;
    let model_leaderboard = compute_model_leaderboard(conn)?;

    Ok(InsightsSnapshot {
        session_count,
        prompt_count,
        token_coverage_sessions,
        total_tokens,
        active_days,
        current_streak,
        longest_streak,
        busiest_day_prompts,
        daily_activity,
        hourly,
        weekday,
        monthly,
        agent_leaderboard,
        project_leaderboard,
        model_leaderboard,
        as_of,
    })
}

fn compute_current_streak(
    daily_map: &std::collections::HashMap<NaiveDate, u32>,
    as_of: NaiveDate,
) -> u64 {
    // Count backwards from as_of; if that day has no prompt, start from
    // yesterday.
    let start = if daily_map.contains_key(&as_of) {
        as_of
    } else if let Some(prev) = as_of.checked_sub_days(chrono::Days::new(1)) {
        if daily_map.contains_key(&prev) {
            prev
        } else {
            return 0;
        }
    } else {
        return 0;
    };
    let mut streak = 0u64;
    let mut d = start;
    loop {
        if daily_map.contains_key(&d) {
            streak += 1;
            if let Some(prev) = d.checked_sub_days(chrono::Days::new(1)) {
                d = prev;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    streak
}

fn compute_longest_streak(daily_map: &std::collections::HashMap<NaiveDate, u32>) -> u64 {
    if daily_map.is_empty() {
        return 0;
    }
    let mut days: Vec<NaiveDate> = daily_map.keys().copied().collect();
    days.sort();
    let mut longest = 1u64;
    let mut current = 1u64;
    for i in 1..days.len() {
        let gap = days[i].signed_duration_since(days[i - 1]).num_days();
        if gap == 1 {
            current += 1;
            if current > longest {
                longest = current;
            }
        } else {
            current = 1;
        }
    }
    longest
}

fn compute_agent_leaderboard(conn: &Connection) -> Result<Vec<AgentTally>> {
    // One bounded GROUP BY: per-agent session counts joined with their real
    // user-prompt counts from the message index. Prompts are scoped to the
    // leaderboard's own non-archived session set, so they intentionally do
    // not sum to the global `prompt_count` (which includes archived
    // sessions). message_fts.role is UNINDEXED (FTS5), so this scan has the
    // same cost profile as the other message_fts aggregations above; it
    // stays bounded by the leaderboard LIMIT.
    let mut stmt = conn.prepare(
        "SELECT s.agent,
                COUNT(DISTINCT s.key) AS sessions,
                COUNT(f.session_key) AS prompts
         FROM sessions s
         LEFT JOIN message_fts f
           ON f.session_key = s.key AND f.role = 'user'
         WHERE s.archived = 0 AND s.agent IS NOT NULL AND s.agent != ''
         GROUP BY s.agent
         ORDER BY sessions DESC LIMIT 20",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(AgentTally {
            agent: r.get::<_, String>(0)?,
            sessions: r.get::<_, i64>(1)? as u64,
            prompts: r.get::<_, i64>(2)? as u64,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

fn compute_project_leaderboard(conn: &Connection) -> Result<Vec<ProjectTally>> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(project_path,''), COALESCE(project_name,''), COUNT(*) AS sessions
         FROM sessions
         WHERE archived = 0
         GROUP BY project_path ORDER BY sessions DESC LIMIT 20",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(ProjectTally {
            project_path: r.get::<_, String>(0)?,
            project_name: r.get::<_, String>(1)?,
            sessions: r.get::<_, i64>(2)? as u64,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

fn compute_model_leaderboard(conn: &Connection) -> Result<Vec<ModelTally>> {
    let mut stmt = conn.prepare(
        "SELECT model, COUNT(*) AS sessions FROM sessions
         WHERE archived = 0 AND model IS NOT NULL AND model != ''
         GROUP BY model ORDER BY sessions DESC LIMIT 20",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(ModelTally {
            model: r.get::<_, String>(0)?,
            sessions: r.get::<_, i64>(1)? as u64,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn make_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (
                key TEXT PRIMARY KEY, native_id TEXT, agent TEXT, title TEXT,
                project_path TEXT, project_name TEXT, file_path TEXT,
                created_at INTEGER, updated_at INTEGER, message_count INTEGER,
                size_bytes INTEGER, git_branch TEXT, model TEXT, tokens_used INTEGER,
                archived INTEGER DEFAULT 0, source TEXT
            );
            CREATE VIRTUAL TABLE message_fts USING fts5(
                session_key UNINDEXED, seq UNINDEXED, role UNINDEXED,
                timestamp UNINDEXED, text, tokenize='trigram'
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn empty_db_returns_zero_snapshot() {
        let conn = make_test_db();
        let as_of = NaiveDate::from_ymd_opt(2026, 8, 30).unwrap();
        let snap = compute_insights_with_conn(&conn, as_of).unwrap();
        assert_eq!(snap.session_count, 0);
        assert_eq!(snap.prompt_count, 0);
        assert_eq!(snap.active_days, 0);
        assert_eq!(snap.current_streak, 0);
    }

    #[test]
    fn prompt_count_excludes_non_user_roles() {
        let conn = make_test_db();
        conn.execute(
            "INSERT INTO sessions VALUES ('k1','','claude','T','/p','P','/f',1000,1000,3,100,NULL,'gpt',NULL,0,NULL)",
            [],
        ).unwrap();
        // ts = 2026-08-28 12:00 UTC
        let ts = 1724846400i64;
        conn.execute_batch(&format!(
            "INSERT INTO message_fts VALUES ('k1',1,'user','{ts}','hi');
             INSERT INTO message_fts VALUES ('k1',2,'assistant','{ts}','sure');
             INSERT INTO message_fts VALUES ('k1',3,'tool','{ts}','result');"
        ))
        .unwrap();
        let as_of = NaiveDate::from_ymd_opt(2026, 8, 30).unwrap();
        let snap = compute_insights_with_conn(&conn, as_of).unwrap();
        assert_eq!(snap.prompt_count, 1, "only user messages count as prompts");
    }

    #[test]
    fn streak_is_counted_correctly() {
        let conn = make_test_db();
        // 3 consecutive days ending on as_of
        let days = [
            NaiveDate::from_ymd_opt(2026, 8, 28).unwrap(),
            NaiveDate::from_ymd_opt(2026, 8, 29).unwrap(),
            NaiveDate::from_ymd_opt(2026, 8, 30).unwrap(),
        ];
        conn.execute("INSERT INTO sessions VALUES ('k1','','claude','T','/p','P','/f',1000,1000,3,100,NULL,NULL,NULL,0,NULL)", []).unwrap();
        for (i, day) in days.iter().enumerate() {
            let ts = day
                .and_hms_opt(12, 0, 0)
                .map(|d| DateTime::<Utc>::from_naive_utc_and_offset(d, Utc).timestamp())
                .unwrap_or(0);
            conn.execute(
                &format!("INSERT INTO message_fts VALUES ('k1',{i},'user','{ts}','prompt')"),
                [],
            )
            .unwrap();
        }
        let as_of = NaiveDate::from_ymd_opt(2026, 8, 30).unwrap();
        let snap = compute_insights_with_conn(&conn, as_of).unwrap();
        assert_eq!(snap.current_streak, 3);
        assert_eq!(snap.longest_streak, 3);
    }

    #[test]
    fn token_coverage_does_not_count_null_tokens() {
        let conn = make_test_db();
        conn.execute("INSERT INTO sessions VALUES ('k1','','claude','T','/p','P','/f',1000,1000,1,100,NULL,NULL,NULL,0,NULL)", []).unwrap();
        conn.execute("INSERT INTO sessions VALUES ('k2','','claude','T','/p','P','/f',1000,1000,1,100,NULL,NULL,5000,0,NULL)", []).unwrap();
        let as_of = NaiveDate::from_ymd_opt(2026, 8, 30).unwrap();
        let snap = compute_insights_with_conn(&conn, as_of).unwrap();
        assert_eq!(snap.token_coverage_sessions, 1);
        assert_eq!(snap.total_tokens, 5000);
    }

    #[test]
    fn agent_leaderboard_counts_real_user_prompts() {
        let conn = make_test_db();
        // k3 is archived: it stays out of the leaderboard tally entirely.
        conn.execute("INSERT INTO sessions VALUES ('k1','','claude','T','/p','P','/f',1000,1000,3,100,NULL,NULL,NULL,0,NULL)", []).unwrap();
        conn.execute("INSERT INTO sessions VALUES ('k2','','codex','T','/p','P','/f',1000,1000,2,100,NULL,NULL,NULL,0,NULL)", []).unwrap();
        conn.execute("INSERT INTO sessions VALUES ('k3','','claude','T','/p','P','/f',1000,1000,1,100,NULL,NULL,NULL,1,NULL)", []).unwrap();
        let ts = 1724846400i64;
        conn.execute_batch(&format!(
            "INSERT INTO message_fts VALUES ('k1',1,'user','{ts}','one');
             INSERT INTO message_fts VALUES ('k1',2,'assistant','{ts}','two');
             INSERT INTO message_fts VALUES ('k1',3,'user','{ts}','three');
             INSERT INTO message_fts VALUES ('k2',1,'user','{ts}','four');
             INSERT INTO message_fts VALUES ('k3',1,'user','{ts}','archived prompt');"
        ))
        .unwrap();
        let as_of = NaiveDate::from_ymd_opt(2026, 8, 30).unwrap();
        let snap = compute_insights_with_conn(&conn, as_of).unwrap();
        let claude = snap
            .agent_leaderboard
            .iter()
            .find(|tally| tally.agent == "claude")
            .unwrap_or_else(|| panic!("missing claude tally"));
        assert_eq!(
            claude.sessions, 1,
            "archived sessions stay out of the tally"
        );
        assert_eq!(
            claude.prompts, 2,
            "assistant messages never count as prompts"
        );
        let codex = snap
            .agent_leaderboard
            .iter()
            .find(|tally| tally.agent == "codex")
            .unwrap_or_else(|| panic!("missing codex tally"));
        assert_eq!(codex.sessions, 1);
        assert_eq!(codex.prompts, 1);
    }
}
