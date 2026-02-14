#!/usr/bin/env python3
"""Generate a ~15-second dark synthwave hold music loop.

Style: Kavinski / Matrix vibe — deep sub-bass, sawtooth arpeggiator in Am,
atmospheric detuned pad, ~100 BPM.

Output: hold-music.wav (44100 Hz, stereo, 16-bit)
"""

import struct
import math
import os

SAMPLE_RATE = 44100
DURATION = 15.0  # seconds — seamless loop
BPM = 100
BEAT = 60.0 / BPM  # seconds per beat

NUM_SAMPLES = int(SAMPLE_RATE * DURATION)


def saw(phase):
    """Sawtooth wave from phase [0, 1) → [-1, 1]."""
    return 2.0 * (phase % 1.0) - 1.0


def sine(phase):
    """Sine wave from phase [0, 1) → [-1, 1]."""
    return math.sin(2.0 * math.pi * phase)


def low_pass(samples, cutoff_freq, sample_rate):
    """Simple one-pole low-pass filter."""
    rc = 1.0 / (2.0 * math.pi * cutoff_freq)
    dt = 1.0 / sample_rate
    alpha = dt / (rc + dt)
    out = [0.0] * len(samples)
    out[0] = alpha * samples[0]
    for i in range(1, len(samples)):
        out[i] = out[i - 1] + alpha * (samples[i] - out[i - 1])
    return out


def generate():
    # Am arpeggio notes (MIDI): A3=57, C4=60, E4=64, G4=67 (Am7)
    arp_notes_midi = [57, 60, 64, 67, 64, 60]  # up and back down
    arp_freqs = [440.0 * (2.0 ** ((m - 69) / 12.0)) for m in arp_notes_midi]

    # Sixteenth note duration for arp
    sixteenth = BEAT / 4.0

    left = [0.0] * NUM_SAMPLES
    right = [0.0] * NUM_SAMPLES

    for i in range(NUM_SAMPLES):
        t = i / SAMPLE_RATE

        # --- Sub-bass: sine at A1 (55 Hz), pulsing with beat ---
        bass_freq = 55.0
        bass_env = 0.5 + 0.5 * sine(t / BEAT)  # pulse with beat
        bass = sine(t * bass_freq) * bass_env * 0.35

        # --- Sawtooth arpeggiator ---
        arp_step = int(t / sixteenth) % len(arp_freqs)
        arp_freq = arp_freqs[arp_step]
        # Envelope: quick attack, moderate decay within each step
        step_t = (t % sixteenth) / sixteenth
        arp_env = max(0.0, 1.0 - step_t * 1.5)
        arp = saw(t * arp_freq) * arp_env * 0.15

        # --- Atmospheric pad: layered detuned saws ---
        pad_base = 220.0  # A3
        pad = 0.0
        for detune in [-0.03, -0.01, 0.0, 0.01, 0.03]:
            freq = pad_base * (1.0 + detune)
            pad += saw(t * freq)
        pad *= 0.04  # quiet pad

        # Also add a fifth (E4 = 329.63 Hz) for richness
        pad5 = 0.0
        for detune in [-0.02, 0.0, 0.02]:
            freq = 329.63 * (1.0 + detune)
            pad5 += saw(t * freq)
        pad5 *= 0.025

        # Slow volume swell on pad
        pad_swell = 0.6 + 0.4 * sine(t / (BEAT * 8))
        pad_total = (pad + pad5) * pad_swell

        # --- Mix ---
        mono = bass + arp + pad_total

        # Slight stereo: arp slightly right, pad slightly left
        left[i] = bass + arp * 0.7 + pad_total * 1.1
        right[i] = bass + arp * 1.1 + pad_total * 0.7

    # Low-pass filter the whole mix (dark feel, ~2kHz cutoff)
    left = low_pass(left, 2000.0, SAMPLE_RATE)
    right = low_pass(right, 2000.0, SAMPLE_RATE)

    # Normalize
    peak = max(max(abs(s) for s in left), max(abs(s) for s in right))
    if peak > 0:
        scale = 0.85 / peak
        left = [s * scale for s in left]
        right = [s * scale for s in right]

    # Crossfade last 0.5s with first 0.5s for seamless loop
    fade_samples = int(SAMPLE_RATE * 0.5)
    for i in range(fade_samples):
        mix = i / fade_samples  # 0→1
        left[i] = left[i] * mix + left[NUM_SAMPLES - fade_samples + i] * (1.0 - mix)
        right[i] = right[i] * mix + right[NUM_SAMPLES - fade_samples + i] * (1.0 - mix)

    return left, right


def write_wav(filename, left, right, sample_rate=44100, bits=16):
    """Write a stereo WAV file."""
    num_samples = len(left)
    num_channels = 2
    byte_rate = sample_rate * num_channels * (bits // 8)
    block_align = num_channels * (bits // 8)
    data_size = num_samples * block_align

    with open(filename, "wb") as f:
        # RIFF header
        f.write(b"RIFF")
        f.write(struct.pack("<I", 36 + data_size))
        f.write(b"WAVE")
        # fmt chunk
        f.write(b"fmt ")
        f.write(struct.pack("<I", 16))  # chunk size
        f.write(struct.pack("<H", 1))  # PCM
        f.write(struct.pack("<H", num_channels))
        f.write(struct.pack("<I", sample_rate))
        f.write(struct.pack("<I", byte_rate))
        f.write(struct.pack("<H", block_align))
        f.write(struct.pack("<H", bits))
        # data chunk
        f.write(b"data")
        f.write(struct.pack("<I", data_size))
        for i in range(num_samples):
            l_sample = max(-32768, min(32767, int(left[i] * 32767)))
            r_sample = max(-32768, min(32767, int(right[i] * 32767)))
            f.write(struct.pack("<hh", l_sample, r_sample))


if __name__ == "__main__":
    print("Generating dark synthwave hold music...")
    left, right = generate()
    out_path = os.path.join(os.path.dirname(__file__), "hold-music.wav")
    write_wav(out_path, left, right)
    file_size = os.path.getsize(out_path)
    print(f"Written: {out_path} ({file_size} bytes, {DURATION}s)")
