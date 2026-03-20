//! Context-aware Chinese filler word removal.
//!
//! Removes hesitation fillers (嗯、呃、啊、哦) that appear mid-sentence as
//! thinking pauses, while preserving them when they serve a grammatical
//! purpose (affirmation, tone particle, etc.).
//!
//! # Examples
//!
//! ```
//! use voicerouter::postprocess::filler::remove_fillers;
//!
//! // Mid-sentence filler removed:
//! assert_eq!(remove_fillers("这个大模型嗯支持中文"), "这个大模型支持中文");
//!
//! // Sentence-initial affirmation preserved:
//! assert_eq!(remove_fillers("嗯，我知道了"), "嗯，我知道了");
//!
//! // Sentence-final tone particle preserved:
//! assert_eq!(remove_fillers("好啊"), "好啊");
//! ```

/// Characters that can be fillers depending on context.
const FILLER_CHARS: &[char] = &['嗯', '呃', '啊', '哦', '额'];

/// Remove filler words from Chinese text using positional heuristics.
///
/// Rules:
/// - **Keep** at sentence start followed by punctuation: "嗯，..." (affirmation)
/// - **Keep** at sentence end: "好啊" (tone particle)
/// - **Keep** when standalone (the entire text is just the filler)
/// - **Remove** when sandwiched between non-filler characters mid-sentence
/// - **Remove** consecutive repeated fillers: "嗯嗯嗯" → ""
#[must_use]
pub fn remove_fillers(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return String::new();
    }

    let mut output = String::with_capacity(text.len());
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if !is_filler(c) {
            output.push(c);
            i += 1;
            continue;
        }

        // Count consecutive filler chars of the same type.
        let run_start = i;
        while i < chars.len() && chars[i] == c {
            i += 1;
        }
        let run_len = i - run_start;

        // Rule: repeated fillers (嗯嗯嗯) → remove entirely.
        if run_len >= 2 {
            // Skip trailing space if any.
            if i < chars.len() && chars[i] == ' ' {
                i += 1;
            }
            continue;
        }

        // Single filler character — decide by context.
        let prev = if run_start > 0 { Some(chars[run_start - 1]) } else { None };
        let next = if i < chars.len() { Some(chars[i]) } else { None };

        if should_keep(prev, c, next) {
            output.push(c);
        }
        // else: silently drop the filler
    }

    // Clean up double spaces left by removal.
    collapse_spaces(&output)
}

/// True if `c` is a potential filler character.
fn is_filler(c: char) -> bool {
    FILLER_CHARS.contains(&c)
}

/// Decide whether a single filler char should be kept based on neighbours.
fn should_keep(prev: Option<char>, _filler: char, next: Option<char>) -> bool {
    // Rule 1: sentence start + punctuation → affirmation ("嗯，我知道")
    if prev.is_none() && next.map_or(false, is_punct) {
        return true;
    }

    // Rule 2: after punctuation + followed by punctuation → interjection ("，嗯，")
    if prev.map_or(false, is_punct) && next.map_or(false, is_punct) {
        return true;
    }

    // Rule 3: sentence end (no next, or next is end punct) → tone particle ("好啊")
    if next.is_none() || next.map_or(false, is_sentence_end_punct) {
        // But only if preceded by a non-filler, non-space char.
        if prev.map_or(false, |p| !is_filler(p) && p != ' ') {
            return true;
        }
    }

    // Rule 4: standalone (no prev, no next) → keep
    if prev.is_none() && next.is_none() {
        return true;
    }

    // Default: mid-sentence filler → remove
    false
}

fn is_punct(c: char) -> bool {
    matches!(c, '，' | '。' | '！' | '？' | '、' | '；' | '：'
        | ',' | '.' | '!' | '?' | ';' | ':')
}

fn is_sentence_end_punct(c: char) -> bool {
    matches!(c, '。' | '！' | '？' | '.' | '!' | '?')
}

/// Collapse runs of multiple spaces into one.
fn collapse_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_space = false;
    for c in text.chars() {
        if c == ' ' {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    out.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mid-sentence fillers → removed
    #[test]
    fn mid_sentence_en_removed() {
        assert_eq!(remove_fillers("这个大模型嗯支持中文"), "这个大模型支持中文");
    }

    #[test]
    fn mid_sentence_e_removed() {
        assert_eq!(remove_fillers("我呃想说"), "我想说");
    }

    #[test]
    fn mid_sentence_o_removed() {
        assert_eq!(remove_fillers("那哦就这样"), "那就这样");
    }

    // Repeated fillers → removed
    #[test]
    fn repeated_removed() {
        assert_eq!(remove_fillers("嗯嗯嗯好的"), "好的");
    }

    #[test]
    fn repeated_with_trailing_space() {
        assert_eq!(remove_fillers("嗯嗯 好的"), "好的");
    }

    // Sentence-initial affirmation → kept
    #[test]
    fn initial_affirmation_kept() {
        assert_eq!(remove_fillers("嗯，我知道了"), "嗯，我知道了");
    }

    // Sentence-final tone particle → kept
    #[test]
    fn final_tone_particle_kept() {
        assert_eq!(remove_fillers("好啊"), "好啊");
    }

    #[test]
    fn final_with_end_punct_kept() {
        assert_eq!(remove_fillers("是啊。"), "是啊。");
    }

    // Standalone → kept
    #[test]
    fn standalone_kept() {
        assert_eq!(remove_fillers("嗯"), "嗯");
    }

    // Complex mixed
    #[test]
    fn mixed_context() {
        assert_eq!(
            remove_fillers("嗯，这个嗯大模型啊支持中文啊。"),
            "嗯，这个大模型支持中文啊。"
        );
    }

    // No fillers → unchanged
    #[test]
    fn no_fillers() {
        assert_eq!(remove_fillers("这是正常的句子"), "这是正常的句子");
    }

    // Empty → empty
    #[test]
    fn empty() {
        assert_eq!(remove_fillers(""), "");
    }
}
