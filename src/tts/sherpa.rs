//! sherpa-onnx TTS engine implementation.

use anyhow::{Context, Result};

use super::TtsEngine;
use crate::asr::models::expand_tilde;
use crate::config::TtsConfig;

pub struct SherpaTts {
    sample_rate: u32,
}

impl SherpaTts {
    pub fn new(config: &TtsConfig) -> Result<Self> {
        let model_dir =
            expand_tilde(&config.model_dir).context("TTS model dir path error")?;
        log::info!("[tts/sherpa] model dir: {}", model_dir.display());
        Ok(Self { sample_rate: 22050 })
    }
}

impl TtsEngine for SherpaTts {
    fn synthesize(&self, text: &str) -> Result<Vec<f32>> {
        log::warn!(
            "[tts/sherpa] TTS synthesis not yet implemented, skipping: {text:?}"
        );
        Ok(vec![0.0; self.sample_rate as usize])
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}
