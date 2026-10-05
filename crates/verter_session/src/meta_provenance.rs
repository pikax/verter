//! Per-host provenance counters for component-meta and semantic-engine observability.

/// Width of the per-variant push-count array in [`MetaProvenance`]: the
/// exclusive bound of [`crate::semantic_query::SemanticNodeTag::bucket_index`].
pub const SEMANTIC_NODE_DATA_DISCRIMINANT_COUNT: usize =
    crate::semantic_query::SEMANTIC_NODE_TAG_BOUND;

/// Per-host provenance counters for component-meta observability.
///
/// AtomicU64 for thread-safe increment. Reset on host close. Not persisted.
/// Host tests read counters directly via `host.provenance()`.
///
/// The `ensure_loaded_*`, `execute_cooperative_*`, `overlay_gate_*`,
/// and `node_arena_*` families count cooperative-execute path
/// selection, intern hot-path activity, and lock hold/wait time so
/// future tuning passes (e.g., interner sharding) can be
/// evidence-driven.
pub struct MetaProvenance {
    pub get_component_meta_calls: std::sync::atomic::AtomicU64,
    pub component_meta_resolved_state_recomputes: std::sync::atomic::AtomicU64,
    pub get_analysis_calls: std::sync::atomic::AtomicU64,
    pub evaluate_types_calls: std::sync::atomic::AtomicU64,
    /// Bumped on every `VerterHost::upsert(...)` call. Used by
    /// `tests/cases/g_session/session_view_isolation.rs` to assert the R17
    /// invariant that session query paths do NOT mutate the host.
    pub host_upsert_calls: std::sync::atomic::AtomicU64,
    /// Bumped on every cache-key derivation that consulted a
    /// [`crate::session_view::SessionView`] via
    /// `view.content_hash_for(canonical)` rather than the base host's
    /// `shallow_file_state(canonical).whole_hash`. Used by
    /// `tests/cases/g_session/session_view_warm_reuse.rs` to assert R17/R18 (the
    /// consumer path is wired through `SessionView`).
    pub view_aware_cache_key_lookups: std::sync::atomic::AtomicU64,
    pub resolved_external_type_cache_hits: std::sync::atomic::AtomicU64,
    pub resolved_external_type_cache_misses: std::sync::atomic::AtomicU64,
    pub resolver_node_cache_hits: std::sync::atomic::AtomicU64,
    pub resolver_node_cache_misses: std::sync::atomic::AtomicU64,
    pub resolver_singleflight_coalesced: std::sync::atomic::AtomicU64,
    pub resolver_cross_view_lane_forks: std::sync::atomic::AtomicU64,
    pub resolver_cycle_detections: std::sync::atomic::AtomicU64,
    pub resolver_route_fact_reuse: std::sync::atomic::AtomicU64,
    pub resolver_barrel_fact_reuse: std::sync::atomic::AtomicU64,
    pub import_resolution_cache_hit_count: std::sync::atomic::AtomicU64,
    pub import_resolution_cache_miss_count: std::sync::atomic::AtomicU64,
    pub dir_index_hit_count: std::sync::atomic::AtomicU64,
    pub dir_index_refresh_count: std::sync::atomic::AtomicU64,
    pub dir_index_dirty_rescan_count: std::sync::atomic::AtomicU64,
    pub native_fs_read_dir_count: std::sync::atomic::AtomicU64,
    pub native_fs_read_file_miss_count: std::sync::atomic::AtomicU64,
    pub payload_cache_hits: std::sync::atomic::AtomicU64,
    pub payload_cache_misses: std::sync::atomic::AtomicU64,
    pub payload_encodes: std::sync::atomic::AtomicU64,
    /// Count of session-overlay RE-ROOTS performed against THIS host —
    /// incremented once per
    /// [`crate::resolver_store::HostStoreView::with_session_overlay`] call
    /// that reaches the `Arc::make_mut` re-root path (a non-empty overlay
    /// or tombstone set). `Arc::make_mut` clones the shared
    /// `StoreViewSnapshot` only when the `Arc` is actually shared
    /// (refcount > 1); a uniquely-owned snapshot is mutated in place — so
    /// this counter is an UPPER BOUND on full snapshot clones, counting
    /// every entry into the re-root work (clone or in-place), which is
    /// exactly the per-application cost the O(1) batch contract bounds. A
    /// no-op (empty-overlay) application keeps the shared base `Arc`
    /// untouched and does NOT bump this counter.
    ///
    /// PER-HOST (not process-global): every `with_session_overlay` call
    /// already carries the `&VerterHost` it overlays, and every rayon
    /// worker in a host batch operates on the SAME host, so this counter
    /// observes worker-side per-job COWs while staying immune to other
    /// hosts' (other tests') overlay activity. A component-meta batch over
    /// an overlay session must apply the overlay ONCE per batch (the
    /// per-batch capture) and SHARE it across all N jobs, so a warm or
    /// cold batch of N performs O(1) overlay COWs on this host; a per-job
    /// re-application drives it O(N) — the regression
    /// `batch_over_overlay_session_applies_overlay_o1_not_per_job` gates
    /// against.
    pub session_overlay_cows: std::sync::atomic::AtomicU64,
    /// Count of FULL overlay-set fingerprint computations performed
    /// against THIS host — incremented once per
    /// [`crate::session_view::overlay_set_fingerprint`] call that walks
    /// the overlay-hash table (collect + sort by canonical + FxHash). An
    /// overlay-bearing view's
    /// [`crate::session_view::SessionView::fingerprint`] is a PURE
    /// function of the view's immutable overlay maps, so the full
    /// computation runs ONCE — at view construction — and every later
    /// `fingerprint()` read returns the memoized `u64` with no recompute.
    ///
    /// PER-HOST (not process-global): an overlay view always carries the
    /// `&VerterHost` it overlays, and every rayon worker in a host batch
    /// reads the SAME shared view's memoized fingerprint, so this counter
    /// observes worker-side reads while staying immune to other hosts'
    /// (other tests') fingerprinting. A component-meta batch over an
    /// overlay session constructs ONE view per batch and shares it across
    /// all N jobs, so a warm or cold batch of N performs O(1) full
    /// fingerprint computations on this host; recomputing per `cache_key`
    /// / per warm-probe / per store would drive it O(N) — the regression
    /// `batch_over_overlay_session_computes_fingerprint_o1_not_per_job`
    /// gates against.
    pub overlay_set_fingerprint_full_computations: std::sync::atomic::AtomicU64,
    /// Count of [`crate::resolver_store::HostStoreView::from_host_read`]
    /// entries against THIS host — every store-view read a warm-cache
    /// validator or batch capture performs, whether the manager serves it
    /// as a cheap token-stable `Arc` clone or a full sweep.
    ///
    /// PER-HOST (not process-global): `from_host_read` already carries the
    /// `&VerterHost` it reads, and every rayon worker in a host batch reads
    /// through the SAME host, so this counter observes worker-side per-job
    /// reads while staying immune to other hosts' (other tests')
    /// store-view traffic. A warm component-meta batch of N must collapse
    /// onto O(1) reads (the single per-batch fixed-view capture); a
    /// per-job-read path drives it ≥ N — the regressions
    /// `warm_batch_payload_from_host_calls_are_o1_not_per_item` /
    /// `warm_analysis_batch_from_host_calls_are_o1_not_per_item` gate
    /// against. The process-global per-call-site table
    /// (`dump_from_host_call_sites`) remains the bench-side ATTRIBUTION
    /// diagnostic; this counter is the hermetic per-host MEASUREMENT.
    pub store_view_from_host_reads: std::sync::atomic::AtomicU64,
    /// `ComponentMetaResultDb::get_with_view` warm-hit count. Bumped
    /// once per call that returns `Some(entry)` after the entry's
    /// `fact_dep_signature` validates under the supplied
    /// [`verter_session_query::facts::store_view::StoreView`]. Used by behavioural
    /// tests to discriminate fact-validation from eager-invalidation:
    /// an entry that survives an unrelated edit must advance this
    /// counter on the second call.
    pub component_meta_result_cache_hits: std::sync::atomic::AtomicU64,
    /// `ComponentMetaResultDb::get_with_view` miss count. Bumped on
    /// every call that returns `None` — whether the entry was absent
    /// from the map OR the entry's `fact_dep_signature` failed
    /// validation. Used by tests to discriminate cache-bypass via
    /// the validator: editing a dep MUST advance this counter on
    /// the second call.
    pub component_meta_result_cache_misses: std::sync::atomic::AtomicU64,
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
    pub indexed_ready_scheduler_snapshot_reuse: std::sync::atomic::AtomicU64,
    pub bundle_cache_hits: std::sync::atomic::AtomicU64,
    /// Request-world prepared-decl bundle memo hits — bumped when a
    /// bundle read is served from the request's `RequestBundleMemo`
    /// instead of re-running its materialisation, in either world
    /// (`Base` or `Overlay`).
    ///
    /// The sibling of `bundle_cache_hits`, and the ONLY observable for
    /// the two classes the shared `prepared_decl_bundles` cache cannot
    /// hold: an overlay-bearing bundle (R17) and a `RequestOnly` one
    /// (a deterministic non-cacheable read in its basis). For a `Shared`
    /// bundle the shared cache would already have served the later
    /// touches, so this counter — not `bundle_cold_flight_runs` — is what
    /// measures the memo there. The end-to-end wiring regression asserts
    /// it moves under a real session-view component-meta request.
    pub bundle_request_memo_hits: std::sync::atomic::AtomicU64,
    pub bundle_materializations: std::sync::atomic::AtomicU64,
    /// Cold bundle flight-body executions: the singleflight lane's cold
    /// run past the in-flight recheck (the deterministic mirror of
    /// `AuditEvent::PreparedDeclBundleCold`). Joiners adopting a
    /// retained rendezvous do not count — the adopt-vs-rerun
    /// discriminator for miss-retention tests, where a surface-empty
    /// re-run bumps NO materialisation counter (the producers conclude
    /// the miss before building anything).
    pub bundle_cold_flight_runs: std::sync::atomic::AtomicU64,
    pub dep_resolution_calls: std::sync::atomic::AtomicU64,
    pub imported_macro_declaration_builds: std::sync::atomic::AtomicU64,
    /// Cold compile-output computes: bumped exactly once per cold run of
    /// `ensure_compile_artifacts` (the path PAST the warm-hit consult, where
    /// the shared compile actually executes). The deterministic, feature-
    /// independent observability rail for compile-slot COALESCING: two
    /// concurrent requests on the SAME `(canonical, profile)` that coalesce
    /// onto one shared compile bump this ONCE; two independent compiles bump it
    /// twice. (The `session_metrics` `compile_requests` counter mirrors this
    /// but is feature-gated; this `MetaProvenance` rail is always on, like the
    /// cold per-file artifact-build dedup counters below.) `reset()` zeroes it.
    pub compile_cold_runs: std::sync::atomic::AtomicU64,

    // ── Cold per-file artifact-build dedup counters ─────────────────────
    //
    // One cold resolve of one canonical performs exactly ONE of each:
    // one eval-program parse, one eval-env build, one shallow-state
    // build, one `IndexedReady` materialisation. These counters are the
    // deterministic observability rail for that contract (no
    // wall-clock); `reset()` zeroes them like every other counter.
    /// OXC eval-program parses performed through the single host parse
    /// entry (`parse_eval_program`). Exactly 1 per cold canonical build.
    pub eval_program_parses: std::sync::atomic::AtomicU64,
    /// Carrier parses performed through the single counted carrier
    /// store-leader frontend boundary — every framework
    /// carrier (`.vue`, `.svelte`, …) increments this exactly once per
    /// elected catalog-frontend parse. The framework-neutral parse-once rail:
    /// a cold build of any carrier file bumps this once, so a duplicate
    /// carrier parse on any host lane (Vue OR Svelte) is counter-visible
    /// without naming a framework.
    pub carrier_parses: std::sync::atomic::AtomicU64,
    /// SFC structure parses (the Vue carrier compatibility rail) —
    /// bumped by the elected store leader only when the dispatched
    /// carrier is Vue, covering the materialise lanes (base + overlay),
    /// the compile/template merged-source lanes, and the lazy
    /// `get_analysis` re-parse fallbacks, so a duplicate-SFC-parse
    /// regression on any Vue host lane stays counter-visible alongside
    /// the neutral `carrier_parses` rail.
    pub sfc_parses: std::sync::atomic::AtomicU64,
    /// Full OXC program parses through `parse_non_sfc_snapshot` —
    /// the scheduler snapshot lane for non-SFC files plus the
    /// `build_snapshot_from_source` analysis read path. Distinct from
    /// `eval_program_parses`
    /// (the `parse_eval_program` funnel) and `sfc_parses` (SFC
    /// structure parses); counted inside the worker fn so every lane
    /// counts.
    pub non_sfc_snapshot_parses: std::sync::atomic::AtomicU64,
    /// Full OXC SCRIPT-program parses on the `.vue` snapshot path —
    /// the position-preserving script source extracted from the SFC and
    /// parsed for export signatures + script analysis. Exactly 1 per
    /// `.vue` snapshot build: both consumers walk the SAME program (the
    /// `_from_program` threading), so a count of 2 on one snapshot
    /// build means a lane re-introduced a per-consumer re-parse of the
    /// same script bytes. Distinct from `sfc_parses` (the SFC STRUCTURE
    /// parse, not an OXC program parse) and `eval_program_parses` (the
    /// eval funnel); counted inside the worker fn so every lane counts.
    pub vue_script_snapshot_parses: std::sync::atomic::AtomicU64,
    /// `EvalEnv` builds initiated by the host (the program-taking
    /// builder plus any call site that forces an internal fallback
    /// build). Exactly 1 per cold canonical build.
    pub eval_env_builds: std::sync::atomic::AtomicU64,
    /// Declaration BODIES lowered to typed IR on behalf of this host —
    /// one increment per type/value/augmentation declaration contributor
    /// whose body (annotation, signature set, object shape, heritage,
    /// member types) was lowered from OXC syntax. The deterministic
    /// demand-scoping rail: publishing a file's `IndexedReady` lowers
    /// ZERO bodies; a semantic query lowers exactly the demanded
    /// declaration closure; a whole-file env demand (fallthrough /
    /// runtime values) lowers the file's full declaration set once.
    pub decl_bodies_lowered: std::sync::atomic::AtomicU64,
    /// `ShallowFileState::from_route_inventory_with_resolver` builds initiated
    /// by host call sites. Exactly 1 per cold canonical build.
    pub shallow_state_builds: std::sync::atomic::AtomicU64,
    /// Cold `IndexedReady` materialisations (base + overlay
    /// materialiser bodies). Exactly 1 per cold canonical build.
    pub indexed_ready_materializes: std::sync::atomic::AtomicU64,

    // ── Contention instrumentation ──────────────────────────────────────
    /// `VerterHost::ensure_loaded` invocation count.
    pub ensure_loaded_calls: std::sync::atomic::AtomicU64,
    /// Time spent inside `Scheduler::wait_or_drive` from `ensure_loaded`.
    pub ensure_loaded_wait_ns: std::sync::atomic::AtomicU64,
    /// Time spent inside `integrate_scheduler_snapshot` from `ensure_loaded`.
    pub ensure_loaded_work_ns: std::sync::atomic::AtomicU64,
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
    /// Scheduler submission count (mirrored from
    /// `verter_scheduler::scheduler::SchedulerCounters::submit_count` via
    /// `VerterHost::provenance_snapshot`). The direct-memoized field stays
    /// zero; `provenance_snapshot` overwrites it with the live value.
    pub scheduler_submit_count: std::sync::atomic::AtomicU64,
    /// Scheduler peak inbox depth (mirrored from `SchedulerCounters`).
    pub scheduler_inbox_depth_max: std::sync::atomic::AtomicU64,
    /// Per-`SemanticNodeData` variant push count, indexed by
    /// `SemanticNodeTag::bucket_index()`. Sized to
    /// [`SEMANTIC_NODE_DATA_DISCRIMINANT_COUNT`] for variant headroom.
    pub node_arena_pushes_per_discriminant:
        [std::sync::atomic::AtomicU64; SEMANTIC_NODE_DATA_DISCRIMINANT_COUNT],

    // ── Family B/C/D producer-install observability ───────────
    //
    // `install_fact_tracer` substrate counters for the 5 caches wired
    // through the producer-install observability surface. Each cache
    // exposes two counters:
    //
    // - `<cache>_fact_tracer_installs` — number of cold-compute calls
    //   wrapped in `install_fact_tracer` (advances once per cold
    //   producer entry).
    // - `<cache>_overflow_refusals` — number of cold-compute calls
    //   whose observation set exceeded `FACT_SIGNATURE_CAP` (1024) and
    //   were therefore NOT admitted to the warm cache (caller
    //   cold-recomputes on next request).
    //
    // Caches: `MemoEntry`, `AppConfigNoOverrideProofDb`, `OwnerImportSurfaceDb`.
    /// Structural-materialiser `install_fact_tracer` wrap count. No
    /// production producer bumps it; the field stays on the public stats
    /// snapshot as a zero-valued row.
    pub materialize_structure_fact_tracer_installs: std::sync::atomic::AtomicU64,
    /// Structural-materialiser overflow-refusal count. No production
    /// producer bumps it; the field stays on the public stats snapshot as
    /// a zero-valued row.
    pub materialize_structure_overflow_refusals: std::sync::atomic::AtomicU64,
    /// `install_fact_tracer` wrap count for `MemoEntry` (semantic
    /// query memo cold builds).
    pub memo_entry_fact_tracer_installs: std::sync::atomic::AtomicU64,
    /// `install_fact_tracer` overflow-refusal count for `MemoEntry`.
    pub memo_entry_overflow_refusals: std::sync::atomic::AtomicU64,
    /// `install_fact_tracer` wrap count for `AppConfigNoOverrideProofDb`.
    pub app_config_proof_fact_tracer_installs: std::sync::atomic::AtomicU64,
    /// `install_fact_tracer` overflow-refusal count for
    /// `AppConfigNoOverrideProofDb`.
    pub app_config_proof_overflow_refusals: std::sync::atomic::AtomicU64,
    /// `install_fact_tracer` wrap count for `OwnerImportSurfaceDb`.
    pub owner_import_surface_fact_tracer_installs: std::sync::atomic::AtomicU64,
    /// `install_fact_tracer` overflow-refusal count for
    /// `OwnerImportSurfaceDb`.
    pub owner_import_surface_overflow_refusals: std::sync::atomic::AtomicU64,
    /// Admission refusals for `OwnerImportSurfaceDb` because an
    /// unresolved direct import could not be rooted in the owner's
    /// path-precise resolution witness (no coverage for the skipped specifier).
    /// The surface is served to the caller but never cached — the next
    /// request cold-recomputes against the live workspace.
    pub owner_import_surface_unrooted_skip_refusals: std::sync::atomic::AtomicU64,
    /// Admission refusals for `OwnerImportSurfaceDb` because the cold
    /// build consumed a FENCED (ReturnOnly) serve — either the traced
    /// scope observed one by value, or a per-binding route walk
    /// returned the strict-admission empty-facts signal. The surface
    /// is served to the caller but never cached; the next request
    /// cold-recomputes against the live workspace.
    pub owner_import_surface_fenced_serve_refusals: std::sync::atomic::AtomicU64,
}

impl Default for MetaProvenance {
    fn default() -> Self {
        Self {
            get_component_meta_calls: std::sync::atomic::AtomicU64::new(0),
            component_meta_resolved_state_recomputes: std::sync::atomic::AtomicU64::new(0),
            get_analysis_calls: std::sync::atomic::AtomicU64::new(0),
            evaluate_types_calls: std::sync::atomic::AtomicU64::new(0),
            host_upsert_calls: std::sync::atomic::AtomicU64::new(0),
            view_aware_cache_key_lookups: std::sync::atomic::AtomicU64::new(0),
            resolved_external_type_cache_hits: std::sync::atomic::AtomicU64::new(0),
            resolved_external_type_cache_misses: std::sync::atomic::AtomicU64::new(0),
            resolver_node_cache_hits: std::sync::atomic::AtomicU64::new(0),
            resolver_node_cache_misses: std::sync::atomic::AtomicU64::new(0),
            resolver_singleflight_coalesced: std::sync::atomic::AtomicU64::new(0),
            resolver_cross_view_lane_forks: std::sync::atomic::AtomicU64::new(0),
            resolver_cycle_detections: std::sync::atomic::AtomicU64::new(0),
            resolver_route_fact_reuse: std::sync::atomic::AtomicU64::new(0),
            resolver_barrel_fact_reuse: std::sync::atomic::AtomicU64::new(0),
            import_resolution_cache_hit_count: std::sync::atomic::AtomicU64::new(0),
            import_resolution_cache_miss_count: std::sync::atomic::AtomicU64::new(0),
            dir_index_hit_count: std::sync::atomic::AtomicU64::new(0),
            dir_index_refresh_count: std::sync::atomic::AtomicU64::new(0),
            dir_index_dirty_rescan_count: std::sync::atomic::AtomicU64::new(0),
            native_fs_read_dir_count: std::sync::atomic::AtomicU64::new(0),
            native_fs_read_file_miss_count: std::sync::atomic::AtomicU64::new(0),
            payload_cache_hits: std::sync::atomic::AtomicU64::new(0),
            payload_cache_misses: std::sync::atomic::AtomicU64::new(0),
            payload_encodes: std::sync::atomic::AtomicU64::new(0),
            session_overlay_cows: std::sync::atomic::AtomicU64::new(0),
            overlay_set_fingerprint_full_computations: std::sync::atomic::AtomicU64::new(0),
            store_view_from_host_reads: std::sync::atomic::AtomicU64::new(0),
            component_meta_result_cache_hits: std::sync::atomic::AtomicU64::new(0),
            component_meta_result_cache_misses: std::sync::atomic::AtomicU64::new(0),
            slot_binding_graph_fact_tracer_emissions: std::sync::atomic::AtomicU64::new(0),
            dispatch_dep_signature_fact_tracer_emissions: std::sync::atomic::AtomicU64::new(0),
            indexed_ready_scheduler_snapshot_reuse: std::sync::atomic::AtomicU64::new(0),
            bundle_cache_hits: std::sync::atomic::AtomicU64::new(0),
            bundle_request_memo_hits: std::sync::atomic::AtomicU64::new(0),
            bundle_materializations: std::sync::atomic::AtomicU64::new(0),
            bundle_cold_flight_runs: std::sync::atomic::AtomicU64::new(0),
            dep_resolution_calls: std::sync::atomic::AtomicU64::new(0),
            imported_macro_declaration_builds: std::sync::atomic::AtomicU64::new(0),
            compile_cold_runs: std::sync::atomic::AtomicU64::new(0),
            eval_program_parses: std::sync::atomic::AtomicU64::new(0),
            carrier_parses: std::sync::atomic::AtomicU64::new(0),
            sfc_parses: std::sync::atomic::AtomicU64::new(0),
            non_sfc_snapshot_parses: std::sync::atomic::AtomicU64::new(0),
            vue_script_snapshot_parses: std::sync::atomic::AtomicU64::new(0),
            eval_env_builds: std::sync::atomic::AtomicU64::new(0),
            decl_bodies_lowered: std::sync::atomic::AtomicU64::new(0),
            shallow_state_builds: std::sync::atomic::AtomicU64::new(0),
            indexed_ready_materializes: std::sync::atomic::AtomicU64::new(0),
            ensure_loaded_calls: std::sync::atomic::AtomicU64::new(0),
            ensure_loaded_wait_ns: std::sync::atomic::AtomicU64::new(0),
            ensure_loaded_work_ns: std::sync::atomic::AtomicU64::new(0),
            execute_cooperative_owner_path: std::sync::atomic::AtomicU64::new(0),
            execute_cooperative_joiner_path: std::sync::atomic::AtomicU64::new(0),
            execute_cooperative_held_ns: std::sync::atomic::AtomicU64::new(0),
            node_arena_pushes: std::sync::atomic::AtomicU64::new(0),
            node_arena_intern_miss: std::sync::atomic::AtomicU64::new(0),
            node_arena_inner_write_wait_ns: std::sync::atomic::AtomicU64::new(0),
            scheduler_submit_count: std::sync::atomic::AtomicU64::new(0),
            scheduler_inbox_depth_max: std::sync::atomic::AtomicU64::new(0),
            node_arena_pushes_per_discriminant: std::array::from_fn(|_| {
                std::sync::atomic::AtomicU64::new(0)
            }),
            materialize_structure_fact_tracer_installs: std::sync::atomic::AtomicU64::new(0),
            materialize_structure_overflow_refusals: std::sync::atomic::AtomicU64::new(0),
            memo_entry_fact_tracer_installs: std::sync::atomic::AtomicU64::new(0),
            memo_entry_overflow_refusals: std::sync::atomic::AtomicU64::new(0),
            app_config_proof_fact_tracer_installs: std::sync::atomic::AtomicU64::new(0),
            app_config_proof_overflow_refusals: std::sync::atomic::AtomicU64::new(0),
            owner_import_surface_fact_tracer_installs: std::sync::atomic::AtomicU64::new(0),
            owner_import_surface_overflow_refusals: std::sync::atomic::AtomicU64::new(0),
            owner_import_surface_unrooted_skip_refusals: std::sync::atomic::AtomicU64::new(0),
            owner_import_surface_fenced_serve_refusals: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

impl std::fmt::Debug for MetaProvenance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use std::sync::atomic::Ordering::Relaxed;
        f.debug_struct("MetaProvenance")
            .field(
                "get_component_meta_calls",
                &self.get_component_meta_calls.load(Relaxed),
            )
            .field(
                "component_meta_resolved_state_recomputes",
                &self.component_meta_resolved_state_recomputes.load(Relaxed),
            )
            .field("get_analysis_calls", &self.get_analysis_calls.load(Relaxed))
            .field(
                "evaluate_types_calls",
                &self.evaluate_types_calls.load(Relaxed),
            )
            .field(
                "resolved_external_type_cache_hits",
                &self.resolved_external_type_cache_hits.load(Relaxed),
            )
            .field(
                "resolved_external_type_cache_misses",
                &self.resolved_external_type_cache_misses.load(Relaxed),
            )
            .field(
                "resolver_node_cache_hits",
                &self.resolver_node_cache_hits.load(Relaxed),
            )
            .field(
                "resolver_node_cache_misses",
                &self.resolver_node_cache_misses.load(Relaxed),
            )
            .field(
                "resolver_singleflight_coalesced",
                &self.resolver_singleflight_coalesced.load(Relaxed),
            )
            .field(
                "resolver_cross_view_lane_forks",
                &self.resolver_cross_view_lane_forks.load(Relaxed),
            )
            .field(
                "resolver_cycle_detections",
                &self.resolver_cycle_detections.load(Relaxed),
            )
            .field(
                "resolver_route_fact_reuse",
                &self.resolver_route_fact_reuse.load(Relaxed),
            )
            .field(
                "resolver_barrel_fact_reuse",
                &self.resolver_barrel_fact_reuse.load(Relaxed),
            )
            .field(
                "import_resolution_cache_hit_count",
                &self.import_resolution_cache_hit_count.load(Relaxed),
            )
            .field(
                "import_resolution_cache_miss_count",
                &self.import_resolution_cache_miss_count.load(Relaxed),
            )
            .field(
                "dir_index_hit_count",
                &self.dir_index_hit_count.load(Relaxed),
            )
            .field(
                "dir_index_refresh_count",
                &self.dir_index_refresh_count.load(Relaxed),
            )
            .field(
                "dir_index_dirty_rescan_count",
                &self.dir_index_dirty_rescan_count.load(Relaxed),
            )
            .field(
                "native_fs_read_dir_count",
                &self.native_fs_read_dir_count.load(Relaxed),
            )
            .field(
                "native_fs_read_file_miss_count",
                &self.native_fs_read_file_miss_count.load(Relaxed),
            )
            .field("payload_cache_hits", &self.payload_cache_hits.load(Relaxed))
            .field(
                "payload_cache_misses",
                &self.payload_cache_misses.load(Relaxed),
            )
            .field("payload_encodes", &self.payload_encodes.load(Relaxed))
            .field(
                "session_overlay_cows",
                &self.session_overlay_cows.load(Relaxed),
            )
            .field(
                "overlay_set_fingerprint_full_computations",
                &self.overlay_set_fingerprint_full_computations.load(Relaxed),
            )
            .field(
                "store_view_from_host_reads",
                &self.store_view_from_host_reads.load(Relaxed),
            )
            .field(
                "indexed_ready_scheduler_snapshot_reuse",
                &self.indexed_ready_scheduler_snapshot_reuse.load(Relaxed),
            )
            .field("bundle_cache_hits", &self.bundle_cache_hits.load(Relaxed))
            .field(
                "bundle_request_memo_hits",
                &self.bundle_request_memo_hits.load(Relaxed),
            )
            .field(
                "bundle_materializations",
                &self.bundle_materializations.load(Relaxed),
            )
            .field(
                "bundle_cold_flight_runs",
                &self.bundle_cold_flight_runs.load(Relaxed),
            )
            .field(
                "dep_resolution_calls",
                &self.dep_resolution_calls.load(Relaxed),
            )
            .field(
                "imported_macro_declaration_builds",
                &self.imported_macro_declaration_builds.load(Relaxed),
            )
            .field("compile_cold_runs", &self.compile_cold_runs.load(Relaxed))
            .field(
                "ensure_loaded_calls",
                &self.ensure_loaded_calls.load(Relaxed),
            )
            .field(
                "ensure_loaded_wait_ns",
                &self.ensure_loaded_wait_ns.load(Relaxed),
            )
            .field(
                "ensure_loaded_work_ns",
                &self.ensure_loaded_work_ns.load(Relaxed),
            )
            .field(
                "execute_cooperative_owner_path",
                &self.execute_cooperative_owner_path.load(Relaxed),
            )
            .field(
                "execute_cooperative_joiner_path",
                &self.execute_cooperative_joiner_path.load(Relaxed),
            )
            .field(
                "execute_cooperative_held_ns",
                &self.execute_cooperative_held_ns.load(Relaxed),
            )
            .field("node_arena_pushes", &self.node_arena_pushes.load(Relaxed))
            .field(
                "node_arena_intern_miss",
                &self.node_arena_intern_miss.load(Relaxed),
            )
            .field(
                "node_arena_inner_write_wait_ns",
                &self.node_arena_inner_write_wait_ns.load(Relaxed),
            )
            .field(
                "scheduler_submit_count",
                &self.scheduler_submit_count.load(Relaxed),
            )
            .field(
                "scheduler_inbox_depth_max",
                &self.scheduler_inbox_depth_max.load(Relaxed),
            )
            .finish()
    }
}

impl MetaProvenance {
    /// Return a point-in-time snapshot of all counters.
    pub fn snapshot(&self) -> MetaProvenanceSnapshot {
        use std::sync::atomic::Ordering::Relaxed;
        MetaProvenanceSnapshot {
            get_component_meta_calls: self.get_component_meta_calls.load(Relaxed),
            component_meta_resolved_state_recomputes: self
                .component_meta_resolved_state_recomputes
                .load(Relaxed),
            get_analysis_calls: self.get_analysis_calls.load(Relaxed),
            evaluate_types_calls: self.evaluate_types_calls.load(Relaxed),
            resolved_external_type_cache_hits: self.resolved_external_type_cache_hits.load(Relaxed),
            resolved_external_type_cache_misses: self
                .resolved_external_type_cache_misses
                .load(Relaxed),
            resolver_node_cache_hits: self.resolver_node_cache_hits.load(Relaxed),
            resolver_node_cache_misses: self.resolver_node_cache_misses.load(Relaxed),
            resolver_singleflight_coalesced: self.resolver_singleflight_coalesced.load(Relaxed),
            resolver_cross_view_lane_forks: self.resolver_cross_view_lane_forks.load(Relaxed),
            resolver_cycle_detections: self.resolver_cycle_detections.load(Relaxed),
            resolver_route_fact_reuse: self.resolver_route_fact_reuse.load(Relaxed),
            resolver_barrel_fact_reuse: self.resolver_barrel_fact_reuse.load(Relaxed),
            import_resolution_cache_hit_count: self.import_resolution_cache_hit_count.load(Relaxed),
            import_resolution_cache_miss_count: self
                .import_resolution_cache_miss_count
                .load(Relaxed),
            dir_index_hit_count: self.dir_index_hit_count.load(Relaxed),
            dir_index_refresh_count: self.dir_index_refresh_count.load(Relaxed),
            dir_index_dirty_rescan_count: self.dir_index_dirty_rescan_count.load(Relaxed),
            native_fs_read_dir_count: self.native_fs_read_dir_count.load(Relaxed),
            native_fs_read_file_miss_count: self.native_fs_read_file_miss_count.load(Relaxed),
            payload_cache_hits: self.payload_cache_hits.load(Relaxed),
            payload_cache_misses: self.payload_cache_misses.load(Relaxed),
            payload_encodes: self.payload_encodes.load(Relaxed),
            session_overlay_cows: self.session_overlay_cows.load(Relaxed),
            overlay_set_fingerprint_full_computations: self
                .overlay_set_fingerprint_full_computations
                .load(Relaxed),
            store_view_from_host_reads: self.store_view_from_host_reads.load(Relaxed),
            component_meta_result_cache_hits: self.component_meta_result_cache_hits.load(Relaxed),
            component_meta_result_cache_misses: self
                .component_meta_result_cache_misses
                .load(Relaxed),
            slot_binding_graph_fact_tracer_emissions: self
                .slot_binding_graph_fact_tracer_emissions
                .load(Relaxed),
            dispatch_dep_signature_fact_tracer_emissions: self
                .dispatch_dep_signature_fact_tracer_emissions
                .load(Relaxed),
            indexed_ready_scheduler_snapshot_reuse: self
                .indexed_ready_scheduler_snapshot_reuse
                .load(Relaxed),
            bundle_cache_hits: self.bundle_cache_hits.load(Relaxed),
            bundle_request_memo_hits: self.bundle_request_memo_hits.load(Relaxed),
            bundle_materializations: self.bundle_materializations.load(Relaxed),
            bundle_cold_flight_runs: self.bundle_cold_flight_runs.load(Relaxed),
            dep_resolution_calls: self.dep_resolution_calls.load(Relaxed),
            imported_macro_declaration_builds: self.imported_macro_declaration_builds.load(Relaxed),
            compile_cold_runs: self.compile_cold_runs.load(Relaxed),
            eval_program_parses: self.eval_program_parses.load(Relaxed),
            carrier_parses: self.carrier_parses.load(Relaxed),
            sfc_parses: self.sfc_parses.load(Relaxed),
            non_sfc_snapshot_parses: self.non_sfc_snapshot_parses.load(Relaxed),
            vue_script_snapshot_parses: self.vue_script_snapshot_parses.load(Relaxed),
            eval_env_builds: self.eval_env_builds.load(Relaxed),
            decl_bodies_lowered: self.decl_bodies_lowered.load(Relaxed),
            shallow_state_builds: self.shallow_state_builds.load(Relaxed),
            indexed_ready_materializes: self.indexed_ready_materializes.load(Relaxed),
            ensure_loaded_calls: self.ensure_loaded_calls.load(Relaxed),
            ensure_loaded_wait_ns: self.ensure_loaded_wait_ns.load(Relaxed),
            ensure_loaded_work_ns: self.ensure_loaded_work_ns.load(Relaxed),
            execute_cooperative_owner_path: self.execute_cooperative_owner_path.load(Relaxed),
            execute_cooperative_joiner_path: self.execute_cooperative_joiner_path.load(Relaxed),
            execute_cooperative_held_ns: self.execute_cooperative_held_ns.load(Relaxed),
            node_arena_pushes: self.node_arena_pushes.load(Relaxed),
            node_arena_intern_miss: self.node_arena_intern_miss.load(Relaxed),
            node_arena_inner_write_wait_ns: self.node_arena_inner_write_wait_ns.load(Relaxed),
            scheduler_submit_count: self.scheduler_submit_count.load(Relaxed),
            scheduler_inbox_depth_max: self.scheduler_inbox_depth_max.load(Relaxed),
            node_arena_pushes_per_discriminant: self
                .node_arena_pushes_per_discriminant
                .iter()
                .map(|slot| slot.load(Relaxed))
                .collect(),
            materialize_structure_fact_tracer_installs: self
                .materialize_structure_fact_tracer_installs
                .load(Relaxed),
            materialize_structure_overflow_refusals: self
                .materialize_structure_overflow_refusals
                .load(Relaxed),
            memo_entry_fact_tracer_installs: self.memo_entry_fact_tracer_installs.load(Relaxed),
            memo_entry_overflow_refusals: self.memo_entry_overflow_refusals.load(Relaxed),
            app_config_proof_fact_tracer_installs: self
                .app_config_proof_fact_tracer_installs
                .load(Relaxed),
            app_config_proof_overflow_refusals: self
                .app_config_proof_overflow_refusals
                .load(Relaxed),
            owner_import_surface_fact_tracer_installs: self
                .owner_import_surface_fact_tracer_installs
                .load(Relaxed),
            owner_import_surface_overflow_refusals: self
                .owner_import_surface_overflow_refusals
                .load(Relaxed),
            owner_import_surface_unrooted_skip_refusals: self
                .owner_import_surface_unrooted_skip_refusals
                .load(Relaxed),
            owner_import_surface_fenced_serve_refusals: self
                .owner_import_surface_fenced_serve_refusals
                .load(Relaxed),
        }
    }

    /// Reset all counters to zero.
    pub fn reset(&self) {
        use std::sync::atomic::Ordering::Relaxed;
        self.get_component_meta_calls.store(0, Relaxed);
        self.component_meta_resolved_state_recomputes
            .store(0, Relaxed);
        self.get_analysis_calls.store(0, Relaxed);
        self.evaluate_types_calls.store(0, Relaxed);
        self.resolved_external_type_cache_hits.store(0, Relaxed);
        self.resolved_external_type_cache_misses.store(0, Relaxed);
        self.resolver_node_cache_hits.store(0, Relaxed);
        self.resolver_node_cache_misses.store(0, Relaxed);
        self.resolver_singleflight_coalesced.store(0, Relaxed);
        self.resolver_cross_view_lane_forks.store(0, Relaxed);
        self.resolver_cycle_detections.store(0, Relaxed);
        self.resolver_route_fact_reuse.store(0, Relaxed);
        self.resolver_barrel_fact_reuse.store(0, Relaxed);
        self.import_resolution_cache_hit_count.store(0, Relaxed);
        self.import_resolution_cache_miss_count.store(0, Relaxed);
        self.dir_index_hit_count.store(0, Relaxed);
        self.dir_index_refresh_count.store(0, Relaxed);
        self.dir_index_dirty_rescan_count.store(0, Relaxed);
        self.native_fs_read_dir_count.store(0, Relaxed);
        self.native_fs_read_file_miss_count.store(0, Relaxed);
        self.payload_cache_hits.store(0, Relaxed);
        self.payload_cache_misses.store(0, Relaxed);
        self.payload_encodes.store(0, Relaxed);
        self.session_overlay_cows.store(0, Relaxed);
        self.overlay_set_fingerprint_full_computations
            .store(0, Relaxed);
        self.store_view_from_host_reads.store(0, Relaxed);
        self.component_meta_result_cache_hits.store(0, Relaxed);
        self.component_meta_result_cache_misses.store(0, Relaxed);
        self.slot_binding_graph_fact_tracer_emissions
            .store(0, Relaxed);
        self.dispatch_dep_signature_fact_tracer_emissions
            .store(0, Relaxed);
        self.indexed_ready_scheduler_snapshot_reuse
            .store(0, Relaxed);
        self.bundle_cache_hits.store(0, Relaxed);
        self.bundle_request_memo_hits.store(0, Relaxed);
        self.bundle_materializations.store(0, Relaxed);
        self.bundle_cold_flight_runs.store(0, Relaxed);
        self.dep_resolution_calls.store(0, Relaxed);
        self.imported_macro_declaration_builds.store(0, Relaxed);
        self.compile_cold_runs.store(0, Relaxed);
        self.eval_program_parses.store(0, Relaxed);
        self.carrier_parses.store(0, Relaxed);
        self.sfc_parses.store(0, Relaxed);
        self.non_sfc_snapshot_parses.store(0, Relaxed);
        self.vue_script_snapshot_parses.store(0, Relaxed);
        self.eval_env_builds.store(0, Relaxed);
        self.decl_bodies_lowered.store(0, Relaxed);
        self.shallow_state_builds.store(0, Relaxed);
        self.indexed_ready_materializes.store(0, Relaxed);
        self.ensure_loaded_calls.store(0, Relaxed);
        self.ensure_loaded_wait_ns.store(0, Relaxed);
        self.ensure_loaded_work_ns.store(0, Relaxed);
        self.execute_cooperative_owner_path.store(0, Relaxed);
        self.execute_cooperative_joiner_path.store(0, Relaxed);
        self.execute_cooperative_held_ns.store(0, Relaxed);
        self.node_arena_pushes.store(0, Relaxed);
        self.node_arena_intern_miss.store(0, Relaxed);
        self.node_arena_inner_write_wait_ns.store(0, Relaxed);
        self.scheduler_submit_count.store(0, Relaxed);
        self.scheduler_inbox_depth_max.store(0, Relaxed);
        for slot in &self.node_arena_pushes_per_discriminant {
            slot.store(0, Relaxed);
        }
        self.materialize_structure_fact_tracer_installs
            .store(0, Relaxed);
        self.materialize_structure_overflow_refusals
            .store(0, Relaxed);
        self.memo_entry_fact_tracer_installs.store(0, Relaxed);
        self.memo_entry_overflow_refusals.store(0, Relaxed);
        self.app_config_proof_fact_tracer_installs.store(0, Relaxed);
        self.app_config_proof_overflow_refusals.store(0, Relaxed);
        self.owner_import_surface_fact_tracer_installs
            .store(0, Relaxed);
        self.owner_import_surface_overflow_refusals
            .store(0, Relaxed);
        self.owner_import_surface_unrooted_skip_refusals
            .store(0, Relaxed);
        self.owner_import_surface_fenced_serve_refusals
            .store(0, Relaxed);
    }
}

/// Serializable point-in-time snapshot of [`MetaProvenance`] counters.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaProvenanceSnapshot {
    pub get_component_meta_calls: u64,
    pub component_meta_resolved_state_recomputes: u64,
    pub get_analysis_calls: u64,
    pub evaluate_types_calls: u64,
    pub resolved_external_type_cache_hits: u64,
    pub resolved_external_type_cache_misses: u64,
    pub resolver_node_cache_hits: u64,
    pub resolver_node_cache_misses: u64,
    pub resolver_singleflight_coalesced: u64,
    pub resolver_cross_view_lane_forks: u64,
    pub resolver_cycle_detections: u64,
    pub resolver_route_fact_reuse: u64,
    pub resolver_barrel_fact_reuse: u64,
    pub import_resolution_cache_hit_count: u64,
    pub import_resolution_cache_miss_count: u64,
    pub dir_index_hit_count: u64,
    pub dir_index_refresh_count: u64,
    pub dir_index_dirty_rescan_count: u64,
    pub native_fs_read_dir_count: u64,
    pub native_fs_read_file_miss_count: u64,
    pub payload_cache_hits: u64,
    pub payload_cache_misses: u64,
    pub payload_encodes: u64,
    /// Session-overlay re-roots performed against this host (one bump per
    /// entry into the `Arc::make_mut` re-root path in
    /// [`crate::resolver_store::HostStoreView::with_session_overlay`] —
    /// an upper bound on actual snapshot clones, since a uniquely-owned
    /// snapshot is mutated in place; no bump on a no-op empty-overlay
    /// application). Per-host, so it observes worker-side per-job re-roots
    /// in a batch while staying isolated from other hosts' overlay
    /// activity.
    pub session_overlay_cows: u64,
    /// Full overlay-set fingerprint computations performed against this
    /// host (one bump per [`crate::session_view::overlay_set_fingerprint`]
    /// call that walks the overlay-hash table — collect + sort + hash —
    /// NOT per memoized `fingerprint()` read). Per-host, so it observes
    /// worker-side reads in a batch while staying isolated from other
    /// hosts' fingerprinting.
    pub overlay_set_fingerprint_full_computations: u64,
    /// [`crate::resolver_store::HostStoreView::from_host_read`] entries
    /// against this host — every store-view read (manager `Arc`-clone hit
    /// or full sweep alike). Per-host, so it observes worker-side per-job
    /// reads in a batch while staying isolated from other hosts'
    /// store-view traffic.
    pub store_view_from_host_reads: u64,
    pub component_meta_result_cache_hits: u64,
    pub component_meta_result_cache_misses: u64,
    /// Per-call count of dispatch fact fan-outs emitted from
    /// `meta_resolve/slot_binding_graph.rs`.
    pub slot_binding_graph_fact_tracer_emissions: u64,
    /// Per-call count of `observe_fact_signature` fan-outs emitted
    /// from the six dispatch-read sites that route through
    /// `meta_resolve::dep_signature::emit_dispatch_dep_signature_facts`
    /// (the projector sites,
    /// `materialize_component_meta_type_expr_until_stable_full` and
    /// `node_root_reaches_transitive_cycle_with_fence`). Used by behavioural tests
    /// to discriminate the fact-tracer path from the legacy
    /// request-tracer path.
    pub dispatch_dep_signature_fact_tracer_emissions: u64,
    pub indexed_ready_scheduler_snapshot_reuse: u64,
    pub bundle_cache_hits: u64,
    pub bundle_request_memo_hits: u64,
    pub bundle_materializations: u64,
    pub bundle_cold_flight_runs: u64,
    pub dep_resolution_calls: u64,
    pub imported_macro_declaration_builds: u64,
    pub compile_cold_runs: u64,
    pub eval_program_parses: u64,
    pub carrier_parses: u64,
    pub sfc_parses: u64,
    pub non_sfc_snapshot_parses: u64,
    pub vue_script_snapshot_parses: u64,
    pub eval_env_builds: u64,
    pub decl_bodies_lowered: u64,
    pub shallow_state_builds: u64,
    pub indexed_ready_materializes: u64,
    /// Contention instrumentation counters surfaced through the
    /// host's `MetaProvenance`.
    pub ensure_loaded_calls: u64,
    pub ensure_loaded_wait_ns: u64,
    pub ensure_loaded_work_ns: u64,
    pub execute_cooperative_owner_path: u64,
    pub execute_cooperative_joiner_path: u64,
    pub execute_cooperative_held_ns: u64,
    pub node_arena_pushes: u64,
    pub node_arena_intern_miss: u64,
    pub node_arena_inner_write_wait_ns: u64,
    pub scheduler_submit_count: u64,
    pub scheduler_inbox_depth_max: u64,
    /// Per-`SemanticNodeData` discriminant push count, indexed by
    /// [`crate::semantic_query::SemanticNodeTag::bucket_index`]
    /// ([`SEMANTIC_NODE_DATA_DISCRIMINANT_COUNT`] entries).
    pub node_arena_pushes_per_discriminant: Vec<u64>,

    // ── Family B/C/D producer-install observability ───────────
    /// Structural-materialiser `install_fact_tracer` wrap count (always
    /// zero: no production producer).
    pub materialize_structure_fact_tracer_installs: u64,
    /// Structural-materialiser overflow-refusal count (always zero: no
    /// production producer).
    pub materialize_structure_overflow_refusals: u64,
    /// `install_fact_tracer` wrap count for `MemoEntry`.
    pub memo_entry_fact_tracer_installs: u64,
    /// `install_fact_tracer` overflow-refusal count for `MemoEntry`.
    pub memo_entry_overflow_refusals: u64,
    /// `install_fact_tracer` wrap count for `AppConfigNoOverrideProofDb`.
    pub app_config_proof_fact_tracer_installs: u64,
    /// `install_fact_tracer` overflow-refusal count for `AppConfigNoOverrideProofDb`.
    pub app_config_proof_overflow_refusals: u64,
    /// `install_fact_tracer` wrap count for `OwnerImportSurfaceDb`.
    pub owner_import_surface_fact_tracer_installs: u64,
    /// `install_fact_tracer` overflow-refusal count for `OwnerImportSurfaceDb`.
    pub owner_import_surface_overflow_refusals: u64,
    pub owner_import_surface_unrooted_skip_refusals: u64,
    pub owner_import_surface_fenced_serve_refusals: u64,
}
