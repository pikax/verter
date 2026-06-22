//! The canonical ECMA-262 `Number::toString` (radix 10) for the workspace.
//!
//! A pure JS-semantics primitive: the shortest-round-trip digit sequence the
//! ECMAScript spec derives, laid out into the spec's positional / fraction /
//! exponent forms, with the equidistant even-tie-break and the `-0` / infinity
//! / `NaN` special spellings. This is the LOWEST reusable owner of the
//! conversion — the Svelte client const-fold (`build_template_chunk`'s
//! `scope.evaluate` fold) and the `verter_session` index-key / relation
//! canonicalizers both consume this single definition, never a per-surface
//! reimplementation.

/// The exact ECMA-262 `Number::toString` (radix 10) spelling of an f64.
///
/// Implements the spec layout over Rust's shortest-round-trip digit sequence
/// (`{:e}` / `Display` both emit the minimal digit string that uniquely
/// round-trips — the same digit sequence the JS spec derives). With `n` the
/// decimal exponent (the value is `0.digits × 10^n`) and `k` the digit count:
///
/// - `k <= n <= 21` — positional integer (`digits` + `n-k` zeros);
/// - `0 < n <= 21`, `n < k` — positional decimal (point inside digits);
/// - `-6 < n <= 0` — positional fraction (`0.` + `-n` zeros + digits);
/// - otherwise — exponent form `d[.rest]e±E` with `E = n-1` (`1e21` →
///   `"1e+21"`, `1e-7` → `"1e-7"`, `1e20` → `"100000000000000000000"`,
///   `1e-6` → `"0.000001"`).
///
/// Rust and the JS spec agree on the shortest digit sequence except on EXACT
/// equidistant ties, where the spec chooses the even candidate
/// ([`js_even_tie_break`]). The special cases align `-0` → `"0"`, the
/// infinities and `NaN` to their JS spellings (`"Infinity"`, `"-Infinity"`,
/// `"NaN"`).
#[must_use]
pub fn js_number_to_string(number: f64) -> String {
    if number.is_nan() {
        return "NaN".to_string();
    }
    if number.is_infinite() {
        return if number > 0.0 {
            "Infinity"
        } else {
            "-Infinity"
        }
        .to_string();
    }
    // `-0.0 == 0.0` is true, so this collapses negative zero to `"0"` like JS.
    if number == 0.0 {
        return "0".to_string();
    }
    let negative = number < 0.0;
    let magnitude = number.abs();
    // `LowerExp` emits the shortest round-trip mantissa as
    // `d[.fraction]e<exp>` with exactly one digit before the point, so
    // the digit string never carries leading or trailing zeros.
    let scientific = format!("{magnitude:e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("LowerExp always emits an exponent");
    let exponent: i64 = exponent
        .parse()
        .expect("LowerExp exponent is a decimal integer");
    let digits: String = mantissa.chars().filter(|ch| *ch != '.').collect();
    let n = exponent + 1;
    let (digits, n) = match js_even_tie_break(magnitude, &digits, n) {
        Some(broken) => broken,
        None => (digits, n),
    };
    let k = digits.len() as i64;
    let body = if k <= n && n <= 21 {
        let mut body = digits;
        body.extend(std::iter::repeat_n('0', (n - k) as usize));
        body
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let sign = if e >= 0 { "+" } else { "-" };
        let magnitude_e = e.unsigned_abs();
        if k == 1 {
            format!("{digits}e{sign}{magnitude_e}")
        } else {
            format!("{}.{}e{sign}{magnitude_e}", &digits[..1], &digits[1..])
        }
    };
    if negative {
        format!("-{body}")
    } else {
        body
    }
}

/// ECMA-262 equidistant tie-break for the shortest digit sequence.
///
/// `Number::toString` (radix 10) requires: among the shortest digit
/// strings `s` whose `s × 10^(n-k)` denotes the value, pick the one
/// CLOSEST to the value, and "if there are two such possible values of
/// s, choose the one that is even". Rust's shortest formatter resolves
/// that exact midpoint by magnitude instead, so on ties whose even
/// candidate is BELOW the value the two spellings diverge
/// (`${161647069304469.12}` is `"161647069304469.12"`, while the Rust
/// digit sequence is `…13`).
///
/// A tie with the neighbor `s' = s ∓ 1` is the exact integer identity
/// `2·value == (s + s')·10^e` (with `e = n - k`). Writing the value as
/// `m·2^p` (the f64 decomposition) and `m = m'·2^a` with `m'` odd, the
/// identity `m'·2^(a+p+1) == (2s ∓ 1)·2^e·5^e` splits into matching
/// 2-adic valuations (`a+p+1 == e`, since `2s ∓ 1` and `5^e` are odd)
/// plus an odd-part equality checked in `u128`. The even neighbor must
/// ALSO denote the value itself (the spec only ranks denoting digit
/// strings) — verified by an exact round-trip parse.
///
/// Returns the replacement `(digits, n)` when the even neighbor wins,
/// `None` when the Rust digit sequence already is the JS spelling.
fn js_even_tie_break(magnitude: f64, digits: &str, n: i64) -> Option<(String, i64)> {
    let last = digits.as_bytes()[digits.len() - 1] - b'0';
    if last.is_multiple_of(2) {
        // An even candidate never loses a spec tie: the only competitor
        // on a tie is odd.
        return None;
    }
    let s: u64 = digits.parse().ok()?;
    let e10 = n - digits.len() as i64;
    let bits = magnitude.to_bits();
    let exp_bits = ((bits >> 52) & 0x7ff) as i64;
    let frac = bits & ((1u64 << 52) - 1);
    let (m, p) = if exp_bits == 0 {
        // Subnormal: no implicit bit, fixed exponent.
        (frac, -1074i64)
    } else {
        (frac | (1u64 << 52), exp_bits - 1075)
    };
    debug_assert!(m != 0, "zero magnitude is handled before digit layout");
    let a = i64::from(m.trailing_zeros());
    // 2-adic valuations must match: `2·value = m'·2^(a+p+1)` vs
    // `(2s ∓ 1)·2^e10·5^e10` with odd `2s ∓ 1`.
    if a + p + 1 != e10 {
        return None;
    }
    let m_odd = u128::from(m >> a);
    let pow5 = |x: i64| u32::try_from(x).ok().and_then(|x| 5u128.checked_pow(x));
    let tie_with = |t: u128| {
        if e10 >= 0 {
            pow5(e10).and_then(|f| t.checked_mul(f)) == Some(m_odd)
        } else {
            pow5(-e10).and_then(|f| m_odd.checked_mul(f)) == Some(t)
        }
    };
    // `s` is odd, so BOTH neighbors are even; at most one can tie.
    let doubled = 2 * u128::from(s);
    for (neighbor, t) in [
        (s.checked_sub(1), doubled - 1),
        (s.checked_add(1), doubled + 1),
    ] {
        let Some(s_even) = neighbor else { continue };
        if s_even == 0 || !tie_with(t) {
            continue;
        }
        // Spec admissibility: the even candidate must itself denote the
        // value (exact correctly-rounded parse).
        if format!("{s_even}e{e10}").parse::<f64>().ok() != Some(magnitude) {
            continue;
        }
        let digits = s_even.to_string();
        let n = digits.len() as i64 + e10;
        return Some((digits, n));
    }
    None
}

/// The exact ECMA-262 `Number(string)` coercion (the `StringToNumber` abstract
/// operation over a `StringNumericLiteral`).
///
/// This is the inverse direction of [`js_number_to_string`] and the other half
/// of the workspace's JS-number-semantics owner: the Svelte client const-fold
/// (`scope.evaluate`'s arithmetic / `Number(...)` coercion) needs JS `Number()`,
/// NOT Rust's `str::parse::<f64>` (which rejects `0x`/`0o`/`0b` prefixes and an
/// empty string, accepts `inf`/`nan`, and differs on signs).
///
/// The grammar (<https://tc39.es/ecma262/#sec-tonumber-applied-to-the-string-type>):
///
/// - Leading / trailing `StrWhiteSpace` (ECMAScript WhiteSpace + LineTerminator)
///   is trimmed; an empty / all-whitespace string is `+0`.
/// - A `NonDecimalIntegerLiteral` — `0x`/`0X` (hex), `0o`/`0O` (octal),
///   `0b`/`0B` (binary), each WITHOUT a sign — is accumulated digit-by-digit
///   into the nearest f64 (`0xffffffffffffffff` → `18446744073709552000`).
/// - `Infinity` / `+Infinity` / `-Infinity` (exact case).
/// - A `StrDecimalLiteral` — an optional `+`/`-` sign over a strict decimal with
///   optional integer / fraction / exponent parts (`.5`, `5.`, `1.5e-2`, but NOT
///   `1e`, `+.`, `.e5`, `1_000`) — parsed by Rust's correctly-rounded `f64`
///   parser AFTER the grammar is validated.
/// - Anything else (`'a 15 b'`, `'+'`, `'0x10g'`) is `NaN`.
#[must_use]
pub fn js_string_to_number(input: &str) -> f64 {
    let s = input.trim_matches(is_js_string_whitespace);
    if s.is_empty() {
        return 0.0;
    }

    // Non-decimal integer literals take NO sign.
    if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return parse_radix_digits(rest, 16);
    }
    if let Some(rest) = s.strip_prefix("0o").or_else(|| s.strip_prefix("0O")) {
        return parse_radix_digits(rest, 8);
    }
    if let Some(rest) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        return parse_radix_digits(rest, 2);
    }

    // `Infinity` with an optional sign.
    match s {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }

    // `StrDecimalLiteral`: optional sign over a strict decimal body. Validate the
    // grammar (Rust's parser is laxer — it accepts `inf`/`nan` and rejects nothing
    // a JS decimal allows), then defer the correctly-rounded conversion to Rust.
    let (sign, body) = match s.as_bytes().first() {
        Some(b'+') => (1.0, &s[1..]),
        Some(b'-') => (-1.0, &s[1..]),
        _ => (1.0, s),
    };
    if !is_js_decimal_body(body) {
        return f64::NAN;
    }
    match body.parse::<f64>() {
        Ok(v) => sign * v,
        Err(_) => f64::NAN,
    }
}

/// Accumulate the digits of a `NonDecimalIntegerLiteral` body (no sign, no prefix)
/// into the nearest f64. An empty body or any non-`radix` digit yields `NaN`.
fn parse_radix_digits(body: &str, radix: u32) -> f64 {
    if body.is_empty() {
        return f64::NAN;
    }
    let mut acc = 0.0_f64;
    let radix_f = f64::from(radix);
    for ch in body.chars() {
        match ch.to_digit(radix) {
            Some(d) => acc = acc * radix_f + f64::from(d),
            None => return f64::NAN,
        }
    }
    acc
}

/// Whether a char is ECMAScript `StrWhiteSpace` (WhiteSpace + LineTerminator) —
/// the leading/trailing trim set of `StringToNumber`. WhiteSpace is the TAB / VT /
/// FF / SP / NBSP / ZWNBSP code points plus any Unicode `Zs` (space separator);
/// LineTerminator is LF / CR / LS / PS.
fn is_js_string_whitespace(ch: char) -> bool {
    matches!(
        ch,
        '\u{0009}'   // TAB
        | '\u{000B}' // VT
        | '\u{000C}' // FF
        | '\u{0020}' // SP
        | '\u{00A0}' // NBSP
        | '\u{FEFF}' // ZWNBSP / BOM
        | '\u{000A}' // LF
        | '\u{000D}' // CR
        | '\u{2028}' // LS
        | '\u{2029}' // PS
    ) || ch.is_whitespace() && is_unicode_space_separator(ch)
}

/// Whether a char is in Unicode general category `Zs` (Space Separator) — the
/// remaining ECMAScript WhiteSpace beyond the named ASCII / NBSP / BOM code points.
fn is_unicode_space_separator(ch: char) -> bool {
    matches!(
        ch,
        '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
    )
}

/// Whether a string is a strict ECMAScript `StrUnsignedDecimalLiteral` (the body of
/// a `StrDecimalLiteral` after the optional sign is stripped). Accepts integer,
/// fraction, and exponent forms (`5`, `.5`, `5.`, `5.5`, `1e3`, `1.5e-2`) but
/// REJECTS a bare/empty fraction or exponent (`.`, `+.`, `.e5`, `1e`, `1e+`) and
/// any non-digit junk (`1_000`, `16abc`). `Infinity` is handled by the caller.
fn is_js_decimal_body(body: &str) -> bool {
    let bytes = body.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let mut i = 0usize;
    let int_digits = count_ascii_digits(bytes, &mut i);
    let mut frac_digits = 0usize;
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        frac_digits = count_ascii_digits(bytes, &mut i);
    }
    // At least one digit must appear in the integer or fraction part (`.` alone is
    // invalid; `5.` and `.5` are valid).
    if int_digits == 0 && frac_digits == 0 {
        return false;
    }
    // Optional exponent: `e`/`E`, an optional sign, then ≥1 digit.
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        i += 1;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        if count_ascii_digits(bytes, &mut i) == 0 {
            return false;
        }
    }
    // No trailing junk.
    i == bytes.len()
}

/// Advance `i` over a maximal run of ASCII digits, returning how many were consumed.
fn count_ascii_digits(bytes: &[u8], i: &mut usize) -> usize {
    let start = *i;
    while *i < bytes.len() && bytes[*i].is_ascii_digit() {
        *i += 1;
    }
    *i - start
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_render_without_a_point() {
        assert_eq!(js_number_to_string(0.0), "0");
        assert_eq!(js_number_to_string(-0.0), "0");
        assert_eq!(js_number_to_string(6.0), "6");
        assert_eq!(js_number_to_string(-1.0), "-1");
        assert_eq!(js_number_to_string(42.0), "42");
        assert_eq!(
            js_number_to_string(9_007_199_254_740_991.0),
            "9007199254740991"
        );
    }

    #[test]
    fn fractions_and_exponents_match_the_spec_layout() {
        assert_eq!(js_number_to_string(0.25), "0.25");
        assert_eq!(js_number_to_string(1e21), "1e+21");
        assert_eq!(js_number_to_string(1e-7), "1e-7");
        assert_eq!(js_number_to_string(1e20), "100000000000000000000");
        assert_eq!(js_number_to_string(1e-6), "0.000001");
    }

    #[test]
    fn special_values_take_their_js_spellings() {
        assert_eq!(js_number_to_string(f64::NAN), "NaN");
        assert_eq!(js_number_to_string(f64::INFINITY), "Infinity");
        assert_eq!(js_number_to_string(f64::NEG_INFINITY), "-Infinity");
    }

    #[test]
    fn even_tie_break_picks_the_spec_digits() {
        // The midpoint `161647069304469.12` whose Rust shortest-digit
        // sequence ends `…13` but whose spec spelling is the even `…12`.
        assert_eq!(
            js_number_to_string(161_647_069_304_469.12),
            "161647069304469.12"
        );
    }

    // ── JS `Number(string)` coercion (`StringToNumber`) ──
    //
    // Every expected value below is the exact result of the JS `Number(...)`
    // global over the same string (verified against the engine), the semantics
    // official `scope.evaluate` applies when coercing a string operand to a
    // number. The Rust `str::parse::<f64>` this replaces diverges on prefixes,
    // the empty string, and `inf`/`nan` spellings.

    /// Assert `js_string_to_number(s)` equals `expected`, with NaN compared by
    /// `is_nan` (NaN != NaN) and `-0`/`+0` distinguished by sign bit.
    fn assert_num(s: &str, expected: f64) {
        let got = js_string_to_number(s);
        if expected.is_nan() {
            assert!(got.is_nan(), "Number({s:?}) expected NaN, got {got}");
        } else {
            assert_eq!(got, expected, "Number({s:?})");
            assert_eq!(
                got.is_sign_negative(),
                expected.is_sign_negative(),
                "Number({s:?}) sign (−0 vs +0)"
            );
        }
    }

    #[test]
    fn non_decimal_integer_prefixes() {
        assert_num("0x10", 16.0);
        assert_num("0X10", 16.0);
        assert_num("0o17", 15.0);
        assert_num("0O17", 15.0);
        assert_num("0b101", 5.0);
        assert_num("0B101", 5.0);
        // Large hex overflows into the nearest f64, exactly like JS.
        assert_num("0xffffffffffffffff", 18_446_744_073_709_552_000.0);
        // A sign before a non-decimal prefix is INVALID → NaN.
        assert_num("+0x10", f64::NAN);
        assert_num("-0x10", f64::NAN);
        // Trailing junk / invalid digit → NaN.
        assert_num("0x10g", f64::NAN);
        assert_num("0b102", f64::NAN);
        // An empty body after the prefix → NaN.
        assert_num("0x", f64::NAN);
    }

    #[test]
    fn empty_and_whitespace_are_zero() {
        assert_num("", 0.0);
        assert_num("   ", 0.0);
        // Tab / VT / FF / CR / LF trim to empty → 0.
        assert_num("\t\n\r", 0.0);
        // NBSP, BOM, em-space (Zs) are whitespace too.
        assert_num("\u{00A0}\u{FEFF}\u{2003}", 0.0);
        // Surrounding whitespace is trimmed.
        assert_num(" 15 ", 15.0);
        assert_num("\u{000C}\u{000B} 5  ", 5.0);
    }

    #[test]
    fn infinity_spellings() {
        assert_num("Infinity", f64::INFINITY);
        assert_num("+Infinity", f64::INFINITY);
        assert_num("-Infinity", f64::NEG_INFINITY);
        // Wrong case is NaN (NOT Rust's `inf`).
        assert_num("infinity", f64::NAN);
        assert_num("INFINITY", f64::NAN);
        assert_num("inf", f64::NAN);
        assert_num("nan", f64::NAN);
        assert_num("NaN", f64::NAN);
    }

    #[test]
    fn decimal_forms_and_signs() {
        assert_num("15", 15.0);
        assert_num("+5", 5.0);
        assert_num("-5", -5.0);
        assert_num("+1.5e3", 1500.0);
        assert_num(".5", 0.5);
        assert_num("5.", 5.0);
        assert_num("5.5", 5.5);
        assert_num("1e3", 1000.0);
        assert_num("1E3", 1000.0);
        assert_num(".5e1", 5.0);
        assert_num("007", 7.0);
        // `-0` keeps its negative sign.
        assert_num("-0", -0.0);
        assert_num("0", 0.0);
        // Overflow / underflow match JS.
        assert_num("1e309", f64::INFINITY);
        assert_num("1e-400", 0.0);
    }

    #[test]
    fn invalid_decimals_are_nan() {
        assert_num("a 15 b", f64::NAN);
        assert_num("15abc", f64::NAN);
        assert_num("+", f64::NAN);
        assert_num("-", f64::NAN);
        assert_num(".", f64::NAN);
        assert_num("+.", f64::NAN);
        assert_num(".e5", f64::NAN);
        assert_num("1e", f64::NAN);
        assert_num("1e+", f64::NAN);
        // Underscores are not allowed in `Number(...)`.
        assert_num("1_000", f64::NAN);
    }
}
