# trinity-echo

[![CI](https://github.com/dnacenta/trinity-echo/actions/workflows/ci.yml/badge.svg?branch=development)](https://github.com/dnacenta/trinity-echo/actions/workflows/ci.yml)
[![License: GPL-3.0](https://img.shields.io/github/license/dnacenta/trinity-echo)](LICENSE)
[![Version](https://img.shields.io/github/v/tag/dnacenta/trinity-echo?label=version&color=green)](https://github.com/dnacenta/trinity-echo/tags)
[![Rust](https://img.shields.io/badge/rust-1.93%2B-orange)](https://rustup.rs/)

Voice interface for Claude Code over the phone. Call in and talk to Claude, or trigger outbound calls from n8n / automation workflows.

Built in Rust. Uses Twilio for telephony, Groq Whisper for speech-to-text, ElevenLabs for text-to-speech, and the Claude Code CLI for reasoning.

## Architecture

### Voice Pipeline

```
                         ┌─────────────────────────────────────┐
                         │          trinity-echo (axum)        │
                         │                                     │
  Phone ◄──► Twilio ◄──►│  WebSocket ◄──► VAD ──► STT (Groq)  │
                         │                         │           │
                         │                    Claude CLI        │
                         │                         │           │
                         │                    TTS (ElevenLabs)  │
                         │                         │           │
                         │                    mu-law encode     │
                         └─────────────────────────────────────┘
```

### n8n Bridge — AI-Initiated Calls

```
  ┌──────────────┐    trigger     ┌──────────────────┐
  │  Any module   │──────────────►│   Orchestrator    │
  │  (slack,      │               │   reads registry, │
  │   standup,    │               │   routes to       │
  │   alerts...) │               │   target module   │
  └──────────────┘               └────────┬─────────┘
                                          │
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
                                 │  trinity-echo     │
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
  ┌─────────┐        ┌──────────┐        ┌───────────────┐
  │  Slack   │◄──────►│   n8n    │◄──────►│ claude-bridge │──► Claude CLI
  └─────────┘        │ (Docker) │        └───────────────┘
                     │          │
  ┌─────────┐        │  modules:│        ┌───────────────┐
  │ Triggers │──────►│  orchest.│──────►│ trinity-echo  │──► Claude CLI
  │ (cron,   │       │  call-   │  API   │ (Rust, axum)  │
  │  webhook,│       │  human,  │        └───────┬───────┘
  │  events) │       │  slack,  │                │
  └─────────┘        │  standup │                ▼
                     └──────────┘        ┌───────────────┐
                                         │    Twilio      │◄──► Phone
                                         └───────────────┘
```

Claude CLI is the brain, n8n is the nervous system, trinity-echo is the voice.

## How It Works

### Inbound calls

1. Someone calls your Twilio number
2. Twilio POSTs to `/twilio/voice` -- responds with TwiML that opens a media stream
3. Twilio connects a WebSocket to `/twilio/media`
4. Audio arrives as base64 mu-law chunks
5. VAD (voice activity detection) buffers audio until it detects silence after speech
6. Complete utterance is converted: mu-law -> PCM -> WAV -> Groq Whisper (STT)
7. Transcript is sent to `claude -p` (Claude Code CLI)
8. Response goes through ElevenLabs TTS -> PCM -> mu-law -> back through the WebSocket
9. Caller hears Claude's response

### Outbound calls

1. POST to `/api/call` with a phone number and optional context
2. trinity-echo calls Twilio REST API to initiate the call
3. Context is stored per `call_sid` so Claude knows *why* it's calling
4. When the recipient picks up, Twilio hits `/twilio/voice/outbound`
5. Same media stream pipeline kicks in -- Claude's first prompt includes the context
6. Context is consumed after first use, subsequent turns are normal conversation

## Prerequisites

- [Rust](https://rustup.rs/) (1.75+)
- [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code) installed and authenticated
- [Twilio](https://www.twilio.com/) account with a phone number
- [Groq](https://console.groq.com/) API key (free tier works)
- [ElevenLabs](https://elevenlabs.io/) API key (free tier: ~10k chars/month)
- A server with a public HTTPS URL (for Twilio webhooks)
- nginx (recommended, for TLS termination and WebSocket proxying)

## Installation

### 1. Clone and build

```bash
git clone https://github.com/dnacenta/trinity-echo.git
cd trinity-echo
cargo build --release
```

The binary will be at `target/release/trinity-echo`.

### 2. Configure

Copy the example files to `~/.trinity-echo/`:

```bash
mkdir -p ~/.trinity-echo
cp config.example.toml ~/.trinity-echo/config.toml
cp .env.example ~/.trinity-echo/.env
```

Edit `~/.trinity-echo/.env` with your API keys:

```bash
TWILIO_ACCOUNT_SID=AC...
TWILIO_AUTH_TOKEN=your_token
GROQ_API_KEY=gsk_...
ELEVENLABS_API_KEY=your_key
TRINITY_API_TOKEN=$(openssl rand -hex 32)
SERVER_EXTERNAL_URL=https://your-server.example.com
```

Edit `~/.trinity-echo/config.toml` -- set your Twilio phone number and adjust defaults as needed. Secrets are loaded from `.env`, so leave them empty in the TOML.

You can override the config directory with `TRINITY_ECHO_CONFIG=/path/to/config.toml`.

### 3. nginx

Copy `deploy/nginx.conf` and replace `your-server.example.com` with your domain:

```bash
sudo cp deploy/nginx.conf /etc/nginx/sites-available/trinity-echo
sudo ln -s /etc/nginx/sites-available/trinity-echo /etc/nginx/sites-enabled/
# Edit server_name and SSL cert paths
sudo nginx -t && sudo systemctl reload nginx
```

TLS is required -- Twilio only sends webhooks over HTTPS. Use certbot or similar.

### 4. systemd

```bash
sudo cp target/release/trinity-echo /usr/local/bin/
sudo cp deploy/trinity-echo.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now trinity-echo
```

### 5. Twilio webhook

In the [Twilio Console](https://console.twilio.com/), set your phone number's voice webhook to:

```
POST https://your-server.example.com/twilio/voice
```

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
| `elevenlabs`  | `api_key`              | --                        | ElevenLabs API key (overridden by env var)       |
| `elevenlabs`  | `voice_id`             | `JAgnJveGGUh4qy4kh6dF`   | ElevenLabs voice ID                              |
| `claude`      | `session_timeout_secs` | `300`                     | Conversation session timeout                     |
| `api`         | `token`                | --                        | Bearer token for `/api/*` (overridden by env var)|
| `vad`         | `silence_threshold_ms` | `1500`                    | Silence duration before utterance ends           |
| `vad`         | `energy_threshold`     | `50`                      | Minimum RMS energy to detect speech              |

### Environment variables

All secrets can be set via env vars (recommended) instead of config.toml:

| Variable               | Overrides                  |
|------------------------|----------------------------|
| `TWILIO_ACCOUNT_SID`   | `twilio.account_sid`       |
| `TWILIO_AUTH_TOKEN`    | `twilio.auth_token`        |
| `GROQ_API_KEY`         | `groq.api_key`             |
| `ELEVENLABS_API_KEY`   | `elevenlabs.api_key`       |
| `TRINITY_API_TOKEN`   | `api.token`                |
| `SERVER_EXTERNAL_URL`  | `server.external_url`      |
| `TRINITY_ECHO_CONFIG` | Config file path            |
| `RUST_LOG`             | Log level filter            |

## Usage

### Call in

Just call your Twilio number. You'll hear "Connected to Claude. Go ahead and speak." then talk normally.

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

The recipient picks up and Claude already knows why it called -- context is injected into the first prompt. The optional `message` field adds a Twilio `<Say>` greeting before the stream starts (usually not needed since Claude handles the greeting via TTS).

### n8n Bridge

trinity-echo integrates with n8n through a bridge architecture:

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

The orchestrator reads the module registry, forwards the payload to the `call-human` webhook, which calls the trinity-echo API with context. When the user picks up, Claude knows exactly what's happening.

Other modules (Slack chat, daily standup, agent reports) can also trigger calls by routing through the orchestrator. See `specs/n8n-bridge-spec.md` for the full specification.

## Costs

| Service      | Free tier                     | Paid                             |
|--------------|-------------------------------|----------------------------------|
| Twilio       | Trial credit (~$15)           | ~$1.15/mo number + per-minute    |
| Groq         | Free (rate-limited)           | Usage-based                      |
| ElevenLabs   | ~10k chars/month              | From $5/month                    |
| Claude Code  | Included with Max plan        | Or API usage                     |

For personal use with a few calls a day, the running cost is minimal beyond the Twilio number.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for branch naming, commit conventions, and workflow.

## License

[GPL-3.0](LICENSE)

## Acknowledgments

*Inspired by [NetworkChuck's claude-phone](https://github.com/networkchuck/claude-phone). Rewritten from scratch in Rust with a different architecture -- no intermediate Node.js server, direct WebSocket pipeline, energy-based VAD, and an outbound call API for automation.*
