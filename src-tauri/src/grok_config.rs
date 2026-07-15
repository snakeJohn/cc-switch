//! Grok Build live configuration (`~/.grok/config.toml`).
//!
//! Switch-style provider projection: only updates CCS-managed keys
//! (`models.default` + fixed `model.cc-switch-active` slot). User sections
//! such as `[ui]`, `[cli]`, non-managed `[model.*]`, and MCP are preserved.

use crate::config::{atomic_write, get_app_config_dir, get_home_dir};
use crate::error::AppError;
use crate::settings::{effective_backup_retain_count, get_grok_override_dir};
use chrono::Local;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::{value, DocumentMut, Item, Table};

/// Fixed third-party model slot id written into live config.
pub const GROK_ACTIVE_MODEL_ID: &str = "cc-switch-active";

/// Default built-in model id used for official providers when none is specified.
pub const GROK_OFFICIAL_DEFAULT_MODEL: &str = "grok-build";

// ============================================================================
// Path Functions
// ============================================================================

/// Resolve Grok config directory.
///
/// Priority:
/// 1. CCS settings `grok_config_dir` (explicit override)
/// 2. Default `~/.grok`
pub fn get_grok_dir() -> PathBuf {
    if let Some(override_dir) = get_grok_override_dir() {
        return override_dir;
    }
    get_home_dir().join(".grok")
}

/// `~/.grok/config.toml` (or override).
pub fn get_grok_config_path() -> PathBuf {
    get_grok_dir().join("config.toml")
}

/// `~/.grok/auth.json` (or override).
pub fn get_grok_auth_path() -> PathBuf {
    get_grok_dir().join("auth.json")
}

/// One non-secret account entry from `auth.json`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrokAuthAccount {
    /// Stable map key in `auth.json` (e.g. `https://auth.x.ai::<client_id>`).
    pub id: String,
    pub email: Option<String>,
    pub user_id: Option<String>,
    pub expires_at: Option<String>,
    /// True when this entry is treated as the active login (first valid entry).
    pub is_active: bool,
}

/// Lightweight official-auth status (no OAuth). Never includes tokens.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrokAuthStatus {
    /// True when `auth.json` exists and has at least one non-empty credential entry.
    pub authenticated: bool,
    /// Resolved path to `auth.json`.
    pub auth_path: String,
    /// Whether the auth.json file exists on disk.
    pub auth_file_exists: bool,
    /// Optional email from the active OIDC entry (if present).
    pub email: Option<String>,
    /// Optional expiry (`expires_at`) from the active entry.
    pub expires_at: Option<String>,
    /// Active account map key (if any).
    pub active_account_id: Option<String>,
    /// All accounts present in auth.json (tokens never included).
    pub accounts: Vec<GrokAuthAccount>,
    /// Hint for UI: run CLI login locally (no in-app OAuth).
    pub login_hint: String,
}

/// Read Grok official login status from `auth.json` without validating tokens remotely.
pub fn get_grok_auth_status() -> GrokAuthStatus {
    get_grok_auth_status_at(&get_grok_auth_path())
}

fn empty_auth_status(auth_path: String, auth_file_exists: bool) -> GrokAuthStatus {
    GrokAuthStatus {
        authenticated: false,
        auth_path,
        auth_file_exists,
        email: None,
        expires_at: None,
        active_account_id: None,
        accounts: Vec::new(),
        login_hint: "请运行 grok login".to_string(),
    }
}

fn entry_is_authenticated(entry: &Value) -> bool {
    let has_key = entry
        .get("key")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.is_empty());
    let has_refresh = entry
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.is_empty());
    has_key || has_refresh
}

fn parse_accounts(value: &Value) -> Vec<GrokAuthAccount> {
    let Some(map) = value.as_object() else {
        return Vec::new();
    };
    let mut accounts = Vec::new();
    let mut active_set = false;
    for (id, entry) in map {
        if !entry_is_authenticated(entry) {
            continue;
        }
        let is_active = !active_set;
        if is_active {
            active_set = true;
        }
        accounts.push(GrokAuthAccount {
            id: id.clone(),
            email: entry
                .get("email")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            user_id: entry
                .get("user_id")
                .or_else(|| entry.get("principal_id"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            expires_at: entry
                .get("expires_at")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            is_active,
        });
    }
    accounts
}

/// Testable path-based status reader.
pub fn get_grok_auth_status_at(auth_path: &Path) -> GrokAuthStatus {
    let path_str = auth_path.to_string_lossy().to_string();

    if !auth_path.is_file() {
        return empty_auth_status(path_str, false);
    }

    let content = match fs::read_to_string(auth_path) {
        Ok(c) => c,
        Err(_) => return empty_auth_status(path_str, true),
    };

    let value: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return empty_auth_status(path_str, true),
    };

    let accounts = parse_accounts(&value);
    let active = accounts.iter().find(|a| a.is_active);
    let authenticated = !accounts.is_empty();

    GrokAuthStatus {
        authenticated,
        auth_path: path_str,
        auth_file_exists: true,
        email: active.and_then(|a| a.email.clone()),
        expires_at: active.and_then(|a| a.expires_at.clone()),
        active_account_id: active.map(|a| a.id.clone()),
        accounts,
        login_hint: "请运行 grok login".to_string(),
    }
}

/// Promote an existing auth.json entry to be the active account (rewrites object
/// order so the selected key is first). Grok CLI typically uses the stored
/// session map; putting the preferred account first is the best-effort switch
/// without re-running OAuth in-app.
pub fn set_active_grok_account(account_id: &str) -> Result<GrokAuthStatus, AppError> {
    set_active_grok_account_at(&get_grok_auth_path(), account_id)
}

pub fn set_active_grok_account_at(
    auth_path: &Path,
    account_id: &str,
) -> Result<GrokAuthStatus, AppError> {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Err(AppError::InvalidInput(
            "Grok account id must not be empty".into(),
        ));
    }
    if !auth_path.is_file() {
        return Err(AppError::Config(format!(
            "auth.json not found: {}",
            auth_path.display()
        )));
    }
    let content = fs::read_to_string(auth_path)
        .map_err(|e| AppError::Config(format!("read auth.json failed: {e}")))?;
    let value: Value = serde_json::from_str(&content)
        .map_err(|e| AppError::Config(format!("parse auth.json failed: {e}")))?;
    let map = value
        .as_object()
        .ok_or_else(|| AppError::Config("auth.json root must be an object".into()))?;
    if !map.contains_key(account_id) {
        return Err(AppError::InvalidInput(format!(
            "Grok account not found: {account_id}"
        )));
    }

    let mut ordered = serde_json::Map::new();
    if let Some(entry) = map.get(account_id) {
        ordered.insert(account_id.to_string(), entry.clone());
    }
    for (k, v) in map {
        if k != account_id {
            ordered.insert(k.clone(), v.clone());
        }
    }
    let pretty = serde_json::to_string_pretty(&Value::Object(ordered))
        .map_err(|e| AppError::Config(format!("serialize auth.json failed: {e}")))?;
    atomic_write(auth_path, pretty.as_bytes())?;
    Ok(get_grok_auth_status_at(auth_path))
}

/// Remove one account entry from auth.json. If none remain, delete the file.
pub fn remove_grok_account(account_id: &str) -> Result<GrokAuthStatus, AppError> {
    remove_grok_account_at(&get_grok_auth_path(), account_id)
}

pub fn remove_grok_account_at(
    auth_path: &Path,
    account_id: &str,
) -> Result<GrokAuthStatus, AppError> {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Err(AppError::InvalidInput(
            "Grok account id must not be empty".into(),
        ));
    }
    if !auth_path.is_file() {
        return Ok(empty_auth_status(
            auth_path.to_string_lossy().to_string(),
            false,
        ));
    }
    let content = fs::read_to_string(auth_path)
        .map_err(|e| AppError::Config(format!("read auth.json failed: {e}")))?;
    let mut value: Value = serde_json::from_str(&content)
        .map_err(|e| AppError::Config(format!("parse auth.json failed: {e}")))?;
    let map = value
        .as_object_mut()
        .ok_or_else(|| AppError::Config("auth.json root must be an object".into()))?;
    map.remove(account_id);
    if map.is_empty() {
        let _ = fs::remove_file(auth_path);
        return Ok(empty_auth_status(
            auth_path.to_string_lossy().to_string(),
            false,
        ));
    }
    let pretty = serde_json::to_string_pretty(&value)
        .map_err(|e| AppError::Config(format!("serialize auth.json failed: {e}")))?;
    atomic_write(auth_path, pretty.as_bytes())?;
    Ok(get_grok_auth_status_at(auth_path))
}

/// Clear all Grok official credentials (`auth.json`).
pub fn logout_grok_accounts() -> Result<GrokAuthStatus, AppError> {
    logout_grok_accounts_at(&get_grok_auth_path())
}

pub fn logout_grok_accounts_at(auth_path: &Path) -> Result<GrokAuthStatus, AppError> {
    if auth_path.is_file() {
        fs::remove_file(auth_path)
            .map_err(|e| AppError::Config(format!("remove auth.json failed: {e}")))?;
    }
    Ok(empty_auth_status(
        auth_path.to_string_lossy().to_string(),
        false,
    ))
}

// ============================================================================
// Public write API
// ============================================================================

/// Project a provider `settings_config` onto the live Grok `config.toml`.
///
/// - Third-party: sets `models.default = "cc-switch-active"` and upserts
///   `[model.cc-switch-active]` with model / base_url / api_key / api_backend / name.
/// - Official: sets `models.default` to the configured built-in model (or
///   `grok-build`); never injects an api_key that would steal the OAuth session.
pub fn write_grok_provider_live(
    settings_config: &Value,
    is_official: bool,
) -> Result<(), AppError> {
    write_grok_provider_live_at(&get_grok_config_path(), settings_config, is_official)
}

/// Same as [`write_grok_provider_live`] but targets an explicit config path
/// (used by unit tests and callers that already resolved the path).
pub fn write_grok_provider_live_at(
    path: &Path,
    settings_config: &Value,
    is_official: bool,
) -> Result<(), AppError> {
    backup_grok_config_if_exists(path)?;

    let mut doc = read_or_empty_doc(path)?;
    apply_provider_to_doc(&mut doc, settings_config, is_official)?;

    let text = doc.to_string();
    atomic_write(path, text.as_bytes())?;
    Ok(())
}

/// Generate a third-party model TOML snippet for the active slot.
pub fn generate_third_party_model_toml(
    name: &str,
    model: &str,
    base_url: &str,
    api_key: &str,
    api_backend: &str,
) -> String {
    let mut doc = DocumentMut::new();
    doc["models"]["default"] = value(GROK_ACTIVE_MODEL_ID);

    let mut model_parent = Table::new();
    model_parent.set_implicit(true);
    model_parent[GROK_ACTIVE_MODEL_ID] =
        Item::Table(active_model_table(name, model, base_url, api_key, api_backend));
    doc["model"] = Item::Table(model_parent);

    doc.to_string()
}

// ============================================================================
// Pure merge (test-friendly)
// ============================================================================

/// Apply provider settings onto an in-memory TOML document.
///
/// Only touches `models.default` and `model.cc-switch-active` (third-party).
/// Leaves all other sections untouched.
pub fn apply_provider_to_doc(
    doc: &mut DocumentMut,
    settings: &Value,
    is_official: bool,
) -> Result<(), AppError> {
    if is_official {
        apply_official(doc, settings)
    } else {
        apply_third_party(doc, settings)
    }
}

fn apply_third_party(doc: &mut DocumentMut, settings: &Value) -> Result<(), AppError> {
    let fields = extract_third_party_fields(settings)?;

    ensure_table(doc, "models");
    doc["models"]["default"] = value(GROK_ACTIVE_MODEL_ID);

    ensure_model_parent(doc);
    let table = active_model_table(
        &fields.name,
        &fields.model,
        &fields.base_url,
        &fields.api_key,
        &fields.api_backend,
    );
    if let Some(parent) = doc["model"].as_table_mut() {
        parent[GROK_ACTIVE_MODEL_ID] = Item::Table(table);
    }

    Ok(())
}

fn apply_official(doc: &mut DocumentMut, settings: &Value) -> Result<(), AppError> {
    let default_model = extract_official_default(settings);

    ensure_table(doc, "models");
    doc["models"]["default"] = value(default_model.as_str());

    // Do not inject api_key into any built-in model table (session steal).
    // Leave existing [model.cc-switch-active] in place if present, but never
    // point models.default at it for official providers.
    Ok(())
}

// ============================================================================
// Field extraction
// ============================================================================

#[derive(Debug, Clone)]
struct ActiveModelFields {
    name: String,
    model: String,
    base_url: String,
    api_key: String,
    api_backend: String,
}

fn extract_third_party_fields(settings: &Value) -> Result<ActiveModelFields, AppError> {
    let meta = settings.get("meta");

    let from_meta = |key: &str| -> Option<String> {
        meta.and_then(|m| m.get(key))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    };

    // Prefer structured meta; fall back to parsing settings.config TOML.
    let mut name = from_meta("displayName").or_else(|| from_meta("name"));
    let mut model = from_meta("model");
    let mut base_url = from_meta("baseUrl").or_else(|| from_meta("base_url"));
    let mut api_key = from_meta("apiKey").or_else(|| from_meta("api_key"));
    let mut api_backend = from_meta("apiBackend").or_else(|| from_meta("api_backend"));

    if name.is_none()
        || model.is_none()
        || base_url.is_none()
        || api_key.is_none()
        || api_backend.is_none()
    {
        if let Some(from_toml) = fields_from_config_toml(settings) {
            name = name.or(Some(from_toml.name));
            model = model.or(Some(from_toml.model));
            base_url = base_url.or(Some(from_toml.base_url));
            api_key = api_key.or(Some(from_toml.api_key));
            api_backend = api_backend.or(Some(from_toml.api_backend));
        }
    }

    let model = model.ok_or_else(|| {
        AppError::Config(
            "Grok third-party provider missing model (meta.model or config TOML)".into(),
        )
    })?;
    let base_url = base_url.ok_or_else(|| {
        AppError::Config(
            "Grok third-party provider missing base_url (meta.baseUrl or config TOML)".into(),
        )
    })?;
    let api_key = api_key.unwrap_or_default();
    let api_backend = api_backend.unwrap_or_else(|| "chat_completions".to_string());
    let name = name.unwrap_or_else(|| model.clone());

    Ok(ActiveModelFields {
        name,
        model,
        base_url,
        api_key,
        api_backend,
    })
}

fn fields_from_config_toml(settings: &Value) -> Option<ActiveModelFields> {
    let config_text = settings.get("config")?.as_str()?.trim();
    if config_text.is_empty() {
        return None;
    }

    let doc = config_text.parse::<DocumentMut>().ok()?;

    // Prefer [model.cc-switch-active], then any single [model.*] table.
    let table = doc
        .get("model")
        .and_then(|m| m.as_table())
        .and_then(|models| {
            models
                .get(GROK_ACTIVE_MODEL_ID)
                .and_then(|item| item.as_table())
                .or_else(|| {
                    models
                        .iter()
                        .find_map(|(_, item)| item.as_table())
                })
        })?;

    let str_field = |key: &str| -> Option<String> {
        table
            .get(key)
            .and_then(|item| item.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    };

    let model = str_field("model")?;
    let base_url = str_field("base_url")?;
    let api_key = str_field("api_key").unwrap_or_default();
    let api_backend = str_field("api_backend").unwrap_or_else(|| "chat_completions".to_string());
    let name = str_field("name").unwrap_or_else(|| model.clone());

    Some(ActiveModelFields {
        name,
        model,
        base_url,
        api_key,
        api_backend,
    })
}

fn extract_official_default(settings: &Value) -> String {
    let meta = settings.get("meta");
    if let Some(model) = meta
        .and_then(|m| m.get("model"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return model.to_string();
    }

    if let Some(config_text) = settings.get("config").and_then(|c| c.as_str()) {
        if let Ok(doc) = config_text.parse::<DocumentMut>() {
            if let Some(default) = doc
                .get("models")
                .and_then(|m| m.get("default"))
                .and_then(|d| d.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty() && *s != GROK_ACTIVE_MODEL_ID)
            {
                return default.to_string();
            }
        }
    }

    GROK_OFFICIAL_DEFAULT_MODEL.to_string()
}

// ============================================================================
// TOML helpers
// ============================================================================

fn active_model_table(
    name: &str,
    model: &str,
    base_url: &str,
    api_key: &str,
    api_backend: &str,
) -> Table {
    let mut table = Table::new();
    table["name"] = value(name);
    table["model"] = value(model);
    table["base_url"] = value(base_url);
    table["api_key"] = value(api_key);
    table["api_backend"] = value(api_backend);
    table
}

fn ensure_table(doc: &mut DocumentMut, key: &str) {
    if !doc.contains_key(key) || !doc[key].is_table() {
        doc[key] = Item::Table(Table::new());
    }
}

fn ensure_model_parent(doc: &mut DocumentMut) {
    if !doc.contains_key("model") || !doc["model"].is_table() {
        let mut parent = Table::new();
        parent.set_implicit(true);
        doc["model"] = Item::Table(parent);
        return;
    }
    if let Some(parent) = doc["model"].as_table_mut() {
        parent.set_implicit(true);
    }
}

fn read_or_empty_doc(path: &Path) -> Result<DocumentMut, AppError> {
    if !path.exists() {
        return Ok(DocumentMut::new());
    }

    let content = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    if content.trim().is_empty() {
        return Ok(DocumentMut::new());
    }

    content.parse::<DocumentMut>().map_err(|e| {
        AppError::Config(format!(
            "Failed to parse Grok config.toml at {}: {e}",
            path.display()
        ))
    })
}

// ============================================================================
// Proxy takeover helpers (P3a)
// ============================================================================

/// Placeholder written into live config while proxy owns the real upstream key.
pub const GROK_PROXY_TOKEN_PLACEHOLDER: &str = "PROXY_MANAGED";

/// Read live `config.toml` as text (empty string when missing).
pub fn read_grok_config_text() -> Result<String, AppError> {
    read_grok_config_text_at(&get_grok_config_path())
}

pub fn read_grok_config_text_at(path: &Path) -> Result<String, AppError> {
    if !path.exists() {
        return Ok(String::new());
    }
    fs::read_to_string(path).map_err(|e| AppError::io(path, e))
}

/// Atomically write full `config.toml` text (preserves non-managed sections).
pub fn write_grok_config_text(text: &str) -> Result<(), AppError> {
    write_grok_config_text_at(&get_grok_config_path(), text)
}

pub fn write_grok_config_text_at(path: &Path, text: &str) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    atomic_write(path, text.as_bytes())
}

/// Backup JSON shape: `{ "toml": "<full config.toml>" }`.
pub fn live_to_backup_json() -> Result<Value, AppError> {
    let toml = read_grok_config_text()?;
    if toml.trim().is_empty() {
        return Err(AppError::Config("Grok config.toml 不存在或为空".into()));
    }
    Ok(serde_json::json!({ "toml": toml }))
}

/// Restore live config from backup JSON produced by [`live_to_backup_json`].
pub fn restore_from_backup_json(backup: &Value) -> Result<(), AppError> {
    let toml = backup
        .get("toml")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Config("Grok 备份缺少 toml 字段".into()))?;
    write_grok_config_text(toml)
}

/// Apply proxy takeover fields onto provider `settings_config` (mutates meta + config TOML).
///
/// Live projection always uses the third-party active slot so `base_url` /
/// `api_key` can point at the local proxy. Official providers are rejected by
/// the proxy service before calling this.
pub fn apply_takeover_fields(settings: &mut Value, proxy_base_url: &str) {
    let proxy_base_url = proxy_base_url.trim().trim_end_matches('/');

    {
        let Some(root) = settings.as_object_mut() else {
            return;
        };
        let meta_value = root
            .entry("meta".to_string())
            .or_insert_with(|| serde_json::json!({}));
        let Some(meta) = meta_value.as_object_mut() else {
            return;
        };
        meta.insert("baseUrl".into(), Value::String(proxy_base_url.to_string()));
        meta.insert(
            "apiKey".into(),
            Value::String(GROK_PROXY_TOKEN_PLACEHOLDER.to_string()),
        );
        meta.insert("isOfficial".into(), Value::Bool(false));
        if !meta.contains_key("apiBackend") && !meta.contains_key("api_backend") {
            meta.insert(
                "apiBackend".into(),
                Value::String("chat_completions".into()),
            );
        }
    }

    // Keep embedded config TOML in sync when present (used by some edit paths).
    let config_text = settings
        .get("config")
        .and_then(|c| c.as_str())
        .map(|s| s.to_string());
    if let Some(config_text) = config_text {
        if let Ok(mut doc) = config_text.parse::<DocumentMut>() {
            if apply_provider_to_doc(&mut doc, settings, false).is_ok() {
                if let Some(obj) = settings.as_object_mut() {
                    obj.insert("config".into(), Value::String(doc.to_string()));
                }
            }
        }
    }
}

/// Whether live config currently has the proxy placeholder key on the active slot.
pub fn is_live_taken_over() -> bool {
    match read_grok_config_text() {
        Ok(text) if !text.trim().is_empty() => is_toml_taken_over(&text),
        _ => false,
    }
}

pub fn is_toml_taken_over(toml_text: &str) -> bool {
    let Ok(doc) = toml_text.parse::<DocumentMut>() else {
        return false;
    };
    let Some(models) = doc.get("model").and_then(|m| m.as_table()) else {
        return false;
    };
    for (_, item) in models.iter() {
        if let Some(table) = item.as_table() {
            if table.get("api_key").and_then(|v| v.as_str()) == Some(GROK_PROXY_TOKEN_PLACEHOLDER)
            {
                return true;
            }
        }
    }
    false
}

/// True when active model base_url matches `expected_proxy_base_url`.
pub fn live_base_url_matches(expected_proxy_base_url: &str) -> bool {
    let Ok(text) = read_grok_config_text() else {
        return false;
    };
    let Ok(doc) = text.parse::<DocumentMut>() else {
        return false;
    };
    let expected = expected_proxy_base_url.trim().trim_end_matches('/');
    let Some(models) = doc.get("model").and_then(|m| m.as_table()) else {
        return false;
    };
    // Prefer managed slot, then any model with proxy placeholder.
    if let Some(active) = models
        .get(GROK_ACTIVE_MODEL_ID)
        .and_then(|i| i.as_table())
    {
        if let Some(url) = active.get("base_url").and_then(|v| v.as_str()) {
            return url.trim().trim_end_matches('/') == expected;
        }
    }
    for (_, item) in models.iter() {
        if let Some(table) = item.as_table() {
            if table.get("api_key").and_then(|v| v.as_str()) == Some(GROK_PROXY_TOKEN_PLACEHOLDER)
            {
                if let Some(url) = table.get("base_url").and_then(|v| v.as_str()) {
                    return url.trim().trim_end_matches('/') == expected;
                }
            }
        }
    }
    false
}

/// Remove proxy placeholder key and local proxy base_url from active model tables.
pub fn cleanup_takeover_placeholders_in_live() -> Result<(), AppError> {
    let path = get_grok_config_path();
    let text = read_grok_config_text_at(&path)?;
    if text.trim().is_empty() {
        return Ok(());
    }
    let mut doc: DocumentMut = text.parse().map_err(|e| {
        AppError::Config(format!(
            "Failed to parse Grok config.toml at {}: {e}",
            path.display()
        ))
    })?;

    let mut changed = false;
    if let Some(models) = doc.get_mut("model").and_then(|m| m.as_table_mut()) {
        let keys: Vec<String> = models.iter().map(|(k, _)| k.to_string()).collect();
        for key in keys {
            let Some(table) = models.get_mut(&key).and_then(|i| i.as_table_mut()) else {
                continue;
            };
            if table.get("api_key").and_then(|v| v.as_str()) == Some(GROK_PROXY_TOKEN_PLACEHOLDER)
            {
                table.remove("api_key");
                changed = true;
            }
            if table
                .get("base_url")
                .and_then(|v| v.as_str())
                .map(is_local_proxy_url)
                .unwrap_or(false)
            {
                table.remove("base_url");
                changed = true;
            }
        }
    }

    if changed {
        write_grok_config_text_at(&path, &doc.to_string())?;
    }
    Ok(())
}

fn is_local_proxy_url(url: &str) -> bool {
    let url = url.trim();
    if !url.starts_with("http://") {
        return false;
    }
    let rest = &url["http://".len()..];
    rest.starts_with("127.0.0.1")
        || rest.starts_with("localhost")
        || rest.starts_with("0.0.0.0")
        || rest.starts_with("[::1]")
        || rest.starts_with("[::]")
        || rest.starts_with("::1")
        || rest.starts_with("::")
}

// ============================================================================
// Backup
// ============================================================================

fn backup_grok_config_if_exists(path: &Path) -> Result<(), AppError> {
    if !path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    if content.trim().is_empty() {
        return Ok(());
    }

    let backup_dir = get_app_config_dir().join("backups").join("grok");
    fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;

    let base_id = format!("grok_{}", Local::now().format("%Y%m%d_%H%M%S"));
    let mut filename = format!("{base_id}.toml");
    let mut backup_path = backup_dir.join(&filename);
    let mut counter = 1;

    while backup_path.exists() {
        filename = format!("{base_id}_{counter}.toml");
        backup_path = backup_dir.join(&filename);
        counter += 1;
    }

    atomic_write(&backup_path, content.as_bytes())?;
    cleanup_grok_backups(&backup_dir)?;
    Ok(())
}

fn cleanup_grok_backups(dir: &Path) -> Result<(), AppError> {
    let retain = effective_backup_retain_count();
    let mut entries = fs::read_dir(dir)
        .map_err(|e| AppError::io(dir, e))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .map(|ext| ext == "toml")
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    if entries.len() <= retain {
        return Ok(());
    }

    entries.sort_by_key(|entry| entry.metadata().and_then(|m| m.modified()).ok());
    let remove_count = entries.len().saturating_sub(retain);
    for entry in entries.into_iter().take(remove_count) {
        if let Err(err) = fs::remove_file(entry.path()) {
            log::warn!(
                "Failed to remove old Grok config backup {}: {err}",
                entry.path().display()
            );
        }
    }

    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merge_preserves_ui_and_writes_active_model() {
        let existing = r#"[ui]
yolo = true

[models]
default = "old"

[cli]
theme = "dark"
"#;
        let mut doc: DocumentMut = existing.parse().expect("parse seed");

        let settings = json!({
            "auth": {},
            "meta": {
                "isOfficial": false,
                "apiBackend": "chat_completions",
                "model": "deepseek-chat",
                "baseUrl": "https://api.deepseek.com/v1",
                "apiKey": "sk-test-key",
                "displayName": "DeepSeek"
            }
        });

        apply_provider_to_doc(&mut doc, &settings, false).expect("apply third-party");

        let out = doc.to_string();
        assert!(
            out.contains("yolo = true") || out.contains("yolo=true"),
            "must preserve [ui] yolo; got:\n{out}"
        );
        assert!(
            out.contains("theme") && out.contains("dark"),
            "must preserve [cli]; got:\n{out}"
        );

        let default = doc["models"]["default"]
            .as_str()
            .expect("models.default should be a string");
        assert_eq!(default, GROK_ACTIVE_MODEL_ID);

        let active = doc["model"][GROK_ACTIVE_MODEL_ID]
            .as_table()
            .expect("[model.cc-switch-active] table");
        assert_eq!(active["base_url"].as_str(), Some("https://api.deepseek.com/v1"));
        assert_eq!(active["api_key"].as_str(), Some("sk-test-key"));
        assert_eq!(active["model"].as_str(), Some("deepseek-chat"));
        assert_eq!(active["api_backend"].as_str(), Some("chat_completions"));
        assert_eq!(active["name"].as_str(), Some("DeepSeek"));
    }

    #[test]
    fn merge_from_config_toml_fallback() {
        let mut doc = DocumentMut::new();
        let settings = json!({
            "auth": {},
            "config": r#"
[models]
default = "cc-switch-active"

[model.cc-switch-active]
name = "Custom"
model = "gpt-4o"
base_url = "https://example.com/v1"
api_key = "sk-from-toml"
api_backend = "responses"
"#
        });

        apply_provider_to_doc(&mut doc, &settings, false).expect("apply from config");

        let active = doc["model"][GROK_ACTIVE_MODEL_ID]
            .as_table()
            .expect("active model table");
        assert_eq!(active["api_key"].as_str(), Some("sk-from-toml"));
        assert_eq!(active["api_backend"].as_str(), Some("responses"));
        assert_eq!(active["base_url"].as_str(), Some("https://example.com/v1"));
        assert_eq!(
            doc["models"]["default"].as_str(),
            Some(GROK_ACTIVE_MODEL_ID)
        );
    }

    #[test]
    fn official_sets_default_without_api_key_injection() {
        let existing = r#"[ui]
yolo = true

[models]
default = "cc-switch-active"

[model.cc-switch-active]
name = "Third"
model = "x"
base_url = "https://x.example/v1"
api_key = "sk-leftover"
api_backend = "chat_completions"
"#;
        let mut doc: DocumentMut = existing.parse().expect("parse seed");

        let settings = json!({
            "auth": {},
            "meta": {
                "isOfficial": true,
                "model": "grok-build"
            }
        });

        apply_provider_to_doc(&mut doc, &settings, true).expect("apply official");

        let out = doc.to_string();
        assert!(
            out.contains("yolo = true") || out.contains("yolo=true"),
            "must preserve [ui]; got:\n{out}"
        );
        assert_eq!(doc["models"]["default"].as_str(), Some("grok-build"));

        // Official path must not invent a built-in model table with api_key.
        // Leftover third-party slot may remain, but must not be the default.
        if let Some(active) = doc
            .get("model")
            .and_then(|m| m.get(GROK_ACTIVE_MODEL_ID))
            .and_then(|i| i.as_table())
        {
            // If leftover slot exists, ensure we did not rewrite default to it.
            assert_ne!(
                doc["models"]["default"].as_str(),
                Some(GROK_ACTIVE_MODEL_ID)
            );
            // Presence of leftover api_key is OK; we simply must not add
            // api_key onto a new official model override table.
            let _ = active;
        }

        // No newly injected official model table with api_key.
        if let Some(model_tbl) = doc.get("model").and_then(|m| m.as_table()) {
            for (key, item) in model_tbl.iter() {
                if key == GROK_ACTIVE_MODEL_ID {
                    continue;
                }
                if let Some(t) = item.as_table() {
                    assert!(
                        t.get("api_key").is_none(),
                        "official path must not inject api_key into model.{key}"
                    );
                }
            }
        }
    }

    #[test]
    fn official_defaults_to_grok_build_when_meta_missing() {
        let mut doc = DocumentMut::new();
        let settings = json!({ "auth": {}, "meta": { "isOfficial": true } });
        apply_provider_to_doc(&mut doc, &settings, true).expect("apply official");
        assert_eq!(
            doc["models"]["default"].as_str(),
            Some(GROK_OFFICIAL_DEFAULT_MODEL)
        );
    }

    #[test]
    fn apply_takeover_fields_sets_proxy_base_and_placeholder() {
        let mut settings = json!({
            "auth": {},
            "meta": {
                "isOfficial": false,
                "apiBackend": "responses",
                "model": "deepseek-chat",
                "baseUrl": "https://api.deepseek.com/v1",
                "apiKey": "sk-real",
                "displayName": "DeepSeek"
            }
        });
        apply_takeover_fields(&mut settings, "http://127.0.0.1:15721/grok/v1");
        assert_eq!(
            settings["meta"]["baseUrl"].as_str(),
            Some("http://127.0.0.1:15721/grok/v1")
        );
        assert_eq!(
            settings["meta"]["apiKey"].as_str(),
            Some(GROK_PROXY_TOKEN_PLACEHOLDER)
        );
        assert_eq!(settings["meta"]["isOfficial"].as_bool(), Some(false));
    }

    #[test]
    fn is_toml_taken_over_detects_placeholder() {
        let toml = r#"
[model.cc-switch-active]
base_url = "http://127.0.0.1:15721/grok/v1"
api_key = "PROXY_MANAGED"
model = "x"
"#;
        assert!(is_toml_taken_over(toml));
        assert!(!is_toml_taken_over(
            r#"
[model.cc-switch-active]
api_key = "sk-real"
"#
        ));
    }

    #[test]
    fn write_grok_provider_live_at_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"[ui]
yolo = true

[models]
default = "old"
"#,
        )
        .expect("seed file");

        let settings = json!({
            "meta": {
                "model": "deepseek-chat",
                "baseUrl": "https://api.deepseek.com/v1",
                "apiKey": "sk-live",
                "displayName": "DeepSeek",
                "apiBackend": "chat_completions"
            }
        });

        write_grok_provider_live_at(&path, &settings, false).expect("write live");

        let text = fs::read_to_string(&path).expect("read back");
        assert!(text.contains("yolo"), "preserve ui: {text}");
        assert!(text.contains(GROK_ACTIVE_MODEL_ID), "active id: {text}");
        assert!(text.contains("sk-live"), "api_key written: {text}");
        assert!(
            text.contains("https://api.deepseek.com/v1"),
            "base_url written: {text}"
        );

        let doc: DocumentMut = text.parse().expect("parse written");
        assert_eq!(
            doc["models"]["default"].as_str(),
            Some(GROK_ACTIVE_MODEL_ID)
        );
    }

    #[test]
    fn generate_third_party_model_toml_contains_slot() {
        let toml = generate_third_party_model_toml(
            "DeepSeek",
            "deepseek-chat",
            "https://api.deepseek.com/v1",
            "sk-x",
            "chat_completions",
        );
        assert!(toml.contains(GROK_ACTIVE_MODEL_ID));
        assert!(toml.contains("deepseek-chat"));
        assert!(toml.contains("sk-x"));
        assert!(toml.contains("chat_completions"));
    }

    #[test]
    fn get_grok_paths_default_under_home_dot_grok() {
        // Without override these resolve under home; we only assert suffix shape
        // when override is unset (cannot mutate global settings safely here).
        let config = get_grok_config_path();
        assert!(
            config.ends_with("config.toml") || config.file_name().map(|n| n == "config.toml").unwrap_or(false)
        );
        let auth = get_grok_auth_path();
        assert!(
            auth.ends_with("auth.json") || auth.file_name().map(|n| n == "auth.json").unwrap_or(false)
        );
        assert_eq!(GROK_ACTIVE_MODEL_ID, "cc-switch-active");
    }

    #[test]
    fn auth_status_missing_file_is_unauthenticated() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("auth.json");
        let status = get_grok_auth_status_at(&path);
        assert!(!status.authenticated);
        assert!(!status.auth_file_exists);
        assert!(status.accounts.is_empty());
        assert_eq!(status.login_hint, "请运行 grok login");
    }

    #[test]
    fn auth_status_reads_email_without_exposing_tokens() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("auth.json");
        fs::write(
            &path,
            r#"{
              "https://auth.x.ai::client": {
                "key": "secret-access-token",
                "refresh_token": "secret-refresh",
                "email": "user@example.com",
                "expires_at": "2026-07-15T04:42:19Z"
              }
            }"#,
        )
        .unwrap();
        let status = get_grok_auth_status_at(&path);
        assert!(status.authenticated);
        assert!(status.auth_file_exists);
        assert_eq!(status.email.as_deref(), Some("user@example.com"));
        assert_eq!(status.expires_at.as_deref(), Some("2026-07-15T04:42:19Z"));
        assert_eq!(status.accounts.len(), 1);
        assert!(status.accounts[0].is_active);
        let json = serde_json::to_string(&status).unwrap();
        assert!(!json.contains("secret-access-token"));
        assert!(!json.contains("secret-refresh"));
    }

    #[test]
    fn set_active_reorders_accounts() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("auth.json");
        fs::write(
            &path,
            r#"{
              "a::1": { "key": "k1", "email": "one@x.ai" },
              "b::2": { "key": "k2", "email": "two@x.ai" }
            }"#,
        )
        .unwrap();
        let status = set_active_grok_account_at(&path, "b::2").unwrap();
        assert_eq!(status.active_account_id.as_deref(), Some("b::2"));
        assert_eq!(status.email.as_deref(), Some("two@x.ai"));
        assert_eq!(status.accounts[0].id, "b::2");
    }
}
