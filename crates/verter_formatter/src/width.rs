//! Pinned string-width 4 semantics, including its emoji and ANSI recognizers.
//! The generated patterns are UTF-16 shadow alphabets: surrogate code units
//! map to U+F0000..U+F07FF so Rust's Unicode regex can match the original JS
//! patterns exactly without admitting invalid UTF-8. See tests/generate-width-data.cjs.
use regex::Regex;
use std::borrow::Cow;
use std::sync::OnceLock;

pub(crate) fn strip_ansi(text: &str) -> Cow<'_, str> {
    static ANSI: OnceLock<Regex> = OnceLock::new();
    if !text.contains(['\x1b', '\u{9b}']) {
        return Cow::Borrowed(text);
    }
    ANSI.get_or_init(|| {
        Regex::new(include_str!("width_data/ansi.regex")).expect("pinned ANSI pattern")
    })
    .replace_all(text, "")
}

pub(crate) fn columns(text: &str) -> usize {
    if text.is_ascii() {
        return text.bytes().filter(|&b| b > 0x1f && b != 0x7f).count();
    }
    static EMOJI: OnceLock<Regex> = OnceLock::new();
    let emoji = EMOJI.get_or_init(|| {
        Regex::new(include_str!("width_data/emoji.regex")).expect("pinned emoji pattern")
    });
    let shadow: String = text
        .encode_utf16()
        .map(|c| {
            let c = u32::from(c);
            char::from_u32(if (0xd800..=0xdfff).contains(&c) {
                c + 0xf0000 - 0xd800
            } else {
                c
            })
            .expect("shadow scalar")
        })
        .collect();
    let mut end = 0;
    let mut width = 0;
    for m in emoji.find_iter(&shadow) {
        width += scalar_columns(&shadow[end..m.start()]) + 2;
        end = m.end();
    }
    width + scalar_columns(&shadow[end..])
}

fn scalar_columns(shadow: &str) -> usize {
    let mut chars = shadow.chars();
    let mut width = 0;
    while let Some(c) = chars.next() {
        let mut c = u32::from(c);
        if (0xf0000..=0xf03ff).contains(&c) {
            let high = c - 0xf0000;
            let low = u32::from(chars.next().expect("paired shadow surrogate")) - 0xf0400;
            c = 0x10000 + (high << 10) + low;
        }
        if c <= 0x1f || (0x7f..=0x9f).contains(&c) || (0x300..=0x36f).contains(&c) {
            continue;
        }
        width += if fullwidth(c) { 2 } else { 1 };
    }
    width
}

// is-fullwidth-code-point 3.0.0's East Asian width ranges (MIT).
fn fullwidth(c: u32) -> bool {
    matches!(c, 0x1100..=0x115f | 0x2329..=0x232a | 0x2e80..=0x303e |
        0x3040..=0x3247 | 0x3250..=0x4dbf | 0x4e00..=0xa4c6 |
        0xa960..=0xa97c | 0xac00..=0xd7a3 | 0xf900..=0xfaff |
        0xfe10..=0xfe19 | 0xfe30..=0xfe6b | 0xff01..=0xff60 |
        0xffe0..=0xffe6 | 0x1b000..=0x1b001 | 0x1f200..=0x1f251 |
        0x20000..=0x3fffd)
}
