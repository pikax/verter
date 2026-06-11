//! Canonical integer index-key domain — the owning module for the
//! [`crate::semantic_query::IndexKey::Number`] payload.
//!
//! [`CanonicalIndexInt`] is a PROOF-CARRYING newtype: its field is
//! private, so outside this module the ONLY ways to obtain one are the
//! two blessed constructors — [`integer_convention_index_key`] (the
//! f64-checked fold every numeric-literal producer routes through) and
//! [`CanonicalIndexInt::from_canonical_i64`] (the `Display`-checked
//! path for genuinely-integer producers, e.g. fixture builders). A raw
//! `IndexKey::Number(n as i64)` is therefore a COMPILE ERROR anywhere
//! else in the workspace — the bounded-convention invariant the
//! retired G4.4 textual classifier approximated is enforced at the
//! language level. Pattern-matching is unrestricted: destructuring
//! binds the already-proven value, and re-constructing
//! `IndexKey::Number` from a bound `CanonicalIndexInt` is an identity
//! copy of admitted proof.
//!
//! The admitted set: exactly the `i64` values whose `Display` IS the
//! canonical [`js_number_to_string`] spelling of their numeric value —
//! the invariant consumers rely on when they render member-name
//! needles with `to_string()` and when the walker's `Index(Number)`
//! recovery raises the value back to an `f64` literal.

use std::fmt;

/// An `i64` index key admitted under the integer convention: its
/// `Display` is the canonical JS spelling of the number it was folded
/// from. Construction is gated by this module (private field); see the
/// module docs for the two blessed constructors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CanonicalIndexInt(i64);

impl CanonicalIndexInt {
    /// `Display`-checked constructor for genuinely-integer producers
    /// (fixture builders, tests). Admits `value` iff its base-10
    /// digits ARE the canonical JS spelling of its numeric value —
    /// the same admission condition as the f64 fold, entered from the
    /// integer side: `value as f64` must round-trip to `value` AND
    /// spell identically. Every `|value| <= 2^53` passes; the sparse
    /// admissible band above passes exactly when sound.
    pub fn from_canonical_i64(value: i64) -> Option<Self> {
        integer_convention_index_key(value as f64).filter(|key| key.0 == value)
    }

    /// The admitted integer. Consumers needing the raw value
    /// (tuple-position folds, `f64` recovery casts) read it here;
    /// there is no way back into a `CanonicalIndexInt` without
    /// re-entering a blessed constructor.
    pub fn get(self) -> i64 {
        self.0
    }
}

/// Sound by construction: the admitted invariant is precisely that the
/// inner `i64`'s base-10 digits ARE the canonical `js_number_to_string`
/// spelling, so delegating to the integer `Display` renders the
/// canonical member-name needle.
impl fmt::Display for CanonicalIndexInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Producer bound for the [`crate::semantic_query::IndexKey::Number`]
/// integer convention — the SINGLE admission predicate every
/// numeric-literal producer (source lowering at
/// `lower::shallow_lower_type_expr`, node normalisation at
/// `evaluate::normalized_index_key_node`, and through it generic
/// substitution at `substitute::substitute_index_key_with_change_tracking`)
/// routes through.
///
/// A numeric literal folds to a [`CanonicalIndexInt`] ONLY when `i64`'s
/// `Display` of the candidate IS the canonical [`js_number_to_string`]
/// spelling of the literal — the exact invariant consumers rely on when
/// they render the needle with `to_string()`. Everything else
/// (non-integral values, `NaN`/infinities, and integral values whose
/// shortest-round-trip spelling diverges from their exact digits — e.g.
/// the f64 `4611686018427387904` (2^62) spells `"4611686018427388000"`,
/// and `9223372036854775808` (2^63) both spells `"9223372036854776000"`
/// AND saturates the f64→i64 cast to a DIFFERENT integer) stays
/// `IndexKey::TypeNode`, where the walker's G4.5 recovery re-derives the
/// canonical needle from the literal node. Every integral `|v| <= 2^53`
/// passes (the f64 is the exact integer and its shortest round-trip is
/// that integer), so the fast path keeps covering the entire safe
/// domain; a sparse band above 2^53 whose exact digits ARE the shortest
/// spelling passes too — harmlessly, because the equality is the
/// soundness condition itself.
///
/// Recovery is VALUE- and CANONICAL-NAME-exact (not bit-for-bit): a
/// folded `i64` came from an f64 whose numeric value it equals, so the
/// symmetric `as f64` raise (`raise::raise_index_key_to_type_expr`,
/// the walker's `Index(Number)` arm) reproduces an f64 that compares
/// equal to the original literal and spells the same canonical
/// `js_number_to_string` name. The one admitted value with two bit
/// patterns is zero: `-0.0` spells `"0"` (JS collapses negative zero),
/// folds to the canonical `0`, and recovers as `+0.0` — a
/// different bit pattern, the same value (`-0.0 == 0.0`) and the same
/// canonical name `"0"`, which is exactly the identity consumers rely
/// on.
pub(crate) fn integer_convention_index_key(number: f64) -> Option<CanonicalIndexInt> {
    // Saturating cast: out-of-range and NaN candidates produce an i64
    // whose Display cannot equal the literal's canonical spelling, so
    // the equality check below rejects them without a range pre-filter.
    let candidate = number as i64;
    (js_number_to_string(number) == candidate.to_string()).then_some(CanonicalIndexInt(candidate))
}

/// JS `Number`→string for a finite-or-special `f64` literal — the canonical
/// numeric string TS produces when interpolating a numeric literal into a
/// template-literal type AND when a numeric-literal key publishes as a
/// property NAME (pinned tsgo, probe10: `Pick<any, 1>` = `{ 1: any }` ≡
/// `{ "1": any }`, `Pick<any, 1.5>` = `{ "1.5": any }`). The key-domain
/// enumeration (`key_literals_from_keyspace_node`) and the non-emitting
/// membership predicate share this single canonicalizer.
///
/// Implements the exact ECMA-262 `Number::toString` (radix 10) layout
/// over Rust's shortest-round-trip digit sequence (`{:e}` / `Display`
/// both emit the minimal digit string that uniquely round-trips — the
/// same digit sequence the JS spec derives). With `n` the decimal
/// exponent (the value is `0.digits × 10^n`) and `k` the digit count:
///
/// - `k <= n <= 21` — positional integer (`digits` + `n-k` zeros);
/// - `0 < n <= 21`, `n < k` — positional decimal (point inside digits);
/// - `-6 < n <= 0` — positional fraction (`0.` + `-n` zeros + digits);
/// - otherwise — exponent form `d[.rest]e±E` with `E = n-1` (pinned
///   tsgo, probe13: `1e21` → `"1e+21"`, `1e-7` → `"1e-7"`,
///   `1e20` → `"100000000000000000000"`, `1e-6` → `"0.000001"`).
///
/// Rust and the JS spec agree on the shortest digit sequence except on
/// EXACT equidistant ties, where the spec chooses the even candidate
/// ([`js_even_tie_break`]; pinned tsgo, probe14). The special cases
/// align `-0` → `"0"`, the infinities and `NaN` to their JS spellings.
pub(crate) fn js_number_to_string(number: f64) -> String {
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
/// candidate is BELOW the value the two spellings diverge (pinned tsgo,
/// probe14: `${161647069304469.12}` is `"161647069304469.12"`, while
/// the Rust digit sequence is `…13`).
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `from_canonical_i64` admits exactly the integers whose digits
    /// are their canonical JS spelling — the same set the f64 fold
    /// admits, entered from the integer side.
    #[test]
    fn from_canonical_i64_is_display_checked() {
        for value in [
            0i64,
            1,
            -1,
            7,
            42,
            -9_007_199_254_740_991,
            9_007_199_254_740_991,
        ] {
            let key = CanonicalIndexInt::from_canonical_i64(value)
                .unwrap_or_else(|| panic!("{value} is canonical and must be admitted"));
            assert_eq!(key.get(), value);
            assert_eq!(key.to_string(), value.to_string(), "Display IS the digits");
        }
        // 2^62: shortest round-trip spelling is `4611686018427388000`,
        // not the exact digits — rejected (the G4.5 TypeNode route owns
        // it).
        assert_eq!(
            CanonicalIndexInt::from_canonical_i64(4_611_686_018_427_387_904),
            None,
            "2^62's digits are not its canonical JS spelling"
        );
        // i64::MIN: -2^63 is exactly representable in f64, but its
        // canonical spelling is the shortest-round-trip
        // `-9223372036854776000` (16 significant digits), not the
        // exact digits — rejected (probe18 class).
        assert_eq!(CanonicalIndexInt::from_canonical_i64(i64::MIN), None);
        // i64::MAX: 2^63 - 1 is not representable; `i64::MAX as f64`
        // rounds to 2^63 whose saturating fold-back diverges — rejected.
        assert_eq!(CanonicalIndexInt::from_canonical_i64(i64::MAX), None);
    }

    /// The two constructors admit the same proof: an f64-folded key and
    /// the integer-side constructor of its value are the same key.
    #[test]
    fn constructors_agree_on_the_shared_domain() {
        for value in [-3.0f64, 0.0, -0.0, 1.0, 21.0, 9_007_199_254_740_991.0] {
            let folded = integer_convention_index_key(value)
                .unwrap_or_else(|| panic!("{value} folds under the integer convention"));
            assert_eq!(
                CanonicalIndexInt::from_canonical_i64(folded.get()),
                Some(folded)
            );
        }
        assert_eq!(
            integer_convention_index_key(1.5),
            None,
            "non-integral literals stay TypeNode"
        );
    }
}
