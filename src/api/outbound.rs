use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct CallRequest {
    /// Phone number to call (E.164 format, e.g., "+34612345678")
    pub to: String,
    /// Optional initial message to speak when the call is answered
    pub message: Option<String>,
    /// Optional context for the AI — why this call is being made.
    /// Injected into the first Claude prompt so it knows the reason for calling.
    pub context: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CallResponse {
    pub call_sid: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

/// POST /api/call — Trigger an outbound call.
///
/// Requires `Authorization: Bearer <token>` header matching the configured api.token.
///
/// Request body:
/// ```json
/// {
///   "to": "+34612345678",
///   "message": "Server CPU at 95%"
/// }
/// ```
pub async fn handle_call(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CallRequest>,
) -> impl IntoResponse {
    // Check bearer token
    if let Err(resp) = check_auth(&headers, &state.config.api.token) {
        return resp;
    }

    tracing::info!(to = %req.to, "Outbound call requested");

    match state.twilio.call(&req.to, req.message.as_deref()).await {
        Ok(call_sid) => {
            // Store context for this call so Claude knows why it's calling
            if let Some(ctx) = req.context {
                state
                    .call_contexts
                    .lock()
                    .await
                    .insert(call_sid.clone(), ctx);
                tracing::info!(call_sid = %call_sid, "Stored call context");
            }
            (
                StatusCode::OK,
                Json(CallResponse {
                    call_sid,
                    status: "initiated".to_string(),
                }),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("Failed to initiate call: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
                .into_response()
        }
    }
}

#[allow(clippy::result_large_err)]
fn check_auth(headers: &HeaderMap, expected_token: &str) -> Result<(), axum::response::Response> {
    if expected_token.is_empty() {
        tracing::warn!("API token not configured — rejecting request");
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "API token not configured".to_string(),
            }),
        )
            .into_response());
    }

    let provided = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match provided {
        Some(token) if token == expected_token => Ok(()),
        _ => {
            tracing::warn!("Unauthorized API request");
            Err((
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "Invalid or missing bearer token".to_string(),
                }),
            )
                .into_response())
        }
    }
}
