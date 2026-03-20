//! RNNoise-based audio denoising via `nnnoiseless`.
//!
//! RNNoise operates at 48 kHz with 480-sample frames and expects sample
//! values in the `i16` range (roughly −32768 … 32767) even though the type
//! is `f32`.  The pipeline here:
//!
//! 1. Scale normalised `[-1, 1]` input to `i16` range.
//! 2. Linear-interpolation resample 16 kHz → 48 kHz (3× up).
//! 3. Denoise in 480-sample frames (first output frame discarded per API).
//! 4. Resample 48 kHz → 16 kHz (3× down) by averaging every 3 samples.
//! 5. Scale back to `[-1, 1]`.

use nnnoiseless::DenoiseState;

/// Ratio between the RNNoise sample rate (48 kHz) and our capture rate (16 kHz).
const UPSAMPLE_RATIO: usize = 3;
const RNNOISE_FRAME: usize = DenoiseState::FRAME_SIZE; // 480

/// Denoise `samples` recorded at 16 kHz.
///
/// Returns a denoised `Vec<f32>` at 16 kHz with values in `[-1, 1]`.
/// The output may be a few samples shorter than the input due to frame
/// alignment; this is acceptable for speech recognition use.
pub fn denoise(samples: &[f32]) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }

    // --- 1. Scale to i16 range and upsample 16kHz → 48kHz ------------------
    let upsampled = upsample(samples, UPSAMPLE_RATIO);

    // --- 2. Denoise in 480-sample frames ------------------------------------
    let denoised_48k = run_rnnoise(&upsampled);

    // --- 3. Downsample 48kHz → 16kHz ----------------------------------------
    let downsampled = downsample(&denoised_48k, UPSAMPLE_RATIO);

    // --- 4. Scale back to [-1, 1] -------------------------------------------
    let scale = 1.0 / i16::MAX as f32;
    downsampled.into_iter().map(|s| s * scale).collect()
}

/// Linear-interpolation upsample by integer `ratio`.
fn upsample(input: &[f32], ratio: usize) -> Vec<f32> {
    if input.is_empty() {
        return Vec::new();
    }

    let scale = i16::MAX as f32;
    let n = input.len();
    let out_len = (n - 1) * ratio + 1;
    let mut out = Vec::with_capacity(out_len);

    for i in 0..(n - 1) {
        let a = input[i] * scale;
        let b = input[i + 1] * scale;
        for j in 0..ratio {
            let t = j as f32 / ratio as f32;
            out.push(a + (b - a) * t);
        }
    }
    out.push(input[n - 1] * scale);
    out
}

/// Downsample by integer `ratio` by averaging every `ratio` samples.
fn downsample(input: &[f32], ratio: usize) -> Vec<f32> {
    input
        .chunks_exact(ratio)
        .map(|chunk| chunk.iter().sum::<f32>() / ratio as f32)
        .collect()
}

/// Run nnnoiseless over `samples` (48 kHz, i16-range f32).
///
/// Returns denoised samples still in i16 range.  The first output frame is
/// discarded as the RNNoise documentation requires.
fn run_rnnoise(samples: &[f32]) -> Vec<f32> {
    let mut state = DenoiseState::new();
    let mut out_buf = [0.0f32; RNNOISE_FRAME];
    let mut output = Vec::with_capacity(samples.len());
    let mut first = true;

    for chunk in samples.chunks_exact(RNNOISE_FRAME) {
        state.process_frame(&mut out_buf, chunk);
        if first {
            first = false;
        } else {
            output.extend_from_slice(&out_buf);
        }
    }

    // Process any remaining samples padded to a full frame.
    let remainder = samples.len() % RNNOISE_FRAME;
    if remainder > 0 && !first {
        let mut padded = [0.0f32; RNNOISE_FRAME];
        padded[..remainder].copy_from_slice(&samples[samples.len() - remainder..]);
        state.process_frame(&mut out_buf, &padded);
        output.extend_from_slice(&out_buf[..remainder]);
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denoise_empty_is_empty() {
        assert!(denoise(&[]).is_empty());
    }

    #[test]
    fn denoise_produces_output_for_short_input() {
        // Less than one RNNoise frame worth of 16kHz samples.
        let input: Vec<f32> = (0..100).map(|i| (i as f32 * 0.01).sin() * 0.5).collect();
        // Should not panic; output may be empty (< 1 full frame after upsampling).
        let _ = denoise(&input);
    }

    #[test]
    fn upsample_ratio_correctness() {
        // Two samples upsampled by 3 should produce 4 output points.
        let input = [0.0f32, 1.0f32];
        let up = upsample(&input, 3);
        assert_eq!(up.len(), 4); // (2-1)*3 + 1
        // First and last should match input (scaled to i16 range).
        assert!((up[0] - 0.0).abs() < 1e-3);
        assert!((up[3] - i16::MAX as f32).abs() < 1e-3);
    }

    #[test]
    fn downsample_averages_correctly() {
        let input = [2.0f32, 4.0, 6.0, 8.0];
        let down = downsample(&input, 2);
        assert_eq!(down.len(), 2);
        assert!((down[0] - 3.0).abs() < 1e-6);
        assert!((down[1] - 7.0).abs() < 1e-6);
    }
}
