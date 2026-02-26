use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::ansi;
use super::prompts::confirm;

/// All values collected from the wizard prompts.
pub struct SetupValues {
    pub twilio_account_sid: String,
    pub twilio_auth_token: String,
    pub twilio_phone_number: String,
    pub groq_api_key: String,
    pub inworld_api_key: String,
    pub inworld_voice_id: String,
    pub external_url: String,
    pub api_token: String,
}

/// Write config.toml and .env to ~/.voice-echo/.
/// Returns the config directory path.
pub fn write_config(values: &SetupValues) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let config_dir = PathBuf::from(home).join(".voice-echo");

    println!("\n  {} Writing configuration", ansi::bold(">>"));

    // Create directory if needed
    if !config_dir.exists() {
        fs::create_dir_all(&config_dir).expect("Failed to create ~/.voice-echo");
    }

    // Write config.toml
    let config_path = config_dir.join("config.toml");
    if config_path.exists() {
        println!("  {} config.toml already exists", ansi::yellow("!"));
        if !confirm("Overwrite?") {
            println!("  Skipping config.toml");
        } else {
            write_config_toml(&config_path, values);
        }
    } else {
        write_config_toml(&config_path, values);
    }

    // Write .env
    let env_path = config_dir.join(".env");
    if env_path.exists() {
        println!("  {} .env already exists", ansi::yellow("!"));
        if !confirm("Overwrite?") {
            println!("  Skipping .env");
        } else {
            write_env_file(&env_path, values);
        }
    } else {
        write_env_file(&env_path, values);
    }

    config_dir
}

fn write_config_toml(path: &Path, values: &SetupValues) {
    let content = format!(
        r#"[server]
host = "0.0.0.0"
port = 8443
# Secrets loaded from .env (SERVER_EXTERNAL_URL)
external_url = ""

[twilio]
# Secrets loaded from .env (TWILIO_ACCOUNT_SID, TWILIO_AUTH_TOKEN)
account_sid = ""
auth_token = ""
phone_number = "{phone}"

[groq]
# Secret loaded from .env (GROQ_API_KEY)
api_key = ""
model = "whisper-large-v3-turbo"

[inworld]
# Secret loaded from .env (INWORLD_API_KEY)
api_key = ""
voice_id = "{voice_id}"
model = "inworld-tts-1.5-max"

[claude]
session_timeout_secs = 300
greeting = "Hello, this is Echo"
dangerously_skip_permissions = false

[api]
# Secret loaded from .env (ECHO_API_TOKEN)
token = ""

[vad]
silence_threshold_ms = 1500
energy_threshold = 50

# [hold_music]
# file = "/path/to/hold-music.wav"
# volume = 0.3
"#,
        phone = values.twilio_phone_number,
        voice_id = values.inworld_voice_id,
    );

    fs::write(path, content).expect("Failed to write config.toml");
    println!("  {} {}", ansi::green("\u{2713}"), path.display());
}

fn write_env_file(path: &Path, values: &SetupValues) {
    let content = format!(
        r#"# Twilio
TWILIO_ACCOUNT_SID={twilio_sid}
TWILIO_AUTH_TOKEN={twilio_token}

# Groq (Whisper STT)
GROQ_API_KEY={groq_key}

# Inworld (TTS)
INWORLD_API_KEY={inworld_key}

# API bearer token for /api/* endpoints
ECHO_API_TOKEN={api_token}

# Public URL where Twilio can reach this server
SERVER_EXTERNAL_URL={external_url}
"#,
        twilio_sid = values.twilio_account_sid,
        twilio_token = values.twilio_auth_token,
        groq_key = values.groq_api_key,
        inworld_key = values.inworld_api_key,
        api_token = values.api_token,
        external_url = values.external_url,
    );

    fs::write(path, &content).expect("Failed to write .env");

    // Set restrictive permissions: owner read/write only
    let perms = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, perms).expect("Failed to set .env permissions");

    println!(
        "  {} {} {}",
        ansi::green("\u{2713}"),
        path.display(),
        ansi::dim("(mode 0600)")
    );
}

/// Copy the current binary to /usr/local/bin/voice-echo.
pub fn install_binary() {
    let current_exe = std::env::current_exe().expect("Failed to get current executable path");
    let target = Path::new("/usr/local/bin/voice-echo");

    match fs::copy(&current_exe, target) {
        Ok(_) => {
            // Make executable
            let perms = fs::Permissions::from_mode(0o755);
            fs::set_permissions(target, perms).ok();
            println!(
                "  {} Copied to {}",
                ansi::green("\u{2713}"),
                target.display()
            );
        }
        Err(e) => {
            println!(
                "  {} Failed to copy binary: {} (try running with sudo)",
                ansi::red("\u{2717}"),
                e
            );
        }
    }
}

/// Write a systemd service unit to /etc/systemd/system/.
pub fn install_systemd() {
    let unit = r#"[Unit]
Description=voice-echo — Voice interface for Claude Code
After=network.target

[Service]
Type=simple
User=root
ExecStart=/usr/local/bin/voice-echo
Environment=RUST_LOG=voice_echo=info,tower_http=info
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
"#;

    let path = Path::new("/etc/systemd/system/voice-echo.service");
    match fs::write(path, unit) {
        Ok(_) => {
            println!("  {} {}", ansi::green("\u{2713}"), path.display());
            println!(
                "  {}",
                ansi::dim("Run: systemctl daemon-reload && systemctl enable --now voice-echo")
            );
        }
        Err(e) => {
            println!(
                "  {} Failed to write service: {} (try running with sudo)",
                ansi::red("\u{2717}"),
                e
            );
        }
    }
}

/// Write an nginx reverse proxy config for the given domain.
pub fn install_nginx(external_url: &str) {
    // Extract domain from URL
    let domain = external_url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');

    let config = format!(
        r#"server {{
    listen 443 ssl;
    server_name {domain};

    ssl_certificate /etc/letsencrypt/live/{domain}/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/{domain}/privkey.pem;

    # Twilio voice webhooks
    location /twilio/ {{
        proxy_pass http://127.0.0.1:8443;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # WebSocket support for /twilio/media
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_read_timeout 86400;
    }}

    # Outbound call API (n8n integration)
    location /api/ {{
        proxy_pass http://127.0.0.1:8443;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }}

    # Health check
    location /health {{
        proxy_pass http://127.0.0.1:8443;
    }}
}}

# Redirect HTTP to HTTPS
server {{
    listen 80;
    server_name {domain};

    location /.well-known/acme-challenge/ {{
        root /var/www/html;
    }}

    location / {{
        return 301 https://$host$request_uri;
    }}
}}
"#,
        domain = domain,
    );

    let path = Path::new("/etc/nginx/sites-available/voice-echo");

    match fs::write(path, &config) {
        Ok(_) => {
            println!("  {} {}", ansi::green("\u{2713}"), path.display());
            println!(
                "  {}",
                ansi::dim("Run: ln -sf /etc/nginx/sites-available/voice-echo /etc/nginx/sites-enabled/ && nginx -t && systemctl reload nginx")
            );
        }
        Err(e) => {
            println!(
                "  {} Failed to write nginx config: {} (try running with sudo)",
                ansi::red("\u{2717}"),
                e
            );
        }
    }
}
