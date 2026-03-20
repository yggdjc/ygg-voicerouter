//! Post-processing pipeline for raw ASR transcripts.
//!
//! The pipeline applies transformations in a fixed order:
//!
//! 1. **English fix** (if `config.fix_english`): merge broken ASR tokens.
//! 2. **Fullwidth punctuation** (if `config.fullwidth_punct`): convert
//!    ASCII punctuation to CJK fullwidth equivalents when adjacent to CJK.
//! 3. **Punct mode**: strip trailing punct, compact spaces around punct, or
//!    leave as-is.
//!
//! # Examples
//!
//! ```
//! use voicerouter::config::{PostprocessConfig, PunctMode};
//! use voicerouter::postprocess::postprocess;
//!
//! let config = PostprocessConfig {
//!     fix_english: true,
//!     fullwidth_punct: true,
//!     punct_mode: PunctMode::StripTrailing,
//!     ..Default::default()
//! };
//! let result = postprocess("你好,世界.", &config);
//! assert_eq!(result, "你好，世界");
//! ```

pub mod english_fix;
pub mod filler;
pub mod number;
pub mod punctuation;

use crate::config::PostprocessConfig;
use english_fix::fix_broken_english;
use filler::remove_fillers;
use number::normalize_numbers;
use punctuation::{
    apply_punct_mode, fullwidth_to_half_in_ascii, half_to_fullwidth, space_cjk_ascii_boundary,
};

/// Run the post-processing pipeline on `text` according to `config`.
///
/// Steps applied in order:
/// 1. `fix_broken_english` — if `config.fix_english` is `true`
/// 2. `half_to_fullwidth` — if `config.fullwidth_punct` is `true`
/// 3. `apply_punct_mode` — always applied
///
/// # Examples
///
/// ```
/// use voicerouter::config::{PostprocessConfig, PunctMode};
/// use voicerouter::postprocess::postprocess;
///
/// // All features enabled
/// let config = PostprocessConfig {
///     fix_english: true,
///     fullwidth_punct: true,
///     punct_mode: PunctMode::StripTrailing,
///     ..Default::default()
/// };
/// assert_eq!(postprocess("你好,世界.", &config), "你好，世界");
///
/// // Keep mode preserves punctuation
/// let config_keep = PostprocessConfig {
///     fix_english: false,
///     fullwidth_punct: false,
///     punct_mode: PunctMode::Keep,
///     ..Default::default()
/// };
/// assert_eq!(postprocess("Hello.", &config_keep), "Hello.");
/// ```
#[must_use]
pub fn postprocess(text: &str, config: &PostprocessConfig) -> String {
    // Remove Chinese filler words (嗯、呃、啊 as hesitation).
    let defilled = remove_fillers(text);

    // Convert Chinese numbers to Arabic digits (ITN).
    let numbered = normalize_numbers(&defilled);

    // Insert spaces at CJK/ASCII boundaries so downstream steps
    // can tokenise English runs correctly (e.g. "了PTT模式" → "了 PTT 模式").
    let spaced = space_cjk_ascii_boundary(&numbered);

    let step1 = if config.fix_english {
        fix_broken_english(&spaced)
    } else {
        spaced
    };

    // Convert fullwidth punct in ASCII context back to half-width
    // (ct-punc outputs fullwidth regardless of language).
    let step1b = fullwidth_to_half_in_ascii(&step1);

    let step2 = if config.fullwidth_punct {
        half_to_fullwidth(&step1b)
    } else {
        step1b
    };

    apply_punct_mode(&step2, config.punct_mode)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PostprocessConfig, PunctMode};

    fn config(fix: bool, fw: bool, mode: PunctMode) -> PostprocessConfig {
        PostprocessConfig {
            fix_english: fix,
            fullwidth_punct: fw,
            punct_mode: mode,
            ..Default::default()
        }
    }

    #[test]
    fn all_disabled_passthrough() {
        let cfg = config(false, false, PunctMode::Keep);
        assert_eq!(postprocess("Hello, world.", &cfg), "Hello, world.");
    }

    #[test]
    fn fullwidth_only() {
        let cfg = config(false, true, PunctMode::Keep);
        assert_eq!(postprocess("你好,世界.", &cfg), "你好，世界。");
    }

    #[test]
    fn fullwidth_and_strip_trailing() {
        let cfg = config(false, true, PunctMode::StripTrailing);
        assert_eq!(postprocess("你好,世界.", &cfg), "你好，世界");
    }

    #[test]
    fn english_fix_and_strip_trailing() {
        let cfg = config(true, false, PunctMode::StripTrailing);
        assert_eq!(postprocess("G P T is great.", &cfg), "GPT is great");
    }

    #[test]
    fn full_pipeline() {
        let cfg = config(true, true, PunctMode::StripTrailing);
        assert_eq!(postprocess("你好,世界.", &cfg), "你好，世界");
    }

    #[test]
    fn replace_space_mode() {
        let cfg = config(false, false, PunctMode::ReplaceSpace);
        assert_eq!(postprocess("Hello. World", &cfg), "Hello.World");
    }

    #[test]
    fn english_fix_disabled_leaves_broken_tokens() {
        let cfg = config(false, false, PunctMode::Keep);
        assert_eq!(postprocess("T oken", &cfg), "T oken");
    }

    #[test]
    fn empty_input() {
        let cfg = config(true, true, PunctMode::StripTrailing);
        assert_eq!(postprocess("", &cfg), "");
    }
}
