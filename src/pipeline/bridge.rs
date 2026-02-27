use serde_json::json;

/// HTTP client for bridge-echo. Sends transcribed speech to the multiplexer
/// and receives Claude's response. All session management and trust context
/// wrapping is handled by bridge-echo.
pub struct BridgeClient {
    url: String,
    caller_name: String,
    client: reqwest::Client,
}

impl BridgeClient {
    pub fn new(bridge_url: &str, caller_name: String) -> Self {
        Self {
            url: format!("{}/chat", bridge_url.trim_end_matches('/')),
            caller_name,
            client: reqwest::Client::new(),
        }
    }

    /// Send a voice transcript to bridge-echo and get the response.
    ///
    /// The `context` parameter is used for outbound calls — it tells Claude
    /// why it initiated the call. Consumed on first utterance.
    pub async fn send(
        &self,
        call_sid: &str,
        transcript: &str,
        context: Option<&str>,
    ) -> Result<String, BridgeError> {
        let mut metadata = json!({
            "call_sid": call_sid,
        });
        if let Some(ctx) = context {
            metadata["context"] = json!(ctx);
        }

        let body = json!({
            "channel": "voice",
            "sender": self.caller_name,
            "message": transcript,
            "metadata": metadata,
        });

        let resp = self
            .client
            .post(&self.url)
            .json(&body)
            .send()
            .await
            .map_err(|e| BridgeError::Request(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(BridgeError::Response(format!("HTTP {status}: {body}")));
        }

        let parsed: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| BridgeError::Parse(e.to_string()))?;

        parsed
            .get("response")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| BridgeError::Parse("Missing 'response' field".into()))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("Bridge request failed: {0}")]
    Request(String),
    #[error("Bridge returned error: {0}")]
    Response(String),
    #[error("Failed to parse bridge response: {0}")]
    Parse(String),
}
