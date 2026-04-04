use clap::{Parser, Subcommand};
use mirageia::config::AppConfig;
use mirageia::proxy;

#[derive(Parser)]
#[command(name = "mirageia", version, about = "Proxy de pseudonymisation intelligent pour API LLM")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Lancer le proxy HTTP (comportement par défaut)
    Proxy {
        /// Mode passthrough : relayer sans pseudonymiser
        #[arg(long)]
        passthrough: bool,
    },

    /// Assistant de configuration interactif
    Setup,

    /// Détecter les PII dans un texte (nécessite le modèle ONNX)
    Detect {
        /// Texte à analyser
        text: String,

        /// Nom du modèle dans ~/.mirageia/models/
        #[arg(short, long, default_value = "piiranha")]
        model: String,
    },

    /// Lancer une commande avec le proxy activé (activation par session)
    Wrap {
        /// Commande à exécuter (ex: claude, python, curl)
        #[arg(trailing_var_arg = true, required = true)]
        command: Vec<String>,

        /// Port du proxy MirageIA
        #[arg(short, long, default_value = "3100")]
        port: u16,
    },

    /// Afficher les requêtes en temps réel (console de monitoring)
    Console {
        /// Adresse du proxy MirageIA
        #[arg(short, long, default_value = "http://127.0.0.1:3100")]
        addr: String,
    },

    /// Vérifier et installer les mises à jour
    Update {
        /// Vérifier seulement, sans installer
        #[arg(long)]
        check: bool,
    },

    /// Arrêter le proxy MirageIA en cours d'exécution
    Stop {
        /// Adresse du proxy MirageIA
        #[arg(short, long, default_value = "http://127.0.0.1:3100")]
        addr: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Setup) => {
            mirageia::setup::run_setup()?;
        }
        Some(Commands::Detect { text, model }) => {
            init_tracing("info");
            run_detect(&text, &model)?;
        }
        Some(Commands::Wrap { command, port }) => {
            run_wrap(command, port).await?;
        }
        Some(Commands::Console { addr }) => {
            run_console(&addr).await?;
        }
        Some(Commands::Update { check }) => {
            mirageia::update::run_update(check).await?;
        }
        Some(Commands::Stop { addr }) => {
            run_stop(&addr).await?;
        }
        Some(Commands::Proxy { .. }) | None => {
            let passthrough = match &cli.command {
                Some(Commands::Proxy { passthrough }) => *passthrough,
                _ => false,
            };

            let mut config = AppConfig::from_env();
            if passthrough {
                config.passthrough = true;
            }
            init_tracing(&config.log_level);

            // Appliquer une mise à jour stagée si disponible
            if let Some(new_version) = mirageia::update::apply_staged_update() {
                tracing::info!("MirageIA mis à jour vers v{} — relancez pour utiliser la nouvelle version", new_version);
                eprintln!();
                eprintln!("  ✓ MirageIA mis à jour vers v{}", new_version);
                eprintln!("    Relancez MirageIA pour utiliser la nouvelle version.");
                eprintln!();
                return Ok(());
            }

            // Au premier lancement, proposer le setup si pas de config
            if !config_exists() {
                eprintln!("Première utilisation ? Lancez `mirageia setup` pour la configuration guidée.");
                eprintln!();
            }

            tracing::info!("MirageIA v{}", env!("CARGO_PKG_VERSION"));

            // Vérification de mise à jour en arrière-plan (silencieuse)
            mirageia::update::spawn_background_check();

            proxy::start_proxy(config).await?;
        }
    }

    Ok(())
}

fn init_tracing(log_level: &str) {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("mirageia={}", log_level).into()),
        )
        .init();
}

fn config_exists() -> bool {
    AppConfig::config_file_path()
        .map(|p| p.exists())
        .unwrap_or(false)
}

async fn run_stop(addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    match client.post(format!("{}/shutdown", addr)).send().await {
        Ok(resp) if resp.status().is_success() => {
            eprintln!("  ✓ MirageIA arrêté");
        }
        Ok(resp) => {
            eprintln!("  ✗ Réponse inattendue : {}", resp.status());
            std::process::exit(1);
        }
        Err(_) => {
            eprintln!("  ✗ MirageIA ne répond pas sur {}", addr);
            eprintln!("    Le proxy n'est peut-être pas en cours d'exécution.");
            std::process::exit(1);
        }
    }
    Ok(())
}

/// Lance une commande enfant avec les variables d'environnement pointant vers le proxy.
async fn run_wrap(command: Vec<String>, port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let proxy_url = format!("http://127.0.0.1:{}", port);

    // Vérifier que le proxy tourne
    match reqwest::get(&format!("{}/health", proxy_url)).await {
        Ok(resp) if resp.status().is_success() => {
            let health: serde_json::Value = resp.json().await?;
            let mode = if health["passthrough"].as_bool().unwrap_or(false) {
                "passthrough"
            } else {
                "pseudonymisation"
            };
            eprintln!("  ✓ MirageIA actif sur {} (mode {})", proxy_url, mode);
        }
        _ => {
            eprintln!("  ✗ MirageIA ne répond pas sur {}", proxy_url);
            eprintln!("    Lancez d'abord : mirageia");
            std::process::exit(1);
        }
    }

    let (program, args) = command.split_first().expect("Commande vide");

    eprintln!(
        "  → Lancement de '{}' avec proxy MirageIA activé",
        command.join(" ")
    );
    eprintln!();

    let status = std::process::Command::new(program)
        .args(args)
        .env("ANTHROPIC_BASE_URL", &proxy_url)
        .env("OPENAI_BASE_URL", &proxy_url)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()?;

    std::process::exit(status.code().unwrap_or(1));
}

/// Se connecte au flux SSE /events du proxy et affiche les événements en temps réel.
async fn run_console(addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    let events_url = format!("{}/events", addr);

    // Vérifier que le proxy tourne
    match reqwest::get(&format!("{}/health", addr)).await {
        Ok(resp) if resp.status().is_success() => {
            let health: serde_json::Value = resp.json().await?;
            let mode = if health["passthrough"].as_bool().unwrap_or(false) {
                "PASSTHROUGH"
            } else {
                "PSEUDONYMISATION"
            };
            let mappings = health["pii_mappings"].as_u64().unwrap_or(0);
            let version = health["version"].as_str().unwrap_or("?");
            eprintln!("  ╔══════════════════════════════════════════╗");
            eprintln!("  ║  MirageIA Console                       ║");
            eprintln!("  ╚══════════════════════════════════════════╝");
            eprintln!();
            eprintln!("  Version    : {}", version);
            eprintln!("  Proxy      : {}", addr);
            eprintln!("  Mode       : {}", mode);
            eprintln!("  Mappings   : {}", mappings);
            eprintln!();
            eprintln!("  En attente de requêtes... (Ctrl+C pour quitter)");
            eprintln!("  ─────────────────────────────────────────────────");
        }
        _ => {
            eprintln!("  ✗ MirageIA ne répond pas sur {}", addr);
            eprintln!("    Lancez d'abord : mirageia");
            std::process::exit(1);
        }
    }

    // Se connecter au flux SSE
    let response = reqwest::get(&events_url).await?;
    let mut stream = response.bytes_stream();

    use futures_util::StreamExt;
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        // Traiter chaque événement SSE complet
        while let Some(pos) = buffer.find("\n\n") {
            let event = buffer[..pos].to_string();
            buffer = buffer[pos + 2..].to_string();

            if let Some(data) = event.strip_prefix("data: ") {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                    print_event(&json);
                }
            }
        }
    }

    Ok(())
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn print_event(event: &serde_json::Value) {
    let timestamp = event["timestamp"]
        .as_str()
        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
        .map(|dt| dt.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| "??:??:??".to_string());

    let direction = event["direction"].as_str().unwrap_or("?");
    let provider = event["provider"].as_str().unwrap_or("???");
    let path = event["path"].as_str().unwrap_or("/");
    let pii_count = event["pii_count"].as_u64().unwrap_or(0);
    let passthrough = event["passthrough"].as_bool().unwrap_or(false);
    let body_size = event["body_size"].as_u64().unwrap_or(0);
    let model = event["model"].as_str();
    let status_code = event["status_code"].as_u64();
    let duration_ms = event["duration_ms"].as_u64();
    let streaming = event["streaming"].as_bool();
    let pii_types = event["pii_types"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let is_request = direction == "→";

    let dir_colored = if is_request {
        format!("\x1b[36m{}\x1b[0m", direction) // cyan pour requête
    } else {
        format!("\x1b[32m{}\x1b[0m", direction) // vert pour réponse
    };

    let mode = if passthrough {
        "\x1b[90mPASS\x1b[0m"
    } else {
        "\x1b[35mPII \x1b[0m"
    };

    if is_request {
        // Ligne requête : direction, mode, provider, path, modèle, taille
        let model_str = model
            .map(|m| format!("  \x1b[94m{}\x1b[0m", m))
            .unwrap_or_default();
        let size_str = if body_size > 0 {
            format!("  \x1b[90m{}\x1b[0m", format_size(body_size))
        } else {
            String::new()
        };

        eprintln!(
            "  \x1b[90m[{}]\x1b[0m {} {} {:<10} {}{}{}",
            timestamp, dir_colored, mode, provider, path, model_str, size_str
        );

        // Sous-ligne PII si détectées
        if pii_count > 0 && !pii_types.is_empty() {
            let types_str = pii_types.join(", ");
            eprintln!(
                "           \x1b[33m├── {} PII : {}\x1b[0m",
                pii_count, types_str
            );
        } else if pii_count > 0 {
            eprintln!(
                "           \x1b[33m├── {} PII détectées\x1b[0m",
                pii_count
            );
        }
    } else {
        // Ligne réponse : direction, status, provider, path, latence, streaming
        let status_str = match status_code {
            Some(code) if (200..300).contains(&code) => format!("\x1b[32m{}\x1b[0m", code),
            Some(code) if (400..500).contains(&code) => format!("\x1b[33m{}\x1b[0m", code),
            Some(code) if code >= 500 => format!("\x1b[31m{}\x1b[0m", code),
            Some(code) => format!("{}", code),
            None => "???".to_string(),
        };
        let duration_str = duration_ms
            .map(|ms| format!("  \x1b[90m{}ms\x1b[0m", ms))
            .unwrap_or_default();
        let stream_str = match streaming {
            Some(true) => "  \x1b[96mstreaming\x1b[0m",
            _ => "",
        };

        eprintln!(
            "  \x1b[90m[{}]\x1b[0m {} {} {:<10} {}{}{}",
            timestamp, dir_colored, status_str, provider, path, duration_str, stream_str
        );
    }
}

#[cfg(feature = "onnx")]
fn run_detect(text: &str, model_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    use mirageia::detection::PiiDetector;

    eprintln!("Chargement du modèle '{}'...", model_name);
    let detector = PiiDetector::from_model_name(model_name)?;

    eprintln!("Analyse du texte ({} caractères)...", text.len());
    let entities = detector.detect(text)?;

    if entities.is_empty() {
        println!("Aucune donnée sensible détectée.");
    } else {
        println!("{} entité(s) détectée(s) :\n", entities.len());
        for entity in &entities {
            println!("  {}", entity);
        }
    }

    Ok(())
}

#[cfg(not(feature = "onnx"))]
fn run_detect(_text: &str, _model_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Erreur : la détection PII nécessite la feature 'onnx'.");
    eprintln!("Recompilez avec : cargo run --features onnx -- detect \"votre texte\"");
    std::process::exit(1);
}
