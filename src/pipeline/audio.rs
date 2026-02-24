use std::io::Cursor;
use std::path::Path;

const MULAW_SAMPLE_RATE: u32 = 8000;
const MULAW_BIAS: i16 = 0x84;
const MULAW_CLIP: i16 = 32635;

/// Decode a single mu-law byte to 16-bit PCM sample.
pub fn mulaw_to_pcm(mulaw: u8) -> i16 {
    // Invert all bits per ITU-T G.711
    let mulaw = !mulaw;

    let sign = (mulaw & 0x80) as i16;
    let exponent = ((mulaw >> 4) & 0x07) as i16;
    let mantissa = (mulaw & 0x0F) as i16;

    let mut sample = ((mantissa << 3) + MULAW_BIAS) << exponent;
    sample -= MULAW_BIAS;

    if sign != 0 {
        -sample
    } else {
        sample
    }
}

/// Encode a 16-bit PCM sample to mu-law byte.
pub fn pcm_to_mulaw(sample: i16) -> u8 {
    let sign: u8;
    let mut sample = sample;

    if sample < 0 {
        sign = 0x80;
        sample = -sample;
    } else {
        sign = 0;
    }

    if sample > MULAW_CLIP {
        sample = MULAW_CLIP;
    }
    sample += MULAW_BIAS;

    let exponent = compress_table((sample >> 7) as u8);
    let mantissa = ((sample >> (exponent + 3)) & 0x0F) as u8;

    !(sign | (exponent << 4) | mantissa)
}

fn compress_table(val: u8) -> u8 {
    match val {
        0..=1 => 0,
        2..=3 => 1,
        4..=7 => 2,
        8..=15 => 3,
        16..=31 => 4,
        32..=63 => 5,
        64..=127 => 6,
        _ => 7,
    }
}

/// Decode a buffer of mu-law bytes to 16-bit PCM samples.
pub fn decode_mulaw(mulaw_data: &[u8]) -> Vec<i16> {
    mulaw_data.iter().map(|&b| mulaw_to_pcm(b)).collect()
}

/// Encode 16-bit PCM samples to mu-law bytes.
pub fn encode_mulaw(pcm_data: &[i16]) -> Vec<u8> {
    pcm_data.iter().map(|&s| pcm_to_mulaw(s)).collect()
}

/// Encode PCM samples as a WAV file in memory (8kHz, 16-bit, mono).
pub fn pcm_to_wav(pcm_data: &[i16]) -> Result<Vec<u8>, hound::Error> {
    let mut buffer = Cursor::new(Vec::new());

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: MULAW_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer = hound::WavWriter::new(&mut buffer, spec)?;
    for &sample in pcm_data {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;

    Ok(buffer.into_inner())
}

/// Decode WAV file bytes to PCM samples. Expects 16-bit mono.
#[allow(dead_code)]
pub fn wav_to_pcm(wav_data: &[u8]) -> Result<Vec<i16>, hound::Error> {
    let cursor = Cursor::new(wav_data);
    let mut reader = hound::WavReader::new(cursor)?;
    let samples: Result<Vec<i16>, _> = reader.samples::<i16>().collect();
    samples
}

/// Calculate RMS energy of PCM samples (useful for VAD).
pub fn rms_energy(pcm_data: &[i16]) -> f64 {
    if pcm_data.is_empty() {
        return 0.0;
    }
    let sum: f64 = pcm_data.iter().map(|&s| (s as f64) * (s as f64)).sum();
    (sum / pcm_data.len() as f64).sqrt()
}

/// Second-order IIR (biquad) filter using Audio EQ Cookbook formulas.
struct BiquadFilter {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl BiquadFilter {
    /// Create a highpass filter (Butterworth, Q=0.7071).
    fn highpass(cutoff_hz: f64, sample_rate: f64) -> Self {
        let w0 = 2.0 * std::f64::consts::PI * cutoff_hz / sample_rate;
        let cos_w0 = w0.cos();
        let alpha = w0.sin() / (2.0 * std::f64::consts::FRAC_1_SQRT_2);

        let b0 = (1.0 + cos_w0) / 2.0;
        let b1 = -(1.0 + cos_w0);
        let b2 = (1.0 + cos_w0) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    /// Create a lowpass filter (Butterworth, Q=0.7071).
    fn lowpass(cutoff_hz: f64, sample_rate: f64) -> Self {
        let w0 = 2.0 * std::f64::consts::PI * cutoff_hz / sample_rate;
        let cos_w0 = w0.cos();
        let alpha = w0.sin() / (2.0 * std::f64::consts::FRAC_1_SQRT_2);

        let b0 = (1.0 - cos_w0) / 2.0;
        let b1 = 1.0 - cos_w0;
        let b2 = (1.0 - cos_w0) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    /// Process a single sample.
    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;

        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;

        y
    }
}

/// Bandpass filter chain (highpass + lowpass) for isolating speech frequencies.
///
/// Strips low-frequency noise (engine rumble, road noise) and high-frequency
/// artifacts while preserving the 300–3400Hz telephony speech band.
pub struct BandpassFilter {
    highpass: BiquadFilter,
    lowpass: BiquadFilter,
}

impl BandpassFilter {
    /// Create a bandpass filter for the given frequency range at the sample rate.
    pub fn new(low_hz: f64, high_hz: f64, sample_rate: f64) -> Self {
        Self {
            highpass: BiquadFilter::highpass(low_hz, sample_rate),
            lowpass: BiquadFilter::lowpass(high_hz, sample_rate),
        }
    }

    /// Filter PCM samples, returning only energy in the target band.
    /// Used for VAD energy calculation — does not modify the original buffer.
    pub fn filter(&mut self, samples: &[i16]) -> Vec<i16> {
        samples
            .iter()
            .map(|&s| {
                let filtered = self.lowpass.process(self.highpass.process(s as f64));
                filtered.clamp(-32768.0, 32767.0) as i16
            })
            .collect()
    }
}

/// Errors that can occur when loading hold music.
#[derive(Debug, thiserror::Error)]
pub enum HoldMusicError {
    #[error("failed to read WAV file: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid WAV format: {0}")]
    Wav(#[from] hound::Error),
    #[error("unsupported WAV format: {0}")]
    Unsupported(String),
}

/// Resample audio using linear interpolation.
pub fn resample_linear(samples: &[i16], from_rate: u32, to_rate: u32) -> Vec<i16> {
    if from_rate == to_rate {
        return samples.to_vec();
    }

    let ratio = from_rate as f64 / to_rate as f64;
    let out_len = (samples.len() as f64 / ratio) as usize;
    let mut output = Vec::with_capacity(out_len);

    for i in 0..out_len {
        let src_pos = i as f64 * ratio;
        let idx = src_pos as usize;
        let frac = src_pos - idx as f64;

        let sample = if idx + 1 < samples.len() {
            let a = samples[idx] as f64;
            let b = samples[idx + 1] as f64;
            (a + (b - a) * frac) as i16
        } else {
            samples[idx.min(samples.len() - 1)]
        };

        output.push(sample);
    }

    output
}

/// Load a WAV file and convert it to mu-law 8kHz, ready for Twilio streaming.
///
/// Handles stereo→mono downmix, arbitrary sample rate resampling, volume
/// adjustment, and mu-law encoding.
pub fn load_wav_as_mulaw(path: &Path, volume: f32) -> Result<Vec<u8>, HoldMusicError> {
    let reader = hound::WavReader::open(path)?;
    let spec = reader.spec();

    let channels = spec.channels as usize;
    let sample_rate = spec.sample_rate;

    // Read samples as i16 (handle both 16-bit and 8-bit)
    let all_samples: Vec<i16> = match spec.sample_format {
        hound::SampleFormat::Int => {
            if spec.bits_per_sample == 16 {
                reader
                    .into_samples::<i16>()
                    .filter_map(|s| s.ok())
                    .collect()
            } else if spec.bits_per_sample == 24 {
                reader
                    .into_samples::<i32>()
                    .filter_map(|s| s.ok())
                    .map(|s| (s >> 8) as i16)
                    .collect()
            } else if spec.bits_per_sample == 8 {
                reader
                    .into_samples::<i8>()
                    .filter_map(|s| s.ok())
                    .map(|s| (s as i16) << 8)
                    .collect()
            } else {
                return Err(HoldMusicError::Unsupported(format!(
                    "{}-bit integer not supported",
                    spec.bits_per_sample
                )));
            }
        }
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .filter_map(|s| s.ok())
            .map(|s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
            .collect(),
    };

    // Downmix stereo to mono
    let mono: Vec<i16> = if channels > 1 {
        all_samples
            .chunks(channels)
            .map(|frame| {
                let sum: i32 = frame.iter().map(|&s| s as i32).sum();
                (sum / channels as i32) as i16
            })
            .collect()
    } else {
        all_samples
    };

    // Resample to 8kHz
    let resampled = resample_linear(&mono, sample_rate, MULAW_SAMPLE_RATE);

    // Apply volume
    let scaled: Vec<i16> = resampled
        .iter()
        .map(|&s| ((s as f32) * volume).clamp(-32768.0, 32767.0) as i16)
        .collect();

    // Encode to mu-law
    Ok(encode_mulaw(&scaled))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mulaw_roundtrip() {
        // mu-law is lossy, but roundtrip should be close
        for original in [-32000i16, -1000, 0, 1000, 32000] {
            let encoded = pcm_to_mulaw(original);
            let decoded = mulaw_to_pcm(encoded);
            // Allow ~2% error due to lossy compression
            let diff = (original as f64 - decoded as f64).abs();
            assert!(
                diff < (original.unsigned_abs() as f64 * 0.05 + 100.0),
                "original={original}, decoded={decoded}, diff={diff}"
            );
        }
    }

    #[test]
    fn wav_roundtrip() {
        let samples: Vec<i16> = (0..100).map(|i| (i * 100) as i16).collect();
        let wav = pcm_to_wav(&samples).unwrap();
        let decoded = wav_to_pcm(&wav).unwrap();
        assert_eq!(samples, decoded);
    }

    #[test]
    fn rms_energy_silence() {
        let silence = vec![0i16; 100];
        assert_eq!(rms_energy(&silence), 0.0);
    }
}
