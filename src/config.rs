use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub twilio: TwilioConfig,
    pub groq: GroqConfig,
    pub elevenlabs: ElevenLabsConfig,
    pub claude: ClaudeConfig,
    pub vad: VadConfig,
    #[serde(default)]
    pub api: ApiConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub external_url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TwilioConfig {
    pub account_sid: String,
    pub auth_token: String,
    pub phone_number: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct GroqConfig {
    pub api_key: String,
    #[serde(default = "default_groq_model")]
    pub model: String,
}

fn default_groq_model() -> String {
    "whisper-large-v3-turbo".to_string()
}

#[derive(Debug, Deserialize, Clone)]
pub struct ElevenLabsConfig {
    pub api_key: String,
    #[serde(default = "default_voice_id")]
    pub voice_id: String,
}

fn default_voice_id() -> String {
    "JAgnJveGGUh4qy4kh6dF".to_string()
}

#[derive(Debug, Deserialize, Clone)]
pub struct ClaudeConfig {
    #[serde(default = "default_session_timeout")]
    pub session_timeout_secs: u64,
    #[serde(default = "default_greeting")]
    pub greeting: String,
    #[serde(default)]
    pub dangerously_skip_permissions: bool,
}

fn default_session_timeout() -> u64 {
    300
}

fn default_greeting() -> String {
    "Hello, this is Morpheus".to_string()
}

#[derive(Debug, Deserialize, Clone)]
pub struct VadConfig {
    #[serde(default = "default_silence_threshold")]
    pub silence_threshold_ms: u64,
    #[serde(default = "default_energy_threshold")]
    pub energy_threshold: u16,
}

fn default_silence_threshold() -> u64 {
    1500
}

fn default_energy_threshold() -> u16 {
    50
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ApiConfig {
    /// Bearer token required for /api/* endpoints. If empty, all requests are rejected.
    #[serde(default)]
    pub token: String,
}

impl Config {
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        // Load .env file from same directory as config.toml
        let env_path = config_dir().join(".env");
        match dotenvy::from_path(&env_path) {
            Ok(()) => tracing::info!("Loaded .env from {}", env_path.display()),
            Err(dotenvy::Error::Io(_)) => {
                tracing::debug!(
                    "No .env file at {}, using environment only",
                    env_path.display()
                );
            }
            Err(e) => tracing::warn!("Failed to parse .env: {e}"),
        }

        let path = config_path();
        tracing::info!("Loading config from {}", path.display());

        let contents = std::fs::read_to_string(&path).map_err(|e| {
            format!(
                "Failed to read config at {}: {}. Copy config.example.toml to {}",
                path.display(),
                e,
                path.display()
            )
        })?;

        let mut config: Config = toml::from_str(&contents)?;

        // Allow env var overrides for secrets
        if let Ok(v) = std::env::var("TWILIO_ACCOUNT_SID") {
            config.twilio.account_sid = v;
        }
        if let Ok(v) = std::env::var("TWILIO_AUTH_TOKEN") {
            config.twilio.auth_token = v;
        }
        if let Ok(v) = std::env::var("GROQ_API_KEY") {
            config.groq.api_key = v;
        }
        if let Ok(v) = std::env::var("ELEVENLABS_API_KEY") {
            config.elevenlabs.api_key = v;
        }
        if let Ok(v) = std::env::var("MORPHEUS_API_TOKEN") {
            config.api.token = v;
        }
        if let Ok(v) = std::env::var("SERVER_EXTERNAL_URL") {
            config.server.external_url = v;
        }

        Ok(config)
    }
}

fn config_dir() -> PathBuf {
    if let Ok(p) = std::env::var("MORPHEUS_LINE_CONFIG") {
        // If pointing to a file, use its parent directory
        let path = PathBuf::from(p);
        return path.parent().map(|p| p.to_path_buf()).unwrap_or(path);
    }

    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".morpheus-line")
}

fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("MORPHEUS_LINE_CONFIG") {
        return PathBuf::from(p);
    }

    config_dir().join("config.toml")
}
