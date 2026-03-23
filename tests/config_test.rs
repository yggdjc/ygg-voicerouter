//! Integration tests for config loading via `Config::load()`.
//!
//! Unit tests for default values and deserialization live in
//! `src/config.rs #[cfg(test)]`. These tests exercise file I/O paths.

use std::io::Write as _;

use tempfile::NamedTempFile;
use voicerouter::config::{Config, HotkeyMode, InjectMethod, PunctMode};

#[test]
fn continuous_config_deserializes() {
    let toml = r#"
[continuous]
enabled = true
speaker_verify = true
speaker_threshold = 0.7
speaker_model = "3dspeaker"
vad_model = "silero"

[continuous.llm]
endpoint = "http://localhost:8080/v1"
model = "claude-haiku"
api_key_env = "TEST_KEY"
"#;
    let config: Config = toml::from_str(toml).expect("parse failed");
    assert!(config.continuous.enabled);
    assert_eq!(config.continuous.speaker_threshold, 0.7);
    assert_eq!(config.continuous.llm.model, "claude-haiku");
    assert_eq!(config.continuous.llm.api_key_env, "TEST_KEY");
}

#[test]
fn continuous_config_defaults() {
    let config = Config::default();
    assert!(!config.continuous.enabled);
    assert!(config.continuous.speaker_verify);
    assert_eq!(config.continuous.speaker_threshold, 0.6);
    assert_eq!(config.continuous.llm.model, "claude-haiku");
}

/// The bundled default config shipped to users by `voicerouter setup`.
const DEFAULT_CONFIG: &str = include_str!("../config.default.toml");

#[test]
fn default_config_toml_parses() {
    let config: Config =
        toml::from_str(DEFAULT_CONFIG).expect("config.default.toml must parse");

    assert_eq!(config.hotkey.key, "KEY_RIGHTALT");
    assert_eq!(config.hotkey.mode, HotkeyMode::Auto);
    assert_eq!(config.audio.sample_rate, 16000);
    assert_eq!(config.postprocess.punct_mode, PunctMode::StripTrailing);
    assert_eq!(config.inject.method, InjectMethod::Auto);
    assert!(config.router.rules.is_empty());
    assert!(config.sound.feedback);
}

#[test]
fn load_from_explicit_path() {
    let mut tmp = NamedTempFile::new().expect("tempfile creation failed");
    write!(tmp, "{DEFAULT_CONFIG}").expect("write failed");

    let path = tmp.path().to_str().expect("path is valid UTF-8");
    let config = Config::load(Some(path)).expect("Config::load must succeed");

    assert_eq!(config.hotkey.key, "KEY_RIGHTALT");
    assert_eq!(config.audio.sample_rate, 16000);
}

#[test]
fn load_nonexistent_path_returns_default() {
    let config = Config::load(Some("/nonexistent/path/voicerouter/config.toml"))
        .expect("missing file must not error");
    assert_eq!(config.hotkey.key, "KEY_RIGHTALT");
}

#[test]
fn partial_config_file_fills_defaults() {
    let mut tmp = NamedTempFile::new().expect("tempfile creation failed");
    writeln!(tmp, "[audio]").expect("write failed");
    writeln!(tmp, "sample_rate = 8000").expect("write failed");

    let path = tmp.path().to_str().expect("path is valid UTF-8");
    let config = Config::load(Some(path)).expect("partial config must parse");

    assert_eq!(config.audio.sample_rate, 8000);
    assert_eq!(config.hotkey.key, "KEY_RIGHTALT");
    assert!(config.sound.feedback);
}
