//! Spoken-to-written normalization for Chinese ASR transcripts.
//!
//! Converts spoken Chinese forms to their written equivalents:
//! - Dot normalization: "readme 点 md" → "readme.md" (ASCII context only)
//! - Percentages: 百分之五十 → 50%
//! - Sequential digits: 二零零八 → 2008, 幺三八 → 138
//! - Arithmetic numbers: 一百二十三 → 123, 三千五百万 → 35000000
//! - Decimals: 三点一四 → 3.14
//! - Year pattern: 二零零八年 → 二〇〇八年 (零→〇, keep Chinese)
//!
//! Uses structural pattern matching — no hardcoded exclusion word lists.
//! A Chinese digit is only converted when in "numeric context" (adjacent to
//! other digits, magnitude words, or part of a recognized pattern).

/// Run all normalization steps on `text`.
#[must_use]
pub fn normalize_spoken(text: &str) -> String {
    let result = normalize_dots(text);
    let result = normalize_percentages(&result);
    normalize_numbers(&result)
}

// ---------------------------------------------------------------------------
// Chinese digit utilities
// ---------------------------------------------------------------------------

/// Map a Chinese digit character to its numeric value.
fn digit_value(c: char) -> Option<u8> {
    match c {
        '零' | '〇' => Some(0),
        '幺' | '一' | '壹' => Some(1),
        '二' | '贰' => Some(2),
        '三' | '叁' => Some(3),
        '四' | '肆' => Some(4),
        '五' | '伍' => Some(5),
        '六' | '陆' => Some(6),
        '七' | '柒' => Some(7),
        '八' | '捌' => Some(8),
        '九' | '玖' => Some(9),
        _ => None,
    }
}

fn is_digit_char(c: char) -> bool {
    digit_value(c).is_some()
}

/// Map a magnitude character to its multiplier.
fn magnitude_value(c: char) -> Option<u64> {
    match c {
        '十' => Some(10),
        '百' => Some(100),
        '千' => Some(1000),
        '万' => Some(10_000),
        '亿' => Some(100_000_000),
        _ => None,
    }
}

fn is_magnitude(c: char) -> bool {
    magnitude_value(c).is_some()
}

/// Value of 两 — only valid before magnitude words.
const LIANG_VALUE: u64 = 2;

// ---------------------------------------------------------------------------
// Step 1: Dot normalization
// ---------------------------------------------------------------------------

/// Convert "点" to "." when nearest non-space chars on both sides are ASCII
/// alphanumeric. Surrounding spaces are consumed. After conversion, also
/// collapse space-separated single ASCII letters that form a file extension
/// (e.g. "m d" → "md") since ASR often splits short tokens.
fn normalize_dots(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if chars[i] == '点' {
            // Look backward for nearest non-space char
            let prev = chars[..i].iter().rev().find(|c| **c != ' ');
            // Look forward for nearest non-space char
            let next = chars[i + 1..].iter().find(|c| **c != ' ');

            if let (Some(&p), Some(&n)) = (prev, next) {
                if p.is_ascii_alphanumeric() && n.is_ascii_alphanumeric() {
                    // Trim trailing spaces before 点, and also collapse
                    // space-separated single letters before the dot
                    // ("read me" stays, but individual letters merge).
                    while result.ends_with(' ') {
                        result.pop();
                    }
                    result.push('.');
                    // Skip leading spaces after 点, then collect the
                    // extension: merge space-separated single ASCII letters.
                    i += 1;
                    while i < len && chars[i] == ' ' {
                        i += 1;
                    }
                    // Merge single-letter tokens: "m d" → "md"
                    while i < len {
                        if chars[i].is_ascii_alphabetic() {
                            result.push(chars[i]);
                            i += 1;
                            // If next is a space followed by a single letter,
                            // consume the space and continue
                            if i < len && chars[i] == ' ' {
                                if i + 1 < len
                                    && chars[i + 1].is_ascii_alphabetic()
                                    && (i + 2 >= len
                                        || !chars[i + 2].is_ascii_alphabetic())
                                {
                                    // Skip space, next iteration picks up the letter
                                    i += 1;
                                    continue;
                                }
                            }
                            break;
                        } else {
                            break;
                        }
                    }
                    continue;
                }
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

// ---------------------------------------------------------------------------
// Step 2: Percentage normalization
// ---------------------------------------------------------------------------

/// Convert 百分之 + chinese_number → {number}%.
fn normalize_percentages(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut remaining = text;

    while let Some(pos) = remaining.find("百分之") {
        result.push_str(&remaining[..pos]);
        let after = &remaining[pos + "百分之".len()..];

        // Try to parse the number after 百分之
        let (num_str, consumed) = parse_number_at(after);
        if consumed > 0 {
            result.push_str(&num_str);
            result.push('%');
            remaining = &after[consumed..];
        } else {
            result.push_str("百分之");
            remaining = after;
        }
    }
    result.push_str(remaining);
    result
}

// ---------------------------------------------------------------------------
// Step 3: Number normalization
// ---------------------------------------------------------------------------

/// Scan text and convert Chinese numbers in numeric context to Arabic digits.
fn normalize_numbers(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut result = String::with_capacity(text.len());
    let mut i = 0;

    while i < len {
        // Try to match a numeric span starting at i
        if let Some((converted, consumed)) = try_convert_number(&chars, i) {
            result.push_str(&converted);
            i += consumed;
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    result
}

/// Try to match and convert a Chinese number starting at `start`.
/// Returns (converted_string, chars_consumed) or None.
fn try_convert_number(chars: &[char], start: usize) -> Option<(String, usize)> {
    let len = chars.len();
    if start >= len {
        return None;
    }

    let c = chars[start];

    // Must start with a digit, 两, or small magnitude (十百千)
    // 万/亿 alone cannot start a number (万一 is idiomatic)
    if !is_digit_char(c) && c != '两' && !matches!(c, '十' | '百' | '千') {
        return None;
    }

    // Scan the full numeric span
    let span_end = scan_numeric_span(chars, start);
    let span_len = span_end - start;

    if span_len == 0 {
        return None;
    }

    let span: Vec<char> = chars[start..span_end].to_vec();

    // Check if followed by 年 → year pattern
    if span_end < len && chars[span_end] == '年' && is_sequential_pattern(&span) {
        let year = year_normalize(&span);
        return Some((year, span_len));
    }

    // Preceded by 第 → ordinal, skip entire span.
    // Also check if any earlier position in this span is preceded by 第
    // (e.g. 第一百二十三: 一 is blocked, but 百 at start+1 would try again).
    if start > 0 && chars[start - 1] == '第' {
        return None;
    }
    // If previous char is a digit/magnitude that was itself preceded by 第,
    // we're inside an ordinal span — check backwards to text boundary.
    {
        let mut j = start;
        while j > 0 && (is_digit_char(chars[j - 1]) || is_magnitude(chars[j - 1]) || chars[j - 1] == '两') {
            j -= 1;
        }
        if j > 0 && chars[j - 1] == '第' {
            return None;
        }
    }

    // Check numeric context: need ≥2 chars, or magnitude involvement, or decimal
    if !is_numeric_context(&span) {
        return None;
    }

    // Check for decimal: span contains 点 between digits
    if let Some(decimal_result) = try_parse_decimal(&span) {
        return Some((decimal_result, span_len));
    }

    // Check if sequential pattern (all simple digits, contains 零/〇, or has 幺)
    if is_sequential_pattern(&span) {
        let s: String = span
            .iter()
            .filter_map(|&c| digit_value(c).map(|d| char::from(b'0' + d)))
            .collect();
        return Some((s, span_len));
    }

    // Arithmetic number
    if let Some(value) = parse_arithmetic(&span) {
        return Some((format_with_commas(value), span_len));
    }

    None
}

/// Scan from `start` to find the end of a contiguous numeric span.
/// A numeric span contains digits, magnitudes, 两, 点(between digits).
fn scan_numeric_span(chars: &[char], start: usize) -> usize {
    let len = chars.len();
    let mut i = start;

    while i < len {
        let c = chars[i];
        if is_digit_char(c) || is_magnitude(c) || c == '两' {
            i += 1;
        } else if c == '点' {
            // Only include 点 if followed by a digit char
            if i + 1 < len && is_digit_char(chars[i + 1]) {
                i += 1;
            } else {
                break;
            }
        } else if c == '零' {
            // Already handled by is_digit_char
            i += 1;
        } else {
            break;
        }
    }
    i
}

/// Check if a span is in "numeric context" (should be converted).
fn is_numeric_context(span: &[char]) -> bool {
    if span.len() >= 2 {
        return true;
    }
    // Single char: only convert if it's a magnitude adjacent to context
    // (handled by span scanning — single non-contextualized chars won't
    // reach here because scan_numeric_span stops at 1 for isolated digits)
    false
}

/// Check if a span looks like sequential digits (phone numbers, codes).
/// Sequential = all simple digit chars (no magnitude words except as part
/// of 零). Contains 零/〇 or 幺, which signals sequential style.
fn is_sequential_pattern(span: &[char]) -> bool {
    let has_magnitude = span.iter().any(|&c| {
        matches!(c, '十' | '百' | '千' | '万' | '亿')
    });
    if has_magnitude {
        return false;
    }
    // Must be all digit chars (no 两, no 点)
    span.iter().all(|&c| is_digit_char(c))
}

/// Normalize year pattern: keep Chinese form, replace 零 with 〇.
fn year_normalize(span: &[char]) -> String {
    span.iter()
        .map(|&c| if c == '零' { '〇' } else { c })
        .collect()
}

/// Try to parse a decimal number: integer 点 fractional_digits.
fn try_parse_decimal(span: &[char]) -> Option<String> {
    let dot_pos = span.iter().position(|&c| c == '点')?;
    if dot_pos == 0 {
        return None;
    }

    let integer_part = &span[..dot_pos];
    let fractional_part = &span[dot_pos + 1..];

    if fractional_part.is_empty() {
        return None;
    }

    // Integer part: could be arithmetic or sequential
    let int_str = if integer_part.len() == 1 {
        digit_value(integer_part[0])?.to_string()
    } else if let Some(v) = parse_arithmetic(integer_part) {
        format_with_commas(v)
    } else {
        // Sequential
        integer_part
            .iter()
            .filter_map(|&c| digit_value(c).map(|d| char::from(b'0' + d)))
            .collect()
    };

    // Fractional part: always sequential (digit by digit)
    let frac_str: String = fractional_part
        .iter()
        .filter_map(|&c| digit_value(c).map(|d| char::from(b'0' + d)))
        .collect();

    Some(format!("{int_str}.{frac_str}"))
}

// ---------------------------------------------------------------------------
// Arithmetic number parser
// ---------------------------------------------------------------------------

/// Parse a Chinese arithmetic number like 一百二十三 → 123.
/// Handles magnitude-based grouping with 万/亿 levels.
fn parse_arithmetic(span: &[char]) -> Option<u64> {
    if span.is_empty() {
        return None;
    }

    // Must contain at least one magnitude word to be arithmetic
    let has_magnitude = span.iter().any(|&c| is_magnitude(c));
    if !has_magnitude && !span.iter().any(|&c| c == '两') {
        return None;
    }

    // Split at 亿
    let (yi_parts, yi_positions) = split_at_magnitude(span, '亿');
    let mut total: u64 = 0;

    for (idx, part) in yi_parts.iter().enumerate() {
        let group_value = parse_wan_group(part)?;
        if idx < yi_positions.len() {
            total += group_value * 100_000_000;
        } else {
            total += group_value;
        }
    }

    Some(total)
}

/// Parse a group that may contain 万 as the highest magnitude.
fn parse_wan_group(span: &[char]) -> Option<u64> {
    if span.is_empty() {
        return Some(0);
    }

    let (wan_parts, wan_positions) = split_at_magnitude(span, '万');
    let mut total: u64 = 0;

    for (idx, part) in wan_parts.iter().enumerate() {
        let group_value = parse_small_group(part)?;
        if idx < wan_positions.len() {
            total += group_value * 10_000;
        } else {
            total += group_value;
        }
    }

    Some(total)
}

/// Parse a group with at most 千/百/十 magnitudes.
fn parse_small_group(span: &[char]) -> Option<u64> {
    if span.is_empty() {
        return Some(0);
    }

    let mut total: u64 = 0;
    let mut current: Option<u64> = None;
    let mut i = 0;

    while i < span.len() {
        let c = span[i];

        if c == '零' || c == '〇' {
            // Placeholder: skip, next digit goes to ones place
            current = None;
            i += 1;
            continue;
        }

        if c == '两' {
            current = Some(LIANG_VALUE);
            i += 1;
            continue;
        }

        if let Some(d) = digit_value(c) {
            current = Some(d as u64);
            i += 1;
            continue;
        }

        if let Some(mag) = magnitude_value(c) {
            if mag <= 1000 {
                // 十百千: multiply current digit by magnitude
                let digit = current.unwrap_or(1); // bare 十 = 10, bare 百 = 100
                total += digit * mag;
                current = None;
                i += 1;
                continue;
            }
        }

        // Unknown char in span
        return None;
    }

    // Trailing digit (ones place)
    if let Some(d) = current {
        total += d;
    }

    Some(total)
}

/// Split a span at occurrences of a specific magnitude character.
/// Returns (parts, positions_of_magnitude).
fn split_at_magnitude(span: &[char], mag: char) -> (Vec<Vec<char>>, Vec<usize>) {
    let mut parts = Vec::new();
    let mut positions = Vec::new();
    let mut current = Vec::new();

    for (i, &c) in span.iter().enumerate() {
        if c == mag {
            parts.push(current.clone());
            positions.push(i);
            current.clear();
        } else {
            current.push(c);
        }
    }
    parts.push(current);
    (parts, positions)
}

// ---------------------------------------------------------------------------
// Helper: parse number at position (for percentage)
// ---------------------------------------------------------------------------

/// Parse a Chinese number starting at the beginning of `text`.
/// Returns (arabic_string, bytes_consumed).
fn parse_number_at(text: &str) -> (String, usize) {
    let chars: Vec<char> = text.chars().collect();
    let span_end = scan_numeric_span(&chars, 0);

    if span_end == 0 {
        // Special case: bare 百 after 百分之
        if chars.first() == Some(&'百') {
            return ("100".to_string(), '百'.len_utf8());
        }
        return (String::new(), 0);
    }

    let span: Vec<char> = chars[..span_end].to_vec();
    let byte_len: usize = span.iter().map(|c| c.len_utf8()).sum();

    // Try decimal
    if let Some(decimal_result) = try_parse_decimal(&span) {
        return (decimal_result, byte_len);
    }

    // Try arithmetic
    if let Some(value) = parse_arithmetic(&span) {
        return (format_with_commas(value), byte_len);
    }

    // Sequential
    if span.iter().all(|&c| is_digit_char(c)) {
        let s: String = span
            .iter()
            .filter_map(|&c| digit_value(c).map(|d| char::from(b'0' + d)))
            .collect();
        return (s, byte_len);
    }

    (String::new(), 0)
}

// ---------------------------------------------------------------------------
/// Format an integer with comma separators every 3 digits.
/// e.g. 1000000 → "1,000,000"
fn format_with_commas(n: u64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let len = bytes.len();
    if len <= 3 {
        return s;
    }
    let mut result = String::with_capacity(len + len / 3);
    let first_group = len % 3;
    if first_group > 0 {
        result.push_str(&s[..first_group]);
    }
    for (i, chunk) in s[first_group..].as_bytes().chunks(3).enumerate() {
        if i > 0 || first_group > 0 {
            result.push(',');
        }
        result.push_str(std::str::from_utf8(chunk).unwrap());
    }
    result
}

// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Dot normalization --

    #[test]
    fn dot_readme_md() {
        assert_eq!(normalize_spoken("readme 点 md"), "readme.md");
    }

    #[test]
    fn dot_config_toml() {
        assert_eq!(normalize_spoken("config 点 toml"), "config.toml");
    }

    #[test]
    fn dot_numeric() {
        assert_eq!(normalize_spoken("3 点 14"), "3.14");
    }

    #[test]
    fn dot_multiple() {
        assert_eq!(normalize_spoken("1 点 0 点 0"), "1.0.0");
    }

    #[test]
    fn dot_split_extension() {
        // ASR splits "md" into "m d"
        assert_eq!(normalize_spoken("readme 点 m d"), "readme.md");
    }

    #[test]
    fn dot_split_extension_install() {
        assert_eq!(normalize_spoken("install 点 m d"), "install.md");
    }

    #[test]
    fn dot_split_extension_sh() {
        assert_eq!(normalize_spoken("install 点 s h"), "install.sh");
    }

    #[test]
    fn dot_no_convert_cjk_before() {
        assert_eq!(normalize_spoken("好一点"), "好一点");
    }

    #[test]
    fn dot_no_convert_cjk_after() {
        assert_eq!(normalize_spoken("点头"), "点头");
    }

    #[test]
    fn dot_no_convert_yidian() {
        assert_eq!(normalize_spoken("一点也不"), "一点也不");
    }

    #[test]
    fn dot_no_convert_youdian() {
        assert_eq!(normalize_spoken("有一点"), "有一点");
    }

    #[test]
    fn dot_at_start() {
        assert_eq!(normalize_spoken("点什么"), "点什么");
    }

    #[test]
    fn dot_at_end() {
        assert_eq!(normalize_spoken("test 点"), "test 点");
    }

    // -- Sequential numbers --

    #[test]
    fn seq_2008() {
        assert_eq!(normalize_spoken("二零零八"), "2008");
    }

    #[test]
    fn seq_13800() {
        assert_eq!(normalize_spoken("一三八零零"), "13800");
    }

    #[test]
    fn seq_2026() {
        assert_eq!(normalize_spoken("二零二六"), "2026");
    }

    #[test]
    fn seq_007() {
        assert_eq!(normalize_spoken("零零七"), "007");
    }

    #[test]
    fn seq_yao() {
        assert_eq!(normalize_spoken("幺三八幺四"), "13814");
    }

    // -- Year pattern --

    #[test]
    fn year_2008() {
        assert_eq!(normalize_spoken("二零零八年"), "二〇〇八年");
    }

    #[test]
    fn year_2026() {
        assert_eq!(normalize_spoken("二零二六年"), "二〇二六年");
    }

    // -- Arithmetic numbers --

    #[test]
    fn arith_123() {
        assert_eq!(normalize_spoken("一百二十三"), "123");
    }

    #[test]
    fn arith_3500() {
        assert_eq!(normalize_spoken("三千五百"), "3,500");
    }

    #[test]
    fn arith_35million() {
        assert_eq!(normalize_spoken("三千五百万"), "35,000,000");
    }

    #[test]
    fn arith_10000() {
        assert_eq!(normalize_spoken("一万"), "10,000");
    }

    #[test]
    fn arith_12() {
        assert_eq!(normalize_spoken("十二"), "12");
    }

    #[test]
    fn arith_20() {
        assert_eq!(normalize_spoken("二十"), "20");
    }

    #[test]
    fn arith_50() {
        assert_eq!(normalize_spoken("五十"), "50");
    }

    #[test]
    fn arith_100000() {
        assert_eq!(normalize_spoken("十万"), "100,000");
    }

    #[test]
    fn arith_123million() {
        assert_eq!(normalize_spoken("一亿两千三百万"), "123,000,000");
    }

    #[test]
    fn arith_200() {
        assert_eq!(normalize_spoken("两百"), "200");
    }

    #[test]
    fn arith_1003_placeholder() {
        assert_eq!(normalize_spoken("一千零三"), "1,003");
    }

    #[test]
    fn arith_205_placeholder() {
        assert_eq!(normalize_spoken("二百零五"), "205");
    }

    #[test]
    fn arith_10001_placeholder() {
        assert_eq!(normalize_spoken("一万零一"), "10,001");
    }

    // -- Decimals --

    #[test]
    fn decimal_pi() {
        assert_eq!(normalize_spoken("三点一四一五九二六"), "3.1415926");
    }

    #[test]
    fn decimal_half() {
        assert_eq!(normalize_spoken("零点五"), "0.5");
    }

    // -- Percentages --

    #[test]
    fn pct_50() {
        assert_eq!(normalize_spoken("百分之五十"), "50%");
    }

    #[test]
    fn pct_pi() {
        assert_eq!(normalize_spoken("百分之三点一四"), "3.14%");
    }

    #[test]
    fn pct_100() {
        assert_eq!(normalize_spoken("百分之百"), "100%");
    }

    #[test]
    fn pct_no_zhi() {
        // 百分 without 之 → unchanged
        assert_eq!(normalize_spoken("百分不够"), "百分不够");
    }

    // -- Ordinals --

    #[test]
    fn ordinal_di_yi() {
        assert_eq!(normalize_spoken("第一"), "第一");
    }

    #[test]
    fn ordinal_di_123() {
        assert_eq!(normalize_spoken("第一百二十三"), "第一百二十三");
    }

    #[test]
    fn ordinal_di_san() {
        assert_eq!(normalize_spoken("第三"), "第三");
    }

    // -- Non-conversion (negative tests) --

    #[test]
    fn no_convert_yibufen() {
        assert_eq!(normalize_spoken("一部分"), "一部分");
    }

    #[test]
    fn no_convert_shiquanshimei() {
        assert_eq!(normalize_spoken("十全十美"), "十全十美");
    }

    #[test]
    fn no_convert_sanxineryi() {
        assert_eq!(normalize_spoken("三心二意"), "三心二意");
    }

    #[test]
    fn no_convert_yimoyiyang() {
        assert_eq!(normalize_spoken("一模一样"), "一模一样");
    }

    #[test]
    fn no_convert_qishangbaxia() {
        assert_eq!(normalize_spoken("七上八下"), "七上八下");
    }

    #[test]
    fn no_convert_yiban() {
        assert_eq!(normalize_spoken("一般"), "一般");
    }

    #[test]
    fn no_convert_wanyi() {
        assert_eq!(normalize_spoken("万一"), "万一");
    }

    #[test]
    fn no_convert_busanbusi() {
        assert_eq!(normalize_spoken("不三不四"), "不三不四");
    }

    #[test]
    fn no_convert_yigeren() {
        assert_eq!(normalize_spoken("一个人"), "一个人");
    }

    #[test]
    fn no_convert_liangge() {
        assert_eq!(normalize_spoken("两个"), "两个");
    }

    #[test]
    fn no_convert_yidian_ye_bu() {
        assert_eq!(normalize_spoken("一点也不"), "一点也不");
    }

    // -- Mixed content --

    #[test]
    fn mixed_readme() {
        assert_eq!(
            normalize_spoken("打开 readme 点 md"),
            "打开 readme.md"
        );
    }

    #[test]
    fn mixed_money() {
        assert_eq!(
            normalize_spoken("他花了三千五百块"),  // 3,500 with commas
            "他花了3,500块"
        );
    }

    // -- Empty / no-op --

    #[test]
    fn empty_input() {
        assert_eq!(normalize_spoken(""), "");
    }

    #[test]
    fn no_numbers() {
        assert_eq!(normalize_spoken("今天天气很好"), "今天天气很好");
    }
}
