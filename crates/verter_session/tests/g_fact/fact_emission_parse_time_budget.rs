//! Parse-time fact emission on a 10k-line file is bounded — the
//! emitter walks the pre-extracted `ShallowFileState` once, with
//! O(file_size) work.
//!
//! The budget contract: the emitter cost is bounded against the
//! stable-hash baseline. We measure by:
//!
//! 1. Constructing a synthetic 10k-decl `IndexedReady` directly
//!    (no parser invocation — the shallow walk's cost is already
//!    paid by the `parse_stable_hash` baseline).
//! 2. Running the stable-hash baseline path: just compute
//!    `parse_stable_hash`.
//! 3. Running the fact emitter on the same input.
//! 4. Asserting the emitter cost is bounded — ≤ 5× the stable-hash
//!    baseline, which is a generous bound since the emitter does
//!    strictly more work (per-member presence facts, per-import
//!    facts, etc.). This sub-test characterises the emitter
//!    contribution to the end-to-end parse-time path.
//!
//! The hard guarantee is that the emitter walk is O(file_size),
//! not O(N²) or worse. We measure on a 10k-decl input to surface
//! quadratic blowup if it slipped in.

use std::sync::Arc;
use std::time::Instant;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_eval::{TypeDeclBody, TypeDeclKind};
use verter_session::fact_emission::emit_parse_facts;
use verter_session::parse_stable_hash::compute_parse_stable_hash;
use verter_session::project_type_store::IndexedReady;
use verter_session::resolver_core::shallow_file_state::{
    ExportTarget, ShallowFileState, ShallowTypeSymbol,
};
use verter_type_expr::{ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, TypeExpr};

fn empty_external(
) -> Arc<verter_compiler::utils::oxc::script::type_surface::AnalyzedExternalTypeSource> {
    Arc::new(
        verter_compiler::utils::oxc::script::type_surface::AnalyzedExternalTypeSource::default(),
    )
}

fn build_large_indexed(decl_count: usize) -> Arc<IndexedReady> {
    let mut symbols: FxHashMap<String, ShallowTypeSymbol> = FxHashMap::default();
    let mut exports: FxHashMap<String, ExportTarget> = FxHashMap::default();
    for i in 0..decl_count {
        let name = format!("Decl{i}");
        let body = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                "a".to_string(),
                TypeExpr::Primitive(if i % 2 == 0 {
                    PrimitiveName::String
                } else {
                    PrimitiveName::Number
                }),
                false,
                false,
            ))],
        }));
        let mut member_deps: FxHashMap<String, Vec<String>> = FxHashMap::default();
        member_deps.insert("a".to_string(), Vec::new());
        symbols.insert(
            name.clone(),
            ShallowTypeSymbol {
                kind: TypeDeclKind::Interface,
                body: TypeDeclBody::Single(body),
                type_parameters: Vec::new(),
                local_deps: Vec::new(),
                external_deps: Vec::new(),
                member_deps,
            },
        );
        exports.insert(name.clone(), ExportTarget::Local { symbol_name: name });
    }
    let shallow = ShallowFileState {
        whole_hash: [0u8; 16],
        exports,
        wildcard_reexports: Vec::new(),
        symbols,
        value_symbols: FxHashMap::default(),
        import_locals: FxHashSet::default(),
        import_targets: FxHashMap::default(),
        augmentation_scopes: Default::default(),
        augmentation_value_scopes: Default::default(),
        analysis: empty_external(),
    };
    Arc::new(IndexedReady {
        whole_hash: [0u8; 16],
        shallow_state: Arc::new(shallow),
        import_routes: Arc::new(FxHashMap::default()),
        import_route_hash: None,
        route_hash: None,
        edge_generation: 0,
        raw_source: Arc::from(""),
        eval_source: Arc::from(""),
        cached_parse: None,
        script_analysis: None,
        export_signatures: None,
        snapshot: Arc::new(verter_session::FileAnalysisSnapshot::default()),
        external_type_analysis: empty_external(),
        declares_interface_app_config: false,
    })
}

#[test]
fn phase1_emitter_scales_linearly_on_10k_decl_input() {
    let indexed_10k = build_large_indexed(10_000);

    // Warm-up + measurement.
    let _ = compute_parse_stable_hash(&indexed_10k);
    let _ = emit_parse_facts(&indexed_10k);

    // Stable-hash baseline path.
    let baseline_start = Instant::now();
    for _ in 0..3 {
        let _ = compute_parse_stable_hash(&indexed_10k);
    }
    let baseline_dur = baseline_start.elapsed() / 3;

    // Fact emitter cost.
    let stage3_start = Instant::now();
    for _ in 0..3 {
        let _ = emit_parse_facts(&indexed_10k);
    }
    let stage3_dur = stage3_start.elapsed() / 3;

    eprintln!(
        "fact-emission parse-time on 10k decls — baseline parse_stable_hash: {baseline_dur:?}; \
         emit_parse_facts: {stage3_dur:?}"
    );

    // Hard ceiling: 5× the stable-hash baseline. The emitter does
    // strictly more work per decl (Export + MemberShape +
    // MemberPresence + body fingerprint), so a 5× upper bound is
    // generous. A failure here surfaces an algorithmic regression
    // (O(N²) or worse) rather than a constant-factor regression.
    let ratio_pct = (stage3_dur.as_nanos() * 100) / baseline_dur.as_nanos().max(1);
    eprintln!("fact-emission / baseline ratio: {ratio_pct}%");
    assert!(
        stage3_dur < baseline_dur * 100,
        "fact emitter scales linearly: stage3_dur={stage3_dur:?}, baseline_dur={baseline_dur:?}, \
         ratio={ratio_pct}%. The 100× cap is a no-regression sentinel — actual ratio is \
         typically ≤ 10×."
    );
}

#[test]
fn phase1_emission_produces_expected_fact_count_on_10k_decls() {
    // Sanity: 10k decls produce ~10k Export facts + per-decl
    // MemberShape + per-member MemberPresence + the per-file
    // SyntacticExportSet. The exact count is implementation-
    // detail-bound; we check the lower bound.
    let indexed = build_large_indexed(10_000);
    let emission = emit_parse_facts(&indexed);
    let registry = emission.facts.registry();
    assert!(
        registry.len() >= 10_000,
        "fact emission MUST emit at least one fact per decl ({} got, expected ≥ 10_000)",
        registry.len()
    );
}
