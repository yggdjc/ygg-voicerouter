//! Tests for speaker verification module.

use voicerouter::continuous::speaker::{cosine_similarity, SpeakerVerifier};

#[test]
fn cosine_similarity_identical_vectors() {
    let a = vec![1.0f32, 0.0, 0.0];
    let b = vec![1.0, 0.0, 0.0];
    let sim = cosine_similarity(&a, &b);
    assert!((sim - 1.0).abs() < 1e-6);
}

#[test]
fn cosine_similarity_orthogonal_vectors() {
    let a = vec![1.0f32, 0.0];
    let b = vec![0.0, 1.0];
    let sim = cosine_similarity(&a, &b);
    assert!(sim.abs() < 1e-6);
}

#[test]
fn cosine_similarity_opposite_vectors() {
    let a = vec![1.0f32, 0.0];
    let b = vec![-1.0, 0.0];
    let sim = cosine_similarity(&a, &b);
    assert!((sim + 1.0).abs() < 1e-6);
}

#[test]
fn cosine_similarity_zero_vector() {
    let a = vec![0.0f32, 0.0];
    let b = vec![1.0, 0.0];
    let sim = cosine_similarity(&a, &b);
    assert_eq!(sim, 0.0);
}

#[test]
fn speaker_verify_accepts_above_threshold() {
    let enrollment = vec![0.5f32, 0.5, 0.5];
    let sample = vec![0.49, 0.51, 0.5];
    let verifier = SpeakerVerifier::from_enrollment(enrollment, 0.6);
    assert!(verifier.verify(&sample));
}

#[test]
fn speaker_verify_rejects_below_threshold() {
    let enrollment = vec![1.0f32, 0.0, 0.0];
    let sample = vec![0.0, 1.0, 0.0];
    let verifier = SpeakerVerifier::from_enrollment(enrollment, 0.6);
    assert!(!verifier.verify(&sample));
}

#[test]
fn speaker_verify_threshold_boundary() {
    // Vectors with exactly known cosine similarity
    let enrollment = vec![1.0f32, 0.0];
    let sample = vec![0.6, 0.8]; // cos = 0.6
    let verifier = SpeakerVerifier::from_enrollment(enrollment, 0.6);
    assert!(verifier.verify(&sample));
}
