//! Program-analysis fact rail: the `FlowBody` whole-body hash validates
//! against the live `FunctionProgramIndex` — a structural index read,
//! never a re-lower — and fails closed on stale hashes, body edits,
//! untracked canonicals, and overlay-only function bodies.

use std::sync::Arc;

use crate::resolver_core::{FactVersionRef, ProgramAnalysisFactRef, StoreView};
use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;

const FLOW_SOURCE: &str = "export function alpha(n: number) {\n\
     \x20 if (n <= 0) return 0;\n\
     \x20 return alpha(n - 1);\n\
     }\n";

fn make_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig::default()))
}

fn upsert(host: &VerterHost, canonical_id: &str, source: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical_id.to_string()),
        input_id: canonical_id.to_string(),
        source: Arc::from(source),
        file_language: crate::LanguageRegistry::global()
            .classify_static(canonical_id)
            .static_resolution(),
        aliases: Vec::new(),
    });
}

fn alpha_fact(host: &VerterHost, canonical_id: &str, hash: crate::types::Hash16) -> FactVersionRef {
    alpha_fact_at(host, canonical_id, hash, 0)
}

fn alpha_fact_at(
    _host: &VerterHost,
    canonical_id: &str,
    hash: crate::types::Hash16,
    overload_ordinal: u32,
) -> FactVersionRef {
    FactVersionRef::ProgramAnalysis(ProgramAnalysisFactRef::FlowBody {
        function: crate::resolver_core::ProgramAnalysisFunctionRef {
            canonical_id: Arc::from(canonical_id),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            merged_symbol_name: Arc::from("alpha"),
            symbol_space: verter_semantic::facts::SymbolSpace::Value,
            function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
            overload_ordinal,
        },
        flow_body_stable_hash: hash,
    })
}

fn live_alpha_hash(host: &VerterHost, canonical_id: &str) -> crate::types::Hash16 {
    host.ensure_indexed_ready(canonical_id)
        .expect("indexed ready")
        .shallow_state
        .decl_bodies()
        .function_program_index()
        .value_function(
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "alpha",
            &verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
            0,
        )
        .expect("alpha is indexed")
        .entry()
        .flow_body_stable_hash
}

#[test]
fn flow_body_fact_validates_against_the_live_index() {
    let host = make_host();
    upsert(&host, "/ws/flow.ts", FLOW_SOURCE);
    let hash = live_alpha_hash(&host, "/ws/flow.ts");
    let view = host.resolver_store_view_read().into_owned_view();
    assert!(
        view.validates(&alpha_fact(&host, "/ws/flow.ts", hash)),
        "the observed whole-body hash validates against the live index"
    );
}

#[test]
fn flow_body_fact_rejects_a_stale_hash() {
    let host = make_host();
    upsert(&host, "/ws/flow.ts", FLOW_SOURCE);
    let view = host.resolver_store_view_read().into_owned_view();
    assert!(
        !view.validates(&alpha_fact(&host, "/ws/flow.ts", [9u8; 16])),
        "a hash the index never produced must fail closed"
    );
}

#[test]
fn flow_body_fact_rejects_after_a_body_edit() {
    let host = make_host();
    upsert(&host, "/ws/flow.ts", FLOW_SOURCE);
    let hash = live_alpha_hash(&host, "/ws/flow.ts");
    let fact = alpha_fact(&host, "/ws/flow.ts", hash);
    let view = host.resolver_store_view_read().into_owned_view();
    assert!(view.validates(&fact), "pre-edit the fact validates");

    upsert(
        &host,
        "/ws/flow.ts",
        &FLOW_SOURCE.replacen("return 0;", "return 1;", 1),
    );
    let view = host.resolver_store_view_read().into_owned_view();
    assert!(
        !view.validates(&fact),
        "a literal body edit invalidates the recorded whole-body hash"
    );
}

#[test]
fn flow_body_fact_rejects_an_untracked_canonical() {
    let host = make_host();
    upsert(&host, "/ws/flow.ts", FLOW_SOURCE);
    let view = host.resolver_store_view_read().into_owned_view();
    assert!(
        !view.validates(&alpha_fact(&host, "/ws/elsewhere.ts", [7u8; 16])),
        "a canonical the view does not track fails closed"
    );
}

#[test]
fn overlay_flow_body_fact_cannot_validate_against_base() {
    // An overlay-only function body has no base artifact: the base
    // view's per-canonical snapshot lacks the overlay canonical, so the
    // fact can never validate against base (overlay results never
    // populate base-only caches).
    let host = make_host();
    upsert(&host, "/ws/flow.ts", FLOW_SOURCE);
    let base_view = host.resolver_store_view_read().into_owned_view();
    let overlay_only_hash = [3u8; 16];
    assert!(
        !base_view.validates(&alpha_fact(&host, "/overlay/only.ts", overlay_only_hash)),
        "an overlay-only function body must never validate against the base view"
    );
}

#[test]
fn flow_body_fact_identity_discriminates_overload_ordinals() {
    let host = make_host();
    upsert(
        &host,
        "/ws/flow.ts",
        "export function alpha(a: string): void;\n\
         export function alpha(n: number) { return n; }\n",
    );
    let indexed = host
        .ensure_indexed_ready("/ws/flow.ts")
        .expect("indexed ready");
    let index = indexed.shallow_state.decl_bodies().function_program_index();
    let entry = index
        .value_function(
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "alpha",
            &verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
            1,
        )
        .expect("the implementation is indexed at ordinal 1")
        .entry();
    let view = host.resolver_store_view_read().into_owned_view();
    // The implementation's hash validates only at its own ordinal.
    assert!(view.validates(&alpha_fact_at(
        &host,
        "/ws/flow.ts",
        entry.flow_body_stable_hash,
        1,
    )));
    let mut wrong_ordinal = alpha_fact_at(&host, "/ws/flow.ts", entry.flow_body_stable_hash, 1);
    let FactVersionRef::ProgramAnalysis(ProgramAnalysisFactRef::FlowBody { function, .. }) =
        &mut wrong_ordinal
    else {
        unreachable!()
    };
    function.overload_ordinal = 0;
    assert!(
        !view.validates(&wrong_ordinal),
        "ordinal 0 is the bodiless overload — no body, no index entry, no validation"
    );
}
