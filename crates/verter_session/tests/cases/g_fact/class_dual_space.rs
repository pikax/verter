//! Dual-space class-symbol guard (`R11`).
//!
//! A `class Foo` declaration occupies BOTH the type space and the
//! value space in TypeScript: `Foo` is a type (`let x: Foo`) AND a
//! value (`new Foo()`, `typeof Foo`). The fact model encodes this as
//! TWO distinct `FactKey::Export` facts — one keyed `("Foo", Type)`
//! and one keyed `("Foo", Value)` — and FORBIDS a single fused
//! `BothTypeValue` symbol space (`SymbolSpace` has no such variant;
//! see `verter_semantic/src/facts/registry.rs`).
//!
//! This guard drives the ACTUAL `class Foo` lowering / shallow-analysis
//! path end-to-end (`analyze_external_type_source` →
//! `parse_and_build_env` → `ShallowFileState::from_analysis_with_resolver` →
//! `emit_parse_facts`) and asserts both `Export` facts serve under
//! distinct spaces on first observation through the lazy body fact
//! path (`FileFacts::lookup_or_compute` — the publish-time registry
//! itself is header-only). It FAILS if class
//! lowering ever collapses to a single space (e.g. only `Type`, only
//! `Value`, or a fused space) — which is strictly stronger than the
//! manual-insert non-collision check
//! `type_and_value_namespace_keys_are_distinct` in the registry unit
//! tests, because it exercises the real declaration-extraction path
//! rather than two hand-built keys.

use std::sync::Arc;

use verter_semantic::facts::{FactKey, SymbolSpace};
use verter_session::fact_emission::emit_parse_facts;
use verter_session::file_artifact_store::InternedName;
use verter_session::project_type_store::IndexedReady;
use verter_session::resolver_core::shallow_file_state::ShallowFileState;

/// Build a real `IndexedReady` from `source` by driving the actual
/// service-backed shallow-analysis lowering path (no hand-built
/// `ShallowFileState`).
fn indexed_from_source(source: &str) -> Arc<IndexedReady> {
    let shallow =
        ShallowFileState::service_backed_for_test_with_hash("/dual-space.ts", source, [0u8; 16]);

    let empty_external = Arc::new(
        verter_parser::utils::oxc::script::type_surface::AnalyzedExternalTypeSource::default(),
    );

    Arc::new(IndexedReady::new_for_test_with_state(
        [0u8; 16],
        shallow,
        Arc::from(source),
        Arc::from(source),
        empty_external,
    ))
}

#[test]
fn class_dual_space_emits_two_symbols() {
    // Drive the real `class Foo` lowering path through shallow analysis
    // and parse-fact emission.
    let source = "export class Foo {\n  bar(): number { return 1 }\n}\n";
    let indexed = indexed_from_source(source);
    let emission = emit_parse_facts(&indexed);
    let facts = &emission.facts;

    let name = InternedName::from("Foo");
    let type_key = FactKey::Export {
        name: name.clone(),
        space: SymbolSpace::Type,
    };
    let value_key = FactKey::Export {
        name: name.clone(),
        space: SymbolSpace::Value,
    };

    // Body-sensitive `Export` facts are LAZY: the publish-time
    // registry omits them (publish lowers zero declaration bodies);
    // first observation computes them through the declaration-body
    // path (`lookup_or_compute`).
    let type_fact = facts.lookup_or_compute(&type_key);
    let value_fact = facts.lookup_or_compute(&value_key);

    assert!(
        type_fact.is_some(),
        "class `Foo` lowering MUST serve an `Export(\"Foo\", Type)` fact \
         on first observation — the class is usable as a type \
         (`let x: Foo`). The eager registry held: {:?}",
        facts.registry().iter().map(|(k, _)| k).collect::<Vec<_>>(),
    );
    assert!(
        value_fact.is_some(),
        "class `Foo` lowering MUST serve an `Export(\"Foo\", Value)` fact \
         on first observation — the class is usable as a value \
         (`new Foo()`, `typeof Foo`). The eager registry held: {:?}",
        facts.registry().iter().map(|(k, _)| k).collect::<Vec<_>>(),
    );

    // Exactly the two spaces, no `Namespace`-space `Export(Foo, …)`
    // leaks from a plain class declaration.
    let namespace_key = FactKey::Export {
        name,
        space: SymbolSpace::Namespace,
    };
    assert!(
        facts.lookup_or_compute(&namespace_key).is_none(),
        "a plain `class Foo` must NOT serve a `Namespace`-space `Export` \
         fact — class lowering occupies exactly Type + Value, not the \
         namespace space",
    );
}
