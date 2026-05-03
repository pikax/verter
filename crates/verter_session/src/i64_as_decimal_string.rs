//! Serde helper — serializes `i64` as a signed decimal string (via
//! `Display`), deserializes by parsing a string (via `FromStr`).
//!
//! Mirrors [`crate::u64_as_decimal_string`] but for the signed-64
//! axis. Architecture rule: every `u64` and every `i64` audit field
//! serializes as a decimal string (`#[serde(with = "...")]`) and is
//! typed in TS as `string` (`#[ts(type = "string")]`). No
//! magnitude-based exceptions; a 'small enough' `i64` is still `i64`,
//! and uniformity > locally-clever encoding.
//!
//! Rationale identical to the `u64` module: JavaScript `Number` loses
//! precision above 2^53 (in either direction for signed values); and
//! `bigint` is not standards-compliant JSON (`JSON.parse` cannot
//! produce it, `JSON.stringify` cannot emit it). A uniform
//! string-transport for every integer field > 32 bits (signed or
//! unsigned) lets TS consumers round-trip audit JSON through
//! `JSON.parse` / `JSON.stringify` with zero loss.

use std::fmt;
use std::str::FromStr;

use serde::de::{self, Visitor};
use serde::{Deserializer, Serializer};

/// Serialize an `i64` as its signed-decimal string representation.
/// Negative values carry a leading `-`; zero and positive values are
/// emitted verbatim (no forced sign).
pub(crate) fn serialize<S: Serializer>(v: &i64, ser: S) -> Result<S::Ok, S::Error> {
    ser.collect_str(v)
}

struct I64DecimalStringVisitor;

impl<'de> Visitor<'de> for I64DecimalStringVisitor {
    type Value = i64;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a decimal i64 encoded as a JSON string")
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<i64, E> {
        i64::from_str(v)
            .map_err(|e| E::custom(format_args!("invalid i64 decimal string `{v}`: {e}")))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<i64, E> {
        self.visit_str(&v)
    }
}

/// Deserialize an `i64` from a decimal string. Rejects non-decimal,
/// overflowing, and trailing-garbage inputs with a serde error.
pub(crate) fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<i64, D::Error> {
    de.deserialize_str(I64DecimalStringVisitor)
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Wrap {
        #[serde(with = "super")]
        v: i64,
    }

    #[test]
    fn i64_as_decimal_string_round_trips_min_zero_max_negative() {
        for case in [i64::MIN, -1i64, 0, 1, i64::MAX] {
            let w = Wrap { v: case };
            let json = serde_json::to_string(&w).expect("serialize");
            assert!(
                json.contains(&format!("\"{case}\"")),
                "expected quoted decimal in `{json}`"
            );
            // Value must NOT appear unquoted (would imply it serialized
            // as a JSON number).
            assert!(
                !json.contains(&format!(":{case},")) && !json.contains(&format!(":{case}}}")),
                "value must not appear unquoted in `{json}`"
            );
            let back: Wrap = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, w, "round-trip failed for {case}");
        }
    }

    #[test]
    fn i64_as_decimal_string_rejects_non_decimal_rejects_overflow_rejects_trailing_garbage() {
        // Non-decimal.
        let err = serde_json::from_str::<Wrap>(r#"{"v":"abc"}"#).unwrap_err();
        assert!(
            err.to_string().contains("invalid i64 decimal string"),
            "unexpected error: {err}"
        );
        // Positive overflow (one past i64::MAX).
        let err = serde_json::from_str::<Wrap>(r#"{"v":"9223372036854775808"}"#).unwrap_err();
        assert!(
            err.to_string().contains("invalid i64 decimal string"),
            "unexpected error: {err}"
        );
        // Negative overflow (one past i64::MIN).
        let err = serde_json::from_str::<Wrap>(r#"{"v":"-9223372036854775809"}"#).unwrap_err();
        assert!(
            err.to_string().contains("invalid i64 decimal string"),
            "unexpected error: {err}"
        );
        // Trailing garbage.
        let err = serde_json::from_str::<Wrap>(r#"{"v":"42abc"}"#).unwrap_err();
        assert!(
            err.to_string().contains("invalid i64 decimal string"),
            "unexpected error: {err}"
        );
        // JSON number rejected — helper only accepts strings. A JS
        // consumer that emits a number would silently lose precision
        // above 2^53, which is what this helper exists to prevent.
        let err = serde_json::from_str::<Wrap>(r#"{"v":42}"#).unwrap_err();
        assert!(
            err.to_string().contains("string"),
            "unexpected error: {err}"
        );
    }
}
