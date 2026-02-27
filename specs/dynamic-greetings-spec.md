# Dynamic Greetings System — Spec

## Overview

Replace the static greeting system with context-aware, configurable greetings that feel natural and adapt to the call direction (inbound vs outbound).

**Key Principle**: Inbound calls are about personality (varied, warm). Outbound calls are about purpose (get to the reason immediately).

## Current State

- Single static greeting string in `config.toml` under `[claude].greeting`
- Same greeting plays for both inbound and outbound calls
- Assistant name ("Echo") and caller name ("D") are hardcoded
- Outbound calls have an optional Twilio `<Say>` pre-stream message, but the greeting still fires after the stream connects
- Context exists for outbound calls but only gets injected after the caller speaks first

## Requirements

### R1 — Configurable Identity
The assistant name and caller name must be configurable at setup time. No hardcoded identity.

### R2 — Inbound Greeting Variation
Inbound calls pick randomly from a pool of greeting templates. Templates support `{name}` substitution for the assistant name.

### R3 — Outbound Context-Aware Opening
Outbound calls skip the generic greeting entirely. Instead, the opening line is derived from the call context/reason. The assistant greets the caller by name and states why it's calling.

### R4 — Backward Compatibility
The existing `[claude].greeting` field continues to work. If the new `[greetings]` section is absent, the system falls back to the current behavior.

## Config Schema

```toml
# New section — identity fields used across the system
[identity]
name = "Echo"           # Assistant name (used in greeting templates + bridge sender)
caller_name = "D"       # Primary caller name (used in outbound greetings + bridge)

# New section — replaces [claude].greeting
[greetings]
inbound = [
    "Hello, this is {name}",
    "Hi there",
    "{name} here",
    "Hey, {name} speaking",
    "Hi, this is {name}",
]
# Template for outbound calls. {caller} = caller_name, {reason} = from call context
outbound_template = "Hey {caller}, {reason}"
# Fallback if outbound call has no context/reason
outbound_fallback = "Hey {caller}, I wanted to talk to you about something"
```

### Defaults (if `[identity]` omitted)
- `name` → `"Echo"`
- `caller_name` → `"User"`

### Defaults (if `[greetings]` omitted)
- Falls back to `[claude].greeting` if set
- If neither exists, uses `"Hello, this is {name}"` as sole inbound template
- `outbound_template` → `"Hey {caller}, {reason}"`
- `outbound_fallback` → `"Hey {caller}, I wanted to talk to you about something"`

## Architecture

```
INBOUND CALL
─────────────
  Phone → Twilio → WS connects
    → send_greeting(direction=Inbound)
      → greetings.inbound[rand]    pick random template
      → replace {name}             from config.identity.name
      → TTS → caller
    → VAD → STT → Claude → TTS

OUTBOUND CALL
──────────────
  n8n trigger → POST /api/call {to, context, reason}
    → Twilio initiates call (NO <Say> element)
    → WS connects
    → send_greeting(direction=Outbound, reason)
      → greetings.outbound_template
      → replace {caller}           from config.identity.caller_name
      → replace {reason}           from call request data
      → TTS → caller
    → Claude gets full context on first turn
      (context injected immediately, no wait for caller speech)
```

## Implementation Plan

### Phase 1 — Config Changes (`config.rs`)

1. Add `IdentityConfig` struct:
   ```rust
   #[derive(Debug, Deserialize, Clone)]
   pub struct IdentityConfig {
       #[serde(default = "default_name")]
       pub name: String,
       #[serde(default = "default_caller_name")]
       pub caller_name: String,
   }
   ```

2. Add `GreetingsConfig` struct:
   ```rust
   #[derive(Debug, Deserialize, Clone)]
   pub struct GreetingsConfig {
       #[serde(default = "default_inbound_greetings")]
       pub inbound: Vec<String>,
       #[serde(default = "default_outbound_template")]
       pub outbound_template: String,
       #[serde(default = "default_outbound_fallback")]
       pub outbound_fallback: String,
   }
   ```

3. Add both to `Config` struct with `#[serde(default)]`
4. Add backward-compat logic: if `[greetings]` is absent and `[claude].greeting` is set, use `[claude].greeting` as the sole inbound template

### Phase 2 — Greeting Logic (`twilio/media.rs`)

1. Add `GreetingDirection` enum:
   ```rust
   enum GreetingDirection {
       Inbound,
       Outbound { reason: Option<String> },
   }
   ```

2. Refactor `send_greeting()` to accept direction:
   - **Inbound**: Pick random template from `config.greetings.inbound`, substitute `{name}`
   - **Outbound**: Use `outbound_template`, substitute `{caller}` and `{reason}`. If no reason provided, use `outbound_fallback`

3. At stream start, determine direction:
   - Check if `call_contexts` has an entry for this `call_sid` → outbound
   - No entry → inbound

### Phase 3 — Outbound Flow Changes

1. **`api/outbound.rs`**: Add `reason` field to `CallRequest`:
   ```rust
   pub struct CallRequest {
       pub to: String,
       pub message: Option<String>,  // deprecated, kept for compat
       pub context: Option<String>,
       pub reason: Option<String>,   // new: short reason for greeting
   }
   ```
   Store reason alongside context in a new `CallMeta` struct.

2. **`twilio/webhook.rs`**: Remove the `<Say>` element from outbound TwiML. The greeting is now handled by the stream, not Twilio's robot voice.

3. **`twilio/media.rs`**: On stream start for outbound calls, retrieve the reason and pass `GreetingDirection::Outbound { reason }` to `send_greeting()`.

4. **Context injection**: For outbound calls, inject context into Claude's prompt immediately at greeting time rather than waiting for the caller's first utterance. This way Claude knows why it called from the start.

### Phase 4 — Bridge Client (`pipeline/bridge.rs`)

1. Replace hardcoded `"sender": "D"` with `config.identity.caller_name`
2. The `BridgeClient::new()` constructor takes the caller name from config

### Phase 5 — Config Example & Docs

1. Update `config.example.toml` with new `[identity]` and `[greetings]` sections
2. Deprecation comment on `[claude].greeting` — still works, new config preferred

## Files Changed

| File | Change |
|------|--------|
| `src/config.rs` | Add `IdentityConfig`, `GreetingsConfig`, backward compat |
| `src/twilio/media.rs` | Refactor `send_greeting()`, direction detection, random pick |
| `src/api/outbound.rs` | Add `reason` field to `CallRequest`, store `CallMeta` |
| `src/twilio/webhook.rs` | Remove `<Say>` from outbound TwiML |
| `src/pipeline/bridge.rs` | Use `config.identity.caller_name` instead of `"D"` |
| `src/main.rs` | Update `AppState` if `CallMeta` replaces plain context string |
| `config.example.toml` | Document new sections |
| `Cargo.toml` | Add `rand` crate if not already present |

## Out of Scope

- Per-caller greeting customization (multi-user support) — future feature
- Time-of-day greeting variation ("Good morning" vs "Good evening") — could add later
- Greeting A/B testing or preference learning — unnecessary complexity for now

## Migration

Zero-downtime migration. The `[claude].greeting` field continues to work. New `[identity]` and `[greetings]` sections are opt-in. Existing configs work unchanged.
