//! HTML entity decoding for the parser.
//!
//! Decodes named, numeric, and hex HTML entities found in template text nodes.
//! Used during parsing to produce decoded text content.

/// Decode a single HTML entity at the start of `s` (which must begin with `&`).
/// Returns the decoded char and the byte length consumed (including `&` and `;`).
fn decode_html_entity(s: &str) -> Option<(char, usize)> {
    if !s.starts_with('&') {
        return None;
    }
    let semi = s[1..].find(';')?;
    if semi > 32 {
        return None;
    }
    let entity = &s[1..semi + 1];
    let ch = match entity {
        // XML predefined entities
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        // Common HTML entities
        "nbsp" => '\u{00A0}',
        "copy" => '\u{00A9}',
        "reg" => '\u{00AE}',
        "trade" => '\u{2122}',
        "mdash" => '\u{2014}',
        "ndash" => '\u{2013}',
        "hellip" => '\u{2026}',
        "laquo" => '\u{00AB}',
        "raquo" => '\u{00BB}',
        "bull" => '\u{2022}',
        "middot" => '\u{00B7}',
        "iexcl" => '\u{00A1}',
        "iquest" => '\u{00BF}',
        "cent" => '\u{00A2}',
        "pound" => '\u{00A3}',
        "yen" => '\u{00A5}',
        "euro" => '\u{20AC}',
        "curren" => '\u{00A4}',
        "sect" => '\u{00A7}',
        "para" => '\u{00B6}',
        "deg" => '\u{00B0}',
        "plusmn" => '\u{00B1}',
        "micro" => '\u{00B5}',
        "times" => '\u{00D7}',
        "divide" => '\u{00F7}',
        "frac14" => '\u{00BC}',
        "frac12" => '\u{00BD}',
        "frac34" => '\u{00BE}',
        "sup1" => '\u{00B9}',
        "sup2" => '\u{00B2}',
        "sup3" => '\u{00B3}',
        // Typographic
        "lsquo" => '\u{2018}',
        "rsquo" => '\u{2019}',
        "ldquo" => '\u{201C}',
        "rdquo" => '\u{201D}',
        "sbquo" => '\u{201A}',
        "bdquo" => '\u{201E}',
        "dagger" => '\u{2020}',
        "Dagger" => '\u{2021}',
        "permil" => '\u{2030}',
        "prime" => '\u{2032}',
        "Prime" => '\u{2033}',
        "lsaquo" => '\u{2039}',
        "rsaquo" => '\u{203A}',
        "oline" => '\u{203E}',
        // Arrows
        "larr" => '\u{2190}',
        "uarr" => '\u{2191}',
        "rarr" => '\u{2192}',
        "darr" => '\u{2193}',
        "harr" => '\u{2194}',
        // Math
        "fnof" => '\u{0192}',
        "infin" => '\u{221E}',
        "radic" => '\u{221A}',
        "sum" => '\u{2211}',
        "prod" => '\u{220F}',
        "minus" => '\u{2212}',
        "lowast" => '\u{2217}',
        "sim" => '\u{223C}',
        "asymp" => '\u{2248}',
        "ne" => '\u{2260}',
        "equiv" => '\u{2261}',
        "le" => '\u{2264}',
        "ge" => '\u{2265}',
        "sub" => '\u{2282}',
        "sup" => '\u{2283}',
        "nsub" => '\u{2284}',
        "sube" => '\u{2286}',
        "supe" => '\u{2287}',
        "oplus" => '\u{2295}',
        "otimes" => '\u{2297}',
        "perp" => '\u{22A5}',
        // Spacing / formatting
        "ensp" => '\u{2002}',
        "emsp" => '\u{2003}',
        "thinsp" => '\u{2009}',
        "zwnj" => '\u{200C}',
        "zwj" => '\u{200D}',
        "lrm" => '\u{200E}',
        "rlm" => '\u{200F}',
        // Latin extended
        "Agrave" => '\u{00C0}',
        "Aacute" => '\u{00C1}',
        "Acirc" => '\u{00C2}',
        "Atilde" => '\u{00C3}',
        "Auml" => '\u{00C4}',
        "Aring" => '\u{00C5}',
        "AElig" => '\u{00C6}',
        "Ccedil" => '\u{00C7}',
        "Egrave" => '\u{00C8}',
        "Eacute" => '\u{00C9}',
        "Ecirc" => '\u{00CA}',
        "Euml" => '\u{00CB}',
        "Igrave" => '\u{00CC}',
        "Iacute" => '\u{00CD}',
        "Icirc" => '\u{00CE}',
        "Iuml" => '\u{00CF}',
        "ETH" => '\u{00D0}',
        "Ntilde" => '\u{00D1}',
        "Ograve" => '\u{00D2}',
        "Oacute" => '\u{00D3}',
        "Ocirc" => '\u{00D4}',
        "Otilde" => '\u{00D5}',
        "Ouml" => '\u{00D6}',
        "Oslash" => '\u{00D8}',
        "Ugrave" => '\u{00D9}',
        "Uacute" => '\u{00DA}',
        "Ucirc" => '\u{00DB}',
        "Uuml" => '\u{00DC}',
        "Yacute" => '\u{00DD}',
        "THORN" => '\u{00DE}',
        "szlig" => '\u{00DF}',
        "agrave" => '\u{00E0}',
        "aacute" => '\u{00E1}',
        "acirc" => '\u{00E2}',
        "atilde" => '\u{00E3}',
        "auml" => '\u{00E4}',
        "aring" => '\u{00E5}',
        "aelig" => '\u{00E6}',
        "ccedil" => '\u{00E7}',
        "egrave" => '\u{00E8}',
        "eacute" => '\u{00E9}',
        "ecirc" => '\u{00EA}',
        "euml" => '\u{00EB}',
        "igrave" => '\u{00EC}',
        "iacute" => '\u{00ED}',
        "icirc" => '\u{00EE}',
        "iuml" => '\u{00EF}',
        "eth" => '\u{00F0}',
        "ntilde" => '\u{00F1}',
        "ograve" => '\u{00F2}',
        "oacute" => '\u{00F3}',
        "ocirc" => '\u{00F4}',
        "otilde" => '\u{00F5}',
        "ouml" => '\u{00F6}',
        "oslash" => '\u{00F8}',
        "ugrave" => '\u{00F9}',
        "uacute" => '\u{00FA}',
        "ucirc" => '\u{00FB}',
        "uuml" => '\u{00FC}',
        "yacute" => '\u{00FD}',
        "thorn" => '\u{00FE}',
        "yuml" => '\u{00FF}',
        // Numeric/hex references
        _ if entity.starts_with('#') => {
            let num = &entity[1..];
            let code_point = if num.starts_with('x') || num.starts_with('X') {
                u32::from_str_radix(&num[1..], 16).ok()?
            } else {
                num.parse::<u32>().ok()?
            };
            char::from_u32(code_point)?
        }
        _ => return None,
    };
    Some((ch, semi + 2))
}

/// Decode HTML entities in `s` and append the result to `buf`.
/// Handles `&quot;` → `"`, `&amp;` → `&`, `&lt;` → `<`, `&gt;` → `>`,
/// `&apos;` → `'`, `&nbsp;` → U+00A0, and numeric/hex references.
pub fn decode_html_entities_into(buf: &mut String, s: &str) {
    // Fast path: no ampersands means no entities
    if !s.contains('&') {
        buf.push_str(s);
        return;
    }

    let bytes = s.as_bytes();
    let mut copy_from = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            if let Some((decoded, entity_len)) = decode_html_entity(&s[i..]) {
                // Flush unmodified region
                if copy_from < i {
                    buf.push_str(&s[copy_from..i]);
                }
                buf.push(decoded);
                i += entity_len;
                copy_from = i;
                continue;
            }
        }
        i += 1;
    }
    // Flush remaining
    if copy_from < bytes.len() {
        buf.push_str(&s[copy_from..]);
    }
}

/// Returns true if the string contains any HTML entity (`&...;`).
pub fn has_html_entities(s: &str) -> bool {
    s.contains('&')
}
