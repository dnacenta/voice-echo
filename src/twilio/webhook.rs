use axum::extract::State;
use axum::response::{IntoResponse, Response};

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

/// Handle POST /twilio/voice/outbound — webhook for outbound calls.
///
/// When Twilio calls someone and they pick up, this webhook provides TwiML.
/// The greeting is handled by the media stream via TTS (better voice quality),
/// so we just open the stream directly.
pub async fn handle_voice_outbound(State(state): State<AppState>) -> Response {
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

fn media_stream_url(external_url: &str) -> String {
    format!(
        "{}/twilio/media",
        external_url
            .replace("https://", "wss://")
            .replace("http://", "ws://")
    )
}
