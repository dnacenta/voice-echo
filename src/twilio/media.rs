use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use base64::Engine;
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::pipeline::{audio, vad::VoiceActivityDetector};
use crate::AppState;

/// Twilio Media Stream WebSocket event types.
#[derive(Debug, Deserialize)]
#[serde(tag = "event")]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
enum StreamEvent {
    Connected {
        #[serde(default)]
        protocol: Option<String>,
    },
    Start {
        #[serde(rename = "streamSid")]
        stream_sid: String,
        start: StartMetadata,
    },
    Media {
        #[serde(rename = "streamSid")]
        stream_sid: String,
        media: MediaPayload,
    },
    Mark {
        #[serde(rename = "streamSid")]
        stream_sid: String,
    },
    Stop {
        #[serde(rename = "streamSid")]
        stream_sid: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct StartMetadata {
    call_sid: String,
    #[serde(default)]
    media_format: Option<MediaFormat>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct MediaFormat {
    #[serde(default)]
    encoding: Option<String>,
    #[serde(default, rename = "sampleRate")]
    sample_rate: Option<u32>,
    #[serde(default)]
    channels: Option<u8>,
}

#[derive(Debug, Deserialize)]
struct MediaPayload {
    payload: String, // base64-encoded mu-law audio
}

/// WebSocket upgrade handler for GET /twilio/media.
pub async fn handle_media_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_media_stream(socket, state))
}

/// Process the Twilio media stream WebSocket connection.
///
/// Uses `tokio::select!` to multiplex between receiving audio from Twilio
/// and sending pipeline responses back. The pipeline runs in spawned tasks
/// so we keep reading incoming audio while STT/Claude/TTS are processing.
async fn handle_media_stream(mut socket: WebSocket, state: AppState) {
    tracing::info!("Twilio media stream connected");

    // Channel for pipeline tasks to queue outbound messages
    let (response_tx, mut response_rx) = mpsc::channel::<Message>(64);

    let mut vad = VoiceActivityDetector::new(
        state.config.vad.energy_threshold,
        state.config.vad.silence_threshold_ms,
    );
    let mut call_sid = String::new();
    let mut stream_sid = String::new();

    loop {
        tokio::select! {
            // Receive from Twilio
            ws_msg = socket.recv() => {
                let msg = match ws_msg {
                    Some(Ok(Message::Text(text))) => text,
                    Some(Ok(Message::Close(_))) | None => {
                        tracing::info!("Media stream closed");
                        break;
                    }
                    Some(Err(e)) => {
                        tracing::error!("WebSocket error: {e}");
                        break;
                    }
                    _ => continue,
                };

                let event: StreamEvent = match serde_json::from_str(&msg) {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::warn!("Failed to parse stream event: {e}");
                        continue;
                    }
                };

                match event {
                    StreamEvent::Connected { .. } => {
                        tracing::info!("Stream connected");
                    }
                    StreamEvent::Start { stream_sid: sid, start } => {
                        call_sid = start.call_sid.clone();
                        stream_sid = sid;
                        tracing::info!(
                            call_sid = %call_sid,
                            stream_sid = %stream_sid,
                            "Stream started"
                        );

                        // Send greeting via TTS
                        let tx = response_tx.clone();
                        let sid = stream_sid.clone();
                        let st = state.clone();
                        tokio::spawn(async move {
                            if let Err(e) = send_greeting(&sid, &st, &tx).await {
                                tracing::error!("Failed to send greeting: {e}");
                            }
                        });
                    }
                    StreamEvent::Media { media, .. } => {
                        let mulaw_bytes = match base64::engine::general_purpose::STANDARD
                            .decode(&media.payload)
                        {
                            Ok(b) => b,
                            Err(e) => {
                                tracing::warn!("Failed to decode base64 audio: {e}");
                                continue;
                            }
                        };

                        if let Some(pcm_utterance) = vad.feed(&mulaw_bytes) {
                            tracing::info!(
                                call_sid = %call_sid,
                                samples = pcm_utterance.len(),
                                "Utterance detected, processing pipeline"
                            );

                            // Spawn pipeline so we don't block the reader
                            let tx = response_tx.clone();
                            let sid = stream_sid.clone();
                            let csid = call_sid.clone();
                            let st = state.clone();

                            tokio::spawn(async move {
                                if let Err(e) = process_utterance(
                                    &pcm_utterance, &csid, &sid, &st, &tx,
                                ).await {
                                    tracing::error!(call_sid = %csid, "Pipeline error: {e}");
                                    if let Err(e) = send_error_message(&sid, &st, &tx).await {
                                        tracing::error!("Failed to send error message: {e}");
                                    }
                                }
                            });
                        }
                    }
                    StreamEvent::Mark { .. } => {
                        tracing::debug!("Mark received");
                    }
                    StreamEvent::Stop { .. } => {
                        tracing::info!(call_sid = %call_sid, "Stream stopped");
                        state.claude.end_session(&call_sid).await;
                        break;
                    }
                }
            }

            // Send queued pipeline responses back to Twilio
            Some(msg) = response_rx.recv() => {
                if let Err(e) = socket.send(msg).await {
                    tracing::error!("Failed to send response to Twilio: {e}");
                    break;
                }
            }
        }
    }
}

/// Full pipeline: PCM → WAV → STT → Claude → TTS → mu-law → channel.
async fn process_utterance(
    pcm_data: &[i16],
    call_sid: &str,
    stream_sid: &str,
    state: &AppState,
    tx: &mpsc::Sender<Message>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // 1. PCM → WAV
    let wav_data = audio::pcm_to_wav(pcm_data)?;
    tracing::debug!(wav_bytes = wav_data.len(), "Encoded WAV");

    // 2. WAV → Text (Groq Whisper)
    let transcript = state.stt.transcribe(wav_data).await?;
    let trimmed = transcript.trim();
    if trimmed.is_empty() {
        tracing::debug!("Empty transcript, skipping");
        return Ok(());
    }
    if is_whisper_hallucination(trimmed) {
        tracing::debug!(transcript = %trimmed, "Filtered whisper hallucination");
        return Ok(());
    }
    tracing::info!(call_sid, transcript = %trimmed, "Transcribed");

    // 3. Text → Claude response
    let response = state.claude.send(call_sid, &transcript).await?;
    tracing::info!(call_sid, response_len = response.len(), "Claude response");

    // 4. Response → TTS audio
    let tts_pcm_bytes = state.tts.synthesize(&response).await?;
    tracing::debug!(tts_bytes = tts_pcm_bytes.len(), "TTS audio generated");

    // 5. Convert to mu-law and send back
    send_audio(stream_sid, &tts_pcm_bytes, tx).await?;

    Ok(())
}

/// Send raw PCM bytes (little-endian i16) as mu-law media messages via the channel.
async fn send_audio(
    stream_sid: &str,
    pcm_bytes: &[u8],
    tx: &mpsc::Sender<Message>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let pcm_samples: Vec<i16> = pcm_bytes
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    let mulaw_data = audio::encode_mulaw(&pcm_samples);

    // Send in ~20ms chunks (160 samples at 8kHz)
    for chunk in mulaw_data.chunks(160) {
        let b64 = base64::engine::general_purpose::STANDARD.encode(chunk);
        let msg = serde_json::json!({
            "event": "media",
            "streamSid": stream_sid,
            "media": { "payload": b64 }
        });
        tx.send(Message::Text(msg.to_string().into())).await?;
    }

    // Mark so Twilio knows when playback ends
    let mark = serde_json::json!({
        "event": "mark",
        "streamSid": stream_sid,
        "mark": { "name": "response_end" }
    });
    tx.send(Message::Text(mark.to_string().into())).await?;

    Ok(())
}

/// Known Whisper hallucinations — phrases it generates from silence/noise.
const WHISPER_HALLUCINATIONS: &[&str] = &[
    "thank you",
    "thank you.",
    "thanks for watching",
    "thanks for watching.",
    "thank you for watching",
    "thank you for watching.",
    "subscribe",
    "like and subscribe",
    "bye",
    "bye.",
    "bye bye",
    "bye bye.",
    "you",
    "you.",
    "the end",
    "the end.",
    "so",
    "...",
    "eh",
    "hmm",
    "uh",
    "oh",
];

fn is_whisper_hallucination(transcript: &str) -> bool {
    let lower = transcript.to_lowercase();
    WHISPER_HALLUCINATIONS.iter().any(|h| lower == *h)
}

/// Speak the configured greeting when a call connects.
async fn send_greeting(
    stream_sid: &str,
    state: &AppState,
    tx: &mpsc::Sender<Message>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let greeting = &state.config.claude.greeting;
    if greeting.is_empty() {
        return Ok(());
    }
    tracing::info!("Sending greeting");
    let pcm_bytes = state.tts.synthesize(greeting).await?;
    send_audio(stream_sid, &pcm_bytes, tx).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_known_hallucinations() {
        assert!(is_whisper_hallucination("thank you"));
        assert!(is_whisper_hallucination("Thank You"));
        assert!(is_whisper_hallucination("THANKS FOR WATCHING."));
        assert!(is_whisper_hallucination("..."));
        assert!(is_whisper_hallucination("Bye bye."));
    }

    #[test]
    fn passes_real_speech() {
        assert!(!is_whisper_hallucination("Hello, how are you?"));
        assert!(!is_whisper_hallucination("I need help with my order"));
        assert!(!is_whisper_hallucination("Thank you for your help today"));
        assert!(!is_whisper_hallucination("bye for now"));
    }

    #[test]
    fn empty_string_is_not_hallucination() {
        assert!(!is_whisper_hallucination(""));
    }
}

/// Speak a fallback error message to the caller when the pipeline fails.
async fn send_error_message(
    stream_sid: &str,
    state: &AppState,
    tx: &mpsc::Sender<Message>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    const FALLBACK: &str = "Sorry, I couldn't process that. Please try again.";

    match state.tts.synthesize(FALLBACK).await {
        Ok(pcm_bytes) => send_audio(stream_sid, &pcm_bytes, tx).await,
        Err(e) => {
            // TTS itself is down — nothing we can do
            tracing::error!("TTS unavailable for error message: {e}");
            Ok(())
        }
    }
}
