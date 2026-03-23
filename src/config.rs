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

/// Error handling policy for pipeline execution.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorPolicy {
    #[default]
    FailFast,
    BestEffort,
}

/// Action to take when a wakeword is detected.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WakewordAction {
    #[default]
    StartRecording,
    PipelinePassthrough,
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
}

impl Default for AsrConfig {
    fn default() -> Self {
        Self {
            model: "paraformer-zh".to_owned(),
            model_dir: "~/.cache/voicerouter/models".to_owned(),
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
    /// Remove onomatopoeic hesitation fillers (嗯、啊、呃 etc.) from output.
    pub remove_fillers: bool,
    /// Normalize spoken forms to written (Chinese numbers→digits, 点→.).
    pub normalize_spoken: bool,
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
            remove_fillers: true,
            normalize_spoken: true,
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
    /// Prefix pattern matched against transcript text.
    pub trigger: String,
    /// Handler name: "inject" or "shell".
    pub handler: String,
    /// Shell command template for "shell" handler. Use `{text}` as placeholder
    /// for the payload (trigger-stripped text). If absent, payload is executed
    /// as-is.
    pub command: Option<String>,
}

/// Router configuration with an optional list of dispatch rules.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RouterConfig {
    /// Ordered list of routing rules. Evaluated top-to-bottom; first match wins.
    pub rules: Vec<Rule>,
}

/// A single pipeline stage definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageConfig {
    pub name: String,
    pub handler: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub condition: Option<String>,
    #[serde(default)]
    pub after: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default = "default_stage_timeout")]
    pub timeout: u64,
}

fn default_stage_timeout() -> u64 {
    10
}

/// Pipeline execution configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PipelineConfig {
    pub stages: Vec<StageConfig>,
    pub error_policy: ErrorPolicy,
    pub max_parallel_stages: usize,
    pub max_concurrent_executions: usize,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            stages: Vec::new(),
            error_policy: ErrorPolicy::default(),
            max_parallel_stages: 4,
            max_concurrent_executions: 2,
        }
    }
}

/// IPC (Unix socket) server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IpcConfig {
    pub enabled: bool,
    pub socket_path: String,
    pub max_connections: usize,
}

impl Default for IpcConfig {
    fn default() -> Self {
        Self { enabled: true, socket_path: String::new(), max_connections: 8 }
    }
}

/// Text-to-speech output configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TtsConfig {
    pub enabled: bool,
    pub engine: String,
    pub model: String,
    pub model_dir: String,
    pub speed: f64,
    pub mute_mic_during_playback: bool,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            engine: "sherpa-onnx".to_owned(),
            model: "kokoro-tts".to_owned(),
            model_dir: "~/.cache/voicerouter/models".to_owned(),
            speed: 1.0,
            mute_mic_during_playback: true,
        }
    }
}

/// Wakeword detection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WakewordConfig {
    pub enabled: bool,
    pub phrases: Vec<String>,
    pub window_seconds: f64,
    pub stride_seconds: f64,
    pub action: WakewordAction,
    pub model: String,
}

impl Default for WakewordConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            phrases: Vec::new(),
            window_seconds: 2.0,
            stride_seconds: 1.0,
            action: WakewordAction::default(),
            model: String::new(),
        }
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
    pub sound: SoundConfig,
    pub pipeline: PipelineConfig,
    pub ipc: IpcConfig,
    pub tts: TtsConfig,
    pub wakeword: WakewordConfig,
}

impl Config {
    /// Return the effective pipeline stages, migrating from legacy `[router]` rules
    /// if `[pipeline]` is not configured.
    ///
    /// If both sections are present, `[pipeline]` takes precedence and a deprecation
    /// warning is emitted for `[router]`.
    #[must_use]
    pub fn effective_pipeline_stages(&self) -> Vec<StageConfig> {
        if !self.pipeline.stages.is_empty() {
            if !self.router.rules.is_empty() {
                log::warn!(
                    "[config] both [router] and [pipeline] defined; \
                     [router] is deprecated and will be ignored"
                );
            }
            return self.pipeline.stages.clone();
        }
        if !self.router.rules.is_empty() {
            log::warn!("[config] [router] is deprecated; migrate to [pipeline]");
            let mut stages: Vec<StageConfig> = self.router.rules.iter().enumerate().map(|(i, rule)| StageConfig {
                name: format!("router_rule_{i}"),
                handler: rule.handler.clone(),
                command: rule.command.clone(),
                condition: Some(format!("starts_with:{}", rule.trigger)),
                after: None,
                url: None,
                method: None,
                body: None,
                timeout: default_stage_timeout(),
            }).collect();
            // Append default inject as fallback for unmatched text.
            stages.push(StageConfig {
                name: "default".into(),
                handler: "inject".into(),
                command: None,
                condition: None,
                after: None,
                url: None,
                method: None,
                body: None,
                timeout: default_stage_timeout(),
            });
            return stages;
        }

        // No pipeline config → default single inject handler.
        vec![StageConfig {
            name: "default".into(),
            handler: "inject".into(),
            command: None,
            condition: None,
            after: None,
            url: None,
            method: None,
            body: None,
            timeout: default_stage_timeout(),
        }]
    }

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

    #[test]
    fn pipeline_config_defaults() {
        let config = Config::default();
        assert!(config.pipeline.stages.is_empty());
        assert_eq!(config.pipeline.error_policy, ErrorPolicy::FailFast);
    }

    #[test]
    fn ipc_config_defaults() {
        let config = Config::default();
        assert!(config.ipc.enabled);
        assert_eq!(config.ipc.max_connections, 8);
    }

    #[test]
    fn pipeline_stage_deserializes() {
        let toml = "[[pipeline.stages]]\nname = \"default\"\nhandler = \"inject\"\n";
        let config: Config = toml::from_str(toml).expect("parse failed");
        assert_eq!(config.pipeline.stages.len(), 1);
        assert_eq!(config.pipeline.stages[0].name, "default");
    }

    #[test]
    fn pipeline_stage_with_condition() {
        let toml = "[[pipeline.stages]]\nname = \"search\"\nhandler = \"shell\"\ncommand = \"firefox {text}\"\ncondition = \"starts_with:搜索\"\n";
        let config: Config = toml::from_str(toml).expect("parse failed");
        assert_eq!(config.pipeline.stages[0].condition.as_deref(), Some("starts_with:搜索"));
    }

    #[test]
    fn router_rules_migrate_to_pipeline() {
        let toml = "[[router.rules]]\ntrigger = \"搜索\"\nhandler = \"shell\"\ncommand = \"firefox https://google.com/search?q={text}\"\n";
        let config: Config = toml::from_str(toml).expect("parse failed");
        let stages = config.effective_pipeline_stages();
        assert_eq!(stages.len(), 2);
        assert_eq!(stages[0].name, "router_rule_0");
        assert_eq!(stages[0].handler, "shell");
        assert_eq!(stages[0].condition.as_deref(), Some("starts_with:搜索"));
        // Fallback inject stage appended automatically.
        assert_eq!(stages[1].name, "default");
        assert_eq!(stages[1].handler, "inject");
    }

    #[test]
    fn pipeline_stages_take_precedence_over_router() {
        let toml = "[[router.rules]]\ntrigger = \"old\"\nhandler = \"shell\"\n\n[[pipeline.stages]]\nname = \"new\"\nhandler = \"inject\"\n";
        let config: Config = toml::from_str(toml).expect("parse failed");
        let stages = config.effective_pipeline_stages();
        assert_eq!(stages.len(), 1);
        assert_eq!(stages[0].name, "new");
    }

    #[test]
    fn tts_config_defaults() {
        let config = Config::default();
        assert!(!config.tts.enabled);
        assert_eq!(config.tts.engine, "sherpa-onnx");
    }

    #[test]
    fn wakeword_config_defaults() {
        let config = Config::default();
        assert!(!config.wakeword.enabled);
        assert!(config.wakeword.phrases.is_empty());
    }
}
