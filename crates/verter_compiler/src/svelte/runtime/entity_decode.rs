//! HTML entity decode + attribute-value re-escape for the static skeleton.
//!
//! A faithful port of `svelte@5.56.3`'s `decode_character_references` +
//! `validate_code` (`phases/1-parse/utils/html.js`) plus the double-quoted
//! attribute-value re-escape (`escape_html(value, is_attr)`). The parser-stored
//! RAW attribute span is DECODED (named longest-match against the vendored HTML5
//! table + numeric refs with an OPTIONAL trailing `;`, the legacy no-`;` boundary
//! treating `_` as a word char) then RE-ESCAPED for the `[&"<]` context.

/// HTML-escape a static ATTRIBUTE VALUE for the double-quoted skeleton, matching
/// the official `escape_html(decode_character_references(raw, true), /*is_attr*/ true)`:
/// the parser-stored RAW attribute span is first DECODED (named + numeric entity
/// references resolve to their characters), then re-escaped for the double-quoted
/// context (`[&"<]` → `&amp;` / `&quot;` / `&lt;`).
///
/// Decode behavior (matching `svelte@5.56.3`'s `decode_character_references`):
///
/// - A NUMERIC reference (`&#65;` decimal / `&#x41;` hex) resolves to its code
///   point via [`validate_code`] (line feed → space, `128..=159` → the Windows-1252
///   remap, surrogate halves / out-of-range → `NUL`, everything else passes through).
/// - A NAMED reference resolves through the canonical HTML5 named-character-reference
///   table ([`super::entity_table`], the vendored official svelte table) by
///   LONGEST match; a legacy no-`;` form decodes only when NOT followed by `=` or an
///   alphanumeric (the HTML attribute-value named-reference rule).
/// - An UNKNOWN reference (`&bogus;`) is NOT decoded — its leading `&` is kept
///   literal, so the re-escape turns it into `&amp;bogus;` (the official behavior).
/// - A bare `&` (no reference) is kept literal → re-escaped to `&amp;`.
pub(super) fn escape_html_attr(value: &str) -> String {
    escape_html_attr_context(&decode_attr_entities(value))
}

/// Decode the named + numeric HTML entity references in a raw attribute value into
/// their characters against the canonical HTML5 table; an unknown reference and a
/// bare `&` are kept literal. A faithful port of
/// `decode_character_references(raw, is_attribute_value=true)` (svelte@5.56.3,
/// `phases/1-parse/utils/html.js`): the numeric pattern's trailing `;` is OPTIONAL
/// (`#(?:x[a-fA-F\d]+|\d+)(?:;)?`), and a legacy no-`;` NAMED reference matches only
/// at the official attribute-value boundary `\b(?!=)` — a following WORD char
/// (`[A-Za-z0-9_]`, so `_` blocks) or `=` prevents the match.
///
/// This is the DECODE-ONLY step (NO re-escaping). It is used both by
/// [`escape_html_attr`] (which then re-escapes for the double-quoted skeleton) and
/// directly by the runtime attribute lowering for a MIXED-attribute LITERAL chunk
/// (`title="&copy; {x} &bogus;"` → the literal `&copy; ` decodes to `© `, the
/// `&bogus;` stays literal, and the runtime concatenates `'© ' + x + ' &bogus;'` —
/// a runtime STRING value that is never re-escaped, matching svelte@5.56.3).
pub(super) fn decode_attr_entities(value: &str) -> String {
    decode_entities(value, /* is_attribute_value */ true)
}

/// Decode the named + numeric HTML entity references in a raw TEXT-CONTENT value
/// (the official `decode_character_references(raw, is_attribute_value=false)`):
/// identical to [`decode_attr_entities`] EXCEPT a legacy no-`;` NAMED reference
/// decodes UNCONDITIONALLY (the content-context entity pattern has NO `\b(?!=)`
/// boundary — only the attribute-value pattern restricts the legacy form). Used to
/// produce a `$.text(seed)` JS-STRING seed (a text-first region's decoded text) and
/// by downstream text-node emitters; the static `from_html` skeleton is NOT decoded
/// (its cloned-HTML template keeps the raw entities, which the browser decodes on
/// clone — verified against svelte@5.56.3).
pub(super) fn decode_text_entities(value: &str) -> String {
    decode_entities(value, /* is_attribute_value */ false)
}

/// The shared decode core for both the attribute-value and text-content contexts:
/// scan for `&`, decode each reference (named longest-match or numeric) per the
/// official `decode_character_references`, keep an undecodable `&` literal. The
/// `is_attribute_value` flag selects the legacy no-`;` named-reference boundary
/// rule (the attribute-value `\b(?!=)` restriction vs the unconditional content
/// form).
fn decode_entities(value: &str, is_attribute_value: bool) -> String {
    let bytes = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'&' {
            let ch = value[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        // A `&` — try to decode a reference (named longest-match or numeric).
        if let Some((decoded, consumed)) = decode_one_entity(&value[i..], is_attribute_value) {
            out.push_str(&decoded);
            i += consumed;
        } else {
            // Not a decodable reference — keep the literal `&`.
            out.push('&');
            i += 1;
        }
    }
    out
}

/// Public test hook: the DECODE-only result of [`decode_attr_entities`] for an
/// attribute-value context (no re-escaping). Lets the table-driven unit tests pin
/// the decode algorithm directly against official-derived cases, independent of
/// the skeleton serializer's re-escape step.
#[cfg(test)]
#[must_use]
pub fn decode_attribute_entities_for_test(value: &str) -> String {
    decode_attr_entities(value)
}

/// Public test hook: the DECODE-only result of [`decode_text_entities`] for a
/// TEXT-CONTENT context (no re-escaping). Pins the content-context decode (the
/// unconditional legacy no-`;` form) directly against official-derived cases.
#[cfg(test)]
#[must_use]
pub fn decode_text_entities_for_test(value: &str) -> String {
    decode_text_entities(value)
}

/// Whether `ch` is an HTML word character for the legacy named-entity attribute
/// boundary `\b`: `[A-Za-z0-9_]`. A following word char (notably `_`) prevents a
/// legacy no-`;` reference from matching. (`is_ascii_alphanumeric` ALONE is wrong —
/// it omits `_`, which the regex `\b` treats as a word char.)
fn is_entity_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

/// Try to decode ONE entity reference starting at the leading `&` of `s` (an
/// attribute-value context). Returns `(decoded_string, bytes_consumed)` on
/// success, or `None` when `s` does not begin a decodable reference (the `&` is
/// then kept literal).
fn decode_one_entity(s: &str, is_attribute_value: bool) -> Option<(String, usize)> {
    debug_assert_eq!(s.as_bytes().first(), Some(&b'&'));
    // Numeric reference: `&#65` / `&#65;` (decimal) / `&#x41` / `&#x41;` (hex). The
    // official pattern `#(?:x[a-fA-F\d]+|\d+)(?:;)?` makes the trailing `;`
    // OPTIONAL — the numeric run ends at the first non-matching char (or `;`).
    if s.as_bytes().get(1) == Some(&b'#') {
        // Determine the radix + the digit run length after `&#` (and an optional
        // `x`/`X`). The run ends at the first non-digit; a trailing `;` (if present)
        // is consumed as part of the reference.
        let after_hash = &s[2..];
        let (radix, digits_off) = match after_hash.as_bytes().first() {
            Some(b'x' | b'X') => (16u32, 1usize),
            _ => (10u32, 0usize),
        };
        let digits_str = &after_hash[digits_off..];
        let is_digit = |c: char| {
            if radix == 16 {
                c.is_ascii_hexdigit()
            } else {
                c.is_ascii_digit()
            }
        };
        let run_len = digits_str.chars().take_while(|&c| is_digit(c)).count();
        if run_len == 0 {
            return None; // `&#` / `&#x` with no digits — not a numeric reference.
        }
        let num = &digits_str[..run_len];
        // The official numeric decode is `parseInt(digits, radix)` (a JS number, no
        // 32-bit ceiling) followed by `if (!code) return match;` then
        // `validate_code(code)`. A value that OVERFLOWS `u32` (`&#9999999999;`) is a
        // TRUTHY but OUT-OF-RANGE code — `validate_code` maps every out-of-range code
        // to NUL — so it DECODES to the NUL char (it is NOT kept literal). Only a
        // FALSY code (`&#0` / `&#x0`) is kept literal.
        let bytes_consumed = {
            let mut consumed = 2 + digits_off + run_len;
            if s.as_bytes().get(consumed) == Some(&b';') {
                consumed += 1;
            }
            consumed
        };
        let validated = match u32::from_str_radix(num, radix) {
            // A FALSY (zero) code is kept literal — the `&` is later escaped.
            Ok(0) => return None,
            Ok(code) => validate_code(code),
            // OVERFLOW: a truthy out-of-range numeric code maps to NUL (the official
            // `validate_code` out-of-range arm), decoded — NOT kept literal.
            Err(_) => 0,
        };
        // `validate_code` may itself yield 0 (NUL) for a surrogate-half / out-of-range
        // in-`u32` code; `char::from_u32(0)` is the NUL char (`String.fromCodePoint(0)`).
        let ch = char::from_u32(validated).unwrap_or('\u{0}');
        return Some((ch.to_string(), bytes_consumed));
    }
    // Named reference — LONGEST match against the canonical table. The candidate
    // name is the text after `&`; try names up to `LONGEST_ENTITY_NAME` bytes,
    // longest first. A `;`-terminated form (the key includes the `;`) is preferred;
    // a legacy no-`;` form decodes only at the official attribute-value boundary
    // `\b(?!=)`: the following char must NOT be a WORD char (`[A-Za-z0-9_]`,
    // including `_`) and must NOT be `=`.
    let after = &s[1..];
    let max = super::entity_table::LONGEST_ENTITY_NAME.min(after.len());
    // Walk candidate end positions on CHAR boundaries (entity names are ASCII, but
    // the slice may sit before non-ASCII bytes).
    let mut best: Option<(u32, usize)> = None; // (code, name_byte_len)
    for end in (1..=max).rev() {
        if !after.is_char_boundary(end) {
            continue;
        }
        let name = &after[..end];
        let Some(code) = super::entity_table::lookup_named_entity(name) else {
            continue;
        };
        if name.ends_with(';') {
            best = Some((code, end));
            break; // a `;`-terminated longest match is authoritative.
        }
        // Legacy no-`;` form: in ATTRIBUTE-VALUE context it decodes only when NOT
        // followed by `=` or a word char (the official `\b(?!=)` boundary). In
        // TEXT-CONTENT context the content entity pattern has NO such boundary, so a
        // legacy no-`;` match decodes UNCONDITIONALLY (the longest legacy name wins).
        // A following `_` BLOCKS the attribute match (it is a word char); a following
        // space / punctuation / end-of-input allows it.
        let blocked = if is_attribute_value {
            let next = after[end..].chars().next();
            matches!(next, Some(c) if c == '=' || is_entity_word_char(c))
        } else {
            false
        };
        if !blocked {
            best = Some((code, end));
            break; // longest legacy match that satisfies the boundary rule.
        }
        // Otherwise keep scanning shorter candidates.
    }
    let (code, name_len) = best?;
    let ch = char::from_u32(validate_code(code))?;
    Some((ch.to_string(), 1 + name_len)) // `&` + the matched name bytes.
}

/// Validate a numeric entity code point per the official `validate_code`
/// (`svelte@5.56.3`): a line feed (`10`) becomes a space; `128..=159` is the
/// Windows-1252 remap; the high/low surrogate range and the disallowed planes map
/// to `NUL` (`0`); the valid planes pass through. Returns the validated code point
/// (which may be `0` for a disallowed input — `char::from_u32(0)` is `NUL`, the
/// official `String.fromCodePoint(0)`).
fn validate_code(code: u32) -> u32 {
    const NUL: u32 = 0;
    // The Windows-1252 remap of code points 128..=159 (the official table).
    const WINDOWS_1252: [u32; 32] = [
        8364, 129, 8218, 402, 8222, 8230, 8224, 8225, 710, 8240, 352, 8249, 338, 141, 381, 143,
        144, 8216, 8217, 8220, 8221, 8226, 8211, 8212, 732, 8482, 353, 8250, 339, 157, 382, 376,
    ];
    match code {
        10 => 32,
        c if c < 128 => c,
        c if c <= 159 => WINDOWS_1252[(c - 128) as usize],
        c if c < 55296 => c,
        c if c <= 57343 => NUL, // UTF-16 surrogate halves.
        c if c <= 65535 => c,
        c if (65536..=131071).contains(&c) => c,
        c if (131072..=196607).contains(&c) => c,
        c if (917504..=917631).contains(&c) || (917760..=917999).contains(&c) => c,
        _ => NUL,
    }
}

/// Re-escape a DECODED attribute value for the double-quoted context, matching the
/// official `escape_html(value, is_attr=true)` over the `ATTR_REGEX = /[&"<]/`
/// (`&` → `&amp;`, `"` → `&quot;`, `<` → `&lt;`).
fn escape_html_attr_context(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            _ => out.push(ch),
        }
    }
    out
}
