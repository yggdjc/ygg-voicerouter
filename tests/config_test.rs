//! Integration tests for config loading.

use std::io::Write as _;

use tempfile::NamedTempFile;
use voicerouter::config::{Config, HotkeyMode, InjectMethod, PunctMode};

/// The bundled default config as a compile-time string.
const DEFAULT_CONFIG: &str = include_str!("../defaults/config.toml");

#[test]
fn default_config_toml_parses() {
    let config: Config = toml::from_str(DEFAULT_CONFIG).expect("defaults/config.toml must parse");

    // [hotkey]
    assert_eq!(config.hotkey.key, "KEY_RIGHTALT");
    assert_eq!(config.hotkey.mode, HotkeyMode::Auto);
    assert!((config.hotkey.hold_delay - 0.3).abs() < f64::EPSILON);

    // [audio]
    assert_eq!(config.audio.sample_rate, 16000);
    assert_eq!(config.audio.channels, 1);
    assert!((config.audio.silence_threshold - 0.01).abs() < f64::EPSILON);
    assert!((config.audio.silence_duration - 1.5).abs() < f64::EPSILON);
    assert_eq!(config.audio.max_record_seconds, 30);
    assert!(config.audio.denoise);

    // [asr]
    assert_eq!(config.asr.model, "paraformer-zh");
    assert_eq!(config.asr.model_dir, "~/.cache/voicerouter/models");
    assert!(config.asr.streaming);

    // [postprocess]
    assert_eq!(config.postprocess.punct_mode, PunctMode::StripTrailing);
    assert!(config.postprocess.fullwidth_punct);
    assert!(config.postprocess.fix_english);

    // [inject]
    assert_eq!(config.inject.method, InjectMethod::Auto);

    // [router] — rules must default to empty
    assert!(config.router.rules.is_empty());

    // [llm]
    assert!(!config.llm.enabled);

    // [sound]
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
    // A path that definitely does not exist returns the default config without error.
    let config = Config::load(Some("/nonexistent/path/voicerouter/config.toml"))
        .expect("missing file must not error");

    // Spot-check defaults
    assert_eq!(config.hotkey.key, "KEY_RIGHTALT");
    assert!(!config.llm.enabled);
}

#[test]
fn partial_config_file_fills_defaults() {
    let mut tmp = NamedTempFile::new().expect("tempfile creation failed");
    writeln!(tmp, "[audio]").expect("write failed");
    writeln!(tmp, "sample_rate = 8000").expect("write failed");

    let path = tmp.path().to_str().expect("path is valid UTF-8");
    let config = Config::load(Some(path)).expect("partial config must parse");

    assert_eq!(config.audio.sample_rate, 8000);
    // Unspecified fields fall back to defaults
    assert_eq!(config.hotkey.key, "KEY_RIGHTALT");
    assert!(config.sound.feedback);
}
