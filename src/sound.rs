//! Audio feedback cues via PipeWire / ALSA.
//!
//! Generates short musical tones for start, done, error, and confirm events.
//! Each public function returns immediately; playback happens on a background
//! thread writing a temp WAV and invoking `pw-play` (fallback: `aplay`).
//!
//! # Sound design
//!
//! - **Start**: C-major chord (C5+E5+G5) with exponential decay — warm, inviting.
//! - **Done**: Descending G5→C5 with perfect-fifth overtones — resolving, calming.
//! - **Error**: Descending minor-second (A4→G#4) — tense, attention-grabbing.
//! - **Confirm**: Double tap at 1000 Hz with harmonics — distinct prompt.
//!
//! All tones use harmonics (2nd + 3rd partial) for richer timbre than pure sine.
//!
//! # Example
//!
//! ```no_run
//! voicerouter::sound::beep_start().ok();
//! ```

use anyhow::Result;

const SAMPLE_RATE: u32 = 48_000;
const AMPLITUDE: f32 = 0.5;

// Musical frequencies (Hz)
const C5: f32 = 523.25;
const E5: f32 = 659.25;
const G5: f32 = 783.99;
const A4: f32 = 440.0;
const G_SHARP4: f32 = 415.30;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Play a C-major chord (C5+E5+G5, 120 ms) — start of recording.
pub fn beep_start() -> Result<()> {
    spawn_playback("voicerouter-cue-start", || {
        let pcm = gen_chord(&[C5, E5, G5], 120);
        play_pcm_blocking(&pcm, "start")
    })
}

/// Play a descending G5→C5 with perfect-fifth overtones (70 ms + 15 ms gap + 70 ms) — done.
pub fn beep_done() -> Result<()> {
    spawn_playback("voicerouter-cue-done", || {
        let fifth_harmonics: &[(f32, f32)] = &[(1.0, 1.0), (1.5, 0.4), (2.0, 0.2)];
        let mut pcm = gen_harmonic_tone(G5, 70, fifth_harmonics);
        pcm.extend(gen_silence(15));
        pcm.extend(gen_harmonic_tone(C5, 70, fifth_harmonics));
        play_pcm_blocking(&pcm, "done")
    })
}

/// Play a descending minor-second A4→G#4 (80 ms + 40 ms gap + 80 ms) — error.
pub fn beep_error() -> Result<()> {
    spawn_playback("voicerouter-cue-error", || {
        let mut pcm = gen_harmonic_tone(A4, 80, DEFAULT_HARMONICS);
        pcm.extend(gen_silence(40));
        pcm.extend(gen_harmonic_tone(G_SHARP4, 80, DEFAULT_HARMONICS));
        play_pcm_blocking(&pcm, "error")
    })
}

/// Play a double-tap at 1000 Hz with harmonics (100 ms + 100 ms gap + 100 ms) — confirm prompt.
pub fn beep_confirm() -> Result<()> {
    spawn_playback("voicerouter-cue-confirm", || {
        let mut pcm = gen_harmonic_tone(1000.0, 100, DEFAULT_HARMONICS);
        pcm.extend(gen_silence(100));
        pcm.extend(gen_harmonic_tone(1000.0, 100, DEFAULT_HARMONICS));
        play_pcm_blocking(&pcm, "confirm")
    })
}

// ---------------------------------------------------------------------------
// Tone generators
// ---------------------------------------------------------------------------

/// Default harmonic partials: fundamental + 2nd (0.3×) + 3rd (0.1×).
const DEFAULT_HARMONICS: &[(f32, f32)] = &[(1.0, 1.0), (2.0, 0.3), (3.0, 0.1)];

/// Generate a single tone with configurable harmonics and exponential decay.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn gen_harmonic_tone(freq: f32, duration_ms: u32, harmonics: &[(f32, f32)]) -> Vec<i16> {
    let total = (SAMPLE_RATE * duration_ms) / 1000;
    let fade = ((SAMPLE_RATE * 8) / 1000).min(total / 2); // 8 ms fade
    let norm: f32 = harmonics.iter().map(|(_, a)| a).sum();

    let mut pcm = Vec::with_capacity(total as usize);
    for i in 0..total {
        let t = i as f32 / SAMPLE_RATE as f32;
        let val: f32 = harmonics
            .iter()
            .map(|&(mult, amp)| amp * (2.0 * std::f32::consts::PI * freq * mult * t).sin())
            .sum::<f32>()
            / norm;
        let decay = (-3.0 * i as f32 / total as f32).exp();
        let env = fade_envelope(i, total, fade);
        let sample = (val * decay * env * AMPLITUDE * 32767.0) as i16;
        pcm.push(sample);
    }
    pcm
}

/// Generate a chord (multiple simultaneous frequencies) with exponential decay.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn gen_chord(freqs: &[f32], duration_ms: u32) -> Vec<i16> {
    let total = (SAMPLE_RATE * duration_ms) / 1000;
    let fade_in = ((SAMPLE_RATE * 15) / 1000).min(total / 2);
    let fade_out = fade_in;
    let n = freqs.len() as f32;

    let mut pcm = Vec::with_capacity(total as usize);
    for i in 0..total {
        let t = i as f32 / SAMPLE_RATE as f32;
        let val: f32 = freqs
            .iter()
            .map(|&f| (2.0 * std::f32::consts::PI * f * t).sin())
            .sum::<f32>()
            / n;
        let decay = (-2.0 * i as f32 / total as f32).exp();
        let env = if i < fade_in {
            i as f32 / fade_in as f32
        } else if i >= total - fade_out {
            (total - i) as f32 / fade_out as f32
        } else {
            1.0
        };
        let sample = (val * decay * env * AMPLITUDE * 32767.0) as i16;
        pcm.push(sample);
    }
    pcm
}

/// Generate silence (zero samples) for the given duration.
fn gen_silence(duration_ms: u32) -> Vec<i16> {
    vec![0i16; ((SAMPLE_RATE * duration_ms) / 1000) as usize]
}

// ---------------------------------------------------------------------------
// Playback
// ---------------------------------------------------------------------------

/// Spawn a named background thread for non-blocking playback.
fn spawn_playback(name: &'static str, f: impl FnOnce() -> Result<()> + Send + 'static) -> Result<()> {
    std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || {
            if let Err(e) = f() {
                log::warn!("sound: {name} failed: {e}");
            }
        })?;
    Ok(())
}

/// Encode PCM as WAV, write to a temp file, and play via pw-play / aplay.
#[allow(clippy::cast_possible_truncation)]
fn play_pcm_blocking(pcm: &[i16], tag: &str) -> Result<()> {
    use std::process::{Command, Stdio};

    let data_size = (pcm.len() * 2) as u32;
    let mut wav = Vec::with_capacity(44 + data_size as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_size).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits/sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    for &s in pcm {
        wav.extend_from_slice(&s.to_le_bytes());
    }

    let tmp = std::env::temp_dir().join(format!("voicerouter-cue-{tag}.wav"));
    std::fs::write(&tmp, &wav)?;

    let result = Command::new("pw-play")
        .arg(&tmp)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    if result.is_err() {
        Command::new("aplay")
            .args(["-q"])
            .arg(&tmp)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok();
    }

    let _ = std::fs::remove_file(&tmp);
    Ok(())
}

/// Linear fade-in / fade-out envelope.
#[inline]
#[allow(clippy::cast_precision_loss)]
fn fade_envelope(i: u32, total: u32, fade_len: u32) -> f32 {
    if fade_len == 0 {
        return 1.0;
    }
    if i < fade_len {
        i as f32 / fade_len as f32
    } else if i >= total - fade_len {
        (total - i) as f32 / fade_len as f32
    } else {
        1.0
    }
}
