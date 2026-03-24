//! ASR engine wrapping sherpa-onnx offline recognizers.
//!
//! # Design
//!
//! sherpa-onnx provides offline recognizers for Paraformer and Whisper models.
//! The public interface is intentionally simple:
//!
//! ```no_run
//! use voicerouter::config::AsrConfig;
//! use voicerouter::asr::engine::AsrEngine;
//!
//! let config = AsrConfig::default();
//! // AsrEngine::new returns an error when model files are absent.
//! // let engine = AsrEngine::new(&config).unwrap();
//! ```

use anyhow::{Context, Result};
use sherpa_onnx::{
    OfflineFunASRNanoModelConfig, OfflineModelConfig, OfflineParaformerModelConfig,
    OfflineRecognizer, OfflineRecognizerConfig, OfflineWhisperModelConfig,
};

use crate::config::AsrConfig;

use super::models::{get_model_paths, prepare_model_dir};

// ---------------------------------------------------------------------------
// AsrEngine
// ---------------------------------------------------------------------------

/// Offline ASR engine backed by a sherpa-onnx recognizer.
///
/// Construct with [`AsrEngine::new`] and call [`AsrEngine::transcribe`] for
/// each audio buffer.
pub struct AsrEngine {
    recognizer: OfflineRecognizer,
    /// Original config retained for error messages.
    _model_name: String,
}

impl AsrEngine {
    /// Create a new [`AsrEngine`] from `config`.
    ///
    /// Model files must already exist under `config.model_dir`. If they are
    /// absent, an error is returned with instructions on how to download them.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The model name is unsupported.
    /// - Model files are not found on disk.
    /// - The underlying sherpa-onnx recognizer fails to initialise.
    pub fn new(config: &AsrConfig) -> Result<Self> {
        let model_dir = prepare_model_dir(&config.model_dir)?;
        let paths = get_model_paths(&config.model, &model_dir)
            .with_context(|| format!("resolving paths for model '{}'", config.model))?;

        // Verify files exist before handing paths to sherpa-onnx, which would
        // crash or produce an opaque error on missing files.
        for path in std::iter::once(&paths.model)
            .chain(std::iter::once(&paths.tokens))
            .chain(paths.extras.iter())
        {
            if !path.exists() {
                anyhow::bail!(
                    "model file not found: {}. Run `ensure_model` or follow the installation \
                     instructions in the log output.",
                    path.display()
                );
            }
        }

        let recognizer_config = build_config(config, &paths)?;
        let recognizer = OfflineRecognizer::create(&recognizer_config)
            .ok_or_else(|| anyhow::anyhow!("failed to create recognizer for '{}'", config.model))?;

        Ok(Self {
            recognizer,
            _model_name: config.model.clone(),
        })
    }

    /// Transcribe `audio` (interleaved f32 PCM at `sample_rate` Hz) and
    /// return the recognised text.
    ///
    /// Returns an empty string for silence or very short input.
    ///
    /// # Errors
    ///
    /// Propagates any internal sherpa-onnx error.
    pub fn transcribe(&mut self, audio: &[f32], sample_rate: u32) -> Result<String> {
        if audio.is_empty() {
            return Ok(String::new());
        }

        let stream = self.recognizer.create_stream();
        stream.accept_waveform(sample_rate as i32, audio);
        self.recognizer.decode(&stream);

        let text = stream
            .get_result()
            .map(|r| r.text.trim().to_owned())
            .unwrap_or_default();

        Ok(text)
    }
}

// ---------------------------------------------------------------------------
// Config construction
// ---------------------------------------------------------------------------

fn build_config(
    config: &AsrConfig,
    paths: &super::models::ModelPaths,
) -> Result<OfflineRecognizerConfig> {
    let model_path = paths.model.to_string_lossy().into_owned();
    let tokens_path = paths.tokens.to_string_lossy().into_owned();

    let model_config = match config.model.as_str() {
        "paraformer-zh" => OfflineModelConfig {
            paraformer: OfflineParaformerModelConfig {
                model: Some(model_path),
            },
            tokens: Some(tokens_path),
            num_threads: 1,
            provider: Some(config.provider.clone()),
            ..Default::default()
        },
        "whisper-tiny-en" | "whisper-base-en" => {
            let decoder = paths
                .extras
                .first()
                .with_context(|| {
                    format!("missing decoder path for model '{}'", config.model)
                })?;
            OfflineModelConfig {
                whisper: OfflineWhisperModelConfig {
                    encoder: Some(model_path),
                    decoder: Some(decoder.to_string_lossy().into_owned()),
                    language: Some("en".into()),
                    ..Default::default()
                },
                tokens: Some(tokens_path),
                num_threads: 1,
                provider: Some(config.provider.clone()),
                ..Default::default()
            }
        }
        "funasr-nano" => {
            // ModelPaths mapping from model_info:
            //   model = encoder_adaptor.int8.onnx
            //   tokens = llm.int8.onnx (repurposed)
            //   extras[0] = embedding.int8.onnx
            //   extras[1] = Qwen3-0.6B (tokenizer directory)
            let embedding = paths.extras.first().with_context(|| "missing embedding path")?;
            let tokenizer = paths.extras.get(1).with_context(|| "missing tokenizer path")?;
            OfflineModelConfig {
                funasr_nano: OfflineFunASRNanoModelConfig {
                    encoder_adaptor: Some(model_path),
                    llm: Some(tokens_path), // repurposed: tokens slot holds llm path
                    embedding: Some(embedding.to_string_lossy().into_owned()),
                    tokenizer: Some(tokenizer.to_string_lossy().into_owned()),
                    language: Some("zh".into()),
                    ..Default::default()
                },
                num_threads: 2,
                provider: Some(config.provider.clone()),
                ..Default::default()
            }
        }
        other => anyhow::bail!("no backend for model '{other}'"),
    };

    Ok(OfflineRecognizerConfig {
        model_config,
        decoding_method: Some("greedy_search".into()),
        ..Default::default()
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AsrConfig;

    /// AsrEngine::new must error when model files are absent.
    #[test]
    fn new_fails_when_model_missing() {
        let mut config = AsrConfig::default();
        // Point model_dir at a directory that definitely has no models.
        let dir = tempfile::TempDir::new().unwrap();
        config.model_dir = dir.path().to_string_lossy().into_owned();
        config.model = "paraformer-zh".to_owned();

        let result = AsrEngine::new(&config);
        assert!(
            result.is_err(),
            "expected error when model files are absent"
        );
    }

    /// Unsupported model names must produce a clear error.
    #[test]
    fn unsupported_model_errors() {
        let mut config = AsrConfig::default();
        let dir = tempfile::TempDir::new().unwrap();
        config.model_dir = dir.path().to_string_lossy().into_owned();
        config.model = "not-a-real-model".to_owned();

        let err = AsrEngine::new(&config).err().expect("expected Err");
        assert!(
            err.to_string().contains("not-a-real-model"),
            "error should mention the unknown model name"
        );
    }
}
