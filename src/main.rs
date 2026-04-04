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
    Proxy,

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
        Some(Commands::Proxy) | None => {
            let config = AppConfig::from_env();
            init_tracing(&config.log_level);

            // Au premier lancement, proposer le setup si pas de config
            if !config_exists() {
                eprintln!("Première utilisation ? Lancez `mirageia setup` pour la configuration guidée.");
                eprintln!();
            }

            tracing::info!("MirageIA v{}", env!("CARGO_PKG_VERSION"));
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
