//! Remove speech disfluency fillers (口水词/填充词) from ASR transcripts.
//!
//! Only targets onomatopoeic hesitation markers (状声词). Connective words like
//! 然后、就是、那个 are intentional in spoken Chinese input and are preserved.
//!
//! - **Remove**: standalone hesitation markers (嗯、啊、哦、噢、呃、额) when
//!   they appear as pause-fillers between clauses or at text boundaries.
//! - **Keep**: affirmative 嗯 ("嗯，我了解了"), exclamatory 啊 ("天气真好啊"),
//!   embedded characters (额头, 呃逆).

use crate::postprocess::punctuation::is_cjk_context;

/// Onomatopoeic hesitation markers — context-dependent fillers.
/// Only removed when standalone (e.g. "额头" keeps 额, "呃逆" keeps 呃).
const CONTEXT_FILLERS: &[&str] = &["嗯", "啊", "哦", "噢", "呃", "额"];

/// Punctuation characters that typically surround fillers in ASR output.
fn is_pause_punct(c: char) -> bool {
    matches!(c, ',' | '，' | '、' | '。' | '.' | '；' | ';')
}

/// Remove filler words from `text`, preserving semantically meaningful ones.
///
/// # Examples
///
/// ```
/// use voicerouter::postprocess::filler::remove_fillers;
///
/// // Standalone hesitation removed
/// assert_eq!(remove_fillers("呃，我想去"), "我想去");
///
/// // Exclamatory 啊 preserved
/// assert_eq!(remove_fillers("天气真好啊"), "天气真好啊");
///
/// // Affirmative 嗯 preserved
/// assert_eq!(remove_fillers("嗯，我了解了"), "嗯，我了解了");
/// ```
#[must_use]
pub fn remove_fillers(text: &str) -> String {
    let mut result = text.to_string();
    for filler in CONTEXT_FILLERS {
        result = remove_filler_occurrences(&result, filler);
    }
    clean_whitespace(&result)
}

/// Check each occurrence of `filler` and remove it if the context indicates
/// hesitation rather than semantic use.
fn remove_filler_occurrences(text: &str, filler: &str) -> String {
    let filler_len = filler.len();
    let mut result = String::with_capacity(text.len());
    let mut remaining = text;

    while let Some(pos) = remaining.find(filler) {
        let before = &remaining[..pos];
        let after = &remaining[pos + filler_len..];

        if should_remove(before, filler, after) {
            // Consume trailing pause punctuation + optional space after filler
            let skip = skip_trailing_punct(after);
            // Also trim preceding pause punctuation when filler is mid-sentence
            // ("我想，嗯，" → "我想" not "我想，")
            let trimmed_before = trim_trailing_punct(before);
            result.push_str(trimmed_before);
            remaining = &after[skip..];
        } else {
            result.push_str(&remaining[..pos + filler_len]);
            remaining = after;
        }
    }
    result.push_str(remaining);
    result
}

/// Determine whether a filler at this position is a hesitation (remove)
/// or carries meaning (keep).
fn should_remove(before: &str, filler: &str, after: &str) -> bool {
    let prev_char = before.trim_end().chars().last();

    match filler {
        // 嗯: keep when affirmative (at text start with substantive follow-up)
        "嗯" => {
            let at_start = before.trim().is_empty();
            let after_trimmed =
                after.trim_start_matches(|c: char| is_pause_punct(c) || c == ' ');
            if at_start && !after_trimmed.is_empty() {
                // "嗯，我了解了" — affirmative, keep
                return false;
            }
            is_filler_position(prev_char)
        }

        // 啊: keep when attached to preceding CJK (exclamatory) or at sentence end
        "啊" => {
            if let Some(pc) = prev_char {
                if before.ends_with(pc) && !before.ends_with(' ') && is_cjk_context(pc) {
                    return false;
                }
            }
            if after.trim().is_empty()
                || after
                    .trim_start()
                    .starts_with(|c: char| c == '。' || c == '！')
            {
                return false;
            }
            is_filler_position(prev_char)
        }

        // 哦/噢: similar to 啊
        "哦" | "噢" => {
            if let Some(pc) = prev_char {
                if before.ends_with(pc) && !before.ends_with(' ') && is_cjk_context(pc) {
                    return false;
                }
            }
            if after.trim().is_empty() {
                return false;
            }
            is_filler_position(prev_char)
        }

        // 呃: almost never a real word component (only 呃逆).
        // Remove in all other contexts — even between CJK chars ("去呃去").
        "呃" => {
            if after.starts_with('逆') {
                return false;
            }
            true
        }

        // 额: many compound words (额头, 额度, 额外, 额定, 前额…).
        // Only remove when standalone (not attached to CJK on either side).
        "额" => {
            if let Some(pc) = prev_char {
                if before.ends_with(pc)
                    && !before.ends_with(' ')
                    && is_cjk_context(pc)
                    && !is_pause_punct(pc)
                {
                    return false;
                }
            }
            if let Some(nc) = after.chars().next() {
                if is_cjk_context(nc) && !is_pause_punct(nc) {
                    return false;
                }
            }
            true
        }

        _ => is_filler_position(prev_char),
    }
}

/// A filler is in "filler position" when preceded by a pause
/// (punctuation, whitespace, or text boundary).
fn is_filler_position(prev_char: Option<char>) -> bool {
    match prev_char {
        None => true,
        Some(c) if is_pause_punct(c) => true,
        Some(c) if c.is_whitespace() => true,
        _ => false,
    }
}

/// Trim trailing pause punctuation + spaces from the end of a string slice.
/// Used to clean up "我想，" → "我想" when the following filler is removed.
fn trim_trailing_punct(s: &str) -> &str {
    let mut end = s.len();
    for c in s.chars().rev() {
        if is_pause_punct(c) || c == ' ' {
            end -= c.len_utf8();
        } else {
            break;
        }
    }
    &s[..end]
}

/// Count bytes of trailing pause punctuation + optional spaces to skip after
/// removing a filler.
fn skip_trailing_punct(s: &str) -> usize {
    let mut count = 0;
    for c in s.chars() {
        if is_pause_punct(c) || c == ' ' {
            count += c.len_utf8();
        } else {
            break;
        }
    }
    count
}

/// Normalize whitespace: collapse runs, trim.
fn clean_whitespace(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut prev_space = false;
    for c in text.chars() {
        if c == ' ' {
            if !prev_space && !result.is_empty() {
                result.push(' ');
            }
            prev_space = true;
        } else {
            prev_space = false;
            result.push(c);
        }
    }
    result.trim().to_string()
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- 呃/额 --

    #[test]
    fn remove_standalone_e_start() {
        assert_eq!(remove_fillers("呃，我想去北京"), "我想去北京");
    }

    #[test]
    fn remove_standalone_e_mid() {
        assert_eq!(remove_fillers("我想，额，去北京"), "我想去北京");
    }

    #[test]
    fn remove_standalone_e_only() {
        assert_eq!(remove_fillers("呃"), "");
    }

    #[test]
    fn keep_e_in_word_etou() {
        assert_eq!(remove_fillers("他的额头很高"), "他的额头很高");
    }

    #[test]
    fn keep_e_in_word_eni() {
        assert_eq!(remove_fillers("他有呃逆的毛病"), "他有呃逆的毛病");
    }

    #[test]
    fn remove_e_stutter() {
        // 呃 between CJK chars as stutter → remove
        assert_eq!(remove_fillers("我想去呃去北京"), "我想去去北京");
    }

    #[test]
    fn remove_e_mid_sentence() {
        assert_eq!(remove_fillers("今天呃天气很好"), "今天天气很好");
    }

    // -- 嗯 --

    #[test]
    fn keep_affirmative_en() {
        assert_eq!(remove_fillers("嗯，我了解了"), "嗯，我了解了");
    }

    #[test]
    fn remove_hesitation_en_mid() {
        assert_eq!(remove_fillers("我想，嗯，去北京"), "我想去北京");
    }

    #[test]
    fn keep_en_in_context() {
        assert_eq!(remove_fillers("他说嗯然后就走了"), "他说嗯然后就走了");
    }

    // -- 啊 --

    #[test]
    fn keep_exclamatory_a() {
        assert_eq!(remove_fillers("天气真好啊"), "天气真好啊");
    }

    #[test]
    fn keep_exclamatory_a_with_punct() {
        assert_eq!(remove_fillers("天气真好啊。"), "天气真好啊。");
    }

    #[test]
    fn remove_hesitation_a() {
        assert_eq!(remove_fillers("啊，我想说的是"), "我想说的是");
    }

    // -- 哦/噢 --

    #[test]
    fn keep_oh_attached() {
        assert_eq!(remove_fillers("是哦"), "是哦");
    }

    #[test]
    fn remove_oh_standalone() {
        assert_eq!(remove_fillers("哦，原来如此"), "原来如此");
    }

    // -- Connectives preserved (not fillers) --

    #[test]
    fn preserve_ranhou() {
        assert_eq!(remove_fillers("然后我就去了"), "然后我就去了");
    }

    #[test]
    fn preserve_jiushi() {
        assert_eq!(remove_fillers("就是这样的"), "就是这样的");
    }

    #[test]
    fn preserve_nage() {
        assert_eq!(remove_fillers("那个人很高"), "那个人很高");
    }

    // -- Edge cases --

    #[test]
    fn empty_input() {
        assert_eq!(remove_fillers(""), "");
    }

    #[test]
    fn no_fillers() {
        assert_eq!(remove_fillers("今天天气很好"), "今天天气很好");
    }

    #[test]
    fn only_fillers() {
        assert_eq!(remove_fillers("呃，额，嗯"), "");
    }

    #[test]
    fn mixed_fillers_and_content() {
        assert_eq!(
            remove_fillers("呃，我想，额，去北京"),
            "我想去北京"
        );
    }

    #[test]
    fn long_sentence_with_fillers() {
        assert_eq!(
            remove_fillers("嗯，今天我想去，呃，那个商场买点东西"),
            "嗯，今天我想去那个商场买点东西"
        );
    }
}
