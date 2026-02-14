# morpheus-line

[![CI](https://github.com/dnacenta/morpheus-line/actions/workflows/ci.yml/badge.svg?branch=development)](https://github.com/dnacenta/morpheus-line/actions/workflows/ci.yml)
[![License: GPL-3.0](https://img.shields.io/github/license/dnacenta/morpheus-line)](LICENSE)
[![Version](https://img.shields.io/github/v/tag/dnacenta/morpheus-line?label=version&color=green)](https://github.com/dnacenta/morpheus-line/tags)
[![Rust](https://img.shields.io/badge/rust-1.93%2B-orange)](https://rustup.rs/)

Voice interface for Claude Code over the phone. Call in and talk to Claude, or trigger outbound calls from n8n / automation workflows.

Built in Rust. Uses Twilio for telephony, Groq Whisper for speech-to-text, ElevenLabs for text-to-speech, and the Claude Code CLI for reasoning.

## Architecture

```
                         ┌─────────────────────────────────────┐
                         │          morpheus-line (axum)        │
                         │                                     │
  Phone ◄──► Twilio ◄──►│  WebSocket ◄──► VAD ──► STT (Groq)  │
                         │                         │           │
                         │                    Claude CLI        │
                         │                         │           │
                         │                    TTS (ElevenLabs)  │
                         │                         │           │
                         │                    mu-law encode     │
                         └─────────────────────────────────────┘
                                        ▲
                                        │ POST /api/call
                                   n8n / curl
```

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

1. POST to `/api/call` with a phone number and optional initial message
2. morpheus-line calls Twilio REST API to initiate the call
3. When the recipient picks up, Twilio hits `/twilio/voice/outbound`
4. Same media stream pipeline kicks in -- full duplex conversation with Claude

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
git clone https://github.com/dnacenta/morpheus-line.git
cd morpheus-line
cargo build --release
```

The binary will be at `target/release/morpheus-line`.

### 2. Configure

Copy the example files to `~/.morpheus-line/`:

```bash
mkdir -p ~/.morpheus-line
cp config.example.toml ~/.morpheus-line/config.toml
cp .env.example ~/.morpheus-line/.env
```

Edit `~/.morpheus-line/.env` with your API keys:

```bash
TWILIO_ACCOUNT_SID=AC...
TWILIO_AUTH_TOKEN=your_token
GROQ_API_KEY=gsk_...
ELEVENLABS_API_KEY=your_key
MORPHEUS_API_TOKEN=$(openssl rand -hex 32)
SERVER_EXTERNAL_URL=https://your-server.example.com
```

Edit `~/.morpheus-line/config.toml` -- set your Twilio phone number and adjust defaults as needed. Secrets are loaded from `.env`, so leave them empty in the TOML.

You can override the config directory with `MORPHEUS_LINE_CONFIG=/path/to/config.toml`.

### 3. nginx

Copy `deploy/nginx.conf` and replace `your-server.example.com` with your domain:

```bash
sudo cp deploy/nginx.conf /etc/nginx/sites-available/morpheus-line
sudo ln -s /etc/nginx/sites-available/morpheus-line /etc/nginx/sites-enabled/
# Edit server_name and SSL cert paths
sudo nginx -t && sudo systemctl reload nginx
```

TLS is required -- Twilio only sends webhooks over HTTPS. Use certbot or similar.

### 4. systemd

```bash
sudo cp target/release/morpheus-line /usr/local/bin/
sudo cp deploy/morpheus-line.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now morpheus-line
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
| `MORPHEUS_API_TOKEN`   | `api.token`                |
| `SERVER_EXTERNAL_URL`  | `server.external_url`      |
| `MORPHEUS_LINE_CONFIG` | Config file path            |
| `RUST_LOG`             | Log level filter            |

## Usage

### Call in

Just call your Twilio number. You'll hear "Connected to Claude. Go ahead and speak." then talk normally.

### Trigger an outbound call

```bash
curl -X POST https://your-server.example.com/api/call \
  -H "Authorization: Bearer YOUR_API_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"to": "+34612345678", "message": "Server CPU at 95 percent"}'
```

The recipient picks up, hears the initial message (if provided), then can talk to Claude.

### n8n integration

Use an HTTP Request node in n8n to POST to `/api/call`. Set the bearer token in the header. Useful for alerting workflows -- Claude can explain what's happening when the call connects.

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
