use std::env;
use std::path::PathBuf;

use serde::Deserialize;

/// Configuration complète de MirageIA.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub listen_addr: String,
    pub anthropic_base_url: String,
    pub openai_base_url: String,
    pub log_level: String,
    /// Termes à ne jamais pseudonymiser (noms publics, termes techniques, etc.).
    pub whitelist: Vec<String>,
    /// Confiance minimale pour la détection regex (0.0–1.0).
    pub confidence_threshold: f32,
    /// Ajouter le header X-MirageIA: active aux requêtes.
    pub add_header: bool,
    /// Mode fail-open : si true, transmet la requête non modifiée en cas d'erreur.
    pub fail_open: bool,
    /// Mode passthrough : si true, le proxy relaie sans pseudonymiser.
    pub passthrough: bool,
}

/// Structure du fichier config.toml (désérialisable).
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct FileConfig {
    proxy: ProxyFileConfig,
    detection: DetectionFileConfig,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct ProxyFileConfig {
    listen_addr: Option<String>,
    anthropic_base_url: Option<String>,
    openai_base_url: Option<String>,
    log_level: Option<String>,
    add_header: Option<bool>,
    fail_open: Option<bool>,
    passthrough: Option<bool>,
}

impl Default for ProxyFileConfig {
    fn default() -> Self {
        Self {
            listen_addr: None,
            anthropic_base_url: None,
            openai_base_url: None,
            log_level: None,
            add_header: None,
            fail_open: None,
            passthrough: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct DetectionFileConfig {
    whitelist: Vec<String>,
    confidence_threshold: Option<f32>,
}

impl Default for DetectionFileConfig {
    fn default() -> Self {
        Self {
            whitelist: Vec::new(),
            confidence_threshold: None,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            listen_addr: "127.0.0.1:3100".to_string(),
            anthropic_base_url: "https://api.anthropic.com".to_string(),
            openai_base_url: "https://api.openai.com".to_string(),
            log_level: "info".to_string(),
            whitelist: Vec::new(),
            confidence_threshold: 0.75,
            add_header: false,
            fail_open: true,
            passthrough: false,
        }
    }
}

impl AppConfig {
    /// Charge la config : fichier TOML (si présent) → variables d'environnement → défauts.
    pub fn load() -> Self {
        let mut config = Self::default();

        // 1. Charger le fichier config.toml s'il existe
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
            if !file_config.detection.whitelist.is_empty() {
                config.whitelist = file_config.detection.whitelist;
            }
            if let Some(threshold) = file_config.detection.confidence_threshold {
                config.confidence_threshold = threshold;
            }
        }

        // 2. Les variables d'environnement prennent le dessus
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

        config
    }

    /// Alias pour compatibilité (utilisé dans main.rs).
    pub fn from_env() -> Self {
        Self::load()
    }

    /// Chemin du fichier de config : ~/.mirageia/config.toml
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.listen_addr, "127.0.0.1:3100");
        assert_eq!(config.anthropic_base_url, "https://api.anthropic.com");
        assert_eq!(config.openai_base_url, "https://api.openai.com");
        assert!(config.whitelist.is_empty());
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
}
