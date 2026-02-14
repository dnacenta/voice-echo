mod api;
mod config;
mod pipeline;
mod twilio;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;
use tower_http::trace::TraceLayer;

use config::Config;
use pipeline::claude::ClaudeBridge;
use pipeline::stt::SttClient;
use pipeline::tts::TtsClient;
use twilio::outbound::TwilioClient;

/// Shared application state accessible from all handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub stt: Arc<SttClient>,
    pub tts: Arc<TtsClient>,
    pub claude: Arc<ClaudeBridge>,
    pub twilio: Arc<TwilioClient>,
}

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "morpheus_line=info,tower_http=info".into()),
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
        "Starting morpheus-line"
    );

    // Build shared state
    let state = AppState {
        stt: Arc::new(SttClient::new(
            config.groq.api_key.clone(),
            config.groq.model.clone(),
        )),
        tts: Arc::new(TtsClient::new(
            config.elevenlabs.api_key.clone(),
            config.elevenlabs.voice_id.clone(),
        )),
        claude: Arc::new(ClaudeBridge::new(
            config.claude.session_timeout_secs,
            config.claude.dangerously_skip_permissions,
        )),
        twilio: Arc::new(TwilioClient::new(
            &config.twilio,
            &config.server.external_url,
        )),
        config: config.clone(),
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
        // Outbound call API (for n8n)
        .route("/api/call", post(api::outbound::handle_call))
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

    axum::serve(listener, app)
        .await
        .expect("Server error");
}

async fn health() -> &'static str {
    "ok"
}
