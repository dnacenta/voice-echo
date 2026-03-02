mod setup;

use voice_echo::config::Config;
use voice_echo::VoiceEcho;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("--setup") => setup::run(),
        Some("--version") => println!("voice-echo {VERSION}"),
        Some("--help") | Some("-h") => print_usage(),
        Some(other) => {
            eprintln!("Unknown option: {other}");
            print_usage();
            std::process::exit(1);
        }
        None => {
            let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
            rt.block_on(server());
        }
    }
}

fn print_usage() {
    println!("voice-echo {VERSION}");
    println!("Voice interface for Claude Code via Twilio");
    println!();
    println!("Usage: voice-echo [OPTIONS]");
    println!();
    println!("Options:");
    println!("  --setup     Run interactive configuration wizard");
    println!("  --version   Print version");
    println!("  --help, -h  Print this help message");
    println!();
    println!("Without options, starts the voice server.");
}

async fn server() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "voice_echo=info,tower_http=info".into()),
        )
        .init();

    let config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load config: {e}");
            std::process::exit(1);
        }
    };

    tracing::info!(
        host = %config.server.host,
        port = config.server.port,
        "Starting voice-echo"
    );

    let mut voice = VoiceEcho::new(config);

    if let Err(e) = voice.start().await {
        tracing::error!("Server error: {e}");
        std::process::exit(1);
    }
}
