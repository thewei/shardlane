// SPDX-License-Identifier: MIT
// Portions Copyright (c) 2026 Corey Chiu; retained under the upstream MIT terms.

//! Antigravity live transcript extractor: agy's authoritative on-disk turn
//! bodies, read on demand by the Host hook adapter to enrich the Shardlane
//! hook journal with real assistant replies.
//!
//! [INPUT]: agy 会话库 `~/.gemini/antigravity-cli/conversations/<uuid>.db`
//! 的 `steps` 表（step_type=15 的 `step_payload` protobuf blob）；
//! `sqlite_ro` 三级只读阶梯。root 可注入以便测试。
//! [OUTPUT]: `AgLiveTurn`（ts_ms + 助手正式回复文本）与
//! `antigravity_live_turns[_in]` 提取器；给 shardlane-host 的
//! agent_hooks::adapter 做 journal enrichment。
//! [POS]: adapters 里 Antigravity 的 live 正文数据面（AntigravityAdapter
//! v2 的核心），与 `antigravity.rs` 的 catalog 元数据卡互补：那边只有
//! session 卡片，这里负责回合正文。载荷是 protobuf 线格式但无 schema
//! 权威，提取只走定点字段路径（.5.1.1 时间戳、.20.1 正式回复），
//! 解析失败一律降级为"该回合无正文"，绝不猜测。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use super::sqlite_ro::open_sqlite_ro;
use std::path::{Path, PathBuf};

/// One assistant turn body recovered from agy's own store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgLiveTurn {
    pub ts_ms: i64,
    pub text: String,
}

/// 单会话提取上限：防御异常会话把 ingest 线程拖垮（实测会话为百级）。
const MAX_TURNS: usize = 1000;
/// 单条文本上限：与 host 侧 journal 写入上限对齐（64K 字符）。
const MAX_TEXT_CHARS: usize = 64 * 1024;

/// Default agy conversations root: ~/.gemini/antigravity-cli/conversations.
pub fn default_conversations_root() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| {
        Path::new(&home)
            .join(".gemini")
            .join("antigravity-cli")
            .join("conversations")
    })
}

/// Extract assistant turn bodies for one conversation (default root).
pub fn antigravity_live_turns(session_id: &str) -> Vec<AgLiveTurn> {
    let Some(root) = default_conversations_root() else {
        return Vec::new();
    };
    antigravity_live_turns_in(&root, session_id)
}

/// Root-injectable extraction. Session id 是路径组件：只认 [A-Za-z0-9-]
/// （agy 的 uuid 形态），其余一律拒绝——fail-closed，不做 sanitize 折叠。
pub fn antigravity_live_turns_in(root: &Path, session_id: &str) -> Vec<AgLiveTurn> {
    if session_id.is_empty()
        || !session_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    {
        return Vec::new();
    }
    let db = root.join(format!("{session_id}.db"));
    let Some(read_only) = open_sqlite_ro(&db, "antigravity-live") else {
        return Vec::new();
    };
    let Ok(mut statement) = read_only
        .conn
        .prepare("SELECT idx, step_payload FROM steps WHERE step_type = 15 ORDER BY idx")
    else {
        return Vec::new();
    };
    let Ok(rows) = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0).unwrap_or_default(),
            row.get::<_, Vec<u8>>(1).unwrap_or_default(),
        ))
    }) else {
        return Vec::new();
    };
    let mut turns: Vec<AgLiveTurn> = rows
        .into_iter()
        .filter_map(|row| {
            let (_idx, payload) = row.ok()?;
            extract_turn(&payload)
        })
        .collect();
    // 窗口裁剪保留最新 MAX_TURNS（idx 末尾是新回合）：裁旧不裁新。
    // 若反过来保留最旧，超长会话的新回合会永远落在窗口外，增量
    // 回填在边界上卡死。
    let skip = turns.len().saturating_sub(MAX_TURNS);
    turns.drain(0..skip);
    // 回合库没有全局墙钟序（idx 即写入序，ts 即步内时刻）；这里以
    // ts 为主序、idx 天然兜底（stable sort 保持同 ts 的 idx 序），
    // 保证与用户感知顺序一致且排序稳定。
    turns.sort_by_key(|turn| turn.ts_ms);
    turns
}

/// 定点提取一个 step_payload：ts = 字段 5→1→{1=秒, 2=纳秒}，回复 =
/// 字段 20→1（正式回复文本）。正文链 fail-closed：缺失/畸形一律
/// None——缺正文比错正文好；时间戳链缺失降级 ts_ms=0（排序最前，
/// 不影响去重与追加）。
fn extract_turn(payload: &[u8]) -> Option<AgLiveTurn> {
    // 时间戳 = 字段 5（生成信息）→ 1（Timestamp{seconds, nanos}）。
    let ts_ms = sub_messages(payload, 5)
        .first()
        .and_then(|gen| sub_messages(gen, 1).first().copied())
        .map(|ts_message| {
            let seconds = sub_varints(ts_message, 1).first().copied().unwrap_or(0);
            let nanos = sub_varints(ts_message, 2).first().copied().unwrap_or(0);
            seconds
                .saturating_mul(1000)
                .saturating_add(nanos / 1_000_000)
        })
        .and_then(|ms| i64::try_from(ms).ok())
        .unwrap_or(0);
    let text = sub_messages(payload, 20).iter().find_map(|interaction| {
        sub_messages(interaction, 1)
            .iter()
            .find_map(|raw| recover_text(raw))
    })?;
    if text.is_empty() {
        return None;
    }
    Some(AgLiveTurn { ts_ms, text })
}

/// protobuf 线格式：收集顶层指定字段的 length-delimited 载荷。
/// 无 schema 权威，遇到未知 wire type 即停（截断安全）。
fn sub_messages(buf: &[u8], field: u32) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some((tag_field, wire, next)) = read_tag(buf, cursor) {
        cursor = next;
        match wire {
            0 => {
                if read_varint(buf, &mut cursor).is_none() {
                    break;
                }
            }
            1 => cursor += 8,
            2 => {
                let Some(len) = read_varint(buf, &mut cursor) else {
                    break;
                };
                let Ok(len) = usize::try_from(len) else {
                    break;
                };
                let Some(end) = cursor.checked_add(len) else {
                    break;
                };
                if buf.len() < end {
                    break;
                }
                if tag_field == field {
                    out.push(&buf[cursor..end]);
                }
                cursor = end;
            }
            5 => cursor += 4,
            _ => break,
        }
    }
    out
}

/// protobuf 线格式：收集顶层指定字段的 varint 值。
fn sub_varints(buf: &[u8], field: u32) -> Vec<u64> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some((tag_field, wire, next)) = read_tag(buf, cursor) {
        cursor = next;
        match wire {
            0 => {
                let Some(value) = read_varint(buf, &mut cursor) else {
                    break;
                };
                if tag_field == field {
                    out.push(value);
                }
            }
            1 => cursor += 8,
            2 => {
                let Some(len) = read_varint(buf, &mut cursor) else {
                    break;
                };
                let Ok(len) = usize::try_from(len) else {
                    break;
                };
                let Some(end) = cursor.checked_add(len) else {
                    break;
                };
                if buf.len() < end {
                    break;
                }
                cursor = end;
            }
            5 => cursor += 4,
            _ => break,
        }
    }
    out
}

/// 读一个 tag（field number + wire type），返回 (field, wire, next_cursor)。
fn read_tag(buf: &[u8], cursor: usize) -> Option<(u32, u64, usize)> {
    let mut at = cursor;
    let tag = read_varint(buf, &mut at)?;
    let field = u32::try_from(tag >> 3).ok()?;
    if field == 0 {
        return None;
    }
    Some((field, tag & 0b111, at))
}

fn read_varint(buf: &[u8], cursor: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    let mut shift = 0u32;
    loop {
        let byte = *buf.get(*cursor)?;
        *cursor += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}

/// 把 length-delimited 载荷还原为"人类正文"：UTF-8 合法、非空、
/// 可打印占比 > 0.9。ids/二进制框架在这里被自然淘汰。
fn recover_text(raw: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(raw).ok()?;
    if text.trim().is_empty() {
        return None;
    }
    // Rust 的 char 没有 is_printable；控制字符之外按可读文本对待，
    // 换行/制表属于可读空白，不算噪音。
    let readable = text
        .chars()
        .filter(|ch| !ch.is_control() || ch.is_whitespace())
        .count();
    if (readable as f64) < text.chars().count() as f64 * 0.9 {
        return None;
    }
    Some(text.chars().take(MAX_TEXT_CHARS).collect())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// 极简 protobuf 编码器（仅测试用）。
    fn len_delimited(field: u32, payload: &[u8]) -> Vec<u8> {
        let mut out = encode_varint((u64::from(field) << 3) | 2);
        out.extend(encode_varint(payload.len() as u64));
        out.extend_from_slice(payload);
        out
    }

    fn varint_field(field: u32, value: u64) -> Vec<u8> {
        let mut out = encode_varint(u64::from(field) << 3);
        out.extend(encode_varint(value));
        out
    }

    fn encode_varint(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                return out;
            }
            out.push(byte | 0x80);
        }
    }

    /// 完整 payload：field5{field1{ts}} + field20{field1{text}}。
    fn encode_step(ts_seconds: u64, reply: &str) -> Vec<u8> {
        let gen = len_delimited(1, &varint_field(1, ts_seconds));
        let interaction = len_delimited(1, reply.as_bytes());
        let mut payload = len_delimited(5, &gen);
        payload.extend(len_delimited(20, &interaction));
        payload
    }

    #[test]
    fn extracts_ts_and_reply_from_wellformed_payload() {
        let payload = encode_step(1_789_798_730, "老杨，ok");
        let turn = extract_turn(&payload).unwrap();
        assert_eq!(turn.ts_ms, 1_789_798_730_000);
        assert_eq!(turn.text, "老杨，ok");
    }

    #[test]
    fn malformed_payload_degrades_to_none() {
        // 空载荷：无 ts 无正文。
        assert!(extract_turn(&[]).is_none());
        // 正文缺失：只有 gen 信息。
        let gen_only = len_delimited(5, &varint_field(1, 42));
        assert!(extract_turn(&gen_only).is_none());
        // 正文是二进制框架：可打印率门淘汰。
        let binary = vec![0x00u8, 0x01, 0x02, 0xff, 0xfe];
        let payload = len_delimited(20, &len_delimited(1, &binary));
        assert!(extract_turn(&payload).is_none());
    }

    #[test]
    fn window_keeps_newest_turns_when_session_exceeds_cap() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("conv-cap.db");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE steps (idx integer PRIMARY KEY, step_type integer NOT NULL,
             step_payload blob);",
        )
        .unwrap();
        let total = (MAX_TURNS + 5) as i64;
        for idx in 1..=total {
            let payload = encode_step(2_000_000_000 + idx as u64, &format!("turn-{idx}"));
            conn.execute(
                "INSERT INTO steps (idx, step_type, step_payload) VALUES (?1, 15, ?2)",
                rusqlite::params![idx, payload],
            )
            .unwrap();
        }
        let turns = antigravity_live_turns_in(temp.path(), "conv-cap");
        assert_eq!(turns.len(), MAX_TURNS);
        let texts: std::collections::HashSet<&str> =
            turns.iter().map(|turn| turn.text.as_str()).collect();
        // 旧的被裁掉、新的全部保留：增量回填不会卡在窗口边界上。
        assert!(!texts.contains("turn-1"));
        assert!(texts.contains(&format!("turn-{total}").as_str()));
    }

    #[test]
    fn traversal_session_ids_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        assert!(antigravity_live_turns_in(temp.path(), "../escape").is_empty());
        assert!(antigravity_live_turns_in(temp.path(), "").is_empty());
        assert!(antigravity_live_turns_in(temp.path(), "ab/cd").is_empty());
        // 合法字符但库不存在：同样空集（而不是错误）。
        assert!(antigravity_live_turns_in(temp.path(), "9780aea4").is_empty());
    }

    #[test]
    fn turns_come_out_sorted_from_a_real_sqlite_db() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("conv-1.db");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE steps (idx integer PRIMARY KEY, step_type integer NOT NULL,
             step_payload blob);",
        )
        .unwrap();
        let later = encode_step(2_000_000_100, "第二条");
        let earlier = encode_step(2_000_000_050, "第一条");
        for (idx, payload) in [(1i64, vec![0u8]), (2, vec![0u8])] {
            conn.execute(
                "INSERT INTO steps (idx, step_type, step_payload) VALUES (?1, 14, ?2)",
                rusqlite::params![idx, payload],
            )
            .unwrap();
        }
        for (idx, payload) in [(3i64, later), (4, earlier)] {
            conn.execute(
                "INSERT INTO steps (idx, step_type, step_payload) VALUES (?1, 15, ?2)",
                rusqlite::params![idx, payload],
            )
            .unwrap();
        }
        let turns = antigravity_live_turns_in(temp.path(), "conv-1");
        assert_eq!(
            turns,
            vec![
                AgLiveTurn {
                    ts_ms: 2_000_000_050_000,
                    text: "第一条".to_string(),
                },
                AgLiveTurn {
                    ts_ms: 2_000_000_100_000,
                    text: "第二条".to_string(),
                },
            ]
        );
    }
}
