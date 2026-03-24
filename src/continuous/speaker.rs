//! Speaker verification via embedding cosine similarity.

/// Compute cosine similarity between two embedding vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

/// Speaker verifier using pre-enrolled embedding.
pub struct SpeakerVerifier {
    enrollment: Vec<f32>,
    threshold: f32,
}

impl SpeakerVerifier {
    /// Create a verifier from a pre-computed enrollment embedding.
    pub fn from_enrollment(embedding: Vec<f32>, threshold: f32) -> Self {
        Self { enrollment: embedding, threshold }
    }

    /// Check if a sample embedding matches the enrolled speaker.
    pub fn verify(&self, sample_embedding: &[f32]) -> bool {
        cosine_similarity(&self.enrollment, sample_embedding) >= self.threshold
    }

    /// Return the cosine similarity score (for logging/debugging).
    pub fn score(&self, sample_embedding: &[f32]) -> f32 {
        cosine_similarity(&self.enrollment, sample_embedding)
    }
}
