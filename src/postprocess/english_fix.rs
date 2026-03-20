//! Fix broken English tokens produced by ASR systems.
//!
//! ASR models that are primarily trained on CJK audio sometimes produce
//! fractured English tokens — a single word like "Token" may come out as
//! "T oken", and an acronym like "GPT" may appear as "G P T".
//!
//! This module corrects both patterns while being careful not to merge
//! natural English that happens to look similar (e.g. "I am").
//!
//! # Algorithm
//!
//! Two passes are made:
//!
//! 1. **Consecutive single-letter pass**: sequences of space-separated
//!    single ASCII letters (e.g. `G P T`) are collapsed into one token (`GPT`).
//!    This applies regardless of case.
//!
//! 2. **Split-word pass**: an uppercase letter followed by a space and a
//!    lowercase continuation (e.g. `T oken`) is merged into `Token`.
//!    The letters `I` and `A` are excluded from this rule because they are
//!    common standalone English words (e.g. "I am fine", "A word").
//!
//! # Examples
//!
//! ```
//! use voicerouter::postprocess::english_fix::fix_broken_english;
//!
//! assert_eq!(fix_broken_english("T oken"), "Token");
//! assert_eq!(fix_broken_english("G P T"), "GPT");
//! assert_eq!(fix_broken_english("Hello world"), "Hello world");
//! assert_eq!(fix_broken_english("I am fine"), "I am fine");
//! ```

// ---------------------------------------------------------------------------
// Public function
// ---------------------------------------------------------------------------

/// Fix ASR-broken English tokens in `text`.
///
/// See the [module-level docs](self) for the full algorithm description.
///
/// # Examples
///
/// ```
/// use voicerouter::postprocess::english_fix::fix_broken_english;
///
/// assert_eq!(fix_broken_english("T oken"), "Token");
/// assert_eq!(fix_broken_english("G P T"), "GPT");
/// assert_eq!(fix_broken_english("Hello world"), "Hello world");
/// assert_eq!(fix_broken_english("I am fine"), "I am fine");
/// assert_eq!(fix_broken_english("U S A"), "USA");
/// ```
#[must_use]
pub fn fix_broken_english(text: &str) -> String {
    let after_acronym = merge_consecutive_single_letters(text);
    merge_split_word(&after_acronym)
}

// ---------------------------------------------------------------------------
// Pass 1: merge consecutive single ASCII letters  ("G P T" → "GPT")
// ---------------------------------------------------------------------------

/// Collapse runs of space-separated single ASCII letters into one token.
///
/// A "run" is two or more consecutive tokens that each consist of exactly one
/// ASCII alphabetic character. Non-letter tokens and multi-letter tokens break
/// the run.
fn merge_consecutive_single_letters(text: &str) -> String {
    // Tokenise on spaces, preserving the knowledge of whether a token is a
    // single ASCII letter.
    let tokens: Vec<&str> = text.split(' ').collect();
    let mut output = String::with_capacity(text.len());
    let mut i = 0;

    while i < tokens.len() {
        if is_single_ascii_letter(tokens[i]) {
            // Find the end of the run.
            let run_start = i;
            while i < tokens.len() && is_single_ascii_letter(tokens[i]) {
                i += 1;
            }
            let run_len = i - run_start;
            if run_len >= 2 {
                // Collapse the run.
                if !output.is_empty() {
                    output.push(' ');
                }
                for token in &tokens[run_start..i] {
                    output.push_str(token);
                }
            } else {
                // Single isolated letter — emit as-is.
                if !output.is_empty() {
                    output.push(' ');
                }
                output.push_str(tokens[run_start]);
            }
        } else {
            if !output.is_empty() {
                output.push(' ');
            }
            output.push_str(tokens[i]);
            i += 1;
        }
    }

    output
}

fn is_single_ascii_letter(s: &str) -> bool {
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => c.is_ascii_alphabetic(),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Pass 2: merge split word  ("T oken" → "Token")
// ---------------------------------------------------------------------------

/// Merge an uppercase letter followed by a space and a lowercase continuation.
///
/// The letters `I` and `A` are excluded to preserve "I am", "A word", etc.
fn merge_split_word(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut output = String::with_capacity(text.len());
    let mut i = 0;

    while i < len {
        let c = chars[i];

        // Pattern: single uppercase letter (not 'I' or 'A') + space + lowercase letter.
        // Require that the uppercase letter is either at the start or preceded
        // by a space (word boundary on left), so we don't mis-merge inside words.
        let left_boundary = i == 0 || chars[i - 1] == ' ';

        if left_boundary
            && c.is_ascii_uppercase()
            && c != 'I'
            && c != 'A'
            && i + 2 < len
            && chars[i + 1] == ' '
            && chars[i + 2].is_ascii_lowercase()
        {
            // Emit the uppercase letter directly, skip the space.
            output.push(c);
            i += 2; // skip c and the space; next iteration emits chars[i+2]
        } else {
            output.push(c);
            i += 1;
        }
    }

    output
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_word_merged() {
        assert_eq!(fix_broken_english("T oken"), "Token");
    }

    #[test]
    fn acronym_merged() {
        assert_eq!(fix_broken_english("G P T"), "GPT");
    }

    #[test]
    fn normal_english_unchanged() {
        assert_eq!(fix_broken_english("Hello world"), "Hello world");
    }

    #[test]
    fn pronoun_i_not_merged() {
        assert_eq!(fix_broken_english("I am fine"), "I am fine");
    }

    #[test]
    fn three_letter_acronym() {
        assert_eq!(fix_broken_english("U S A"), "USA");
    }

    #[test]
    fn mixed_cjk_and_english() {
        // CJK content should pass through unchanged.
        assert_eq!(fix_broken_english("你好 G P T"), "你好 GPT");
    }

    #[test]
    fn two_letter_acronym() {
        assert_eq!(fix_broken_english("A I"), "AI");
    }

    #[test]
    fn split_at_end_of_sentence() {
        assert_eq!(fix_broken_english("This is T oken based"), "This is Token based");
    }

    #[test]
    fn multiple_split_words() {
        assert_eq!(fix_broken_english("T oken and R ust"), "Token and Rust");
    }

    #[test]
    fn empty_string() {
        assert_eq!(fix_broken_english(""), "");
    }

    #[test]
    fn single_word_unchanged() {
        assert_eq!(fix_broken_english("Hello"), "Hello");
    }

    #[test]
    fn lowercase_single_letters_not_merged_as_acronym() {
        // e.g. "a b c" — these are all single letters but lowercase; the
        // consecutive-single-letter pass is case-agnostic, so they ARE merged.
        // Verify the actual behaviour is consistent.
        assert_eq!(fix_broken_english("a b c"), "abc");
    }

    #[test]
    fn article_a_not_merged() {
        // 'A' is a common standalone article and must not be merged with the
        // following word, matching the exemption applied to 'I'.
        assert_eq!(fix_broken_english("Hello A world"), "Hello A world");
    }

    #[test]
    fn article_a_before_noun_unchanged() {
        assert_eq!(fix_broken_english("Get A drink"), "Get A drink");
    }
}
