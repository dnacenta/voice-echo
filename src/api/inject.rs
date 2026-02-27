use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::registry::CallRegistry;
use crate::AppState;

use super::outbound::check_auth;

#[derive(Debug, Deserialize)]
pub struct InjectRequest {
    /// The call_sid to inject audio into.
    pub call_sid: String,
    /// Text to synthesize and speak into the active call.
    pub text: String,
}

#[derive(Debug, Serialize)]
struct InjectResponse {
    status: String,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

/// POST /api/inject — Inject TTS audio into an active call.
///
/// Used by bridge-echo to route cross-channel responses to voice.
/// When D sends a Discord message during a call, bridge-echo sends
/// the Claude response here instead of back to Discord.
///
/// Requires `Authorization: Bearer <token>` header.
pub async fn handle_inject(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<InjectRequest>,
) -> impl IntoResponse {
    if let Err(resp) = check_auth(&headers, &state.config.api.token) {
        return resp;
    }

    tracing::info!(call_sid = %req.call_sid, text_len = req.text.len(), "Inject requested");

    // Look up the active call
    let entry = state.call_registry.get(&req.call_sid).await;
    let Some(entry) = entry else {
        tracing::warn!(call_sid = %req.call_sid, "No active call found for inject");
        return (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("No active call with sid {}", req.call_sid),
            }),
        )
            .into_response();
    };

    // Run TTS
    let tts_mulaw = match state.tts.synthesize(&req.text).await {
        Ok(data) => data,
        Err(e) => {
            tracing::error!(call_sid = %req.call_sid, "TTS failed for inject: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("TTS synthesis failed: {e}"),
                }),
            )
                .into_response();
        }
    };

    // Suppress VAD while injected audio plays
    entry.set_speaking(true);

    // Send audio frames through the call's response channel
    if let Err(e) = CallRegistry::send_audio(&entry, &tts_mulaw).await {
        tracing::error!(call_sid = %req.call_sid, "Failed to inject audio: {e}");
        entry.set_speaking(false);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to send audio: {e}"),
            }),
        )
            .into_response();
    }

    tracing::info!(
        call_sid = %req.call_sid,
        tts_bytes = tts_mulaw.len(),
        "Audio injected successfully"
    );

    (
        StatusCode::OK,
        Json(InjectResponse {
            status: "injected".to_string(),
        }),
    )
        .into_response()
}
