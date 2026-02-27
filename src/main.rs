mod api;
mod config;
mod discord;
mod pipeline;
pub mod registry;
mod setup;
mod twilio;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::sync::Mutex;

use axum::routing::{get, post};
use axum::Router;
use tower_http::trace::TraceLayer;

use config::Config;
use pipeline::audio;
use pipeline::bridge::BridgeClient;
use pipeline::claude::ClaudeBridge;
use pipeline::stt::SttClient;
use pipeline::tts::TtsClient;
use registry::CallRegistry;
use twilio::outbound::TwilioClient;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// How voice-echo gets Claude responses: either locally or via bridge-echo.
#[derive(Clone)]
pub enum Brain {
    /// Direct Claude Code CLI invocation (standalone mode).
    Local(Arc<ClaudeBridge>),
    /// HTTP client to bridge-echo multiplexer (multiplexed mode).
    Bridge(Arc<BridgeClient>),
}

/// Metadata for outbound calls — context for Claude and reason for the greeting.
#[derive(Clone, Debug)]
pub struct CallMeta {
    /// Full context injected into Claude's first prompt (consumed on first utterance).
    pub context: Option<String>,
    /// Short reason for calling, used in the outbound greeting TTS.
    pub reason: Option<String>,
}

/// Shared application state accessible from all handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub stt: Arc<SttClient>,
    pub tts: Arc<TtsClient>,
    pub brain: Brain,
    pub twilio: Arc<TwilioClient>,
    /// Pre-converted mu-law hold music data, if configured.
    pub hold_music: Option<Arc<Vec<u8>>>,
    /// Metadata for outbound calls, keyed by call_sid.
    /// Reason is peeked at for the greeting; context is consumed on first utterance.
    pub call_metas: Arc<Mutex<HashMap<String, CallMeta>>>,
    /// Registry of active calls for cross-channel audio injection.
    pub call_registry: CallRegistry,
}

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

    // Load config
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

    // Load hold music if configured
    let hold_music = config.hold_music.as_ref().and_then(|hm| {
        let path = std::path::Path::new(&hm.file);
        match audio::load_wav_as_mulaw(path, hm.volume) {
            Ok(data) => {
                tracing::info!(
                    path = %hm.file,
                    volume = hm.volume,
                    mulaw_bytes = data.len(),
                    "Loaded hold music"
                );
                Some(Arc::new(data))
            }
            Err(e) => {
                tracing::warn!(path = %hm.file, "Failed to load hold music: {e}");
                None
            }
        }
    });

    // Build brain — bridge-echo multiplexer if configured, local Claude otherwise
    let brain = if let Some(ref url) = config.claude.bridge_url {
        tracing::info!(url = %url, "Using bridge-echo multiplexer");
        Brain::Bridge(Arc::new(BridgeClient::new(
            url,
            config.identity.caller_name.clone(),
        )))
    } else {
        tracing::info!("Using local Claude Code CLI");
        Brain::Local(Arc::new(ClaudeBridge::new(
            config.claude.session_timeout_secs,
            config.claude.dangerously_skip_permissions,
            config
                .claude
                .self_path
                .as_ref()
                .map(std::path::PathBuf::from),
        )))
    };

    // Build shared state
    let state = AppState {
        stt: Arc::new(SttClient::new(
            config.groq.api_key.clone(),
            config.groq.model.clone(),
        )),
        tts: Arc::new(TtsClient::new(
            config.inworld.api_key.clone(),
            config.inworld.voice_id.clone(),
            config.inworld.model.clone(),
        )),
        brain,
        twilio: Arc::new(TwilioClient::new(
            &config.twilio,
            &config.server.external_url,
        )),
        config: config.clone(),
        hold_music,
        call_metas: Arc::new(Mutex::new(HashMap::new())),
        call_registry: CallRegistry::new(),
    };

    // Build router
    let app = Router::new()
        // Twilio webhooks
        .route("/twilio/voice", post(twilio::webhook::handle_voice))
        .route(
            "/twilio/voice/outbound",
            post(twilio::webhook::handle_voice_outbound),
        )
        // Twilio media stream (WebSocket)
        .route("/twilio/media", get(twilio::media::handle_media_upgrade))
        // Discord voice sidecar stream (WebSocket)
        .route(
            "/discord-stream",
            get(discord::stream::handle_discord_upgrade),
        )
        // Outbound call API (for n8n)
        .route("/api/call", post(api::outbound::handle_call))
        // Inject audio into active call (for bridge-echo cross-channel routing)
        .route("/api/inject", post(api::inject::handle_inject))
        // Health check
        .route("/health", get(health))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // Start server
    let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port)
        .parse()
        .expect("Invalid server address");

    tracing::info!(%addr, "Listening");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind");

    axum::serve(listener, app).await.expect("Server error");
}

async fn health() -> &'static str {
    "ok"
}
