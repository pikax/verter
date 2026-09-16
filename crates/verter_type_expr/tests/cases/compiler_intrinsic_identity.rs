//! A compiler intrinsic application is NOT a named reference.
//!
//! The two may render identically — deliberately, because the checker prints
//! `Awaited<T>` either way — but they are different semantic objects and
//! nothing may collapse them. This pins every half of that claim.
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use verter_type_expr::{
    referenced_names, render_type_expr_display, type_expr_from_json, CompilerIntrinsicTypeOp,
    TypeExpr,
};

fn operand() -> TypeExpr {
    TypeExpr::Ref {
        name: Arc::from("T"),
        type_arguments: verter_type_expr::empty_type_args(),
    }
}

/// `Awaited<T>` as AUTHORED — a name the scope may still resolve.
fn authored_awaited() -> TypeExpr {
    TypeExpr::Ref {
        name: Arc::from("Awaited"),
        type_arguments: Arc::from(vec![operand()].into_boxed_slice()),
    }
}

/// `Awaited<T>` once its identity is RESOLVED as compiler-native.
fn resolved_awaited() -> TypeExpr {
    TypeExpr::IntrinsicApplication {
        op: CompilerIntrinsicTypeOp::Awaited,
        arguments: Arc::from(vec![operand()].into_boxed_slice()),
    }
}

#[test]
fn an_intrinsic_application_is_not_the_reference_that_renders_the_same() {
    assert_ne!(
        authored_awaited(),
        resolved_awaited(),
        "a resolved compiler intrinsic must never compare equal to an authored \
         Ref(\"Awaited\") — collapsing them re-admits string identity into semantics"
    );
}

#[test]
fn both_render_as_awaited_of_the_operand() {
    let authored = render_type_expr_display(&authored_awaited()).expect("authored renders");
    let resolved = render_type_expr_display(&resolved_awaited()).expect("resolved renders");
    assert_eq!(authored.text, "Awaited<T>");
    assert_eq!(
        resolved.text, "Awaited<T>",
        "rendering deliberately coincides — the checker prints the same text"
    );
}

#[test]
fn only_the_authored_reference_names_something_resolvable() {
    let authored = referenced_names(&authored_awaited());
    let authored_heads: Vec<&str> = authored
        .type_names
        .iter()
        .map(|n| n.head.as_str())
        .collect();
    assert!(
        authored_heads.contains(&"Awaited"),
        "an authored reference names something resolvable, got {authored_heads:?}"
    );

    let resolved = referenced_names(&resolved_awaited());
    let resolved_heads: Vec<&str> = resolved
        .type_names
        .iter()
        .map(|n| n.head.as_str())
        .collect();
    assert!(
        !resolved_heads.contains(&"Awaited"),
        "a compiler intrinsic names NO declaration — surfacing it would make a \
         consumer resolve it in scope; got {resolved_heads:?}"
    );
    assert!(
        resolved_heads.contains(&"T"),
        "the operands still participate in the walk, got {resolved_heads:?}"
    );
}

#[test]
fn the_two_hash_differently() {
    fn digest(expr: &TypeExpr) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        expr.hash(&mut h);
        h.finish()
    }
    assert_ne!(
        digest(&authored_awaited()),
        digest(&resolved_awaited()),
        "distinct semantic objects must not share a content-addressed key"
    );
}

#[test]
fn the_wire_form_keeps_the_op_identity_and_round_trips() {
    let value = resolved_awaited().to_json_value();
    assert_eq!(value["kind"], "intrinsicApplication");
    assert_eq!(value["op"], "awaited");
    let back = type_expr_from_json(&value).expect("round trips");
    assert_eq!(back, resolved_awaited());
    assert_ne!(
        back,
        authored_awaited(),
        "the wire form must not decode a compiler intrinsic into a named reference"
    );
}

#[test]
fn an_unknown_wire_op_fails_to_decode_rather_than_guessing() {
    let mut value = resolved_awaited().to_json_value();
    value["op"] = serde_json::Value::String("uppercase".to_string());
    assert!(
        type_expr_from_json(&value).is_none(),
        "an unrecognised op must fail the decode, never fall back to another operation"
    );
}
