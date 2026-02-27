use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axum::extract::ws::Message;
use base64::Engine;
use tokio::sync::{mpsc, Mutex};

/// Audio transport type for a registered call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// Twilio media stream — audio wrapped in JSON event envelope.
    Twilio,
    /// Discord voice sidecar — plain mu-law frames, no wrapping.
    Discord,
}

/// A registered active call with handles to inject audio.
pub struct ActiveCall {
    pub stream_sid: String,
    pub response_tx: mpsc::Sender<Message>,
    pub speaking: Arc<AtomicBool>,
}

/// Thread-safe handle to an active call's resources.
#[derive(Clone)]
pub struct CallEntry {
    pub stream_sid: String,
    pub transport: Transport,
    response_tx: mpsc::Sender<Message>,
    speaking: Arc<AtomicBool>,
}

impl CallEntry {
    pub fn set_speaking(&self, value: bool) {
        self.speaking.store(value, Ordering::Relaxed);
    }
}

/// Registry of active calls, keyed by call_sid.
///
/// Allows the inject endpoint to look up an active call and push
/// TTS audio into it without going through the normal pipeline.
#[derive(Clone)]
pub struct CallRegistry {
    inner: Arc<Mutex<HashMap<String, CallEntry>>>,
}

impl Default for CallRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CallRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a new active call.
    pub async fn register(
        &self,
        call_sid: String,
        stream_sid: String,
        transport: Transport,
        response_tx: mpsc::Sender<Message>,
        speaking: Arc<AtomicBool>,
    ) {
        tracing::info!(
            call_sid = %call_sid,
            stream_sid = %stream_sid,
            transport = ?transport,
            "Call registered"
        );
        self.inner.lock().await.insert(
            call_sid,
            CallEntry {
                stream_sid,
                transport,
                response_tx,
                speaking,
            },
        );
    }

    /// Deregister a call when it ends.
    pub async fn deregister(&self, call_sid: &str) {
        if self.inner.lock().await.remove(call_sid).is_some() {
            tracing::info!(call_sid = %call_sid, "Call deregistered");
        }
    }

    /// Look up an active call by call_sid.
    pub async fn get(&self, call_sid: &str) -> Option<CallEntry> {
        self.inner.lock().await.get(call_sid).cloned()
    }

    /// Send mu-law audio frames into an active call.
    ///
    /// Dispatches based on transport type:
    /// - Twilio: wraps in JSON event envelope with base64 payload + mark event
    /// - Discord: sends plain mu-law chunks as binary + JSON mark event
    pub async fn send_audio(
        entry: &CallEntry,
        mulaw_bytes: &[u8],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        match entry.transport {
            Transport::Twilio => {
                for chunk in mulaw_bytes.chunks(160) {
                    let b64 = base64::engine::general_purpose::STANDARD.encode(chunk);
                    let msg = serde_json::json!({
                        "event": "media",
                        "streamSid": entry.stream_sid,
                        "media": { "payload": b64 }
                    });
                    entry
                        .response_tx
                        .send(Message::Text(msg.to_string().into()))
                        .await?;
                }

                // Mark so Twilio knows when playback ends (resets VAD via Mark handler)
                let mark = serde_json::json!({
                    "event": "mark",
                    "streamSid": entry.stream_sid,
                    "mark": { "name": "inject_end" }
                });
                entry
                    .response_tx
                    .send(Message::Text(mark.to_string().into()))
                    .await?;
            }
            Transport::Discord => {
                // Discord sidecar expects plain JSON audio messages (no Twilio envelope)
                for chunk in mulaw_bytes.chunks(160) {
                    let b64 = base64::engine::general_purpose::STANDARD.encode(chunk);
                    let msg = serde_json::json!({
                        "type": "audio",
                        "audio": b64
                    });
                    entry
                        .response_tx
                        .send(Message::Text(msg.to_string().into()))
                        .await?;
                }

                // Mark so the discord stream handler resets VAD
                let mark = serde_json::json!({ "type": "mark" });
                entry
                    .response_tx
                    .send(Message::Text(mark.to_string().into()))
                    .await?;
            }
        }

        Ok(())
    }
}
