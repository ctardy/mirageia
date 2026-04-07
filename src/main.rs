use clap::{Parser, Subcommand};
use mirageia::config::AppConfig;
use mirageia::proxy;
use mirageia::proxy::server::start_proxy as proxy_start;

#[derive(Parser)]
#[command(name = "mirageia", version, about = "Intelligent pseudonymization proxy for LLM APIs")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the HTTP proxy (default behavior)
    Proxy {
        /// Passthrough mode: relay without pseudonymizing
        #[arg(long)]
        passthrough: bool,
    },

    /// Interactive configuration wizard
    Setup,

    /// Detect PII in text (requires ONNX model)
    Detect {
        /// Text to analyze
        text: String,

        /// Model name in ~/.mirageia/models/
        #[arg(short, long, default_value = "piiranha")]
        model: String,
    },

    /// Run a command with the proxy enabled (per-session activation)
    Wrap {
        /// Command to execute (e.g., claude, python, curl)
        #[arg(trailing_var_arg = true, required = true)]
        command: Vec<String>,

        /// MirageIA proxy port
        #[arg(short, long, default_value = "3100")]
        port: u16,
    },

    /// Display requests in real time (monitoring console)
    Console {
        /// MirageIA proxy address
        #[arg(short, long, default_value = "http://127.0.0.1:3100")]
        addr: String,
    },

    /// Check and install updates
    Update {
        /// Check only, without installing
        #[arg(long)]
        check: bool,
    },

    /// Stop the running MirageIA proxy
    Stop {
        /// MirageIA proxy address
        #[arg(short, long, default_value = "http://127.0.0.1:3100")]
        addr: String,
    },

    /// Manage cached ONNX models (~/.mirageia/models/)
    Model {
        #[command(subcommand)]
        action: ModelAction,
    },
}

#[derive(Subcommand)]
enum ModelAction {
    /// List cached models
    List,

    /// Download a model from HuggingFace
    Download {
        /// HuggingFace model name (e.g., dslim/bert-base-NER)
        name: String,
    },

    /// Set the active model
    Use {
        /// Name of the model to activate
        name: String,
    },

    /// Delete a model from cache
    Delete {
        /// Name of the model to delete
        name: String,
    },

    /// Verify SHA-256 integrity of the active model
    Verify,
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
        Some(Commands::Model { action }) => {
            run_model_command(action)?;
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

            // Apply staged update if available
            if let Some(new_version) = mirageia::update::apply_staged_update() {
                tracing::info!("MirageIA updated to v{} — restart to use the new version", new_version);
                eprintln!();
                eprintln!("  ✓ MirageIA updated to v{}", new_version);
                eprintln!("    Restart MirageIA to use the new version.");
                eprintln!();
                return Ok(());
            }

            // On first launch, suggest setup if no config exists
            if !config_exists() {
                eprintln!("First time? Run `mirageia setup` for guided configuration.");
                eprintln!();
            }

            tracing::info!("MirageIA v{}", env!("CARGO_PKG_VERSION"));

            // Validate upstream URLs before starting — reject SSRF-prone configs
            if let Err(e) = config.validate() {
                tracing::error!("Configuration invalide : {}", e);
                eprintln!("Configuration error: {}", e);
                std::process::exit(1);
            }

            // Background update check (silent)
            mirageia::update::spawn_background_check();

            proxy::start_proxy(config).await?;
        }
    }

    Ok(())
}

fn run_model_command(action: ModelAction) -> Result<(), Box<dyn std::error::Error>> {
    use mirageia::detection::model_manager;

    match action {
        ModelAction::List => {
            let models = model_manager::list_models();
            if models.is_empty() {
                eprintln!("No cached models.");
                eprintln!("Use `mirageia model download <name>` to download a model.");
            } else {
                eprintln!("Cached models:");
                for (name, is_active) in &models {
                    let marker = if *is_active { " <- active" } else { "" };
                    eprintln!("  {}{}", name, marker);
                }
            }
        }

        ModelAction::Download { name } => {
            eprintln!("Downloading model '{}'...", name);
            match model_manager::ensure_model(&name) {
                Ok(path) => {
                    eprintln!("  ✓ Model downloaded: {:?}", path);
                }
                Err(e) => {
                    eprintln!("  ✗ Failed: {}", e);
                    std::process::exit(1);
                }
            }
        }

        ModelAction::Use { name } => {
            match model_manager::set_active_model(&name) {
                Ok(()) => {
                    eprintln!("  ✓ Active model set: {}", name);
                }
                Err(e) => {
                    eprintln!("  ✗ Failed: {}", e);
                    std::process::exit(1);
                }
            }
        }

        ModelAction::Delete { name } => {
            match model_manager::delete_model(&name) {
                Ok(()) => {
                    eprintln!("  ✓ Model '{}' deleted from cache", name);
                }
                Err(e) => {
                    eprintln!("  ✗ Failed: {}", e);
                    std::process::exit(1);
                }
            }
        }

        ModelAction::Verify => {
            let active = model_manager::get_active_model();
            match active {
                None => {
                    eprintln!("No active model configured.");
                    eprintln!("Use `mirageia model use <name>` to set one.");
                    std::process::exit(1);
                }
                Some(name) => {
                    eprintln!("Verifying active model '{}'...", name);
                    match model_manager::verify_model(&name) {
                        Ok(true) => {
                            eprintln!("  ✓ Integrity verified");
                        }
                        Ok(false) => {
                            eprintln!("  ✗ Model missing or corrupted");
                            std::process::exit(1);
                        }
                        Err(e) => {
                            eprintln!("  ✗ Verification error: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
            }
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
            eprintln!("  ✓ MirageIA stopped");
        }
        Ok(resp) => {
            eprintln!("  ✗ Unexpected response: {}", resp.status());
            std::process::exit(1);
        }
        Err(_) => {
            eprintln!("  ✗ MirageIA not responding on {}", addr);
            eprintln!("    The proxy may not be running.");
            std::process::exit(1);
        }
    }
    Ok(())
}

/// Starts the proxy in the background if not already running. Waits up to 5s.
async fn ensure_proxy_running(proxy_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Already running?
    if reqwest::get(&format!("{}/health", proxy_url)).await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
    {
        return Ok(());
    }

    eprintln!("  Starting MirageIA proxy in the background...");
    let config = AppConfig::load();
    tokio::spawn(async move {
        let _ = proxy_start(config).await;
    });

    for _ in 0..50 {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        if reqwest::get(&format!("{}/health", proxy_url)).await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            eprintln!("  ✓ MirageIA started");
            return Ok(());
        }
    }

    eprintln!("  ✗ MirageIA failed to start within 5s");
    std::process::exit(1);
}

/// Launches a child command with environment variables pointing to the proxy.
async fn run_wrap(command: Vec<String>, port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let proxy_url = format!("http://127.0.0.1:{}", port);

    ensure_proxy_running(&proxy_url).await?;

    // Show status
    if let Ok(resp) = reqwest::get(&format!("{}/health", proxy_url)).await {
        if let Ok(health) = resp.json::<serde_json::Value>().await {
            let mode = if health["passthrough"].as_bool().unwrap_or(false) { "passthrough" } else { "pseudonymization" };
            eprintln!("  ✓ MirageIA active on {} (mode {})", proxy_url, mode);
        }
    }

    let (program, args) = command.split_first().expect("Empty command");

    eprintln!(
        "  -> Launching '{}' with MirageIA proxy enabled",
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

/// Connects to the proxy's /events SSE stream and displays events in real time.
async fn run_console(addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    let events_url = format!("{}/events", addr);

    ensure_proxy_running(addr).await?;

    let health: serde_json::Value = reqwest::get(&format!("{}/health", addr))
        .await?.json().await?;
    let mode = if health["passthrough"].as_bool().unwrap_or(false) { "PASSTHROUGH" } else { "PSEUDONYMIZATION" };
    let mappings = health["pii_mappings"].as_u64().unwrap_or(0);
    let version = health["version"].as_str().unwrap_or("?");
    let detection = match health["onnx_model"].as_str() {
        Some(model) => format!("regex + ONNX ({})", model),
        None => "regex only".to_string(),
    };
    eprintln!("  ╔══════════════════════════════════════════╗");
    eprintln!("  ║  MirageIA Console                       ║");
    eprintln!("  ╚══════════════════════════════════════════╝");
    eprintln!();
    eprintln!("  Version    : {}", version);
    eprintln!("  Proxy      : {}", addr);
    eprintln!("  Mode       : {}", mode);
    eprintln!("  Detection  : {}", detection);
    eprintln!("  Mappings   : {}", mappings);
    eprintln!();
    eprintln!("  Waiting for requests... (Ctrl+C to quit)");
    eprintln!("  -------------------------------------------------");

    // Connect to SSE stream
    let response = reqwest::get(&events_url).await?;
    let mut stream = response.bytes_stream();

    use futures_util::StreamExt;
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        // Process each complete SSE event
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

    // Error event — display and return
    if let Some(err) = event["error"].as_str() {
        let provider = event["provider"].as_str().unwrap_or("???");
        let path = event["path"].as_str().unwrap_or("/");
        eprintln!(
            "  \x1b[90m[{}]\x1b[0m \x1b[31m✗ ERR\x1b[0m {:<10} {}  \x1b[31m{}\x1b[0m",
            timestamp, provider, path, err
        );
        return;
    }

    let is_request = direction == "\u{2192}";

    let dir_colored = if is_request {
        format!("\x1b[36m{}\x1b[0m", direction) // cyan for request
    } else {
        format!("\x1b[32m{}\x1b[0m", direction) // green for response
    };

    let mode = if passthrough {
        "\x1b[90mPASS\x1b[0m"
    } else {
        "\x1b[35mPII \x1b[0m"
    };

    if is_request {
        // Request line: direction, mode, provider, path, model, size
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

        // PII sub-line if detected
        if pii_count > 0 && !pii_types.is_empty() {
            let types_str = pii_types.join(", ");
            eprintln!(
                "           \x1b[33m|-- {} PII: {}\x1b[0m",
                pii_count, types_str
            );
        } else if pii_count > 0 {
            eprintln!(
                "           \x1b[33m|-- {} PII detected\x1b[0m",
                pii_count
            );
        }
    } else {
        // Response line: direction, status, provider, path, latency, streaming
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

    eprintln!("Loading model '{}'...", model_name);
    let detector = PiiDetector::from_model_name(model_name)?;

    eprintln!("Analyzing text ({} characters)...", text.len());
    let entities = detector.detect(text)?;

    if entities.is_empty() {
        println!("No sensitive data detected.");
    } else {
        println!("{} entity(ies) detected:\n", entities.len());
        for entity in &entities {
            println!("  {}", entity);
        }
    }

    Ok(())
}

#[cfg(not(feature = "onnx"))]
fn run_detect(_text: &str, _model_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Error: PII detection requires the 'onnx' feature.");
    eprintln!("Recompile with: cargo run --features onnx -- detect \"your text\"");
    std::process::exit(1);
}
