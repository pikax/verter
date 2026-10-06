//! Reverse-mapped inference is owned by the relation session: the relation
//! authority's ownership check, run in this crate's lib suite where its guard
//! is bound.

#[test]
pub(crate) fn reverse_mapped_inference_is_relation_owned_in_session() {
    crate::project_semantic_dispatch::relation::reverse_ownership_tests::reverse_mapped_inference_is_relation_owned_in_session();
}
