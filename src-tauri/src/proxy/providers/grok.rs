//! Grok Build provider adapter — pure passthrough over OpenAI-compatible or
//! Anthropic Messages wire formats selected by `meta.apiBackend`.
//!
//! Credentials and base URL live in provider `settings_config.meta` (and the
//! projected `config.toml` active model slot). No Chat↔Responses conversion.

use super::{AuthInfo, AuthStrategy, ProviderAdapter};
use crate::provider::Provider;
use crate::proxy::error::ProxyError;

/// Grok adapter (Bearer auth + meta baseUrl).
pub struct GrokAdapter;

impl GrokAdapter {
    pub fn new() -> Self {
        Self
    }

    fn meta_str(provider: &Provider, keys: &[&str]) -> Option<String> {
        let meta = provider.settings_config.get("meta")?;
        for key in keys {
            if let Some(value) = meta
                .get(*key)
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                return Some(value.to_string());
            }
        }
        None
    }

    fn extract_key(provider: &Provider) -> Option<String> {
        if let Some(key) = Self::meta_str(provider, &["apiKey", "api_key"]) {
            return Some(key);
        }

        if let Some(auth) = provider.settings_config.get("auth") {
            if let Some(key) = auth
                .get("OPENAI_API_KEY")
                .or_else(|| auth.get("api_key"))
                .or_else(|| auth.get("apiKey"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                return Some(key.to_string());
            }
        }

        if let Some(config) = provider.settings_config.get("config").and_then(|c| c.as_str()) {
            if let Ok(doc) = config.parse::<toml_edit::DocumentMut>() {
                if let Some(models) = doc.get("model").and_then(|m| m.as_table()) {
                    for (_, item) in models.iter() {
                        if let Some(table) = item.as_table() {
                            if let Some(key) = table
                                .get("api_key")
                                .and_then(|v| v.as_str())
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                            {
                                return Some(key.to_string());
                            }
                        }
                    }
                }
            }
        }

        None
    }
}

impl Default for GrokAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderAdapter for GrokAdapter {
    fn name(&self) -> &'static str {
        "Grok"
    }

    fn extract_base_url(&self, provider: &Provider) -> Result<String, ProxyError> {
        if let Some(url) = Self::meta_str(provider, &["baseUrl", "base_url"]) {
            return Ok(url.trim_end_matches('/').to_string());
        }

        if let Some(url) = provider
            .settings_config
            .get("base_url")
            .or_else(|| provider.settings_config.get("baseURL"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Ok(url.trim_end_matches('/').to_string());
        }

        if let Some(config) = provider.settings_config.get("config").and_then(|c| c.as_str()) {
            if let Ok(doc) = config.parse::<toml_edit::DocumentMut>() {
                if let Some(models) = doc.get("model").and_then(|m| m.as_table()) {
                    // Prefer managed slot.
                    if let Some(url) = models
                        .get(crate::grok_config::GROK_ACTIVE_MODEL_ID)
                        .and_then(|i| i.as_table())
                        .and_then(|t| t.get("base_url"))
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                    {
                        return Ok(url.trim_end_matches('/').to_string());
                    }
                    for (_, item) in models.iter() {
                        if let Some(url) = item
                            .as_table()
                            .and_then(|t| t.get("base_url"))
                            .and_then(|v| v.as_str())
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                        {
                            return Ok(url.trim_end_matches('/').to_string());
                        }
                    }
                }
            }
        }

        Err(ProxyError::ConfigError(
            "Grok Provider 缺少 base_url / meta.baseUrl 配置".to_string(),
        ))
    }

    fn extract_auth(&self, provider: &Provider) -> Option<AuthInfo> {
        Self::extract_key(provider).map(|key| AuthInfo::new(key, AuthStrategy::Bearer))
    }

    fn build_url(&self, base_url: &str, endpoint: &str) -> String {
        // Reuse Codex URL joining rules (origin vs /v1 vs custom prefix).
        super::codex::CodexAdapter::new().build_url(base_url, endpoint)
    }

    fn get_auth_headers(
        &self,
        auth: &AuthInfo,
    ) -> Result<Vec<(http::HeaderName, http::HeaderValue)>, ProxyError> {
        use super::adapter::auth_header_value;
        Ok(vec![(
            http::header::AUTHORIZATION,
            auth_header_value(&format!("Bearer {}", auth.api_key))?,
        )])
    }
}
