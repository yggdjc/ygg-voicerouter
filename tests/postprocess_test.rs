//! Integration tests for the post-processing pipeline.

use voicerouter::config::{PostprocessConfig, PunctMode};
use voicerouter::postprocess::{
    english_fix::fix_broken_english,
    postprocess,
    punctuation::{apply_punct_mode, half_to_fullwidth},
};

// ---------------------------------------------------------------------------
// Punctuation tests
// ---------------------------------------------------------------------------

#[test]
fn punct_comma_cjk_adjacent() {
    assert_eq!(half_to_fullwidth("你好,世界"), "你好，世界");
}

#[test]
fn punct_comma_ascii_unchanged() {
    assert_eq!(half_to_fullwidth("Hello, world"), "Hello, world");
}

#[test]
fn punct_period_cjk_adjacent() {
    assert_eq!(half_to_fullwidth("测试.结束"), "测试。结束");
}

#[test]
fn punct_period_ascii_unchanged() {
    assert_eq!(half_to_fullwidth("test.end"), "test.end");
}

#[test]
fn punct_mixed_right_cjk_triggers_conversion() {
    assert_eq!(half_to_fullwidth("你好world,测试"), "你好world，测试");
}

#[test]
fn punct_colon_cjk_adjacent() {
    assert_eq!(half_to_fullwidth("你好:世界"), "你好：世界");
}

#[test]
fn punct_semicolon_cjk_adjacent() {
    assert_eq!(half_to_fullwidth("你好;世界"), "你好；世界");
}

#[test]
fn punct_question_cjk_adjacent() {
    assert_eq!(half_to_fullwidth("你好?"), "你好？");
}

#[test]
fn punct_exclamation_cjk_adjacent() {
    assert_eq!(half_to_fullwidth("你好!"), "你好！");
}

#[test]
fn punct_parentheses_cjk_adjacent() {
    assert_eq!(half_to_fullwidth("你(好)"), "你（好）");
}

#[test]
fn punct_empty_string() {
    assert_eq!(half_to_fullwidth(""), "");
}

#[test]
fn punct_no_cjk_all_ascii_unchanged() {
    assert_eq!(half_to_fullwidth("Hello, world! How are you?"), "Hello, world! How are you?");
}

#[test]
fn punct_mode_keep_unchanged() {
    assert_eq!(apply_punct_mode("Hello.", PunctMode::Keep), "Hello.");
    assert_eq!(apply_punct_mode("你好。", PunctMode::Keep), "你好。");
}

#[test]
fn punct_mode_strip_trailing_ascii() {
    assert_eq!(apply_punct_mode("Hello.", PunctMode::StripTrailing), "Hello");
    assert_eq!(apply_punct_mode("Hello!!", PunctMode::StripTrailing), "Hello");
    assert_eq!(apply_punct_mode("Hello?,", PunctMode::StripTrailing), "Hello");
}

#[test]
fn punct_mode_strip_trailing_cjk() {
    assert_eq!(apply_punct_mode("你好。", PunctMode::StripTrailing), "你好");
    assert_eq!(apply_punct_mode("你好！", PunctMode::StripTrailing), "你好");
}

#[test]
fn punct_mode_strip_trailing_no_punct() {
    assert_eq!(apply_punct_mode("Hello", PunctMode::StripTrailing), "Hello");
}

#[test]
fn punct_mode_replace_space_removes_post_punct_space() {
    assert_eq!(apply_punct_mode("Hello. World", PunctMode::ReplaceSpace), "Hello.World");
}

#[test]
fn punct_mode_replace_space_multiple() {
    assert_eq!(
        apply_punct_mode("Hi. Hello. World", PunctMode::ReplaceSpace),
        "Hi.Hello.World"
    );
}

#[test]
fn punct_mode_replace_space_no_space_after_punct() {
    assert_eq!(apply_punct_mode("Hello.", PunctMode::ReplaceSpace), "Hello.");
}

// ---------------------------------------------------------------------------
// English fix tests
// ---------------------------------------------------------------------------

#[test]
fn english_fix_split_word() {
    assert_eq!(fix_broken_english("T oken"), "Token");
}

#[test]
fn english_fix_acronym_three_letters() {
    assert_eq!(fix_broken_english("G P T"), "GPT");
}

#[test]
fn english_fix_normal_unchanged() {
    assert_eq!(fix_broken_english("Hello world"), "Hello world");
}

#[test]
fn english_fix_pronoun_i_unchanged() {
    assert_eq!(fix_broken_english("I am fine"), "I am fine");
}

#[test]
fn english_fix_acronym_two_letters() {
    assert_eq!(fix_broken_english("A I"), "AI");
}

#[test]
fn english_fix_usa() {
    assert_eq!(fix_broken_english("U S A"), "USA");
}

#[test]
fn english_fix_multiple_splits() {
    assert_eq!(fix_broken_english("T oken and R ust"), "Token and Rust");
}

#[test]
fn english_fix_empty() {
    assert_eq!(fix_broken_english(""), "");
}

#[test]
fn english_fix_isolated_uppercase_a_not_merged() {
    // 'A' is a common standalone article and must not be merged, matching the
    // exemption already applied to 'I'.
    assert_eq!(fix_broken_english("Hello A world"), "Hello A world");
}

// ---------------------------------------------------------------------------
// Pipeline tests
// ---------------------------------------------------------------------------

fn cfg(fix: bool, fw: bool, mode: PunctMode) -> PostprocessConfig {
    PostprocessConfig { fix_english: fix, fullwidth_punct: fw, punct_mode: mode }
}

#[test]
fn pipeline_all_disabled_passthrough() {
    assert_eq!(postprocess("Hello, world.", &cfg(false, false, PunctMode::Keep)), "Hello, world.");
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
    // All features + strip trailing
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
    assert_eq!(postprocess("T oken", &cfg(false, false, PunctMode::Keep)), "T oken");
}

#[test]
fn pipeline_empty_input() {
    assert_eq!(postprocess("", &cfg(true, true, PunctMode::StripTrailing)), "");
}

#[test]
fn pipeline_cjk_with_english_acronym_and_punct() {
    // "G P T 很好," — acronym fixed, comma converted, no trailing punct to strip
    assert_eq!(
        postprocess("G P T 很好,", &cfg(true, true, PunctMode::StripTrailing)),
        "GPT 很好"
    );
}
