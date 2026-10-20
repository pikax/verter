# Production-only state sweep inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).
These rows record the bookkeeping sweep across the engine, source, session,
scheduler, workspace and binding owners: every field, store, hook, counter,
timer and trace found on the charter surfaces, classified by its actual
production consumer. A row whose gate reads `absent` was deleted; a row whose
gate reads `always` with an `OPTIONAL` classification is optional state still
compiled into default builds and is listed again under
[Open sweep items](#open-sweep-items).

"Observation gate" abbreviates
`cfg(any(test, feature = "test-support", feature = "semantic-observe"))`: the
state exists in the owner's own tests, in dependents' test builds through the
owner's `test-support`, and in measurement builds, and nowhere else.

Where a row here restates an item another owner's inventory already lists,
this row supersedes that owner's gate column.

## Cargo features

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `attribution` feature (`verter_audit`, `verter_session`, `verter_bench`) | none | OPTIONAL, retired | Folded into each owner's `semantic-observe`; no crate declares it | `Cargo.toml` of each owner | absent |
| `currency_probe` feature (`verter_session_query`, `verter_workspace`, `verter_session`, `verter_bench`) | none | OPTIONAL, retired | Folded into `semantic-observe`; the probe rail compiles under the query boundary's `semantic-observe` | `Cargo.toml` of each owner | absent |
| `hotpath` feature (`verter_parser`, `verter_diagnostics`, `verter_compiler`, `verter_semantic`, `verter_type_engine`, `verter_scheduler`, `verter_workspace`, `verter_session`, `verter_lsp`, `verter_bench`) | none | OPTIONAL, retired | Folded into `semantic-observe`, which now activates the optional `hotpath` dependency itself; no crate declares a `hotpath` feature | `Cargo.toml` of each owner | absent |
| `semantic-observe` in `verter_parser` and `verter_diagnostics` | none; measurement harnesses only | OPTIONAL | Forwarded by the semantic, compiler, LSP and benchmark owners | `Cargo.toml` | `cfg(feature = "semantic-observe")` |
| `hotpath-alloc` (`verter_parser`, `verter_compiler`, `verter_bench`) | none; allocation profiling only | OPTIONAL | Layered on `semantic-observe`, which it implies | `Cargo.toml` | `cfg(feature = "semantic-observe")` plus hotpath's allocation mode |
| `verter_session` test self-edge on `verter_audit/semantic-observe` | The CSS-domain chargeability tests read work-attribution counters | OPTIONAL | Test and example builds of the session crate only; enables the audit crate's own gate and no other layer's capture | `crates/verter_session/Cargo.toml` `[dev-dependencies]` | dev edge only |
| Feature-closure guard (`observe_feature_closure`) | Architecture guard: no shipped root (`verter_napi`, `verter_lsp`, `verter_wasm`, `verter_tsc`, host and WASM targets, normal/build edges and the root's dev edges) can activate a seed or anything a seed turns on; seeds are every member's `semantic-observe` and every member feature that implies a coordinated gate | REQUIRED (guard) | Test-only; resolves features with `cargo metadata` and `cargo tree` | `crates/verter_source_policy_gate/tests/cases/observe_feature_closure.rs` | test |

## verter_audit

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Work-attribution counter table, scope guards, attributing allocator, report renderers (`attribution::{read, snapshot, snapshot_all, reset, record_*}`) | none; benchmark and report readers only | OPTIONAL | Process-global cells with explicit resets | `src/attribution/{table,scope,alloc,report}.rs` | `cfg(feature = "semantic-observe")` |
| `attribute!` / `attribute_n!` / `attribute_max!` / `attribute_scope!` / `attribute_digest!` recording macros | none | OPTIONAL | Expand to a site-name check without the gate; the amount argument is not evaluated | `src/attribution/mod.rs` | `cfg(feature = "semantic-observe")` (site-name check `always`) |
| `observe::{CaptureAvailability, ObserveMode, capture}` | Benchmark probe identity reports capture availability; no engine consumer yet | REQUIRED (capability vocabulary) | Compile-time constants | `src/observe/mod.rs` | always |
| `AuditObserver`, `AuditEvent`, `CURRENT_OBSERVER` TLS, `current_observer`, `install_observer`, `NoOpObserver` | Audit record construction only | OPTIONAL | Per installed request context | `src/observer.rs`, `src/noop.rs` | always (runtime-gated by installation; see open items) |

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `AtomicSemanticGraphStats` counters (`hits`, `misses`, `same_path_sentinel_returns`, `waits_ms`, `joined_waits`, `inflight_aborted_retries`, `joiner_view_mismatch_forks`, `cold_aborts_swept`, `origin_edges_emitted`, builder counters, memo hit/miss counters), each a `Tally` | none; tests and measurement readers through `stats_snapshot` | OPTIONAL | One store; `Tally` is zero-size without the gate | `src/semantic_query_memo/stats.rs` | observation gate |
| `in_flight_current` / `in_flight_peak` and the in-flight CAS loop | none | OPTIONAL | One store; `InFlightStatsGuard` drop is a no-op without the gate | `src/semantic_query_memo/stats.rs` | observation gate |
| Path-length reservoir (`path_length_samples`, `record_path_length`) | none | OPTIONAL | One store; capped at 8192 samples | `src/semantic_query_memo/{stats,mod}.rs` | observation gate |
| `SemanticGraphStore::stats_snapshot` / `SemanticGraphStats` reader, `DerivationStore::iter_edges`, `sorted_percentile`, `intern_preserving_scope_call_count` | none; tests and measurement | OPTIONAL | Derived on read | `src/semantic_query_memo/{stats,derivation,nodes}.rs` | observation gate |
| Blocking-wait clock read feeding `waits_ms` | none | OPTIONAL | One wait | `src/semantic_query_memo/producer.rs` | observation gate |
| `budget_fallback_count` / `record_budget_fallback`, `projection_depth_samples` / `record_projection_depth`, `projection_depth_p50/p95` | none; no production writer | OPTIONAL, deleted | — | `src/semantic_query_memo/{stats,mod}.rs`, `src/semantic_query.rs` | absent |
| `loop5_instrumentation` statics, `TimerGuard`, dispatch-kind timers, dump, stall watchdog, and every call site (memo lookup, cold build, raise, dispatch, lowering, worklist, registry append) | none; the investigation example and tests | OPTIONAL | Process-global, explicit reset | `src/loop5_instrumentation.rs` and call sites in `src/{semantic_query_memo,project_semantic_dispatch}/`, `verter_session` `src/host_manage/component_meta_methods.rs` | observation gate |
| Per-operator dispatch clock pair in `dispatch_operator_with_recurse` | none | OPTIONAL | One dispatch | `src/project_semantic_dispatch/raise.rs` | observation gate |
| `ExecutionCounters` (`steps`, `starts`, `reuses`, `cycles`, `external_waits`, `pending_polls`, `peak_frames`, `subscription_inspections`) | none; the runtime's own work-bound tests | OPTIONAL | One execution | `src/semantic_execution.rs` | `cfg(test)` (supersedes the `pending_polls` and `subscription_inspections` gates recorded by the continuation-runtime owner) |
| `SemanticExecution.live_frames` | Frame-arena occupancy for the drive and its debug assertion | REQUIRED-lifetime | One execution | `src/semantic_execution.rs` | always |
| `FunctionFlowGraphStore.builds`, `FlowSliceStores.demand_plans` | none; fixture assertions through `build_count` / `demand_plan_count` | OPTIONAL | One store | `src/cache_runtime/flow_slice_node.rs` | `cfg(any(test, feature = "test-support"))` |
| `RequestContext.frontier_target_none_pairs_seen` and `observe_frontier_target_none_for_pair` | none; no caller | OPTIONAL, deleted | — | `src/request_context.rs` | absent |
| `SemanticGraphStore::memo_family_dump_for_diagnostics` | none; no caller | OPTIONAL, deleted | — | `src/semantic_query_memo/observability.rs` | absent |
| `node_ownership_counts`, `node_count`, `node_storage_slots`, `union_view_count`, `stable_key_class_count`, `pending_destroyed_nodes`, `unresolved_reach_count`, `memo_entry_counts_by_family`, `memo_candidate_count`, `memo_entry_count` | Host retention snapshot and LSP statistics; `node_count` also feeds synthetic binding identity | REQUIRED-lifetime | Derived on read from resident tables | `src/semantic_query_memo/observability.rs` | always |
| `candidate_admission_seq`, `DepSignatureInterner.inserts_since_sweep`, `RefusalSummaryTable` length/clears | Reverse-index identity, interner sweep trigger, refusal summary admission | REQUIRED / REQUIRED-lifetime | One store | `src/semantic_query_memo/{mod,interner,refusal_summary}.rs` | always |
| `RequestContext` cancellation, `projection_budget`, `committed_input`, `canonical_id`, `request_result_is_partial`, request memos (`dep_canonical_memo`, `import_route_observations`, `normalized_canonicals`) | Budget, cancellation, validity and request memo decisions | REQUIRED / REQUIRED-budget | One request | `src/request_context.rs` | always |
| TLS `CURRENT_REQUEST_CONTEXT`, `EXECUTION_ABORTS`, `ROOT_TEMPLATE_DEMAND_DEPTH`, `COLD_COMPUTE_COMPLETENESS`, `DEFERRED_PARTIAL_STICKY_DEPTH` | Budget, cancellation and partiality carriage | REQUIRED | Per installed request | `src/request_context.rs` | always |
| Fact tracing (`ACTIVE_TRACERS`, `ACTIVE_RECORDERS`, `FactReadRecorder`, `ACTIVE_REFUSAL_SCOPES`), `ExactHashEventRecorder` | Admission and reuse rail; identity bytes | REQUIRED | Per traced compute | `src/fact_tracing/`, `src/semantic_query.rs` | always |
| `RequestContext` audit counters and sets (`trace_id`, cache event tallies, `PerRequestCacheCounters`, `bypass_diagnostics`, `type_resolution_*`, `flow_return_*`, `compile_*`, resolver/import/substitute/mapped/recursive counters, `recursive_substitute_seen`, `mapped_member_seen`, `mapper_source_ordinals`) and the `AuditObserver` impl | Audit record builders only | OPTIONAL | One request | `src/request_context.rs` | always (open item) |
| `EngineProvenance` push counters, write-lock clock reads, cooperative owner/joiner path counters, `ProducerLease.started` | `getProvenance` debug API only | OPTIONAL | One store / arena | `src/engine_provenance.rs`, `src/semantic_query_memo/{arena,producer}.rs` | always (open item) |
| Lock-acquisition and dependency-signature observers | Audit records only | OPTIONAL | Per call | `src/request_observers.rs` | always (open item) |
| Derivation / origin-edge store | Audit footprint walks only; carries retention charges while it exists | OPTIONAL content with REQUIRED-lifetime charges | One store | `src/semantic_query_memo/{derivation,origin_edges}.rs` | always (open item) |
| `tracing::debug!` memo hit / miss / suppress | none beyond a subscriber | OPTIONAL | Per lookup | `src/semantic_query_memo/{mod,producer}.rs` | always (open item) |
| Relation-side explanation state | — | relation-owned | — | `src/project_semantic_dispatch/relation*` | owned by the relation inventory |

## verter_scheduler

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `SchedulerDag.capacity_counter` aggregate permit counter, `DagCapacityReservation.counter`, `SchedulerDag::reserve_capacity` (untyped reservation) | none; tests only | REQUIRED-looking, deleted | — | `src/dag.rs`, `src/dag/capacity.rs` | absent |
| `SchedulerDag::in_flight_permits` | Diagnostic accessor | REQUIRED-lifetime | Derived on read as the sum of the per-class counters | `src/dag.rs` | always |
| `cpu_counter`, `io_counter`, `reset_epoch` | Admission and reservation epoch | REQUIRED-budget / REQUIRED-lifetime | One DAG | `src/dag.rs`, `src/dag/capacity.rs` | always |
| `cache_node_completed` / `cache_node_cancelled`, `CacheNodeTerminalCounts`, `cache_node_terminal_counts` | none; the scheduler's own router tests | OPTIONAL | One DAG | `src/dag.rs` | `cfg(test)` |
| `BfsMetrics` | none; returned to tests by `dep_reaches_owner_with_metrics` | OPTIONAL | Stack-local per walk | `src/dag.rs` | always (open item) |
| `SchedulerCounters{submit_count, inbox_depth_max}` | `getProvenance` debug API only | OPTIONAL | One scheduler | `src/scheduler.rs` | always (open item) |
| `DagNode.enqueue_time` / `ReadyJob.enqueue_time`, pump-loop `pending_len` scan, inbox depth read, `publish_scheduler_dispatch` | Audit queue-dwell and depth records only | OPTIONAL | Per node / per pump iteration | `src/dag.rs`, `src/scheduler/driver.rs` | always (open item) |
| `PumpStats.made_progress` | Park-versus-spin decision | REQUIRED | Per pump | `src/scheduler/driver.rs` | always |
| Instrumented DAG mutex and `hotpath::measure` attributes | none; profiling only | OPTIONAL | Profiler capture window | `src/scheduler.rs`, `src/scheduler/driver.rs` | `cfg(feature = "semantic-observe")` |

## verter_execution

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Aggregate owner probe and compaction examination counter (`OwnerEntries.examined`) | none; the crate's own tests | OPTIONAL | Per aggregate token | `src/cancellation.rs` | `cfg(test)` — a test-only proof counter needs no capture gate |
| `RequestContextLike::capture_enabled` / `timing_enabled` / `record_cache_event` / `on_dedup_joiner` | Audit record construction; `timing_enabled` gates optional timing | OPTIONAL (runtime capture flags) | Per request context | `src/request_context.rs` | always (open item: replace with root-entry capture-mode carriage) |
| `cancellation_token`, opaque context install/clear guards | Cancellation and request-context carriage | REQUIRED / REQUIRED-lifetime | Per dispatch | `src/request_context.rs` | always |

## verter_semantic_source

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `DeclLoweringService` workers, account, queue capacity; `ShardEntry` refcount and pin | Lowering dispatch and retention | REQUIRED / REQUIRED-lifetime | Service lifetime | `src/decl_lowering.rs` | always |
| `DeclLoweringCounters` (`eval_program_parses`, `eval_env_builds`, `decl_bodies_lowered`, `decl_statements_lowered`) | `getProvenance` debug API only | OPTIONAL | One host | `src/decl_lowering.rs`, `src/decl_body_memo.rs` | always (open item) |
| `DeclLoweringCounters.class_elements_lowered` | none | OPTIONAL | One host | `src/decl_lowering.rs` | observation gate |
| `DeclLoweringService.live_leases` | LSP statistics only | OPTIONAL | Service lifetime | `src/decl_lowering.rs` | always (open item) |
| `DeclLoweringService.handoffs`, `handoff_count` | none; batching work-count tests | OPTIONAL | Service lifetime | `src/decl_lowering.rs` | `cfg(any(test, feature = "test-support"))` |
| Worker-side clock pair in `acquire_lease_and_run` | Handoff profile only | OPTIONAL | One acquisition | `src/decl_lowering.rs` | read only for a profiled acquisition (`VERTER_DECL_HANDOFF_PROFILE`) |
| `HandoffStats`, `global_handoff_stats`, `dump_/reset_decl_handoff_stats` | none; examples only | OPTIONAL | Process; runtime environment opt-in | `src/decl_lowering.rs` | `cfg(not(target_arch = "wasm32"))` (open item) |
| Lowering and capture-lookup probes | none | OPTIONAL | Per probe | `src/decl_body_memo.rs`, `src/flow_slice_content*.rs` | `cfg(any(test, feature = "test-support"))` |

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Currency chokepoint `probe_scope!` / `probe_tally!` sites | none; measurement harness | OPTIONAL | Scope guard per call | currency call sites | `cfg(feature = "semantic-observe")` (query boundary's gate) |
| `hotpath::measure` attributes | none; profiling | OPTIONAL | Profiler window | `src/host_*`, `src/meta_resolve/origin_graph.rs` | `cfg(feature = "semantic-observe")` |
| `loop5_instrumentation` registry-append timer | none | OPTIONAL | One call | `src/host_manage/component_meta_methods.rs` | observation gate |
| `HostAuditRuntime` record store, sampler state, registration; `AuditRequestRegistration` always taking the active arm | Audit APIs only | OPTIONAL | Host lifetime / per request | `src/host_audit_runtime.rs`, `src/audit_records_store.rs` | always (open item) |
| `MetaProvenance` counters and timers, `ProjectTypeStoreCounters`, `ResolverCounters`, `VueMacroCodegenCounters` | `getProvenance` debug API, tests, or none | OPTIONAL | Host lifetime | `src/meta_provenance.rs`, `src/project_type_store.rs`, `src/resolver_core/mod.rs`, `src/typeinfo/vue_macro_codegen.rs` | always (open item) |
| `ResolutionCounters.local_closure_steps` (written, never read) and `builder_expansion_steps` (never written or read) | none | OPTIONAL, deleted | — | `src/resolver_core/{shallow_file_state,external_type_frontier}.rs` | absent |
| `StoredRecord.inserted_at` | NAPI/WASM latest-record and since-threshold audit queries | OPTIONAL (audit envelope) | One stored record | `src/component_meta_audit/audit_records_store.rs` | always (open item 1) |
| `ResolutionCounters.frontier_symbol_visits` | Frontier traversal budget | REQUIRED-budget | One resolution | `src/resolver_core/shallow_file_state.rs` | always |
| `VerterHost::retention_snapshot()` occupancy and charge fields; typed store `len()` accessors | Host retention inspection and policy accounting | REQUIRED-lifetime | Derived on read | `src/host_lifecycle.rs`, `src/project_type_store.rs` | always |

## verter_session_query

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `currency_probe` sites, guards, `snapshot` / `reset`, `probe_scope!` / `probe_tally!` | none; measurement harness | OPTIONAL | Process-global site totals with explicit resets | `src/currency_probe.rs` | `cfg(feature = "semantic-observe")` |

## verter_workspace

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `hotpath::measure` attributes on file reads and import resolution | none; profiling | OPTIONAL | Profiler window | `src/{engine,filesystem}.rs` | `cfg(feature = "semantic-observe")` |
| `VfsProvenance` counters | `getProvenance` debug API only | OPTIONAL | One workspace | `src/types.rs`, `src/{engine,filesystem}.rs` | always (open item) |
| `KNOWN_CANONICALS_CALLS` and its readers | none; tests only | OPTIONAL | Process-global | `src/engine.rs` | `cfg(any(test, feature = "test-support"))` |
| `OverlayWorkCounters`, `OverlayWorkCounts`, `ResolutionOverlaySnapshot::work_counts` | none; overlay lane tests only | OPTIONAL | One overlay snapshot and its clones | `src/resolution_currency.rs` | `cfg(any(test, feature = "test-support"))` |
| `PublishedContextSelection.table_clears`, `ContextMembershipRow.evaluations`, `VfsProvenance.resolution_evidence_live_read_count` | none; tests only | OPTIONAL | One workspace | `src/{resolution_currency,published_state,filesystem}.rs` | always (open item) |
| `WorkspaceResourceSnapshot` / `FreshnessResidency`, collection `len()` accessors | Published-project check at construction; host retention snapshot | REQUIRED-lifetime | Derived on read | `src/{traits,engine}.rs`, `src/freshness/` | always |

## verter_compiler, verter_parser, verter_diagnostics, verter_semantic

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `hotpath::measure` attributes on compile, codegen, source-map, AST, lint and analysis entries | none; profiling | OPTIONAL | Profiler window | `crates/verter_compiler/src/**`, `crates/verter_parser/src/ast/mod.rs`, `crates/verter_diagnostics/src/linter.rs`, `crates/verter_semantic/src/analysis/build.rs` | `cfg(feature = "semantic-observe")` |
| Compile phase timers reported to the audit observer | Audit records only | OPTIONAL | One compile | `crates/verter_compiler/src/compile/mod.rs` | always (open item) |
| `CodeTransform` per-op audit emission (`record_audit_op`) | Audit records only | OPTIONAL | Per op | `crates/verter_compiler/src/code_transform/code_transform.rs` | always (open item) |

## Bindings

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| NAPI / WASM audit record encoders and audit methods | Public audit API | OPTIONAL (capture surface) | Per call | `crates/verter_napi/src/audit.rs`, `crates/verter_wasm/src/audit.rs` | always (open item: report unavailable capture when compiled out) |
| NAPI counting global allocator | Memory audit API | OPTIONAL | Process; runtime-enabled flag | `crates/verter_napi/src/memory_audit.rs` | always (open item) |
| FFI offset visit counter | none | OPTIONAL | Per conversion | `crates/verter_ffi/src/offset.rs` | `cfg(any(test, feature = "semantic-observe"))` |

## Open sweep items

These remain after this sweep step. Each is optional state still compiled
into default builds, or a required-looking field with no production consumer.
They stay listed here until gated or deleted.

1. The audit envelope is runtime-gated, not compile-gated. `RequestContext`
   carries about a hundred audit-only counters and sets beside its required
   budget, cancellation and partiality state; `current_observer()` is polled
   on hot paths (per `CodeTransform` op, per memo lookup, per recursive
   substitution); `AuditRequestRegistration` takes its active arm even with
   `audit_enabled` off. The target splits the required request state from an
   optional capture record selected once at root entry (or job submission)
   and carried through resumptions, with NAPI/WASM/LSP audit methods
   reporting unavailable capture when `semantic-observe` is compiled out.
2. `getProvenance` debug payload sources (`MetaProvenance`, `EngineProvenance`,
   `DeclLoweringCounters`, `SchedulerCounters`, `VfsProvenance`) are always
   compiled. Gating them needs the public provenance payload to report
   unavailable capture instead of zeros.
3. Scheduler audit work on the pump loop: the `pending_len` scan under the DAG
   lock, inbox depth read, per-node enqueue clock and per-dispatch
   `publish_scheduler_dispatch` allocation.
4. Test-only counters still compiled always: `BfsMetrics`, `table_clears`,
   `evaluations`, `resolution_evidence_live_read_count`, `ResolverCounters`,
   `ProjectTypeStoreCounters`, `VueMacroCodegenCounters`, and the native
   `HandoffStats` profile.
5. Always-zero audit wire fields with no writer (`resolver_store_view_calls`,
   `materialize_structure_*`, several `MetaProvenance` fields). Removing them
   is a wire-contract change owned by the audit and typeinfo wire owners.
6. Paired off-versus-stripped deterministic counter and allocation evidence on
   the quick corpus, and generated-code inspection of the hot paths, are not
   yet recorded.
