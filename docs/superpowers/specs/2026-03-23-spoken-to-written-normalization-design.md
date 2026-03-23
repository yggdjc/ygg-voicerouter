# Spoken-to-Written Normalization Design

## Goal

Convert spoken Chinese forms to written equivalents in ASR output, covering numbers, decimals, percentages, and filename dots. No hardcoded exclusion word lists — use structural pattern matching to distinguish numeric usage from idiomatic usage.

## Scope

### In scope
- Sequential digit conversion: 二零零八 → 2008
- Decimal conversion: 三点一四 → 3.14
- Percentage conversion: 百分之五十 → 50%
- Arithmetic number conversion: 一百二十三 → 123, 三千五百万 → 35000000
- Dot-to-period for filenames: "readme 点 md" → "readme.md" (only when both sides are ASCII)

### Out of scope
- Date/time normalization (二零二六年三月 → 2026年3月)
- Symbol verbalization (下划线→_, 斜杠→/)
- Programming term conversion
- Case control (大写→uppercase)

## Architecture

New module `src/postprocess/normalize.rs` in the existing postprocess pipeline.

### Pipeline position

```
remove_fillers → normalize → space_cjk_ascii_boundary → fix_broken_english → fullwidth_to_half_in_ascii → half_to_fullwidth → apply_punct_mode
```

Normalize runs before `space_cjk_ascii_boundary` so it sees the raw de-fillered text without inserted spaces. This ensures patterns like "二零零八年" are contiguous when matched. After normalization converts them to "2008年", the subsequent `space_cjk_ascii_boundary` step correctly inserts spaces at the new CJK/ASCII boundaries.

### Processing steps (in order)

1. **Dot normalization**: "点" → `.` when the nearest non-space characters on both sides are ASCII alphanumeric. Surrounding spaces are consumed. ("readme 点 md" → "readme.md"). Multiple dots are handled independently ("1 点 0 点 0" → "1.0.0").
2. **Percentage**: 百分之 + chinese_number → {number}%。百 after 百分之 is parsed as the number 100 (百分之百 → 100%).
3. **General number conversion**: sequential digits, arithmetic numbers, decimals. Decimal "点" between Chinese digits is handled here (三点一四→3.14), NOT by step 1 (which only handles ASCII context).

### Disambiguation rules

**第 + number**: Ordinal prefix 第 suppresses conversion. 第一百二十三 → 第一百二十三 (not 第123). Implementation: skip numeric pattern when preceded by 第.

**Year pattern**: Sequential Chinese digits followed by 年 → keep Chinese form, replace 零 with 〇. 二零零八年→二〇〇八年, 二零二六年→二〇二六年. This takes priority over normal sequential digit conversion.

**两 vs 二**: 两 only converts in arithmetic context (before a magnitude word: 两百→200, 两千→2000, 两万→20000). 两 does NOT appear in sequential digit strings. 两个 → 两个 (not converted; 个 is a measure word, not a numeric unit).

### Numeric context detection (no exclusion word list)

A Chinese digit is only converted when it appears in a "numeric context" — at least one of:

- **Adjacent to another Chinese digit character** (≥2 consecutive): 二零, 一三八
- **Adjacent to a magnitude word** (十百千万亿): 三百, 一千五
- **Part of 百分之 pattern**: 百分之五十
- **Part of decimal pattern**: number + 点 + number

A single Chinese digit surrounded only by non-digit, non-magnitude CJK characters is NOT in numeric context and is left unchanged.

Examples of what is NOT converted:
- 一部分 (一 surrounded by ordinary CJK)
- 十全十美 (十 not adjacent to digit or magnitude, separated by non-numeric CJK)
- 三心二意 (digits separated by non-numeric CJK)
- 一模一样, 七上八下, 不三不四 (same pattern)
- 万一 (idiomatic usage; 万 preceded by nothing, 一 followed by non-numeric CJK)
- 一般, 一个人 (一 followed by non-numeric, non-unit CJK)

### Chinese digit mapping

| Character | Value |
|-----------|-------|
| 零/〇 | 0 |
| 幺 | 1 (sequential only, e.g. phone numbers) |
| 一/壹 | 1 |
| 二/贰 | 2 |
| 两 | 2 (arithmetic only, before magnitude words) |
| 三/叁 | 3 |
| 四/肆 | 4 |
| 五/伍 | 5 |
| 六/陆 | 6 |
| 七/柒 | 7 |
| 八/捌 | 8 |
| 九/玖 | 9 |

Magnitude words: 十(10), 百(100), 千(1000), 万(10000), 亿(100000000)

### Arithmetic number parsing algorithm

Parse Chinese arithmetic numbers using magnitude-based grouping:

1. Split at 亿 boundary → parse each group
2. Within each group, split at 万 boundary → parse each sub-group
3. Within each sub-group, accumulate: digit × magnitude(千/百/十), plus trailing digit
4. Bare magnitude (百, 千) implies leading 一 (百 = 100, 千 = 1000)
5. 零 as placeholder: skip to next digit position (一千零三 = 1000 + 3 = 1003)
6. Example: 一亿两千三百万 → 1×亿 + (2×千 + 3×百)×万 = 123000000

### Config

`PostprocessConfig.normalize_spoken: bool` — default `true`.

## Testing strategy

Comprehensive test coverage with positive and negative cases:

**Dot normalization** (~10 tests):
- readme 点 md → readme.md
- config 点 toml → config.toml
- 3 点 14 → 3.14
- 1 点 0 点 0 → 1.0.0 (multiple dots)
- 好一点 → 好一点 (no conversion — 一 is CJK, not ASCII)
- 点头 → 点头 (no conversion — 头 is CJK)
- 一点也不 → 一点也不 (no conversion)
- 有一点 → 有一点 (no conversion)
- 点 at start/end of text → unchanged

**Sequential numbers** (~10 tests):
- 二零零八 → 2008
- 一三八零零 → 13800
- 二零二六 → 2026
- 零零七 → 007
- 幺三八幺四 → 13814
- 二零零八年 → 二〇〇八年 (year: keep Chinese, 零→〇)
- 二零二六年 → 二〇二六年 (year: keep Chinese, 零→〇)

**Arithmetic numbers** (~15 tests):
- 一百二十三 → 123
- 三千五百 → 3500
- 三千五百万 → 35000000
- 一万 → 10000
- 十二 → 12
- 二十 → 20
- 五十 → 50
- 十万 → 100000
- 一亿两千三百万 → 123000000
- 两百 → 200
- 百 (bare, after 百分之) → 100
- 一千零三 → 1003 (零 as placeholder)
- 二百零五 → 205 (零 as placeholder)
- 一万零一 → 10001 (零 as placeholder)

**Decimals** (~5 tests):
- 三点一四一五九二六 → 3.1415926
- 零点五 → 0.5

**Percentages** (~5 tests):
- 百分之五十 → 50%
- 百分之三点一四 → 3.14%
- 百分之百 → 100%
- 百分 without 之 → unchanged

**Ordinals** (~3 tests):
- 第一 → 第一
- 第一百二十三 → 第一百二十三
- 第三 → 第三

**Non-conversion (negative tests)** (~15 tests):
- 一部分 → 一部分
- 十全十美 → 十全十美
- 三心二意 → 三心二意
- 一模一样 → 一模一样
- 七上八下 → 七上八下
- 一般 → 一般
- 万一 → 万一
- 不三不四 → 不三不四
- 一个人 → 一个人
- 两个 → 两个
- 一点也不 → 一点也不

**Mixed content** (~5 tests):
- 打开 readme 点 md → 打开 readme.md
- 他花了三千五百块 → 他花了3500块

## Estimated size

~500-600 lines of Rust (implementation + tests), no new dependencies.
