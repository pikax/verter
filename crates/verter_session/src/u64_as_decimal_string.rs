//! Serde helper — serializes `u64` as a decimal string (via `Display`),
//! deserializes by parsing a string (via `FromStr`).
//!
//! Rationale: JavaScript `Number` loses precision above `2^53`, and
//! `bigint` is not standards-compliant JSON (`JSON.parse` cannot
//! produce it and `JSON.stringify` cannot emit it). Every `u64` field
//! in the audit record schema routes through this helper so the TS
//! consumer round-trips through `JSON.parse` / `JSON.stringify` without
//! loss. Consumers that need arithmetic call `BigInt(s)` themselves.
//!
//! Architecture rule: u64 JSON transport is a stringified decimal. The
//! corresponding ts-rs annotation `#[ts(type = "string")]` lives on
//! every field that uses this helper.

use std::fmt;
use std::str::FromStr;

use serde::de::{self, Visitor};
use serde::{Deserializer, Serializer};

/// Serialize a `u64` as its decimal string representation.
pub(crate) fn serialize<S: Serializer>(v: &u64, ser: S) -> Result<S::Ok, S::Error> {
    // `collect_str` calls `Display::fmt` and forwards to
    // `serialize_str` — no intermediate allocation beyond the
    // serializer's own buffer.
    ser.collect_str(v)
}

struct U64DecimalStringVisitor;

impl<'de> Visitor<'de> for U64DecimalStringVisitor {
    type Value = u64;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a decimal u64 encoded as a JSON string")
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<u64, E> {
        u64::from_str(v)
            .map_err(|e| E::custom(format_args!("invalid u64 decimal string `{v}`: {e}")))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<u64, E> {
        self.visit_str(&v)
    }
}

/// Deserialize a `u64` from a decimal string. Rejects non-decimal,
/// negative, and overflowing inputs with a serde error.
pub(crate) fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<u64, D::Error> {
    de.deserialize_str(U64DecimalStringVisitor)
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    // Use the helper on a test-only wrapper so we exercise the exact
    // code path a derived struct's `#[serde(with = "...")]` expands
    // into.
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Wrap {
        #[serde(with = "super")]
        v: u64,
    }

    #[test]
    fn u64_as_decimal_string_round_trips_zero_min_and_max() {
        for case in [0u64, 1, u64::MAX] {
            let w = Wrap { v: case };
            let json = serde_json::to_string(&w).expect("serialize");
            // Value must appear as a JSON string, not a number.
            assert!(
                json.contains(&format!("\"{case}\"")),
                "expected quoted decimal in `{json}`"
            );
            assert!(
                !json.contains(&format!(":{case},")) && !json.contains(&format!(":{case}}}")),
                "value must not appear unquoted in `{json}`"
            );
            let back: Wrap = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, w, "round-trip failed for {case}");
        }
    }

    #[test]
    fn u64_as_decimal_string_rejects_non_decimal_rejects_overflow_rejects_negative_as_serde_error()
    {
        // Non-decimal.
        let err = serde_json::from_str::<Wrap>(r#"{"v":"abc"}"#).unwrap_err();
        assert!(
            err.to_string().contains("invalid u64 decimal string"),
            "unexpected error: {err}"
        );
        // Negative (rejected by FromStr<u64>).
        let err = serde_json::from_str::<Wrap>(r#"{"v":"-1"}"#).unwrap_err();
        assert!(
            err.to_string().contains("invalid u64 decimal string"),
            "unexpected error: {err}"
        );
        // Overflow (one past u64::MAX).
        let err = serde_json::from_str::<Wrap>(r#"{"v":"18446744073709551616"}"#).unwrap_err();
        assert!(
            err.to_string().contains("invalid u64 decimal string"),
            "unexpected error: {err}"
        );
        // JSON number (helper only accepts strings; a number variant
        // would silently lose precision above 2^53, which is exactly
        // what this helper exists to prevent).
        let err = serde_json::from_str::<Wrap>(r#"{"v":42}"#).unwrap_err();
        assert!(
            err.to_string().contains("string"),
            "unexpected error: {err}"
        );
    }
}
