//! Tests for VAD (Voice Activity Detection) module.

use voicerouter::vad::{VadConfig, VadDetector, VadEvent};

#[test]
fn detects_speech_segment() {
    let mut vad = VadDetector::new(&VadConfig { sample_rate: 16000, threshold: 0.02 });

    let silence = vec![0.001f32; 8000];
    let events = vad.feed(&silence);
    assert!(events.is_empty());

    let speech: Vec<f32> = (0..8000).map(|i| 0.3 * (i as f32 * 0.1).sin()).collect();
    let events = vad.feed(&speech);
    assert!(events.is_empty(), "segment should not emit during speech");

    let silence = vec![0.001f32; 16000];
    let mut events = vad.feed(&silence);
    if events.is_empty() {
        events = vad.feed(&silence);
    }

    assert_eq!(events.len(), 1, "should emit exactly one segment");
    assert!(matches!(&events[0], VadEvent::Segment(s) if !s.is_empty()));
}

#[test]
fn ignores_pure_silence() {
    let mut vad = VadDetector::new(&VadConfig { sample_rate: 16000, threshold: 0.02 });
    let silence = vec![0.001f32; 32000];
    let events = vad.feed(&silence);
    assert!(events.is_empty());
}

#[test]
fn minimum_segment_length() {
    let mut vad = VadDetector::new(&VadConfig { sample_rate: 16000, threshold: 0.02 });
    let short_speech: Vec<f32> = (0..800).map(|i| 0.3 * (i as f32 * 0.1).sin()).collect();
    let events = vad.feed(&short_speech);
    assert!(events.is_empty());

    let silence = vec![0.001f32; 16000];
    let mut events = vad.feed(&silence);
    events.extend(vad.feed(&silence));
    assert!(events.is_empty(), "very short speech should be discarded");
}

#[test]
fn in_speech_accessor() {
    let mut vad = VadDetector::new(&VadConfig { sample_rate: 16000, threshold: 0.02 });
    assert!(!vad.in_speech());
    let speech: Vec<f32> = vec![0.1; 800];
    let _ = vad.feed(&speech);
    assert!(vad.in_speech());
}
