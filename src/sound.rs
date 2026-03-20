//! Audio feedback beeps using cpal.
//!
//! Generates short sine-wave tones for start, done, and error events. Each
//! function returns immediately; playback happens on a background thread.
//!
//! The caller is responsible for checking `config.sound.feedback` before
//! invoking these functions.
//!
//! # Example
//!
//! ```no_run
//! voicerouter::sound::beep_start().ok();
//! ```

use std::sync::{Arc, Mutex};

use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Play a short start-of-recording beep (880 Hz, 100 ms).
///
/// Returns immediately; sound plays in the background.
///
/// # Errors
///
/// Returns an error if thread spawning fails.
pub fn beep_start() -> Result<()> {
    play_beep(880.0, 100)
}

/// Play a short done/success beep (660 Hz, 100 ms).
///
/// Returns immediately; sound plays in the background.
///
/// # Errors
///
/// Returns an error if thread spawning fails.
pub fn beep_done() -> Result<()> {
    play_beep(660.0, 100)
}

/// Play a short error beep (330 Hz, 200 ms).
///
/// Returns immediately; sound plays in the background.
///
/// # Errors
///
/// Returns an error if thread spawning fails.
pub fn beep_error() -> Result<()> {
    play_beep(330.0, 200)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Spawn a background thread that generates and plays a sine-wave beep.
///
/// `freq_hz` is the tone frequency. `duration_ms` is the duration in
/// milliseconds. The beep has a 5 ms linear fade-in and fade-out envelope to
/// prevent click artifacts.
fn play_beep(freq_hz: f32, duration_ms: u64) -> Result<()> {
    std::thread::Builder::new()
        .name("voicerouter-beep".to_owned())
        .spawn(move || {
            if let Err(e) = play_beep_blocking(freq_hz, duration_ms) {
                log::warn!("sound: beep failed: {e}");
            }
        })?;
    Ok(())
}

/// Open the default output device and stream a sine-wave beep to completion.
///
/// Blocks the calling thread until the beep has finished playing, making it
/// suitable for use inside a dedicated background thread.
fn play_beep_blocking(freq_hz: f32, duration_ms: u64) -> Result<()> {
    let host = cpal::default_host();

    let Some(device) = host.default_output_device() else {
        log::warn!("sound: no default output device — skipping beep");
        return Ok(());
    };

    let supported = match device.default_output_config() {
        Ok(c) => c,
        Err(e) => {
            log::warn!("sound: cannot get output config: {e} — skipping beep");
            return Ok(());
        }
    };

    match supported.sample_format() {
        cpal::SampleFormat::F32 => stream_beep::<f32>(&device, &supported.into(), freq_hz, duration_ms),
        cpal::SampleFormat::I16 => stream_beep::<i16>(&device, &supported.into(), freq_hz, duration_ms),
        cpal::SampleFormat::U16 => stream_beep::<u16>(&device, &supported.into(), freq_hz, duration_ms),
        cpal::SampleFormat::I8  => stream_beep::<i8>( &device, &supported.into(), freq_hz, duration_ms),
        cpal::SampleFormat::I32 => stream_beep::<i32>(&device, &supported.into(), freq_hz, duration_ms),
        cpal::SampleFormat::I64 => stream_beep::<i64>(&device, &supported.into(), freq_hz, duration_ms),
        cpal::SampleFormat::U8  => stream_beep::<u8>( &device, &supported.into(), freq_hz, duration_ms),
        cpal::SampleFormat::U32 => stream_beep::<u32>(&device, &supported.into(), freq_hz, duration_ms),
        cpal::SampleFormat::U64 => stream_beep::<u64>(&device, &supported.into(), freq_hz, duration_ms),
        cpal::SampleFormat::F64 => stream_beep::<f64>(&device, &supported.into(), freq_hz, duration_ms),
        fmt => {
            log::warn!("sound: unsupported sample format {fmt} — skipping beep");
            Ok(())
        }
    }
}

/// Build and play a cpal output stream for a single beep, then block until
/// the pre-generated sample buffer has been consumed.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn stream_beep<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    freq_hz: f32,
    duration_ms: u64,
) -> Result<()>
where
    T: SizedSample + FromSample<f32>,
{
    let sample_rate = config.sample_rate.0 as f32;
    let channels = config.channels as usize;
    let total_samples = ((sample_rate * duration_ms as f32) / 1000.0) as usize;

    let samples = build_samples(freq_hz, sample_rate, total_samples);
    let samples = Arc::new(samples);
    let cursor = Arc::new(Mutex::new(0usize));

    let samples_cb = Arc::clone(&samples);
    let cursor_cb = Arc::clone(&cursor);

    let stream = device.build_output_stream(
        config,
        move |output: &mut [T], _info: &cpal::OutputCallbackInfo| {
            fill_output(output, channels, &samples_cb, &cursor_cb);
        },
        |err| log::warn!("sound: stream error: {err}"),
        None,
    )?;

    stream.play()?;

    // Poll until the cursor has consumed all samples, then let the stream drop.
    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let pos = *cursor.lock().expect("cursor mutex poisoned");
        if pos >= samples.len() {
            break;
        }
    }

    Ok(())
}

/// Pre-generate `total_samples` f32 sine-wave samples with fade-in/fade-out
/// envelopes of 5 ms each.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn build_samples(freq_hz: f32, sample_rate: f32, total_samples: usize) -> Vec<f32> {
    let fade_samples = ((sample_rate * 5.0) / 1000.0) as usize;
    // Clamp fade to at most half the total so very short beeps still work.
    let fade_len = fade_samples.min(total_samples / 2);

    (0..total_samples)
        .map(|i| {
            let t = i as f32 / sample_rate;
            let sine = (2.0 * std::f32::consts::PI * freq_hz * t).sin();
            let envelope = fade_envelope(i, total_samples, fade_len);
            // Attenuate to 60 % amplitude to keep beeps unobtrusive.
            sine * envelope * 0.6
        })
        .collect()
}

/// Compute a linear fade-in / fade-out envelope value in [0.0, 1.0].
#[inline]
#[allow(clippy::cast_precision_loss)]
fn fade_envelope(i: usize, total: usize, fade_len: usize) -> f32 {
    if fade_len == 0 {
        return 1.0;
    }
    if i < fade_len {
        // Fade in
        i as f32 / fade_len as f32
    } else if i >= total - fade_len {
        // Fade out
        (total - i) as f32 / fade_len as f32
    } else {
        1.0
    }
}

/// Write mono samples from the shared buffer into the interleaved output slice.
fn fill_output<T>(
    output: &mut [T],
    channels: usize,
    samples: &[f32],
    cursor: &Mutex<usize>,
) where
    T: SizedSample + FromSample<f32>,
{
    let mut pos = cursor.lock().expect("cursor mutex poisoned");
    for frame in output.chunks_mut(channels) {
        let sample_value: T = if *pos < samples.len() {
            let v = T::from_sample(samples[*pos]);
            *pos += 1;
            v
        } else {
            T::from_sample(0.0_f32)
        };
        for ch in frame.iter_mut() {
            *ch = sample_value;
        }
    }
}
