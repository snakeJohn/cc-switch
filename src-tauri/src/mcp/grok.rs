//! Grok MCP 同步模块
//!
//! 将统一 MCP 服务器配置同步到 `~/.grok/config.toml` 的 `[mcp_servers.<id>]`。
//! 概念形状与 Codex 一致；仅更新 `mcp_servers` 表，保留 `[ui]` 等其它段落。

use serde_json::Value;
use std::path::Path;

use crate::app_config::MultiAppConfig;
use crate::error::AppError;
use crate::grok_config;

use super::codex::json_server_to_toml_table;

fn should_sync_grok_mcp() -> bool {
    // Grok 未安装/未初始化时：~/.grok 目录不存在。
    // 与 Codex 一致：目录缺失时跳过写入/删除，不创建任何文件或目录。
    grok_config::get_grok_dir().exists()
}

/// 将单个 MCP 服务器同步到 Grok live 配置（`[mcp_servers.<id>]`）
pub fn sync_single_server_to_grok(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_grok_mcp() {
        return Ok(());
    }
    sync_single_server_to_grok_at(&grok_config::get_grok_config_path(), id, server_spec)
}

/// Path-parameterized upsert for tests and callers that already resolved the path.
pub fn sync_single_server_to_grok_at(
    config_path: &Path,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    use toml_edit::Item;

    let mut doc = if config_path.exists() {
        let content =
            std::fs::read_to_string(config_path).map_err(|e| AppError::io(config_path, e))?;
        // 解析失败必须报错而不是用空文档顶替：写回空文档会清空用户其它段落
        content.parse::<toml_edit::DocumentMut>().map_err(|e| {
            AppError::McpValidation(format!("解析 Grok config.toml 失败: {e}"))
        })?
    } else {
        toml_edit::DocumentMut::new()
    };

    if !doc.contains_key("mcp_servers") {
        doc["mcp_servers"] = toml_edit::table();
    }

    let toml_table = json_server_to_toml_table(server_spec)?;
    doc["mcp_servers"][id] = Item::Table(toml_table);

    let new_text = doc.to_string();
    crate::config::write_text_file(config_path, &new_text)?;
    Ok(())
}

/// 从 Grok live 配置中移除单个 MCP 服务器
pub fn remove_server_from_grok(id: &str) -> Result<(), AppError> {
    if !should_sync_grok_mcp() {
        return Ok(());
    }
    remove_server_from_grok_at(&grok_config::get_grok_config_path(), id)
}

/// Path-parameterized remove for tests and callers that already resolved the path.
pub fn remove_server_from_grok_at(config_path: &Path, id: &str) -> Result<(), AppError> {
    if !config_path.exists() {
        return Ok(());
    }

    let content =
        std::fs::read_to_string(config_path).map_err(|e| AppError::io(config_path, e))?;

    let mut doc = match content.parse::<toml_edit::DocumentMut>() {
        Ok(doc) => doc,
        Err(e) => {
            log::warn!("解析 Grok config.toml 失败: {e}，跳过删除操作");
            return Ok(());
        }
    };

    if let Some(mcp_servers) = doc.get_mut("mcp_servers").and_then(|s| s.as_table_mut()) {
        mcp_servers.remove(id);
    }

    let new_text = doc.to_string();
    crate::config::write_text_file(config_path, &new_text)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn write_initial(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, text).expect("write initial config");
    }

    #[test]
    fn upsert_empty_doc_writes_mcp_servers_section() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");

        let spec = json!({
            "type": "stdio",
            "command": "npx",
            "args": ["-y", "mcp-server-time"],
        });

        sync_single_server_to_grok_at(&path, "time", &spec).expect("upsert");

        let text = fs::read_to_string(&path).expect("read back");
        assert!(
            text.contains("[mcp_servers.time]"),
            "expected [mcp_servers.time], got:\n{text}"
        );
        assert!(text.contains("command = \"npx\""), "command missing:\n{text}");
        assert!(text.contains("type = \"stdio\""), "type missing:\n{text}");
    }

    #[test]
    fn remove_drops_server_preserves_ui_section() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");

        write_initial(
            &path,
            r#"[ui]
theme = "dark"
locale = "zh"

[mcp_servers.time]
type = "stdio"
command = "npx"
args = ["-y", "mcp-server-time"]
"#,
        );

        remove_server_from_grok_at(&path, "time").expect("remove");

        let text = fs::read_to_string(&path).expect("read back");
        assert!(
            !text.contains("[mcp_servers.time]"),
            "server section should be gone:\n{text}"
        );
        assert!(
            text.contains("[ui]"),
            "non-MCP [ui] section must be preserved:\n{text}"
        );
        assert!(
            text.contains("theme = \"dark\""),
            "ui theme must remain:\n{text}"
        );
    }

    #[test]
    fn upsert_preserves_existing_ui_section() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");

        write_initial(
            &path,
            r#"[ui]
theme = "light"

[models]
default = "grok-build"
"#,
        );

        let spec = json!({
            "type": "http",
            "url": "https://example.com/mcp",
            "headers": { "Authorization": "Bearer x" }
        });

        sync_single_server_to_grok_at(&path, "remote", &spec).expect("upsert");

        let text = fs::read_to_string(&path).expect("read back");
        assert!(
            text.contains("[mcp_servers.remote]"),
            "expected remote section:\n{text}"
        );
        assert!(text.contains("[ui]"), "ui preserved:\n{text}");
        assert!(text.contains("theme = \"light\""), "ui value:\n{text}");
        assert!(
            text.contains("default = \"grok-build\""),
            "models preserved:\n{text}"
        );
        assert!(
            text.contains("url = \"https://example.com/mcp\""),
            "url written:\n{text}"
        );
    }
}
