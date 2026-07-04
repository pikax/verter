//! @ai-generated - Named-symbol operator-reduction bridge characterisation.
//!
//! Characterises the `materialize_through_aliases` reduction bridge: a
//! top-level alias whose body is an `IndexedAccess` (`type X = Y[K]`) or
//! `KeyOf` (`type X = keyof Y`) operator reduces to its member type / key
//! union under the publication modes (`Navigate` / `Shallow` / `Expanded`)
//! through the SHARED `SemanticQueryKey::IndexedAccess` / `KeyOf` reducer
//! (which canonicalises to `ProjectPath` / the keyof builder), and PRESERVES
//! the operator carrier under `Skeleton` (the mode gate) and for symbolic
//! objects the reducer cannot resolve.

use super::support::*;
use crate::semantic_query::{
    ProjectionMode, ProjectionReductionContext, SemanticNodeData, SemanticQueryApi,
    SemanticQueryKey, SemanticQueryOutput,
};

const OPERATOR_REDUCTION: &str = include_str!("fixtures/operator_reduction.ts");
const INDEX_SIGNATURES: &str = include_str!("fixtures/index_signatures.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/operator_reduction.ts", OPERATOR_REDUCTION);
}

fn upsert_index(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/index_signatures.ts", INDEX_SIGNATURES);
}

// ── Reduction: operator-bodied aliases reduce to their member/key value ──

#[test]
fn named_resolve_reduces_indexed_access_alias_to_member_type() {
    // `type ConcreteLookup = KeySurface["id"]` reduces to `string` through the
    // shared IndexedAccess reducer — NOT an un-reduced `IndexedAccess` carrier.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/operator_reduction.ts",
        "ConcreteLookup",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::String);
    assert!(
        !matches!(expr, TypeExpr::IndexedAccess { .. }),
        "named resolve must REDUCE the indexed-access alias, not publish the \
         un-reduced carrier: {expr:?}"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn named_resolve_reduces_keyof_alias_to_key_union() {
    // `type ConcreteKeys = keyof KeySurface` reduces to the member-name literal
    // union — NOT an un-reduced `KeyOf` carrier.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/operator_reduction.ts",
        "ConcreteKeys",
        &[],
        ProjectionMode::Expanded,
    );

    assert_literal_union(&expr, &["count", "id"]);
    assert!(
        !matches!(expr, TypeExpr::KeyOf(_)),
        "named resolve must REDUCE the keyof alias, not publish the un-reduced \
         carrier: {expr:?}"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn named_resolve_reduces_indexed_access_under_navigate_and_shallow() {
    // The reduction is gated on the publication modes — Navigate and Shallow
    // reduce a named-member indexed access the same as Expanded (the violation
    // was for all three publication modes).
    let host = make_host_with_footprint();
    upsert(&host);

    // Expanded resolves the terminal scalar.
    let (expanded, _record) = resolve_expr(
        &host,
        "/fixtures/operator_reduction.ts",
        "ConcreteLookup",
        &[],
        ProjectionMode::Expanded,
    );
    assert_primitive(&expanded, PrimitiveName::String);

    // Navigate / Shallow reduce the operator too — the discriminating
    // bug-fix fact is that they no longer publish the un-reduced
    // `IndexedAccess` carrier (the mode-shallow surface they produce is a
    // separate mode contract; the violation was leaving the operator shell).
    for mode in [ProjectionMode::Navigate, ProjectionMode::Shallow] {
        let (expr, _record) = resolve_expr(
            &host,
            "/fixtures/operator_reduction.ts",
            "ConcreteLookup",
            &[],
            mode,
        );
        assert!(
            !matches!(expr, TypeExpr::IndexedAccess { .. }),
            "{mode:?} must reduce the indexed-access operator, not publish the \
             un-reduced carrier: {expr:?}"
        );
    }
}

// ── One-resolver: the FFI evaluate path reduces IDENTICALLY ──

#[test]
fn evaluate_expr_reduces_operator_identically_to_named_resolve() {
    // The public FFI `evaluate_type_expression` entry
    // (`WireOperation::EvaluateExpression`) MUST reduce an operator-bodied
    // expression through the SAME single canonical materializer the
    // named-resolve path uses — not an un-bridged evaluate-local copy.
    //
    // A divergent evaluate path with its OWN `materialize_through_aliases`
    // lacking operator arms would terminate at the carrier-preserving
    // `_ => Ok(current)` arm and return the UN-reduced `IndexedAccess` / `KeyOf`
    // carrier when evaluating `KeySurface["id"]` / `keyof KeySurface`. This test
    // pins that both entry-points share the ONE bridged helper: it FAILS if the
    // evaluate result is an operator carrier and PASSES when it is the reduced
    // terminal.
    let host = make_host_with_footprint();
    upsert(&host);
    let scope = "/fixtures/operator_reduction.ts";

    for mode in [
        ProjectionMode::Expanded,
        ProjectionMode::Navigate,
        ProjectionMode::Shallow,
    ] {
        // Indexed access: `KeySurface["id"]` → `string` on BOTH paths.
        let (named, _r) = resolve_expr(&host, scope, "ConcreteLookup", &[], mode);
        let (evaluated, _r) = evaluate_expr(&host, scope, "KeySurface[\"id\"]", mode);
        assert!(
            !matches!(evaluated, TypeExpr::IndexedAccess { .. }),
            "{mode:?} FFI evaluate must REDUCE the indexed-access operator, not \
             publish the un-reduced carrier: {evaluated:?}"
        );
        assert_eq!(
            evaluated, named,
            "{mode:?} FFI evaluate of `KeySurface[\"id\"]` must reduce IDENTICALLY \
             to the named-resolve of `ConcreteLookup`"
        );

        // keyof: `keyof KeySurface` → the member-name literal union on both.
        let (named_keys, _r) = resolve_expr(&host, scope, "ConcreteKeys", &[], mode);
        let (evaluated_keys, _r) = evaluate_expr(&host, scope, "keyof KeySurface", mode);
        assert!(
            !matches!(evaluated_keys, TypeExpr::KeyOf(_)),
            "{mode:?} FFI evaluate must REDUCE the keyof operator, not publish the \
             un-reduced carrier: {evaluated_keys:?}"
        );
        assert_eq!(
            evaluated_keys, named_keys,
            "{mode:?} FFI evaluate of `keyof KeySurface` must reduce IDENTICALLY to \
             the named-resolve of `ConcreteKeys`"
        );
    }
}

// ── Preservation guard #1: symbolic IndexedAccess (open object) survives ──

#[test]
fn open_typeparam_object_indexed_lookup_preserved_under_publication_reduction() {
    // `type SymbolicLookup<T> = T["id"]` resolved with NO type args keeps the
    // object an open `TypeParam`. The shared reducer cannot resolve a concrete
    // surface, so the bridge MUST preserve the `IndexedAccess` carrier rather
    // than fabricate a member type or collapse to `never` / a miss.
    //
    // Discriminating: if the bridge ignored the reducer's no-progress signal
    // (e.g. forced a member or errored), this would not round-trip back to an
    // IndexedAccess carrier.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, _record) = resolve_expr(
        &host,
        "/fixtures/operator_reduction.ts",
        "SymbolicLookup",
        &[],
        ProjectionMode::Expanded,
    );

    assert!(
        matches!(expr, TypeExpr::IndexedAccess { .. }),
        "symbolic `T[\"id\"]` (open object) must preserve the IndexedAccess \
         carrier, got {expr:?}"
    );
}

// ── Preservation guard #2: Skeleton / structural-transit preserves carrier ──

#[test]
fn skeleton_structural_transit_instantiate_preserves_operator_carriers() {
    // The mode gate (`Navigate | Shallow | Expanded`) MUST exclude Skeleton:
    // the BFS / generic-helper traversal mode preserves operator carriers so
    // Conditional branches do not collapse. Two independent assertions:
    //
    // (a) NAMED resolve in `Skeleton` returns the un-reduced `IndexedAccess`
    //     carrier (discriminating against widening / dropping the mode gate —
    //     remove the gate and this reduces to `string` and fails).
    // (b) The bare `Instantiate { context: structural_transit(Skeleton) }`
    //     dispatch — the path `ref_root_reaches_transitive_cycle_node` drives —
    //     returns the un-reduced operator node (bare Instantiate stays
    //     identity-preserving; the bridge never fires on it).
    let host = make_host_with_footprint();
    upsert_index(&host);

    // (a) named resolve in Skeleton mode.
    let (expr, _record) = resolve_expr(
        &host,
        "/fixtures/index_signatures.ts",
        "NumericLookup",
        &[],
        ProjectionMode::Skeleton,
    );
    assert!(
        matches!(expr, TypeExpr::IndexedAccess { .. }),
        "Skeleton named resolve must PRESERVE the IndexedAccess carrier (the \
         mode gate excludes Skeleton), got {expr:?}"
    );

    // (b) bare structural-transit Skeleton Instantiate.
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(&host_ctx);

    let canonical: Arc<str> = Arc::from("/fixtures/index_signatures.ts");
    let base = dispatch.type_slot_for(Arc::clone(&canonical), Arc::from("NumericLookup"));
    let key = SemanticQueryKey::Instantiate(crate::semantic_query::InstantiateKey::new(
        base,
        Arc::from(Vec::new().into_boxed_slice()),
        dispatch.instantiate_context_for(
            &canonical,
            ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Skeleton),
        ),
    ));
    let node = match dispatch.execute_type_node(key) {
        crate::semantic_query::QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
        crate::semantic_query::QueryResult::Recursive(value) => value,
        other => panic!("structural-transit Skeleton Instantiate failed: {other:?}"),
    };
    let graph = host.project_type_store().semantic_graph();
    let mut data = graph.node_data(node);
    // Unwrap a single Alias shell if present (bare Instantiate may return the
    // alias wrapper without unwrapping; the carrier underneath must be the
    // operator node, NOT a reduced member).
    if let Some(SemanticNodeData::Alias(inner)) = data.as_deref() {
        data = graph.node_data(*inner);
    }
    assert!(
        matches!(
            data.as_deref(),
            Some(SemanticNodeData::IndexedAccess { .. })
        ),
        "bare structural-transit Skeleton Instantiate must return the \
         un-reduced IndexedAccess carrier, got {:?}",
        data.as_deref()
    );
}

#[test]
fn evaluate_keyof_reduces_on_cold_navigate_dispatch() {
    // DISCRIMINATING (cold-path Navigate keyof): Navigate-mode body lowering
    // deliberately preserves a no-args named reference as a `DeclRef` carrier
    // (cycle-BFS visibility — `lower.rs` carrier-preservation), so a COLD
    // `Instantiate { published(Navigate) }` of `type X = keyof Y` hands
    // `build_key_of` a `DeclRef` operand. `build_key_of`'s own contract
    // (documented at the bridge's KeyOf arm) is that an UN-RESOLVED reference
    // operand returns the deferred `KeyOf` carrier — which the shared
    // materializer bridge then surfaces and reduces. A `DeclRef` operand
    // falling through to the `Opaque(Miss)` arm instead publishes a
    // `semanticMiss` terminal for the whole evaluate.
    //
    // FAILS while `build_key_of` lacks the `DeclRef`/`InstantiationRef`
    // carrier arm (evaluate returns `Unknown { raw: "semanticMiss" }`);
    // PASSES once the carrier defers and the bridge reduces it. The warm
    // path cannot mask this: the keyof evaluate is the FIRST dispatch on a
    // fresh host, so no prior Expanded materialization can satisfy it.
    let host = make_host_with_footprint();
    upsert(&host);
    let scope = "/fixtures/operator_reduction.ts";

    let (named_keys, _r) =
        resolve_expr(&host, scope, "ConcreteKeys", &[], ProjectionMode::Navigate);
    // The named value is anchored to the REAL key union (not merely compared
    // against the evaluate result): pre-fix BOTH cold Navigate paths degraded
    // to the same `semanticMiss` terminal, so a bare parity assert would pass
    // vacuously.
    assert_literal_union(&named_keys, &["count", "id"]);
    let host2 = make_host_with_footprint();
    upsert(&host2);
    let (evaluated_keys, _r) =
        evaluate_expr(&host2, scope, "keyof KeySurface", ProjectionMode::Navigate);
    assert!(
        !matches!(evaluated_keys, TypeExpr::KeyOf(_)),
        "cold Navigate FFI evaluate must REDUCE the keyof operator, not publish \
         the un-reduced carrier: {evaluated_keys:?}"
    );
    assert_eq!(
        evaluated_keys, named_keys,
        "cold Navigate FFI evaluate of `keyof KeySurface` must reduce IDENTICALLY \
         to the named-resolve of `ConcreteKeys` — a `DeclRef` operand must defer \
         to the bridge, not degrade to a semanticMiss terminal"
    );
}
