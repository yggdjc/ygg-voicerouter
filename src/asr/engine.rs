//! ASR engine wrapping sherpa-rs offline recognizers.
//!
//! # Design
//!
//! sherpa-rs 0.6 exposes **offline-only** recognizers; there is no
//! streaming/online API for Paraformer in this version.
//!
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

use crate::config::AsrConfig;

use super::models::{get_model_paths, prepare_model_dir};

// ---------------------------------------------------------------------------
// Backend enum
// ---------------------------------------------------------------------------

/// Inner recognizer, one variant per supported model family.
enum Backend {
    Paraformer(sherpa_rs::paraformer::ParaformerRecognizer),
    Whisper(sherpa_rs::whisper::WhisperRecognizer),
}

// ---------------------------------------------------------------------------
// AsrEngine
// ---------------------------------------------------------------------------

/// Offline ASR engine backed by a sherpa-onnx recognizer.
///
/// Construct with [`AsrEngine::new`] and call [`AsrEngine::transcribe`] for
/// each audio buffer.
// Backend contains raw pointers; those don't implement Debug, so we skip
// deriving it for the struct and implement a manual one that omits internals.
pub struct AsrEngine {
    backend: Backend,
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

        let backend = build_backend(&config.model, &paths)?;

        Ok(Self {
            backend,
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

        let text = match &mut self.backend {
            Backend::Paraformer(rec) => rec.transcribe(sample_rate, audio).text,
            Backend::Whisper(rec) => rec.transcribe(sample_rate, audio).text,
        };

        Ok(text.trim().to_owned())
    }

}

// ---------------------------------------------------------------------------
// Backend construction
// ---------------------------------------------------------------------------

fn build_backend(
    model_name: &str,
    paths: &super::models::ModelPaths,
) -> Result<Backend> {
    match model_name {
        "paraformer-zh" => {
            let cfg = sherpa_rs::paraformer::ParaformerConfig {
                model: paths.model.to_string_lossy().into_owned(),
                tokens: paths.tokens.to_string_lossy().into_owned(),
                ..Default::default()
            };
            let rec = sherpa_rs::paraformer::ParaformerRecognizer::new(cfg)
                .map_err(|e| anyhow::anyhow!("ParaformerRecognizer init failed: {e}"))?;
            Ok(Backend::Paraformer(rec))
        }
        "whisper-tiny-en" | "whisper-base-en" => {
            // extras[0] is the decoder
            let decoder = paths
                .extras
                .first()
                .with_context(|| format!("missing decoder path for model '{model_name}'"))?;
            let cfg = sherpa_rs::whisper::WhisperConfig {
                encoder: paths.model.to_string_lossy().into_owned(),
                decoder: decoder.to_string_lossy().into_owned(),
                tokens: paths.tokens.to_string_lossy().into_owned(),
                language: "en".to_owned(),
                ..Default::default()
            };
            let rec = sherpa_rs::whisper::WhisperRecognizer::new(cfg)
                .map_err(|e| anyhow::anyhow!("WhisperRecognizer init failed: {e}"))?;
            Ok(Backend::Whisper(rec))
        }
        other => anyhow::bail!("no backend for model '{other}'"),
    }
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
