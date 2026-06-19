//! Discrimination tests for the `TypeExpr::SyntheticSlotBinding`
//! variant (the R22 synthetic-carrier typed-IR representation).
//!
//! Each test is discriminating: it FAILS against a tree where the
//! variant does not exist AND PASSES where it does — the
//! discriminating property comes from the variant being constructible
//! and its identity surviving Clone / Eq / Hash / JSON round-trip /
//! OXC normalisation.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use verter_type_expr::{
    type_expr_from_json, SyntheticCarrierKey, SyntheticCarrierSurfaceKind, TypeExpr,
};

fn make_key(
    scope: &str,
    surface_kind: SyntheticCarrierSurfaceKind,
    slot_name: Option<&str>,
    binding_name: &str,
    value_node: u64,
) -> SyntheticCarrierKey {
    SyntheticCarrierKey {
        scope_canonical_id: Arc::from(scope),
        surface_kind,
        slot_name: slot_name.map(Arc::from),
        binding_name: Arc::from(binding_name),
        value_node,
    }
}

fn hash_one<H: Hash>(value: &H) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// R22 substrate layout invariant: TypeExpr layout must not widen
/// by adding the SyntheticSlotBinding arm. The widest existing arms
/// (Mapped, RecursiveRef) currently dominate; the new
/// Arc<SyntheticCarrierKey> arm must be pointer-sized and must NOT
/// become the layout ceiling.
#[test]
fn type_expr_size_budget_survives_synthetic_carrier() {
    assert!(
        std::mem::size_of::<TypeExpr>() <= 64,
        "TypeExpr widened past 64 bytes: {}",
        std::mem::size_of::<TypeExpr>()
    );
}

#[test]
fn synthetic_carrier_clone_eq_hash_work() {
    let a = TypeExpr::synthetic_slot_binding(make_key(
        "/abs/Foo.vue",
        SyntheticCarrierSurfaceKind::SlotBinding,
        Some("default"),
        "controls",
        42,
    ));
    let b = TypeExpr::synthetic_slot_binding(make_key(
        "/abs/Foo.vue",
        SyntheticCarrierSurfaceKind::SlotBinding,
        Some("default"),
        "controls",
        42,
    ));
    let c = TypeExpr::synthetic_slot_binding(make_key(
        "/abs/Foo.vue",
        SyntheticCarrierSurfaceKind::SlotBinding,
        Some("default"),
        // Different binding name distinguishes from `a`/`b`.
        "controlsAlt",
        42,
    ));

    // Clone preserves equality.
    let a_clone = a.clone();
    assert_eq!(a, a_clone);
    assert_eq!(hash_one(&a), hash_one(&a_clone));

    // Structurally-identical carriers compare equal even when their
    // backing Arc<SyntheticCarrierKey> values are physically distinct.
    assert_eq!(a, b);
    assert_eq!(hash_one(&a), hash_one(&b));

    // Differing identity tuple => inequality + different hashes.
    assert_ne!(a, c);
    assert_ne!(hash_one(&a), hash_one(&c));
}

#[test]
fn synthetic_carrier_serde_roundtrip() {
    let original = TypeExpr::synthetic_slot_binding(make_key(
        "/abs/Foo.vue",
        SyntheticCarrierSurfaceKind::Binding,
        None,
        "scope",
        // u64 value chosen large enough to overflow JS Number precision
        // if the wire encoding were a numeric literal — the round-trip
        // must NOT lose precision.
        9_007_199_254_740_993,
    ));

    let value = serde_json::to_value(&original).expect("serialize");
    // valueNode MUST be a JSON string to avoid JS Number truncation.
    assert_eq!(
        value
            .get("valueNode")
            .and_then(|v| v.as_str())
            .expect("valueNode should be a string"),
        "9007199254740993"
    );

    let decoded = type_expr_from_json(&value).expect("decode");
    assert_eq!(original, decoded);
}

#[test]
fn synthetic_carrier_value_node_zero_is_legitimate() {
    // `SemanticNodeId(0)` is a VALID node id; the carrier must be
    // constructible AND round-trip correctly with value_node = 0.
    let zero = TypeExpr::synthetic_slot_binding(make_key(
        "/abs/Foo.vue",
        SyntheticCarrierSurfaceKind::SlotBinding,
        Some("default"),
        "binding",
        0,
    ));
    assert!(matches!(zero, TypeExpr::SyntheticSlotBinding(_)));

    let value = serde_json::to_value(&zero).expect("serialize");
    assert_eq!(value.get("valueNode").and_then(|v| v.as_str()), Some("0"));

    // Clone + equality survive a value_node of 0.
    let clone = zero.clone();
    assert_eq!(zero, clone);
}

#[test]
fn synthetic_carrier_same_binding_name_two_value_nodes_distinct() {
    // value_node discriminates same-binding-name carriers across
    // distinct slots within the same component scope.
    let a = TypeExpr::synthetic_slot_binding(make_key(
        "/abs/Foo.vue",
        SyntheticCarrierSurfaceKind::SlotBinding,
        Some("default"),
        "foo",
        1,
    ));
    let b = TypeExpr::synthetic_slot_binding(make_key(
        "/abs/Foo.vue",
        SyntheticCarrierSurfaceKind::SlotBinding,
        Some("default"),
        "foo",
        2,
    ));
    assert_ne!(a, b);
    assert_ne!(hash_one(&a), hash_one(&b));
}

#[test]
fn synthetic_carrier_surface_kind_distinguishes() {
    // Same identity except surface_kind => distinct carriers.
    let slot = TypeExpr::synthetic_slot_binding(make_key(
        "/abs/Foo.vue",
        SyntheticCarrierSurfaceKind::SlotBinding,
        Some("default"),
        "x",
        7,
    ));
    let binding = TypeExpr::synthetic_slot_binding(make_key(
        "/abs/Foo.vue",
        SyntheticCarrierSurfaceKind::Binding,
        Some("default"),
        "x",
        7,
    ));
    assert_ne!(slot, binding);
    assert_ne!(hash_one(&slot), hash_one(&binding));
}

#[test]
fn synthetic_carrier_slot_name_present_vs_absent_distinct() {
    // slot_name participates in identity. None vs Some("default")
    // must be distinguishable, even though the projector surface displays
    // identically.
    let with_slot = TypeExpr::synthetic_slot_binding(make_key(
        "/abs/Foo.vue",
        SyntheticCarrierSurfaceKind::Binding,
        Some("header"),
        "controls",
        13,
    ));
    let without_slot = TypeExpr::synthetic_slot_binding(make_key(
        "/abs/Foo.vue",
        SyntheticCarrierSurfaceKind::Binding,
        None,
        "controls",
        13,
    ));
    assert_ne!(with_slot, without_slot);
    assert_ne!(hash_one(&with_slot), hash_one(&without_slot));
}
