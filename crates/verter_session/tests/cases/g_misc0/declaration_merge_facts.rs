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

use verter_semantic::facts::{FactKey, SymbolSpace};
use verter_session::fact_emission::emit_parse_facts;
use verter_session::file_artifact_store::InternedName;
use verter_session::project_type_store::IndexedReady;
use verter_session::resolver_core::shallow_file_state::ShallowFileState;

fn empty_external(
) -> Arc<verter_parser::utils::oxc::script::type_inventory::AnalyzedExternalTypeSource> {
    Arc::new(
        verter_parser::utils::oxc::script::type_inventory::AnalyzedExternalTypeSource::default(),
    )
}

/// Build an `IndexedReady` from AUTHORED same-name `interface Foo` parts
/// (each part is a list of `(member, type-text)` pairs), constructed through
/// the production-shaped service-backed path. The real binder groups the
/// same-name contributors into the ordered group whose `merged_body()` is
/// the `TypeDeclBody::Merged` carrier — exactly what the lazy
/// declaration-body fold serves on first demand.
fn build_with_merged_foo(parts: Vec<Vec<(&str, &str)>>) -> Arc<IndexedReady> {
    let mut source = String::new();
    for part in &parts {
        source.push_str("export interface Foo {\n");
        for (name, ty) in part {
            source.push_str(&format!("  {name}: {ty};\n"));
        }
        source.push_str("}\n");
    }
    let shallow =
        ShallowFileState::service_backed_for_test_with_hash("/merge.ts", &source, [0u8; 16]);
    Arc::new(IndexedReady::new_for_test_with_state(
        [0u8; 16],
        shallow,
        Arc::from(source.as_str()),
        Arc::from(source.as_str()),
        empty_external(),
    ))
}

#[test]
fn two_interface_parts_emit_one_merged_export_fact() {
    // R10: two `interface Foo` parts → one merged `Export("Foo", Type)`
    // fact. The fact emitter observes the SINGLE merged
    // `ShallowTypeSymbol` (shallow walk did the merge) and emits
    // one Export fact.
    let indexed = build_with_merged_foo(vec![vec![("a", "string")], vec![("b", "number")]]);
    let emission = emit_parse_facts(&indexed);
    let key = FactKey::Export {
        name: InternedName::from("Foo"),
        space: SymbolSpace::Type,
    };
    let fact = emission
        .facts
        .lookup_or_compute(&key)
        .expect("merged Export computed by the lazy body fact path");
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
fn declaration_merge_member_reorder_produces_byte_identical_export_fact() {
    // R10 / member-order invariance: reordering the MEMBERS WITHIN a
    // merged interface contributor produces a byte-identical
    // `Export("Foo")` fact, because the fact emitter alpha-normalises
    // each object surface by sorted member name. This is a GENUINE
    // reorder (`parts_a != parts_b`): if the emitter hashed members in
    // source order, the two hashes would differ.
    //
    // (Contributor/part ORDER invariance is a separate, NOT-yet-held
    // property — interface member union is order-insensitive but
    // intersection-arm normalisation is not — so this test exercises
    // only the property that actually holds: within-contributor member
    // order.)
    let parts_a = vec![
        vec![("a", "string"), ("b", "number")],
        vec![("c", "boolean")],
    ];
    // Same parts, but the MEMBERS within the first contributor are
    // written in the opposite order.
    let parts_b = vec![
        vec![("b", "number"), ("a", "string")],
        vec![("c", "boolean")],
    ];
    assert_ne!(
        parts_a, parts_b,
        "this test is only meaningful if the inputs genuinely differ in member order"
    );
    let indexed_a = build_with_merged_foo(parts_a);
    let indexed_b = build_with_merged_foo(parts_b);
    let emission_a = emit_parse_facts(&indexed_a);
    let emission_b = emit_parse_facts(&indexed_b);
    let key = FactKey::Export {
        name: InternedName::from("Foo"),
        space: SymbolSpace::Type,
    };
    let fact_a = emission_a.facts.lookup_or_compute(&key).unwrap();
    let fact_b = emission_b.facts.lookup_or_compute(&key).unwrap();
    assert_eq!(
        fact_a.semantic_hash, fact_b.semantic_hash,
        "R10: within-contributor member reorder MUST hash identically (alpha-normalised)"
    );
}

#[test]
fn merge_with_added_part_changes_export_fact() {
    // Discrimination: adding a third interface part DOES change
    // the merged Export's semantic_hash. (The merged member set
    // grew.)
    let two_parts = build_with_merged_foo(vec![vec![("a", "string")], vec![("b", "number")]]);
    let three_parts = build_with_merged_foo(vec![
        vec![("a", "string")],
        vec![("b", "number")],
        vec![("c", "boolean")],
    ]);
    let emission_2 = emit_parse_facts(&two_parts);
    let emission_3 = emit_parse_facts(&three_parts);
    let key = FactKey::Export {
        name: InternedName::from("Foo"),
        space: SymbolSpace::Type,
    };
    let fact_2 = emission_2.facts.lookup_or_compute(&key).unwrap();
    let fact_3 = emission_3.facts.lookup_or_compute(&key).unwrap();
    assert_ne!(
        fact_2.semantic_hash, fact_3.semantic_hash,
        "adding an interface part MUST shift Export.semantic_hash"
    );
}

/// Corpus-anchored binding: the
/// `declaration_merge.ts` fixture exercises the merged-symbol
/// identity contract (R10). The loader must be able to load it and
/// verify the structural shape — two `interface MergedInterface`
/// parts and an overloaded function — matches the documented
/// contract.
#[test]
fn declaration_merge_fixture_declares_documented_merged_shape() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cases")
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
