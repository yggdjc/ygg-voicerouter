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

use anyhow::Result;

use crate::config::AudioConfig;
use recorder::AudioRecorder;
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

/// Peak windowed RMS — the loudest 50ms window in `samples`.
///
/// If any window in the recording exceeds the silence threshold,
/// the recording contains speech. This is far more robust than
/// overall RMS for low-gain microphones where speech and noise
/// floor are close in average energy.
pub fn peak_rms(samples: &[f32], sample_rate: u32) -> f32 {
    let window_size = (sample_rate as usize) / 20; // 50ms
    samples
        .chunks(window_size)
        .filter(|w| w.len() == window_size)
        .map(|w| compute_rms(w))
        .fold(0.0_f32, f32::max)
}

// ---------------------------------------------------------------------------
// Noise floor tracking
// ---------------------------------------------------------------------------

/// Tracks noise floor across recordings using exponential moving average.
///
/// Each recording is analyzed in 50ms windows. The P15 (15th percentile)
/// of windowed RMS values estimates the noise floor — this captures the
/// quiet segments (background noise) while ignoring speech peaks.
///
/// The threshold for silence detection is `noise_floor × 3`.
pub struct NoiseTracker {
    noise_floor: f32,
    floor_min: f32,
    ceiling: f32,
    sample_rate: u32,
}

impl NoiseTracker {
    /// Create a tracker with an initial noise floor estimate.
    pub fn new(initial: f32, floor_min: f32, sample_rate: u32) -> Self {
        Self {
            noise_floor: initial,
            floor_min,
            ceiling: 0.05,
            sample_rate,
        }
    }

    /// Current silence threshold (noise_floor × 2, clamped).
    pub fn threshold(&self) -> f32 {
        (self.noise_floor * 2.0).clamp(self.floor_min, self.ceiling)
    }

    /// Update noise floor estimate from a completed recording.
    ///
    /// Extracts the P15 of windowed RMS as the noise floor, then blends
    /// it into the running estimate with EMA (alpha=0.3).
    pub fn update(&mut self, samples: &[f32]) {
        let window_size = (self.sample_rate as usize) / 20; // 50ms
        let mut window_rms: Vec<f32> = samples
            .chunks(window_size)
            .filter(|w| w.len() == window_size)
            .map(|w| compute_rms(w))
            .collect();

        if window_rms.len() < 3 {
            return;
        }

        window_rms.sort_by(|a, b| a.partial_cmp(b).unwrap());

        // P15: 15th percentile — captures quiet segments, skips
        // digital silence (all-zero) and speech peaks.
        let p15_idx = window_rms.len() * 15 / 100;
        let measured = window_rms[p15_idx.max(1)];

        // Skip update if measured is near-zero (mic muted / digital silence)
        // or suspiciously high (speech leaked into a "silence" sample).
        if measured < 0.0001 || measured > self.noise_floor * 5.0 {
            return;
        }

        // EMA blend: 30% new measurement, 70% history.
        let alpha = 0.3_f32;
        let prev = self.noise_floor;
        self.noise_floor = prev * (1.0 - alpha) + measured * alpha;

        log::debug!(
            "noise tracker: measured={measured:.4}, floor={prev:.4} → {:.4}, \
             threshold={:.4}",
            self.noise_floor,
            self.threshold()
        );
    }
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

/// Compute root-mean-square amplitude of `samples`.
pub fn compute_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

/// Record 1 second of ambient noise and derive a silence threshold.
///
/// Splits the sample into 50ms windows, computes RMS per window, and
/// takes the **median** as the noise floor estimate.  The median is
/// robust to transient spikes (keyboard clicks, coughs).
///
/// Returns `median_rms * 3`, clamped to `[floor, 0.05]`.
/// `floor` is the config `silence_threshold` — the absolute minimum.
pub fn calibrate_silence(
    pipeline: &mut AudioPipeline,
    sample_rate: u32,
    floor: f32,
) -> f32 {
    log::info!("calibrating silence threshold (1s ambient sample)…");

    if pipeline.start_recording().is_err() {
        log::warn!("calibration failed to start — using floor {floor}");
        return floor;
    }
    std::thread::sleep(std::time::Duration::from_secs(1));
    let samples = pipeline.stop_recording().unwrap_or_default();

    if samples.is_empty() {
        log::warn!("calibration got no samples — using floor {floor}");
        return floor;
    }

    // 50ms windows at the given sample rate.
    let window_size = (sample_rate as usize) / 20;
    let mut window_rms: Vec<f32> = samples
        .chunks(window_size)
        .filter(|w| w.len() == window_size)
        .map(|w| compute_rms(w))
        .collect();

    if window_rms.is_empty() {
        log::warn!("calibration too short for windowing — using floor {floor}");
        return floor;
    }

    window_rms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = window_rms[window_rms.len() / 2];

    // Threshold = 2× noise floor, clamped to sane bounds.
    let ceiling = 0.05_f32;
    let threshold = (median * 2.0).clamp(floor, ceiling);

    log::info!(
        "noise floor (median RMS): {median:.4}, threshold: {threshold:.4} \
         (floor: {floor}, ceiling: {ceiling})"
    );
    threshold
}
