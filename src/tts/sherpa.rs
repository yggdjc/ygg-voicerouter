//! sherpa-onnx Kokoro TTS engine implementation.

use anyhow::{bail, Context, Result};

use super::TtsEngine;
use crate::asr::models::expand_tilde;
use crate::config::TtsConfig;

pub struct SherpaTts {
    tts: sherpa_onnx::OfflineTts,
    speed: f32,
    sid: i32,
}

impl SherpaTts {
    pub fn new(config: &TtsConfig) -> Result<Self> {
        let model_dir = expand_tilde(&config.model_dir)
            .context("TTS model dir path error")?;
        let model_subdir = model_dir.join(&config.model);
        log::info!("[tts/sherpa] loading model from {}", model_subdir.display());

        let model_file = model_subdir.join("model.onnx");
        if !model_file.exists() {
            bail!(
                "TTS model not found at {}. Download with: \
                 cd {} && curl -LO https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/{}.tar.bz2 && tar xf {}.tar.bz2",
                model_file.display(),
                model_dir.display(),
                config.model,
                config.model,
            );
        }

        let voices_file = model_subdir.join("voices.bin");
        let tokens_file = model_subdir.join("tokens.txt");
        let data_dir = model_subdir.join("espeak-ng-data");
        let dict_dir = model_subdir.join("dict");
        let lexicon_zh = model_subdir.join("lexicon-zh.txt");
        let lexicon_us = model_subdir.join("lexicon-us-en.txt");

        // Build comma-separated lexicon list from available files.
        let mut lexicon_parts = Vec::new();
        if lexicon_zh.exists() {
            lexicon_parts.push(lexicon_zh.to_string_lossy().into_owned());
        }
        if lexicon_us.exists() {
            lexicon_parts.push(lexicon_us.to_string_lossy().into_owned());
        }
        let lexicon = if lexicon_parts.is_empty() {
            None
        } else {
            Some(lexicon_parts.join(","))
        };

        // Build rule_fsts for number/date/phone normalization.
        let fst_names = ["number-zh.fst", "date-zh.fst", "phone-zh.fst"];
        let fst_paths: Vec<String> = fst_names
            .iter()
            .map(|f| model_subdir.join(f))
            .filter(|p| p.exists())
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let rule_fsts = if fst_paths.is_empty() {
            None
        } else {
            Some(fst_paths.join(","))
        };

        let tts_config = sherpa_onnx::OfflineTtsConfig {
            model: sherpa_onnx::OfflineTtsModelConfig {
                kokoro: sherpa_onnx::OfflineTtsKokoroModelConfig {
                    model: Some(model_file.to_string_lossy().into_owned()),
                    voices: Some(voices_file.to_string_lossy().into_owned()),
                    tokens: Some(tokens_file.to_string_lossy().into_owned()),
                    data_dir: if data_dir.exists() {
                        Some(data_dir.to_string_lossy().into_owned())
                    } else {
                        None
                    },
                    dict_dir: if dict_dir.exists() {
                        Some(dict_dir.to_string_lossy().into_owned())
                    } else {
                        None
                    },
                    lexicon,
                    length_scale: 1.0,
                    ..Default::default()
                },
                num_threads: 2,
                provider: Some(config.provider.clone()),
                ..Default::default()
            },
            rule_fsts,
            max_num_sentences: 1,
            ..Default::default()
        };

        let tts = sherpa_onnx::OfflineTts::create(&tts_config)
            .ok_or_else(|| anyhow::anyhow!("failed to create Kokoro TTS engine"))?;

        log::info!(
            "[tts/sherpa] Kokoro ready — {} speakers, {}Hz",
            tts.num_speakers(),
            tts.sample_rate(),
        );

        Ok(Self {
            tts,
            speed: config.speed as f32,
            sid: config.sid,
        })
    }
}

impl TtsEngine for SherpaTts {
    fn synthesize(&self, text: &str) -> Result<Vec<f32>> {
        let gen_config = sherpa_onnx::GenerationConfig {
            speed: self.speed,
            sid: self.sid,
            ..Default::default()
        };

        let audio = self.tts
            .generate_with_config(text, &gen_config, None::<fn(&[f32], f32) -> bool>)
            .ok_or_else(|| anyhow::anyhow!("TTS generation failed for: {text:?}"))?;

        let samples = audio.samples().to_vec();
        if samples.is_empty() {
            bail!("TTS produced empty audio for: {text:?}");
        }

        Ok(samples)
    }

    fn sample_rate(&self) -> u32 {
        self.tts.sample_rate() as u32
    }
}
