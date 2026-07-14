//! HTML entity decode + attribute-value re-escape for the static skeleton.
//!
//! A faithful port of `svelte@5.56.3`'s `decode_character_references` +
//! `validate_code` (`phases/1-parse/utils/html.js`) plus the double-quoted
//! attribute-value re-escape (`escape_html(value, is_attr)`). The parser-stored
//! RAW attribute span is DECODED ONCE at the attribute-IR producer boundary
//! (named longest-match against the vendored HTML5 table + numeric refs with an
//! OPTIONAL trailing `;`, the legacy no-`;` boundary treating `_` as a word
//! char) into the opaque [`DecodedAttrValue`]; skeleton serializers RE-ESCAPE
//! the decoded value for the `[&"<]` context via [`escape_decoded_attr`]
//! (escape-only — never a second decode).

/// A static attribute value DECODED at the attribute-IR producer boundary — the
/// single semantic value the CSS scope matcher and every client emitter consume,
/// so the two can never disagree on what the attribute "means" (`class="a&#32;b"`
/// is the word list `a b` for BOTH the `.b` selector match and the serialized
/// skeleton). Constructed ONLY via [`DecodedAttrValue::decode`] (decode once, at
/// construction); consumers read the decoded text via [`as_str`](Self::as_str)
/// and re-escape via [`escape_decoded_attr`] — NEVER a second decode (an
/// already-decoded `&amp;` re-decoding to `&` is the double-decode bug this
/// newtype exists to prevent).
///
/// The struct is declared `pub` inside this PRIVATE module so the `pub` field
/// `StaticAttrValue::value` may carry it (the interface-visibility rule); it
/// stays unnameable — and its accessors uncallable — outside the runtime
/// module, keeping the newtype opaque at the crate surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedAttrValue(String);

impl DecodedAttrValue {
    /// Decode a RAW parser attribute span into its semantic value — the official
    /// `decode_character_references(raw, true)` run ONCE, at IR construction.
    ///
    /// The SAME single pass that produces the semantic value reports each
    /// decoded reference's spelled [`EntityRefForm`] to `observe` as it
    /// consumes it — the producer-boundary provenance hook: one scan yields
    /// both the decoded value and the lexical form facts, so no consumer
    /// ever re-scans the raw bytes. A caller with no provenance consumer
    /// passes a no-op closure, which monomorphizes to the exact
    /// observer-free loop.
    #[must_use]
    pub(super) fn decode(raw: &str, observe: &mut impl FnMut(EntityRefForm)) -> Self {
        Self(decode_entities_observing(
            raw, /* is_attribute_value */ true, observe,
        ))
    }

    /// The decoded attribute text (already-decoded; never decode it again).
    #[must_use]
    pub(super) fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the decoded value is the empty string (the `class=""` /
    /// `disabled=""` present-but-empty distinction the emitters test).
    #[must_use]
    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// ESCAPE-ONLY serialization of an already-decoded attribute value for the
/// double-quoted static skeleton, matching the official
/// `escape_html(decode_character_references(raw, true), /*is_attr*/ true)`
/// pipeline with the DECODE half owned by the producer boundary
/// ([`DecodedAttrValue::decode`]) and only the re-escape (`[&"<]` → `&amp;` /
/// `&quot;` / `&lt;`) applied here. Escaping performs NO decode — a second
/// decode over an already-decoded value would double-decode (`&amp;amp;` raw →
/// decoded `&amp;` → wrongly `&`).
#[must_use]
pub(super) fn escape_decoded_attr(v: &DecodedAttrValue) -> String {
    escape_html_attr_context(v.as_str())
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
/// This is the DECODE-ONLY step (NO re-escaping). It shares the single
/// decode core with [`DecodedAttrValue::decode`] (the attribute-IR producer
/// boundary; skeleton serializers later re-escape via
/// [`escape_decoded_attr`]) and is called
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

/// The spelled FORM of ONE decoded entity reference — the lexical fact the
/// single decode pass reports to its observer as it consumes the reference
/// (an UNDECODABLE `&…` reports nothing; it stays literal text). Numeric
/// forms split by radix prefix: `&#32`/`&#32;` is [`Decimal`](Self::Decimal),
/// `&#x20`/`&#x20;` (lowercase-`x` prefix only) is [`Hex`](Self::Hex).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EntityRefForm {
    /// A named reference (`&amp;` / the legacy no-`;` `&amp`).
    Named,
    /// A decimal numeric reference (`&#32` / `&#32;`).
    Decimal,
    /// A hex numeric reference (`&#x20` / `&#x20;`).
    Hex,
}

/// The shared decode core for both the attribute-value and text-content contexts:
/// scan for `&`, decode each reference (named longest-match or numeric) per the
/// official `decode_character_references`, keep an undecodable `&` literal. The
/// `is_attribute_value` flag selects the legacy no-`;` named-reference boundary
/// rule (the attribute-value `\b(?!=)` restriction vs the unconditional content
/// form).
fn decode_entities(value: &str, is_attribute_value: bool) -> String {
    decode_entities_observing(value, is_attribute_value, &mut |_| {})
}

/// [`decode_entities`] with an OBSERVER reporting each decoded reference's
/// spelled [`EntityRefForm`] — the SINGLE pass emits the lexical form facts
/// while it produces the decoded value, so provenance consumers never run a
/// second scan. The no-op-observer instantiation IS the plain decode.
fn decode_entities_observing(
    value: &str,
    is_attribute_value: bool,
    observe: &mut impl FnMut(EntityRefForm),
) -> String {
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
        if let Some((decoded, consumed, form)) = decode_one_entity(&value[i..], is_attribute_value)
        {
            observe(form);
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
/// attribute-value context). Returns `(decoded_string, bytes_consumed,
/// spelled_form)` on success, or `None` when `s` does not begin a decodable
/// reference (the `&` is then kept literal). The form is a byproduct of the
/// arm the decode already takes — reporting it costs no extra scan.
fn decode_one_entity(s: &str, is_attribute_value: bool) -> Option<(String, usize, EntityRefForm)> {
    debug_assert_eq!(s.as_bytes().first(), Some(&b'&'));
    // Numeric reference: `&#65` / `&#65;` (decimal) / `&#x41` / `&#x41;` (hex). The
    // official pattern `#(?:x[a-fA-F\d]+|\d+)(?:;)?` makes the trailing `;`
    // OPTIONAL — the numeric run ends at the first non-matching char (or `;`).
    if s.as_bytes().get(1) == Some(&b'#') {
        // Determine the radix + the digit run length after `&#` (and an optional
        // LOWERCASE `x` — the official pattern `#(?:x[a-fA-F\d]+|\d+)(?:;)?`
        // accepts a lowercase hex PREFIX only; an uppercase `X` falls through
        // to the decimal arm, matches no digits, and the reference is kept
        // literal — `&#X41;` never decodes. Hex DIGITS keep both cases.). The
        // run ends at the first non-digit; a trailing `;` (if present) is
        // consumed as part of the reference.
        let after_hash = &s[2..];
        let (radix, digits_off) = match after_hash.as_bytes().first() {
            Some(b'x') => (16u32, 1usize),
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
        let form = if radix == 16 {
            EntityRefForm::Hex
        } else {
            EntityRefForm::Decimal
        };
        return Some((ch.to_string(), bytes_consumed, form));
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
    // `&` + the matched name bytes.
    Some((ch.to_string(), 1 + name_len, EntityRefForm::Named))
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
/// (`&` → `&amp;`, `"` → `&quot;`, `<` → `&lt;`). Shared by [`escape_decoded_attr`]
/// (the `DecodedAttrValue`-typed skeleton path) and the `collect_static_attrs`
/// resolved-value serializer (whose value is the already-decoded `as_str`).
pub(super) fn escape_html_attr_context(value: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The single decode pass reports each DECODED reference's spelled form,
    /// in encounter order, while producing the decoded value — one scan, two
    /// outputs. An undecodable reference reports nothing (it stays literal).
    #[test]
    fn observed_decode_reports_each_decoded_reference_form_in_one_pass() {
        let mut forms: Vec<EntityRefForm> = Vec::new();
        let decoded =
            DecodedAttrValue::decode("a&amp;&#65;&#x41;b&bogus;", &mut |form| forms.push(form));
        assert_eq!(decoded.as_str(), "a&AAb&bogus;");
        assert_eq!(
            forms,
            vec![
                EntityRefForm::Named,
                EntityRefForm::Decimal,
                EntityRefForm::Hex
            ],
            "one report per DECODED reference, encounter order; `&bogus;` \
             stays literal and reports nothing"
        );
    }

    /// The uppercase-`X` numeric prefix never decodes (official pattern) —
    /// and therefore never reports a form; the legacy no-`;` named form
    /// reports `Named` exactly when the attribute boundary rule lets it
    /// decode.
    #[test]
    fn observer_mirrors_the_decode_verdict_exactly() {
        let mut forms: Vec<EntityRefForm> = Vec::new();
        let decoded = DecodedAttrValue::decode("a&#X41;b", &mut |form| forms.push(form));
        assert_eq!(decoded.as_str(), "a&#X41;b", "uppercase X is not an entity");
        assert!(forms.is_empty(), "no decode, no report");

        forms.clear();
        // `&amp` (no `;`) followed by a non-word char DECODES in attribute
        // context — and reports Named.
        let decoded = DecodedAttrValue::decode("a&amp b", &mut |form| forms.push(form));
        assert_eq!(decoded.as_str(), "a& b");
        assert_eq!(forms, vec![EntityRefForm::Named]);

        forms.clear();
        // `&amp_` is BLOCKED by the word-char boundary — no decode, no report.
        let decoded = DecodedAttrValue::decode("a&amp_b", &mut |form| forms.push(form));
        assert_eq!(decoded.as_str(), "a&amp_b");
        assert!(forms.is_empty());
    }

    /// The observer never perturbs the decode: the no-op-observer
    /// instantiation and an accumulating observer produce byte-identical
    /// decoded values (the observer is REPORT-ONLY on the single pass).
    #[test]
    fn observer_never_perturbs_the_decoded_value() {
        for raw in ["a b", "a&#32;b", "a&#x20;b", "a&amp;&bogus;b", "&#0;"] {
            let mut forms: Vec<EntityRefForm> = Vec::new();
            assert_eq!(
                DecodedAttrValue::decode(raw, &mut |_| {}),
                DecodedAttrValue::decode(raw, &mut |form| forms.push(form)),
                "raw: {raw:?}"
            );
        }
    }

    /// COMPILE-TIME arity inventory: `DecodedAttrValue` is exactly ONE
    /// decoded `String` — the tuple pattern below fails to compile if any
    /// field (a ZST included) is added. Only this module can destructure the
    /// private field, so the proof lives here.
    #[test]
    fn decoded_attr_value_is_exactly_one_decoded_string() {
        let v = DecodedAttrValue::decode("x", &mut |_| {});
        let DecodedAttrValue(inner) = &v;
        assert_eq!(inner, "x");
    }
}
