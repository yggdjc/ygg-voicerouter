//! Model download and path management for ASR models.
//!
//! Supported models:
//! - `paraformer-zh`: Chinese offline model (bilingual zh/en).
//! - `whisper-tiny-en`, `whisper-base-en`: Whisper variants for English.
//! - `funasr-nano`: FunASR Nano 0.8B LLM-based model (zh/en/ja, with ITN).
//!
//! Model files are stored under a configurable directory (default:
//! `~/.cache/voicerouter/models/<model_name>/`).

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use log::{info, warn};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Metadata for a single file belonging to a model.
#[derive(Debug, Clone)]
pub struct ModelFile {
    /// Remote URL where the file can be downloaded.
    pub url: String,
    /// Absolute local path where the file should be stored.
    pub local_path: PathBuf,
}

/// Full metadata for a supported model, including all required files.
#[derive(Debug, Clone)]
pub struct ModelInfo {
    /// Unique model identifier, matching what `AsrConfig::model` expects.
    pub name: String,
    /// All files that must be present for the model to be usable.
    pub files: Vec<ModelFile>,
}

/// Resolved paths for the files that sherpa-onnx needs at inference time.
#[derive(Debug, Clone)]
pub struct ModelPaths {
    /// Path to the ONNX model file.
    pub model: PathBuf,
    /// Path to the tokens/vocabulary file.
    pub tokens: PathBuf,
    /// Additional paths (encoder, decoder, etc.) for multi-file models.
    pub extras: Vec<PathBuf>,
}

// ---------------------------------------------------------------------------
// Model registry
// ---------------------------------------------------------------------------

/// Return the canonical [`ModelInfo`] for `model_name` relative to `model_dir`.
///
/// # Errors
///
/// Returns an error if `model_name` is not in the registry.
pub fn model_info(model_name: &str, model_dir: &Path) -> Result<ModelInfo> {
    let base = model_dir.join(model_name);
    match model_name {
        "paraformer-zh" => Ok(ModelInfo {
            name: model_name.to_owned(),
            files: vec![
                ModelFile {
                    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/\
                          sherpa-onnx-paraformer-zh-2023-09-14.tar.bz2"
                        .to_owned(),
                    local_path: base.join("model.int8.onnx"),
                },
                ModelFile {
                    url: String::new(), // bundled inside the same archive
                    local_path: base.join("tokens.txt"),
                },
            ],
        }),
        "whisper-tiny-en" => Ok(ModelInfo {
            name: model_name.to_owned(),
            files: vec![
                ModelFile {
                    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/\
                          sherpa-onnx-whisper-tiny.en.tar.bz2"
                        .to_owned(),
                    local_path: base.join("tiny.en-encoder.int8.onnx"),
                },
                ModelFile {
                    url: String::new(),
                    local_path: base.join("tiny.en-tokens.txt"),
                },
                ModelFile {
                    url: String::new(),
                    local_path: base.join("tiny.en-decoder.int8.onnx"),
                },
            ],
        }),
        "whisper-base-en" => Ok(ModelInfo {
            name: model_name.to_owned(),
            files: vec![
                ModelFile {
                    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/\
                          sherpa-onnx-whisper-base.en.tar.bz2"
                        .to_owned(),
                    local_path: base.join("base.en-encoder.int8.onnx"),
                },
                ModelFile {
                    url: String::new(),
                    local_path: base.join("base.en-tokens.txt"),
                },
                ModelFile {
                    url: String::new(),
                    local_path: base.join("base.en-decoder.int8.onnx"),
                },
            ],
        }),
        "funasr-nano" => Ok(ModelInfo {
            name: model_name.to_owned(),
            files: vec![
                ModelFile {
                    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/\
                          sherpa-onnx-funasr-nano-int8-2025-12-30.tar.bz2"
                        .to_owned(),
                    local_path: base.join("encoder_adaptor.int8.onnx"),
                },
                ModelFile {
                    url: String::new(),
                    local_path: base.join("llm.int8.onnx"),
                },
                ModelFile {
                    url: String::new(),
                    local_path: base.join("embedding.int8.onnx"),
                },
                ModelFile {
                    url: String::new(),
                    local_path: base.join("Qwen3-0.6B"),
                },
            ],
        }),
        other => bail!(
            "unsupported model '{other}'. Supported: paraformer-zh, funasr-nano, whisper-tiny-en, whisper-base-en"
        ),
    }
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Return [`ModelPaths`] for `model_name` inside `model_dir` without checking
/// whether the files actually exist.
///
/// Derived from [`model_info`] so the model registry is defined in one place.
///
/// # Errors
///
/// Returns an error if `model_name` is not in the registry.
pub fn get_model_paths(model_name: &str, model_dir: &Path) -> Result<ModelPaths> {
    let info = model_info(model_name, model_dir)?;
    let mut files = info.files.into_iter();

    let model = files
        .next()
        .map(|f| f.local_path)
        .context("model has no files")?;

    let tokens_path = files
        .next()
        .map(|f| f.local_path)
        .context("model has no tokens file")?;

    let extras: Vec<PathBuf> = files.map(|f| f.local_path).collect();

    Ok(ModelPaths {
        model,
        tokens: tokens_path,
        extras,
    })
}

/// Check whether all required files for `model_name` are present in
/// `model_dir`.
///
/// # Errors
///
/// Returns an error if `model_name` is not in the registry.
pub fn model_files_exist(model_name: &str, model_dir: &Path) -> Result<bool> {
    let info = model_info(model_name, model_dir)?;
    Ok(info.files.iter().all(|f| f.local_path.exists()))
}

/// Ensure that the model files for `model_name` are present under `model_dir`.
///
/// If the files already exist, returns immediately. If they are missing, prints
/// download instructions and returns an error — automatic downloading of
/// large archives (.tar.bz2) is deferred to manual setup to avoid silent
/// multi-hundred-MB fetches during runtime.
///
/// # Errors
///
/// Returns an error when model files are absent (with instructions) or when
/// `model_name` is not in the registry.
pub fn ensure_model(model_name: &str, model_dir: &Path) -> Result<ModelPaths> {
    let paths = get_model_paths(model_name, model_dir)?;

    if model_files_exist(model_name, model_dir)? {
        return Ok(paths);
    }

    // Files are missing — provide actionable instructions instead of silently
    // downloading hundreds of MB.
    let info = model_info(model_name, model_dir)?;
    let archive_url = info
        .files
        .iter()
        .find(|f| !f.url.is_empty())
        .map_or("(unknown URL)", |f| f.url.as_str());
    let target_dir = model_dir.join(model_name);

    warn!("Model '{model_name}' files not found in {}", target_dir.display());
    warn!("To install, run:");
    warn!("  mkdir -p {}", target_dir.display());
    warn!("  cd {}", model_dir.display());
    warn!("  curl -LO {archive_url}");
    warn!("  tar -xjf $(basename {archive_url})");
    warn!("  mv sherpa-onnx-*/ {model_name}/");

    bail!(
        "model '{model_name}' not found in {}. See log output for installation instructions.",
        target_dir.display()
    )
}

/// Expand a leading `~` in `path` using the home directory from the
/// environment.  Returns `path` unchanged if it does not start with `~`.
///
/// # Errors
///
/// Returns an error if `~` is present but the home directory cannot be
/// resolved.
pub fn expand_tilde(path: &str) -> Result<PathBuf> {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = dirs::home_dir().context("cannot determine home directory")?;
        Ok(home.join(rest))
    } else if path == "~" {
        dirs::home_dir().context("cannot determine home directory")
    } else {
        Ok(PathBuf::from(path))
    }
}

/// Resolve and create the model directory.
///
/// Tilde expansion is applied, then the directory (and all parents) are
/// created if they do not yet exist.
///
/// # Errors
///
/// Returns an error if the directory cannot be created or tilde expansion
/// fails.
pub fn prepare_model_dir(raw_path: &str) -> Result<PathBuf> {
    let dir = expand_tilde(raw_path)?;
    if !dir.exists() {
        info!("Creating model directory: {}", dir.display());
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("creating model directory: {}", dir.display()))?;
    }
    Ok(dir)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn unsupported_model_errors() {
        let dir = TempDir::new().unwrap();
        assert!(model_info("nonexistent-model", dir.path()).is_err());
        assert!(get_model_paths("nonexistent-model", dir.path()).is_err());
    }

    #[test]
    fn paraformer_paths_are_correct() {
        let dir = TempDir::new().unwrap();
        let paths = get_model_paths("paraformer-zh", dir.path()).unwrap();
        assert!(paths.model.ends_with("model.int8.onnx"));
        assert!(paths.tokens.ends_with("tokens.txt"));
        assert!(paths.extras.is_empty());
    }

    #[test]
    fn whisper_tiny_has_decoder_extra() {
        let dir = TempDir::new().unwrap();
        let paths = get_model_paths("whisper-tiny-en", dir.path()).unwrap();
        assert_eq!(paths.extras.len(), 1);
        assert!(paths.extras[0].ends_with("tiny.en-decoder.int8.onnx"));
    }

    #[test]
    fn model_files_absent_returns_false() {
        let dir = TempDir::new().unwrap();
        assert!(!model_files_exist("paraformer-zh", dir.path()).unwrap());
    }

    #[test]
    fn model_files_present_returns_true() {
        let dir = TempDir::new().unwrap();
        let model_dir = dir.path().join("paraformer-zh");
        std::fs::create_dir_all(&model_dir).unwrap();
        std::fs::write(model_dir.join("model.int8.onnx"), b"").unwrap();
        std::fs::write(model_dir.join("tokens.txt"), b"").unwrap();
        assert!(model_files_exist("paraformer-zh", dir.path()).unwrap());
    }

    #[test]
    fn ensure_model_errors_when_missing() {
        let dir = TempDir::new().unwrap();
        let result = ensure_model("paraformer-zh", dir.path());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("paraformer-zh"));
    }

    #[test]
    fn expand_tilde_home() {
        let result = expand_tilde("~/foo/bar").unwrap();
        let home = dirs::home_dir().unwrap();
        assert_eq!(result, home.join("foo/bar"));
    }

    #[test]
    fn expand_tilde_no_tilde() {
        let result = expand_tilde("/absolute/path").unwrap();
        assert_eq!(result, PathBuf::from("/absolute/path"));
    }

    #[test]
    fn prepare_model_dir_creates_dir() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("deep").join("nested");
        let raw = sub.to_str().unwrap();
        let result = prepare_model_dir(raw).unwrap();
        assert!(result.exists());
    }
}
