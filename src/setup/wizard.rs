use std::env;
use std::fs;
use std::path::PathBuf;

use dialoguer::{Confirm, Input, MultiSelect, theme::ColorfulTheme};

/// Résultat de l'assistant de configuration.
#[derive(Debug)]
pub struct SetupResult {
    pub listen_port: u16,
    pub providers: Vec<LlmProvider>,
    pub whitelist: Vec<String>,
    pub config_path: PathBuf,
    pub shell_configured: bool,
}

#[derive(Debug, Clone)]
pub struct LlmProvider {
    pub name: String,
    pub base_url: String,
    pub env_var: String,
    pub api_key_var: String,
    pub api_key_detected: bool,
}

/// Lance l'assistant de configuration interactif.
pub fn run_setup() -> Result<SetupResult, Box<dyn std::error::Error>> {
    let theme = ColorfulTheme::default();

    println!();
    println!("  ╔══════════════════════════════════════════╗");
    println!("  ║  MirageIA — Assistant de configuration   ║");
    println!("  ╚════��════════════════���════════════════════╝");
    println!();

    // --- Étape 1 : Détection de l'environnement ---
    let os_name = detect_os();
    let shell_name = detect_shell();
    println!("  Système détecté : {} ({})", os_name, shell_name);
    println!();

    // --- ��tape 2 : Choix du port ---
    let listen_port: u16 = Input::with_theme(&theme)
        .with_prompt("Port d'écoute du proxy")
        .default(3100)
        .interact_text()?;

    println!();

    // --- Étape 3 : Choix des providers LLM ---
    let provider_options = [("Anthropic (Claude)", "https://api.anthropic.com", "ANTHROPIC_BASE_URL", "ANTHROPIC_API_KEY"),
        ("OpenAI (GPT)", "https://api.openai.com", "OPENAI_BASE_URL", "OPENAI_API_KEY"),
        ("Google Gemini", "https://generativelanguage.googleapis.com", "GEMINI_BASE_URL", "GEMINI_API_KEY"),
        ("Mistral AI", "https://api.mistral.ai", "MISTRAL_BASE_URL", "MISTRAL_API_KEY")];

    let display_options: Vec<String> = provider_options
        .iter()
        .map(|(name, _, _, key_var)| {
            let detected = env::var(key_var).is_ok();
            if detected {
                format!("{} ✓ clé API détectée", name)
            } else {
                name.to_string()
            }
        })
        .collect();

    // Pré-sélectionner les providers dont la clé API est détectée
    let defaults: Vec<bool> = provider_options
        .iter()
        .map(|(_, _, _, key_var)| env::var(key_var).is_ok())
        .collect();

    let selected = MultiSelect::with_theme(&theme)
        .with_prompt("Quels providers LLM utilisez-vous ? (Espace pour sélectionner, Entrée pour valider)")
        .items(&display_options)
        .defaults(&defaults)
        .interact()?;

    if selected.is_empty() {
        println!();
        println!("  ⚠ Aucun provider sélectionné. Vous pourrez configurer manuellement plus tard.");
        println!("    Fichier : ~/.mirageia/config.toml");
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

    // --- Étape 4 : Whitelist ---
    let add_whitelist = Confirm::with_theme(&theme)
        .with_prompt("Ajouter des termes à ne jamais pseudonymiser (whitelist) ?")
        .default(false)
        .interact()?;

    let mut whitelist = vec!["localhost".to_string(), "127.0.0.1".to_string()];

    if add_whitelist {
        println!("  Entrez les termes séparés par des virgules (ex: Thomas Edison, Martin Fowler)");
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

    // --- Étape 5 : Générer la configuration ---
    let config_dir = dirs::home_dir()
        .expect("Impossible de trouver le répertoire home")
        .join(".mirageia");

    fs::create_dir_all(&config_dir)?;

    let config_path = config_dir.join("config.toml");
    let config_content = generate_config(listen_port, &providers, &whitelist);

    if config_path.exists() {
        let overwrite = Confirm::with_theme(&theme)
            .with_prompt(format!(
                "{} existe déjà. Écraser ?",
                config_path.display()
            ))
            .default(false)
            .interact()?;

        if !overwrite {
            println!("  Configuration conservée.");
            println!();
        } else {
            fs::write(&config_path, &config_content)?;
            println!("  ✓ Configuration écrite dans {}", config_path.display());
        }
    } else {
        fs::write(&config_path, &config_content)?;
        println!("  ✓ Configuration écrite dans {}", config_path.display());
    }

    println!();

    // --- Étape 6 : Configurer le shell ---
    let shell_configured = configure_shell(&theme, listen_port, &providers, &shell_name)?;

    // --- Résumé ---
    print_summary(listen_port, &providers, &whitelist, &config_path, shell_configured);

    Ok(SetupResult {
        listen_port,
        providers,
        whitelist,
        config_path,
        shell_configured,
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

    "inconnu".to_string()
}

fn generate_config(port: u16, providers: &[LlmProvider], whitelist: &[String]) -> String {
    let mut config = String::new();

    config.push_str("# Configuration MirageIA\n");
    config.push_str("# Généré par `mirageia setup`\n\n");

    config.push_str("[proxy]\n");
    config.push_str(&format!("listen_addr = \"127.0.0.1:{}\"\n", port));
    config.push_str("log_level = \"info\"\n");
    config.push_str("fail_open = true\n");
    config.push('\n');

    // URLs des providers
    for provider in providers {
        let key = match provider.name.as_str() {
            n if n.contains("Anthropic") => "anthropic_base_url",
            n if n.contains("OpenAI") => "openai_base_url",
            _ => continue, // Les autres providers ne sont pas encore supportés dans le routeur
        };
        config.push_str(&format!("# {} — URL par défaut, pas besoin de changer\n", provider.name));
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

fn configure_shell(
    theme: &ColorfulTheme,
    port: u16,
    providers: &[LlmProvider],
    shell_name: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    if providers.is_empty() {
        return Ok(false);
    }

    // Construire les lignes d'export
    let mut export_lines = Vec::new();
    for provider in providers {
        export_lines.push(format!(
            "export {}=http://localhost:{}",
            provider.env_var, port
        ));
    }

    println!("  Pour rediriger vos outils vers MirageIA, ajoutez ces lignes à votre shell :");
    println!();
    for line in &export_lines {
        println!("    {}", line);
    }
    println!();

    // Trouver le fichier de profil
    let profile_path = match shell_name {
        "zsh" => dirs::home_dir().map(|h| h.join(".zshrc")),
        "bash" | "Git Bash" => dirs::home_dir().map(|h| h.join(".bashrc")),
        "fish" => dirs::home_dir().map(|h| h.join(".config").join("fish").join("config.fish")),
        _ => None,
    };

    if let Some(profile) = profile_path {
        let auto_configure = Confirm::with_theme(theme)
            .with_prompt(format!(
                "Ajouter automatiquement à {} ?",
                profile.display()
            ))
            .default(true)
            .interact()?;

        if auto_configure {
            let mut content = fs::read_to_string(&profile).unwrap_or_default();

            // Vérifier si déjà configuré
            if content.contains("MIRAGEIA") || content.contains(&format!("localhost:{}", port)) {
                println!("  ⚠ Configuration MirageIA déjà présente dans {}", profile.display());
                return Ok(true);
            }

            content.push_str("\n# MirageIA — proxy de pseudonymisation LLM\n");
            for line in &export_lines {
                content.push_str(line);
                content.push('\n');
            }

            fs::write(&profile, content)?;
            println!("  ✓ {} mis à jour", profile.display());
            println!("  ⚠ Rechargez votre shell : source {}", profile.display());
            return Ok(true);
        }
    } else {
        println!("  Shell '{}' — ajoutez les lignes manuellement.", shell_name);
    }

    Ok(false)
}

fn print_summary(
    port: u16,
    providers: &[LlmProvider],
    whitelist: &[String],
    config_path: &std::path::Path,
    shell_configured: bool,
) {
    println!();
    println!("  ╔═══════════════════════���══════════════════╗");
    println!("  ║  Configuration terminée !                ║");
    println!("  ╚═════════��══════════════════���═════════════╝");
    println!();
    println!("  Résumé :");
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
            println!("    ⚠ Clés manquantes : {}", missing_keys.join(", "));
        }
    }

    if whitelist.len() > 2 {
        // Plus que localhost et 127.0.0.1
        println!("    Whitelist       : {} termes", whitelist.len());
    }

    if shell_configured {
        println!("    Shell           : ✓ configuré");
    } else {
        println!("    Shell           : configuration manuelle requise");
    }

    println!();
    println!("  Pour démarrer MirageIA :");
    println!();
    println!("    mirageia");
    println!();

    if !providers.is_empty() {
        let missing: Vec<&LlmProvider> = providers.iter().filter(|p| !p.api_key_detected).collect();
        if !missing.is_empty() {
            println!("  N'oubliez pas de configurer vos clés API :");
            for p in missing {
                println!("    export {}=<votre-clé>", p.api_key_var);
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
        // On tourne sur Windows dans ce projet
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

        let config = generate_config(3100, &providers, &whitelist);

        assert!(config.contains("listen_addr = \"127.0.0.1:3100\""));
        assert!(config.contains("fail_open = true"));
        assert!(config.contains("confidence_threshold = 0.75"));
        assert!(config.contains("\"localhost\""));
        assert!(config.contains("Anthropic"));
    }

    #[test]
    fn test_generate_config_custom_port() {
        let config = generate_config(4200, &[], &[]);
        assert!(config.contains("listen_addr = \"127.0.0.1:4200\""));
    }

    #[test]
    fn test_generate_config_multiple_whitelist() {
        let whitelist = vec![
            "localhost".to_string(),
            "127.0.0.1".to_string(),
            "Thomas Edison".to_string(),
        ];
        let config = generate_config(3100, &[], &whitelist);
        assert!(config.contains("\"Thomas Edison\""));
        assert!(config.contains("\"127.0.0.1\""));
    }

    #[test]
    fn test_generate_config_empty_whitelist() {
        let config = generate_config(3100, &[], &[]);
        assert!(!config.contains("whitelist"));
    }
}
