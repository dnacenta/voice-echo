use crate::config::TwilioConfig;

/// Twilio REST API client for initiating outbound calls.
pub struct TwilioClient {
    client: reqwest::Client,
    account_sid: String,
    auth_token: String,
    from_number: String,
    external_url: String,
}

impl TwilioClient {
    pub fn new(twilio_config: &TwilioConfig, external_url: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            account_sid: twilio_config.account_sid.clone(),
            auth_token: twilio_config.auth_token.clone(),
            from_number: twilio_config.phone_number.clone(),
            external_url: external_url.to_string(),
        }
    }

    /// Initiate an outbound call. Twilio will call `to`, and when answered,
    /// POST to our /twilio/voice/outbound webhook which provides TwiML
    /// to connect the media stream. The greeting is handled by the stream via TTS.
    pub async fn call(&self, to: &str) -> Result<String, OutboundError> {
        let url = format!(
            "https://api.twilio.com/2010-04-01/Accounts/{}/Calls.json",
            self.account_sid
        );

        let webhook_url = format!("{}/twilio/voice/outbound", self.external_url);

        let params = [
            ("To", to),
            ("From", &self.from_number),
            ("Url", &webhook_url),
        ];

        let resp = self
            .client
            .post(&url)
            .basic_auth(&self.account_sid, Some(&self.auth_token))
            .form(&params)
            .send()
            .await
            .map_err(|e| OutboundError::Request(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(OutboundError::Api(format!("{status}: {body}")));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| OutboundError::Request(e.to_string()))?;

        let call_sid = body["sid"].as_str().unwrap_or("unknown").to_string();

        tracing::info!(to, call_sid = %call_sid, "Outbound call initiated");
        Ok(call_sid)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OutboundError {
    #[error("HTTP request failed: {0}")]
    Request(String),
    #[error("Twilio API error: {0}")]
    Api(String),
}
