// A real build-dependency consumer, outside workspace dev-feature unification.
fn main() {
    let host = verter_session::VerterHost::new_standalone(Default::default());
    assert_eq!(host.work_snapshot().resolver_node_cache_misses, 0);
    assert_eq!(
        host.project_type_store()
            .semantic_graph()
            .work_snapshot()
            .misses,
        0
    );
}
