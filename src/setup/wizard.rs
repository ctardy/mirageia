use std::env;
use std::fs;
use std::path::PathBuf;

use dialoguer::{Confirm, Input, MultiSelect, theme::ColorfulTheme};

/// Result of the configuration wizard.
#[derive(Debug)]
pub struct SetupResult {
    pub listen_port: u16,
    pub providers: Vec<LlmProvider>,
    pub whitelist: Vec<String>,
    pub config_path: PathBuf,
    pub shell_configured: bool,
    pub onnx_model_configured: bool,
    pub upstream_proxy: Option<String>,
    pub danger_accept_invalid_certs: bool,
}

#[derive(Debug, Clone)]
pub struct LlmProvider {
    pub name: String,
    pub base_url: String,
    pub env_var: String,
    pub api_key_var: String,
    pub api_key_detected: bool,
}

/// Launches the interactive configuration wizard.
pub fn run_setup() -> Result<SetupResult, Box<dyn std::error::Error>> {
    let theme = ColorfulTheme::default();

    println!();
    println!("  ╔══════════════════════════════════════════╗");
    println!("  ║  MirageIA — Configuration Wizard         ║");
    println!("  ╚══════════════════════════════════════════╝");
    println!();

    // --- Step 1: Environment detection ---
    let os_name = detect_os();
    let shell_name = detect_shell();
    println!("  Detected system: {} ({})", os_name, shell_name);
    println!();

    // --- Step 2: Port selection ---
    let listen_port: u16 = Input::with_theme(&theme)
        .with_prompt("Proxy listening port")
        .default(3100)
        .interact_text()?;

    println!();

    // --- Step 3: LLM provider selection ---
    let provider_options = [("Anthropic (Claude)", "https://api.anthropic.com", "ANTHROPIC_BASE_URL", "ANTHROPIC_API_KEY"),
        ("OpenAI (GPT)", "https://api.openai.com", "OPENAI_BASE_URL", "OPENAI_API_KEY"),
        ("Google Gemini", "https://generativelanguage.googleapis.com", "GEMINI_BASE_URL", "GEMINI_API_KEY"),
        ("Mistral AI", "https://api.mistral.ai", "MISTRAL_BASE_URL", "MISTRAL_API_KEY")];

    let display_options: Vec<String> = provider_options
        .iter()
        .map(|(name, _, _, key_var)| {
            let detected = env::var(key_var).is_ok();
            if detected {
                format!("{} ✓ API key detected", name)
            } else {
                name.to_string()
            }
        })
        .collect();

    // Pre-select providers whose API key is detected
    let defaults: Vec<bool> = provider_options
        .iter()
        .map(|(_, _, _, key_var)| env::var(key_var).is_ok())
        .collect();

    let selected = MultiSelect::with_theme(&theme)
        .with_prompt("Which LLM providers do you use? (Space to select, Enter to confirm)")
        .items(&display_options)
        .defaults(&defaults)
        .interact()?;

    if selected.is_empty() {
        println!();
        println!("  ⚠ No provider selected. You can configure manually later.");
        println!("    File: ~/.mirageia/config.toml");
    }

    let providers: Vec<LlmProvider> = selected
        .iter()
        .map(|&i| {
            let (name, url, env_var, key_var) = provider_options[i];
            LlmProvider {
                name: name.to_string(),
                base_url: url.to_string(),
                env_var: env_var.to_string(),
                api_key_var: key_var.to_string(),
                api_key_detected: env::var(key_var).is_ok(),
            }
        })
        .collect();

    println!();

    // --- Step 4: Whitelist ---
    let add_whitelist = Confirm::with_theme(&theme)
        .with_prompt("Add terms to never pseudonymize (whitelist)?")
        .default(false)
        .interact()?;

    let mut whitelist = vec!["localhost".to_string(), "127.0.0.1".to_string()];

    if add_whitelist {
        println!("  Enter terms separated by commas (e.g., Thomas Edison, Martin Fowler)");
        let input: String = Input::with_theme(&theme)
            .with_prompt("Whitelist")
            .default(String::new())
            .allow_empty(true)
            .interact_text()?;

        for term in input.split(',') {
            let trimmed = term.trim().to_string();
            if !trimmed.is_empty() {
                whitelist.push(trimmed);
            }
        }
    }

    println!();

    // --- Step 5: Corporate proxy ---
    let upstream_proxy = setup_upstream_proxy(&theme)?;

    // --- Step 5b: SSL inspection (only if a proxy was configured) ---
    let danger_accept_invalid_certs = if upstream_proxy.is_some() {
        setup_danger_accept_invalid_certs(&theme)?
    } else {
        false
    };

    println!();

    // --- Step 6: ONNX contextual detection ---
    let onnx_model_configured = setup_onnx_model(&theme)?;

    println!();

    // --- Step 7: Generate configuration ---
    let config_dir = dirs::home_dir()
        .expect("Cannot find home directory")
        .join(".mirageia");

    fs::create_dir_all(&config_dir)?;

    let config_path = config_dir.join("config.toml");
    let config_content = generate_config(listen_port, &providers, &whitelist, upstream_proxy.as_deref(), danger_accept_invalid_certs);

    if config_path.exists() {
        let overwrite = Confirm::with_theme(&theme)
            .with_prompt(format!(
                "{} already exists. Overwrite?",
                config_path.display()
            ))
            .default(false)
            .interact()?;

        if !overwrite {
            println!("  Configuration preserved.");
            println!();
        } else {
            fs::write(&config_path, &config_content)?;
            println!("  ✓ Configuration written to {}", config_path.display());
        }
    } else {
        fs::write(&config_path, &config_content)?;
        println!("  ✓ Configuration written to {}", config_path.display());
    }

    println!();

    // --- Step 8: Shell configuration ---
    let shell_configured = configure_shell(&theme, listen_port, &providers, &shell_name)?;

    // --- Summary ---
    print_summary(listen_port, &providers, &whitelist, &config_path, shell_configured, onnx_model_configured, upstream_proxy.as_deref(), danger_accept_invalid_certs);

    Ok(SetupResult {
        listen_port,
        providers,
        whitelist,
        config_path,
        shell_configured,
        onnx_model_configured,
        upstream_proxy,
        danger_accept_invalid_certs,
    })
}

fn detect_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else {
        "Linux"
    }
}

fn detect_shell() -> String {
    if let Ok(shell) = env::var("SHELL") {
        if shell.contains("zsh") {
            return "zsh".to_string();
        } else if shell.contains("bash") {
            return "bash".to_string();
        } else if shell.contains("fish") {
            return "fish".to_string();
        }
    }

    // Windows
    if env::var("PSModulePath").is_ok() {
        return "PowerShell".to_string();
    }

    if env::var("MSYSTEM").is_ok() {
        return "Git Bash".to_string();
    }

    "unknown".to_string()
}

fn generate_config(port: u16, providers: &[LlmProvider], whitelist: &[String], upstream_proxy: Option<&str>, danger_accept_invalid_certs: bool) -> String {
    let mut config = String::new();

    config.push_str("# MirageIA Configuration\n");
    config.push_str("# Generated by `mirageia setup`\n\n");

    config.push_str("[proxy]\n");
    config.push_str(&format!("listen_addr = \"127.0.0.1:{}\"\n", port));
    config.push_str("log_level = \"info\"\n");
    config.push_str("fail_open = true\n");
    if let Some(proxy) = upstream_proxy {
        config.push_str(&format!("upstream_proxy = \"{}\"\n", proxy));
    }
    if danger_accept_invalid_certs {
        config.push_str("danger_accept_invalid_certs = true\n");
    }
    config.push('\n');

    // Provider URLs
    for provider in providers {
        let key = match provider.name.as_str() {
            n if n.contains("Anthropic") => "anthropic_base_url",
            n if n.contains("OpenAI") => "openai_base_url",
            _ => continue, // Other providers not yet supported in the router
        };
        config.push_str(&format!("# {} — default URL, no need to change\n", provider.name));
        config.push_str(&format!("# {} = \"{}\"\n", key, provider.base_url));
    }

    config.push('\n');
    config.push_str("[detection]\n");
    config.push_str("confidence_threshold = 0.75\n");

    if !whitelist.is_empty() {
        config.push_str("whitelist = [\n");
        for term in whitelist {
            config.push_str(&format!("    \"{}\",\n", term));
        }
        config.push_str("]\n");
    }

    config
}

fn setup_upstream_proxy(theme: &ColorfulTheme) -> Result<Option<String>, Box<dyn std::error::Error>> {
    // Check if already set via environment
    if let Ok(existing) = env::var("HTTPS_PROXY").or_else(|_| env::var("HTTP_PROXY")) {
        println!("  ✓ Corporate proxy detected from environment: {}", existing);
        let keep = Confirm::with_theme(theme)
            .with_prompt("Save this proxy to config.toml?")
            .default(true)
            .interact()?;
        return Ok(if keep { Some(existing) } else { None });
    }

    let behind_proxy = Confirm::with_theme(theme)
        .with_prompt("Are you behind a corporate proxy?")
        .default(false)
        .interact()?;

    if !behind_proxy {
        return Ok(None);
    }

    let proxy_url: String = Input::with_theme(theme)
        .with_prompt("Proxy URL (e.g. http://proxy.corp:8080)")
        .interact_text()?;

    let trimmed = proxy_url.trim().to_string();
    if trimmed.is_empty() {
        return Ok(None);
    }

    println!("  ✓ Proxy configured: {}", trimmed);
    Ok(Some(trimmed))
}

fn setup_danger_accept_invalid_certs(theme: &ColorfulTheme) -> Result<bool, Box<dyn std::error::Error>> {
    println!("  Some corporate proxies perform SSL inspection (MITM) and present");
    println!("  their own certificate — which MirageIA would reject by default.");
    println!();

    let accept = Confirm::with_theme(theme)
        .with_prompt("Does your proxy do SSL inspection? (accept invalid certificates)")
        .default(false)
        .interact()?;

    if accept {
        println!("  ✓ danger_accept_invalid_certs = true (TLS validation disabled for upstream)");
    } else {
        println!("  Standard TLS validation kept.");
    }

    Ok(accept)
}

fn setup_onnx_model(theme: &ColorfulTheme) -> Result<bool, Box<dyn std::error::Error>> {
    use crate::detection::model_manager;

    const MODEL: &str = "iiiorg/piiranha-v1-detect-personal-information";

    // Already configured — skip
    if let Some(active) = model_manager::get_active_model() {
        println!("  ✓ ONNX model already active: {}", active);
        return Ok(true);
    }

    println!("  PII detection: regex mode (default, no download required).");
    println!("  ONNX contextual mode is more accurate (understands context,");
    println!("  fewer false positives) but requires a ~337 MB one-time download.");
    println!();

    let enable = Confirm::with_theme(theme)
        .with_prompt("Enable ONNX contextual detection? (~337 MB download)")
        .default(false)
        .interact()?;

    if !enable {
        println!("  Skipped — regex detection active. Enable later: mirageia model download {}", MODEL);
        return Ok(false);
    }

    println!();
    println!("  Downloading model '{}'...", MODEL);
    println!("  (this may take a few minutes depending on your connection)");

    match model_manager::ensure_model(MODEL) {
        Ok(_) => {
            model_manager::set_active_model(MODEL)
                .map_err(|e| format!("Failed to activate model: {}", e))?;
            println!("  ✓ Model downloaded and activated");
            Ok(true)
        }
        Err(e) => {
            println!("  ✗ Download failed: {}", e);
            println!("  Falling back to regex detection.");
            println!("  Retry later: mirageia model download {}", MODEL);
            Ok(false)
        }
    }
}

fn configure_shell(
    theme: &ColorfulTheme,
    port: u16,
    providers: &[LlmProvider],
    shell_name: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    if providers.is_empty() {
        return Ok(false);
    }

    // Build export lines
    let mut export_lines = Vec::new();
    for provider in providers {
        export_lines.push(format!(
            "export {}=http://localhost:{}",
            provider.env_var, port
        ));
    }

    println!("  To redirect your tools through MirageIA, add these lines to your shell:");
    println!();
    for line in &export_lines {
        println!("    {}", line);
    }
    println!();

    // Find the profile file
    let profile_path = match shell_name {
        "zsh" => dirs::home_dir().map(|h| h.join(".zshrc")),
        "bash" | "Git Bash" => dirs::home_dir().map(|h| h.join(".bashrc")),
        "fish" => dirs::home_dir().map(|h| h.join(".config").join("fish").join("config.fish")),
        _ => None,
    };

    if let Some(profile) = profile_path {
        let auto_configure = Confirm::with_theme(theme)
            .with_prompt(format!(
                "Add automatically to {}?",
                profile.display()
            ))
            .default(true)
            .interact()?;

        if auto_configure {
            let mut content = fs::read_to_string(&profile).unwrap_or_default();

            // Check if already configured
            if content.contains("MIRAGEIA") || content.contains(&format!("localhost:{}", port)) {
                println!("  ⚠ MirageIA configuration already present in {}", profile.display());
                return Ok(true);
            }

            content.push_str("\n# MirageIA — LLM pseudonymization proxy\n");
            for line in &export_lines {
                content.push_str(line);
                content.push('\n');
            }

            fs::write(&profile, content)?;
            println!("  ✓ {} updated", profile.display());
            println!("  ⚠ Reload your shell: source {}", profile.display());
            return Ok(true);
        }
    } else {
        println!("  Shell '{}' — add the lines manually.", shell_name);
    }

    Ok(false)
}

fn print_summary(
    port: u16,
    providers: &[LlmProvider],
    whitelist: &[String],
    config_path: &std::path::Path,
    shell_configured: bool,
    onnx_model_configured: bool,
    upstream_proxy: Option<&str>,
    danger_accept_invalid_certs: bool,
) {
    println!();
    println!("  ╔══════════════════════════════════════════╗");
    println!("  ║  Configuration complete!                 ║");
    println!("  ╚══════════════════════════════════════════╝");
    println!();
    println!("  Summary:");
    println!("    Proxy           : http://127.0.0.1:{}", port);
    println!("    Config          : {}", config_path.display());

    if !providers.is_empty() {
        println!("    Providers       : {}", providers.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", "));

        let missing_keys: Vec<&str> = providers
            .iter()
            .filter(|p| !p.api_key_detected)
            .map(|p| p.api_key_var.as_str())
            .collect();

        if !missing_keys.is_empty() {
            println!("    ⚠ Missing keys  : {}", missing_keys.join(", "));
        }
    }

    if whitelist.len() > 2 {
        // More than localhost and 127.0.0.1
        println!("    Whitelist       : {} terms", whitelist.len());
    }

    if let Some(proxy) = upstream_proxy {
        println!("    Corporate proxy : {}", proxy);
        if danger_accept_invalid_certs {
            println!("    SSL inspection  : ✓ accept invalid certs enabled");
        }
    }

    if onnx_model_configured {
        println!("    PII detection   : ✓ ONNX contextual model active");
    } else {
        println!("    PII detection   : regex (enable later: mirageia model download iiiorg/piiranha-v1-detect-personal-information)");
    }

    if shell_configured {
        println!("    Shell           : ✓ configured");
    } else {
        println!("    Shell           : manual configuration required");
    }

    println!();
    println!("  To start MirageIA:");
    println!();
    println!("    mirageia");
    println!();

    if !providers.is_empty() {
        let missing: Vec<&LlmProvider> = providers.iter().filter(|p| !p.api_key_detected).collect();
        if !missing.is_empty() {
            println!("  Don't forget to set your API keys:");
            for p in missing {
                println!("    export {}=<your-key>", p.api_key_var);
            }
            println!();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_os() {
        let os = detect_os();
        assert!(!os.is_empty());
        #[cfg(target_os = "windows")]
        assert_eq!(os, "Windows");
    }

    #[test]
    fn test_detect_shell() {
        let shell = detect_shell();
        assert!(!shell.is_empty());
    }

    #[test]
    fn test_generate_config_basic() {
        let providers = vec![LlmProvider {
            name: "Anthropic (Claude)".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            env_var: "ANTHROPIC_BASE_URL".to_string(),
            api_key_var: "ANTHROPIC_API_KEY".to_string(),
            api_key_detected: false,
        }];
        let whitelist = vec!["localhost".to_string()];

        let config = generate_config(3100, &providers, &whitelist, None, false);

        assert!(config.contains("listen_addr = \"127.0.0.1:3100\""));
        assert!(config.contains("fail_open = true"));
        assert!(config.contains("confidence_threshold = 0.75"));
        assert!(config.contains("\"localhost\""));
        assert!(config.contains("Anthropic"));
    }

    #[test]
    fn test_generate_config_custom_port() {
        let config = generate_config(4200, &[], &[], None, false);
        assert!(config.contains("listen_addr = \"127.0.0.1:4200\""));
    }

    #[test]
    fn test_generate_config_multiple_whitelist() {
        let whitelist = vec![
            "localhost".to_string(),
            "127.0.0.1".to_string(),
            "Thomas Edison".to_string(),
        ];
        let config = generate_config(3100, &[], &whitelist, None, false);
        assert!(config.contains("\"Thomas Edison\""));
        assert!(config.contains("\"127.0.0.1\""));
    }

    #[test]
    fn test_generate_config_empty_whitelist() {
        let config = generate_config(3100, &[], &[], None, false);
        assert!(!config.contains("whitelist"));
    }
}
