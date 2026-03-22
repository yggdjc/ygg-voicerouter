//! Integration tests for the post-processing pipeline.
//!
//! Unit tests for individual functions (`half_to_fullwidth`, `fix_broken_english`,
//! `apply_punct_mode`) live in their respective `#[cfg(test)]` modules.
//! These tests exercise the full `postprocess()` pipeline across modules.

use voicerouter::config::{PostprocessConfig, PunctMode};
use voicerouter::postprocess::postprocess;

fn cfg(fix: bool, fw: bool, mode: PunctMode) -> PostprocessConfig {
    PostprocessConfig {
        fix_english: fix,
        fullwidth_punct: fw,
        punct_mode: mode,
        ..Default::default()
    }
}

#[test]
fn pipeline_all_disabled_passthrough() {
    assert_eq!(
        postprocess("Hello, world.", &cfg(false, false, PunctMode::Keep)),
        "Hello, world."
    );
}

#[test]
fn pipeline_fullwidth_only() {
    assert_eq!(
        postprocess("你好,世界.", &cfg(false, true, PunctMode::Keep)),
        "你好，世界。"
    );
}

#[test]
fn pipeline_fullwidth_strip_trailing() {
    assert_eq!(
        postprocess("你好,世界.", &cfg(false, true, PunctMode::StripTrailing)),
        "你好，世界"
    );
}

#[test]
fn pipeline_english_fix_and_strip() {
    assert_eq!(
        postprocess("G P T is great.", &cfg(true, false, PunctMode::StripTrailing)),
        "GPT is great"
    );
}

#[test]
fn pipeline_full() {
    assert_eq!(
        postprocess("你好,世界.", &cfg(true, true, PunctMode::StripTrailing)),
        "你好，世界"
    );
}

#[test]
fn pipeline_replace_space() {
    assert_eq!(
        postprocess("Hello. World", &cfg(false, false, PunctMode::ReplaceSpace)),
        "Hello.World"
    );
}

#[test]
fn pipeline_english_fix_disabled_leaves_broken() {
    assert_eq!(
        postprocess("T oken", &cfg(false, false, PunctMode::Keep)),
        "T oken"
    );
}

#[test]
fn pipeline_empty_input() {
    assert_eq!(
        postprocess("", &cfg(true, true, PunctMode::StripTrailing)),
        ""
    );
}

#[test]
fn pipeline_cjk_with_english_acronym_and_punct() {
    assert_eq!(
        postprocess("G P T 很好,", &cfg(true, true, PunctMode::StripTrailing)),
        "GPT 很好"
    );
}
