//! Configuration types and loading logic for voicerouter.
//!
//! Configuration is loaded from a TOML file. All sections have sensible
//! defaults so a partial config is valid.
//!
//! # Example
//!
//! ```no_run
//! use voicerouter::config::Config;
//!
//! let config = Config::load(None).expect("failed to load config");
//! println!("hotkey: {}", config.hotkey.key);
//! ```

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Sub-config structs
// ---------------------------------------------------------------------------

/// Hotkey trigger configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeyConfig {
    /// evdev key name, e.g. `KEY_RIGHTALT`.
    pub key: String,
    /// Activation mode: `ptt`, `toggle`, or `auto`.
    pub mode: String,
    /// Seconds of hold before activating in `auto` mode.
    pub hold_delay: f64,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            key: "KEY_RIGHTALT".to_owned(),
            mode: "auto".to_owned(),
            hold_delay: 0.3,
        }
    }
}

/// Audio capture configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    pub sample_rate: u32,
    pub channels: u16,
    /// RMS amplitude below which audio is considered silence.
    pub silence_threshold: f64,
    /// Seconds of silence before recording stops.
    pub silence_duration: f64,
    /// Hard cap on recording length in seconds.
    pub max_record_seconds: u32,
    /// Whether to apply RNNoise denoising.
    pub denoise: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16000,
            channels: 1,
            silence_threshold: 0.01,
            silence_duration: 1.5,
            max_record_seconds: 30,
            denoise: true,
        }
    }
}

/// ASR (speech recognition) configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AsrConfig {
    /// Model identifier, e.g. `paraformer-zh`.
    pub model: String,
    /// Directory where model files are stored (tilde-expanded at runtime).
    pub model_dir: String,
    /// Use streaming (online) inference when available.
    pub streaming: bool,
}

impl Default for AsrConfig {
    fn default() -> Self {
        Self {
            model: "paraformer-zh".to_owned(),
            model_dir: "~/.cache/voicerouter/models".to_owned(),
            streaming: true,
        }
    }
}

/// Post-processing configuration applied to raw ASR output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PostprocessConfig {
    /// How trailing punctuation is handled: `strip_trailing` or `keep`.
    pub punct_mode: String,
    /// Convert ASCII punctuation to fullwidth CJK equivalents.
    pub fullwidth_punct: bool,
    /// Attempt to fix spacing around inline English words.
    pub fix_english: bool,
}

impl Default for PostprocessConfig {
    fn default() -> Self {
        Self {
            punct_mode: "strip_trailing".to_owned(),
            fullwidth_punct: true,
            fix_english: true,
        }
    }
}

/// Text injection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InjectConfig {
    /// Injection back-end: `auto`, `xdotool`, `ydotool`, or `clipboard`.
    pub method: String,
}

impl Default for InjectConfig {
    fn default() -> Self {
        Self {
            method: "auto".to_owned(),
        }
    }
}

/// A single routing rule mapping a trigger pattern to a handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    /// Regex or prefix pattern matched against transcript text.
    pub trigger: String,
    /// Handler name or command to invoke when matched.
    pub handler: String,
}

/// Router configuration with an optional list of dispatch rules.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RouterConfig {
    /// Ordered list of routing rules. Evaluated top-to-bottom; first match wins.
    pub rules: Vec<Rule>,
}

/// LLM integration configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    pub enabled: bool,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self { enabled: false }
    }
}

/// Audio feedback (earcon) configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundConfig {
    /// Play start/stop feedback sounds.
    pub feedback: bool,
}

impl Default for SoundConfig {
    fn default() -> Self {
        Self { feedback: true }
    }
}

// ---------------------------------------------------------------------------
// Top-level Config
// ---------------------------------------------------------------------------

/// Complete voicerouter configuration.
///
/// All sections default to sensible values so that an empty or partial TOML
/// file is valid.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub hotkey: HotkeyConfig,
    pub audio: AudioConfig,
    pub asr: AsrConfig,
    pub postprocess: PostprocessConfig,
    pub inject: InjectConfig,
    pub router: RouterConfig,
    pub llm: LlmConfig,
    pub sound: SoundConfig,
}

impl Config {
    /// Return the default config file path: `~/.config/voicerouter/config.toml`.
    ///
    /// Returns `None` if the home directory cannot be determined.
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("voicerouter").join("config.toml"))
    }

    /// Load configuration from `path`, or from `Config::default_path()` if
    /// `path` is `None`.
    ///
    /// If the resolved file does not exist the default `Config` is returned
    /// without error, allowing first-run usage before any setup.
    ///
    /// # Errors
    ///
    /// Returns an error if the file exists but cannot be read or parsed.
    pub fn load(path: Option<&str>) -> Result<Self> {
        let resolved: Option<PathBuf> = match path {
            Some(p) => Some(PathBuf::from(p)),
            None => Self::default_path(),
        };

        let Some(config_path) = resolved else {
            return Ok(Self::default());
        };

        if !config_path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&config_path)
            .with_context(|| format!("reading config file: {}", config_path.display()))?;

        toml::from_str(&content)
            .with_context(|| format!("parsing config file: {}", config_path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        let config = Config::default();
        assert_eq!(config.hotkey.key, "KEY_RIGHTALT");
        assert_eq!(config.audio.sample_rate, 16000);
        assert!(!config.llm.enabled);
        assert!(config.sound.feedback);
    }

    #[test]
    fn partial_toml_uses_defaults() {
        let toml = "[audio]\nsample_rate = 8000\n";
        let config: Config = toml::from_str(toml).expect("parse failed");
        assert_eq!(config.audio.sample_rate, 8000);
        // Other fields from default
        assert_eq!(config.hotkey.key, "KEY_RIGHTALT");
    }

    #[test]
    fn router_rules_default_empty() {
        let config = Config::default();
        assert!(config.router.rules.is_empty());
    }
}
