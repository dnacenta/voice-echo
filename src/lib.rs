//! voice-echo — Voice interface for AI entities via Twilio.
//!
//! This crate provides a complete voice pipeline: Twilio WebSocket audio streaming,
//! voice activity detection, speech-to-text (Groq Whisper), LLM bridge (Claude),
//! and text-to-speech (Inworld). It can be used as a standalone binary or as a
//! library dependency in echo-system.
//!
//! # Usage as a library
//!
//! ```no_run
//! use voice_echo::{VoiceEcho, config::Config};
//!
//! # fn run() {
//! let config = Config::load().expect("config");
//! let mut voice = VoiceEcho::new(config);
//! // voice.start().await.expect("server");
//! # }
//! ```

pub mod api;
pub mod config;
pub mod greeting;
pub mod pipeline;
pub mod twilio;

use std::any::Any;
use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;
use echo_system_types::plugin::{Plugin, PluginContext, PluginResult, PluginRole};
use echo_system_types::{HealthStatus, PluginMeta, SetupPrompt};
use tokio::sync::Mutex;
use tower_http::trace::TraceLayer;

use config::Config;
use pipeline::audio;
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
    /// Pre-converted mu-law hold music data, if configured.
    pub hold_music: Option<Arc<Vec<u8>>>,
    /// Context for outbound calls, keyed by call_sid.
    /// Consumed on first utterance so the LLM knows why it called.
    pub call_contexts: Arc<Mutex<HashMap<String, String>>>,
}

/// The voice-echo plugin. Manages the voice pipeline lifecycle.
pub struct VoiceEcho {
    config: Config,
    state: Option<AppState>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl VoiceEcho {
    /// Create a new VoiceEcho instance from config.
    pub fn new(config: Config) -> Self {
        Self {
            config,
            state: None,
            shutdown_tx: None,
        }
    }

    /// Start the voice server. Builds state, binds the listener, and serves.
    /// This blocks until the server is shut down via `stop()`.
    pub async fn start(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let config = &self.config;

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
            claude: Arc::new(ClaudeBridge::new(
                config.claude.session_timeout_secs,
                config.claude.dangerously_skip_permissions,
                config
                    .claude
                    .self_path
                    .as_ref()
                    .map(std::path::PathBuf::from),
            )),
            twilio: Arc::new(TwilioClient::new(
                &config.twilio,
                &config.server.external_url,
            )),
            config: config.clone(),
            hold_music,
            call_contexts: Arc::new(Mutex::new(HashMap::new())),
        };

        self.state = Some(state.clone());

        let app = self.build_router(state);

        let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port)
            .parse()
            .map_err(|e| format!("Invalid server address: {e}"))?;

        tracing::info!(%addr, "Listening");

        let listener = tokio::net::TcpListener::bind(addr).await?;

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        self.shutdown_tx = Some(shutdown_tx);

        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await?;

        Ok(())
    }

    /// Stop the voice server gracefully.
    pub async fn stop(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        self.state = None;
        Ok(())
    }

    /// Report health status.
    fn health_check(&self) -> HealthStatus {
        match &self.state {
            Some(_) => HealthStatus::Healthy,
            None => HealthStatus::Down("not started".into()),
        }
    }

    /// Return the Axum router with all voice-echo routes.
    /// Returns `None` if the server hasn't been started (no state).
    pub fn routes(&self) -> Option<Router> {
        let state = self.state.as_ref()?;
        Some(self.build_router(state.clone()))
    }

    /// Configuration prompts for the echo-system init wizard.
    fn get_setup_prompts() -> Vec<SetupPrompt> {
        vec![
            SetupPrompt {
                key: "external_url".into(),
                question: "External URL (where Twilio can reach this server):".into(),
                required: true,
                secret: false,
                default: None,
            },
            SetupPrompt {
                key: "twilio_account_sid".into(),
                question: "Twilio Account SID:".into(),
                required: true,
                secret: false,
                default: None,
            },
            SetupPrompt {
                key: "twilio_auth_token".into(),
                question: "Twilio Auth Token:".into(),
                required: true,
                secret: true,
                default: None,
            },
            SetupPrompt {
                key: "twilio_phone_number".into(),
                question: "Twilio Phone Number (E.164):".into(),
                required: true,
                secret: false,
                default: None,
            },
            SetupPrompt {
                key: "groq_api_key".into(),
                question: "Groq API Key (for Whisper STT):".into(),
                required: true,
                secret: true,
                default: None,
            },
            SetupPrompt {
                key: "inworld_api_key".into(),
                question: "Inworld API Key (for TTS):".into(),
                required: true,
                secret: true,
                default: None,
            },
            SetupPrompt {
                key: "inworld_voice_id".into(),
                question: "Inworld Voice ID:".into(),
                required: false,
                secret: false,
                default: Some("Olivia".into()),
            },
            SetupPrompt {
                key: "api_token".into(),
                question: "API Token (for outbound call auth):".into(),
                required: false,
                secret: true,
                default: None,
            },
        ]
    }

    fn build_router(&self, state: AppState) -> Router {
        Router::new()
            .route("/twilio/voice", post(twilio::webhook::handle_voice))
            .route(
                "/twilio/voice/outbound",
                post(twilio::webhook::handle_voice_outbound),
            )
            .route("/twilio/media", get(twilio::media::handle_media_upgrade))
            .route("/api/call", post(api::outbound::handle_call))
            .route("/health", get(health_handler))
            .layer(TraceLayer::new_for_http())
            .with_state(state)
    }
}

/// Factory function — creates a fully initialized voice-echo plugin.
pub async fn create(
    config: &serde_json::Value,
    _ctx: &PluginContext,
) -> Result<Box<dyn Plugin>, Box<dyn std::error::Error + Send + Sync>> {
    let cfg: Config = serde_json::from_value(config.clone())?;
    Ok(Box::new(VoiceEcho::new(cfg)))
}

impl Plugin for VoiceEcho {
    fn meta(&self) -> PluginMeta {
        PluginMeta {
            name: "voice-echo".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "Voice interface via Twilio".into(),
        }
    }

    fn role(&self) -> PluginRole {
        PluginRole::Interface
    }

    fn start(&mut self) -> PluginResult<'_> {
        Box::pin(async move { self.start().await })
    }

    fn stop(&mut self) -> PluginResult<'_> {
        Box::pin(async move { self.stop().await })
    }

    fn health(&self) -> Pin<Box<dyn Future<Output = HealthStatus> + Send + '_>> {
        Box::pin(async move { self.health_check() })
    }

    fn setup_prompts(&self) -> Vec<SetupPrompt> {
        Self::get_setup_prompts()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

async fn health_handler() -> &'static str {
    "ok"
}
