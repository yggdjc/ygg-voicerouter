//! Audio capture pipeline.
//!
//! The public entry point is [`AudioPipeline`], which wraps an [`AudioRecorder`].
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

pub mod recorder;

pub use recorder::AudioRecorder;

use anyhow::Result;

use crate::config::AudioConfig;

/// Audio capture pipeline.
pub struct AudioPipeline {
    recorder: AudioRecorder,
}

impl AudioPipeline {
    /// Initialise the pipeline from an [`AudioConfig`].
    pub fn new(config: &AudioConfig) -> Result<Self> {
        let recorder = AudioRecorder::new(config.sample_rate, config.channels)?;
        Ok(Self { recorder })
    }

    /// Begin capturing audio.
    pub fn start_recording(&mut self) -> Result<()> {
        self.recorder.start()
    }

    /// Stop recording and return the captured samples.
    pub fn stop_recording(&mut self) -> Option<Vec<f32>> {
        self.recorder.stop()
    }

    /// Root-mean-square amplitude of the most recent ~100 ms of captured audio.
    pub fn rms(&self) -> f32 {
        self.recorder.rms()
    }
}
