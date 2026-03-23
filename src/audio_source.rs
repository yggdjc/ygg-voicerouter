//! AudioSource — owns the cpal input stream and broadcasts PCM chunks.

use std::sync::Arc;

use anyhow::{Context, Result};
use crossbeam::channel::Sender;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::config::AudioConfig;

/// A chunk of mono f32 PCM samples shared via Arc (zero-copy fan-out).
pub type AudioChunk = Arc<[f32]>;

/// Starts the cpal input stream and broadcasts chunks to all subscribers.
///
/// This function blocks the calling thread until `stop` is signalled.
/// It is meant to be spawned on a dedicated thread.
pub fn run_audio_source(
    config: &AudioConfig,
    subscribers: Vec<Sender<AudioChunk>>,
    stop: crossbeam::channel::Receiver<()>,
) -> Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .context("no default input device")?;

    let device_name = device.name().unwrap_or_else(|_| "<unknown>".into());

    let mut configs: Vec<cpal::SupportedStreamConfigRange> = device
        .supported_input_configs()
        .context("failed to query input configs")?
        .collect();

    if configs.is_empty() {
        anyhow::bail!("device '{}' has no input configs", device_name);
    }

    configs.sort_by_key(|c| {
        let fmt = if c.sample_format() == cpal::SampleFormat::F32 { 0i32 } else { 1 };
        (fmt, -(c.max_sample_rate().0 as i32))
    });

    let best = &configs[0];
    let target = cpal::SampleRate(config.sample_rate);
    let actual = if target >= best.min_sample_rate() && target <= best.max_sample_rate() {
        target
    } else {
        cpal::SampleRate(target.0.clamp(best.min_sample_rate().0, best.max_sample_rate().0))
    };

    let supported = (*best).with_sample_rate(actual);
    let stream_config = cpal::StreamConfig {
        channels: supported.channels(),
        sample_rate: supported.sample_rate(),
        buffer_size: cpal::BufferSize::Default,
    };

    log::info!(
        "[audio_source] device '{}': {} ch @ {} Hz",
        device_name,
        stream_config.channels,
        stream_config.sample_rate.0,
    );

    let channels = stream_config.channels as usize;
    let subs = subscribers;

    let stream = device.build_input_stream(
        &stream_config,
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            let mono: Vec<f32> = if channels == 1 {
                data.to_vec()
            } else {
                data.chunks_exact(channels)
                    .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                    .collect()
            };

            let chunk: AudioChunk = Arc::from(mono.into_boxed_slice());

            for tx in &subs {
                let _ = tx.try_send(Arc::clone(&chunk));
            }
        },
        |err| log::error!("[audio_source] stream error: {err}"),
        None,
    )
    .context("failed to build input stream")?;

    stream.play().context("failed to start input stream")?;
    log::info!("[audio_source] streaming");

    // Block until stop signal.
    let _ = stop.recv();
    log::info!("[audio_source] stopped");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_chunk_is_arc_slice() {
        let chunk: AudioChunk = Arc::from(vec![0.0f32; 160].into_boxed_slice());
        assert_eq!(chunk.len(), 160);
        let clone = Arc::clone(&chunk);
        assert_eq!(clone.len(), 160);
    }
}
