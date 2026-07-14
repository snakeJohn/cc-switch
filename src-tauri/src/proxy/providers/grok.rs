//! Grok Build provider adapter — pure passthrough over OpenAI-compatible or
//! Anthropic Messages wire formats selected by `meta.apiBackend`.
//!
//! Credentials and base URL live in provider `settings_config.meta` (and the
//! projected `config.toml` active model slot). No Chat↔Responses conversion.

use super::{AuthInfo, AuthStrategy, ProviderAdapter};
use crate::grok_config::GROK_PROXY_TOKEN_PLACEHOLDER;
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

    /// True when provider is configured for Anthropic Messages wire format.
    fn uses_messages_backend(provider: &Provider) -> bool {
        Self::meta_str(provider, &["apiBackend", "api_backend"])
            .map(|backend| backend.eq_ignore_ascii_case("messages"))
            .unwrap_or(false)
    }

    /// Accept only real upstream credentials — never the live-takeover placeholder.
    fn is_usable_key(key: &str) -> bool {
        let trimmed = key.trim();
        !trimmed.is_empty() && trimmed != GROK_PROXY_TOKEN_PLACEHOLDER
    }

    fn extract_key(provider: &Provider) -> Option<String> {
        if let Some(key) = Self::meta_str(provider, &["apiKey", "api_key"]) {
            if Self::is_usable_key(&key) {
                return Some(key);
            }
            // Contaminated meta: fall through to other credential sources.
        }

        if let Some(auth) = provider.settings_config.get("auth") {
            if let Some(key) = auth
                .get("OPENAI_API_KEY")
                .or_else(|| auth.get("api_key"))
                .or_else(|| auth.get("apiKey"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| Self::is_usable_key(s))
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
                                .filter(|s| Self::is_usable_key(s))
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
        // messages / Anthropic-compatible: x-api-key (anthropic-version filled by forwarder).
        // chat_completions / responses: pure Bearer.
        let strategy = if Self::uses_messages_backend(provider) {
            AuthStrategy::Anthropic
        } else {
            AuthStrategy::Bearer
        };
        Self::extract_key(provider).map(|key| AuthInfo::new(key, strategy))
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
        // Anthropic Messages gateway: only x-api-key (mutually exclusive with Bearer).
        // anthropic-version is injected by the forwarder when missing.
        if auth.strategy == AuthStrategy::Anthropic {
            return Ok(vec![(
                http::HeaderName::from_static("x-api-key"),
                auth_header_value(&auth.api_key)?,
            )]);
        }
        Ok(vec![(
            http::header::AUTHORIZATION,
            auth_header_value(&format!("Bearer {}", auth.api_key))?,
        )])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider_with_meta(meta: serde_json::Value) -> Provider {
        Provider {
            id: "test-grok".to_string(),
            name: "Test Grok".to_string(),
            settings_config: json!({ "meta": meta }),
            website_url: None,
            category: Some("grok".to_string()),
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    #[test]
    fn extract_key_rejects_proxy_managed_placeholder() {
        let provider = provider_with_meta(json!({
            "apiKey": "PROXY_MANAGED",
            "baseUrl": "https://api.x.ai",
            "apiBackend": "chat_completions",
        }));
        assert!(GrokAdapter::extract_key(&provider).is_none());
        assert!(GrokAdapter::new().extract_auth(&provider).is_none());
    }

    #[test]
    fn extract_key_accepts_real_api_key() {
        let provider = provider_with_meta(json!({
            "apiKey": "xai-real-key",
            "baseUrl": "https://api.x.ai",
            "apiBackend": "chat_completions",
        }));
        assert_eq!(
            GrokAdapter::extract_key(&provider).as_deref(),
            Some("xai-real-key")
        );
    }

    #[test]
    fn extract_auth_messages_uses_anthropic_x_api_key() {
        let adapter = GrokAdapter::new();
        let provider = provider_with_meta(json!({
            "apiKey": "sk-ant-test",
            "baseUrl": "https://gateway.example/v1",
            "apiBackend": "messages",
        }));
        let auth = adapter.extract_auth(&provider).expect("auth");
        assert_eq!(auth.strategy, AuthStrategy::Anthropic);
        let headers = adapter.get_auth_headers(&auth).unwrap();
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].0.as_str(), "x-api-key");
        assert_eq!(headers[0].1.to_str().unwrap(), "sk-ant-test");
    }

    #[test]
    fn extract_auth_chat_completions_uses_bearer() {
        let adapter = GrokAdapter::new();
        let provider = provider_with_meta(json!({
            "apiKey": "xai-key",
            "baseUrl": "https://api.x.ai",
            "apiBackend": "chat_completions",
        }));
        let auth = adapter.extract_auth(&provider).expect("auth");
        assert_eq!(auth.strategy, AuthStrategy::Bearer);
        let headers = adapter.get_auth_headers(&auth).unwrap();
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].0.as_str(), "authorization");
        assert_eq!(headers[0].1.to_str().unwrap(), "Bearer xai-key");
    }

    #[test]
    fn extract_auth_responses_uses_bearer() {
        let adapter = GrokAdapter::new();
        let provider = provider_with_meta(json!({
            "apiKey": "xai-key",
            "baseUrl": "https://api.x.ai",
            "apiBackend": "responses",
        }));
        let auth = adapter.extract_auth(&provider).expect("auth");
        assert_eq!(auth.strategy, AuthStrategy::Bearer);
    }
}
