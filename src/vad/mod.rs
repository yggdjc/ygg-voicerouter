//! Energy-based Voice Activity Detection (shared module).

use crate::audio;

const MIN_SEGMENT_SECS: f32 = 0.3;
const SILENCE_AFTER_SPEECH_SECS: f32 = 0.5;
const WINDOW_SAMPLES: usize = 800;

#[derive(Debug, PartialEq)]
pub enum VadEvent {
    Segment(Vec<f32>),
}

pub struct VadConfig {
    pub sample_rate: u32,
    pub threshold: f32,
}

pub struct VadDetector {
    sample_rate: u32,
    threshold: f32,
    in_speech: bool,
    onset_sample: usize,
    speech_end: usize,
    buffer: Vec<f32>,
    silence_samples: usize,
}

impl VadDetector {
    pub fn new(config: &VadConfig) -> Self {
        Self {
            sample_rate: config.sample_rate,
            threshold: config.threshold,
            in_speech: false,
            onset_sample: 0,
            speech_end: 0,
            buffer: Vec::new(),
            silence_samples: 0,
        }
    }

    pub fn in_speech(&self) -> bool {
        self.in_speech
    }

    pub fn feed(&mut self, samples: &[f32]) -> Vec<VadEvent> {
        let mut events = Vec::new();
        for chunk in samples.chunks(WINDOW_SAMPLES) {
            if let Some(segment) = self.process_window(chunk) {
                events.push(VadEvent::Segment(segment));
            }
        }
        events
    }

    /// Process a single analysis window. Returns Some(segment) if a complete speech segment was detected.
    fn process_window(&mut self, chunk: &[f32]) -> Option<Vec<f32>> {
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
            return None;
        }

        // Currently in speech
        self.buffer.extend_from_slice(chunk);

        if is_speech {
            self.silence_samples = 0;
            self.speech_end = self.buffer.len();
            return None;
        }

        // Silence during speech — check if enough to end segment
        self.silence_samples += chunk.len();
        let silence_secs = self.silence_samples as f32 / self.sample_rate as f32;

        if silence_secs < SILENCE_AFTER_SPEECH_SECS {
            return None;
        }

        let segment = &self.buffer[self.onset_sample..self.speech_end];
        let dur = segment.len() as f32 / self.sample_rate as f32;
        let result = if dur >= MIN_SEGMENT_SECS {
            Some(segment.to_vec())
        } else {
            None
        };

        self.buffer.clear();
        self.silence_samples = 0;
        self.in_speech = false;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config() -> VadConfig {
        VadConfig { sample_rate: 16000, threshold: 0.01 }
    }

    #[test]
    fn silence_produces_no_events() {
        let mut vad = VadDetector::new(&make_config());
        let silence = vec![0.0f32; 16000];
        let events = vad.feed(&silence);
        assert!(events.is_empty());
    }

    #[test]
    fn speech_then_silence_produces_segment() {
        let mut vad = VadDetector::new(&make_config());
        let speech: Vec<f32> = vec![0.1; 8000];
        let silence: Vec<f32> = vec![0.0; 16000];

        let mut all_events = vad.feed(&speech);
        all_events.extend(vad.feed(&silence));

        assert_eq!(all_events.len(), 1);
        assert!(matches!(&all_events[0], VadEvent::Segment(s) if !s.is_empty()));
    }

    #[test]
    fn short_speech_is_discarded() {
        let mut vad = VadDetector::new(&make_config());
        let speech: Vec<f32> = vec![0.1; 1600];
        let silence: Vec<f32> = vec![0.0; 16000];

        let mut all_events = vad.feed(&speech);
        all_events.extend(vad.feed(&silence));

        assert!(all_events.is_empty());
    }

    #[test]
    fn in_speech_accessor() {
        let mut vad = VadDetector::new(&make_config());
        assert!(!vad.in_speech());
        let speech: Vec<f32> = vec![0.1; 800];
        let _ = vad.feed(&speech);
        assert!(vad.in_speech());
    }
}
