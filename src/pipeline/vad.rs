use crate::pipeline::audio::{self, BandpassFilter};
use std::time::{Duration, Instant};

const SAMPLE_RATE: f64 = 8000.0;
const SPEECH_LOW_HZ: f64 = 300.0;
const SPEECH_HIGH_HZ: f64 = 3000.0;

/// Energy-based Voice Activity Detection with noise resilience.
///
/// Buffers incoming mu-law audio chunks. Applies a bandpass filter to isolate
/// speech frequencies before energy calculation, optionally adapts the energy
/// threshold to the ambient noise floor, and enforces a maximum utterance
/// duration as a safety net.
pub struct VoiceActivityDetector {
    /// Accumulated PCM samples for the current utterance (unfiltered)
    pcm_buffer: Vec<i16>,
    /// Whether we've detected speech in the current utterance
    has_speech: bool,
    /// When the last speech was detected
    last_speech_at: Option<Instant>,
    /// When the current utterance started (for max duration timeout)
    utterance_start: Option<Instant>,
    /// Base energy threshold (used as initial/fallback value)
    energy_threshold: f64,
    /// How long silence must last before we consider speech done
    silence_threshold: Duration,
    /// Maximum utterance duration before force-sending to STT
    max_utterance_duration: Option<Duration>,
    /// Bandpass filter isolating speech frequencies for VAD energy calculation
    bandpass: BandpassFilter,
    /// Whether adaptive threshold is enabled
    adaptive: bool,
    /// Running estimate of background noise energy
    noise_floor: f64,
    /// Speech must exceed noise_floor * this multiplier
    noise_floor_multiplier: f64,
    /// Decay factor for noise floor exponential moving average (0.99–0.999)
    noise_floor_decay: f64,
}

impl VoiceActivityDetector {
    pub fn new(energy_threshold: u16, silence_threshold_ms: u64) -> Self {
        Self {
            pcm_buffer: Vec::with_capacity(8000 * 30),
            has_speech: false,
            last_speech_at: None,
            utterance_start: None,
            energy_threshold: energy_threshold as f64,
            silence_threshold: Duration::from_millis(silence_threshold_ms),
            max_utterance_duration: None,
            bandpass: BandpassFilter::new(SPEECH_LOW_HZ, SPEECH_HIGH_HZ, SAMPLE_RATE),
            adaptive: false,
            noise_floor: 0.0,
            noise_floor_multiplier: 3.0,
            noise_floor_decay: 0.995,
        }
    }

    /// Enable adaptive threshold mode.
    pub fn with_adaptive(mut self, multiplier: f64, decay: f64) -> Self {
        self.adaptive = true;
        self.noise_floor_multiplier = multiplier;
        self.noise_floor_decay = decay;
        self
    }

    /// Set maximum utterance duration (safety net).
    pub fn with_max_utterance(mut self, secs: u64) -> Self {
        self.max_utterance_duration = Some(Duration::from_secs(secs));
        self
    }

    /// The effective speech threshold, accounting for adaptive mode.
    fn speech_threshold(&self) -> f64 {
        if self.adaptive && self.noise_floor > 0.0 {
            (self.noise_floor * self.noise_floor_multiplier).max(self.energy_threshold)
        } else {
            self.energy_threshold
        }
    }

    /// Update the noise floor estimate during silence periods.
    fn update_noise_floor(&mut self, energy: f64) {
        if !self.adaptive {
            return;
        }
        if self.noise_floor == 0.0 {
            // First sample — initialize directly
            self.noise_floor = energy;
        } else {
            // Exponential moving average
            self.noise_floor =
                self.noise_floor_decay * self.noise_floor + (1.0 - self.noise_floor_decay) * energy;
        }
    }

    /// Feed a chunk of mu-law audio. Returns Some(pcm_samples) when a
    /// complete utterance is detected (speech followed by silence gap,
    /// or max utterance duration exceeded).
    pub fn feed(&mut self, mulaw_chunk: &[u8]) -> Option<Vec<i16>> {
        let pcm = audio::decode_mulaw(mulaw_chunk);

        // Filter to speech band before energy calculation
        let filtered = self.bandpass.filter(&pcm);
        let energy = audio::rms_energy(&filtered);

        // Buffer the original (unfiltered) audio for STT
        self.pcm_buffer.extend_from_slice(&pcm);

        let threshold = self.speech_threshold();

        if energy > threshold {
            if !self.has_speech {
                self.utterance_start = Some(Instant::now());
                tracing::debug!(
                    energy = format!("{energy:.1}"),
                    threshold = format!("{threshold:.1}"),
                    noise_floor = format!("{:.1}", self.noise_floor),
                    "Speech started"
                );
            }
            self.has_speech = true;
            self.last_speech_at = Some(Instant::now());
        } else if !self.has_speech {
            // Only update noise floor when we're confident it's not speech
            self.update_noise_floor(energy);
        }

        if self.has_speech {
            // Check max utterance duration (safety net)
            if let (Some(max_dur), Some(start)) =
                (self.max_utterance_duration, self.utterance_start)
            {
                if start.elapsed() >= max_dur {
                    tracing::warn!(
                        samples = self.pcm_buffer.len(),
                        "Max utterance duration reached, force-sending"
                    );
                    return Some(self.take_utterance());
                }
            }

            // Check silence gap
            if let Some(last) = self.last_speech_at {
                if last.elapsed() >= self.silence_threshold {
                    return Some(self.take_utterance());
                }
            }
        }

        // Prevent unbounded growth if no speech is ever detected
        if !self.has_speech && self.pcm_buffer.len() > 8000 * 5 {
            self.pcm_buffer.clear();
        }

        None
    }

    /// Extract the buffered utterance and reset state.
    fn take_utterance(&mut self) -> Vec<i16> {
        let utterance = std::mem::take(&mut self.pcm_buffer);
        self.has_speech = false;
        self.last_speech_at = None;
        self.utterance_start = None;
        utterance
    }

    /// Reset the detector state (e.g., between conversation turns).
    pub fn reset(&mut self) {
        self.pcm_buffer.clear();
        self.has_speech = false;
        self.last_speech_at = None;
        self.utterance_start = None;
        // Don't reset noise_floor or bandpass — they should persist across turns
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::audio::pcm_to_mulaw;

    #[test]
    fn silence_does_not_trigger() {
        let mut vad = VoiceActivityDetector::new(50, 500);
        let silence = vec![0u8; 160];
        for _ in 0..100 {
            assert!(vad.feed(&silence).is_none());
        }
    }

    #[test]
    fn loud_signal_detected() {
        let mut vad = VoiceActivityDetector::new(50, 100);

        let loud_pcm: Vec<i16> = (0..160).map(|i| ((i % 50) * 500) as i16).collect();
        let loud_mulaw: Vec<u8> = loud_pcm.iter().map(|&s| pcm_to_mulaw(s)).collect();

        let result = vad.feed(&loud_mulaw);
        // Won't trigger yet — silence threshold not elapsed
        assert!(result.is_none());
        assert!(vad.has_speech);
    }

    #[test]
    fn max_utterance_forces_emit() {
        // 0s max utterance — speech detection and timeout fire in the same feed
        let mut vad = VoiceActivityDetector::new(50, 5000).with_max_utterance(0);

        let loud_pcm: Vec<i16> = (0..160).map(|i| ((i % 50) * 500) as i16).collect();
        let loud_mulaw: Vec<u8> = loud_pcm.iter().map(|&s| pcm_to_mulaw(s)).collect();

        // With 0s timeout, first feed detects speech AND exceeds max duration immediately
        let result = vad.feed(&loud_mulaw);
        assert!(result.is_some());
    }

    #[test]
    fn adaptive_threshold_adjusts() {
        let mut vad = VoiceActivityDetector::new(50, 500).with_adaptive(3.0, 0.99);

        // Feed low-energy "noise" to build up noise floor
        let noise_pcm: Vec<i16> = (0..160).map(|i| ((i % 10) * 3) as i16).collect();
        let noise_mulaw: Vec<u8> = noise_pcm.iter().map(|&s| pcm_to_mulaw(s)).collect();
        for _ in 0..50 {
            vad.feed(&noise_mulaw);
        }

        // Noise floor should be established
        assert!(vad.noise_floor > 0.0);
        // Threshold should be at least noise_floor * multiplier
        assert!(vad.speech_threshold() >= vad.noise_floor * 3.0);
    }
}
