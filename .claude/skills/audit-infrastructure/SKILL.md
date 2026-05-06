---
name: audit-infrastructure
description: "Verter audit infrastructure — RequestAuditRecord, RequestKind variants, producer entry-points, AuditRequestRegistration lifecycle, HostAuditRuntime, NAPI/WASM bindings, BatchAuditAggregator"
---

# Audit Infrastructure

Verter's audit infrastructure provides per-request observability for every public host entry-point: component-meta resolution, compile, semantic analysis, type resolution, workspace operations, LSP request handlers, MCP tool invocations, and bundler-batch summaries. Each audited request produces one `RequestAuditRecord` envelope carrying timing, memory, store counters, scheduler attribution, per-file reads, optional semantic footprint, and a strongly-typed kind-specific payload.

For end-user API reference and debug workflows see [`docs/audit-footprint/`](../../docs/audit-footprint/).

## Architecture Overview — Substrate Vs Session

Audit state is split between a leaf substrate crate (`verter_audit`) and the session crate (`verter_session`). The split is enforced architecturally — `verter_audit` may depend only on `verter_span` plus ecosystem crates, never on `verter_session` or any other `verter_*` crate.

| Layer | Crate | Owns |
| --- | --- | --- |
| **Substrate** (DTOs + observer trait) | `verter_audit` | `RequestAuditRecord`, `RequestKind`, `RequestKindPayload`, per-kind payload structs, `AuditObserver` trait, `current_observer()` TLS accessor, `NoOpObserver`, `AuditConfig` + `AuditConsumerFilter`, `BatchAuditAggregator` + `AuditRecordSource`, `IncidentalFields` masking trait, `WALKER_DEPTH_CAP` |
| **Session** (lifecycle + runtime) | `verter_session` | `HostAuditRuntime`, `AuditRequestRegistration::{Active, Noop}`, `AuditRecordsStore`, `RequestContext` (implements `AuditObserver`), `RequestContextGuard`, peak-RSS sampler thread, `LspAuditSession`, audited entry-points (`compile_with_audit`, `analyze_with_audit`, `resolve_type_with_audit`, `audit_workspace_op`, `audit_mcp_tool_call`, `get_component_meta_with_resolution`) |

Substrate/session isolation is mechanically enforced by the `verter_audit_no_upward_deps` guard (rejects any `verter_*` dependency in `verter_audit/Cargo.toml` other than `verter_span`) and the `audit_substrate_isolation` guard (rejects any `use verter_*` reference under `crates/verter_audit/src/` other than `verter_span`).

## `RequestAuditRecord` Envelope

The top-level record (`crates/verter_audit/src/record.rs`) is the single shape every consumer reads:

| Field | Type | Description |
| --- | --- | --- |
| `request_id` | `u64` (decimal-string transport) | Monotonic id stamped at the public entry-point. Unique per audited request |
| `canonical_id` | `String` | Canonical file id the request targeted. Empty for kinds without a single canonical (some MCP tools) |
| `kind` | `RequestKind` | Discriminant naming the producer surface |
| `parent_request_id` | `Option<String>` | Correlation id for nested audited requests (sniffed from the scheduler-side TLS slot at construction) |
| `from_cache` | `bool` | `true` when the request was satisfied from a warm result cache |
| `timings` | `RequestTimingAudit` | Per-phase wall-clock timings (ms) |
| `memory` | `RequestMemoryAudit` | RSS snapshots (before/after/delta + peak from sampler) |
| `store` | `RequestStoreAudit` | Generic store/view counters |
| `footprint` | `Option<RequestFootprintAudit>` | Semantic footprint (component-meta only, gated by `HostConfig::footprint_capture`) |
| `scheduler` | `Option<SchedulerAudit>` | Scheduler-side attribution at first dispatch (native only) |
| `files` | `Vec<FileAudit>` | Per-file attribution deduplicated by canonical id |
| `waits` | `Option<WaitAudit>` | Lock + queue contention (gated by `audit_timing_capture`) |
| `kind_payload` | `RequestKindPayload` | Strongly-typed payload paired with `kind` |

### `RequestKind` Variants

| Variant | Payload | Producer |
| --- | --- | --- |
| `ComponentMeta` | `ComponentMetaPayload` | `VerterHost::get_component_meta_with_resolution` |
| `TypeResolution` | `TypeResolutionPayload` | `VerterHost::resolve_type_with_audit` |
| `SemanticAnalysis` | `SemanticAnalysisPayload` | `VerterHost::analyze_with_audit` |
| `Compile { target: CompileTargetTag }` | `CompilePayload` | `VerterHost::compile_with_audit` / `compile_with_audit_options` |
| `Workspace { op: WorkspaceOp }` | `WorkspacePayload` | `VerterHost::audit_workspace_op` |
| `Lsp { method: LspMethodTag }` | `LspRequestPayload` | `verter_lsp::audit_harness::run_with_audit` (per LSP handler) |
| `Mcp { tool: String }` | `McpToolPayload` | `VerterHost::audit_mcp_tool_call` |
| `BundlerBatch { kind: BundlerKindTag }` | `BundlerBatchPayload` | `BatchAuditAggregator::summarize` |
| `Custom { name: String }` | `RequestKindPayload::None` | Open-ended escape hatch |

### Typed Payload Accessors

`RequestAuditRecord` exposes typed accessors so tests and consumers can fetch the kind-specific payload without pattern-matching:

- `component_meta_payload() -> Option<&ComponentMetaPayload>`
- `type_resolution_payload() -> Option<&TypeResolutionPayload>`
- `compile_payload() -> Option<&CompilePayload>`
- `semantic_analysis_payload() -> Option<&SemanticAnalysisPayload>`
- `workspace_payload() -> Option<&WorkspacePayload>`
- `lsp_payload() -> Option<&LspRequestPayload>`
- `mcp_payload() -> Option<&McpToolPayload>`
- `bundler_batch_payload() -> Option<&BundlerBatchPayload>`

Each returns `None` when `kind_payload` is not the matching variant.

## Producer Entry-Points

Every public audited entry-point follows the same lifecycle: stamp a request id, build a `RequestContext` keyed by the matching `RequestKind`, construct an `AuditRequestRegistration` BEFORE installing the TLS guard, run the producer body under either `RequestContextGuard` (active) or `install_noop_observer()` (filtered), assemble the typed payload from per-request counters, and finalise through the registration. Filtered kinds short-circuit to `None`; the producer body always runs regardless of audit state.

### Component-Meta

`VerterHost::get_component_meta_with_resolution(canonical_id, mode)` is the canonical component-meta entry-point. It returns `(Option<ComponentMetaAnalysis>, Option<ResolvedComponentMetaState>)`. The audit record is published into the host's bounded `AuditRecordsStore` and drained via `VerterHost::take_audit_record(request_id)`.

The `AuditedRequest` builder (`crates/verter_session/src/audited_request.rs`) wraps one such call in a request-scoped audit harness that resets per-thread counters, validates exactly one request was created, and returns `(ComponentMetaAnalysis, ResolvedComponentMetaState, RequestAuditRecord)` as a single triple. `AuditedRequestBuilder::resolve_component_meta` is the test-facing convenience surface; `AuditedRequestBuilder::run_custom` lets a closure issue arbitrary single-request audited work.

### Compile

`VerterHost::compile_with_audit(canonical_id, target) -> (VerterCompileResult, Option<RequestAuditRecord>)` and the variant `compile_with_audit_options(canonical_id, target, verter_options)` for callers that need explicit `force_vapor` / `force_js` control. The `target` bitset maps to a `CompileTargetTag` (`Vdom`, `Ide`, `Vapor`) on the record's `kind` discriminant. Producer-side instrumentation in `verter_compiler` emits `record_phase_timing` at parse / transform / codegen / css_analysis / sourcemap boundaries and `record_event(CompileCodeTransformOp)` at every `CodeTransform` operation entry — the session-side `RequestContext` accumulates these into per-request atomics that `assemble_compile_payload` reads at finalize time.

### Semantic Analysis

`VerterHost::analyze_with_audit(canonical_id) -> (Option<AnalysisReady>, Option<RequestAuditRecord>)`. Probes the `IndexedReadyDb` cache before constructing the registration so the observed `from_cache` state is unaffected by the audit work itself. Audit-disabled fast path runs `materialize_analysis_ready` with no `RequestContextGuard` installed.

### Type Resolution

`VerterHost::resolve_type_with_audit(query: SemanticQueryKey, canonical_hint: &str) -> (Option<TypeResolutionResult>, Option<RequestAuditRecord>)`. Drives one `ProjectSemanticDispatch::execute(query)` call inside the audit window. The `TypeResolutionPayload` reports the caller's projection mode (derived from the query variant) plus per-mode counters mined off the active `RequestContext`.

### Workspace

`VerterHost::audit_workspace_op(op: WorkspaceOp) -> RequestAuditRecord`. Drives `WorkspaceAccess::audit_op(op)` under audit. The trait method itself does not enter the active-request registry — the wrapper constructs the `AuditRequestRegistration` first so the registry slot precedes the workspace traversal. Returns the record unconditionally; the `Noop` arm only suppresses the records-store side effect, not the producer's output.

### LSP

`verter_lsp::audit_harness::run_with_audit(host, method, canonical_id, position, budget, body, populate)` wraps each LSP handler future in:

1. An `LspAuditSession` keyed by `LspMethodTag` and the canonical (constructed via `VerterHost::lsp_audit_begin`).
2. `tokio::time::timeout(budget, body)` driving the per-method budget from `LspMethodTimeoutsConfig`.
3. `finalize_ok(payload)` on success or `finalize_cancelled()` on timeout (publishing the cancellation marker).
4. Optional drain to `VERTER_LSP_AUDIT_TRACE_OUT` (JSON-lines append, configurable via env var).

Audit-disabled fast path returns `body.await` directly without any registration cost.

### MCP

`VerterHost::audit_mcp_tool_call(tool_name, canonical_id, args_size_bytes, f) -> (T, Option<RequestAuditRecord>)` wraps a closure `FnOnce(&Arc<Self>) -> McpToolOutcome<T>` under audit. The closure's `McpToolOutcome { value, result_size_bytes, error }` carries the two facts the wrapper cannot infer (response size and optional error message). Sub-requests the closure spawns inherit the MCP request's id as their `parent_request_id` via the scheduler-side TLS slot.

## `AuditRequestRegistration` Lifecycle

Every audited entry-point allocates exactly one `AuditRequestRegistration` (`crates/verter_session/src/host_audit_runtime.rs`):

```text
AuditRequestRegistration ::= Active(ActiveRegistration) | Noop
```

- **`Active`** — captures a `Weak<RequestContext>` slot in `HostAuditRuntime::active_requests`. `finalize(record)` atomically removes the slot and publishes the record into `AuditRecordsStore` (idempotent — first call wins). `Drop` defensively sweeps the slot when `finalize` did not run (panic / cancellation paths).
- **`Noop`** — returned when `AuditConfig::consumer_filter` rejects the request's `RequestKind`. Holds no state; `finalize` returns `false` and downstream emits no record.

The three lifecycle methods on `HostAuditRuntime` (`register_active_request`, `finalize_active_request`, `drop_active_request`) are crate-private and have exactly ONE in-tree call site each, all in `host_audit_runtime.rs`. The `audit_request_registration_lifecycle` architecture guard mechanically enforces this — adding a caller anywhere else fails the build.

## Substrate TLS — `current_observer()`

Lower crates emit audit signals through `verter_audit::current_observer() -> Option<Arc<dyn AuditObserver>>` (declared in `crates/verter_audit/src/observer.rs`). The accessor reads a thread-local slot installed by either `RequestContextGuard::install` (active path) or `install_noop_observer()` (filtered path).

The `AuditObserver` trait carries default no-op implementations so producers only override what they care about:

- `record_event(event: AuditEvent)` — counter-style attribution (`InflightAbortedRetry`, `ColdAbortSwept`, `CompileCodeTransformOp`).
- `record_cache_event(layer: &'static str, hit: bool)` — per-layer hit/miss decision.
- `record_file(canonical_id, layer: VfsLayer, bytes_read, cache_hit)` — workspace file read.
- `record_lock_acquisition(lock_name: &'static str, wait_ns: u64)` — single lock acquisition wait.
- `record_phase_timing(phase: &'static str, elapsed_ms: f64)` — phase-boundary timing.
- `record_scheduler_dispatch(audit: SchedulerAudit)` — first-dispatch attribution (subsequent calls bump the dispatch counter).

The session-side `RequestContext` provides full implementations; the substrate's `NoOpObserver` leaves them defaulted. The `audit_observer_single_accessor` architecture guard enforces that the five lower crates (`verter_compiler`, `verter_semantic`, `verter_workspace`, `verter_lsp`, `verter_mcp_server`) reach audit state ONLY through `verter_audit::current_observer()` — the session-internal `current_request_context()` typed accessor is forbidden in those crates.

## Consumer Filter (Install-Time)

`AuditConfig::consumer_filter` (`crates/verter_audit/src/config.rs`) is a `u32` bitset deciding which `RequestKind` variants emit records. Bits are positionally stable via the `KindBit` enum (`ComponentMeta = 0`, `TypeResolution = 1`, `SemanticAnalysis = 2`, `Compile = 3`, `Workspace = 4`, `Lsp = 5`, `Mcp = 6`, `BundlerBatch = 7`, `Custom = 8`).

| Constructor | Behaviour |
| --- | --- |
| `AuditConsumerFilter::default()` / `allow_all()` | Allow every kind |
| `deny_all()` | Reject every kind |
| `allow_only([KindBit::…, …])` | Allow only the listed kinds |
| `.allow(KindBit::…)` / `.deny(KindBit::…)` | Toggle a single bit (chainable) |

The filter is read ONCE at registration time inside `AuditRequestRegistration::new` and CANNOT change for that request's lifetime — filtered kinds skip `active_requests` and never produce a record. The current `AuditConfig` snapshot is mirrored from `HostConfig` flags in `host_construction.rs` (today only `audit_timing_capture` is wired through; the consumer filter defaults to allow-all). Tests that need a non-default filter swap the runtime's `AuditConfig` via a test-only helper without bypassing the `active_requests` privacy boundary.

## `HostAuditRuntime` & Sampler Thread

`HostAuditRuntime` (`crates/verter_session/src/host_audit_runtime.rs`) wraps the `AuditRecordsStore` instance, the `AuditConfig` snapshot, and the active-request registry. Each `VerterHost` owns one independent runtime; multiple hosts in one process do NOT share audit state.

### Public surface

- `audit_config() -> Arc<AuditConfig>` — borrow the config snapshot.
- `audit_records_store() -> &Arc<AuditRecordsStore>` — borrow the records store.
- `snapshot() -> AuditRuntimeSnapshot` — read-only view of `(active_request_count, active_request_ids, records_store_size, records_store_capacity)`.
- `take_record(request_id) -> Option<RequestAuditRecord>` — drain a specific record.

The `audit_records_store` is bounded — `AUDIT_RECORDS_STORE_CAPACITY = 256`. Insertion at capacity evicts the oldest entry by insertion order.

### Peak-RSS sampler thread (native only)

On native targets, each runtime owns at most ONE peak-RSS sampler thread:

- Spawns lazily on the first `AuditRequestRegistration::new` call when `AuditConfig::audit_timing_capture` is on (single-shot start latch via `compare_exchange`).
- Holds a `Weak<HostAuditRuntime>` so the runtime ↔ thread cycle is broken — runtime drop releases the strong count, the next `weak.upgrade()` returns `None`, the thread terminates.
- Ticks every 50 ms and writes `fetch_max(current_process_rss())` into each in-flight request's `process_rss_peak_bytes` slot.
- The runtime's `Drop` impl explicitly joins the handle so dropped hosts do not leak threads. Process-static `SAMPLER_THREAD_SPAWN_COUNT` and `SAMPLER_THREAD_JOIN_COUNT` counters discriminate "sampler did not spawn" from "sampler spawned but did not join".

WASM targets are gated off via `#[cfg(not(target_arch = "wasm32"))]` — no sampler thread, `process_rss_peak_bytes` stays at `0` regardless of `audit_timing_capture` state.

## Architecture Guards

The audit infrastructure ships with a set of mechanical guards that enforce the architectural invariants. All live in `crates/verter_session/tests/architecture_guards.rs` unless noted:

| Guard | Role |
| --- | --- |
| `verter_audit_no_upward_deps` | `verter_audit/Cargo.toml` may declare only `verter_span` from the `verter_*` namespace |
| `audit_substrate_isolation` | Source files under `crates/verter_audit/src/` may `use` only `verter_span`, `std`, and external crates — no other `verter_*` reference |
| `audit_request_registration_lifecycle` | The three lifecycle methods (`register_active_request`, `finalize_active_request`, `drop_active_request`) on `HostAuditRuntime` have exactly ONE in-tree caller each, all inside `host_audit_runtime.rs` |
| `audit_observer_single_accessor` | The five lower crates (`verter_compiler`, `verter_semantic`, `verter_workspace`, `verter_lsp`, `verter_mcp_server`) reach the substrate ONLY via `verter_audit::current_observer()` — `current_request_context` is forbidden |
| `audit_no_hot_loop_instrumentation` | Phase-boundary instrumentation only; the canonical `(crate, function_path)` denylist forbids `current_observer()` calls inside hot-loop bodies |
| `audit_counter_single_helper` | The two `record_inflight_aborted_retry` / `record_cold_abort_swept` increments live in helper bodies only — no inline `fetch_add` callers anywhere else in the codebase |
| `wave_3_entry_points_propagate_tls` | Each audited `*_with_audit` entry-point has at least one paired test that drives it AND calls `assert_observer_reaches(...)` so TLS propagation is mechanically verified |
| `audit_ts_bindings_are_in_sync` (in `tests/ts_bindings.rs`) | `packages/types/audit.generated.ts` matches what `ts-rs` would regenerate from current Rust DTOs |

The general `no_phase_archaeology_in_production_code` and `external_corpus_paths_not_present_outside_gated_tests` guards apply across the workspace, including audit code.

## NAPI / WASM Bindings

The native and WASM hosts expose the same set of typed audited entry-points alongside two non-destructive query surfaces:

| JS export | Rust binding | Returns |
| --- | --- | --- |
| `getComponentMetaWithAudit` | `MetaSession::get_component_meta_with_audit` | `Buffer` (JSON `{ payload, audit }`) |
| `compileWithAudit` | `VerterHost::compile_with_audit` | `Buffer` (JSON record) |
| `analyzeWithAudit` | `VerterHost::analyze_with_audit` | `Buffer` (JSON record) |
| `resolveTypeWithAudit` | `VerterHost::resolve_type_with_audit` | `Buffer` (JSON record) |
| `auditWorkspaceOp` | `VerterHost::audit_workspace_op` | `Buffer` (JSON record) |
| `getLastAuditRecord` | drains the most recent record from `AuditRecordsStore` | `Buffer` (JSON record or empty) |
| `getAuditRecords({ kind?, sinceRequestId?, limit? })` | non-destructive filtered query | `Buffer` (JSON array) |
| `getBundlerBatchSummary({ kind?, sinceRequestId? })` | invokes `BatchAuditAggregator` over the store | `Buffer` (JSON `BundlerBatchPayload`) |

NAPI bindings live in `crates/verter_napi/src/audit.rs` (helper types + decoders) and the inline `#[napi] impl NapiVerterHost` block in `crates/verter_napi/src/lib.rs`. WASM bindings live in `crates/verter_wasm/src/audit.rs` + `crates/verter_wasm/src/lib.rs`. All exports return `Buffer` (JSON UTF-8 payload) for parity with the original `getComponentMetaWithAudit` contract; consumers decode against `@verter/types/audit.generated.ts`.

## `BatchAuditAggregator`

`BatchAuditAggregator` (`crates/verter_audit/src/batch.rs`) folds an `AuditRecordSource` into a `BundlerBatchPayload`. The substrate stays leaf — the aggregator depends only on the trait callback contract and never on owning crates:

```text
trait AuditRecordSource {
    fn for_each_record(&self, f: &mut dyn FnMut(Instant, &RequestAuditRecord));
}
```

`AuditRecordsStore` implements `AuditRecordSource`; non-destructive iteration exposes each stored record together with its insertion `Instant`. `BatchAuditAggregator::summarize(since)` partitions the window by `RequestKind`, accumulates total duration, total bytes parsed, `from_cache_count`, and `cache_hit_rate`, and tracks the top-`SLOWEST_RECORD_LIMIT` (= 5) slowest records as `SlowRecordSummary` entries. Empty sources yield a zeroed payload with no division-by-zero on `cache_hit_rate`.

`since` filters records inserted strictly after the supplied `Instant`. Bundler integrations call `summarize(Some(last_summary_instant))` on every flush so each batch reports only the work completed since the last call.

## Tests & TLS-Propagation Harness

`verter_session::tests::audit_tls_harness::assert_observer_reaches(install_audit, f)` is the primary verification primitive for TLS propagation. It runs the supplied closure under either a `RequestContextGuard` (`install_audit = true`) or no guard (`install_audit = false`, the control case), records whether `verter_audit::current_observer().is_some()` was visible on the calling thread, and exposes a `WorkerSinkHandle` so workers spawned inside the closure can report their own observation via `report_worker_observer_presence`.

Worker threads spawned bare via `std::thread::spawn` get a fresh TLS slot by construction. Closures that need observer propagation into a worker pool must either install the guard again on the worker or rely on a runtime that already plumbs `RequestContextGuard` through to its workers (the production scheduler does this for its rayon pool).

The `wave_3_entry_points_propagate_tls` architecture guard pins the `(entry_point_symbol, paired_test_files)` invariant — every `*_with_audit` entry-point has at least one test that both invokes the symbol AND calls `assert_observer_reaches(...)`. Tests living in `crates/verter_session/tests/tls_harness_in_crate.rs`, `tls_harness_cross_crate.rs`, and `semantic_analysis_audit_tls_propagation.rs` exercise the harness across in-crate, cross-crate, and analysis-specific propagation.

## Key Files

| File | Role |
| --- | --- |
| `crates/verter_audit/src/lib.rs` | Substrate root + re-exports |
| `crates/verter_audit/src/record.rs` | `RequestAuditRecord`, `RequestKind`, `RequestKindPayload`, `IncidentalFields` |
| `crates/verter_audit/src/observer.rs` | `AuditObserver` trait, `current_observer()`, `install_observer` guard |
| `crates/verter_audit/src/noop.rs` | `NoOpObserver`, `install_noop_observer()` |
| `crates/verter_audit/src/config.rs` | `AuditConfig`, `AuditConsumerFilter`, `KindBit` |
| `crates/verter_audit/src/payloads/` | Per-`RequestKind` payload data structs |
| `crates/verter_audit/src/batch.rs` | `BatchAuditAggregator`, `AuditRecordSource`, `SLOWEST_RECORD_LIMIT` |
| `crates/verter_session/src/host_audit_runtime.rs` | `HostAuditRuntime`, `AuditRequestRegistration`, sampler thread |
| `crates/verter_session/src/component_meta_audit/audit_records_store.rs` | `AuditRecordsStore`, capacity = 256 |
| `crates/verter_session/src/audited_request.rs` | `AuditedRequest` builder + run-custom harness |
| `crates/verter_session/src/host_compile_audit.rs` | `VerterHost::compile_with_audit` |
| `crates/verter_session/src/host_analyze_audit.rs` | `VerterHost::analyze_with_audit` |
| `crates/verter_session/src/host_resolve_type_audit.rs` | `VerterHost::resolve_type_with_audit` |
| `crates/verter_session/src/host_workspace_audit.rs` | `VerterHost::audit_workspace_op` |
| `crates/verter_session/src/host_mcp_audit.rs` | `VerterHost::audit_mcp_tool_call`, `McpToolOutcome` |
| `crates/verter_session/src/host_lsp_audit.rs` | `LspAuditSession`, `lsp_audit_begin` |
| `crates/verter_lsp/src/audit_harness.rs` | `run_with_audit`, `payload_with_position`, `drain_to_trace_out` |
| `crates/verter_session/src/tests/audit_tls_harness.rs` | `assert_observer_reaches`, `WorkerSinkHandle`, `report_worker_observer_presence` |
| `crates/verter_napi/src/audit.rs` + `crates/verter_napi/src/lib.rs` | NAPI typed entry-points |
| `crates/verter_wasm/src/audit.rs` + `crates/verter_wasm/src/lib.rs` | WASM typed entry-points |
| `packages/types/audit.generated.ts` | TS bindings (regenerated via `ts-rs`) |
