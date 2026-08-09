//! @ai-generated - The v4 `relation_verdict` oracle rows (capture-only).
//!
//! The 26 relation identities of `RELATION_QUERY_SPECS`, each validated
//! against its checked-in tsgo-captured snapshot through the relation
//! consumption driver (`oracle::relation_driver`) — tsgo NEVER launches here.
//! Parity is enforced for the 18 non-ledger rows; the 8 known-mismatch ledger
//! rows assert the engine's live answer against the registry PIN instead, so a
//! future engine fix flips loudly. This family is NOT M=0: the honest state is
//! captured-records-with-known-mismatch-ledger
//! (`docs/arch/ri0-relation-verdict-oracle-addendum.md` §5).

use super::oracle::identity;
use super::oracle::query_specs::{RelationQuerySpec, RELATION_QUERY_SPECS};
use super::oracle::relation_driver;
use super::oracle::relation_probe::{self, RelationVerdict, RelationVerdictValue};
use super::oracle::snapshot;
use super::support::*;

/// Load one relation spec's checked-in snapshot and materialize its stored
/// value into the normalized boundary (the same rails the sweep rides).
fn load_oracle_value(spec: &RelationQuerySpec) -> RelationVerdictValue {
    let env = super::oracle::driver::pinned_env();
    let id = relation_probe::relation_identity_from_spec(spec)
        .unwrap_or_else(|e| panic!("{}: identity derivation: {e:?}", spec.row_function));
    let snapshot_id = identity::derive_relation_snapshot_id(&id, &env);
    let path = super::oracle::driver::snapshot_abs_path(spec.oracle_family, &snapshot_id);
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("{}: read {}: {e}", spec.row_function, path.display()));
    let json: serde_json::Value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|e| panic!("{}: parse: {e}", spec.row_function));
    let decoded = snapshot::decode_strict(&json)
        .unwrap_or_else(|e| panic!("{}: strict decode: {e:?}", spec.row_function));
    snapshot::materialize_relation_value(&decoded)
        .unwrap_or_else(|e| panic!("{}: materialize: {e:?}", spec.row_function))
}

fn spec_named(row_function: &'static str) -> &'static RelationQuerySpec {
    RELATION_QUERY_SPECS
        .iter()
        .find(|s| s.row_function == row_function)
        .unwrap_or_else(|| panic!("registry seats {row_function}"))
}

// =====================================================================
// The capture-only sweep: every registry row against its snapshot +
// the engine observation under the parity/ledger posture.
// =====================================================================

#[test]
fn relation_verdict_oracle_rows_match_captured_snapshots() {
    relation_driver::run_relation_rows();
}

// =====================================================================
// The union wrapper prevents distribution: `any` produces ONE
// `true`, `never` produces ONE `true` (no distribution collapse, no
// both-branch union) — pinned by the CAPTURED snapshots; removing the
// wrapper fails the strict header inverse (probe-side guard).
// =====================================================================

#[test]
fn relation_any_and_never_capture_one_true_with_no_bindings() {
    for row in [
        "relation_any_extends_string",
        "relation_never_extends_string",
    ] {
        let value = load_oracle_value(spec_named(row));
        assert_eq!(
            value.verdict,
            RelationVerdict::Assignable,
            "{row}: the tuple wire yields ONE true (no distribution)"
        );
        assert!(
            value.bindings.is_empty(),
            "{row}: a binder-free relation captures no bindings"
        );
    }
}

// =====================================================================
// A false verdict carries NO bindings (captured), and the infer
// rows' captured bindings land in target-pattern binder preorder with the
// projected bound.
// =====================================================================

#[test]
fn relation_false_verdict_captures_no_bindings() {
    for row in [
        "relation_unknown_extends_string",
        "relation_string_extends_never",
        "relation_whole_union_not_assignable",
    ] {
        let value = load_oracle_value(spec_named(row));
        assert_eq!(
            value.verdict,
            RelationVerdict::NotAssignable,
            "{row}: captured false"
        );
        assert!(
            value.bindings.is_empty(),
            "{row}: a false verdict carries no bindings"
        );
    }
}

#[test]
fn relation_infer_rows_capture_ordered_bindings_with_projected_bounds() {
    // `{ value: number } → { value: infer V }` binds V = number.
    let value = load_oracle_value(spec_named("relation_infer_value_of_object"));
    assert_eq!(value.verdict, RelationVerdict::Assignable);
    assert_eq!(value.bindings.len(), 1);
    assert_eq!(value.bindings[0].ordinal, 0);
    assert_eq!(value.bindings[0].name, "V");
    assert!(matches!(
        value.bindings[0].bound,
        verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
    ));

    // `[1, 2, 3] → [unknown, ...infer R]` binds R = [2, 3] (tuple-tail bound
    // normalizes exactly).
    let value = load_oracle_value(spec_named("relation_infer_tail_of_tuple"));
    assert_eq!(value.verdict, RelationVerdict::Assignable);
    assert_eq!(value.bindings.len(), 1);
    assert_eq!(value.bindings[0].name, "R");
    let verter_type_expr::TypeExpr::Tuple { elements, readonly } = &value.bindings[0].bound else {
        panic!(
            "tail bound must be a tuple, got {:?}",
            value.bindings[0].bound
        );
    };
    assert!(!readonly);
    assert_eq!(elements.len(), 2);

    // `() => "hello" → (...args: any[]) => infer R` binds R = "hello"
    // (literal identity kept, not the `string` primitive).
    let value = load_oracle_value(spec_named("relation_infer_return_of_function"));
    assert_eq!(value.verdict, RelationVerdict::Assignable);
    assert_eq!(value.bindings.len(), 1);
    assert!(matches!(
        value.bindings[0].bound,
        verter_type_expr::TypeExpr::Literal(verter_type_expr::LiteralValue::String(_))
    ));
}

// =====================================================================
// The family-execute ACTIVATION guards: `execute(SemanticQueryKey::Relate)`
// is the LIVE sole relation authority (the degenerate `Miss` arm and the
// execute-invisibility fence are deleted; the adapter rides it).
// =====================================================================

#[test]
fn relate_family_execute_is_the_live_relation_authority() {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{
        ProjectionMode, QueryResult, RelationOutcome, SemanticQueryApi, SemanticQueryValue,
    };
    use std::sync::Arc;

    let host = make_host_with_footprint();
    let canonical = "/fixtures/relate_family_guard.ts";
    upsert_ts(
        &host,
        canonical,
        "type __RelateSource = string;\ntype __RelateTarget = number;\n",
    );
    let resolve = |name: &str| {
        let (outcome, _record) = host
            .resolve_named_symbol_with_audit(canonical, name, Some(ProjectionMode::Expanded))
            .into_parts();
        outcome
            .ok()
            .flatten()
            .unwrap_or_else(|| panic!("{name} must resolve"))
    };
    let source = resolve("__RelateSource");
    let target = resolve("__RelateTarget");

    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);

    // The full relation identity (the same constructor the authority uses).
    let key = dispatch.relate_key_for(source, target);
    let result = dispatch.execute(key.to_query_key());
    match result {
        QueryResult::Value(output) => match output.value {
            SemanticQueryValue::Relation(payload) => {
                assert_eq!(
                    payload.outcome,
                    RelationOutcome::NotAssignable,
                    "string is not assignable to number — the live authority must decide"
                );
            }
            other => panic!("execute(Relate) must produce SemanticQueryValue::Relation, got {other:?}"),
        },
        other => panic!(
            "execute(Relate) must be a LIVE producer (the degenerate Miss arm is deleted), got {other:?}"
        ),
    }
}

/// WARM replay + non-aliasing guard (the activation's warm path): a
/// decided judgement admits into the `Relate` family and warm-serves the
/// SAME payload through `execute(Relate)`; `execute_type_node(Relate)`
/// rejects with `ValueDomainMismatch` (a Relation value never narrows to
/// a type node); a generation bump misses the warm read and recomputes.
#[test]
fn relate_family_execute_warm_replays_decided_payload() {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{
        ProjectionMode, QueryError, QueryResult, RelationOutcome, SemanticQueryApi,
        SemanticQueryValue,
    };
    use std::sync::Arc;

    let host = make_host_with_footprint();
    let canonical = "/fixtures/relate_family_warm_replay.ts";
    upsert_ts(
        &host,
        canonical,
        "type __WarmRelateSource = string;\ntype __WarmRelateTarget = number;\n",
    );
    let resolve = |name: &str| {
        let (outcome, _record) = host
            .resolve_named_symbol_with_audit(canonical, name, Some(ProjectionMode::Expanded))
            .into_parts();
        outcome
            .ok()
            .flatten()
            .unwrap_or_else(|| panic!("{name} must resolve"))
    };
    let source = resolve("__WarmRelateSource");
    let target = resolve("__WarmRelateTarget");

    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);

    let key = dispatch.relate_key_for(source, target);
    let query_key = key.to_query_key();
    // Cold decide → one admitted entry.
    let first = dispatch.execute(query_key.clone());
    let graph = host.project_type_store().semantic_graph();
    assert_eq!(
        graph.relation_memo_count(),
        1,
        "fixture: the cold decide admitted one relation entry",
    );
    assert!(
        graph.get_relation_payload(host.as_ref(), &key).is_some(),
        "fixture: the admitted entry warm-serves through the payload read",
    );
    assert!(
        matches!(
            first,
            QueryResult::Value(ref output)
                if matches!(&output.value, SemanticQueryValue::Relation(payload)
                    if payload.outcome == RelationOutcome::NotAssignable)
        ),
        "the cold decide must produce the NotAssignable payload",
    );

    // Warm replay: the SAME `execute(Relate)` serves the SAME payload
    // without growing the memo (the activation's warm path).
    let second = dispatch.execute(query_key.clone());
    assert!(
        matches!(
            second,
            QueryResult::Value(ref output)
                if matches!(&output.value, SemanticQueryValue::Relation(payload)
                    if payload.outcome == RelationOutcome::NotAssignable)
        ),
        "the warm replay must serve the same NotAssignable payload",
    );
    assert_eq!(
        graph.relation_memo_count(),
        1,
        "the warm replay must not grow the memo",
    );

    // `execute_type_node(Relate)`: a Relation value never narrows to a
    // type node — `ValueDomainMismatch`, never a fabricated node.
    let executed_node = dispatch.execute_type_node(query_key);
    assert!(
        matches!(
            executed_node,
            QueryResult::Error(QueryError::ValueDomainMismatch { .. })
        ),
        "execute_type_node(Relate) must reject a Relation value with ValueDomainMismatch, got {executed_node:?}",
    );

    // Generation gate: a project-generation bump misses the warm read
    // (the entry stays but recomputes on next ask).
    host.project_type_store().bump_project_generation();
    assert!(
        graph.get_relation_payload(host.as_ref(), &key).is_none(),
        "a project-generation bump must miss the warm relation read",
    );
}

// =====================================================================
// F1 — the stored identity is authenticated against the stored
// `snapshot_id`: a tampered identity axis whose top-level id + filename
// were left intact passes the registry⇄file env-pin rails (the pre-F1
// state) but FAILS the redrive-from-stored-identity authentication.
// =====================================================================

#[test]
fn tampered_stored_identity_fails_id_redrive_authentication() {
    use super::oracle::relation_driver::{
        authenticate_stored_id, validate_relation_env_pins, RelationDriverError,
    };

    let spec = spec_named("relation_infer_value_of_object");
    let env = super::oracle::driver::pinned_env();
    let registry_identity = relation_probe::relation_identity_from_spec(spec)
        .expect("registry spec derives its identity");
    let snapshot_id = identity::derive_relation_snapshot_id(&registry_identity, &env);
    let path = super::oracle::driver::snapshot_abs_path(spec.oracle_family, &snapshot_id);
    let bytes = std::fs::read(&path).expect("read the checked-in snapshot");
    let valid_json: serde_json::Value = serde_json::from_slice(&bytes).expect("parse");

    // The untouched document passes authentication.
    let valid = snapshot::decode_strict(&valid_json).expect("valid decodes");
    authenticate_stored_id(&valid).expect("the checked-in snapshot authenticates");

    // Tamper ONE identity axis (`identity.policy.variance`), leaving the
    // top-level snapshot_id + filename intact.
    let mut tampered_json = valid_json.clone();
    tampered_json["identity"]["policy"]["variance"] = serde_json::json!("strict_contravariance");
    let tampered =
        snapshot::decode_strict(&tampered_json).expect("a valid-tag tamper still strictly decodes");

    // RED PROOF (the pre-F1 rails): registry⇄file validation ACCEPTS the
    // tampered document — the stored top-level id still matches the
    // registry-derived id, because the tamper never touched it.
    validate_relation_env_pins(&tampered, spec, &registry_identity, &env)
        .expect("the registry⇄file rails alone accept the tamper (pre-F1 hole)");

    // GREEN: the redrive-from-stored-identity authentication REJECTS it — the
    // stored identity no longer hashes to the stored id.
    assert!(
        matches!(
            authenticate_stored_id(&tampered),
            Err(RelationDriverError::EnvPinMismatch { ref field, .. })
                if field == "snapshot_id(redrive-from-stored-identity)"
        ),
        "a tampered identity axis must fail the redrive authentication"
    );
}
