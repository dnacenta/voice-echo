# voice-echo

[![CI](https://github.com/dnacenta/voice-echo/actions/workflows/ci.yml/badge.svg?branch=development)](https://github.com/dnacenta/voice-echo/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/github/license/dnacenta/voice-echo)](LICENSE)
[![Version](https://img.shields.io/github/v/tag/dnacenta/voice-echo?label=version&color=green)](https://github.com/dnacenta/voice-echo/tags)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange)](https://rustup.rs/)

Voice interface for Claude Code over the phone. Call in and talk to Claude, or trigger outbound calls from n8n / automation workflows.

Built in Rust. Uses Twilio for telephony, Groq Whisper for speech-to-text, Inworld for text-to-speech, and the Claude Code CLI for reasoning.

## Architecture

### Voice Pipeline

```
                         ┌─────────────────────────────────────┐
                         │          voice-echo (axum)        │
                         │                                     │
  Phone ◄──► Twilio ◄──►│  WebSocket ◄──► VAD ──► STT (Groq)  │
                         │                         │           │
                         │                    Claude CLI        │
                         │                         │           │
                         │                    TTS (Inworld)     │
                         │                     mulaw 8kHz       │
                         └─────────────────────────────────────┘
```

### AI-Initiated Outbound Calls (n8n Bridge)

```
  ┌──────────────┐    trigger     ┌──────────────────┐
  │  Any n8n      │──────────────►│   Orchestrator    │
  │  workflow     │               │   reads registry, │
  │  (alerts,     │               │   routes to       │
  │   cron,       │               │   target module   │
  │   events...) │               └────────┬─────────┘
  └──────────────┘                        │
                                          ▼
                                 ┌──────────────────┐
                                 │   call-human      │
                                 │   builds request, │
                                 │   passes context  │
                                 └────────┬─────────┘
                                          │
                                          │ POST /api/call
                                          │ { to, context }
                                          ▼
                                 ┌──────────────────┐
                                 │  voice-echo     │
                                 │  stores context   │──► Twilio ──► Phone rings
                                 │  per call_sid     │
                                 └────────┬─────────┘
                                          │
                                          │ caller picks up
                                          ▼
                                 ┌──────────────────┐
                                 │  Claude CLI       │
                                 │  first prompt     │
                                 │  includes context │
                                 │  "I'm calling     │
                                 │   because..."     │
                                 └──────────────────┘
```

### Full System

```
                     ┌──────────┐
  ┌─────────┐        │   n8n    │        ┌───────────────┐
  │ Triggers │──────►│ (Docker) │──────►│ voice-echo  │──► Claude CLI
  │ (cron,   │       │          │  API   │ (Rust, axum)  │
  │  webhook,│       │  orchest.│        └───────┬───────┘
  │  alerts, │       │  call-   │                │
  │  events) │       │  human   │                ▼
  └─────────┘        └──────────┘        ┌───────────────┐
                                         │    Twilio      │◄──► Phone
                                         └───────────────┘
```

## Prerequisites

- [Rust](https://rustup.rs/) (1.80+)
- [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code) installed and authenticated
- [Twilio](https://www.twilio.com/) account with a phone number
- [Groq](https://console.groq.com/) API key (free tier works)
- [Inworld](https://inworld.ai/tts) API key (sign up at platform.inworld.ai)
- A server with a public HTTPS URL (for Twilio webhooks)
- nginx (recommended, for TLS termination and WebSocket proxying)

## Installation

### 1. Clone and build

```bash
git clone https://github.com/dnacenta/voice-echo.git
cd voice-echo
cargo build --release
```

### 2. Run the setup wizard

```bash
./target/release/voice-echo --setup
```

The wizard walks you through the entire setup:

- Checks that `rustc`, `claude`, and `openssl` are available
- Prompts for Twilio, Groq, and Inworld credentials (masked input)
- Asks for your server's external URL
- Generates an API token for the outbound call endpoint
- Writes `~/.voice-echo/config.toml`
- Optionally copies the binary to `/usr/local/bin/`, installs a systemd service, and generates an nginx reverse proxy config

If you skip the optional steps during the wizard, you can always set them up manually using the templates in `deploy/`.

### 3. Twilio webhook

In the [Twilio Console](https://console.twilio.com/), set your phone number's voice webhook to:

```
POST https://your-server.example.com/twilio/voice
```

### 4. Start

```bash
voice-echo
```

Or if you installed the systemd service:

```bash
sudo systemctl enable --now voice-echo
```

### Manual configuration

If you prefer to skip the wizard and configure by hand:

```bash
mkdir -p ~/.voice-echo
cp config.example.toml ~/.voice-echo/config.toml
cp .env.example ~/.voice-echo/.env
chmod 600 ~/.voice-echo/.env
```

Edit `.env` with your API keys, and `config.toml` for your Twilio phone number and other settings. Secrets are loaded from `.env`, so leave them empty in the TOML. See `deploy/nginx.conf` and `deploy/voice-echo.service` for server setup templates.

You can override the config directory with `ECHO_CONFIG=/path/to/config.toml`.

## Configuration Reference

### config.toml

| Section       | Field                  | Default                   | Description                                      |
|---------------|------------------------|---------------------------|--------------------------------------------------|
| `server`      | `host`                 | --                        | Bind address (e.g. `0.0.0.0`)                    |
| `server`      | `port`                 | --                        | Bind port (e.g. `8443`)                          |
| `server`      | `external_url`         | --                        | Public HTTPS URL (overridden by `SERVER_EXTERNAL_URL` env var) |
| `twilio`      | `account_sid`          | --                        | Twilio Account SID (overridden by env var)       |
| `twilio`      | `auth_token`           | --                        | Twilio Auth Token (overridden by env var)        |
| `twilio`      | `phone_number`         | --                        | Your Twilio phone number (E.164)                 |
| `groq`        | `api_key`              | --                        | Groq API key (overridden by env var)             |
| `groq`        | `model`                | `whisper-large-v3-turbo`  | Whisper model to use                             |
| `inworld`     | `api_key`              | --                        | Inworld API key (overridden by env var)          |
| `inworld`     | `voice_id`             | `Olivia`                  | Inworld voice name                               |
| `inworld`     | `model`                | `inworld-tts-1.5-max`    | Inworld TTS model                                |
| `claude`      | `session_timeout_secs` | `300`                     | Conversation session timeout                     |
| `claude`      | `greeting`             | `Hello, this is Echo`  | Initial TTS greeting when a call connects        |
| `claude`      | `dangerously_skip_permissions` | `false`           | Allow Claude CLI to run tools without prompting (see [Customizing Claude](#customizing-claude)) |
| `api`         | `token`                | --                        | Bearer token for `/api/*` (overridden by env var)|
| `vad`         | `silence_threshold_ms` | `1500`                    | Silence duration before utterance ends           |
| `vad`         | `energy_threshold`     | `50`                      | Minimum RMS energy to detect speech              |
| `hold_music`  | `file`                 | --                        | Optional path to a WAV file for hold music       |
| `hold_music`  | `volume`               | `0.3`                     | Playback volume (0.0 to 1.0)                     |

### Environment variables

All secrets can be set via env vars (recommended) instead of config.toml:

| Variable               | Overrides                  |
|------------------------|----------------------------|
| `TWILIO_ACCOUNT_SID`   | `twilio.account_sid`       |
| `TWILIO_AUTH_TOKEN`    | `twilio.auth_token`        |
| `GROQ_API_KEY`         | `groq.api_key`             |
| `INWORLD_API_KEY`      | `inworld.api_key`          |
| `ECHO_API_TOKEN`   | `api.token`                |
| `SERVER_EXTERNAL_URL`  | `server.external_url`      |
| `ECHO_CONFIG` | Config file path            |
| `RUST_LOG`             | Log level filter (e.g. `voice_echo=debug,tower_http=debug`) |

## Customizing Claude

voice-echo spawns the `claude` CLI for each conversation. Claude Code reads a `CLAUDE.md` file from the working directory to set its behavior — this is how you turn generic Claude into your personalized voice assistant.

Create a `CLAUDE.md` in the directory where voice-echo runs (typically the project root or the home directory of the service user). This file should contain instructions tailored for a voice context:

- **Persona**: Define who Claude is on the phone — name, tone, personality.
- **Voice-first rules**: Tell Claude to never use markdown, bullet points, numbered lists, or any text formatting. Everything it outputs will be spoken aloud via TTS.
- **Brevity**: Phone calls are not lectures. Two to four sentences per response is usually enough.
- **Language**: If you want multilingual support, specify which languages and when to switch.
- **Capabilities**: Define what Claude can and can't do — run commands, access APIs, check services, etc.
- **Boundaries**: Set security rules, topics to avoid, or information to never disclose.

Without a `CLAUDE.md`, Claude will behave as its default self — functional but generic.

### Permissions

Claude Code normally prompts for permission before running tools (shell commands, file edits, etc.). On a phone call there's no terminal to approve prompts, so you have two options:

1. **`dangerously_skip_permissions = true`** in config.toml — Claude runs all tools without asking. Powerful but risky. Only use this if you trust the instructions in your `CLAUDE.md` and have locked down what Claude can access.
2. **Pre-approve tools** via Claude Code's `settings.json` or `allowedTools` configuration. This gives you granular control over which tools are auto-approved without blanket permission.

See the [Claude Code documentation](https://docs.anthropic.com/en/docs/claude-code) for details on permission configuration.

## Server Setup

### TLS certificates

Twilio requires HTTPS for webhooks. If you're using nginx (recommended), get a free certificate with [certbot](https://certbot.eff.org/):

```bash
sudo apt install certbot python3-certbot-nginx
sudo certbot --nginx -d your-server.example.com
```

Certificates auto-renew via a systemd timer. The nginx template in `deploy/nginx.conf` is already configured for the Let's Encrypt certificate paths.

### systemd

The included service file (`deploy/voice-echo.service`) runs as root for simplicity. For production, consider creating a dedicated user:

```bash
sudo useradd -r -s /usr/sbin/nologin voice-echo
```

Then update `User=voice-echo` in the service file and ensure the user has read access to `~/.voice-echo/` and the `claude` CLI.

## Usage

### Call in

Just call your Twilio number. You'll hear the configured greeting, then talk normally.

### Trigger an outbound call

```bash
curl -X POST https://your-server.example.com/api/call \
  -H "Authorization: Bearer YOUR_API_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "to": "+34612345678",
    "context": "Server CPU at 95% for the last 10 minutes. Top processes: n8n 45%, claude 30%."
  }'
```

The recipient picks up and Claude already knows why it called — context is injected into the first prompt.

#### `POST /api/call`

Requires `Authorization: Bearer <token>` header.

| Field     | Type   | Required | Description                                                                 |
|-----------|--------|----------|-----------------------------------------------------------------------------|
| `to`      | string | yes      | Phone number in E.164 format (e.g. `+34612345678`)                         |
| `context` | string | no       | Injected into Claude's first prompt so it knows why it's calling            |
| `message` | string | no       | Twilio `<Say>` greeting before the stream starts (usually not needed since Claude handles the greeting via TTS) |

### n8n Bridge

voice-echo integrates with n8n through a bridge architecture:

- **Orchestrator** -- central webhook that routes triggers to registered modules
- **Modules** -- individual workflows managed via a JSON registry
- **call-human** -- module that triggers outbound calls with context

Trigger a call from any n8n workflow via the orchestrator:

```bash
curl -X POST http://localhost:5678/webhook/orchestrator \
  -H "Content-Type: application/json" \
  -H "X-Bridge-Secret: YOUR_BRIDGE_SECRET" \
  -d '{
    "action": "trigger",
    "module": "call-human",
    "data": {
      "reason": "Server CPU critical",
      "context": "CPU at 95% for 10 minutes. Load average 12.5.",
      "urgency": "high"
    }
  }'
```

The orchestrator reads the module registry, forwards the payload to the `call-human` webhook, which calls the voice-echo API with context. When the user picks up, Claude knows exactly what's happening.

Any n8n workflow can trigger calls by routing through the orchestrator. See `specs/n8n-bridge-spec.md` for the full specification.

## Costs

| Service      | Free tier                     | Paid                             |
|--------------|-------------------------------|----------------------------------|
| Twilio       | Trial credit (~$15)           | ~$1.15/mo number + per-minute    |
| Groq         | Free (rate-limited)           | Usage-based                      |
| Inworld TTS  | Free tier available           | ~$5/1M chars                     |
| Claude Code  | Included with Max plan        | Or API usage                     |

For personal use with a few calls a day, the running cost is minimal beyond the Twilio number.

## Troubleshooting

**Twilio returns a 502 or "connection refused"**
Twilio can't reach your server. Verify nginx is running, your DNS points to the server, and the TLS certificate is valid. Test with `curl -I https://your-server.example.com/health`.

**WebSocket closes immediately**
Check that nginx has WebSocket proxying enabled (the `Upgrade` and `Connection` headers in `deploy/nginx.conf`). Also check `proxy_read_timeout` — Twilio media streams are long-lived.

**"Failed to load config" on startup**
The config file is missing or malformed. Run `voice-echo --setup` to generate it, or manually copy `config.example.toml` to `~/.voice-echo/config.toml`.

**Claude doesn't respond or times out**
Make sure the `claude` CLI is installed, in `PATH`, and authenticated. Run `claude --version` and `claude "hello"` manually to verify. If running as a systemd service, ensure the service user's `PATH` includes the Claude binary.

**No audio / silence after speaking**
The VAD energy threshold may be too high for your microphone or phone quality. Lower `vad.energy_threshold` (try `30` or `20`). Check `RUST_LOG=voice_echo=debug` for VAD activity logs.

**TTS sounds robotic or uses the wrong voice**
Verify your `inworld.voice_id` is valid. Preview voices at the [Inworld TTS Playground](https://inworld.ai/tts). You can also create custom voices in Inworld Studio.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for branch naming, commit conventions, and workflow.

## License

[MIT](LICENSE)

## Acknowledgments

*Inspired by [NetworkChuck's claude-phone](https://github.com/networkchuck/claude-phone). Rewritten from scratch in Rust with a different architecture -- no intermediate Node.js server, direct WebSocket pipeline, energy-based VAD, and an outbound call API for automation.*
