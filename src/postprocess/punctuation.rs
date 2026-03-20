//! Punctuation normalization for ASR output.
//!
//! Converts half-width ASCII punctuation to fullwidth CJK equivalents when
//! adjacent characters are CJK, and provides punct-mode post-processing.
//!
//! # Examples
//!
//! ```
//! use voicerouter::postprocess::punctuation::{half_to_fullwidth, apply_punct_mode};
//! use voicerouter::config::PunctMode;
//!
//! assert_eq!(half_to_fullwidth("你好,世界"), "你好，世界");
//! assert_eq!(half_to_fullwidth("Hello, world"), "Hello, world");
//! assert_eq!(apply_punct_mode("Hello.", PunctMode::StripTrailing), "Hello");
//! assert_eq!(apply_punct_mode("Hello.", PunctMode::Keep), "Hello.");
//! ```

use crate::config::PunctMode;

// ---------------------------------------------------------------------------
// CJK detection
// ---------------------------------------------------------------------------

/// Returns `true` if `c` is relevant to CJK punctuation adjacency detection.
///
/// Beyond the CJK Unified Ideograph blocks this intentionally includes:
/// - **CJK Symbols and Punctuation** (U+3000..=U+303F): used as sentence
///   delimiters in CJK text, so punctuation next to them should be fullwidth.
/// - **Halfwidth and Fullwidth Forms** (U+FF00..=U+FFEF): already-fullwidth
///   characters; treating them as CJK context prevents double-conversion
///   artefacts.
/// - **Hiragana + Katakana** (U+3040..=U+30FF): Japanese syllabic scripts that
///   follow the same punctuation conventions as CJK ideographs.
fn is_cjk_context(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}'   // CJK Unified Ideographs
        | '\u{3400}'..='\u{4DBF}' // CJK Extension A
        | '\u{20000}'..='\u{2A6DF}' // CJK Extension B
        | '\u{2A700}'..='\u{2B73F}' // CJK Extension C
        | '\u{2B740}'..='\u{2B81F}' // CJK Extension D
        | '\u{2B820}'..='\u{2CEAF}' // CJK Extension E
        | '\u{F900}'..='\u{FAFF}'  // CJK Compatibility Ideographs
        | '\u{2F800}'..='\u{2FA1F}' // CJK Compatibility Supplement
        | '\u{3000}'..='\u{303F}'  // CJK Symbols and Punctuation
        | '\u{FF00}'..='\u{FFEF}'  // Halfwidth/Fullwidth Forms
        | '\u{3040}'..='\u{30FF}'  // Hiragana + Katakana
    )
}

// ---------------------------------------------------------------------------
// CJK / ASCII boundary spacing
// ---------------------------------------------------------------------------

/// Insert a space at every boundary between CJK and ASCII-letter runs.
///
/// ASR models (especially Paraformer) often glue Chinese and English together
/// without spaces, e.g. `"了PTT模式"`.  This function normalises such text to
/// `"了 PTT 模式"`, enabling downstream token-level processing such as
/// acronym merging.
///
/// # Examples
///
/// ```
/// use voicerouter::postprocess::punctuation::space_cjk_ascii_boundary;
///
/// assert_eq!(space_cjk_ascii_boundary("了PTT模式"), "了 PTT 模式");
/// assert_eq!(space_cjk_ascii_boundary("Hello世界"), "Hello 世界");
/// assert_eq!(space_cjk_ascii_boundary("纯中文"), "纯中文");
/// assert_eq!(space_cjk_ascii_boundary("pure ascii"), "pure ascii");
/// ```
#[must_use]
pub fn space_cjk_ascii_boundary(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut output = String::with_capacity(text.len() + 8);

    for (i, &c) in chars.iter().enumerate() {
        if i > 0 {
            let prev = chars[i - 1];
            let need_space =
                (is_cjk_context(prev) && c.is_ascii_alphanumeric())
                || (prev.is_ascii_alphanumeric() && is_cjk_context(c));
            if need_space {
                output.push(' ');
            }
        }
        output.push(c);
    }

    output
}

// ---------------------------------------------------------------------------
// Punct mapping
// ---------------------------------------------------------------------------

/// Maps an ASCII punctuation character to its fullwidth equivalent.
/// Returns `None` for characters that have no mapping.
fn to_fullwidth(c: char) -> Option<char> {
    match c {
        ',' => Some('，'),
        '.' => Some('。'),
        ':' => Some('：'),
        ';' => Some('；'),
        '?' => Some('？'),
        '!' => Some('！'),
        '(' => Some('（'),
        ')' => Some('）'),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Public functions
// ---------------------------------------------------------------------------

/// Convert half-width punctuation to fullwidth ONLY when adjacent to CJK
/// characters.
///
/// A punctuation character is eligible for conversion when at least one of
/// the immediately preceding or following characters is CJK. If both
/// neighbours are ASCII/Latin the character is left unchanged.
///
/// # Examples
///
/// ```
/// use voicerouter::postprocess::punctuation::half_to_fullwidth;
///
/// assert_eq!(half_to_fullwidth("你好,世界"), "你好，世界");
/// assert_eq!(half_to_fullwidth("Hello, world"), "Hello, world");
/// assert_eq!(half_to_fullwidth("测试.结束"), "测试。结束");
/// assert_eq!(half_to_fullwidth("test.end"), "test.end");
/// assert_eq!(half_to_fullwidth("你好world,测试"), "你好world，测试");
/// ```
#[must_use]
pub fn half_to_fullwidth(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut output = String::with_capacity(text.len());

    for (i, &c) in chars.iter().enumerate() {
        if let Some(fw) = to_fullwidth(c) {
            let prev_is_cjk = i > 0 && is_cjk_context(chars[i - 1]);
            let next_is_cjk = i + 1 < chars.len() && is_cjk_context(chars[i + 1]);

            if prev_is_cjk || next_is_cjk {
                output.push(fw);
            } else {
                output.push(c);
            }
        } else {
            output.push(c);
        }
    }

    output
}

/// Convert fullwidth punctuation back to half-width when both neighbours are
/// ASCII (i.e. the punctuation is inside an English context).
///
/// This is the reverse of [`half_to_fullwidth`] and is needed because
/// ct-punc outputs fullwidth punctuation regardless of language context.
///
/// # Examples
///
/// ```
/// use voicerouter::postprocess::punctuation::fullwidth_to_half_in_ascii;
///
/// assert_eq!(fullwidth_to_half_in_ascii("apple，banana"), "apple,banana");
/// assert_eq!(fullwidth_to_half_in_ascii("你好，世界"), "你好，世界");
/// assert_eq!(fullwidth_to_half_in_ascii("hello。world"), "hello.world");
/// assert_eq!(fullwidth_to_half_in_ascii("test， next"), "test, next");
/// ```
#[must_use]
pub fn fullwidth_to_half_in_ascii(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut output = String::with_capacity(text.len());

    for (i, &c) in chars.iter().enumerate() {
        if let Some(hw) = to_halfwidth(c) {
            let prev_is_ascii = i == 0
                || chars[i - 1].is_ascii_alphanumeric()
                || chars[i - 1].is_ascii_whitespace();
            let next_is_ascii = i + 1 >= chars.len()
                || chars[i + 1].is_ascii_alphanumeric()
                || chars[i + 1].is_ascii_whitespace();

            if prev_is_ascii && next_is_ascii {
                output.push(hw);
            } else {
                output.push(c);
            }
        } else {
            output.push(c);
        }
    }

    output
}

/// Maps a fullwidth punctuation character to its half-width equivalent.
fn to_halfwidth(c: char) -> Option<char> {
    match c {
        '，' => Some(','),
        '。' => Some('.'),
        '：' => Some(':'),
        '；' => Some(';'),
        '？' => Some('?'),
        '！' => Some('!'),
        '（' => Some('('),
        '）' => Some(')'),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Trailing punctuation set
// ---------------------------------------------------------------------------

const TRAILING_PUNCT: &[char] = &[
    '.', ',', '!', '?', ':', ';',
    '。', '，', '！', '？', '：', '；',
    '、', '…',
];

/// Apply a [`PunctMode`] transformation to `text`.
///
/// - [`PunctMode::Keep`]: return the string unchanged.
/// - [`PunctMode::StripTrailing`]: remove all trailing punctuation characters.
/// - [`PunctMode::ReplaceSpace`]: remove the space that follows a
///   sentence-final punctuation mark (useful for continuous dictation where
///   the next segment will be appended directly).
///
/// # Examples
///
/// ```
/// use voicerouter::postprocess::punctuation::apply_punct_mode;
/// use voicerouter::config::PunctMode;
///
/// assert_eq!(apply_punct_mode("Hello.", PunctMode::Keep), "Hello.");
/// assert_eq!(apply_punct_mode("Hello.", PunctMode::StripTrailing), "Hello");
/// assert_eq!(apply_punct_mode("Hello. World", PunctMode::ReplaceSpace), "Hello.World");
/// ```
#[must_use]
pub fn apply_punct_mode(text: &str, mode: PunctMode) -> String {
    match mode {
        PunctMode::Keep => text.to_owned(),
        PunctMode::StripTrailing => text.trim_end_matches(TRAILING_PUNCT).to_owned(),
        PunctMode::ReplaceSpace => {
            // Remove a space that directly follows a sentence-final punctuation.
            let chars: Vec<char> = text.chars().collect();
            let mut output = String::with_capacity(text.len());
            let mut i = 0;
            while i < chars.len() {
                let c = chars[i];
                output.push(c);
                // If this is a sentence-final punct and next char is a space, skip the space.
                if TRAILING_PUNCT.contains(&c) && i + 1 < chars.len() && chars[i + 1] == ' ' {
                    i += 2; // skip the space
                    continue;
                }
                i += 1;
            }
            output
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PunctMode;

    #[test]
    fn cjk_adjacent_comma_converted() {
        assert_eq!(half_to_fullwidth("你好,世界"), "你好，世界");
    }

    #[test]
    fn ascii_adjacent_comma_unchanged() {
        assert_eq!(half_to_fullwidth("Hello, world"), "Hello, world");
    }

    #[test]
    fn cjk_adjacent_period_converted() {
        assert_eq!(half_to_fullwidth("测试.结束"), "测试。结束");
    }

    #[test]
    fn ascii_adjacent_period_unchanged() {
        assert_eq!(half_to_fullwidth("test.end"), "test.end");
    }

    #[test]
    fn mixed_text_right_cjk_triggers_conversion() {
        assert_eq!(half_to_fullwidth("你好world,测试"), "你好world，测试");
    }

    #[test]
    fn all_mapped_chars_converted_when_cjk_adjacent() {
        assert_eq!(half_to_fullwidth("你好:世界"), "你好：世界");
        assert_eq!(half_to_fullwidth("你好;世界"), "你好；世界");
        assert_eq!(half_to_fullwidth("你好?世界"), "你好？世界");
        assert_eq!(half_to_fullwidth("你好!世界"), "你好！世界");
        assert_eq!(half_to_fullwidth("你好(世界)"), "你好（世界）");
    }

    #[test]
    fn punct_at_start_next_cjk() {
        // Punct at position 0: only next char matters.
        assert_eq!(half_to_fullwidth(",世界"), "，世界");
    }

    #[test]
    fn punct_at_end_prev_cjk() {
        // Punct at last position: only prev char matters.
        assert_eq!(half_to_fullwidth("世界,"), "世界，");
    }

    #[test]
    fn apply_keep_unchanged() {
        assert_eq!(apply_punct_mode("Hello.", PunctMode::Keep), "Hello.");
        assert_eq!(apply_punct_mode("Hello", PunctMode::Keep), "Hello");
    }

    #[test]
    fn apply_strip_trailing_ascii() {
        assert_eq!(apply_punct_mode("Hello.", PunctMode::StripTrailing), "Hello");
        assert_eq!(apply_punct_mode("Hello!!", PunctMode::StripTrailing), "Hello");
        assert_eq!(apply_punct_mode("Hello", PunctMode::StripTrailing), "Hello");
    }

    #[test]
    fn apply_strip_trailing_cjk_punct() {
        assert_eq!(apply_punct_mode("你好。", PunctMode::StripTrailing), "你好");
        assert_eq!(apply_punct_mode("你好！", PunctMode::StripTrailing), "你好");
    }

    #[test]
    fn apply_replace_space_removes_post_punct_space() {
        assert_eq!(
            apply_punct_mode("Hello. World", PunctMode::ReplaceSpace),
            "Hello.World"
        );
    }

    #[test]
    fn apply_replace_space_no_trailing_space_unchanged() {
        assert_eq!(
            apply_punct_mode("Hello.", PunctMode::ReplaceSpace),
            "Hello."
        );
    }

    #[test]
    fn apply_replace_space_multiple_punct_spaces() {
        assert_eq!(
            apply_punct_mode("Hi. Hello. World", PunctMode::ReplaceSpace),
            "Hi.Hello.World"
        );
    }
}
