//! Admission decisions over real resolve-domain witness facts, which only a
//! live session host can mint.

use std::sync::Arc;

use crate::semantic_query::admit::{admit_decision, Admission};
use crate::semantic_query::{BrokenInputClass, ResultTaint};
use verter_session_query::facts::fact_cache::{
    FactVersionRef, ReadSetSignature, ResolveImportsFactRef,
};
use verter_session_query::facts::registry::{FactKey, FactLane, InternedName, SymbolSpace};

fn sig_from(facts: Vec<FactVersionRef>) -> ReadSetSignature {
    ReadSetSignature::new(Arc::from(facts.into_boxed_slice()))
}

fn import_route_fact() -> FactVersionRef {
    // The import-route rooting rail is a REAL resolve-domain
    // resolution witness (a forged one is impossible by design —
    // `ResolutionFactRef`'s fields are sealed to `verter_workspace`).
    crate::for_tests::resolution_witness_fact_for_tests()
}

fn negative_resolved_import_fact() -> FactVersionRef {
    use verter_session_query::facts::registry::InternedSpecifier;
    FactVersionRef::ResolveImports(ResolveImportsFactRef::Semantic {
        canonical_id: "/importer.ts".to_string(),
        key: FactKey::ResolvedImportClause {
            specifier: InternedSpecifier::from("./missing"),
            binding: InternedName::from("X"),
            space: SymbolSpace::Type,
            resolved_canonical: Arc::from(
                verter_session_query::resolution::unresolved::UNRESOLVED_SENTINEL,
            ),
            resolved_source_name: InternedName::from("X"),
        },
        lane: FactLane::Semantic,
        expected_hash: [3u8; 16],
    })
}

#[test]
fn missing_dependency_warm_only_with_import_route_fact() {
    // Fact recorded → Warm (the invalidation rail exists).
    assert_eq!(
        admit_decision(
            ResultTaint::Partial(BrokenInputClass::MissingDependency),
            &sig_from(vec![import_route_fact()]),
        ),
        Admission::Warm
    );
    // Fact absent → ReturnOnly (no rail; a positive FileWholeHash must
    // not stand in for the missing-dep rail).
    assert_eq!(
        admit_decision(
            ResultTaint::Partial(BrokenInputClass::MissingDependency),
            &sig_from(vec![FactVersionRef::FileWholeHash {
                canonical_id: "/x.ts".to_string(),
                hash: [1u8; 16],
            }]),
        ),
        Admission::ReturnOnly
    );
}

#[test]
fn unresolved_reference_warm_only_with_negative_resolution_fact() {
    assert_eq!(
        admit_decision(
            ResultTaint::Partial(BrokenInputClass::UnresolvedReference),
            &sig_from(vec![negative_resolved_import_fact()]),
        ),
        Admission::Warm
    );
    assert_eq!(
        admit_decision(
            ResultTaint::Partial(BrokenInputClass::UnresolvedReference),
            &sig_from(vec![]),
        ),
        Admission::ReturnOnly
    );
}

#[test]
fn unstable_and_broken_classes_are_returnonly_regardless_of_facts() {
    let rich = sig_from(vec![import_route_fact(), negative_resolved_import_fact()]);
    for taint in [
        ResultTaint::Partial(BrokenInputClass::IncompleteDeclaration),
        ResultTaint::Partial(BrokenInputClass::SyntaxError),
        ResultTaint::Partial(BrokenInputClass::TornRead),
        ResultTaint::Broken(BrokenInputClass::SyntaxError),
        ResultTaint::Broken(BrokenInputClass::TornRead),
        ResultTaint::Broken(BrokenInputClass::MissingDependency),
    ] {
        assert_eq!(
            admit_decision(taint, &rich),
            Admission::ReturnOnly,
            "{taint:?} must be ReturnOnly even with a rich fact rail"
        );
    }
}
