//! Energy-based Voice Activity Detection.
//!
//! Detects speech segments by tracking RMS energy above a threshold.
//! Emits complete segments (onset to offset) via callback.

use crate::audio;

/// Minimum speech duration (seconds) to emit a segment.
const MIN_SEGMENT_SECS: f32 = 0.3;
/// Silence duration (seconds) after speech to trigger end-of-segment.
const SILENCE_AFTER_SPEECH_SECS: f32 = 0.5;
/// Analysis window size in samples (50ms at 16kHz = 800 samples).
const WINDOW_SAMPLES: usize = 800;

pub struct EnergyVad {
    sample_rate: u32,
    threshold: f32,
    in_speech: bool,
    onset_sample: usize,
    /// Buffer index just past the last speech (above-threshold) window.
    speech_end: usize,
    buffer: Vec<f32>,
    silence_samples: usize,
}

impl EnergyVad {
    pub fn new(sample_rate: u32, threshold: f32) -> Self {
        Self {
            sample_rate,
            threshold,
            in_speech: false,
            onset_sample: 0,
            speech_end: 0,
            buffer: Vec::new(),
            silence_samples: 0,
        }
    }

    /// Feed audio samples. When a complete speech segment is detected,
    /// the callback is called with the segment samples.
    pub fn feed(
        &mut self,
        samples: &[f32],
        on_segment: &mut impl FnMut(&[f32]),
    ) {
        for chunk in samples.chunks(WINDOW_SAMPLES) {
            let rms = audio::compute_rms(chunk);
            let is_speech = rms >= self.threshold;

            if !self.in_speech {
                if is_speech {
                    self.onset_sample = self.buffer.len();
                    self.buffer.extend_from_slice(chunk);
                    self.speech_end = self.buffer.len();
                    self.silence_samples = 0;
                    self.in_speech = true;
                }
            } else {
                self.buffer.extend_from_slice(chunk);

                if is_speech {
                    self.silence_samples = 0;
                    self.speech_end = self.buffer.len();
                } else {
                    self.silence_samples += chunk.len();
                    let silence_secs =
                        self.silence_samples as f32 / self.sample_rate as f32;

                    if silence_secs >= SILENCE_AFTER_SPEECH_SECS {
                        let segment = &self.buffer
                            [self.onset_sample..self.speech_end];
                        let dur =
                            segment.len() as f32 / self.sample_rate as f32;

                        if dur >= MIN_SEGMENT_SECS {
                            on_segment(segment);
                        }

                        self.buffer.clear();
                        self.silence_samples = 0;
                        self.in_speech = false;
                    }
                }
            }
        }
    }
}
