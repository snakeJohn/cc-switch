//! Grok Build 会话日志使用追踪
//!
//! 从 `~/.grok/sessions/<encoded-cwd>/<session-id>/` 提取 token 使用数据。
//!
//! ## 真实会话布局（2026-07 本机 `~/.grok` 观测）
//! ```text
//! ~/.grok/sessions/<url-encoded-cwd>/<session-id>/
//!   summary.json       # id / current_model_id / timestamps
//!   events.jsonl       # 生命周期事件（turn_started/first_token/tool_*…）— 无 usage 字段
//!   updates.jsonl      # ACP 流 + 高置信 turn 汇总（含 usage）
//!   signals.json       # 会话级聚合计数，无 per-turn 计费拆分
//!   chat_history.jsonl # 消息原文
//! ```
//!
//! ## 高置信 usage 字段路径（仅解析此类事件）
//! - 文件: `updates.jsonl`
//! - 条件: `params.update.sessionUpdate == "turn_completed"` 且 `params.update.usage` 为对象
//! - 字段:
//!   - `params.sessionId`（或目录名）→ session_id
//!   - `params.update.prompt_id` → 稳定 request 后缀
//!   - `params.update.usage.inputTokens`
//!   - `params.update.usage.outputTokens`
//!   - `params.update.usage.cachedReadTokens`
//!   - `params.update.usage.reasoningTokens`（并入 output，对齐 Gemini thoughts）
//!   - `params.update.usage.modelUsage` 的首个 key → model（缺省 `summary.json.current_model_id` / `"unknown"`）
//!   - 行顶层 `timestamp`（unix 秒）或 `_meta.agentTimestampMs`（毫秒）
//! - 缺关键字段 / 全零 token → skip
//! - `usageIsIncomplete: true` 仍导入（取消 turn 的部分用量有记录），全零则 skip
//!
//! ## 数据流
//! ```text
//! updates.jsonl turn_completed.usage → DedupKey → proxy_request_logs
//!   app_type=grok · data_source=grok_session
//! ```

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::grok_config::get_grok_dir;
use crate::proxy::usage::calculator::{CostCalculator, ModelPricing};
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    get_sync_state, metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use crate::services::usage_stats::{find_model_pricing, should_skip_session_insert, DedupKey};
use rust_decimal::Decimal;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const GROK_REQUEST_ID_PREFIX: &str = "grok_session";

/// 单次 turn_completed 的 token 数据
#[derive(Debug, Clone, PartialEq, Eq)]
struct GrokTurnUsage {
    session_id: String,
    prompt_id: String,
    model: String,
    input_tokens: u32,
    output_tokens: u32,
    cached_read_tokens: u32,
    reasoning_tokens: u32,
    /// unix 秒
    created_at: i64,
}

impl GrokTurnUsage {
    fn is_zero(&self) -> bool {
        self.input_tokens == 0
            && self.output_tokens == 0
            && self.cached_read_tokens == 0
            && self.reasoning_tokens == 0
    }

    fn billable_output(&self) -> u32 {
        self.output_tokens.saturating_add(self.reasoning_tokens)
    }
}

/// 同步 Grok 使用数据（从会话 updates.jsonl）
pub fn sync_grok_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let grok_dir = get_grok_dir();
    let files = collect_grok_update_files(&grok_dir);

    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: files.len() as u32,
        errors: vec![],
    };

    if files.is_empty() {
        return Ok(result);
    }

    for file_path in &files {
        match sync_single_grok_file(db, file_path) {
            Ok((imported, skipped)) => {
                result.imported += imported;
                result.skipped += skipped;
            }
            Err(e) => {
                let msg = format!("Grok 会话文件解析失败 {}: {e}", file_path.display());
                log::warn!("[GROK-SYNC] {msg}");
                result.errors.push(msg);
            }
        }
    }

    if result.imported > 0 {
        log::info!(
            "[GROK-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个文件",
            result.imported,
            result.skipped,
            result.files_scanned
        );
    }

    Ok(result)
}

/// 收集 `sessions/**/updates.jsonl`（限制深度，兼容 encoded-cwd / session-id 两层）
fn collect_grok_update_files(grok_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let sessions_dir = grok_dir.join("sessions");
    if !sessions_dir.is_dir() {
        return files;
    }
    collect_named_jsonl_recursive(&sessions_dir, "updates.jsonl", &mut files, 0, 4);
    files
}

fn collect_named_jsonl_recursive(
    dir: &Path,
    file_name: &str,
    files: &mut Vec<PathBuf>,
    depth: u32,
    max_depth: u32,
) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && depth < max_depth {
            collect_named_jsonl_recursive(&path, file_name, files, depth + 1, max_depth);
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case(file_name))
        {
            files.push(path);
        }
    }
}

fn sync_single_grok_file(db: &Database, file_path: &Path) -> Result<(u32, u32), AppError> {
    let file_path_str = file_path.to_string_lossy().to_string();

    let metadata = fs::metadata(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件元数据: {e}")))?;
    let file_modified = metadata_modified_nanos(&metadata);

    let (last_modified, last_offset) = get_sync_state(db, &file_path_str)?;
    if file_modified <= last_modified {
        return Ok((0, 0));
    }

    let fallback_model = read_summary_model(file_path.parent());
    let fallback_session_id = file_path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let file =
        fs::File::open(file_path).map_err(|e| AppError::Config(format!("无法打开文件: {e}")))?;
    let reader = BufReader::new(file);

    let mut line_offset: i64 = 0;
    let mut imported: u32 = 0;
    let mut skipped: u32 = 0;

    for line_result in reader.lines() {
        line_offset += 1;

        let line = match line_result {
            Ok(l) => l,
            Err(_) => continue,
        };

        if line.trim().is_empty() {
            continue;
        }

        // 快速过滤：turn_completed + usage
        if !line.contains("\"turn_completed\"") || !line.contains("\"usage\"") {
            continue;
        }

        // 已处理行仍需计入 offset，但可跳过解析后的插入
        let parse_for_insert = line_offset > last_offset;

        let value: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let Some(mut usage) = parse_turn_completed_usage(
            &value,
            &fallback_session_id,
            fallback_model.as_deref(),
        ) else {
            continue;
        };

        if usage.is_zero() {
            continue;
        }

        if !parse_for_insert {
            continue;
        }

        // 防御：prompt_id 空时用 eventId / 行号
        if usage.prompt_id.is_empty() || usage.prompt_id == "unknown" {
            usage.prompt_id = value
                .pointer("/params/update/_meta/eventId")
                .or_else(|| value.pointer("/_meta/eventId"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("line-{line_offset}"));
        }

        let request_id = format!(
            "{GROK_REQUEST_ID_PREFIX}:{}:{}",
            usage.session_id, usage.prompt_id
        );

        match insert_grok_session_entry(db, &request_id, &usage) {
            Ok(true) => imported += 1,
            Ok(false) => skipped += 1,
            Err(e) => {
                log::warn!("[GROK-SYNC] 插入失败 ({request_id}): {e}");
                skipped += 1;
            }
        }
    }

    update_sync_state(db, &file_path_str, file_modified, line_offset)?;
    Ok((imported, skipped))
}

fn read_summary_model(session_dir: Option<&Path>) -> Option<String> {
    let dir = session_dir?;
    let path = dir.join("summary.json");
    let content = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&content).ok()?;
    value
        .get("current_model_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// 解析 turn_completed usage 事件；低置信 / 缺字段返回 None。
fn parse_turn_completed_usage(
    value: &serde_json::Value,
    fallback_session_id: &str,
    fallback_model: Option<&str>,
) -> Option<GrokTurnUsage> {
    let params = value.get("params")?;
    let update = params.get("update")?;

    if update.get("sessionUpdate").and_then(|v| v.as_str()) != Some("turn_completed") {
        return None;
    }

    let usage = update.get("usage")?;
    if !usage.is_object() {
        return None;
    }

    let input_tokens = json_u32(usage.get("inputTokens")?)?;
    let output_tokens = json_u32(usage.get("outputTokens")?)?;
    // cache / reasoning 允许缺省为 0
    let cached_read_tokens = usage
        .get("cachedReadTokens")
        .and_then(json_u32)
        .unwrap_or(0);
    let reasoning_tokens = usage
        .get("reasoningTokens")
        .and_then(json_u32)
        .unwrap_or(0);

    let session_id = params
        .get("sessionId")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback_session_id)
        .to_string();

    let prompt_id = update
        .get("prompt_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("unknown")
        .to_string();

    let model = first_model_usage_key(usage)
        .or_else(|| fallback_model.map(|s| s.to_string()))
        .unwrap_or_else(|| "unknown".to_string());

    let created_at = parse_event_timestamp(value).unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });

    Some(GrokTurnUsage {
        session_id,
        prompt_id,
        model,
        input_tokens,
        output_tokens,
        cached_read_tokens,
        reasoning_tokens,
        created_at,
    })
}

fn first_model_usage_key(usage: &serde_json::Value) -> Option<String> {
    let model_usage = usage.get("modelUsage")?.as_object()?;
    model_usage.keys().next().map(|k| k.to_string())
}

fn json_u32(v: &serde_json::Value) -> Option<u32> {
    if let Some(n) = v.as_u64() {
        return Some(n.min(u32::MAX as u64) as u32);
    }
    if let Some(n) = v.as_i64() {
        if n < 0 {
            return None;
        }
        return Some((n as u64).min(u32::MAX as u64) as u32);
    }
    None
}

fn parse_event_timestamp(value: &serde_json::Value) -> Option<i64> {
    if let Some(secs) = value.get("timestamp").and_then(|v| v.as_i64()) {
        // Grok updates 使用 unix 秒；若像毫秒则缩小
        if secs > 10_000_000_000 {
            return Some(secs / 1000);
        }
        return Some(secs);
    }
    if let Some(ms) = value
        .pointer("/params/update/_meta/agentTimestampMs")
        .or_else(|| value.pointer("/_meta/agentTimestampMs"))
        .and_then(|v| v.as_i64())
    {
        return Some(ms / 1000);
    }
    None
}

fn insert_grok_session_entry(
    db: &Database,
    request_id: &str,
    usage: &GrokTurnUsage,
) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);

    let output_tokens = usage.billable_output();

    let dedup_key = DedupKey {
        app_type: "grok",
        model: &usage.model,
        input_tokens: usage.input_tokens,
        output_tokens,
        cache_read_tokens: usage.cached_read_tokens,
        cache_creation_tokens: 0,
        created_at: usage.created_at,
    };
    if should_skip_session_insert(&conn, request_id, &dedup_key)? {
        return Ok(false);
    }

    let token_usage = TokenUsage {
        input_tokens: usage.input_tokens,
        output_tokens,
        cache_read_tokens: usage.cached_read_tokens,
        cache_creation_tokens: 0,
        model: Some(usage.model.clone()),
        message_id: None,
    };

    let pricing = find_grok_pricing(&conn, &usage.model);
    let multiplier = Decimal::from(1);
    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) = match pricing {
        Some(p) => {
            let cost = CostCalculator::calculate_for_app("grok", &token_usage, &p, multiplier);
            (
                cost.input_cost.to_string(),
                cost.output_cost.to_string(),
                cost.cache_read_cost.to_string(),
                cost.cache_creation_cost.to_string(),
                cost.total_cost.to_string(),
            )
        }
        None => (
            "0".to_string(),
            "0".to_string(),
            "0".to_string(),
            "0".to_string(),
            "0".to_string(),
        ),
    };

    let inserted_rows = conn
        .execute(
            "INSERT OR IGNORE INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, error_message, session_id,
            provider_type, is_streaming, cost_multiplier, created_at, data_source
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)",
            rusqlite::params![
                request_id,
                "_grok_session",
                "grok",
                usage.model,
                usage.model,
                usage.input_tokens,
                output_tokens,
                usage.cached_read_tokens,
                0i64,
                input_cost,
                output_cost,
                cache_read_cost,
                cache_creation_cost,
                total_cost,
                0i64,
                Option::<i64>::None,
                200i64,
                Option::<String>::None,
                Some(usage.session_id.clone()),
                Some("grok_session"),
                1i64,
                "1.0",
                usage.created_at,
                "grok_session",
            ],
        )
        .map_err(|e| AppError::Database(format!("插入 Grok 会话日志失败: {e}")))?;

    if inserted_rows > 0 {
        crate::usage_events::notify_log_recorded();
    }

    Ok(inserted_rows > 0)
}

fn find_grok_pricing(conn: &rusqlite::Connection, model_id: &str) -> Option<ModelPricing> {
    find_model_pricing(conn, model_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample_turn_completed_line() -> String {
        serde_json::json!({
            "timestamp": 1784069416i64,
            "method": "_x.ai/session/update",
            "params": {
                "sessionId": "sess-abc",
                "update": {
                    "sessionUpdate": "turn_completed",
                    "prompt_id": "prompt-1",
                    "stop_reason": "end_turn",
                    "usage": {
                        "inputTokens": 245872,
                        "outputTokens": 2959,
                        "totalTokens": 248831,
                        "cachedReadTokens": 211968,
                        "reasoningTokens": 1416,
                        "modelCalls": 6,
                        "modelUsage": {
                            "grok-4.5": {
                                "inputTokens": 245872,
                                "outputTokens": 2959,
                                "cachedReadTokens": 211968,
                                "reasoningTokens": 1416
                            }
                        },
                        "numTurns": 6
                    },
                    "_meta": {
                        "eventId": "sess-abc-1878",
                        "agentTimestampMs": 1784069416331i64
                    }
                }
            }
        })
        .to_string()
    }

    #[test]
    fn test_parse_turn_completed_usage_high_confidence() {
        let value: serde_json::Value =
            serde_json::from_str(&sample_turn_completed_line()).unwrap();
        let usage = parse_turn_completed_usage(&value, "fallback", Some("fallback-model")).unwrap();
        assert_eq!(usage.session_id, "sess-abc");
        assert_eq!(usage.prompt_id, "prompt-1");
        assert_eq!(usage.model, "grok-4.5");
        assert_eq!(usage.input_tokens, 245872);
        assert_eq!(usage.output_tokens, 2959);
        assert_eq!(usage.cached_read_tokens, 211968);
        assert_eq!(usage.reasoning_tokens, 1416);
        assert_eq!(usage.billable_output(), 2959 + 1416);
        assert_eq!(usage.created_at, 1784069416);
    }

    #[test]
    fn test_parse_skips_missing_usage_fields() {
        let value = serde_json::json!({
            "timestamp": 1,
            "params": {
                "sessionId": "s",
                "update": {
                    "sessionUpdate": "turn_completed",
                    "prompt_id": "p",
                    "usage": { "inputTokens": 10 }
                }
            }
        });
        assert!(parse_turn_completed_usage(&value, "s", None).is_none());
    }

    #[test]
    fn test_parse_skips_non_turn_completed() {
        let value = serde_json::json!({
            "timestamp": 1,
            "params": {
                "sessionId": "s",
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "usage": {
                        "inputTokens": 10,
                        "outputTokens": 2
                    }
                }
            }
        });
        assert!(parse_turn_completed_usage(&value, "s", None).is_none());
    }

    #[test]
    fn test_collect_grok_update_files() {
        let temp = tempdir().unwrap();
        let sessions = temp.path().join("sessions").join("cwd").join("sid");
        fs::create_dir_all(&sessions).unwrap();
        fs::write(sessions.join("updates.jsonl"), "{}\n").unwrap();
        fs::write(sessions.join("events.jsonl"), "{}\n").unwrap();

        let files = collect_grok_update_files(temp.path());
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("updates.jsonl"));
    }

    #[test]
    fn test_collect_empty_when_no_sessions() {
        let files = collect_grok_update_files(Path::new("/nonexistent/path"));
        assert!(files.is_empty());
    }

    #[test]
    fn test_sync_imports_turn_completed() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let session = temp
            .path()
            .join("sessions")
            .join("cwd")
            .join("sess-abc");
        fs::create_dir_all(&session).unwrap();
        fs::write(
            session.join("summary.json"),
            r#"{"current_model_id":"grok-4.5"}"#,
        )
        .unwrap();
        fs::write(
            session.join("updates.jsonl"),
            format!(
                "{}\n{}\n",
                r#"{"timestamp":1,"method":"session/update","params":{"sessionId":"sess-abc","update":{"sessionUpdate":"user_message_chunk"}}}"#,
                sample_turn_completed_line()
            ),
        )
        .unwrap();

        let files = collect_grok_update_files(temp.path());
        assert_eq!(files.len(), 1);
        let (imported, skipped) = sync_single_grok_file(&db, &files[0])?;
        assert_eq!(imported, 1);
        assert_eq!(skipped, 0);

        // Drop the connection guard before the second sync (non-reentrant Mutex).
        {
            let conn = lock_conn!(db.conn);
            let row: (String, i64, i64, i64, String) = conn.query_row(
                "SELECT request_id, input_tokens, output_tokens, cache_read_tokens, data_source
                 FROM proxy_request_logs WHERE app_type = 'grok'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )?;
            assert_eq!(row.0, "grok_session:sess-abc:prompt-1");
            assert_eq!(row.1, 245872);
            assert_eq!(row.2, 2959 + 1416);
            assert_eq!(row.3, 211968);
            assert_eq!(row.4, "grok_session");
        }

        // 增量：文件未变则跳过
        let (imported2, skipped2) = sync_single_grok_file(&db, &files[0])?;
        assert_eq!((imported2, skipped2), (0, 0));

        Ok(())
    }

    #[test]
    fn test_insert_skips_matching_proxy_log() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    "grok-proxy",
                    "xai",
                    "grok",
                    "grok-4.5",
                    "grok-4.5",
                    100,
                    50,
                    10,
                    0,
                    "0.01",
                    100,
                    200,
                    1000,
                    "proxy"
                ],
            )?;
        }

        let usage = GrokTurnUsage {
            session_id: "s1".into(),
            prompt_id: "p1".into(),
            model: "grok-4.5".into(),
            input_tokens: 100,
            output_tokens: 50,
            cached_read_tokens: 10,
            reasoning_tokens: 0,
            created_at: 1000,
        };
        let inserted = insert_grok_session_entry(&db, "grok_session:s1:p1", &usage)?;
        assert!(!inserted);

        let conn = lock_conn!(db.conn);
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |r| {
            r.get(0)
        })?;
        assert_eq!(count, 1);
        Ok(())
    }
}
