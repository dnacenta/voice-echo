use crate::pipeline::audio;
use std::time::Instant;

/// Simple energy-based Voice Activity Detection.
///
/// Buffers incoming mu-law audio chunks. When speech is detected followed
/// by a silence gap exceeding the threshold, emits the complete utterance.
pub struct VoiceActivityDetector {
    /// Accumulated PCM samples for the current utterance
    pcm_buffer: Vec<i16>,
    /// Whether we've detected speech in the current utterance
    has_speech: bool,
    /// When the last speech was detected
    last_speech_at: Option<Instant>,
    /// Minimum RMS energy to consider as speech
    energy_threshold: f64,
    /// How long silence must last before we consider speech done
    silence_threshold: std::time::Duration,
}

impl VoiceActivityDetector {
    pub fn new(energy_threshold: u16, silence_threshold_ms: u64) -> Self {
        Self {
            pcm_buffer: Vec::with_capacity(8000 * 30), // ~30s at 8kHz
            has_speech: false,
            last_speech_at: None,
            energy_threshold: energy_threshold as f64,
            silence_threshold: std::time::Duration::from_millis(silence_threshold_ms),
        }
    }

    /// Feed a chunk of mu-law audio. Returns Some(pcm_samples) when a
    /// complete utterance is detected (speech followed by silence gap).
    pub fn feed(&mut self, mulaw_chunk: &[u8]) -> Option<Vec<i16>> {
        let pcm = audio::decode_mulaw(mulaw_chunk);
        let energy = audio::rms_energy(&pcm);

        self.pcm_buffer.extend_from_slice(&pcm);

        if energy > self.energy_threshold {
            self.has_speech = true;
            self.last_speech_at = Some(Instant::now());
        }

        // Check if we have speech followed by enough silence
        if self.has_speech {
            if let Some(last) = self.last_speech_at {
                if last.elapsed() >= self.silence_threshold {
                    let utterance = std::mem::take(&mut self.pcm_buffer);
                    self.has_speech = false;
                    self.last_speech_at = None;
                    return Some(utterance);
                }
            }
        }

        // Prevent unbounded growth if no speech is ever detected
        if !self.has_speech && self.pcm_buffer.len() > 8000 * 5 {
            self.pcm_buffer.clear();
        }

        None
    }

    /// Reset the detector state (e.g., between conversation turns).
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.pcm_buffer.clear();
        self.has_speech = false;
        self.last_speech_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::audio::pcm_to_mulaw;

    #[test]
    fn silence_does_not_trigger() {
        let mut vad = VoiceActivityDetector::new(50, 500);
        let silence = vec![0u8; 160]; // 20ms of mu-law silence (not real silence encoding but low energy)
        for _ in 0..100 {
            assert!(vad.feed(&silence).is_none());
        }
    }

    #[test]
    fn loud_signal_detected() {
        let mut vad = VoiceActivityDetector::new(50, 100);

        // Feed loud audio
        let loud_pcm: Vec<i16> = (0..160).map(|i| ((i % 50) * 500) as i16).collect();
        let loud_mulaw: Vec<u8> = loud_pcm.iter().map(|&s| pcm_to_mulaw(s)).collect();

        let result = vad.feed(&loud_mulaw);
        // Won't trigger yet — silence threshold not elapsed
        assert!(result.is_none());
        assert!(vad.has_speech);
    }
}
