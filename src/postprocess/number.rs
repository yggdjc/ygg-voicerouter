//! Chinese number → Arabic digit conversion (Inverse Text Normalization).
//!
//! Converts spoken-form Chinese numbers to written-form Arabic digits in
//! context-appropriate situations, while leaving non-numeric uses untouched.
//!
//! # Examples
//!
//! ```
//! use voicerouter::postprocess::number::normalize_numbers;
//!
//! assert_eq!(normalize_numbers("v零点一"), "v0.1");
//! assert_eq!(normalize_numbers("三点一四"), "3.14");
//! assert_eq!(normalize_numbers("二零二六年三月一号"), "2026年3月1号");
//! assert_eq!(normalize_numbers("readme点md"), "readme.md");
//! assert_eq!(normalize_numbers("一百二十三个"), "123个");
//! assert_eq!(normalize_numbers("一个苹果"), "一个苹果");
//! ```

use chinese_number::{ChineseCountMethod, ChineseToNumber};

/// Single Chinese digit → Arabic digit mapping (for sequential digit patterns).
fn digit_val(c: char) -> Option<u8> {
    match c {
        '零' => Some(0),
        '一' => Some(1),
        '二' => Some(2),
        '三' => Some(3),
        '四' => Some(4),
        '五' => Some(5),
        '六' => Some(6),
        '七' => Some(7),
        '八' => Some(8),
        '九' => Some(9),
        '两' => Some(2),
        _ => None,
    }
}

/// True if `c` is a Chinese digit character (零一二三四五六七八九两).
fn is_cn_digit(c: char) -> bool {
    digit_val(c).is_some()
}

/// True if `c` is a Chinese numeric multiplier (十百千万亿).
fn is_cn_multiplier(c: char) -> bool {
    matches!(c, '十' | '百' | '千' | '万' | '亿')
}

/// True if `c` is part of a Chinese numeric expression.
fn is_cn_numeric(c: char) -> bool {
    is_cn_digit(c) || is_cn_multiplier(c)
}

/// Date/time suffixes that indicate the preceding number is a date component.
fn is_date_suffix(c: char) -> bool {
    matches!(c, '年' | '月' | '号' | '日' | '时' | '分' | '秒' | '点')
}

/// Context markers that indicate a non-numeric use (e.g. "一个", "一些").
fn is_measure_word(c: char) -> bool {
    matches!(c, '个' | '些' | '种' | '次' | '位' | '条' | '件' | '把'
        | '只' | '双' | '对' | '块' | '片' | '组' | '群' | '批'
        | '套' | '台' | '部' | '本' | '张' | '支' | '根' | '颗')
}

/// Convert a run of Chinese numeric characters to an Arabic number string.
///
/// Handles two patterns:
/// - Sequential digits: 一九二九 → 1929 (each char maps independently)
/// - Weighted: 一百二十三 → 123 (uses chinese-number crate)
fn convert_cn_number(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();

    // Check if it's all simple digits (no multipliers) — sequential pattern.
    let all_simple = chars.iter().all(|c| is_cn_digit(*c));
    if all_simple {
        return chars
            .iter()
            .filter_map(|c| digit_val(*c))
            .map(|d| (b'0' + d) as char)
            .collect();
    }

    // Weighted pattern: use chinese-number crate.
    match s.to_number(ChineseCountMethod::TenThousand) {
        Ok(n) => {
            let n: i64 = n;
            n.to_string()
        }
        Err(_) => s.to_owned(), // fallback: keep original
    }
}

/// Run Chinese number normalization on text.
///
/// Scans for runs of Chinese numeric characters and converts them based
/// on context:
/// - Before date suffixes (年月号日): always convert
/// - Before measure words (个些种): skip (keep Chinese)
/// - Adjacent to ASCII or "点": convert (version/decimal)
/// - Standalone numeric runs of 2+ chars: convert
#[must_use]
pub fn normalize_numbers(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut output = String::with_capacity(text.len());
    let mut i = 0;

    while i < chars.len() {
        // Handle "点" as decimal point or file extension dot.
        if chars[i] == '点' {
            let prev_is_digit = i > 0
                && (chars[i - 1].is_ascii_digit()
                    || is_cn_digit(chars[i - 1])
                    || chars[i - 1].is_ascii_alphanumeric());
            let next_is_digit_or_alpha = i + 1 < chars.len()
                && (is_cn_digit(chars[i + 1]) || chars[i + 1].is_ascii_alphanumeric());

            if prev_is_digit && next_is_digit_or_alpha {
                // Decimal or file extension: "三点一四" or "readme点md"
                output.push('.');
                // If next chars are Chinese digits after decimal point, convert them.
                if i + 1 < chars.len() && is_cn_digit(chars[i + 1]) {
                    i += 1;
                    while i < chars.len() && is_cn_digit(chars[i]) {
                        if let Some(d) = digit_val(chars[i]) {
                            output.push((b'0' + d) as char);
                        }
                        i += 1;
                    }
                    continue;
                }
                i += 1;
                continue;
            }
        }

        // Find a run of Chinese numeric characters.
        if is_cn_numeric(chars[i]) {
            let run_start = i;
            while i < chars.len() && is_cn_numeric(chars[i]) {
                i += 1;
            }
            let run: String = chars[run_start..i].iter().collect();

            // Single digit without context — check if it's a measure word pattern.
            if run.chars().count() == 1 {
                let next = if i < chars.len() { Some(chars[i]) } else { None };
                if next.map_or(false, is_measure_word) {
                    // "一个苹果" — keep Chinese
                    output.push_str(&run);
                    continue;
                }
            }

            // Check what follows the numeric run.
            let next = if i < chars.len() { Some(chars[i]) } else { None };

            if next.map_or(false, is_date_suffix) {
                // Date context: always convert.
                output.push_str(&convert_cn_number(&run));
            } else if next == Some('点') {
                // Decimal: convert the integer part, "点" handled next iteration.
                output.push_str(&convert_cn_number(&run));
            } else if run.chars().count() >= 2 {
                // Multi-char numeric run: convert.
                output.push_str(&convert_cn_number(&run));
            } else {
                // Single digit, ambiguous context: keep.
                output.push_str(&run);
            }
            continue;
        }

        output.push(chars[i]);
        i += 1;
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    // Decimals
    #[test]
    fn decimal() {
        assert_eq!(normalize_numbers("零点一"), "0.1");
    }

    #[test]
    fn decimal_pi() {
        assert_eq!(normalize_numbers("三点一四一五九二六"), "3.1415926");
    }

    // Version
    #[test]
    fn version() {
        assert_eq!(normalize_numbers("v零点一"), "v0.1");
    }

    // File extension
    #[test]
    fn file_extension() {
        assert_eq!(normalize_numbers("readme点md"), "readme.md");
    }

    // Year
    #[test]
    fn year() {
        assert_eq!(normalize_numbers("二零二六年"), "2026年");
    }

    #[test]
    fn year_month_day() {
        assert_eq!(normalize_numbers("二零二六年三月一号"), "2026年3月1号");
    }

    // Weighted numbers
    #[test]
    fn hundred() {
        assert_eq!(normalize_numbers("一百二十三"), "123");
    }

    #[test]
    fn twenty() {
        assert_eq!(normalize_numbers("二十"), "20");
    }

    // Measure word — keep Chinese
    #[test]
    fn measure_word_kept() {
        assert_eq!(normalize_numbers("一个苹果"), "一个苹果");
    }

    #[test]
    fn measure_word_kept_2() {
        assert_eq!(normalize_numbers("两个人"), "两个人");
    }

    // Sequential digits
    #[test]
    fn sequential_digits() {
        assert_eq!(normalize_numbers("一九二九"), "1929");
    }

    // Mixed
    #[test]
    fn mixed_text() {
        assert_eq!(
            normalize_numbers("版本v零点一发布于二零二六年"),
            "版本v0.1发布于2026年"
        );
    }

    // No numbers — unchanged
    #[test]
    fn no_numbers() {
        assert_eq!(normalize_numbers("这是正常的句子"), "这是正常的句子");
    }

    // Empty
    #[test]
    fn empty() {
        assert_eq!(normalize_numbers(""), "");
    }
}
