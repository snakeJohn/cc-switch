//! Grok Build session scanner for Session Manager.
//!
//! Layout (see `~/.grok/docs/user-guide/17-sessions.md`):
//! ```text
//! ~/.grok/sessions/<url-encoded-cwd>/<session-id>/
//!   summary.json
//!   chat_history.jsonl
//!   updates.jsonl
//!   ...
//! ```

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::grok_config::get_grok_dir;
use crate::session_manager::{SessionMessage, SessionMeta};

use super::utils::{parse_timestamp_to_ms, truncate_summary, TITLE_MAX_CHARS};

const PROVIDER_ID: &str = "grok";

pub fn session_roots() -> Vec<PathBuf> {
    vec![get_grok_dir().join("sessions")]
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let roots = session_roots();
    let mut sessions = Vec::new();
    for root in roots {
        collect_sessions_under(&root, &mut sessions);
    }
    sessions
}

fn collect_sessions_under(root: &Path, out: &mut Vec<SessionMeta>) {
    if !root.is_dir() {
        return;
    }
    let Ok(cwd_entries) = std::fs::read_dir(root) else {
        return;
    };
    for cwd_entry in cwd_entries.flatten() {
        let cwd_path = cwd_entry.path();
        if !cwd_path.is_dir() {
            continue;
        }
        // Skip non-session siblings (e.g. session_search.sqlite lives under sessions/).
        let Ok(session_entries) = std::fs::read_dir(&cwd_path) else {
            continue;
        };
        for session_entry in session_entries.flatten() {
            let session_dir = session_entry.path();
            if !session_dir.is_dir() {
                continue;
            }
            let summary_path = session_dir.join("summary.json");
            if !summary_path.is_file() {
                continue;
            }
            if let Some(meta) = parse_session_dir(&session_dir, &summary_path) {
                out.push(meta);
            }
        }
    }
}

fn parse_session_dir(session_dir: &Path, summary_path: &Path) -> Option<SessionMeta> {
    let data = std::fs::read_to_string(summary_path).ok()?;
    let value: Value = serde_json::from_str(&data).ok()?;

    let session_id = value
        .pointer("/info/id")
        .and_then(Value::as_str)
        .or_else(|| value.get("id").and_then(Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            session_dir
                .file_name()
                .and_then(|n| n.to_str())
                .map(str::to_string)
        })?;

    let project_dir = value
        .pointer("/info/cwd")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            session_dir
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .and_then(urlencoding_decode)
        });

    let created_at = value
        .get("created_at")
        .and_then(parse_timestamp_to_ms)
        .or_else(|| value.get("createdAt").and_then(parse_timestamp_to_ms));
    let last_active_at = value
        .get("updated_at")
        .and_then(parse_timestamp_to_ms)
        .or_else(|| value.get("updatedAt").and_then(parse_timestamp_to_ms))
        .or(created_at);

    let title_from_summary = value
        .get("session_summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| truncate_summary(s, TITLE_MAX_CHARS));

    let title = title_from_summary.or_else(|| first_user_title(session_dir));

    let source_path = session_dir.to_string_lossy().to_string();

    Some(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id: session_id.clone(),
        title: title.clone(),
        summary: title,
        project_dir,
        created_at,
        last_active_at,
        source_path: Some(source_path),
        resume_command: Some(format!("grok --resume {session_id}")),
    })
}

fn first_user_title(session_dir: &Path) -> Option<String> {
    let history = session_dir.join("chat_history.jsonl");
    if !history.is_file() {
        return None;
    }
    let file = File::open(history).ok()?;
    let reader = BufReader::new(file);
    for line in reader.lines().take(40) {
        let Ok(line) = line else { continue };
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("user") {
            continue;
        }
        // Skip pure system-reminder synthetic user rows when possible.
        if value
            .get("synthetic_reason")
            .and_then(Value::as_str)
            .is_some()
        {
            continue;
        }
        let text = extract_message_text(value.get("content"));
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Prefer short user prompts; strip huge system-reminder blobs.
        if trimmed.starts_with("<system-reminder>") {
            continue;
        }
        return Some(truncate_summary(trimmed, TITLE_MAX_CHARS));
    }
    None
}

fn extract_message_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| {
                if let Some(s) = item.as_str() {
                    return Some(s.to_string());
                }
                item.get("text")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Some(Value::Object(map)) => map
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    }
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    // source_path is the session directory (not a single file).
    let history = if path.is_dir() {
        path.join("chat_history.jsonl")
    } else if path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n == "chat_history.jsonl")
    {
        path.to_path_buf()
    } else {
        path.with_file_name("chat_history.jsonl")
    };

    if !history.is_file() {
        return Ok(Vec::new());
    }

    let file =
        File::open(&history).map_err(|e| format!("Failed to open Grok chat history: {e}"))?;
    let reader = BufReader::new(file);
    let mut result = Vec::new();

    for line in reader.lines() {
        let Ok(line) = line else { continue };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        let role = match value.get("type").and_then(Value::as_str) {
            Some("user") => "user",
            Some("assistant") | Some("ai") | Some("model") => "assistant",
            Some("system") => "system",
            Some("tool") | Some("tool_result") => "tool",
            _ => continue,
        };

        // Skip synthetic system reminders injected as user messages.
        if role == "user"
            && value
                .get("synthetic_reason")
                .and_then(Value::as_str)
                .is_some()
        {
            continue;
        }

        let content = extract_message_text(value.get("content"));
        if content.trim().is_empty() {
            continue;
        }
        if role == "user" && content.trim_start().starts_with("<system-reminder>") {
            continue;
        }
        if role == "system" && content.len() > 4000 {
            // System prompts are huge; keep session view focused on dialogue.
            continue;
        }

        let ts = value
            .get("timestamp")
            .and_then(parse_timestamp_to_ms)
            .or_else(|| value.get("ts").and_then(parse_timestamp_to_ms));

        result.push(SessionMessage {
            role: role.to_string(),
            content,
            ts,
        });
    }

    Ok(result)
}

pub fn delete_session(_root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    let session_dir = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| format!("Invalid Grok session path: {}", path.display()))?
    };

    let summary = session_dir.join("summary.json");
    if summary.is_file() {
        if let Some(meta) = parse_session_dir(&session_dir, &summary) {
            if meta.session_id != session_id {
                return Err(format!(
                    "Grok session ID mismatch: expected {session_id}, found {}",
                    meta.session_id
                ));
            }
        }
    } else {
        let dir_name = session_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if dir_name != session_id {
            return Err(format!(
                "Grok session ID mismatch: expected {session_id}, found {dir_name}"
            ));
        }
    }

    std::fs::remove_dir_all(&session_dir).map_err(|e| {
        format!(
            "Failed to delete Grok session directory {}: {e}",
            session_dir.display()
        )
    })?;
    Ok(true)
}

/// Minimal percent-decoding for Grok's URL-encoded cwd folder names.
fn urlencoding_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let h = hex_nibble(bytes[i + 1])?;
                let l = hex_nibble(bytes[i + 2])?;
                out.push((h << 4) | l);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn scans_summary_and_loads_chat_history() {
        let tmp = tempdir().unwrap();
        let session_dir = tmp
            .path()
            .join("sessions")
            .join("C%3A%5Cproj")
            .join("abc-123");
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(
            session_dir.join("summary.json"),
            r#"{
              "info": { "id": "abc-123", "cwd": "C:\\proj" },
              "session_summary": "Hello world",
              "created_at": "2026-07-14T03:02:58Z",
              "updated_at": "2026-07-14T04:00:00Z"
            }"#,
        )
        .unwrap();
        std::fs::write(
            session_dir.join("chat_history.jsonl"),
            r#"{"type":"user","content":"hi there"}
{"type":"assistant","content":[{"type":"text","text":"hello"}]}
"#,
        )
        .unwrap();

        // Point get_grok_dir via temporary env is hard; call parse directly.
        let meta = parse_session_dir(&session_dir, &session_dir.join("summary.json")).unwrap();
        assert_eq!(meta.session_id, "abc-123");
        assert_eq!(meta.project_dir.as_deref(), Some("C:\\proj"));
        assert_eq!(meta.title.as_deref(), Some("Hello world"));
        assert_eq!(
            meta.resume_command.as_deref(),
            Some("grok --resume abc-123")
        );

        let messages = load_messages(&session_dir).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].content, "hello");
    }

    #[test]
    fn decodes_url_encoded_cwd_folder() {
        assert_eq!(
            urlencoding_decode("C%3A%5CUsers%5Cme").as_deref(),
            Some("C:\\Users\\me")
        );
    }
}
