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

pub use verter_type_expr::CanonicalIndexInt;

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
/// `IndexKey::Computed`, where the walker's G4.5 recovery re-derives the
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
    CanonicalIndexInt::from_js_number(number)
}

/// JS `Number`→string for a finite-or-special `f64` literal — the canonical
/// numeric string TS produces when interpolating a numeric literal into a
/// template-literal type AND when a numeric-literal key publishes as a
/// property NAME (pinned tsgo, probe10: `Pick<any, 1>` = `{ 1: any }` ≡
/// `{ "1": any }`, `Pick<any, 1.5>` = `{ "1.5": any }`). The key-domain
/// enumeration (`key_literals_from_keyspace_node`) and the non-emitting
/// membership predicate share this single canonicalizer.
///
/// The exact ECMA-262 `Number::toString` (radix 10) layout — including the
/// even-tie-break (pinned tsgo, probe14) and the `1e21` → `"1e+21"` /
/// `1e-7` → `"1e-7"` exponent forms (probe13) — lives in the workspace-canonical
/// [`verter_compiler::js_number`] (the lowest reusable owner, shared with the
/// Svelte client const-fold). Re-exported here because this module OWNS the
/// `IndexKey::Number` payload that the spelling canonicalizes.
pub(crate) use verter_compiler::js_number::js_number_to_string;

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
