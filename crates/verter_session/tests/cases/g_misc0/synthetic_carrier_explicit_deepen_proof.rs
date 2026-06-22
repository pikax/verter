//! Positive executable proof of the synthetic-carrier explicit
//! deepening route through the content-free
//! [`verter_session::semantic_query::SyntheticBindingId`] cache identity.
//!
//! Contract under proof (per `[[component-meta-shallow-by-default-rule]]`
//! and the `TypeExpr::SyntheticSlotBinding` rustdoc): the ONLY legitimate
//! way to deepen a `TypeExpr::SyntheticSlotBinding(SyntheticCarrierKey)`
//! carrier into its underlying member shape is to construct the cache key
//!
//! ```ignore
//! ShapeCacheKey::synthetic_binding_whole(
//!     SyntheticBindingId::from_carrier_key(&carrier),
//!     mode,
//! )
//! ```
//!
//! and consult `ShapeCacheDb`. The cache identity is the content-free
//! [`SyntheticBindingId`] — `(scope_canonical_id, surface_kind,
//! slot_name, binding_name)`. The carrier's `value_node` arena ordinal is
//! value-side PROVENANCE only: it round-trips through
//! `SemanticNodeData::SyntheticBinding { id, value_node }` at the compat
//! materialisation boundary (re-attached via
//! `SyntheticBindingId::to_carrier_key`), and NEVER enters the cache key.
//!
//! No production consumer exercises this route today — every projector,
//! reducer, registry, and graph-builder site treats the carrier as a
//! shallow terminal. This test stands alone (synthetic-only fixture) and
//! proves the content-free cache-key identity is well-defined for any
//! future consumer that does need to deepen the carrier.
//!
//! `ShapeCacheDb` is a SINGLE-ENTRY cache: warm correctness holds only
//! through the fact-signature validation rail. The rail half of this
//! contract (a warm entry whose self-root names a changed/untracked
//! canonical is recomputed cold, never stale-served) is exercised
//! in-crate at `crate::query_db_self_root_tests::
//! shape_cache_db_synthetic_binding_untracked_self_root_rejects_warm_entry`,
//! because `ShapeCacheDb::get_or_compute` / `ResolverContext` /
//! `MaterializedTypeExpr` are crate-internal. This cross-crate proof
//! exercises the pure identity: collapse (same identity ⇒ one entry),
//! the still-discriminating axes, and the value-side provenance
//! round-trip.
//!
//! The complementary architecture guard
//! `synthetic_carrier_explicit_deepen_routes_through_shape_cache_key`
//! at `tests/cases/g_misc2/synthetic_carrier_explicit_deepen_routes_through_shape_cache_key.rs`
//! bans any production source that constructs
//! `SemanticNodeId(<ident>.value_node)` as a cache key — the `value_node`
//! ordinal is provenance, never identity.
//!
//! Discrimination (RED-on-revert):
//!   - The collapse assertion asserts two carriers with the SAME
//!     `(scope, surface_kind, slot_name, binding_name)` but DIFFERENT
//!     `value_node` produce ONE entry (`live_count() == 1`). This is the
//!     INVERSE of the retired proof (which asserted distinct `value_node`
//!     ⇒ distinct entries). On the old `SemanticNodeId(value_node)` key
//!     the second insert would key disjointly and `live_count() == 2`.
//!   - The still-discriminating axes (different scope / slot / binding /
//!     surface_kind / mode each MISS) prove the identity tuple is
//!     complete, not collapsed too far.
//!   - The provenance round-trip asserts the `value_node` survives ONLY
//!     as value-side data: `from_carrier_key` drops it from the identity
//!     and `to_carrier_key` re-attaches the CURRENT ordinal.

use std::sync::Arc;

use verter_session::component_meta_caches::ShapeCacheDb;
use verter_session::semantic_query::{ProjectionMode, SyntheticBindingId};
use verter_type_expr::{SyntheticCarrierKey, SyntheticCarrierSurfaceKind, TypeExpr};

/// Construct a synthetic carrier with a distinct `value_node`. The
/// `value_node` is value-side provenance — it does NOT participate in the
/// content-free cache identity.
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
/// carrier — used so an assertion catches a regression where the cache
/// route silently returns the carrier itself instead of the deep shape.
fn deep_type_for_proof() -> TypeExpr {
    TypeExpr::Literal(verter_type_expr::LiteralValue::String(
        "DEEP_TYPE_PROOF".to_string(),
    ))
}

#[test]
fn synthetic_carrier_deepen_round_trips_through_content_free_identity() {
    let db = ShapeCacheDb::new();
    let carrier = make_carrier(
        "/component-meta-cache-proof/Owner.vue",
        "default",
        "items",
        424242,
    );
    let deep = deep_type_for_proof();

    // Publish the deep type under the content-free synthetic-binding
    // identity.
    db.insert_synthetic_carrier_deep_for_test(&carrier, ProjectionMode::Shallow, deep.clone());

    // The cache must hold exactly one entry — the synthetic-binding deep
    // entry. Discriminates against a silent no-op insert.
    assert_eq!(
        db.live_count(),
        1,
        "ShapeCacheDb must hold the inserted synthetic-binding deep entry \
         under the content-free identity"
    );

    // The content-free identity lookup MUST return the deep type — not
    // the carrier, not None.
    let got = db
        .get_synthetic_carrier_deep_for_test(&carrier, ProjectionMode::Shallow)
        .expect(
            "ShapeCacheKey::synthetic_binding_whole(SyntheticBindingId::from_carrier_key(\
             &carrier), Shallow) must hit the entry inserted under the same \
             content-free identity tuple",
        );
    assert_eq!(
        got, deep,
        "cache route must return the materialised deep TypeExpr, not the \
         carrier or another value"
    );
}

/// Equivalence (the NEW invariant, INVERSE of the retired proof): two
/// carriers with the SAME `(scope, surface_kind, slot_name, binding_name)`
/// but DIFFERENT `value_node` now produce the SAME `ShapeCacheKey`. The
/// content-free `SyntheticBindingId` is the identity, so an insert under
/// carrier-A warm-hits a lookup under carrier-B and the cache holds ONE
/// entry, not two.
#[test]
fn synthetic_carrier_deepen_collapses_same_identity_carriers_by_dropping_value_node() {
    let db = ShapeCacheDb::new();
    let carrier_a = make_carrier(
        "/component-meta-cache-proof/Collapse.vue",
        "header",
        "items",
        100,
    );
    // Same identity tuple, DIFFERENT value_node provenance.
    let carrier_b = SyntheticCarrierKey {
        value_node: 200,
        ..carrier_a.clone()
    };

    let deep_a = TypeExpr::Literal(verter_type_expr::LiteralValue::String("DEEP_A".to_string()));

    // Insert under carrier-A.
    db.insert_synthetic_carrier_deep_for_test(&carrier_a, ProjectionMode::Shallow, deep_a.clone());

    // A lookup under carrier-B (different `value_node`, same identity)
    // MUST warm-hit the entry inserted under carrier-A. On the retired
    // `SemanticNodeId(value_node)` key this would miss.
    let got_b = db
        .get_synthetic_carrier_deep_for_test(&carrier_b, ProjectionMode::Shallow)
        .expect(
            "a carrier with a different `value_node` but the same content-free \
             identity MUST warm-hit the entry inserted under the first carrier — \
             `value_node` is provenance, not identity",
        );
    assert_eq!(
        got_b, deep_a,
        "the content-free identity collapses same-identity carriers: \
         carrier_b's lookup returns carrier_a's deep type"
    );

    // Insert under carrier-B as well: because the key collapses onto the
    // SAME identity, the cache MUST still hold exactly ONE entry. This is
    // the discriminating inverse of the retired
    // `..._distinguishes_same_name_carriers_by_value_node` test (which
    // asserted `live_count() == 2`).
    db.insert_synthetic_carrier_deep_for_test(&carrier_b, ProjectionMode::Shallow, deep_a.clone());
    assert_eq!(
        db.live_count(),
        1,
        "two same-identity carriers with distinct `value_node` MUST collapse \
         onto ONE cache entry — the content-free `SyntheticBindingId` drops \
         the `value_node` ordinal from the identity"
    );
}

/// Still-discriminating axes: each component of the content-free identity
/// tuple is a real key axis. A carrier differing only in
/// `scope_canonical_id`, `slot_name`, `binding_name`, `surface_kind`, or
/// the looked-up `ProjectionMode` MUST miss a peek for the original
/// carrier — proving the identity is complete, not collapsed too far.
#[test]
fn synthetic_carrier_deepen_still_discriminates_identity_axes() {
    let db = ShapeCacheDb::new();
    let carrier = make_carrier(
        "/component-meta-cache-proof/Axes.vue",
        "default",
        "items",
        7,
    );
    let deep = deep_type_for_proof();
    db.insert_synthetic_carrier_deep_for_test(&carrier, ProjectionMode::Shallow, deep.clone());

    // Different scope_canonical_id MUST miss.
    let other_scope = SyntheticCarrierKey {
        scope_canonical_id: Arc::from("/component-meta-cache-proof/OtherOwner.vue"),
        ..carrier.clone()
    };
    assert!(
        db.get_synthetic_carrier_deep_for_test(&other_scope, ProjectionMode::Shallow)
            .is_none(),
        "identity must be keyed on `scope_canonical_id`; a carrier under a \
         different scope must miss"
    );

    // Different slot_name MUST miss.
    let other_slot = SyntheticCarrierKey {
        slot_name: Some(Arc::from("footer")),
        ..carrier.clone()
    };
    assert!(
        db.get_synthetic_carrier_deep_for_test(&other_slot, ProjectionMode::Shallow)
            .is_none(),
        "identity must be keyed on `slot_name`; a carrier under a different \
         slot must miss"
    );

    // Different binding_name MUST miss.
    let other_binding = SyntheticCarrierKey {
        binding_name: Arc::from("rows"),
        ..carrier.clone()
    };
    assert!(
        db.get_synthetic_carrier_deep_for_test(&other_binding, ProjectionMode::Shallow)
            .is_none(),
        "identity must be keyed on `binding_name`; a carrier with a different \
         bound name must miss"
    );

    // Different surface_kind MUST miss — and ONLY `surface_kind` varies.
    // `slot_name` is held equal to the base carrier's `Some("default")`
    // (NOT flipped to `None`) so this case isolates the `surface_kind`
    // axis: if `surface_kind` were dropped from `SyntheticBindingId`,
    // every other identity field would match the base and the peek would
    // HIT — flipping this assertion RED. The previous form also flipped
    // `slot_name: None`, so a key that silently dropped `surface_kind`
    // would still have missed on `slot_name`, leaving the `surface_kind`
    // axis unproven.
    let other_surface = SyntheticCarrierKey {
        surface_kind: SyntheticCarrierSurfaceKind::Binding,
        ..carrier.clone()
    };
    assert!(
        db.get_synthetic_carrier_deep_for_test(&other_surface, ProjectionMode::Shallow)
            .is_none(),
        "identity must be keyed on `surface_kind`; a `Binding`-surface carrier \
         (same `slot_name` as the base) must miss a `SlotBinding`-surface entry"
    );

    // Different ProjectionMode MUST miss (the demand axis).
    assert!(
        db.get_synthetic_carrier_deep_for_test(&carrier, ProjectionMode::Expanded)
            .is_none(),
        "demand must be keyed on `ProjectionMode`; a peek with a different \
         mode must miss"
    );
}

/// Value-side provenance round-trip: the `value_node` ordinal is dropped
/// from the content-free identity by `from_carrier_key` and re-attached
/// (as the CURRENT ordinal) by `to_carrier_key`. This proves the ordinal
/// survives ONLY as value-side provenance — the round-trip the compat
/// materialisation boundary (`SemanticNodeData::SyntheticBinding`) uses.
#[test]
fn synthetic_binding_identity_drops_value_node_and_round_trips_provenance() {
    let carrier = make_carrier(
        "/component-meta-cache-proof/Provenance.vue",
        "default",
        "items",
        99,
    );

    // `from_carrier_key` projects the content-free identity (no
    // `value_node`).
    let id = SyntheticBindingId::from_carrier_key(&carrier);
    assert_eq!(id.scope_canonical_id, carrier.scope_canonical_id);
    assert_eq!(id.surface_kind, carrier.surface_kind);
    assert_eq!(id.slot_name, carrier.slot_name);
    assert_eq!(id.binding_name, carrier.binding_name);

    // Two carriers differing ONLY in `value_node` project to the SAME
    // identity — the ordinal is not part of the identity.
    let carrier_other_node = SyntheticCarrierKey {
        value_node: carrier.value_node + 555,
        ..carrier.clone()
    };
    assert_eq!(
        id,
        SyntheticBindingId::from_carrier_key(&carrier_other_node),
        "the content-free identity must be invariant under `value_node` — \
         the ordinal is value-side provenance, never identity"
    );

    // `to_carrier_key` re-attaches the value-side ordinal verbatim. This
    // is the compat-boundary re-hydration (`SemanticNodeData::\
    // SyntheticBinding { id, value_node }.raise()` →
    // `TypeExpr::SyntheticSlotBinding(id.to_carrier_key(value_node))`).
    let rehydrated = id.to_carrier_key(carrier.value_node);
    assert_eq!(
        rehydrated, carrier,
        "re-attaching the original `value_node` to the content-free identity \
         must reconstruct the full carrier verbatim"
    );

    // Re-attaching a DIFFERENT ordinal yields a carrier that differs ONLY
    // in `value_node` — the identity stays the same.
    let rehydrated_other = id.to_carrier_key(carrier.value_node + 1);
    assert_ne!(
        rehydrated_other, carrier,
        "re-attaching a different `value_node` must produce a distinct \
         carrier (the ordinal is preserved as provenance)"
    );
    assert_eq!(
        SyntheticBindingId::from_carrier_key(&rehydrated_other),
        id,
        "but the re-projected content-free identity must be unchanged by the \
         different `value_node`"
    );
}

/// Shape-route KEY-CLASSIFIER assert: the shape-route key classifier
/// (`ShapeCacheKey::type_expr_whole_with_context`, surfaced through the
/// `#[cfg(any(test, debug_assertions))]` helper
/// `ShapeCacheDb::type_expr_shape_route_keys_subject_for_test`) returns
/// `None` for an unkeyable nested-carrier subject — so it keys NO slot,
/// never a forged key folding the carrier's `value_node` ordinal. This
/// asserts the CLASSIFIER's `None`/`Some` verdict ONLY; it does NOT drive a
/// full production caller and makes no end-to-end "no cache write" claim
/// (the warm-admission / single-entry rail is covered separately in the
/// in-crate `query_db_self_root_tests`). It is the SHAPE-route analog of
/// `MaterializationCacheKey`'s root-less-anonymous-subject `None`
/// (`derive_materialization_subject` returns `None` for a genuinely
/// root-less anonymous node → that subject keys no DB slot).
///
/// Discriminating: a `TypeExpr` that NESTS a `SyntheticSlotBinding` carrier
/// under a composite (`Parenthesized(carrier)`) is an unkeyable subject —
/// its identity would depend on the carrier's store-relative `value_node`
/// ordinal, which has no content-free representation — so the classifier
/// returns `None`. A clean carrier-free `TypeExpr` classifies to a sound
/// subject (`Some`). A top-level-only classifier (one that checks only the
/// root node for a carrier) would build a `TypeExpr` subject for the nested
/// case and return `Some`, flipping the nested assertion RED.
#[test]
fn shape_route_keys_no_slot_for_unkeyable_nested_carrier_subject() {
    use verter_session::component_meta_caches::ShapeCacheDb;
    use verter_type_expr::LiteralValue;

    let scope = "/component-meta-cache-proof/ShapeAnon.vue";
    let mode = ProjectionMode::Shallow;

    let carrier = make_carrier(scope, "default", "items", 7);

    // A composite that NESTS the carrier under `Parenthesized` — unkeyable.
    let nested: Arc<TypeExpr> = Arc::new(TypeExpr::Parenthesized(Arc::new(
        TypeExpr::SyntheticSlotBinding(Arc::new(carrier)),
    )));
    assert!(
        !ShapeCacheDb::type_expr_shape_route_keys_subject_for_test(
            Arc::from(scope),
            Arc::clone(&nested),
            mode,
        ),
        "a NESTED synthetic carrier (`Parenthesized(SyntheticSlotBinding(..))`) is an \
         unkeyable shape subject — the shape-route key classifier \
         (`type_expr_whole_with_context`) MUST return `None`, so it keys NO slot and \
         cannot forge a key folding the carrier's `value_node`",
    );

    // A clean carrier-free `TypeExpr` classifies to a SOUND subject (`Some`).
    let clean: Arc<TypeExpr> = Arc::new(TypeExpr::Literal(LiteralValue::Number(42.0)));
    assert!(
        ShapeCacheDb::type_expr_shape_route_keys_subject_for_test(Arc::from(scope), clean, mode,),
        "a carrier-free `TypeExpr` must classify to a sound shape subject (`Some`) — \
         proving the `None` above is the unkeyable-subject signal, not a blanket refusal",
    );
}
