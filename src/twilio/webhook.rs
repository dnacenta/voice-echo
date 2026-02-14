use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::AppState;

/// Handle POST /twilio/voice — Twilio webhook for incoming calls.
///
/// Responds with TwiML that connects the call to a WebSocket media stream.
/// Twilio will then open a WSS connection to /twilio/media where we handle
/// the actual audio.
pub async fn handle_voice(State(state): State<AppState>) -> Response {
    let ws_url = media_stream_url(&state.config.server.external_url);

    let twiml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Response>
    <Connect>
        <Stream url="{ws_url}" />
    </Connect>
</Response>"#
    );

    ([("Content-Type", "text/xml")], twiml).into_response()
}

#[derive(Debug, Deserialize)]
pub struct OutboundQuery {
    #[serde(default)]
    pub message: Option<String>,
}

/// Handle POST /twilio/voice/outbound — webhook for outbound calls.
///
/// When Twilio calls someone and they pick up, this webhook provides TwiML.
/// If a message query param is set, it speaks that first, then opens the stream.
pub async fn handle_voice_outbound(
    State(state): State<AppState>,
    Query(query): Query<OutboundQuery>,
) -> Response {
    let ws_url = media_stream_url(&state.config.server.external_url);

    let say_element = match &query.message {
        Some(msg) if !msg.is_empty() => format!("\n    <Say>{}</Say>", escape_xml(msg)),
        _ => String::new(),
    };

    let twiml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Response>{say_element}
    <Connect>
        <Stream url="{ws_url}" />
    </Connect>
</Response>"#
    );

    ([("Content-Type", "text/xml")], twiml).into_response()
}

fn media_stream_url(external_url: &str) -> String {
    format!(
        "{}/twilio/media",
        external_url
            .replace("https://", "wss://")
            .replace("http://", "ws://")
    )
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
