use base64::Engine;
use serde::Deserialize;

/// Inworld text-to-speech client.
///
/// Returns raw mu-law 8kHz audio — ready for Twilio with no conversion needed.
pub struct TtsClient {
    client: reqwest::Client,
    api_key: String,
    voice_id: String,
    model: String,
}

/// Inworld's per-request character limit.
const MAX_CHARS: usize = 2000;

/// Inworld TTS response shape.
#[derive(Deserialize)]
struct TtsResponse {
    #[serde(rename = "audioContent")]
    audio_content: String,
}

impl TtsClient {
    pub fn new(api_key: String, voice_id: String, model: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            voice_id,
            model,
        }
    }

    /// Convert text to raw mu-law 8kHz audio bytes using the default voice.
    pub async fn synthesize(&self, text: &str) -> Result<Vec<u8>, TtsError> {
        self.synthesize_with_voice(text, &self.voice_id).await
    }

    /// Convert text to raw mu-law 8kHz audio bytes using an explicit voice ID.
    pub async fn synthesize_with_voice(
        &self,
        text: &str,
        voice_id: &str,
    ) -> Result<Vec<u8>, TtsError> {
        let chunks = split_text(text, MAX_CHARS);
        let mut all_audio = Vec::new();

        for chunk in &chunks {
            let audio = self.synthesize_chunk(chunk, voice_id).await?;
            all_audio.extend_from_slice(&audio);
        }

        Ok(all_audio)
    }

    /// Synthesize a single chunk (must be <= MAX_CHARS).
    async fn synthesize_chunk(&self, text: &str, voice_id: &str) -> Result<Vec<u8>, TtsError> {
        let body = serde_json::json!({
            "text": text,
            "voiceId": voice_id,
            "modelId": &self.model,
            "audioConfig": {
                "audioEncoding": "MULAW",
                "sampleRateHertz": 8000
            }
        });

        let resp = self
            .client
            .post("https://api.inworld.ai/tts/v1/voice")
            .header("Authorization", format!("Basic {}", &self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| TtsError::Request(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(TtsError::Api(format!("{status}: {body}")));
        }

        let tts_resp: TtsResponse = resp
            .json()
            .await
            .map_err(|e| TtsError::Request(e.to_string()))?;

        base64::engine::general_purpose::STANDARD
            .decode(&tts_resp.audio_content)
            .map_err(|e| TtsError::Api(format!("Bad base64 in audioContent: {e}")))
    }
}

/// Split text at sentence boundaries to stay under the character limit.
///
/// Splits on `. `, `! `, `? ` boundaries. If a single sentence exceeds the
/// limit, falls back to splitting at the limit (mid-word if necessary).
fn split_text(text: &str, max_chars: usize) -> Vec<&str> {
    if text.len() <= max_chars {
        return vec![text];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= max_chars {
            chunks.push(remaining);
            break;
        }

        // Find the last sentence boundary within the limit
        let search_slice = &remaining[..max_chars];
        let split_pos = search_slice
            .rmatch_indices(". ")
            .chain(search_slice.rmatch_indices("! "))
            .chain(search_slice.rmatch_indices("? "))
            .map(|(i, s)| i + s.len())
            .max();

        let pos = split_pos.unwrap_or(max_chars);
        chunks.push(&remaining[..pos]);
        remaining = remaining[pos..].trim_start();
    }

    chunks
}

#[derive(Debug, thiserror::Error)]
pub enum TtsError {
    #[error("HTTP request failed: {0}")]
    Request(String),
    #[error("API error: {0}")]
    Api(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_not_split() {
        let chunks = split_text("Hello world.", 2000);
        assert_eq!(chunks, vec!["Hello world."]);
    }

    #[test]
    fn splits_at_sentence_boundary() {
        let text = "First sentence. Second sentence. Third sentence.";
        let chunks = split_text(text, 35);
        assert_eq!(chunks[0], "First sentence. Second sentence. ");
        assert_eq!(chunks[1], "Third sentence.");
    }

    #[test]
    fn falls_back_to_hard_split() {
        let text = "A".repeat(3000);
        let chunks = split_text(&text, 2000);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 2000);
        assert_eq!(chunks[1].len(), 1000);
    }
}
