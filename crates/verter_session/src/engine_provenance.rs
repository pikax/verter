//! The semantic engine's provenance counters.

/// Width of the per-variant push-count array in [`EngineProvenance`]: the
/// exclusive bound of [`crate::semantic_query::SemanticNodeTag::bucket_index`].
pub const SEMANTIC_NODE_DATA_DISCRIMINANT_COUNT: usize =
    crate::semantic_query::SEMANTIC_NODE_TAG_BOUND;

/// The semantic engine's work counters: the deterministic observability rail
/// for the semantic graph, its cooperative executor and the memo fact tracer.
/// Owned by the engine and shared (one `Arc`) by every graph store and
/// observer set a host builds; the host's provenance facade aggregates it.
/// Relaxed increments, reset only by the facade.
pub struct EngineProvenance {
    /// Count of dispatch fact fan-outs emitted from the slot-binding graph.
    /// The traversal has no cache boundary of its own, so behavioral tests use
    /// this counter to prove its dependency evidence reached the request tracer.
    pub slot_binding_graph_fact_tracer_emissions: std::sync::atomic::AtomicU64,
    /// Count of `observe_fact_signature` fan-out calls emitted from
    /// `meta_resolve::dep_signature::emit_dispatch_dep_signature_facts`
    /// — the helper invoked by dispatch reads that
    /// have no result cache of their own (the projector sites,
    /// `materialize_component_meta_type_expr_until_stable_full` and
    /// `node_root_reaches_transitive_cycle_with_fence`). The helper bumps this
    /// counter on every `observe_fact_signature` call.
    pub dispatch_dep_signature_fact_tracer_emissions: std::sync::atomic::AtomicU64,
    /// `SemanticGraphStore::execute_cooperative` calls that became the cold
    /// owner (claimed in-flight slot).
    pub execute_cooperative_owner_path: std::sync::atomic::AtomicU64,
    /// `execute_cooperative` calls that joined an in-flight build.
    pub execute_cooperative_joiner_path: std::sync::atomic::AtomicU64,
    /// Time the cold owner held the in-flight slot (build duration).
    pub execute_cooperative_held_ns: std::sync::atomic::AtomicU64,
    /// `NodeArena::push_impl` total call count (every push, exempt or not).
    pub node_arena_pushes: std::sync::atomic::AtomicU64,
    /// `NodeArena::push_impl` calls that allocated a new arena slot
    /// (equal to `node_arena_pushes` while every push allocates; diverges
    /// once structural interning serves a push from an existing slot).
    pub node_arena_intern_miss: std::sync::atomic::AtomicU64,
    /// Time spent waiting on `ArenaInner` mutex acquisition during pushes
    /// (lock-contention observability counter).
    pub node_arena_inner_write_wait_ns: std::sync::atomic::AtomicU64,
    /// Per-`SemanticNodeData` variant push count, indexed by
    /// `SemanticNodeTag::bucket_index()`. Sized to
    /// [`SEMANTIC_NODE_DATA_DISCRIMINANT_COUNT`] for variant headroom.
    pub node_arena_pushes_per_discriminant:
        [std::sync::atomic::AtomicU64; SEMANTIC_NODE_DATA_DISCRIMINANT_COUNT],
    /// `install_fact_tracer` wrap count for `MemoEntry` (semantic
    /// query memo cold builds).
    pub memo_entry_fact_tracer_installs: std::sync::atomic::AtomicU64,
    /// `install_fact_tracer` overflow-refusal count for `MemoEntry`.
    pub memo_entry_overflow_refusals: std::sync::atomic::AtomicU64,
}

impl Default for EngineProvenance {
    fn default() -> Self {
        Self {
            slot_binding_graph_fact_tracer_emissions: std::sync::atomic::AtomicU64::new(0),
            dispatch_dep_signature_fact_tracer_emissions: std::sync::atomic::AtomicU64::new(0),
            execute_cooperative_owner_path: std::sync::atomic::AtomicU64::new(0),
            execute_cooperative_joiner_path: std::sync::atomic::AtomicU64::new(0),
            execute_cooperative_held_ns: std::sync::atomic::AtomicU64::new(0),
            node_arena_pushes: std::sync::atomic::AtomicU64::new(0),
            node_arena_intern_miss: std::sync::atomic::AtomicU64::new(0),
            node_arena_inner_write_wait_ns: std::sync::atomic::AtomicU64::new(0),
            node_arena_pushes_per_discriminant: std::array::from_fn(|_| {
                std::sync::atomic::AtomicU64::new(0)
            }),
            memo_entry_fact_tracer_installs: std::sync::atomic::AtomicU64::new(0),
            memo_entry_overflow_refusals: std::sync::atomic::AtomicU64::new(0),
        }
    }
}
