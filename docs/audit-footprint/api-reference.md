# Audit Footprint — API reference

Audit DTOs and the producer-side observer trait live in
[`verter_audit`](https://github.com/pikax/verter/blob/main/crates/verter_audit/src/lib.rs) — the leaf
observability substrate. Higher-level concrete machinery
(`HostAuditRuntime`, `AuditRecordsStore`, the per-request
accumulator, the footprint miner, the `AuditRequestRegistration`
lifecycle) lives in
[`verter_session`](https://github.com/pikax/verter/blob/main/crates/verter_session/src/host_audit_runtime.rs).
TS bindings are generated from Rust via `ts-rs` into
[`packages/types/audit.generated.ts`](../../packages/types/audit.generated.ts);
the sync test `audit_ts_bindings_are_in_sync` fails with a unified
diff if the two drift.

## Substrate vs session split

| Layer                   | Crate            | Owns                                                                                                                                                          |
| ----------------------- | ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Substrate (DTOs)        | `verter_audit`   | `RequestAuditRecord` envelope, `RequestKind` / `RequestKindPayload`, all eight payload structs, footprint records, derivation graph DTOs, `StructuredAuditEvent`, `AuditObserver` trait, `AuditEvent`, `NoOpObserver`, `current_observer` TLS, `AuditConfig` + `AuditConsumerFilter` |
| Session (host runtime)  | `verter_session` | `HostAuditRuntime`, `AuditRecordsStore`, `AuditRequestRegistration` (lifecycle), `RequestContext` + TLS guard, accumulator, footprint miner, peak-RSS sampler, structured-trace macros, `AuditedRequest` test harness                                                                |

The substrate is a leaf — it depends only on `verter_span` and has no
back-edge to higher crates. Lower crates (`verter_compiler`,
`verter_workspace`, `verter_scheduler`, …) emit through the
[`AuditObserver`] trait without knowing whether a `HostAuditRuntime`
is installed; the session-side `RequestContext` is the production
implementer of that trait.

## Record types

Top-level envelope and shared sub-records (substrate):

| Rust type (`verter_audit::…`)                | Summary                                                                                |
| -------------------------------------------- | -------------------------------------------------------------------------------------- |
| `record::RequestAuditRecord`                 | Top-level envelope. Carries kind + typed payload, timings, store/memory, footprint, scheduler audit, files, waits. |
| `record::RequestKind`                        | Producer-surface discriminant — `ComponentMeta`, `TypeResolution`, `SemanticAnalysis`, `Compile { target }`, `Workspace { op }`, `Lsp { method }`, `Mcp { tool }`, `BundlerBatch { kind }`, `Custom { name }`. |
| `record::RequestKindPayload`                 | Strongly-typed payload paired with `RequestKind` (variant tag matches kind discriminant). |
| `record::RequestPhaseAudit`                  | Per-phase timing record (name + ms).                                                   |
| `timing::RequestTimingAudit`                 | Phase timings in milliseconds (`f64`).                                                 |
| `memory::RequestMemoryAudit`                 | RSS + host-cache + workspace byte snapshots.                                           |
| `store::RequestStoreAudit`                   | Kind-agnostic store/view counters + materialiser counters + cache-layer breakdown.     |
| `footprint::RequestFootprintAudit`           | Derived footprint — see below.                                                         |
| `scheduler::SchedulerAudit`                  | Scheduler dispatch facts (worker-pool counts, depths).                                 |
| `waits::WaitAudit`                           | Lock-wait + queue-wait nanosecond totals (gated on `audit_timing_capture`).            |
| `files::FileAudit` / `files::FileRole`       | Per-file attribution (Entry / DirectImport / TransitiveImport / TypeDep / IndexedReadyBuild / NotLoaded / ResolverWalk). |

### Per-`RequestKind` payload structs

Each variant of `RequestKind` carries a matching `RequestKindPayload`
arm. The payload structs live under
[`verter_audit::payloads`](https://github.com/pikax/verter/blob/main/crates/verter_audit/src/payloads/mod.rs):

| Payload (`verter_audit::payloads::…`)        | Paired `RequestKind`              | Notes                                                                                          |
| -------------------------------------------- | --------------------------------- | ---------------------------------------------------------------------------------------------- |
| `component_meta::ComponentMetaPayload`       | `ComponentMeta`                   | Materializer-specific store counters + solver counters (only meaningful for component-meta).   |
| `type_resolution::TypeResolutionPayload`     | `TypeResolution`                  | Per-query-mode counters for `resolver_core`.                                                   |
| `semantic::SemanticAnalysisPayload`          | `SemanticAnalysis`                | Counters for an `AnalysisReady` build.                                                         |
| `compile::CompilePayload`                    | `Compile { target }`              | Per-phase compile timings, codegen counts, `code_transform_ops`.                               |
| `workspace::WorkspacePayload`                | `Workspace { op }`                | `WorkspaceOp` is one of `AuditResolve { specifier, from }`, `DepGraphTraverse { root }`, `ResolverWalk { specifier }`. |
| `lsp::LspRequestPayload`                     | `Lsp { method }`                  | Per-request LSP method counters; carries `PositionInfo` when applicable.                       |
| `mcp::McpToolPayload`                        | `Mcp { tool }`                    | Tool name, arg/result sizes, optional error message.                                           |
| `bundler::BundlerBatchPayload`               | `BundlerBatch { kind }`           | Aggregated batch summary; carries `SlowRecordSummary` entries.                                 |

`RequestKindPayload::None` is used when the producer has not
populated a typed payload yet — the envelope still carries the
generic timing/memory/store/footprint data.

### Stringly-typed kind tags

Variants that need a small parameter use mirror enums from
[`verter_audit::payloads::tags`](https://github.com/pikax/verter/blob/main/crates/verter_audit/src/payloads/tags.rs)
so the substrate stays decoupled from owning crates' concrete types:

- `CompileTargetTag` — `Vdom` / `Vapor` / `Ide`.
- `BundlerKindTag` — `Vite` / `Webpack` / `Rollup` / `Esbuild` / `Rolldown` / `Other`.
- `LspMethodTag` — `Hover` / `GotoDefinition` / `Completion` / `References` / `Diagnostics` / `DocumentSymbols` / `SemanticTokens` / `InlayHints` / `CodeAction` / `Rename` / `Other`.
- `ProjectionModeTag` — `Identity` / `Navigate` / `Shallow` / `Expanded` / `Skeleton`.

### `RequestStoreAudit` materialiser counters

Six `u64` counters (decimal-string serialized, `string` in TS) on
`RequestStoreAudit` for the session-layer materialiser:

| Field                              | Meaning                                                                                |
| ---------------------------------- | -------------------------------------------------------------------------------------- |
| `materialize_structure_calls`      | Total `materialize_component_meta_structure` invocations during the request.           |
| `materialize_structure_cache_hits` | Subset satisfied by the materialiser's `MaterializeStructureDb` peek (warm cache).     |
| `node_arena_lock_acquisitions`     | Lock acquisitions on the per-scope `NodeArena` dedup index.                            |
| `family_map_lock_acquisitions`     | Lock acquisitions on the family-map dep-signature reverse index.                       |
| `dep_signature_merges`             | Times a `dep_signature` was merged into the materialiser's `local_fence`.              |
| `dep_signature_intern_hits`        | Subset of `dep_signature_merges` that hit an existing intern bucket (no allocation).   |

Cache hit rate: `materialize_structure_cache_hits /
materialize_structure_calls` — should be `> 0` on warm/cold-seq
passes (warm peek satisfies repeat lookups). Intern hit rate:
`dep_signature_intern_hits / dep_signature_merges` — exercises
the content-hash bucketed `Weak`-ref interner.

### Cache-outcome enum

`origin_graph::CacheOutcomeKind` (used by both
`RequestStoreAudit::cache_outcomes` and
`StructuredAuditEvent::MaterializeStructureExit`):

| Variant                | Meaning                                                                  |
| ---------------------- | ------------------------------------------------------------------------ |
| `Hit`                  | Warm cache hit.                                                          |
| `Miss`                 | Cache miss (no entry present).                                           |
| `JoinedWait`           | Joined a peer's in-flight slot and waited.                               |
| `Sentinel`             | Observed a sentinel (placeholder) entry.                                 |
| `ColdBuild`            | Performed a cold build from source.                                      |
| `InflightAbortedRetry` | Retry loop after an in-flight slot was aborted.                          |
| `ColdAbortSwept`       | Cold entry reaped during generation reconciliation.                      |
| `Tainted`              | Path-dependent outcome (depth-fuse trip, scope-unloaded mid-compute, or `Recursive` sub-call). Non-cacheable; propagates as `MaterializeOutcome::Tainted`. |

### Materialise-skip-reason enum

`origin_graph::MaterializeSkipReason` (carried by
`StructuredAuditEvent::MaterializeStructurePolicySkip`):

| Variant                                | Meaning                                                                 |
| -------------------------------------- | ----------------------------------------------------------------------- |
| `FunctionPropertyAtNested`             | Object-property lookup hit a function-typed property at `Nested` axis.  |
| `GenericRefWithArgsTopLevel`           | Top-level generic ref carried explicit type arguments (reserved for the `InstantiationRef` arm). |
| `PackageRefTopLevel`                   | Top-level ref resolved under `node_modules/` — package types stay opaque. |
| `RegistryRouteNotInlineMaterialisable` | Registry-route check rejected the input as not inline-materialisable (e.g. `Pick`/`Omit` over a non-bare root). |
| `NonStructuralTopLevel`                | Top-level shape is non-structural (primitive, literal, type-param, etc.). |
| `RegistryRouteCycleGuard`              | Registry-route walk detected a cycle and stopped.                       |
| `RecursiveHelperCycleGuard`            | Recursive-helper traversal stopped on a cycle.                          |

### Footprint

`footprint::RequestFootprintAudit` fields, each a `Vec<R>` where `R`
derives `serde + ts-rs::TS`:

- `indexed_ready_builds: Vec<IndexedReadyBuildRecord>`
- `vfs_reads: Vec<VfsReadRecord>`
- `shared_load_reuses: Vec<SharedLoadReuseRecord>`
- `instantiations: Vec<InstantiationRecord>`
- `projections: Vec<ProjectionRecord>`
- `conditional_decisions: Vec<ConditionalRecord>`
- `substitutions: Vec<SubstitutionRecord>`
- `alias_resolutions: Vec<AliasResolveRecord>`
- `materializations: Vec<MaterializationRecord>`
- `cache_outcomes: CacheOutcomeTally` (exact per-context counters).
- `graph_completeness: GraphCompletenessReport`
- `derivation_subgraph: DerivationSubgraph`

Methods:

- `loaded_files() -> Vec<Arc<str>>` — union of `vfs_reads` +
  `shared_load_reuses`, sorted + deduplicated.
- `declared_dependency_files() -> Vec<Arc<str>>` — broader union
  including `indexed_ready_builds`.
- `mask_incidental_spans() -> Self` — clones with flaky fields
  stripped (VFS reads). Snapshot tests use this.

## Derivation subgraph

`origin_graph::DerivationSubgraph { nodes: Vec<NodeRecord>, edges: Vec<DerivationEdgeRecord> }`.

- `NodeId(u32)` and `EdgeId(u32)` are in-audit opaque indices.
- `NodeRecord { kind: SemanticNodeKind, named_identity: Option<NamedIdentity>, structural_hash: Hash16, display_label: Arc<str> }`.
- `DerivationEdgeRecord { result: NodeId, kind: OriginEdgeKind, sources: Vec<NodeId>, meta: OriginEdgeMetaDto }`.
- `SemanticNodeKind` is `#[non_exhaustive]` — forward-compatible
  with future variants via the `Other { name }` catch-all.

Sort keys (deterministic NodeId assignment):

- Nodes: `(kind as u32, structural_hash, named_identity.map(|n| (canonical_id, symbol_name, args_fingerprint)))`.
- Edges: `(result, kind as u32, sources)`.

Identical requests produce identical serialized footprints
regardless of thread interleaving.

## Walker

```rust
impl RequestAuditRecord {
    pub fn why_loaded(&self, canonical_id: &str) -> ProvenanceChain;
    pub fn why_instantiated(
        &self,
        decl_canonical_id: &str,
        decl_symbol_name: &str,
        args_fingerprint: Hash16,
    ) -> ProvenanceChain;

    pub fn assert_loaded_files_exactly<I, S>(&self, expected: I) -> Result<(), AssertionDiff>
    where I: IntoIterator<Item = S>, S: AsRef<str>;

    pub fn assert_declared_dependency_files_exactly<I, S>(&self, expected: I) -> Result<(), AssertionDiff>
    where I: IntoIterator<Item = S>, S: AsRef<str>;
}
```

`ProvenanceChain` carries `steps: Vec<ProvenanceStep>`,
`terminated: ChainTermination`, and `shared_load_terminals: Vec<SharedLoadReuseRecord>`.
Depth cap is `verter_audit::record::WALKER_DEPTH_CAP = 256`.

## Observer trait

[`verter_audit::observer::AuditObserver`](https://github.com/pikax/verter/blob/main/crates/verter_audit/src/observer.rs)
is the producer-side hook. Lower crates emit through
`current_observer()` (TLS) without knowing whether the consumer is
the session-side `RequestContext`, the `NoOpObserver`, or a test
fake:

```rust
pub trait AuditObserver: Send + Sync {
    fn record_event(&self, _event: AuditEvent) {}
    fn record_cache_event(&self, _layer: &'static str, _hit: bool) {}
    fn record_file(&self, _canonical_id: &str, _layer: VfsLayer, _bytes_read: u64, _cache_hit: bool) {}
    fn record_lock_acquisition(&self, _lock_name: &'static str, _wait_ns: u64) {}
    fn record_phase_timing(&self, _phase: &'static str, _elapsed_ms: f64) {}
    fn record_scheduler_dispatch(&self, _audit: SchedulerAudit) {}
}
```

`AuditEvent` is the compact counter-style tag for events without a
structured payload (`InflightAbortedRetry`, `ColdAbortSwept`,
`CompileCodeTransformOp`). All trait methods default to no-ops so
producers override only what they observe.

`install_observer(...)` returns an `ObserverGuard` (RAII) that
restores the prior observer on drop. `NoOpObserver` is the trivial
implementation that lower crates install when audit is disabled or
the consumer filter rejected the request kind.

## Host runtime — `verter_session::HostAuditRuntime`

The session-side concrete runtime owned by `VerterHost`. Wraps the
records store, the audit-config snapshot, and the active-request
registry; on native targets it also owns the at-most-one peak-RSS
sampler thread.

```rust
impl HostAuditRuntime {
    pub fn new(config: Arc<AuditConfig>, capacity: usize) -> Self;
    pub fn audit_config(&self) -> Arc<AuditConfig>;
    pub fn audit_records_store(&self) -> &Arc<AuditRecordsStore>;
    pub fn snapshot(&self) -> AuditRuntimeSnapshot;
    pub fn take_record(&self, request_id: u64) -> Option<RequestAuditRecord>;
}
```

The active-request registry is strictly behind crate-private surface
methods (`register_active_request`, `finalize_active_request`,
`drop_active_request`, `for_each_active_request`,
`ensure_sampler_started`); the
[`AuditRequestRegistration`](https://github.com/pikax/verter/blob/main/crates/verter_session/src/host_audit_runtime.rs)
lifecycle is the single authority for inserts and removes.

`AuditRuntimeSnapshot` exposes the records-store length, the active
in-flight request count, and the sampler-started flag for
diagnostics.

### `AuditRequestRegistration` lifecycle

`AuditRequestRegistration` is the per-request RAII handle that
brackets a logical audited request:

- `Active(...)` — captures a slot in the host's active-request
  registry; emits a record on `finalize`.
- `Noop` — the `AuditConsumerFilter` rejected the kind at
  registration time; downstream emits no record.

Constructed via `AuditRequestRegistration::new(host, ctx)`.
Finalised via `AuditRequestRegistration::finalize(record)`
(idempotent). A defensive `Drop` impl cleans up the registry entry
on panic / cancellation paths so leaked slots cannot accumulate.

`VerterHost::host_audit_runtime() -> &HostAuditRuntime` and
`VerterHost::host_audit_runtime_arc() -> Arc<HostAuditRuntime>` are
the public accessors.

## Public API surface — `VerterHost::*_with_audit`

Every audited entry-point lives on `VerterHost` and follows the same
shape: stamp request id, install `RequestContextGuard`, run the
underlying operation, finalise the registration, return the record:

| Method                                                                  | Purpose                                                                                 |
| ----------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| `get_component_meta_with_audit(canonical_id)`                           | Audited `getComponentMeta` — the original surface.                                      |
| `resolve_type_with_audit(key, canonical_id)`                            | Single type-resolution query through `ProjectSemanticDispatch`.                         |
| `compile_with_audit(canonical_id, target)`                              | Compile for the requested codegen target (default options).                             |
| `compile_with_audit_options(canonical_id, target, force_js)`            | Compile with explicit options (no defaults).                                            |
| `analyze_with_audit(canonical_id)`                                      | Materialise `AnalysisReady` for the canonical.                                          |
| `audit_workspace_op(op: WorkspaceOp)`                                   | Drive a workspace traversal under audit (`AuditResolve` / `DepGraphTraverse` / `ResolverWalk`). |
| `lsp_audit_begin(method, canonical_id) -> LspAuditSession`              | Open an audited LSP handler session — the handler drives finalize through the returned session. |
| `audit_mcp_tool_call(tool_name, canonical_id, args_size, f)`            | Wrap an MCP tool invocation; the closure returns `McpToolOutcome<T>`.                   |
| `take_audit_record(request_id) -> Option<RequestAuditRecord>`           | Drain a published record by id.                                                         |

`get_component_meta_with_resolution` is the canonical
component-meta entry-point used by tests and consumers; the audit
record is published into the host's records store and can be drained
with `take_audit_record(request_id)` afterwards.

## Harness — `AuditedRequest`

```rust
use verter_session::audited_request::AuditedRequest;

// hermetic
let (analysis, resolution, record) = AuditedRequest::builder()
    .files([("/a.vue", SRC)])
    .resolve("/a.vue")?;

// attach to existing host
let (analysis, resolution, record) = AuditedRequest::builder()
    .attach_to(host)
    .resolve("/a.vue")?;

// closure variant — useful for multi-step fixtures
let (analysis, resolution, record) = AuditedRequest::builder()
    .attach_to(host)
    .run_custom(|host| host.get_component_meta_with_resolution(id))?;
```

Errors: `NestedAuditNotSupported`, `MultipleRequestsInSingleRun`,
`AuditRecordMissing`, `PrerequisitesNotMet`, `ResolutionFailed`.

## NAPI / WASM bindings

NAPI bindings live in
[`crates/verter_napi/src/audit.rs`](https://github.com/pikax/verter/blob/main/crates/verter_napi/src/audit.rs)
and [`crates/verter_napi/src/lib.rs`](https://github.com/pikax/verter/blob/main/crates/verter_napi/src/lib.rs).
Each entry-point wraps a `VerterHost::*_with_audit` Rust producer
and returns the produced `RequestAuditRecord` as a JSON Buffer:

```ts
// packages/native/index.ts
class VerterHost {
  resolveTypeWithAudit(canonicalId: string, declName: string): Buffer | null;
  compileWithAudit(canonicalId: string, target: string): Buffer | null;
  analyzeWithAudit(canonicalId: string): Buffer | null;
  auditWorkspaceOp(op: WorkspaceOpArgument): Buffer;
  getLastAuditRecord(): Buffer | null;
  getAuditRecords(filter?: AuditRecordFilter): Buffer;
  getBundlerBatchSummary(args?: BundlerBatchSummaryArgs): Buffer;
}

class ComponentMetaSession {
  getComponentMetaWithAudit(canonicalOrAlias: string): Buffer | null;
  whyLoadedFromAuditJson(auditJson: string, canonicalId: string): string;
  whyInstantiatedFromAuditJson(
    auditJson: string,
    declCanonicalId: string,
    declSymbolName: string,
    argsFingerprintHex: string,
  ): string;
}
```

Audit must be enabled on the host config (`auditEnabled: true`) for
the producer entry-points to publish a record; otherwise they
short-circuit to `null` (the underlying operation still runs).
`auditWorkspaceOp` always returns a record because the workspace
producer drives the operation regardless of audit configuration.

`getAuditRecords` filter fields are independent — combining them
narrows further:

- `kind`: `"ComponentMeta"`, `"TypeResolution"`,
  `"SemanticAnalysis"`, `"Compile"`, `"Workspace"`, `"Lsp"`, `"Mcp"`,
  `"BundlerBatch"`, `"Custom"`.
- `sinceRequestId`: minimum request id (exclusive) as a decimal
  string.
- `limit`: cap the returned record count (oldest-first).

WASM mirrors the surface via `wasm-bindgen` in
[`crates/verter_wasm/src/lib.rs`](https://github.com/pikax/verter/blob/main/crates/verter_wasm/src/lib.rs)
with the same JSON-shaped return values. `analyze_with_audit` on
WASM short-circuits because the scheduler is native-only.

All walker bindings (`whyLoadedFromAuditJson`,
`whyInstantiatedFromAuditJson`) are synchronous — no `async fn`, no
`wasm_bindgen_futures::future_to_promise`. TS wrappers may wrap the
sync call in `Promise.resolve(...)` at the consumer layer.

## `u64` / `i64` transport

Every audit integer field larger than 32 bits — signed or unsigned —
transports as a decimal string. `RequestAuditRecord`'s `request_id`,
`bytes_read`, `duration_ns`, and `process_rss_delta_bytes` (i64)
round-trip through `JSON.parse` / `JSON.stringify` with zero
precision loss. Consumers that need arithmetic call `BigInt(s)`.

u32 and smaller remain JS `number`.

## Retrieval store

`AuditRecordsStore` is the host's bounded records store
(`AUDIT_RECORDS_STORE_CAPACITY = 256`). Insert order controls
eviction; oldest entries evict on overflow. Strict insert-then-take
— no access refresh.

`VerterHost::take_audit_record(request_id) -> Option<RequestAuditRecord>`
drains the entry. NAPI exposes the same surface via
`getLastAuditRecord` (drain most recent) and `getAuditRecords`
(non-destructive filtered query).

## Configuration — `AuditConfig`

`verter_audit::config::AuditConfig` is the host-owned audit-config
snapshot:

```rust
pub struct AuditConfig {
    pub consumer_filter: AuditConsumerFilter,
    pub audit_timing_capture: bool,
}
```

- `consumer_filter`: `AuditConsumerFilter` bitset — defaults to
  allow-every-kind. Use `deny(KindBit::…)` to strip a kind, or
  `allow_only(...)` to build a filter from scratch.
- `audit_timing_capture`: gates the timing surface. When `true`,
  `HostAuditRuntime` spawns the host-owned peak-RSS sampler thread
  on the first audit-enabled request and per-file timing helpers run
  their `Instant::now()` captures. Validation requires
  `audit_enabled = true` whenever this flag is enabled.

`HostConfig` exposes `audit_enabled` + `footprint_capture` as the
top-level toggles; both must be on for `getComponentMetaWithAudit`
to produce a populated record.
