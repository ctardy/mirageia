use std::env;
use std::path::PathBuf;
use std::str::FromStr;

use serde::Deserialize;

/// Full MirageIA configuration.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub listen_addr: String,
    pub anthropic_base_url: String,
    pub openai_base_url: String,
    pub log_level: String,
    /// Terms that should never be pseudonymized (public names, technical terms, etc.).
    pub whitelist: Vec<String>,
    /// Minimum confidence threshold for regex detection (0.0-1.0).
    pub confidence_threshold: f32,
    /// Add the X-MirageIA: active header to requests.
    pub add_header: bool,
    /// Fail-open mode: if true, forwards the unmodified request on error.
    pub fail_open: bool,
    /// Passthrough mode: if true, the proxy relays without pseudonymizing.
    pub passthrough: bool,
    /// ONNX model name to use for contextual PII detection (e.g. "iiiorg/piiranha-v1-detect-personal-information").
    /// If None, falls back to the active model configured via `mirageia model use`.
    pub model_name: Option<String>,
    /// Optional bearer token required on all LLM proxy requests.
    /// If None, authentication is disabled.
    pub proxy_token: Option<String>,
}

/// Structure of the config.toml file (deserializable).
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct FileConfig {
    proxy: ProxyFileConfig,
    detection: DetectionFileConfig,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct ProxyFileConfig {
    listen_addr: Option<String>,
    anthropic_base_url: Option<String>,
    openai_base_url: Option<String>,
    log_level: Option<String>,
    add_header: Option<bool>,
    fail_open: Option<bool>,
    passthrough: Option<bool>,
    proxy_token: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct DetectionFileConfig {
    whitelist: Vec<String>,
    confidence_threshold: Option<f32>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            listen_addr: "127.0.0.1:3100".to_string(),
            anthropic_base_url: "https://api.anthropic.com".to_string(),
            openai_base_url: "https://api.openai.com".to_string(),
            log_level: "info".to_string(),
            whitelist: vec![
                "127.0.0.1".to_string(),
                "localhost".to_string(),
                "::1".to_string(),
            ],
            confidence_threshold: 0.75,
            add_header: false,
            fail_open: true,
            passthrough: false,
            model_name: None,
            proxy_token: None,
        }
    }
}

impl AppConfig {
    /// Loads the config: TOML file (if present) -> environment variables -> defaults.
    pub fn load() -> Self {
        let mut config = Self::default();

        // 1. Load the config.toml file if it exists
        if let Some(file_config) = Self::load_config_file() {
            if let Some(addr) = file_config.proxy.listen_addr {
                config.listen_addr = addr;
            }
            if let Some(url) = file_config.proxy.anthropic_base_url {
                config.anthropic_base_url = url;
            }
            if let Some(url) = file_config.proxy.openai_base_url {
                config.openai_base_url = url;
            }
            if let Some(level) = file_config.proxy.log_level {
                config.log_level = level;
            }
            if let Some(add) = file_config.proxy.add_header {
                config.add_header = add;
            }
            if let Some(fo) = file_config.proxy.fail_open {
                config.fail_open = fo;
            }
            if let Some(pt) = file_config.proxy.passthrough {
                config.passthrough = pt;
            }
            if let Some(token) = file_config.proxy.proxy_token {
                config.proxy_token = Some(token);
            }
            if !file_config.detection.whitelist.is_empty() {
                // Merge with the default whitelist (loopback, etc.)
                for term in file_config.detection.whitelist {
                    if !config.whitelist.iter().any(|w| w.eq_ignore_ascii_case(&term)) {
                        config.whitelist.push(term);
                    }
                }
            }
            if let Some(threshold) = file_config.detection.confidence_threshold {
                config.confidence_threshold = threshold;
            }
        }

        // 2. Environment variables take precedence
        if let Ok(addr) = env::var("MIRAGEIA_LISTEN_ADDR") {
            config.listen_addr = addr;
        }
        if let Ok(url) = env::var("MIRAGEIA_ANTHROPIC_URL") {
            config.anthropic_base_url = url;
        }
        if let Ok(url) = env::var("MIRAGEIA_OPENAI_URL") {
            config.openai_base_url = url;
        }
        if let Ok(level) = env::var("MIRAGEIA_LOG_LEVEL") {
            config.log_level = level;
        }
        if env::var("MIRAGEIA_PASSTHROUGH").is_ok() {
            config.passthrough = true;
        }
        if let Ok(name) = env::var("MIRAGEIA_MODEL_NAME") {
            config.model_name = Some(name);
        }
        if let Ok(token) = env::var("MIRAGEIA_PROXY_TOKEN") {
            config.proxy_token = Some(token);
        }

        config
    }

    /// Alias for compatibility (used in main.rs).
    pub fn from_env() -> Self {
        Self::load()
    }

    /// Validates the configuration, returning an error if any upstream URL is unsafe.
    pub fn validate(&self) -> Result<(), String> {
        for (name, url) in [
            ("anthropic_base_url", &self.anthropic_base_url),
            ("openai_base_url", &self.openai_base_url),
        ] {
            validate_upstream_url(name, url)?;
        }
        Ok(())
    }

    /// Path to the config file: ~/.mirageia/config.toml
    pub fn config_file_path() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".mirageia").join("config.toml"))
    }

    fn load_config_file() -> Option<FileConfig> {
        let path = Self::config_file_path()?;
        let content = std::fs::read_to_string(&path).ok()?;
        match toml::from_str::<FileConfig>(&content) {
            Ok(config) => {
                tracing::info!("Config chargée depuis {:?}", path);
                Some(config)
            }
            Err(e) => {
                tracing::warn!("Erreur dans {:?} : {}", path, e);
                None
            }
        }
    }
}

/// Validates that an upstream URL is safe to use as a proxy target.
///
/// Rejects localhost, loopback, private IPv4 ranges, IPv6 loopback, and cloud
/// metadata addresses to prevent SSRF attacks.
fn validate_upstream_url(name: &str, url: &str) -> Result<(), String> {
    let parsed = url::Url::from_str(url)
        .map_err(|e| format!("Invalid {}: cannot parse URL: {}", name, e))?;

    // Only http and https are valid upstream schemes
    match parsed.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(format!(
                "Invalid {}: scheme '{}' is not allowed (use http or https)",
                name, scheme
            ));
        }
    }

    let host = parsed
        .host_str()
        .filter(|h| !h.is_empty())
        .ok_or_else(|| format!("Invalid {}: host is empty", name))?;

    // Reject known local / private hosts
    let blocked = [
        "localhost",
        "127.0.0.1",
        "::1",
        "0.0.0.0",
    ];
    if blocked.iter().any(|b| host.eq_ignore_ascii_case(b)) {
        return Err(format!(
            "Invalid {}: host '{}' is a local/loopback address",
            name, host
        ));
    }

    // Reject private IPv4 ranges and cloud metadata address
    let private_prefixes = [
        "10.",
        "192.168.",
        "172.16.", "172.17.", "172.18.", "172.19.",
        "172.20.", "172.21.", "172.22.", "172.23.",
        "172.24.", "172.25.", "172.26.", "172.27.",
        "172.28.", "172.29.", "172.30.", "172.31.",
        "169.254.",
    ];
    if private_prefixes.iter().any(|prefix| host.starts_with(prefix)) {
        return Err(format!(
            "Invalid {}: host '{}' resolves to a private/reserved IP range",
            name, host
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.listen_addr, "127.0.0.1:3100");
        assert_eq!(config.anthropic_base_url, "https://api.anthropic.com");
        assert_eq!(config.openai_base_url, "https://api.openai.com");
        assert!(config.whitelist.contains(&"127.0.0.1".to_string()));
        assert!(config.whitelist.contains(&"localhost".to_string()));
        assert!(config.whitelist.contains(&"::1".to_string()));
        assert_eq!(config.confidence_threshold, 0.75);
        assert!(config.fail_open);
        assert!(!config.add_header);
        assert!(!config.passthrough);
    }

    #[test]
    fn test_parse_config_toml() {
        let toml_str = r#"
[proxy]
listen_addr = "0.0.0.0:4000"
log_level = "debug"
add_header = true

[detection]
whitelist = ["Thomas Edison", "localhost", "127.0.0.1"]
confidence_threshold = 0.8
"#;

        let file_config: FileConfig = toml::from_str(toml_str).unwrap();

        assert_eq!(file_config.proxy.listen_addr.as_deref(), Some("0.0.0.0:4000"));
        assert_eq!(file_config.proxy.log_level.as_deref(), Some("debug"));
        assert_eq!(file_config.proxy.add_header, Some(true));
        assert_eq!(file_config.detection.whitelist.len(), 3);
        assert_eq!(file_config.detection.confidence_threshold, Some(0.8));
    }

    #[test]
    fn test_parse_empty_config_toml() {
        let toml_str = "";
        let file_config: FileConfig = toml::from_str(toml_str).unwrap();

        assert!(file_config.proxy.listen_addr.is_none());
        assert!(file_config.detection.whitelist.is_empty());
    }

    #[test]
    fn test_parse_partial_config_toml() {
        let toml_str = r#"
[detection]
whitelist = ["Einstein"]
"#;

        let file_config: FileConfig = toml::from_str(toml_str).unwrap();

        assert!(file_config.proxy.listen_addr.is_none());
        assert_eq!(file_config.detection.whitelist, vec!["Einstein"]);
    }

    #[test]
    fn test_parse_passthrough_config_toml() {
        let toml_str = r#"
[proxy]
passthrough = true
"#;
        let file_config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(file_config.proxy.passthrough, Some(true));
    }

    #[test]
    fn test_config_file_path() {
        let path = AppConfig::config_file_path();
        assert!(path.is_some());
        let path = path.unwrap();
        assert!(
            path.ends_with(".mirageia/config.toml")
                || path.ends_with(".mirageia\\config.toml")
        );
    }

    // --- validate_upstream_url tests ---

    #[test]
    fn test_validate_valid_https_url() {
        assert!(validate_upstream_url("anthropic_base_url", "https://api.anthropic.com").is_ok());
        assert!(validate_upstream_url("openai_base_url", "https://api.openai.com").is_ok());
    }

    #[test]
    fn test_validate_valid_http_url() {
        assert!(validate_upstream_url("test", "http://example.com").is_ok());
    }

    #[test]
    fn test_validate_rejects_localhost() {
        assert!(validate_upstream_url("test", "http://localhost:8080").is_err());
        assert!(validate_upstream_url("test", "https://127.0.0.1/api").is_err());
        assert!(validate_upstream_url("test", "http://0.0.0.0").is_err());
    }

    #[test]
    fn test_validate_rejects_ipv6_loopback() {
        assert!(validate_upstream_url("test", "http://[::1]/api").is_err());
    }

    #[test]
    fn test_validate_rejects_private_ipv4() {
        assert!(validate_upstream_url("test", "http://10.0.0.1").is_err());
        assert!(validate_upstream_url("test", "http://192.168.1.1").is_err());
        assert!(validate_upstream_url("test", "http://172.16.0.1").is_err());
        assert!(validate_upstream_url("test", "http://172.31.255.255").is_err());
    }

    #[test]
    fn test_validate_rejects_cloud_metadata() {
        assert!(validate_upstream_url("test", "http://169.254.169.254/latest/meta-data").is_err());
    }

    #[test]
    fn test_validate_rejects_invalid_scheme() {
        assert!(validate_upstream_url("test", "file:///etc/passwd").is_err());
        assert!(validate_upstream_url("test", "ftp://example.com").is_err());
    }

    #[test]
    fn test_validate_rejects_unparseable_url() {
        assert!(validate_upstream_url("test", "not a url").is_err());
    }

    #[test]
    fn test_appconfig_validate_default() {
        let config = AppConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_appconfig_validate_rejects_ssrf() {
        let mut config = AppConfig::default();
        config.anthropic_base_url = "http://169.254.169.254/latest/meta-data".to_string();
        assert!(config.validate().is_err());
    }
}
