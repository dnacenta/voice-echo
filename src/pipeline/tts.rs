/// ElevenLabs text-to-speech client.
pub struct TtsClient {
    client: reqwest::Client,
    api_key: String,
    voice_id: String,
}

impl TtsClient {
    pub fn new(api_key: String, voice_id: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            voice_id,
        }
    }

    /// Convert text to audio bytes (PCM 16-bit, 8kHz mono) using the default voice.
    pub async fn synthesize(&self, text: &str) -> Result<Vec<u8>, TtsError> {
        self.synthesize_with_voice(text, &self.voice_id).await
    }

    /// Convert text to audio bytes (PCM 16-bit, 8kHz mono) using an explicit voice ID.
    pub async fn synthesize_with_voice(
        &self,
        text: &str,
        voice_id: &str,
    ) -> Result<Vec<u8>, TtsError> {
        let url = format!(
            "https://api.elevenlabs.io/v1/text-to-speech/{}",
            voice_id
        );

        let body = serde_json::json!({
            "text": text,
            "model_id": "eleven_turbo_v2_5",
            "output_format": "pcm_16000",
            "voice_settings": {
                "stability": 0.5,
                "similarity_boost": 0.75
            }
        });

        let resp = self
            .client
            .post(&url)
            .header("xi-api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .query(&[("output_format", "pcm_16000")])
            .json(&body)
            .send()
            .await
            .map_err(|e| TtsError::Request(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(TtsError::Api(format!("{status}: {body}")));
        }

        let audio_bytes = resp
            .bytes()
            .await
            .map_err(|e| TtsError::Request(e.to_string()))?;

        // ElevenLabs returns raw PCM at 16kHz. We need to downsample to 8kHz
        // for Twilio's mu-law encoding. Simple decimation by 2.
        let pcm_16k = bytes_to_pcm(&audio_bytes);
        let pcm_8k = downsample_2x(&pcm_16k);

        // Convert back to bytes
        Ok(pcm_to_bytes(&pcm_8k))
    }
}

/// Convert raw bytes (little-endian i16) to PCM sample vec.
fn bytes_to_pcm(data: &[u8]) -> Vec<i16> {
    data.chunks_exact(2)
        .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
        .collect()
}

/// Convert PCM samples back to little-endian bytes.
fn pcm_to_bytes(samples: &[i16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    bytes
}

/// Simple 2x downsampling by averaging adjacent samples.
fn downsample_2x(samples: &[i16]) -> Vec<i16> {
    samples
        .chunks(2)
        .map(|pair| {
            if pair.len() == 2 {
                ((pair[0] as i32 + pair[1] as i32) / 2) as i16
            } else {
                pair[0]
            }
        })
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum TtsError {
    #[error("HTTP request failed: {0}")]
    Request(String),
    #[error("API error: {0}")]
    Api(String),
}
