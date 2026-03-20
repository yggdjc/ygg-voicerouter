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
// Mode enums
// ---------------------------------------------------------------------------

/// Hotkey activation mode.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyMode {
    /// Push-to-talk: active only while key is held.
    Ptt,
    /// Toggle: press once to start, press again to stop.
    Toggle,
    /// Automatic: short press toggles, long press acts as PTT.
    #[default]
    Auto,
}

/// How trailing punctuation is handled in ASR output.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PunctMode {
    /// Remove trailing punctuation marks.
    #[default]
    StripTrailing,
    /// Keep punctuation as produced by the ASR model.
    Keep,
    /// Replace space characters around punctuation.
    ReplaceSpace,
}

/// Text injection back-end.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum InjectMethod {
    /// Detect the best available method at runtime.
    #[default]
    Auto,
    /// Copy to clipboard and simulate paste keystroke.
    ClipboardPaste,
    /// Use `wtype` (Wayland).
    Wtype,
    /// Use `xdotool` (X11).
    Xdotool,
}

// ---------------------------------------------------------------------------
// Sub-config structs
// ---------------------------------------------------------------------------

/// Hotkey trigger configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeyConfig {
    /// evdev key name, e.g. `KEY_RIGHTALT`.
    pub key: String,
    /// Activation mode.
    pub mode: HotkeyMode,
    /// Seconds of hold before activating in `auto` mode.
    pub hold_delay: f64,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            key: "KEY_RIGHTALT".to_owned(),
            mode: HotkeyMode::default(),
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
    /// Whether to apply `RNNoise` denoising.
    pub denoise: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16000,
            channels: 1,
            silence_threshold: 0.012,
            silence_duration: 3.0,
            max_record_seconds: 60,
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
    /// How trailing punctuation is handled.
    pub punct_mode: PunctMode,
    /// Convert ASCII punctuation to fullwidth CJK equivalents.
    pub fullwidth_punct: bool,
    /// Attempt to fix spacing around inline English words.
    pub fix_english: bool,
    /// Restore punctuation using ct-transformer model (sherpa-onnx).
    pub restore_punctuation: bool,
    /// Path to the ct-transformer punctuation model directory.
    pub punctuation_model: String,
}

impl Default for PostprocessConfig {
    fn default() -> Self {
        Self {
            punct_mode: PunctMode::default(),
            fullwidth_punct: true,
            fix_english: true,
            restore_punctuation: true,
            punctuation_model: "~/.cache/voicerouter/models/ct-punc".to_owned(),
        }
    }
}

/// Text injection configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct InjectConfig {
    /// Injection back-end.
    pub method: InjectMethod,
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    pub enabled: bool,
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
    #[must_use]
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

    #[test]
    fn hotkey_mode_deserializes() {
        let toml = "[hotkey]\nmode = \"ptt\"\n";
        let config: Config = toml::from_str(toml).expect("parse failed");
        assert_eq!(config.hotkey.mode, HotkeyMode::Ptt);
    }

    #[test]
    fn punct_mode_deserializes() {
        let toml = "[postprocess]\npunct_mode = \"keep\"\n";
        let config: Config = toml::from_str(toml).expect("parse failed");
        assert_eq!(config.postprocess.punct_mode, PunctMode::Keep);
    }

    #[test]
    fn inject_method_deserializes() {
        let toml = "[inject]\nmethod = \"wtype\"\n";
        let config: Config = toml::from_str(toml).expect("parse failed");
        assert_eq!(config.inject.method, InjectMethod::Wtype);
    }

    #[test]
    fn default_modes_are_correct() {
        let config = Config::default();
        assert_eq!(config.hotkey.mode, HotkeyMode::Auto);
        assert_eq!(config.postprocess.punct_mode, PunctMode::StripTrailing);
        assert_eq!(config.inject.method, InjectMethod::Auto);
    }
}
