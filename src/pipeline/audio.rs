use std::io::Cursor;

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
