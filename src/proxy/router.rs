use crate::config::AppConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Anthropic,
    OpenAI,
}

/// Determines the provider from the request path.
pub fn resolve_provider(path: &str) -> Option<Provider> {
    if path.starts_with("/v1/messages") {
        Some(Provider::Anthropic)
    } else if path.starts_with("/v1/chat/completions")
        || path.starts_with("/v1/embeddings")
        || path.starts_with("/v1/models")
    {
        Some(Provider::OpenAI)
    } else {
        None
    }
}

/// Builds the full upstream URL for the given provider.
///
/// Returns an error if the constructed URL has an invalid scheme (not http/https).
pub fn upstream_url(provider: Provider, path: &str, config: &AppConfig) -> Result<String, String> {
    let base = match provider {
        Provider::Anthropic => &config.anthropic_base_url,
        Provider::OpenAI => &config.openai_base_url,
    };

    let url = format!("{}{}", base.trim_end_matches('/'), path);

    // Light validation: confirm the scheme is http or https
    let scheme_end = url.find("://").ok_or_else(|| {
        format!("Invalid upstream URL '{}': missing scheme", url)
    })?;
    let scheme = &url[..scheme_end];
    if scheme != "http" && scheme != "https" {
        return Err(format!(
            "Invalid upstream URL '{}': scheme '{}' is not allowed",
            url, scheme
        ));
    }

    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_anthropic_messages() {
        assert_eq!(resolve_provider("/v1/messages"), Some(Provider::Anthropic));
    }

    #[test]
    fn test_resolve_anthropic_messages_with_suffix() {
        assert_eq!(
            resolve_provider("/v1/messages/count_tokens"),
            Some(Provider::Anthropic)
        );
    }

    #[test]
    fn test_resolve_openai_chat() {
        assert_eq!(
            resolve_provider("/v1/chat/completions"),
            Some(Provider::OpenAI)
        );
    }

    #[test]
    fn test_resolve_openai_embeddings() {
        assert_eq!(
            resolve_provider("/v1/embeddings"),
            Some(Provider::OpenAI)
        );
    }

    #[test]
    fn test_resolve_unknown() {
        assert_eq!(resolve_provider("/unknown/path"), None);
    }

    #[test]
    fn test_upstream_url_anthropic() {
        let config = AppConfig::default();
        let url = upstream_url(Provider::Anthropic, "/v1/messages", &config).unwrap();
        assert_eq!(url, "https://api.anthropic.com/v1/messages");
    }

    #[test]
    fn test_upstream_url_openai() {
        let config = AppConfig::default();
        let url = upstream_url(Provider::OpenAI, "/v1/chat/completions", &config).unwrap();
        assert_eq!(url, "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn test_upstream_url_trailing_slash() {
        let mut config = AppConfig::default();
        config.anthropic_base_url = "https://api.anthropic.com/".to_string();
        let url = upstream_url(Provider::Anthropic, "/v1/messages", &config).unwrap();
        assert_eq!(url, "https://api.anthropic.com/v1/messages");
    }

    #[test]
    fn test_upstream_url_invalid_scheme() {
        let mut config = AppConfig::default();
        config.anthropic_base_url = "file:///etc/passwd".to_string();
        let result = upstream_url(Provider::Anthropic, "/v1/messages", &config);
        assert!(result.is_err());
    }
}
