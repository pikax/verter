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
| `verter_session` test self-edge on `verter_audit/semantic-observe` | none; the suites that read work-attribution counters run in the instrumentation lane | OPTIONAL, deleted | The session's own test build stays capture-off, so the canonical gate tests the uncaptured configuration | `crates/verter_session/Cargo.toml` `[dev-dependencies]` | absent |
| `semantic-observe` in `verter_execution`, `verter_napi`, `verter_wasm`, `verter_mcp`, `verter_tsc`, `verter_ffi` | none; measurement and instrumentation-lane builds | OPTIONAL | The execution crate gates its request-context capture hooks; the binding, MCP and tsc roots forward to the session's opt-in | `Cargo.toml` | `cfg(feature = "semantic-observe")` |
| `hotpath` profiler activation | none; profiling | OPTIONAL | `semantic-observe` enables the optional `hotpath` dependency; its profiler (`hotpath/hotpath`) is switched on only for non-wasm32 targets, so an instrumented wasm32 build compiles hotpath's no-op form | target-specific `[dependencies]` of each owner | `cfg(feature = "semantic-observe")`, native targets |
| Instrumentation lane (`scripts/observe-lane.mjs`, CI `rust-observe`) | Runs every suite that reads optional capture: each workspace member declaring `semantic-observe`, read from `cargo metadata`, tested with it on under the provider-free core filter | REQUIRED (lane) | CI and local runs | `scripts/observe-lane.mjs`, `.github/workflows/ci.yml` | always |
| Feature-closure guard (`observe_feature_closure`) | Architecture guard: no shipped root (`verter_napi`, `verter_lsp`, `verter_wasm`, `verter_tsc`, host and WASM targets, normal/build edges and the root's dev edges) nor the session's own test build can activate a seed or anything a seed turns on; seeds are every member's `semantic-observe` and every member feature that implies a coordinated gate | REQUIRED (guard) | Test-only; resolves features with `cargo metadata` and `cargo tree` | `crates/verter_source_policy_gate/tests/cases/observe_feature_closure.rs` | test |

## verter_audit

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Work-attribution counter table, scope guards, attributing allocator, report renderers (`attribution::{read, snapshot, snapshot_all, reset, record_*}`) | none; benchmark and report readers only | OPTIONAL | Process-global cells with explicit resets | `src/attribution/{table,scope,alloc,report}.rs` | `cfg(feature = "semantic-observe")` |
| `attribute!` / `attribute_n!` / `attribute_max!` / `attribute_scope!` / `attribute_digest!` recording macros | none | OPTIONAL | Expand to a site-name check without the gate; the amount argument is not evaluated | `src/attribution/mod.rs` | `cfg(feature = "semantic-observe")` (site-name check `always`) |
| `observe::{CaptureAvailability, ObserveMode, capture}`, `AuditCaptureState::Unavailable` | Benchmark probe identity, the session's capture selection, and every public audit surface reporting unavailable capture | REQUIRED (capability vocabulary) | Compile-time constants | `src/observe/mod.rs`, `src/record.rs` | always |
| `observe::{ObservedU64, ObservedU32, ObservedU16, ObservedFlag, Stopwatch, Sample}` | Storage and clock reads for optional counters, timers and samples across the owners | OPTIONAL | Each owner's state; zero-size, and never read, without the gate | `src/observe/mod.rs` | readers `cfg(feature = "semantic-observe")`; types always (zero-size) |
| `CURRENT_OBSERVER` TLS, `current_observer`, `install_observer`, `clear_observer`, `ObserverGuard`, `observe!` emission | Audit record construction only | OPTIONAL | Per installed request context | `src/observer.rs` | `cfg(feature = "semantic-observe")`; `observe!` type-checks its body without emitting code otherwise |
| `AuditObserver`, `AuditEvent` | Vocabulary the observer and emission sites share | OPTIONAL | — | `src/observer.rs` | always (types only; no instance exists without the gate) |
| `NoOpObserver` | none | OPTIONAL, deleted | — | `src/noop.rs` | absent |

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
| `RequestContext` audit counters and sets (`trace_id`, `kind`, footprint/timing switches, accumulator and registration, `parent_request_id`, scheduler attribution, RSS peak, cache event tallies, `PerRequestCacheCounters`, `bypass_diagnostics`, `type_resolution_*`, `flow_return_*`, `compile_*`, resolver/import/substitute/mapped/recursive counters, `synthesis_active_depth`, `recursive_substitute_seen`, `mapped_member_seen`, `mapper_source_ordinals`), the `AuditObserver` impl, `RequestAuditFinalization`, `CURRENT_ACCUMULATOR`, `NESTED_AUDIT_GUARD`, `REQUESTS_CREATED`, `observe_request!` emission | Audit record builders only | OPTIONAL | One request | `src/request_context.rs`, `src/request_observers.rs` | `cfg(feature = "semantic-observe")` (counters are zero-size `Observed*` fields) |
| `EngineProvenance` push counters, write-lock clock reads and held-time buckets, cooperative owner/joiner path counters | `getProvenance` debug API (capture builds only) | OPTIONAL | One store / arena | `src/engine_provenance.rs`, `src/semantic_query_memo/{arena,producer}.rs` | `cfg(feature = "semantic-observe")` (`ObservedU64` fields; no clock read without it) |
| `ProducerLease.started` | The engine-provenance hold timer and the store's own timing tests | OPTIONAL | One producer lease | `src/semantic_query_memo/producer.rs` | observation gate |
| Lock-acquisition and dependency-signature observers, structured-event pushes, `component_meta_trace_*!` | Audit records only | OPTIONAL | Per call | `src/request_observers.rs` | `cfg(feature = "semantic-observe")` |
| Derivation / origin-edge store and `record_origin_edge` (fence interning, bucket record, sidecar tracking, `origin_edges_emitted`) | The audit origin graph (`audit_enabled && footprint_capture`, so capture builds only) and the derivation-layer tests | OPTIONAL content with REQUIRED-lifetime charges while it holds edges | One store | `src/semantic_query_memo/{derivation,origin_edges,stats}.rs` | recording under the observation gate: a production build records no edge, so the store stays empty and charges nothing; the read-only walks stay always compiled and see an empty store |
| Origin-edge call-site arguments (`sources` slices, `OriginMeta`, builder fence) built at each `record_origin_edge` site | Feeds the recorder above | OPTIONAL | Per derivation | `src/project_semantic_dispatch/` | always (open item) |
| Origin-edge push into the request accumulator | Audit footprint mining only | OPTIONAL | Per recorded edge | `src/semantic_query_memo/origin_edges.rs` | `cfg(feature = "semantic-observe")` |
| `tracing::debug!` memo hit / miss / suppress | none beyond a subscriber | OPTIONAL | Per lookup | `src/semantic_query_memo/{mod,producer}.rs` | always (open item) |
| Relation-side explanation state | — | relation-owned | — | `src/project_semantic_dispatch/relation*` | owned by the relation inventory |

## verter_scheduler

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `SchedulerDag.capacity_counter` aggregate permit counter, `DagCapacityReservation.counter`, `SchedulerDag::reserve_capacity` (untyped reservation) | none; tests only | REQUIRED-looking, deleted | — | `src/dag.rs`, `src/dag/capacity.rs` | absent |
| `SchedulerDag::in_flight_permits` | Diagnostic accessor | REQUIRED-lifetime | Derived on read as the sum of the per-class counters | `src/dag.rs` | always |
| `cpu_counter`, `io_counter`, `reset_epoch` | Admission and reservation epoch | REQUIRED-budget / REQUIRED-lifetime | One DAG | `src/dag.rs`, `src/dag/capacity.rs` | always |
| `cache_node_completed` / `cache_node_cancelled`, `CacheNodeTerminalCounts`, `cache_node_terminal_counts` | none; the scheduler's own router tests | OPTIONAL | One DAG | `src/dag.rs` | `cfg(test)` |
| `BfsMetrics` | none; returned to tests by `dep_reaches_owner_with_metrics` | OPTIONAL | Stack-local per walk | `src/dag.rs` | `cfg(test)` (zero-size outside tests; `record_enqueue` is a no-op) |
| `SchedulerCounters{submit_count, inbox_depth_max}`, `counters()`, the inbox `len()` read | `getProvenance` debug API (capture builds only) | OPTIONAL | One scheduler | `src/scheduler.rs` | `cfg(feature = "semantic-observe")` (`ObservedU64` fields) |
| `DagNode.enqueue_time` / `ReadyJob.enqueue_time` (`Stopwatch`), pump-loop `pending_len` scan, inbox depth read, queue dwell (`Sample`s), `publish_scheduler_dispatch`, `DedupJoinerEvent` / `dedup_events` | Audit queue-dwell, depth and dedup records only | OPTIONAL | Per node / per pump iteration | `src/dag.rs`, `src/scheduler/{driver,admission}.rs` | `cfg(feature = "semantic-observe")` (no clock read, scan or allocation without it) |
| `PumpStats.made_progress` | Park-versus-spin decision | REQUIRED | Per pump | `src/scheduler/driver.rs` | always |
| Instrumented DAG mutex and `hotpath::measure` attributes | none; profiling only | OPTIONAL | Profiler capture window | `src/scheduler.rs`, `src/scheduler/driver.rs` | `cfg(feature = "semantic-observe")` |

## verter_execution

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Aggregate owner probe and compaction examination counter (`OwnerEntries.examined`) | none; the crate's own tests | OPTIONAL | Per aggregate token | `src/cancellation.rs` | `cfg(test)` — a test-only proof counter needs no capture gate |
| `RequestContextLike::capture_enabled` / `timing_enabled` / `record_cache_event` / `on_dedup_joiner`, `current_timing_enabled` | Audit record construction; `timing_enabled` gates optional timing | OPTIONAL | Per request context | `src/request_context.rs` | `cfg(feature = "semantic-observe")` |
| `cancellation_token`, opaque context install/clear guards | Cancellation and request-context carriage | REQUIRED / REQUIRED-lifetime | Per dispatch | `src/request_context.rs` | always |

## verter_semantic_source

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `DeclLoweringService` workers, account, queue capacity; `ShardEntry` refcount and pin | Lowering dispatch and retention | REQUIRED / REQUIRED-lifetime | Service lifetime | `src/decl_lowering.rs` | always |
| `DeclLoweringCounters` (`eval_program_parses`, `eval_env_builds`, `decl_bodies_lowered`, `decl_statements_lowered`) and the two `total_decl_count()` walks feeding them | `getProvenance` debug API (capture builds only) and the lazy-lowering guards, which run in the instrumentation lane | OPTIONAL | One host | `src/decl_lowering.rs`, `src/decl_body_memo.rs` | `cfg(feature = "semantic-observe")` (`ObservedU64` fields) |
| `DeclLoweringCounters.class_elements_lowered` | none | OPTIONAL | One host | `src/decl_lowering.rs` | observation gate |
| `DeclLoweringService.live_leases` | LSP statistics only | OPTIONAL | Service lifetime | `src/decl_lowering.rs` | always (open item) |
| `DeclLoweringService.handoffs`, `handoff_count` | none; batching work-count tests | OPTIONAL | Service lifetime | `src/decl_lowering.rs` | `cfg(any(test, feature = "test-support"))` |
| Worker-side clock pair in `acquire_lease_and_run` | Handoff profile only | OPTIONAL | One acquisition | `src/decl_lowering.rs` | read only for a profiled acquisition (`VERTER_DECL_HANDOFF_PROFILE`) |
| `HandoffStats`, `global_handoff_stats`, `dump_/reset_decl_handoff_stats`, the `VERTER_DECL_HANDOFF_PROFILE` lookup, the service's `profile` field | none; the profiling examples (`required-features = ["semantic-observe"]`) | OPTIONAL | Process; runtime environment opt-in | `src/decl_lowering.rs` | `cfg(feature = "semantic-observe")` (an empty type and an always-`None` profile without it) |
| Lowering and capture-lookup probes | none | OPTIONAL | Per probe | `src/decl_body_memo.rs`, `src/flow_slice_content*.rs` | `cfg(any(test, feature = "test-support"))` |

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Currency chokepoint `probe_scope!` / `probe_tally!` sites | none; measurement harness | OPTIONAL | Scope guard per call | currency call sites | `cfg(feature = "semantic-observe")` (query boundary's gate) |
| `hotpath::measure` attributes | none; profiling | OPTIONAL | Profiler window | `src/host_*`, `src/meta_resolve/origin_graph.rs` | `cfg(feature = "semantic-observe")` |
| `loop5_instrumentation` registry-append timer | none | OPTIONAL | One call | `src/host_manage/component_meta_methods.rs` | observation gate |
| `HostAuditRuntime` record store, sampler state, registration, `AuditRequestRegistration`, the audited-request envelope, the session VFS audit sink, footprint attachment | Audit APIs only | OPTIONAL | Host lifetime / per request | `src/host_audit_runtime.rs`, `src/audited_request.rs`, `src/session_vfs_sink.rs`, `src/component_meta_audit/`, `src/typeinfo/footprint_attach.rs` | `cfg(feature = "semantic-observe")` |
| `capture::{availability, normalize_host_config, uncaptured_state}`; audited entries' uncaptured tails | Every audited entry: selects captured or uncaptured execution at root entry; a default build clears the capture switches at host construction and returns real results with `Unavailable` records; both modes install the same request context | REQUIRED (capture selection) | Host construction / per request | `src/capture.rs`, `src/host_*_audit.rs`, `src/component_meta_host.rs`, `src/typeinfo/` | always |
| `MetaProvenance` counters and timers, `MetaProvenanceSnapshot`, `VerterHost::provenance_snapshot`, the session `get_provenance` methods | `getProvenance` debug API (capture builds only); the NAPI method fails with the unavailable-capture error otherwise | OPTIONAL | Host lifetime | `src/meta_provenance.rs`, `src/host_lifecycle.rs`, `src/meta.rs`, `src/component_meta_host.rs` | `cfg(feature = "semantic-observe")` (`ObservedU64` fields) |
| `ProjectTypeStoreCounters` (`OccupancyCounter` handles shared by the cache stores) | none; tests and measurement readers | OPTIONAL | Host lifetime | `src/project_type_store.rs`, `crates/verter_type_engine/src/cache_runtime/occupancy.rs` | observation gate (`OccupancyCounter` is zero-size without it) |
| `ResolverCounters` | none; the session's own unit tests | OPTIONAL | One resolver runtime | `src/resolver_core/{mod,resolver_runtime}.rs` | `cfg(test)` (zero-size outside tests) |
| `VueMacroCodegenCounters` | none; the session's own contract suites | OPTIONAL | One codegen invocation | `src/typeinfo/vue_macro_codegen{.rs,/}` | `cfg(test)` (zero-size outside tests; the record methods are no-ops) |
| `ResolutionCounters.local_closure_steps` (written, never read) and `builder_expansion_steps` (never written or read) | none | OPTIONAL, deleted | — | `src/resolver_core/{shallow_file_state,external_type_frontier}.rs` | absent |
| `StoredRecord.inserted_at` | NAPI/WASM latest-record and since-threshold audit queries | OPTIONAL (audit envelope) | One stored record | `src/component_meta_audit/audit_records_store.rs` | `cfg(feature = "semantic-observe")` |
| Capture on/off equivalence suite | Proves the same component-meta, compile and named-type requests return byte-identical wire answers through plain and audited entries with capture off and on, in both the default build and the instrumentation lane | REQUIRED (test) | Test-only | `crates/verter_ffi/src/convert/capture_equivalence_tests.rs` | test |
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
| `VfsProvenance` counters, `VfsProvenanceSnapshot`, `vfs_provenance_snapshot` | `getProvenance` debug API (capture builds only) | OPTIONAL | One workspace | `src/types.rs`, `src/{engine,filesystem}.rs` | counters `cfg(feature = "semantic-observe")` (`ObservedU64`); the snapshot reader under the observation gate |
| `VfsReadEvent`, `VfsAuditSink`, sink registration, `register_/deregister_audit_sink`, `audit_op`; `VfsAuditSinks` / `VfsReadProbe` read probes | Audit per-file read records only | OPTIONAL | One workspace / per read | `src/{audit_sink,traits,memory,filesystem}.rs` | `cfg(feature = "semantic-observe")` (the registry and probe are zero-size without it) |
| `KNOWN_CANONICALS_CALLS` and its readers | none; tests only | OPTIONAL | Process-global | `src/engine.rs` | `cfg(any(test, feature = "test-support"))` |
| `OverlayWorkCounters`, `OverlayWorkCounts`, `ResolutionOverlaySnapshot::work_counts` | none; overlay lane tests only | OPTIONAL | One overlay snapshot and its clones | `src/resolution_currency.rs` | `cfg(any(test, feature = "test-support"))` |
| `PublishedContextSelection.table_clears`, `ContextMembershipRow.evaluations` | none; the workspace crate's own tests | OPTIONAL | One workspace | `src/{resolution_currency,published_state}.rs` | `cfg(test)` |
| `VfsProvenance.resolution_evidence_live_read_count` | none; LSP tests through the workspace's `test-support` | OPTIONAL | One workspace | `src/{types,filesystem}.rs` | observation gate |
| `WorkspaceResourceSnapshot` / `FreshnessResidency`, collection `len()` accessors | Published-project check at construction; host retention snapshot | REQUIRED-lifetime | Derived on read | `src/{traits,engine}.rs`, `src/freshness/` | always |

## verter_compiler, verter_parser, verter_diagnostics, verter_semantic

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `hotpath::measure` attributes on compile, codegen, source-map, AST, lint and analysis entries | none; profiling | OPTIONAL | Profiler window | `crates/verter_compiler/src/**`, `crates/verter_parser/src/ast/mod.rs`, `crates/verter_diagnostics/src/linter.rs`, `crates/verter_semantic/src/analysis/build.rs` | `cfg(feature = "semantic-observe")` |
| Compile phase timers and source-map clocks reported to the audit observer | Audit records only | OPTIONAL | One compile | `crates/verter_compiler/src/compile/mod.rs` | `cfg(feature = "semantic-observe")` (`observe!`, `Stopwatch`) |
| `CodeTransform` per-op audit emission (`record_audit_op`) | Audit records only | OPTIONAL | Per op | `crates/verter_compiler/src/code_transform/code_transform.rs` | `cfg(feature = "semantic-observe")` (`observe!`) |

## Bindings

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| NAPI / WASM audit record encoders and record readers (`getLastAuditRecord`, `getAuditRecords`, `getBundlerBatchSummary`, why-loaded / why-instantiated walkers, `getComponentMetaWithAudit`) | Public audit API | OPTIONAL (capture surface) | Per call | `crates/verter_napi/src/{audit,lib,meta}.rs`, `crates/verter_wasm/src/{audit,lib}.rs` | readers `cfg(feature = "semantic-observe")`; without it they fail with the unavailable-capture error |
| NAPI / WASM `captureAvailability()` | Consumers choosing whether to expect records | REQUIRED (capability) | Per call | `crates/verter_napi/src/audit.rs`, `crates/verter_wasm/src/audit.rs` | always |
| LSP `verter/getAuditRecord` / `verter/getAuditRecent`, hover provenance, the LSP audit harness | Audit and debug clients | OPTIONAL | Per request | `crates/verter_lsp/src/` | `cfg(feature = "semantic-observe")`; the methods answer with the unavailable-capture error otherwise |
| NAPI counting global allocator | Memory audit API | OPTIONAL | Process; runtime-enabled flag | `crates/verter_napi/src/memory_audit.rs` | always (open item) |
| FFI offset visit counter | none | OPTIONAL | Per conversion | `crates/verter_ffi/src/offset.rs` | `cfg(any(test, feature = "semantic-observe"))` |

## Paired build evidence

One machine (macOS, aarch64, 8 logical CPUs), release profile, the
signature-kernel harness's quick corpus
(`cargo run --release -p verter_session --example signature_kernel_bench -- --modules 4 --depth 4 --samples 6 --cold-samples 3 --soak 20`),
built once without and once with `--features semantic-observe`. Every host in
the harness runs with audit capture off, so the comparison is capture-off in
an instrumented binary against the stripped default binary.

- Answers: the 24 witnesses' outcomes in every state are identical across the
  two builds (24 complete, 0 degraded, 0 refused in each).
- Allocations (counting global allocator, deterministic per workload), default
  then instrumented: warm query 1660 → 1903 allocations (+14.6%), 246 184 →
  319 704 bytes; declaration edit 3652 → 3902 (+6.8%); augmentation edit
  2966 → 3015 (+1.7%); local edit 10 543 → 10 624 (+0.8%); cold load 101 188 →
  101 573 (+0.4%); restart 101 177 → 101 565 (+0.4%). The soak's final live
  heap is 3 634 059 bytes stripped and 3 946 370 bytes instrumented.
- Latency medians move the same way (warm query 4.2 µs → 5.6 µs), but six
  samples per workload make them indicative only; the allocation figures are
  the deterministic evidence.
- Generated code: the stripped harness binary is 30 252 560 bytes against
  33 114 848 instrumented; it carries no `current_observer` /
  `CURRENT_OBSERVER` or request-accumulator symbol (3 and 2 in the
  instrumented binary).

## Open sweep items

These remain after this sweep. Each is optional state still compiled into
default builds, or work this sweep could not finish, with the reason.

1. Origin-edge call sites still build their arguments (`sources` slices,
   `OriginMeta`, the builder fence) before calling `record_origin_edge`, whose
   body records nothing in a production build. Removing that construction
   means wrapping each of the call sites across `project_semantic_dispatch/`
   in an emission macro.
2. `tracing::debug!` memo hit / miss / suppress events and
   `DeclLoweringService.live_leases` (LSP statistics) are always compiled.
3. The NAPI counting global allocator stays always compiled behind its
   runtime-enabled flag; the memory-audit API reads it.
4. Always-zero audit wire fields with no writer (`resolver_store_view_calls`,
   `materialize_structure_*`). The audit envelope's additions are purely
   additive and these fields stay on the wire as zero-valued schema until a
   wire-contract retirement; removing them is owned by the audit and typeinfo
   wire owners, not this sweep. A default build no longer emits them at all:
   it returns no captured record.
5. Diagnostic tools that read audit records — `packages/benchmark/src/_audit-component.ts`,
   `scripts/benchmark/trace-component-corpus.mjs` and the native and WASM
   `probe-transport-surface.mjs` scripts — need a binding built with
   `semantic-observe`; against a default binding they fail with the
   unavailable-capture error.
6. Guards that assert only provenance counters now run in the instrumentation
   lane (CI `rust-observe`) rather than the canonical gate:
   `indexed_ready_publish_lowers_zero_decl_bodies`,
   `resolve_unrelated_symbol_lowers_only_demanded_decl`,
   `lazy_decl_body_singleflight_lowers_once`,
   `lazy_decl_lowering_uses_scheduler_snapshot_not_reparse`,
   `emit_parse_facts_never_hashes_decl_bodies`,
   `fallthrough_runtime_reuse_survives_host_cache_clear` and
   `compat_one_napi_call_audit`.
7. Relation explanations lease their operands for the dispatcher's lifetime
   in a capture build, so the public-carrier drain proof
   (`a_live_dispatchers_discarded_results_release_while_retained_ones_outlive_it`)
   holds only in the default build. The explanation state is relation-owned
   and recorded here for the relation inventory.
