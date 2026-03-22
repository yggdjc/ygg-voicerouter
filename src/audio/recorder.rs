//! Audio capture via cpal.
//!
//! Opens the default input device, requests 16 kHz mono f32 samples, and
//! collects them into a shared buffer.  If the device does not natively
//! support 16 kHz the closest supported sample rate is used and the caller
//! is responsible for resampling (the pipeline does this in `mod.rs`).

use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate, Stream, StreamConfig, SupportedStreamConfigRange};

/// Window (in samples) used for the rolling RMS measurement — 100 ms at 16 kHz.
const RMS_WINDOW: usize = 1600;

/// Shared state written by the cpal callback and read by the main thread.
struct SharedBuffer {
    samples: Vec<f32>,
    /// Circular window of recent samples used to compute RMS.
    rms_window: Vec<f32>,
    rms_pos: usize,
}

impl SharedBuffer {
    fn new() -> Self {
        Self {
            samples: Vec::new(),
            rms_window: vec![0.0; RMS_WINDOW],
            rms_pos: 0,
        }
    }

    fn push_samples(&mut self, data: &[f32]) {
        self.samples.extend_from_slice(data);
        for &s in data {
            self.rms_window[self.rms_pos] = s;
            self.rms_pos = (self.rms_pos + 1) % RMS_WINDOW;
        }
    }

    fn rms(&self) -> f32 {
        super::compute_rms(&self.rms_window)
    }
}

/// Captures audio from the default input device.
///
/// The actual sample rate used may differ from the requested rate when the
/// device does not support it.
pub struct AudioRecorder {
    device_name: String,
    /// The stream config that was negotiated with the device.
    stream_config: StreamConfig,
    shared: Arc<Mutex<SharedBuffer>>,
    /// Live cpal stream; kept here so it is not dropped while recording.
    stream: Option<Stream>,
    recording: bool,
}

impl AudioRecorder {
    /// Create a new recorder targeting `sample_rate` Hz mono.
    ///
    /// Returns an error if no default input device exists.
    pub fn new(sample_rate: u32, _channels: u16) -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .context("no default input device found")?;

        let device_name = device
            .name()
            .unwrap_or_else(|_| "<unknown>".to_owned());

        // Pick the best supported input config: prefer f32 mono at the
        // requested rate; fall back to the highest-quality available config.
        let mut configs: Vec<SupportedStreamConfigRange> = device
            .supported_input_configs()
            .context("failed to query supported input configs")?
            .collect();

        if configs.is_empty() {
            bail!("device '{}' has no supported input configs", device_name);
        }

        // Sort: prefer f32, then more channels (we will downmix later), then
        // higher max sample rate.
        configs.sort_by_key(|c| {
            let fmt_score = if c.sample_format() == SampleFormat::F32 { 0i32 } else { 1 };
            let ch_score = c.channels() as i32;
            let rate_score = -(c.max_sample_rate().0 as i32);
            (fmt_score, ch_score, rate_score)
        });

        let best = &configs[0];
        let target_rate = SampleRate(sample_rate);
        let actual_rate = if target_rate >= best.min_sample_rate()
            && target_rate <= best.max_sample_rate()
        {
            target_rate
        } else {
            // Clamp to supported range.
            SampleRate(
                target_rate
                    .0
                    .clamp(best.min_sample_rate().0, best.max_sample_rate().0),
            )
        };

        let supported = (*best).with_sample_rate(actual_rate);
        let stream_config = StreamConfig {
            channels: supported.channels(),
            sample_rate: supported.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };

        log::debug!(
            "audio device '{}': {} ch @ {} Hz ({})",
            device_name,
            stream_config.channels,
            stream_config.sample_rate.0,
            supported.sample_format(),
        );

        Ok(Self {
            device_name,
            stream_config,
            shared: Arc::new(Mutex::new(SharedBuffer::new())),
            stream: None,
            recording: false,
        })
    }

    /// Start capturing audio.  Clears any previously recorded samples.
    pub fn start(&mut self) -> Result<()> {
        // Reset shared buffer.
        {
            let mut buf = self.shared.lock().unwrap();
            *buf = SharedBuffer::new();
        }

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .context("no default input device found (start)")?;

        let shared = Arc::clone(&self.shared);
        let channels = self.stream_config.channels as usize;
        let config = self.stream_config.clone();

        // Build an f32 input stream regardless of native format; cpal will
        // convert for us via the typed builder.
        let stream = device
            .build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    // Downmix to mono by averaging channels.
                    let mono: Vec<f32> = if channels == 1 {
                        data.to_vec()
                    } else {
                        data.chunks_exact(channels)
                            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                            .collect()
                    };
                    if let Ok(mut buf) = shared.lock() {
                        buf.push_samples(&mono);
                    }
                },
                move |err| {
                    log::error!("audio input stream error: {err}");
                },
                None,
            )
            .with_context(|| {
                format!("failed to build input stream on '{}'", self.device_name)
            })?;

        stream.play().context("failed to start input stream")?;
        self.stream = Some(stream);
        self.recording = true;
        Ok(())
    }

    /// Stop recording and return the captured samples.
    ///
    /// Returns `None` if recording was not active.
    pub fn stop(&mut self) -> Option<Vec<f32>> {
        if !self.recording {
            return None;
        }
        // Drop the stream first to stop the callback.
        self.stream = None;
        self.recording = false;

        let buf = self.shared.lock().unwrap();
        Some(buf.samples.clone())
    }

    /// Root-mean-square amplitude of the most recent ~100 ms of audio.
    ///
    /// Returns 0.0 when recording has not started.
    pub fn rms(&self) -> f32 {
        self.shared.lock().unwrap().rms()
    }

}
