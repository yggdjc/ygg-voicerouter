/// Split text into sentences for TTS playback.
///
/// Splits on Chinese (。！？) and English (. ! ?) sentence-ending punctuation.
/// Fragments shorter than 4 characters are merged into the next sentence.
/// Trailing text without punctuation is kept as a standalone sentence.
/// Does not split on '.' between digits (e.g. "25.5").
pub fn split_sentences(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }

    let mut sentences: Vec<String> = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();

    for (i, &ch) in chars.iter().enumerate() {
        current.push(ch);
        if matches!(ch, '。' | '！' | '？' | '!' | '?') {
            sentences.push(std::mem::take(&mut current));
        } else if ch == '.' {
            let prev_digit = i > 0 && chars[i - 1].is_ascii_digit();
            let next_digit = i + 1 < chars.len() && chars[i + 1].is_ascii_digit();
            if !(prev_digit && next_digit) {
                sentences.push(std::mem::take(&mut current));
            }
        }
    }
    if !current.is_empty() {
        sentences.push(current);
    }

    // Merge short fragments (< 4 chars) into the next sentence.
    // A short fragment is prepended to the following sentence rather than emitted alone.
    let mut merged: Vec<String> = Vec::new();
    let mut carry = String::new();
    for s in sentences {
        let candidate = format!("{carry}{s}");
        if candidate.chars().count() >= 4 {
            merged.push(candidate);
            carry = String::new();
        } else {
            carry = candidate;
        }
    }
    if !carry.is_empty() {
        if let Some(last) = merged.last_mut() {
            last.push_str(&carry);
        } else {
            merged.push(carry);
        }
    }

    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_punctuation() {
        let result = split_sentences("今天天气很好。明天会下雨！后天呢？");
        assert_eq!(result, vec![
            "今天天气很好。",
            "明天会下雨！",
            "后天呢？",
        ]);
    }

    #[test]
    fn english_punctuation() {
        let result = split_sentences("Hello world. How are you! Fine?");
        assert_eq!(result, vec![
            "Hello world.",
            " How are you!",
            " Fine?",
        ]);
    }

    #[test]
    fn merge_short_fragments() {
        let result = split_sentences("好。今天天气不错。");
        assert_eq!(result, vec!["好。今天天气不错。"]);
    }

    #[test]
    fn trailing_without_punctuation() {
        let result = split_sentences("第一句话。然后这里没有标点");
        assert_eq!(result, vec!["第一句话。", "然后这里没有标点"]);
    }

    #[test]
    fn single_sentence_no_punctuation() {
        let result = split_sentences("就这样吧");
        assert_eq!(result, vec!["就这样吧"]);
    }

    #[test]
    fn empty_input() {
        let result = split_sentences("");
        assert!(result.is_empty());
    }

    #[test]
    fn mixed_chinese_english() {
        // "你好。" is 3 chars — below the 4-char threshold — so it merges into the next sentence.
        let result = split_sentences("你好。Hello world. 再见！");
        assert_eq!(result, vec!["你好。Hello world.", " 再见！"]);
    }

    #[test]
    fn decimal_numbers_not_split() {
        let result = split_sentences("温度是25.5度。");
        assert_eq!(result, vec!["温度是25.5度。"]);
    }
}
