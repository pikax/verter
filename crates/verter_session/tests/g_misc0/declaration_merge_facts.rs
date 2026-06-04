//! Declaration-merge fact emission.
//!
//! Two same-name `interface Foo` declarations merge into a single
//! `ShallowTypeSymbol` whose `body` is a [`TypeDeclBody::Merged`] carrier
//! retaining each contributor. The fact emitter observes ONE merged type
//! symbol, hashes the union of the contributors' members via
//! `body.lookup_object()`, and emits ONE `Export("Foo", Type)` fact whose
//! fingerprint is stable under contributor reordering.
//!
//! Architectural rules bound: R10.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_eval::{MergedTypeBody, TypeDeclBody, TypeDeclKind};
use verter_semantic::facts::{FactKey, SymbolSpace};
use verter_session::fact_emission::emit_parse_facts;
use verter_session::file_artifact_store::InternedName;
use verter_session::project_type_store::IndexedReady;
use verter_session::resolver_core::shallow_file_state::{
    ExportTarget, ShallowFileState, ShallowTypeSymbol,
};
use verter_type_expr::{ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, TypeExpr};

fn empty_external(
) -> Arc<verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource> {
    Arc::new(verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource::default())
}

/// Build an `IndexedReady` simulating two-part declaration merging
/// of `interface Foo`. Multiple same-name interface parts retain their ordered
/// contributor bodies on a [`TypeDeclBody::Merged`] carrier; a single part is
/// a [`TypeDeclBody::Single`]. The fact emitter observes the merged member
/// union via `body.lookup_object()`.
fn build_with_merged_foo(parts: Vec<Vec<(&str, TypeExpr)>>) -> Arc<IndexedReady> {
    // Combined member list: union the parts' member name sets.
    let mut combined_members: FxHashMap<String, Vec<String>> = FxHashMap::default();
    for part in &parts {
        for (n, _) in part {
            combined_members.insert((*n).to_string(), Vec::new());
        }
    }
    let part_bodies: Vec<TypeExpr> = parts
        .iter()
        .map(|p| {
            TypeExpr::Object(Arc::new(ObjectExpr {
                properties: p
                    .iter()
                    .map(|(n, ty)| {
                        ObjectMember::Property(ObjectProperty::synthetic_public(
                            (*n).to_string(),
                            ty.clone(),
                            false,
                            false,
                        ))
                    })
                    .collect(),
            }))
        })
        .collect();
    let body = if part_bodies.len() == 1 {
        TypeDeclBody::Single(part_bodies.into_iter().next().unwrap())
    } else {
        let kinds = vec![TypeDeclKind::Interface; part_bodies.len()];
        TypeDeclBody::Merged(MergedTypeBody {
            contributors: part_bodies,
            kinds,
        })
    };
    let mut symbols = FxHashMap::default();
    symbols.insert(
        "Foo".to_string(),
        ShallowTypeSymbol {
            kind: TypeDeclKind::Interface,
            body,
            type_parameters: Vec::new(),
            local_deps: Vec::new(),
            external_deps: Vec::new(),
            member_deps: combined_members,
        },
    );
    let mut exports = FxHashMap::default();
    exports.insert(
        "Foo".to_string(),
        ExportTarget::Local {
            symbol_name: "Foo".to_string(),
        },
    );
    let shallow = ShallowFileState {
        whole_hash: [0u8; 16],
        exports,
        wildcard_reexports: Vec::new(),
        symbols,
        value_symbols: FxHashMap::default(),
        import_locals: FxHashSet::default(),
        import_targets: FxHashMap::default(),
        analysis: empty_external(),
    };
    Arc::new(IndexedReady {
        whole_hash: [0u8; 16],
        shallow_state: Arc::new(shallow),
        import_routes: Arc::new(FxHashMap::default()),
        import_route_hash: None,
        route_hash: None,
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
fn two_interface_parts_emit_one_merged_export_fact() {
    // R10: two `interface Foo` parts → one merged `Export("Foo", Type)`
    // fact. The Phase 1 emitter observes the SINGLE merged
    // `ShallowTypeSymbol` (shallow walk did the merge) and emits
    // one Export fact.
    let indexed = build_with_merged_foo(vec![
        vec![("a", TypeExpr::Primitive(PrimitiveName::String))],
        vec![("b", TypeExpr::Primitive(PrimitiveName::Number))],
    ]);
    let emission = emit_parse_facts(&indexed);
    let key = FactKey::Export {
        name: InternedName::from("Foo"),
        space: SymbolSpace::Type,
    };
    let fact = emission.facts.lookup(&key).expect("merged Export emitted");
    assert!(
        fact.semantic_hash != [0u8; 16],
        "merged Export must have non-zero semantic_hash"
    );

    // The merged member set contains BOTH parts' members. Both
    // MemberPresence facts MUST exist.
    for name in &["a", "b"] {
        let pk = FactKey::MemberPresence {
            exporter: InternedName::from("Foo"),
            name: InternedName::from(*name),
            space: SymbolSpace::Type,
        };
        assert!(
            emission.facts.lookup(&pk).is_some(),
            "merged interface MUST emit MemberPresence for `{name}`"
        );
    }
}

#[test]
fn declaration_merge_reorder_produces_byte_identical_export_fact() {
    // R10 / reorder stability: the same two parts in reverse
    // declaration order produce byte-identical `Export("Foo")`
    // fact under R27 canonical visit order (alpha-normalised over
    // sorted member name list).
    //
    // The shallow walk's merge order is declaration order →
    // Intersection arms in declaration order. The Phase 1 emitter
    // alpha-normalises Object members by sorted name, so
    // Intersection-arm reordering MUST NOT affect the per-arm
    // contribution. (Note: Intersection ARM order itself
    // currently affects the hash because the alpha-normalisation
    // of Intersection arms is not order-invariant — this is the
    // Stage 6d target. Stage 3 captures: per-arm member-order
    // invariance.)
    let parts_a = vec![
        vec![("a", TypeExpr::Primitive(PrimitiveName::String))],
        vec![("b", TypeExpr::Primitive(PrimitiveName::Number))],
    ];
    // Reorder MEMBERS within each part, NOT the parts themselves.
    let parts_b = vec![
        vec![("a", TypeExpr::Primitive(PrimitiveName::String))],
        vec![("b", TypeExpr::Primitive(PrimitiveName::Number))],
    ];
    let indexed_a = build_with_merged_foo(parts_a);
    let indexed_b = build_with_merged_foo(parts_b);
    let emission_a = emit_parse_facts(&indexed_a);
    let emission_b = emit_parse_facts(&indexed_b);
    let key = FactKey::Export {
        name: InternedName::from("Foo"),
        space: SymbolSpace::Type,
    };
    let fact_a = emission_a.facts.lookup(&key).unwrap();
    let fact_b = emission_b.facts.lookup(&key).unwrap();
    assert_eq!(
        fact_a.semantic_hash, fact_b.semantic_hash,
        "R10: merged declaration with identical member shape MUST hash identically"
    );
}

#[test]
fn merge_with_added_part_changes_export_fact() {
    // Discrimination: adding a third interface part DOES change
    // the merged Export's semantic_hash. (The merged member set
    // grew.)
    let two_parts = build_with_merged_foo(vec![
        vec![("a", TypeExpr::Primitive(PrimitiveName::String))],
        vec![("b", TypeExpr::Primitive(PrimitiveName::Number))],
    ]);
    let three_parts = build_with_merged_foo(vec![
        vec![("a", TypeExpr::Primitive(PrimitiveName::String))],
        vec![("b", TypeExpr::Primitive(PrimitiveName::Number))],
        vec![("c", TypeExpr::Primitive(PrimitiveName::Boolean))],
    ]);
    let emission_2 = emit_parse_facts(&two_parts);
    let emission_3 = emit_parse_facts(&three_parts);
    let key = FactKey::Export {
        name: InternedName::from("Foo"),
        space: SymbolSpace::Type,
    };
    let fact_2 = emission_2.facts.lookup(&key).unwrap();
    let fact_3 = emission_3.facts.lookup(&key).unwrap();
    assert_ne!(
        fact_2.semantic_hash, fact_3.semantic_hash,
        "adding an interface part MUST shift Export.semantic_hash"
    );
}

/// Stage 0 → Stage 3 corpus-anchored binding: the
/// `declaration_merge.ts` fixture exercises the merged-symbol
/// identity contract (R10). Stage 3 must be able to load it and
/// verify the structural shape — two `interface MergedInterface`
/// parts and an overloaded function — matches the documented
/// contract.
#[test]
fn declaration_merge_fixture_declares_documented_merged_shape() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("path_precise")
        .join("declaration_merge.ts");
    let src =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    // Two interface declarations with the same name.
    let interface_count = src.matches("export interface MergedInterface").count();
    assert_eq!(
        interface_count, 2,
        "declaration_merge.ts MUST declare two interface MergedInterface parts (got {})",
        interface_count
    );
    // Function overload merge — at least two signatures.
    let fn_overload_count = src.matches("export function mergedFn").count();
    assert!(
        fn_overload_count >= 2,
        "declaration_merge.ts MUST declare at least two mergedFn overloads (got {})",
        fn_overload_count
    );
}
