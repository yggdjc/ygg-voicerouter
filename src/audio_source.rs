//! AudioSource — owns the cpal input stream and broadcasts PCM chunks.

use std::sync::Arc;

use anyhow::{Context, Result};
use crossbeam::channel::Sender;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::config::AudioConfig;

/// A chunk of mono f32 PCM samples shared via Arc (zero-copy fan-out).
pub type AudioChunk = Arc<[f32]>;

/// Commands sent to audio_source to open/close the device in lazy mode.
#[derive(Debug, Clone, Copy)]
pub enum AudioControl {
    Open,
    Close,
}

struct ResolvedDevice {
    device: cpal::Device,
    stream_config: cpal::StreamConfig,
    channels: usize,
}

fn resolve_device(config: &AudioConfig) -> Result<ResolvedDevice> {
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
    Ok(ResolvedDevice { device, stream_config, channels })
}

fn build_stream(
    resolved: &ResolvedDevice,
    subs: &Arc<[Sender<AudioChunk>]>,
) -> Result<cpal::Stream> {
    let channels = resolved.channels;
    let subs = Arc::clone(subs);

    let stream = resolved.device.build_input_stream(
        &resolved.stream_config,
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            let mono: Vec<f32> = if channels == 1 {
                data.to_vec()
            } else {
                data.chunks_exact(channels)
                    .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                    .collect()
            };

            let chunk: AudioChunk = Arc::from(mono.into_boxed_slice());

            for tx in subs.iter() {
                let _ = tx.try_send(Arc::clone(&chunk));
            }
        },
        |err| log::error!("[audio_source] stream error: {err}"),
        None,
    )
    .context("failed to build input stream")?;

    stream.play().context("failed to start input stream")?;
    Ok(stream)
}

/// Starts the cpal input stream and broadcasts chunks to all subscribers.
///
/// This function blocks the calling thread until `stop` is signalled.
/// It is meant to be spawned on a dedicated thread.
///
/// When `control` is `None`, the device streams continuously (always-on).
/// When `control` is `Some(rx)`, the device opens/closes on demand (lazy mode).
pub fn run_audio_source(
    config: &AudioConfig,
    subscribers: Vec<Sender<AudioChunk>>,
    stop: crossbeam::channel::Receiver<()>,
    control: Option<crossbeam::channel::Receiver<AudioControl>>,
) -> Result<()> {
    let resolved = resolve_device(config)?;
    let subs: Arc<[Sender<AudioChunk>]> = Arc::from(subscribers.into_boxed_slice());

    match control {
        None => run_always_on(&resolved, &subs, &stop),
        Some(ctrl) => run_lazy(&resolved, &subs, &stop, &ctrl),
    }
}

fn run_always_on(
    resolved: &ResolvedDevice,
    subs: &Arc<[Sender<AudioChunk>]>,
    stop: &crossbeam::channel::Receiver<()>,
) -> Result<()> {
    let _stream = build_stream(resolved, subs)?;
    log::info!("[audio_source] streaming");
    let _ = stop.recv();
    log::info!("[audio_source] stopped");
    Ok(())
}

fn run_lazy(
    resolved: &ResolvedDevice,
    subs: &Arc<[Sender<AudioChunk>]>,
    stop: &crossbeam::channel::Receiver<()>,
    ctrl: &crossbeam::channel::Receiver<AudioControl>,
) -> Result<()> {
    log::info!("[audio_source] lazy mode, device closed");
    let mut stream: Option<cpal::Stream> = None;
    let mut open_count: u32 = 0;

    loop {
        crossbeam::select! {
            recv(ctrl) -> msg => {
                match msg {
                    Ok(AudioControl::Open) => {
                        open_count += 1;
                        if open_count == 1 {
                            match build_stream(resolved, subs) {
                                Ok(s) => {
                                    log::info!("[audio_source] device opened");
                                    stream = Some(s);
                                }
                                Err(e) => {
                                    log::error!(
                                        "[audio_source] failed to open: {e:#}"
                                    );
                                    open_count = 0;
                                }
                            }
                        }
                    }
                    Ok(AudioControl::Close) => {
                        open_count = open_count.saturating_sub(1);
                        if open_count == 0 && stream.is_some() {
                            stream = None;
                            log::info!("[audio_source] device closed");
                        }
                    }
                    Err(_) => break,
                }
            }
            recv(stop) -> _ => break,
        }
    }

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

    #[test]
    fn audio_control_debug_display() {
        let open = AudioControl::Open;
        let close = AudioControl::Close;
        assert_eq!(format!("{open:?}"), "Open");
        assert_eq!(format!("{close:?}"), "Close");
    }
}
