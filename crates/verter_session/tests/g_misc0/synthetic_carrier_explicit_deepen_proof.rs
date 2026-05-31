//! Positive executable proof of the synthetic-carrier explicit
//! deepening route through `ShapeCacheKey::semantic_node_whole`.
//!
//! Contract under proof (per `[[component-meta-shallow-by-default-rule]]`
//! and the `TypeExpr::SyntheticSlotBinding` rustdoc): the ONLY
//! legitimate way to deepen a `TypeExpr::SyntheticSlotBinding(
//! SyntheticCarrierKey)` carrier into its underlying member shape is
//! to construct the cache key
//!
//! ```ignore
//! ShapeCacheKey::semantic_node_whole(
//!     carrier.scope_canonical_id.clone(),
//!     SemanticNodeId(carrier.value_node),
//!     mode,
//! )
//! ```
//!
//! and consult `ShapeCacheDb`. No production consumer exercises this
//! route today — every projector, reducer, registry, and graph-builder
//! site treats the carrier as a shallow terminal. This test stands
//! alone (synthetic-only fixture) and proves the cache-key identity
//! round-trip is well-defined for any future consumer that does need
//! to deepen the carrier.
//!
//! The complementary architecture guard
//! `synthetic_carrier_explicit_deepen_routes_through_shape_cache_key`
//! at `tests/no_carrier_verdict_db.rs` bans any production source
//! that constructs `SemanticNodeId(<ident>.value_node)` outside the
//! legitimate cache-route call.
//!
//! Discrimination (RED-on-revert):
//!   - The proof's positive assertion asserts the carrier-aware cache
//!     route returns the inserted DEEP type. Reverting the
//!     cache-route key to a different `SemanticNodeId` (or a different
//!     scope, or a different `ProjectionMode`) makes the warm peek
//!     miss and the assertion fails.
//!   - The proof's negative assertion verifies the lookup is keyed on
//!     `value_node`: changing the looked-up carrier's `value_node`
//!     must miss.

use std::sync::Arc;

use verter_session::component_meta_caches::ShapeCacheDb;
use verter_session::semantic_query::ProjectionMode;
use verter_type_expr::{SyntheticCarrierKey, SyntheticCarrierSurfaceKind, TypeExpr};

/// Construct a synthetic carrier with a distinct `value_node` for
/// identity-discrimination assertions.
fn make_carrier(scope: &str, slot: &str, binding: &str, value_node: u64) -> SyntheticCarrierKey {
    SyntheticCarrierKey {
        scope_canonical_id: Arc::from(scope),
        surface_kind: SyntheticCarrierSurfaceKind::SlotBinding,
        slot_name: Some(Arc::from(slot)),
        binding_name: Arc::from(binding),
        value_node,
    }
}

/// A distinct deep `TypeExpr` value that cannot be confused with a
/// carrier — used so the assertion catches a regression where the
/// cache route silently returns the carrier itself instead of the
/// deep shape.
fn deep_type_for_proof() -> TypeExpr {
    TypeExpr::Literal(verter_type_expr::LiteralValue::String(
        "DEEP_TYPE_PROOF".to_string(),
    ))
}

#[test]
fn synthetic_carrier_deepen_round_trips_through_shape_cache_db() {
    let db = ShapeCacheDb::new();
    let carrier = make_carrier(
        "/component-meta-cache-proof/Owner.vue",
        "default",
        "items",
        424242,
    );
    let deep = deep_type_for_proof();

    // Publish the deep type under the legitimate cache-route identity.
    db.insert_synthetic_carrier_deep_for_test(&carrier, ProjectionMode::Shallow, deep.clone());

    // The cache must hold exactly one entry — the synthetic-carrier
    // deep entry. Discriminates against a silent no-op insert.
    assert_eq!(
        db.live_count(),
        1,
        "ShapeCacheDb must hold the inserted synthetic-carrier deep \
         entry under the cache-route identity"
    );

    // The legitimate cache-route lookup MUST return the deep type —
    // not the carrier, not None.
    let got = db
        .get_synthetic_carrier_deep_for_test(&carrier, ProjectionMode::Shallow)
        .expect(
            "ShapeCacheKey::semantic_node_whole(scope, \
             SemanticNodeId(carrier.value_node), Shallow) must hit the \
             entry inserted under the same identity tuple",
        );
    assert_eq!(
        got, deep,
        "cache route must return the materialised deep TypeExpr, not \
         the carrier or another value"
    );

    // Discriminating negative #1: looking up a DIFFERENT carrier
    // (same identity tuple except `value_node`) MUST miss. Proves the
    // cache route is keyed on `value_node`, not on
    // `(scope, slot, binding)` alone.
    let other_carrier = SyntheticCarrierKey {
        value_node: carrier.value_node + 1,
        ..carrier.clone()
    };
    assert!(
        db.get_synthetic_carrier_deep_for_test(&other_carrier, ProjectionMode::Shallow)
            .is_none(),
        "cache route must be keyed on `value_node`; a carrier with a \
         different `value_node` must miss"
    );

    // Discriminating negative #2: looking up under a DIFFERENT scope
    // MUST miss. Proves the cache route is keyed on
    // `scope_canonical_id`, not on `value_node` alone.
    let other_scope_carrier = SyntheticCarrierKey {
        scope_canonical_id: Arc::from("/component-meta-cache-proof/OtherOwner.vue"),
        ..carrier.clone()
    };
    assert!(
        db.get_synthetic_carrier_deep_for_test(&other_scope_carrier, ProjectionMode::Shallow)
            .is_none(),
        "cache route must be keyed on `scope_canonical_id`; a carrier \
         under a different scope must miss"
    );

    // Discriminating negative #3: looking up under a DIFFERENT
    // `ProjectionMode` MUST miss. Proves the cache route is keyed on
    // the projection mode, not on the carrier identity alone.
    assert!(
        db.get_synthetic_carrier_deep_for_test(&carrier, ProjectionMode::Expanded)
            .is_none(),
        "cache route must be keyed on `ProjectionMode`; a peek with a \
         different mode must miss"
    );
}

/// Concurrent-carrier admissibility: two distinct carriers (same
/// `(scope, slot, binding)` tuple, different `value_node`) must
/// coexist in the cache as distinct entries — the `value_node`
/// discriminates same-name carriers across different slots of the
/// same component (the brief's identity contract for
/// `SyntheticCarrierKey`).
#[test]
fn synthetic_carrier_deepen_distinguishes_same_name_carriers_by_value_node() {
    let db = ShapeCacheDb::new();
    let carrier_a = make_carrier(
        "/component-meta-cache-proof/Distinct.vue",
        "header",
        "items",
        100,
    );
    let carrier_b = make_carrier(
        "/component-meta-cache-proof/Distinct.vue",
        "header",
        "items",
        200,
    );

    let deep_a = TypeExpr::Literal(verter_type_expr::LiteralValue::String("DEEP_A".to_string()));
    let deep_b = TypeExpr::Literal(verter_type_expr::LiteralValue::String("DEEP_B".to_string()));

    db.insert_synthetic_carrier_deep_for_test(&carrier_a, ProjectionMode::Shallow, deep_a.clone());
    db.insert_synthetic_carrier_deep_for_test(&carrier_b, ProjectionMode::Shallow, deep_b.clone());

    assert_eq!(
        db.live_count(),
        2,
        "two same-name carriers with distinct `value_node` must occupy \
         two distinct cache entries"
    );

    let got_a = db
        .get_synthetic_carrier_deep_for_test(&carrier_a, ProjectionMode::Shallow)
        .expect("carrier_a must hit");
    let got_b = db
        .get_synthetic_carrier_deep_for_test(&carrier_b, ProjectionMode::Shallow)
        .expect("carrier_b must hit");

    assert_eq!(
        got_a, deep_a,
        "carrier_a lookup must return DEEP_A, not DEEP_B — proves the \
         cache route discriminates same-name carriers by `value_node`"
    );
    assert_eq!(
        got_b, deep_b,
        "carrier_b lookup must return DEEP_B, not DEEP_A — proves the \
         cache route discriminates same-name carriers by `value_node`"
    );
}
