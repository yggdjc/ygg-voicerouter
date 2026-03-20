//! Audio feedback beeps via PipeWire / PulseAudio (`paplay`).
//!
//! Generates short sine-wave tones for start, done, and error events. Each
//! function returns immediately; playback happens on a background thread
//! piping raw WAV data to `paplay` so the system audio routing (including
//! Bluetooth) is handled correctly.
//!
//! # Example
//!
//! ```no_run
//! voicerouter::sound::beep_start().ok();
//! ```

use anyhow::Result;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Play a short start-of-recording beep (880 Hz, 150 ms).
pub fn beep_start() -> Result<()> {
    play_beep(880.0, 150)
}

/// Play a short done/success beep (660 Hz, 150 ms).
pub fn beep_done() -> Result<()> {
    play_beep(660.0, 150)
}

/// Play a short error beep (330 Hz, 250 ms).
pub fn beep_error() -> Result<()> {
    play_beep(330.0, 250)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Spawn a background thread that generates a WAV beep and pipes it to
/// `paplay`.
fn play_beep(freq_hz: f32, duration_ms: u32) -> Result<()> {
    std::thread::Builder::new()
        .name("voicerouter-beep".to_owned())
        .spawn(move || {
            if let Err(e) = play_beep_blocking(freq_hz, duration_ms) {
                log::warn!("sound: beep failed: {e}");
            }
        })?;
    Ok(())
}

/// Generate a mono 16-bit 48 kHz WAV in memory and pipe it to `paplay`.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn play_beep_blocking(freq_hz: f32, duration_ms: u32) -> Result<()> {
    use std::process::{Command, Stdio};

    let sample_rate: u32 = 48_000;
    let total = (sample_rate * duration_ms) / 1000;
    let fade = ((sample_rate * 5) / 1000).min(total / 2); // 5 ms fade

    // Generate 16-bit PCM samples.
    let mut pcm = Vec::with_capacity(total as usize * 2);
    for i in 0..total {
        let t = i as f32 / sample_rate as f32;
        let sine = (2.0 * std::f32::consts::PI * freq_hz * t).sin();
        let env = fade_envelope(i, total, fade);
        let sample = (sine * env * 0.6 * 32767.0) as i16;
        pcm.extend_from_slice(&sample.to_le_bytes());
    }

    // Build a minimal WAV header.
    let data_size = pcm.len() as u32;
    let mut wav = Vec::with_capacity(44 + pcm.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_size).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    wav.extend_from_slice(&pcm);

    // Write WAV to a temp file and play via pw-play (PipeWire) or aplay.
    let tmp = std::env::temp_dir().join(format!("voicerouter-beep-{}.wav", freq_hz as u32));
    std::fs::write(&tmp, &wav)?;

    let result = Command::new("pw-play")
        .arg(&tmp)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    // Fallback to aplay if pw-play unavailable.
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
