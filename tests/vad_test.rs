//! Tests for VAD (Voice Activity Detection) module.

use voicerouter::continuous::vad::EnergyVad;

#[test]
fn detects_speech_segment() {
    // 16kHz, threshold 0.02
    let mut vad = EnergyVad::new(16000, 0.02);
    let mut segments: Vec<Vec<f32>> = Vec::new();

    // Feed 500ms silence
    let silence = vec![0.001f32; 8000];
    vad.feed(&silence, &mut |seg: &[f32]| segments.push(seg.to_vec()));
    assert!(segments.is_empty());

    // Feed 500ms speech (loud signal)
    let speech: Vec<f32> = (0..8000).map(|i| 0.3 * (i as f32 * 0.1).sin()).collect();
    vad.feed(&speech, &mut |seg: &[f32]| segments.push(seg.to_vec()));
    assert!(segments.is_empty(), "segment should not emit during speech");

    // Feed 500ms silence to trigger end-of-speech
    vad.feed(&silence, &mut |seg: &[f32]| segments.push(seg.to_vec()));

    // May need more silence to pass the min silence duration
    vad.feed(&silence, &mut |seg: &[f32]| segments.push(seg.to_vec()));

    assert_eq!(segments.len(), 1, "should emit exactly one segment");
    assert!(!segments[0].is_empty());
}

#[test]
fn ignores_pure_silence() {
    let mut vad = EnergyVad::new(16000, 0.02);
    let mut segments: Vec<Vec<f32>> = Vec::new();

    // Feed 2 seconds of silence
    let silence = vec![0.001f32; 32000];
    vad.feed(&silence, &mut |seg: &[f32]| segments.push(seg.to_vec()));

    assert!(segments.is_empty());
}

#[test]
fn minimum_segment_length() {
    let mut vad = EnergyVad::new(16000, 0.02);
    let mut segments: Vec<Vec<f32>> = Vec::new();

    // Feed very short speech (50ms) — too short, should be discarded
    let short_speech: Vec<f32> = (0..800).map(|i| 0.3 * (i as f32 * 0.1).sin()).collect();
    vad.feed(&short_speech, &mut |seg: &[f32]| segments.push(seg.to_vec()));

    // Feed silence to flush
    let silence = vec![0.001f32; 16000];
    vad.feed(&silence, &mut |seg: &[f32]| segments.push(seg.to_vec()));
    vad.feed(&silence, &mut |seg: &[f32]| segments.push(seg.to_vec()));

    // Very short segments should be discarded
    assert!(segments.is_empty(), "very short speech should be discarded");
}
