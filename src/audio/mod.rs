//! Audio capture and denoising pipeline.
//!
//! The public entry point is [`AudioPipeline`], which wraps an [`AudioRecorder`]
//! and optionally applies RNNoise denoising via [`denoise`].
//!
//! # Example
//!
//! ```no_run
//! use voicerouter::audio::AudioPipeline;
//! use voicerouter::config::AudioConfig;
//!
//! let config = AudioConfig::default();
//! let mut pipeline = AudioPipeline::new(&config).expect("audio init failed");
//! pipeline.start_recording().expect("start failed");
//! // … wait for speech …
//! if let Some(samples) = pipeline.stop_recording() {
//!     println!("captured {} samples", samples.len());
//! }
//! ```

pub mod denoise;
pub mod recorder;

pub use recorder::AudioRecorder;

use anyhow::Result;

use crate::config::AudioConfig;
use denoise::denoise as apply_denoise;

/// Combined audio capture and optional denoising pipeline.
pub struct AudioPipeline {
    recorder: AudioRecorder,
    denoise_enabled: bool,
}

impl AudioPipeline {
    /// Initialise the pipeline from an [`AudioConfig`].
    ///
    /// # Errors
    ///
    /// Returns an error if no default input device is available or if the
    /// device rejects the requested configuration.
    pub fn new(config: &AudioConfig) -> Result<Self> {
        let recorder = AudioRecorder::new(config.sample_rate, config.channels)?;
        Ok(Self {
            recorder,
            denoise_enabled: config.denoise,
        })
    }

    /// Begin capturing audio.  Clears any previously recorded samples.
    ///
    /// # Errors
    ///
    /// Returns an error if the audio stream cannot be started.
    pub fn start_recording(&mut self) -> Result<()> {
        self.recorder.start()
    }

    /// Stop recording and return the captured (and possibly denoised) samples.
    ///
    /// Returns `None` if recording was not active.
    pub fn stop_recording(&mut self) -> Option<Vec<f32>> {
        let raw = self.recorder.stop()?;
        if self.denoise_enabled {
            log::debug!("applying RNNoise denoise to {} samples", raw.len());
            Some(apply_denoise(&raw))
        } else {
            Some(raw)
        }
    }

    /// Root-mean-square amplitude of the most recent ~100 ms of captured audio.
    pub fn rms(&self) -> f32 {
        self.recorder.rms()
    }
}
