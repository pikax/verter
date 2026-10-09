//! Source-shape guard: SCC member admission is owned by the semantic graph
//! store. Reads only engine sources, so it stays beside the store.

#[test]
fn production_relation_admission_is_semantic_store_owned() {
    let store_source = include_str!("scc_publish.rs");
    let producer_source = include_str!("../project_semantic_dispatch/relation.rs");

    // Ownership: the family singleflight publishes the SCC ROOT's entry
    // (its cold-build output IS the payload), and
    // `SemanticGraphStore::publish_scc_members_fenced` is the ONE batched
    // member-admission path both authorities' drains ride. The store owns
    // the write, its root-witness fence, and its in-flight fence; the
    // engine supplies only the computed payloads + the SCC-union carrier.
    let owner_start = store_source
        .find("\n    pub(crate) fn publish_scc_members_fenced")
        .expect("SemanticGraphStore must own the batched SCC member publish");
    let owner_body = &store_source[owner_start..];
    assert!(
        owner_body.contains("reverse_index::register_reverse_index("),
        "the member publish must land the (entries, memo_budget, reverse-index) consistency cluster"
    );
    assert!(
        owner_body.contains("SemanticQueryValue::Relation(member.payload)"),
        "the member publish must store the PUBLIC Relation payload — never a compute-side verdict"
    );
    assert!(
        !producer_source.contains("graph.insert_relation("),
        "the relation engine must supply computation and roots, never reach a raw seed write"
    );
    assert!(
        producer_source.contains(".publish_scc_members_fenced("),
        "the authority's SCC drain must ride the store-owned batched member publish"
    );
    assert!(
        store_source.lines().any(|line| line
            .trim_start()
            .starts_with("pub(crate) fn publish_scc_members_fenced<")),
        "the production relation write must be crate-private"
    );
}

mod substrate;
