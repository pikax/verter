//! T5-3 — the `Unknown` wire/display/hash bytes are UNCHANGED by the opaque
//! `UnknownValue` payload: JSON emits exactly `{"kind":"unknown","raw":…}`
//! (no provenance field), display emits exactly the raw text (empty raw keeps
//! `EmptyUnknownSource`), and the recursive hash stream keeps discriminant 21
//! plus the raw `str` stream only. Decoding any `unknown` JSON yields the
//! `WireOpaque` provenance while equality/hash stay raw-only.
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use verter_type_expr::{
    render_type_expr_display, type_expr_from_json, TypeExpr, TypeExprDisplayError,
    UnknownProvenance, UnknownValue,
};

fn unknown_fixtures() -> Vec<(&'static str, UnknownValue)> {
    vec![
        (
            "Custom & Raw",
            UnknownValue::unsupported_syntax("Custom & Raw"),
        ),
        ("{Foo}", UnknownValue::jsdoc_parse_fallback("{Foo}")),
        ("opaque<>", UnknownValue::wire_opaque("opaque<>")),
        (
            "semanticMiss",
            UnknownValue::compatibility_projection("semanticMiss"),
        ),
        (
            "semanticObjectSurface",
            UnknownValue::compatibility_projection("semanticObjectSurface"),
        ),
    ]
}

#[test]
fn unknown_json_bytes_are_exact_and_provenance_free() {
    for (raw, value) in unknown_fixtures() {
        let expr = TypeExpr::Unknown(value);
        let json = expr.to_json_value();
        // Exact shape: two keys, no provenance.
        assert_eq!(
            json,
            serde_json::json!({ "kind": "unknown", "raw": raw }),
            "the JSON wire shape must be exactly kind+raw"
        );
        let object = json.as_object().expect("a JSON object");
        assert_eq!(object.len(), 2, "NO provenance field on the wire");
        // Byte-exact serde string.
        let bytes = serde_json::to_string(&expr).expect("serialize");
        let expected = format!(
            "{{\"kind\":\"unknown\",\"raw\":{}}}",
            serde_json::to_string(raw).unwrap()
        );
        assert_eq!(bytes, expected, "the serde bytes are unchanged");
        // Roundtrip: decodes via `wire_opaque`; raw-only equality holds
        // against the original regardless of provenance.
        let decoded = type_expr_from_json(&json).expect("decode");
        let TypeExpr::Unknown(decoded_value) = &decoded else {
            panic!("the unknown kind must decode to Unknown");
        };
        assert_eq!(decoded_value.raw(), raw);
        assert_eq!(decoded_value.provenance(), UnknownProvenance::WireOpaque);
        assert_eq!(decoded, expr, "equality is RAW-ONLY (provenance-blind)");
    }

    // An UNRECOGNISED kind also decodes as wire-opaque raw text (the legacy
    // forward-compat fallback).
    let fallback = type_expr_from_json(&serde_json::json!({ "kind": "someFutureKind" }))
        .expect("the kind fallback decodes");
    assert_eq!(
        fallback,
        TypeExpr::Unknown(UnknownValue::wire_opaque("someFutureKind"))
    );
}

#[test]
fn unknown_display_bytes_are_exact_and_empty_keeps_error() {
    for (raw, value) in unknown_fixtures() {
        let rendered = render_type_expr_display(&TypeExpr::Unknown(value))
            .expect("a non-empty unknown displays");
        assert_eq!(rendered.text, raw, "display emits EXACTLY the raw text");
    }
    // Empty raw keeps the EmptyUnknownSource behaviour.
    assert_eq!(
        render_type_expr_display(&TypeExpr::Unknown(UnknownValue::missing_output())),
        Err(TypeExprDisplayError::EmptyUnknownSource)
    );
}

#[test]
fn unknown_hash_stream_is_raw_only_and_provenance_blind() {
    fn digest(expr: &TypeExpr) -> u64 {
        let mut h = DefaultHasher::new();
        expr.hash(&mut h);
        h.finish()
    }
    for (raw, value) in unknown_fixtures() {
        let expr = TypeExpr::Unknown(value);
        // Reference stream: discriminant 21 (isize) + the raw `str` stream —
        // byte-identical to the legacy `Unknown { raw: String }` derive.
        let mut reference = DefaultHasher::new();
        21isize.hash(&mut reference);
        raw.hash(&mut reference);
        assert_eq!(
            digest(&expr),
            reference.finish(),
            "hash stream must stay discriminant-21 + raw bytes only"
        );
        // Provenance never enters identity: same raw, any provenance ⇒ equal
        // value AND equal digest.
        let twin = TypeExpr::Unknown(UnknownValue::unsupported_syntax(raw));
        assert_eq!(expr, twin);
        assert_eq!(digest(&expr), digest(&twin));
    }
}
