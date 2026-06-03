> **SUPERSEDED.** This document is historical. The live authority is [`docs/arch/semantic-db-overhaul-unified-remaining-plan.md`](./semantic-db-overhaul-unified-remaining-plan.md) (and the native-typeinfo-parity doc-set). Sections below are retained for provenance; where they contradict the unified plan, the unified plan wins.

> **Status (2026-06-02):** Remaining work from this plan is now tracked in [`semantic-db-overhaul-unified-remaining-plan.md`](./semantic-db-overhaul-unified-remaining-plan.md), which merges + sequences the remaining items of this plan with the other track. Drive new work from the unified plan; this file remains as historical/detail reference.

# Cache runtime overhaul plan

This plan is the end-state spec for Verter's cache runtime. It defines
twelve implementation blocks that together land:

- a typed `cache_runtime` substrate (`ArtifactNode` / `QueryNode` /
  `CacheAdmission<V>`) replacing the legacy `cooperative_admission`
  module;
- one `WorldSnapshot` request-identity type that never enters a cache
  key;
- a typed `SignatureAdmission` gate that distinguishes empty from
  overflowed read-set signatures at the type level;
- a uniform enumeration of every artifact-node and query-identity-node
  cache layer on the new substrate;
- three public compile modes (`Stateless` / `Content` / `Session`) with
  observable downgrade reasons and a deterministic `stable_hash`
  conversion to `WorldSnapshot` env-hash inputs;
- `compile_many` as a transactional batch on a host-owned CPU pool;
- a scheduler integration that introduces typed `TaskKind` routing,
  per-call CPU-concurrency semaphores, dependency-DAG submission,
  driver-safe nonblocking pool submission, and a generic dedupe-hook
  trait — with `verter_scheduler` strictly free of any `verter_session`
  dependency (H20);
- removal of every bespoke per-call cache-invalidation list on
  `VerterHost`;
- a persistent pure-artifact cache gated by a sealed `BaseWriteToken`
  capability witness and a sealed `PersistentArtifactNode` supertrait;
- a memory-policy layer with weighted eviction, per-snapshot pin
  registry, and five typed `StructuredAuditEvent` variants;
- native flow-return on the same artifact-node substrate, split into
  two nodes (`FlowBodyHashNode` → `FlowLoweredBodyNode`) so the
  pre-lookup hash circularity breaks;
- a typed bench schema reporting cache mode, source-map policy, batch
  shape, thread count, hit count, fallback count, and non-admission
  count.

Every block lands code callable in production, exercised by `cargo test
--workspace --tests --verbose`, audited where relevant. No empty test
bodies. No `todo!()`. No dual-path shims. No `or equivalently` deferred
choices.

## Context

Today's cache implementation has the right core philosophy — lazy
validation, fact-granular signatures, content-addressed parse
artifacts, query-identity semantic caches, `ComputeAdmission::ReturnOnly`
for valid-but-not-cacheable results — but several concerns remain
coupled:

- Pure SFC compilation flows through host/session machinery only
  needed for workspace-aware semantic answers; `verter_napi::compile`
  silently picks the heaviest path with no public `stateless` /
  `content` / `session` distinction.
- `compile_many` (`crates/verter_session/src/host_compile.rs:117`)
  constructs a local Rayon pool per call (`:143-147`). The pool
  preserves the 8 MiB Windows stack
  (`compile_many_default_pool_has_8mib_stack`), but two back-to-back
  invocations rebuild it twice, and the scheduler's CPU executor has no
  way to share it.
- `cooperative_admission.rs` already encodes the three-way admission
  contract (`ComputeAdmission` at
  `crates/verter_session/src/cooperative_admission.rs:152`) but
  `finalise_signature_or_empty`
  (`crates/verter_session/src/compile_fact_emission.rs:469`) silently
  converts tracer overflow into `Arc::from(Vec::new())`, collapsing the
  empty / overflow distinction at the carrier type itself.
- `SemanticQueryKey::Instantiate { base: DeclIdentity { whole_hash, .. } }`
  (`crates/verter_session/src/semantic_query.rs:1143`) and
  `ResolveMacroPayload { owner: DeclIdentity { whole_hash, .. } }`
  (`:1282`) embed `whole_hash` directly in the query key, violating skill R6
  (query-identity keys never include content/version hashes).
- The scheduler exposes only a single-request `submit_request` and a
  loop-based `submit_batch`
  (`crates/verter_scheduler/src/scheduler.rs:273,312`); there is no
  dependency-DAG submission API and the scheduler shares CPU/I/O
  through a single rayon pool that can starve `compile_many`'s outer
  coordinator.
- Module augmentation runs against
  `AugmentationTargetKey { project_identity, resolve_env_hash,
  lib_env_hash, target }` (R29) but the population path is not
  type-gated against overlay views.
- Persistent caching is an architectural opportunity but the project
  must persist only pure content-addressed artifacts until semantic
  query admission is fully audited.
- Native function-body flow-return
  (`/tmp/verter-native-flow-return-coverage.md`) proposes a parallel
  `FileArtifactStore::flow_lowered_body_for` mechanism that must land
  ON this substrate, not parallel to it.

The end state is a deterministic incremental computation engine: every
reusable output is a node in an artifact or query graph, every
workspace-aware result records the facts it read, every warm hit is
validated against the live `StoreView` before return, the five env-hash
dimensions stay split, cache admission is typed at every producer,
singleflight is required for every cold cacheable node, persistent
storage is restricted to pure artifacts gated by a sealed capability
witness, and native flow-return targets the same artifact-node trait
surface.

## Hard cache rules (H1–H23)

The thirty-one architectural cache rules `R1–R31` live in
`.claude/skills/type-cache-architecture/SKILL.md` lines 110–810. That
skill is the owning surface. This overhaul does NOT modify the text of
any `R<n>` rule. The plan introduces its own per-block tightenings and
new rules under the `H1–H23` namespace, with an explicit `H ↔ R`
cross-reference table below.

**Numbering reservation (CRITICAL).** `R1–R31` are reserved for the
skill. The plan never authors text under an `R<n>` heading. Per-block
changes that touch the skill list the section update under their
`#### Owning-doc updates` subsection and continue to cite skill rules
by their canonical `R<n>` identifier.

**Tightened rules (plan-level discriminating guards over existing skill
rules):**

- `H3` (query-identity keys exclude `fact_dep_signature` AND content
  hashes AND version hashes) — corresponds to skill `R6`. Guard:
  `cache_key_runtime_guards::semantic_query_keys_contain_no_content_hash_or_fact_signature`
  (Block 4).
- `H5` (empty vs overflowed signatures are different cacheable states) —
  corresponds to skill `R20` + `R31`. Carrier discriminator is
  `ReadSetSignature.overflowed: bool`. Guard:
  `read_set_signature_carrier::empty_and_overflow_are_distinguishable_at_carrier_type`
  (Block 3).
- `H11` (overlay/base separation type-gated by the sealed
  `BaseWriteToken` capability witness) — corresponds to skill `R17`.
  Guard:
  `persistent_overlay_compile_error::cache_overlay_snapshot_cannot_construct_base_write_token`
  (Block 9).
- `H14` (singleflight required for every cold cacheable node) —
  corresponds to skill `R19` + `R26`. Guard:
  `cache_runtime_singleflight::cold_cacheable_node_computes_once_for_two_joiners`
  (Block 2).

**New plan-level rules introduced by this overhaul** (all registered in
`crates/verter_session/tests/critical_rules_have_guards.rs::every_critical_rule_in_docs_has_registered_guard`
in the same block that introduces them):

- `H15` typed `CacheAdmission<V>` gate (Block 2 — `Cacheable(V)` holds
  the unwrapped value; the substrate wraps in `Arc::new(...)` at
  admission).
- `H16` typed `SignatureAdmission` gate (Block 3).
- `H17` typed `BaseWriteToken` view gate for persistent writes (Block 9).
- `H18` sealed `PersistentArtifactNode` trait (Block 9).
- `H19` block-vocabulary ban (Block 1).
- `H20` `verter_scheduler` does NOT depend on `verter_session`
  (Block 7).
- `H21` single readiness authority — file stages AND cache nodes share
  one driver-owned `SchedulerDag`; the dispatch path performs NO linear
  job scan (Block 7, DECISION 1). Guards:
  `scheduler_has_single_readiness_authority`,
  `scheduler_dispatch_path_no_linear_job_scan`.
- `H22` backpressure is typed at DAG admission, never a submitter-side
  ready-queue push/spin; the driver is the only readiness mutator
  (Block 7, DECISION 2). Guards:
  `submit_dag_backpressure_is_typed_before_readiness_mutation`,
  `scheduler_submission_paths_do_not_call_yield_now`.
- `H23` driver dispatch never blocks on worker-pool submission. DAG
  admission reserves CPU/I/O capacity before readiness mutation; driver
  code may call only nonblocking pool submit APIs. Exhausted admitted
  capacity fails at admission; unexpected nonblocking submit failure
  returns the borrowed permit and parks the node in a driver-owned
  deferred lane. Guards:
  `driver_never_blocks_on_io_pool_send`,
  `driver_never_blocks_on_cpu_pool_submit`,
  `pool_capacity_reserved_before_ready_seed`,
  `dag_capacity_reservation_is_single_accounting_source`,
  `capacity_reservation_releases_exactly_once_on_completion_cancel_panic_shutdown`,
  `pool_permit_and_dag_budget_cannot_double_release`,
  `deferred_lane_is_bounded_by_admitted_capacity`,
  `deferred_lane_eventually_runs_under_sustained_cpu_saturation`,
  `critical_ready_work_not_starved_by_deferred_background_work`,
  `scheduler_model_random_dags_preserve_readiness_invariants`,
  `scheduler_model_capacity_returns_to_zero_at_quiescence`,
  `scheduler_model_driver_never_blocks_under_seeded_pool_failures`.

**Full `H ↔ R` cross-reference table** (preserved verbatim from the
prior plan — every reviewer concurred this mapping is sound; entries
without a skill correspondence are new):

| Plan H# | Skill R# | Semantic |
|---|---|---|
| H1  | R1   | `upsert` is a cache-state no-op on unchanged quintuple |
| H2  | R2   | `upsert` means the source changed |
| H3  | R6   | query-identity keys exclude `fact_dep_signature`, content hashes, version hashes |
| H4  | R3   | reverse-dependent invalidation is forbidden |
| H5  | R20 + R31 | empty signatures and overflowed signatures are different cacheable states |
| H6  | R8   | only final per-owner payloads admit query-identity entries |
| H7  | R9   | reuse is the default; recomputation is the exception |
| H8  | R10  | facts use stable `FactKey`s; per-key invalidation, not vector indices |
| H9  | R11  | binding-naming facts carry `SymbolSpace ∈ {Type, Value, Namespace}` |
| H10 | R12  | parse-domain vs resolve-domain fact separation |
| H11 | R17  | overlay/base separation; type-gated by the sealed `BaseWriteToken` witness |
| H12 | R18  | `SessionView` passed explicitly through `ResolverContext` |
| H13 | R20  | multi-candidate storage isolates concurrent overlay variants |
| H14 | R19 + R26 | singleflight required for every cold cacheable node, validated against the `ValidatedFactCache` substrate |
| H15 | (new) | typed `CacheAdmission<V>` gate at every producer; substrate wraps in `Arc::new(...)` at admission |
| H16 | (new) | typed `SignatureAdmission` gate; overflow routes to `NonCacheable` |
| H17 | (new) | typed `BaseWriteToken` view gate for persistent writes |
| H18 | (new) | sealed `PersistentArtifactNode` trait (query nodes cannot persist) |
| H19 | (new) | source comments must not contain plan vocabulary (`block <n>`, `cache-runtime overhaul`, `runtime cutover`) |
| H20 | (new) | `verter_scheduler` crate must not depend on `verter_session`; cache-runtime dedupe runs in `verter_session` via the generic `DedupeHook` trait |
| H21 | (new) | single driver-owned `SchedulerDag` readiness authority for all work; dispatch reads four priority lanes, never a linear job scan; `JobIndex` / `BlockerRegistry` deleted |
| H22 | (new) | backpressure is typed at DAG admission (`SubmissionResult<T>` + `DagAdmissionBudget` + condvar); no submitter-side ready-queue push, spin, or `yield_now`; driver is sole readiness mutator |
| H23 | (new) | worker-pool submission is nonblocking on the driver path; CPU/I/O capacity is reserved by one `DagCapacityReservation`, released exactly once, and deferred lanes are bounded/fair |

## Inter-block dependency DAG

```
   B1 (WorldSnapshot + plan-vocabulary guard)
   │
   ▼
   B2 (cache_runtime/ rename + ArtifactNode / QueryNode / CacheAdmission<V>)
   │
   ├──────────────► B7 (scheduler/cache-runtime integration; B2 only)
   │                 │
   ├──────────────► B10 (memory policy + audit; B2 only)
   │
   ▼
   B3 (SignatureAdmission, CompileSlot retyping — consumes B2's CacheAdmission)
   │
   ▼
   B4 (artifact + query-identity nodes; B2 + B3)
   │
   ├──────────────► B5 (CompileCacheMode; B1 + B4)
   │                 │
   │                 ▼
   ├──────────────► B6 (compile_many transaction; B2 + B4 + B5 + B7 + B12-types)
   ├──────────────► B8 (host-cache rehoming; B4)
   │
   ▼
   B9 (persistent pure artifacts; B2 + B4)
   │
   ▼
   B11 (flow-return on runtime; B2 + B4 + B9)
   │
   └──► B12 (benchmarks + regression gates; depends on every code-producing block)
```

**B6 forward-reference notes.** B6's `compile_many` body references
`CpuConcurrencySemaphore` and `CacheNodeDagNode` (introduced by B7
at `crates/verter_scheduler/src/{cpu_concurrency,node}.rs`) and
`MAX_TEST_TIMEOUT` (introduced by B12 at
`crates/verter_session/src/test_support/timeout.rs`). The DAG above
records both edges (B6 → B7, B6 → B12-types) so the implementer
lands B7's scheduler-side types and B12's `MAX_TEST_TIMEOUT`
constant BEFORE B6's `compile_many_no_pool_deadlock` /
`cpu_concurrency_semaphore_caps_concurrent_cpu_tasks` tests compile
(the latter was originally drafted as `threads_option_honored` when
`compile_many` carried a per-call `threads` option; the option has
since been removed and the semaphore capacity is sourced from the
scheduler config rather than the batch options). The
`MAX_TEST_TIMEOUT` landing is a one-line constant + module
declaration, intentionally small so the B12-types edge into B6 is
cheap; the full B12 benchmark surface still depends on the rest of
B6.

Prose explanation (verbatim — no cycles):

- **B1** has no in-plan prerequisites.
- **B2** is a pure rename of `cooperative_admission` into
  `cache_runtime/`, plus the `ArtifactNode` / `QueryNode` trait
  definitions and the `CacheAdmission<V>` re-export. **B2 introduces NO
  `SignatureAdmission` reference** — that type is owned by B3.
- **B3** consumes B2's renamed `CacheAdmission`. Overflow paths route
  `SignatureAdmission::NonCacheable` into `CacheAdmission::ReturnOnly`
  at the producer call site, not inside B2's trait.
- **B4** depends on B2 (`ArtifactNode` / `QueryNode`) and B3
  (`SignatureAdmission` / `ReadSetSignature`).
- **B5** depends on B1 (`WorldSnapshot`) and B4
  (`CompileOutputNode_*`).
- **B6** depends on B2 + B4 + B5 + B7 (consumes B7's
  `CpuConcurrencySemaphore` and `CacheNodeDagNode` types in
  `compile_many`'s DAG submission path) + B12-types (consumes
  `MAX_TEST_TIMEOUT` in `compile_many_no_pool_deadlock`). B12's
  full benchmark surface still depends on the rest of B6 — only
  the constant lands early.
- **B7** depends on B2 only. The scheduler crate itself does **NOT**
  depend on `verter_session` (H20). Cache-runtime dedupe runs in
  `verter_session` BEFORE submission via the generic `DedupeHook` trait
  defined inside `verter_scheduler`.
- **B8** depends on B4.
- **B9** depends on B2 + B4.
- **B10** depends on B2 only.
- **B11** depends on B2 + B4 + B9; first task inside B11 is moving
  `/tmp/verter-native-flow-return-coverage.md` to
  `docs/arch/native-flow-return.md`.
- **B12** depends on every code-producing block.

Parallel execution is permitted within an unblocked layer. B2 and B3
must land sequentially.

## Calling conventions (used throughout this plan)

These conventions appear in every block. They are defined here once so
each block snippet compiles against the same substrate:

- **`ResolverContext`** — `pub(crate) trait ResolverContext: sealed::Sealed`
  defined at `crates/verter_session/src/resolver_core/resolver_context.rs:152`.
  No lifetime parameter. Every plan snippet that consumes it writes
  `ctx: &dyn ResolverContext`. This is the substrate's exact shape; the
  rewrite never introduces `&ResolverContext<'_>` or any other form.
- **`ReadSetSignature::validate_with_self_roots`** — `pub(crate) fn`
  defined at `crates/verter_session/src/fact_signature_helpers.rs:732`,
  signature
  `fn validate_with_self_roots(&self, ctx: &dyn ResolverContext, self_root_canonicals: &[Arc<str>]) -> bool`.
  Block 4's post-compute publish gate calls this method against the
  caller's `&dyn ResolverContext` — NOT against any
  `WorldSnapshot::current_whole_hash`-style accessor (none exists).
- **`Hash16`** — `pub type Hash16 = [u8; 16]` at
  `crates/verter_session/src/types.rs:12`. Plain `[u8; 16]`; not a
  newtype. `stable_hash()` returns the bare array.
- **`StoreViewCompatToken`** — existing per-process compat token type
  on `ResolverContext` used by the singleflight substrate at
  `crates/verter_session/src/resolver_core/mod.rs:1129`.
- **`OverlayIdentity`** — newtype wrapper around the existing overlay
  session id, introduced by Block 1.
- **`OpaqueRequestContext`** — `pub struct OpaqueRequestContext(pub Arc<dyn RequestContextLike>)`
  at `crates/verter_scheduler/src/request_context.rs:103`. Scheduler-local;
  carries no `verter_session` path.
- **`cache_runtime::lookup`** — single canonical entry point:
  ```rust
  pub(crate) fn lookup<N: ArtifactNode>(
      node: &N,
      key: &N::Key,
      snapshot: &WorldSnapshot,
      ctx: &dyn ResolverContext,
  ) -> Result<Arc<N::Value>, LookupError>;
  ```
  Used by every caller in this plan. Visibility is `pub(crate)`
  because the function signature names `&dyn ResolverContext`, and
  substrate `ResolverContext` is `pub(crate)` (so a `pub fn` would
  trip `clippy::private_interfaces`). Every caller lives inside
  `verter_session`. The substrate stores `Arc<Value>` keyed by
  `Key`; the node impl is consumed by short-lived reference and
  never retained. `QueryNode` has a parallel
  `cache_runtime::query::lookup<N: QueryNode>(...)` (same
  `pub(crate)` visibility, same rationale) with multi-candidate
  storage.

## Type and identifier inventory

Every type the plan introduces is declared in exactly one block and is
defined before any other block refers to it. The table below names the
owning block and the per-file path; cross-block references rely on
this table.

| Type / item | Owning block | Owning file |
|---|---|---|
| `WorldSnapshot`, `WorldSnapshot::*_dims`, `WorldSnapshot::base_write_token`, `WorldSnapshotDims`, `OverlayIdentity` | B1 | `crates/verter_session/src/cache_runtime/world_snapshot.rs` |
| `ArtifactNode`, `QueryNode`, `ComputeCtx`, `LookupError`, `CacheAdmission<V>` | B2 | `crates/verter_session/src/cache_runtime/{artifact,query,admission}.rs` |
| `CancellationToken` (scheduler-local) | B7 | `crates/verter_scheduler/src/cancellation.rs` |
| `Candidate<V>`, `QuerySlot<V>`, `PublishOutcome` | B2 | `crates/verter_session/src/cache_runtime/query.rs` |
| `NonAdmissionReason` | B10 (leaf-substrate) | `crates/verter_audit/src/structured_event.rs` (re-exported into `verter_session::cache_runtime::query` for publish-pipeline use; `verter_audit` never depends on `verter_session`) |
| `SignatureAdmission`, `CacheEntry<V>`, `ReadSetSignature` retypings | B3 | `crates/verter_session/src/cache_runtime/{admission,store}.rs` + `fact_signature_helpers.rs` |
| `FileArtifactKey`, `ResolvedImportFactsKey`, `CompileOutputKey`, `CompileOutputSlotKey`, `AugmentationTargetKey`, `AnalysisSlotKey`, `AnalysisCandidate`, `ResolvedDeclSlotIdentity` | B4 | per-row in `Block 4` table |
| `CompileCacheMode`, `SourceMapPolicy`, `DowngradeReason`, `CompileResult`, `CompileRequest`, `CompileBatchOptions`, `CompileBatchInput`, `CompileBatchEntry` | B5 | `crates/verter_session/src/host_compile_types.rs` |
| `HostCpuPool` | B6 / B7 | `crates/verter_scheduler/src/host_cpu_pool.rs` |
| `SchedulerCpuPool`, `SchedulerIoPool`, `PoolSubmitError`, `CpuConcurrencySemaphore`, `CpuConcurrencyPermit` | B7 | `crates/verter_scheduler/src/{pool,cpu_concurrency}.rs` |
| `TaskKind` variants (new shape), `Priority`, `TargetStage` (Hash bumped) | B7 | `crates/verter_scheduler/src/stage.rs` |
| `WorkKind`, `WorkNodeKey`, `WorkKindKey`, `SchedulerDag`, `SchedulerDagNode`, `SchedulerDagEdge`, `NodeId` (unified readiness substrate — §7.0; `CacheNodeDag*` are the cache-node specialization) | B7 | `crates/verter_scheduler/src/node.rs` |
| `KeyedJob`, `DedupKey`, `DagHandle`, `DagState`, `CacheNodeId` | B7 | `crates/verter_scheduler/src/job.rs` |
| `MAX_READY_QUEUE_DEPTH`, `SubmissionResult<T>` (generic), `DagAdmissionBudget` | B7 | `crates/verter_scheduler/src/queue.rs` |
| `CacheNodeDag`, `CacheNodeDagNode`, `CacheNodeDagEdge`, `EdgeGate`, `CacheNodeDispatchCtx`, `CacheNodeOutcome`, `CacheNodeValue`, `AdmissionDisposition`, `DagCompletionAggregator`, `CacheNodeCompletionSender` | B7 | `crates/verter_scheduler/src/node.rs` |
| `SchedulerCacheId` | B7 | `crates/verter_scheduler/src/cache_id.rs` |
| `DedupeHook`, `DedupeJoiner` | B7 | `crates/verter_scheduler/src/dedupe_hook.rs` |
| `CacheNodeMetrics`, `MemoryPolicy`, `ActiveSnapshotPinRegistry`, `SnapshotId`, `CacheEntryId`, `EvictionRingBuffer`, `AdmissionDecision`, `ColdMissReason`, `StaleReason` | B10 | `crates/verter_session/src/cache_runtime/{metrics,memory_policy}.rs` (single weight shape: `ArtifactNode::weight_bytes` + `QueryNode::weight_bytes` from B2; no separate `WeightedAccountable` trait) |
| `StructuredAuditEvent::CacheNode*` variants | B10 | `crates/verter_audit/src/structured_event.rs` |
| `PersistentArtifactNode`, `BaseWriteToken`, `BaseToken`, `PersistentCache`, `ManifestHeader`, `PERSISTENT_SCHEMA_VERSION` | B9 | `crates/verter_session/src/cache_runtime/persistent/{mod,cas,manifest}.rs` |
| `FlowBodyHashNode`, `FlowBodyHashKey`, `FlowBodyHashOutcome`, `FlowLoweredBodyNode`, `FlowLoweredBodyKey`, `FlowLoweredBody`, `SymbolId`, `ParseStableHash`, `ParserVersion` | B11 | `crates/verter_session/src/cache_runtime/{flow_body_hash,flow_lowered_body}.rs` |
| `BenchResultRow`, `MAX_TEST_TIMEOUT` | B12 | `packages/benchmark/src/cache-runtime-bench.ts`, `crates/verter_session/src/test_support/timeout.rs` |

`SchedulerJobKind` (substrate at `crates/verter_scheduler/src/stage.rs:19`)
is **retained** post-cutover. It carries `ComponentMeta { canonical_id }`
for the existing component-meta batch fan-out path; the new
`TaskKind::CacheNode { cache_id, key_hash }` lives alongside it on the
ready-queue element type — which is `Arc<CacheNodeDagNode>` (the
bare node is not `Clone` — see the `CacheNodeCompletionSender`
doc-comment), NOT a separate `ReadySubmission` wrapper (the legacy
`ReadySubmission` shape is retired) — and is dispatched through the same
`Scheduler::scheduler_cpu_pool` infrastructure. The two enums never
alias: `SchedulerJobKind` discriminates non-staged jobs at the
component-meta batch entry point; `TaskKind` discriminates staged
file-progression work AND cache-node work at the per-task dispatch
site.

**Substrate types DELETED by B7** (DECISION 1 — the staged-ordering /
blocker substrate is replaced by the single driver-owned
`SchedulerDag`; none of these may survive on a non-test scheduler
path): `JobIndex`, `QueueEntry`, `EffectiveKey`, `AgingConfig` (and the
`SchedulerConfig.aging` field), `BlockerRegistry`, `BlockerRef`,
`UnblockedJob`, `has_pending_blockers`, `Submission::BlockerResolved`,
the `Scheduler.job_index` / `Scheduler.deferred_blocker_ids` fields, and
file-stage ordering through `FileNode.pending_requests`. The
non-generic `SubmissionResult` is replaced by the generic
`SubmissionResult<T>`.

## Changes

The twelve implementation blocks below land in dependency order. Each
block carries its own `#### Context`, `#### Changes`, `#### Legacy
Deletions`, `#### Verification`, `#### Discriminating tests`, `####
Owning-doc updates`, `#### Public API mirrors`, and `#### Blocks blocked
by this block`.

### 1. WorldSnapshot + plan-vocabulary guard

#### Context

The cache runtime needs one explicit type that captures the
deterministic identity of a request: project identity, the five
env-hash dimensions (R21), compiler version, plugin versions,
source-map policy, public-API mode, overlay identity, and the
in-process compat token used by
`crates/verter_session/src/resolver_core/mod.rs:1129`'s
`FxHashMap<(K, StoreViewCompatToken), Arc<FlightState<V, E>>>`
singleflight substrate.

Today this identity is implicit, threaded through `ResolverContext`,
`IdeProjectConfig::*_env_hash` accessors, `StoreViewCompatToken`, and
ad-hoc tuples. Block 1 lifts it into one `WorldSnapshot` struct that
later blocks consume.

Block 1 also extends the existing
`no_phase_archaeology_in_production_code` guard
(`crates/verter_session/tests/architecture_guards.rs:4421`) to ban
plan vocabulary specific to this overhaul: `block N`,
`cache-runtime overhaul`, `runtime cutover` (H19). Source must read as
final-state once landed.

#### Changes

Create `crates/verter_session/src/cache_runtime/world_snapshot.rs`:

```rust
use crate::types::Hash16;

/// Request-concurrency identity for the cache runtime.
///
/// `WorldSnapshot` is the single deterministic identity for ONE
/// in-flight request. It is what `cooperative_get_or_insert` lanes
/// coalesce on; it is NOT a cache key (R21 forbids bundling the five
/// env hashes into a single `project_config_hash` on any cache layer
/// — per-layer keys continue to embed only the dimensions they
/// actually depend on).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorldSnapshot {
    pub compat_token: StoreViewCompatToken,
    pub project_identity: Hash16,
    pub parse_env_hash: Hash16,
    pub resolve_env_hash: Hash16,
    pub type_env_hash: Hash16,
    pub lib_env_hash: Hash16,
    pub source_map_policy_hash: Hash16,
    pub public_api_mode_hash: Hash16,
    pub compiler_version: Hash16,
    pub plugin_versions: Hash16,
    pub overlay_identity: Option<OverlayIdentity>,
    /// World generation under which the snapshot was constructed.
    /// Block 4's query-identity caches stamp this on
    /// `CacheEntry.validated_at_generation` at admission; Block 10's
    /// memory policy reads it to decide pin lifetime.
    pub generation: u64,
}

/// Newtype wrapper around the existing overlay session id. Empty when
/// the snapshot represents a base view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OverlayIdentity(pub u64);

/// Pre-computed env-dimension bundle the caller assembles at the
/// request entry boundary.
///
/// Carrying the dims as a struct keeps `WorldSnapshot::from_request`
/// substrate-friendly: the four env-hash accessors on
/// `verter_workspace::resolver::IdeProjectConfig` take an
/// `&EnvHashInputs<'_>` argument (defined at
/// `crates/verter_workspace/src/env_hash.rs:107-203`); the caller
/// computes the four `Hash16`s once and packs them here. The trio
/// `compiler_version` / `plugin_versions` / `world_generation` are
/// NOT on `IdeProjectConfig` — they are host-side identity dimensions
/// the caller already tracks (e.g. the host's installed compiler
/// version, the host's plugin registry hash, the host's monotonic
/// world generation counter).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorldSnapshotDims {
    pub project_identity: Hash16,
    pub parse_env_hash: Hash16,
    pub resolve_env_hash: Hash16,
    pub type_env_hash: Hash16,
    pub lib_env_hash: Hash16,
    pub compiler_version: Hash16,
    pub plugin_versions: Hash16,
    pub world_generation: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct ParseEnvDims { pub project_identity: Hash16, pub parse_env_hash: Hash16 }
#[derive(Debug, Clone, Copy)]
pub struct ResolveEnvDims { pub project_identity: Hash16, pub parse_env_hash: Hash16, pub resolve_env_hash: Hash16 }
#[derive(Debug, Clone, Copy)]
pub struct TypeEnvDims { pub project_identity: Hash16, pub parse_env_hash: Hash16, pub resolve_env_hash: Hash16, pub type_env_hash: Hash16, pub lib_env_hash: Hash16 }
#[derive(Debug, Clone, Copy)]
pub struct CompileEnvDims {
    pub project_identity: Hash16,
    pub parse_env_hash: Hash16,
    pub resolve_env_hash: Hash16,
    pub type_env_hash: Hash16,
    pub lib_env_hash: Hash16,
    pub source_map_policy_hash: Hash16,
    pub public_api_mode_hash: Hash16,
    pub compiler_version: Hash16,
    pub plugin_versions: Hash16,
}

impl WorldSnapshot {
    /// Build a `WorldSnapshot` from a `ResolverContext` plus the
    /// pre-computed env-dimension bundle the request carries.
    ///
    /// `dims: WorldSnapshotDims` is the substrate-friendly carrier
    /// for the four env hashes + project identity + compiler /
    /// plugin / world-generation identity. The host populates it at
    /// the request entry boundary by calling the existing
    /// `IdeProjectConfig::parse_env_hash(&EnvHashInputs)`,
    /// `::resolve_env_hash(...)`, `::type_env_hash(...)`,
    /// `::lib_env_hash(...)`, `::project_identity()` accessors and
    /// mixing in the host-side `compiler_version` / `plugin_versions`
    /// / `world_generation` it already tracks (the four env
    /// accessors live in
    /// `crates/verter_workspace/src/env_hash.rs:107..217`;
    /// `project_identity` returns a `Hash16` at `:217`; the trio of
    /// `compiler_version` / `plugin_versions` / `world_generation`
    /// are host-side and DO NOT live on `IdeProjectConfig`).
    ///
    /// `compat_token` is read through `StoreView` (`ctx.store_view()
    /// .compat_token()`) — `StoreView::compat_token` is the
    /// authoritative accessor at
    /// `crates/verter_session/src/resolver_core/mod.rs:148-149`.
    ///
    /// `overlay_identity` is read off the active session id the
    /// caller already has at the request entry boundary (e.g.
    /// `SessionResolverContext::session_id()`). The bare-host
    /// `ResolverContext` rail constructs a `WorldSnapshot` with
    /// `overlay_identity = None`; the session rail passes
    /// `Some(OverlayIdentity(session_id))`. A standalone
    /// `ResolverContext::overlay_identity()` accessor is NOT
    /// introduced — the snapshot is the canonical place for that
    /// identity and the caller injects it.
    pub fn from_request(
        ctx: &dyn ResolverContext,
        dims: WorldSnapshotDims,
        overlay_identity: Option<OverlayIdentity>,
        public_api_mode_hash: Hash16,
        source_map_policy_hash: Hash16,
    ) -> Self {
        Self {
            compat_token: ctx.store_view().compat_token(),
            project_identity: dims.project_identity,
            parse_env_hash: dims.parse_env_hash,
            resolve_env_hash: dims.resolve_env_hash,
            type_env_hash: dims.type_env_hash,
            lib_env_hash: dims.lib_env_hash,
            source_map_policy_hash,
            public_api_mode_hash,
            compiler_version: dims.compiler_version,
            plugin_versions: dims.plugin_versions,
            overlay_identity,
            generation: dims.world_generation,
        }
    }

    pub fn parse_dims(&self) -> ParseEnvDims { ParseEnvDims { project_identity: self.project_identity, parse_env_hash: self.parse_env_hash } }
    pub fn resolve_dims(&self) -> ResolveEnvDims { ResolveEnvDims { project_identity: self.project_identity, parse_env_hash: self.parse_env_hash, resolve_env_hash: self.resolve_env_hash } }
    pub fn type_dims(&self) -> TypeEnvDims { TypeEnvDims { project_identity: self.project_identity, parse_env_hash: self.parse_env_hash, resolve_env_hash: self.resolve_env_hash, type_env_hash: self.type_env_hash, lib_env_hash: self.lib_env_hash } }
    pub fn compile_dims(&self) -> CompileEnvDims { CompileEnvDims { project_identity: self.project_identity, parse_env_hash: self.parse_env_hash, resolve_env_hash: self.resolve_env_hash, type_env_hash: self.type_env_hash, lib_env_hash: self.lib_env_hash, source_map_policy_hash: self.source_map_policy_hash, public_api_mode_hash: self.public_api_mode_hash, compiler_version: self.compiler_version, plugin_versions: self.plugin_versions } }
}
```

The `source_map_policy_hash` and `public_api_mode_hash` fields use
the existing `Hash16 = [u8; 16]` alias
(`crates/verter_session/src/types.rs:12`). Block 5 introduces the
typed enums `SourceMapPolicy` and `CompileCacheMode` plus their
`stable_hash() -> Hash16` conversions; Block 5 does NOT change the
`WorldSnapshot` struct shape. `WorldSnapshot::base_write_token` is
added by Block 9 (it requires the `BaseToken` type which Block 9
introduces); Block 1 only lands the field plumbing.

Extend `crates/verter_session/tests/architecture_guards.rs`'s
`no_phase_archaeology_in_production_code` guard predicate with three
new banned patterns:
- `\bblock \d+\b` (word-boundary; "the request loop blocks once per
  flight" does NOT match);
- `cache-runtime overhaul`;
- `runtime cutover`.

Patterns apply to `crates/*/src/**` only.
`guard7_predicate_rejects_deliberate_violations` gains three
fabricated lines proving the new patterns are caught.

#### Legacy Deletions

None inside this block.

#### Verification

```
cargo test --package verter_session world_snapshot --tests --verbose
cargo test --package verter_session architecture_guards::guard7 --tests --verbose
cargo test --workspace --tests --verbose
cargo clippy --workspace -- -D warnings
cargo fmt --all
pnpm install --frozen-lockfile
```

#### Discriminating tests

- `crates/verter_session/tests/world_snapshot_is_not_a_cache_key.rs::no_cache_layer_keys_on_world_snapshot_as_a_whole`
  — walks `crates/verter_session/src/**` for struct definitions whose
  name ends `Key` / `Identity`; asserts no field of type
  `WorldSnapshot` regardless of name. Field names are not load-bearing:
  the scanner strips common wrapper constructors (`Arc<>`, `Option<>`,
  `Box<>`, references, raw pointers), inspects every named field and
  every tuple-struct positional element, and rejects any field whose
  type mentions `WorldSnapshot` at a word boundary. Positive: every
  R21-scoped key embeds its scoped dimensions only. Negative: synthetic
  regressions covering arbitrary field names, `Arc`-wrapped fields, and
  tuple-position fields are all detected via fixture strings.
- `crates/verter_session/src/cache_runtime/world_snapshot.rs::tests::world_snapshot_from_request_matches_all_request_identity_dimensions`
  — inline `#[cfg(test)] mod tests` inside the owning module (no
  test-only constructor on the production type, no `for_tests`
  re-export — `WorldSnapshot` stays truly `pub(crate)`). Builds two
  requests with identical env dims but different `overlay_identity`;
  asserts the resulting `WorldSnapshot`s differ; builds two
  identical-input snapshots and asserts they hash equal. The same
  module hosts `world_snapshot_diverges_on_every_identity_dimension`
  and a `from_request`-rail test that drives the production
  `WorldSnapshot::from_request` constructor through the bare-host
  `impl ResolverContext for VerterHost` rail.
- `crates/verter_session/tests/architecture_guards.rs::guard7_predicate_rejects_block_vocabulary`
  — fabricated lines `// block 5: rehome the compile cache`,
  `// cache-runtime overhaul wiring`, `// runtime cutover landing step`
  all trip the guard; a benign comment mentioning `// the request loop
  blocks once per flight` does NOT trip (whole-word match on `block N`).
- `crates/verter_session/tests/plan_h_to_r_mapping.rs::h_to_r_mapping_is_semantically_accurate`
  — walks the H↔R table in this plan, extracts each `(H<n>, R<m>)`
  pair, reads the skill text for `R<m>`'s rule definition, and asserts
  the rule text contains the keyword set the H entry declares. Pinned
  keyword fixture inside the test (e.g. `H5` requires
  `{overflow, NonCacheable, BudgetExceeded}`; a synthetic remapping of
  `H5` to skill `R5 + R28` fails because skill `R5`/`R28`'s keywords differ). Updating
  the fixture requires touching the test alongside the mapping.
- `crates/verter_session/tests/plan_rule_namespace.rs::plan_rule_namespace_uses_h_not_r`
  — walks the plan markdown, enumerates every `\bR(20|14|17|26|19|6|5|11|28)\b`
  occurrence, and checks each against an allowlist of skill-reference
  contexts (the H↔R table, `corresponds to skill R<n>` prose, and
  `Owning-doc updates` skill paragraphs). Any plan-level rule
  reference using `R<n>` outside the allowlist fails.

#### Owning-doc updates

- `.claude/skills/type-cache-architecture/SKILL.md` — append a
  `WorldSnapshot` section under "Cache substrate" naming the type, its
  field list, and the architecture guard. Append a new
  `## Block-vocabulary ban (CRITICAL)` section stating verbatim:
  source comments under `crates/*/src/**` must not contain plan
  vocabulary specific to this overhaul (`\bblock \d+\b`,
  `cache-runtime overhaul`, `runtime cutover`); the guard is
  `architecture_guards::guard7_predicate_rejects_block_vocabulary`
  inside `no_phase_archaeology_in_production_code`. New
  `CRITICAL_RULE_GUARDS` entry:
  `("Block-vocabulary ban", &["guard7_predicate_rejects_block_vocabulary"])`.
- `docs/arch/fact-based-cache.md` — append a line in the env-hash
  audit table noting that `WorldSnapshot` exposes per-layer dim
  accessors and never enters a key as a whole.

#### Public API mirrors

`WorldSnapshot` is `pub(crate)` from `cache_runtime/mod.rs`. Not
exposed on any binding surface.

- Rust crate (`verter_session`): `pub(crate)` re-export.
- NAPI / WASM / protocol DTO / TS generated types / JS wrappers /
  compat: not exposed.

#### Blocks blocked by this block

- B2 (consumes `WorldSnapshot::compat_token` and `*_dims()`).
- B5 (`WorldSnapshot.public_api_mode_hash` and
  `source_map_policy_hash` populated by Block 5 enums).
- B9 (`WorldSnapshot::base_write_token` added by Block 9).
- B11 (flow-return dispatches through `WorldSnapshot::type_dims()`).

### 2. Cache-runtime substrate (`cache_runtime/`)

#### Context

`cooperative_admission.rs` already encodes the three-way typed
admission contract at
`crates/verter_session/src/cooperative_admission.rs:152`
(`ComputeAdmission<V, Entry>`). Skill rule `R26` says `ValidatedFactCache<K, V>`
at `crates/verter_session/src/resolver_core/mod.rs:576` is the
substrate. The new cache runtime is a typed wrapper layer on top of
the existing primitives, not a parallel substrate.

Block 2a lands one thing: rehome `cooperative_admission.rs` to
`cache_runtime/singleflight.rs` preserving the live API and
semantics. The new trait surface + lookup/publish runtime land in the
merged cutover block (B2b+B4) with their first real consumers.

Concretely, Block 2a:

1. moves `cooperative_admission.rs` → `cache_runtime/singleflight.rs`
   (and the sibling test file `cooperative_admission_tests.rs` →
   `cache_runtime/singleflight_tests.rs`), preserving the
   `ComputeAdmission<V, Entry>` enum (`Cacheable(Entry)` /
   `ReturnOnly(V)` / `Failed`), all three `cooperative_*` entry
   points, and the `project: FnOnce(&Entry) -> V` closure model
   verbatim;
2. updates every `crate::cooperative_admission::*` import to
   `crate::cache_runtime::singleflight::*`;
3. removes the `pub mod cooperative_admission;` declaration from
   `lib.rs` and adds `pub(crate) mod singleflight;` under
   `cache_runtime`.

The cache-runtime trait surface and lookup/publish runtime are NOT
part of this block; they land in the merged cutover (§4) with their
first real consumers. This block touches nothing beyond the rename.

#### Changes

This block is a pure rehome. It does not create the cache-runtime
trait surface or any new module beyond the rename; the trait surface
and lookup/publish runtime land in the merged cutover (§4) with their
first real consumers.

The two changes under `crates/verter_session/src/cache_runtime/`:

- add `pub(crate) mod singleflight;` to `cache_runtime/mod.rs`
  (alongside the already-landed `world_snapshot`);
- `singleflight.rs` — RENAMED FROM `cooperative_admission.rs`,
  preserving the live API and semantics verbatim.

The verbatim API that moves over from
`cooperative_admission.rs` (verified against the current tree — every
callable accounted for):

```
cooperative_admission::ComputeAdmission<V, Entry>    (:152)
    → cache_runtime::singleflight::ComputeAdmission<V, Entry>
cooperative_admission::cooperative_get_or_insert     (:542)
    → cache_runtime::singleflight::cooperative_get_or_insert
cooperative_admission::cooperative_get_or_insert_with_post_publish (:659)
    → cache_runtime::singleflight::cooperative_get_or_insert_with_post_publish
cooperative_admission::cooperative_admit_with_post_publish (:896)
    → cache_runtime::singleflight::cooperative_admit_with_post_publish
cooperative_admission::InflightTable<K>              (:204)
    → cache_runtime::singleflight::InflightTable
cooperative_admission::InflightSlot                  (:167)
    → cache_runtime::singleflight::InflightSlot
cooperative_admission::InflightSlotState             (:173)
    → cache_runtime::singleflight::InflightSlotState
cooperative_admission::InflightPanicGuard            (:255)
    → cache_runtime::singleflight::InflightPanicGuard
cooperative_admission::RemovalCleanupPreHookGuard    (:372)
    → cache_runtime::singleflight::RemovalCleanupPreHookGuard
cooperative_admission::PostRevalidatePrePublishHookGuard (:417)
    → cache_runtime::singleflight::PostRevalidatePrePublishHookGuard
cooperative_admission::install_removal_cleanup_pre_hook (:365)
    → cache_runtime::singleflight::install_removal_cleanup_pre_hook
cooperative_admission::install_post_revalidate_pre_publish_hook (:407)
    → cache_runtime::singleflight::install_post_revalidate_pre_publish_hook
cooperative_admission::retire_slot_if_current        (:325)
    → cache_runtime::singleflight::retire_slot_if_current
cooperative_admission::remove_published_entry_with_cleanup (:459)
    → cache_runtime::singleflight::remove_published_entry_with_cleanup
```

No symbols are renamed and no symbols are dropped. The
`ComputeAdmission<V, Entry>` enum keeps its three variants AND both
type parameters verbatim — the stored carrier (`Entry`) and the
projected value (`V`) are semantically distinct (the carrier holds
the dep-signature / self-root / generation metadata that a joiner
view-validates against; the value is what the caller receives), so
they must not be collapsed.

The bare `cooperative_get_or_insert` entry point currently has no
in-crate caller (every cache routes through
`cooperative_get_or_insert_with_post_publish`); it is retained as the
minimal admission shape and annotated `#[allow(dead_code)]` rather
than dropped, so the primitive keeps its complete API surface.

The same move relocates `cooperative_admission_tests.rs` to
`cache_runtime/singleflight_tests.rs` (it is a `#[cfg(test)] #[path]`
child `mod` of `singleflight`, so the move is a rename plus the
parent's `#[path]` update — there is no `lib.rs` test-mod
declaration). After the move, no `cooperative_admission*` file
remains in the tree; the structural guard
`tests/cache_runtime_singleflight_rehome.rs` asserts both the absence
of the old paths and the presence of the new ones.

Repoint the rehome-sensitive guard
`tests/block_1_i_discriminators.rs::cooperative_return_only_not_shared_to_joiners`
to read the canonical `cache_runtime/singleflight.rs` path.


#### Legacy Deletions

- REMOVE `pub mod cooperative_admission;` (and the
  `cooperative_admission_tests` mod declaration) from `lib.rs`.
- DELETE `crates/verter_session/src/cooperative_admission.rs` after
  contents move to `cache_runtime/singleflight.rs`. No forwarder.
- MIGRATE
  `crates/verter_session/src/cooperative_admission_tests.rs` →
  `crates/verter_session/src/cache_runtime/singleflight_tests.rs`.
  Rename only; preserve every test function name, every fixture,
  every helper.
- UPDATE every `use crate::cooperative_admission::*` import across
  the call sites enumerated in the rename table to
  `use crate::cache_runtime::singleflight::*` (`ComputeAdmission`
  and the `cooperative_*` entry points resolve through this path —
  `CacheAdmission` is NOT a symbol this rehome lands).

#### Verification

```
cargo test --package verter_session cache_runtime --tests --verbose
cargo test --package verter_session cooperative --tests --verbose   # asserts the legacy name resolves nowhere
cargo test --workspace --tests --verbose
cargo clippy --workspace -- -D warnings
cargo fmt --all
pnpm install --frozen-lockfile
```

#### Discriminating tests

The rehome is structural; the discriminators pin the move (not new
behaviour — the primitive's behavioural tests move verbatim with it).

- `crates/verter_session/tests/cache_runtime_singleflight_rehome.rs::singleflight_primitive_lives_under_cache_runtime`
  — reads the `src/` directory listing; asserts
  `cache_runtime/singleflight.rs` and
  `cache_runtime/singleflight_tests.rs` exist AND the crate-root
  `cooperative_admission.rs` / `cooperative_admission_tests.rs` do
  not. Discriminating: FAILS on the pre-rehome layout (primitive at
  the crate root), PASSES once rehomed.
- `crates/verter_session/tests/cache_runtime_singleflight_rehome.rs::rehomed_singleflight_owns_the_verbatim_primitive_api`
  — reads `cache_runtime/singleflight.rs`; asserts it still declares
  `pub enum ComputeAdmission<V, Entry>` with all three variants
  (`Cacheable(Entry)` / `ReturnOnly(V)` / `Failed`) and all three
  `cooperative_*` entry points. Discriminating: a rehome that
  collapsed the two type parameters or dropped an entry point fails.
- `crates/verter_session/tests/block_1_i_discriminators.rs::cooperative_return_only_not_shared_to_joiners`
  — reads the rehomed `cache_runtime/singleflight.rs` and asserts the
  `ReturnOnly` arm marks `non_cacheable_winner` (so joiners fork and
  cold-recompute) rather than broadcasting `V` through a channel.
  This is the rehome-sensitive guard repointed by this block.

#### Owning-doc updates

- `.claude/skills/type-cache-architecture/SKILL.md` — replace any
  `cooperative_admission` path references with
  `cache_runtime::singleflight`.

#### Public API mirrors

The rehomed primitive stays crate-internal — `cache_runtime` and its
`singleflight` submodule are both `pub(crate)`.

- Rust crate (`verter_session`): `pub(crate)` rehome only; no new
  public surface.
- NAPI / WASM / protocol DTO / TS types / JS wrappers / compat: no
  exposure in this block.

#### Blocks blocked by this block

- B4 (the merged cutover builds its `ArtifactNode` / `QueryNode`
  trait surface and lookup/publish runtime on top of the rehomed
  `singleflight` primitive).

### 3. Typed `SignatureAdmission` + `CompileSlot` retyping

#### Context

`finalise_signature_or_empty`
(`crates/verter_session/src/compile_fact_emission.rs:469`) collapses
overflow into `Arc::from(Vec::<_>::new())`. Worse,
`CompileSlot.fact_dep_signature: Arc<[FactVersionRef]>`
(`crates/verter_session/src/types.rs:1352`) is the carrier type
itself — there is no `overflowed` discriminant on the type, so even
if every producer correctly returned a typed admission, the value
coming back into the slot could not carry the overflow bit. H5
(skill R20 + R31) says empty and overflowed are different states.
Block 3 makes that distinction representable at the type level and
replaces the helper.

Block 3 lands AFTER Block 2 has shipped the renamed
`cache_runtime::admission::CacheAdmission`. Block 3 imports
`CacheAdmission` from `cache_runtime::admission`, defines
`SignatureAdmission` alongside it, and arranges producer call sites
so that `SignatureAdmission::NonCacheable` becomes
`CacheAdmission::ReturnOnly(value)` at the cold-compute return site.

#### Changes

`ReadSetSignature` keeps its **correctness oracle** role (path-precise
fact list + overflow bit). The **concurrency oracle**
(`validated_at_generation: u64`) moves to a separate cache-entry
wrapper. The two concerns never mix on the same struct:

```rust
// crates/verter_session/src/fact_signature_helpers.rs (UNCHANGED SHAPE)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReadSetSignature {
    pub facts: std::sync::Arc<[FactVersionRef]>,
    pub overflowed: bool,
    // validated_at_generation is NOT a field here.
}

impl ReadSetSignature {
    pub fn empty() -> Self { Self { facts: std::sync::Arc::from([]), overflowed: false } }
    pub fn overflow() -> Self { Self { facts: std::sync::Arc::from([]), overflowed: true } }
    pub fn from_facts(facts: std::sync::Arc<[FactVersionRef]>) -> Self { Self { facts, overflowed: false } }
    pub fn is_cacheable(&self) -> bool { !self.overflowed }
    // validate_with_self_roots already exists at :732 (pub(crate)).
}

// crates/verter_session/src/cache_runtime/store.rs (NEW)
#[derive(Debug, Clone)]
pub struct CacheEntry<V> {
    pub value: V,                                // Arc<...> typically
    pub signature: ReadSetSignature,             // correctness oracle
    pub validated_at_generation: u64,            // concurrency oracle
}

// crates/verter_session/src/cache_runtime/admission.rs (additions)
pub enum SignatureAdmission {
    Cacheable(ReadSetSignature),
    NonCacheable,
}

impl SignatureAdmission {
    /// Convert the tracer's `finalise()` outcome into a typed
    /// admission. `gen` is recorded on the cache entry at admission
    /// time, NOT stamped onto the signature itself.
    pub fn from_finalise(out: FactReadSetFinalise) -> Self {
        match out {
            FactReadSetFinalise::Ok(sig) => SignatureAdmission::Cacheable(
                ReadSetSignature::from_facts(sig),
            ),
            FactReadSetFinalise::Overflow => SignatureAdmission::NonCacheable,
        }
    }
}
```

Every cache publish site reads the live generation from
`snapshot.generation` (Block 1 field) and populates
`CacheEntry.validated_at_generation` at admission. Warm-hit validation
reads both `entry.signature` (for fact revalidation via
`validate_with_self_roots`) AND `entry.validated_at_generation` (for
TOCTOU / generation supersession), but the two reads target two
distinct fields on the entry — they never mix on the carrier.

Retype `CompileSlot.fact_dep_signature`
(`crates/verter_session/src/types.rs:1352`) from
`Arc<[FactVersionRef]>` to `ReadSetSignature`. The slot now
structurally carries the overflow bit. Read sites that previously
checked `fact_dep_signature.is_empty()` migrate to
`fact_dep_signature.facts.is_empty() && !fact_dep_signature.overflowed`
explicitly; warm-validation paths refuse admission when
`overflowed == true`.

Files that consume `CompileSlot.fact_dep_signature` or build
signatures (every site is in the migration commit):

- `crates/verter_session/src/types.rs` — retype the field.
- `crates/verter_session/src/compile_fact_emission.rs` — delete
  `finalise_signature_or_empty`; producers call
  `SignatureAdmission::from_finalise(tracer.finalise())` and route
  `NonCacheable` to `CacheAdmission::ReturnOnly(value)`.
- `crates/verter_session/src/host_compile.rs` — warm-hit path
  validates `slot.fact_dep_signature.is_cacheable()` before
  admission.
- `crates/verter_session/src/component_meta_materialize.rs` and
  `component_meta_caches.rs` — every per-member shape cache write-site
  adopts `SignatureAdmission` at the cold path.
- `crates/verter_session/src/semantic_query_memo/mod.rs` — every
  `MemoEntry` admission adopts the typed gate; overflow surfaces as
  a typed non-cacheable variant.
- `crates/verter_session/src/fact_signature_helpers.rs` — every helper
  that constructed `Arc<[FactVersionRef]>` directly migrates to
  returning `SignatureAdmission`.

Producer contract: any cold compute that observes the tracer's
`finalise()` overflowing returns `CacheAdmission::ReturnOnly(value)`.
The cache substrate does NOT admit; the value is returned to the
caller; subsequent cold callers cold-recompute. `ReturnOnly` is
winner-only: joiners fork and cold-recompute because there is no
carrier to validate against their own view (it carries no `Entry` /
dep-signature carrier, so a joiner cannot view-validate it). It is
NOT broadcast or shared to joiners.

#### Legacy Deletions

- DELETE `finalise_signature_or_empty` and every call site.
- DELETE every direct construction of `Arc::from(Vec::<FactVersionRef>::new())`
  outside the explicit `ReadSetSignature::empty()` /
  `ReadSetSignature::overflow()` constructors.
- DELETE every fallback branch that interpreted an empty `Arc<[]>` as
  "maybe safe" — fact-validation paths now read the explicit
  `overflowed` bit.

#### Verification

```
cargo test --package verter_session compile_fact_emission --tests --verbose
cargo test --package verter_session signature_admission --tests --verbose
cargo test --workspace --tests --verbose
cargo clippy --workspace -- -D warnings
cargo fmt --all
pnpm install --frozen-lockfile
```

#### Discriminating tests

- `crates/verter_session/tests/compile_cache_overflow_return_only.rs::compile_fact_signature_overflow_does_not_publish_compile_slot`
  — builds an SFC whose compile-time tracer overflows (synthetic
  tracer of >1024 fact refs). Positive: the compile result returns
  to the caller. Negative: `host.compile_slot_for(&canonical).is_none()`
  — no slot is published.
- `crates/verter_session/tests/admission_overflow_routes_to_return_only.rs::cold_compute_with_overflow_signature_does_not_publish_compile_slot`
  — synthetic tracer overflows. Positive: the caller receives the
  computed value (`Some(_)`); a second cold call cold-rebuilds
  (`compute_call_count == 2`). Negative: the cache map remains empty
  between calls.
- `crates/verter_session/tests/read_set_signature_carrier.rs::empty_and_overflow_are_distinguishable_at_carrier_type`
  — fails against the pre-change `Arc<[FactVersionRef]>` carrier
  because no bit exists to inspect. Positive:
  `ReadSetSignature::empty().is_cacheable() == true` and
  `ReadSetSignature::overflow().is_cacheable() == false`. Negative:
  no helper exists that converts an overflow into a cacheable
  signature.
- `crates/verter_session/tests/finalise_signature_or_empty_is_gone.rs::no_call_site_constructs_empty_signature_from_overflow`
  — greps `crates/verter_session/src/**` for
  `Arc::from\(Vec::<.*FactVersion.*>::new\(\)\)` outside whitelisted
  ctor code. Synthetic violation in a `#[cfg(test)]` fixture string
  is detected.
- `crates/verter_session/tests/separation_of_concerns.rs::read_set_signature_has_no_generation_field`
  — walks `fact_signature_helpers.rs`'s `ReadSetSignature` via
  `syn::parse_file`; asserts the struct has exactly two public fields
  named `facts` and `overflowed`, and NO field named
  `validated_at_generation`. A synthetic regression that added one
  trips the guard.
- `crates/verter_session/tests/separation_of_concerns.rs::cache_entry_carries_generation_distinct_from_signature`
  — asserts `cache_runtime/store.rs`'s `CacheEntry` has three fields:
  `value`, `signature`, `validated_at_generation`.

#### Owning-doc updates

- `.claude/skills/type-cache-architecture/SKILL.md` — update the
  signature-overflow contract section (H5 / R20 + R31) to point at
  `ReadSetSignature` and `SignatureAdmission`. Append a new
  `## Typed SignatureAdmission gate (CRITICAL)` section: producers
  convert `FactReadSet::finalise()` into `SignatureAdmission`
  (`Cacheable(ReadSetSignature)` / `NonCacheable`); overflow routes to
  `NonCacheable` which the cache substrate translates into
  `CacheAdmission::ReturnOnly(value)`. Direct construction of
  `Arc::from(Vec::<FactVersionRef>::new())` is banned. Cite the
  guards `empty_and_overflow_are_distinguishable_at_carrier_type` and
  `no_call_site_constructs_empty_signature_from_overflow`. New
  `CRITICAL_RULE_GUARDS` entry:
  `("Typed SignatureAdmission gate", &["empty_and_overflow_are_distinguishable_at_carrier_type", "no_call_site_constructs_empty_signature_from_overflow", "compile_fact_signature_overflow_does_not_publish_compile_slot"])`.
- `docs/arch/fact-based-cache.md` — append an entry noting
  `CompileSlot.fact_dep_signature: ReadSetSignature`.

#### Public API mirrors

- Rust crate (`verter_session`): `pub` `ReadSetSignature` +
  `SignatureAdmission` re-export from `cache_runtime::admission`.
- NAPI / WASM / protocol DTO / TS types / JS wrappers / compat: no
  exposure. Block 10's
  `StructuredAuditEvent::CacheNodeNonAdmission` surfaces the
  observable form of `SignatureAdmission::NonCacheable` via a typed
  reason variant.

#### Blocks blocked by this block

- B4 (every artifact node consumes `ReadSetSignature` and routes
  overflow through `SignatureAdmission::NonCacheable` →
  `CacheAdmission::ReturnOnly`).
- B5 (compile entry point consumes `SignatureAdmission` for
  session-mode admission decisions).
- B6 (`compile_many` warm-hit path validates
  `slot.fact_dep_signature.is_cacheable()`).
- B11 (flow-return budget-exceeded routes through
  `SignatureAdmission::NonCacheable`).

### 4. Artifact + query-identity cache enumeration on the new substrate

#### Context

The plan touches both content-addressed artifact caches (skill `R5`/`R6`
content/version hash in key) and query-identity caches (skill `R20`
multi-candidate, version-free key). Block 4 enumerates every one,
names its `ArtifactNode` / `QueryNode` implementation, names its skill `R20`
candidate discriminant where multi-candidate, names the skill `R30`/`R31`
exact-policy-identity audit, enforces skill `R29` module augmentation runtime
semantics, preserves skill `R28` `MemberPresence` / `Member` two-fact
granularity, and lands the
`SemanticQueryKey::Instantiate { base: DeclIdentity }` →
`SemanticQueryKey::Instantiate { base: ResolvedDeclSlotIdentity }`
migration.

This block (merged B2b+B4) also owns the new lookup/publish runtime —
the `ComputeCtx` context, the `ArtifactNode` / `QueryNode` traits, the
node-facing `CacheAdmission<V>`, the `Candidate<V>` / `QuerySlot<V>`
multi-candidate structures, and the `lookup` / `query::lookup` /
`publish` entry points — and migrates ALL direct production
cooperative-admission callers in one change; no old direct caller
path remains. `QueryCache<N>` wraps the existing `ValidatedFactCache`
/ bounded-candidate substrate, not a parallel validation map. These
are landed as compile-real implementations against today's substrate,
each with its first real consumer in the same change. After this
cutover, direct production use of `singleflight::cooperative_*` is
rejected outside `cache_runtime`; the primitive remains internal
implementation, not a second public path.

#### Changes

Per-row, one `ArtifactNode` or `QueryNode` impl is created.

**Content-addressed artifact nodes (skill `R5`/`R6` — key carries content/version):**

| Cache | Key | Value | Dim audit |
|---|---|---|---|
| `FileArtifactStore` (`IndexedReady`+facts+edges+`parse_stable_hash`+augmentations) | `FileArtifactKey { canonical, content_hash, parse_env_hash, parser_version }` | `FileArtifacts` | parse env, parser_version, content_hash |
| `ResolvedImportFacts` | `ResolvedImportFactsKey { canonical, content_hash, parse_env_hash, resolve_env_hash, parser_version }` | `ResolvedImportFacts` | parse, resolve; does NOT carry `lib_env_hash` |
| Typed-IR resolve (`ResolvedLocalType`) | `(canonical, parse_stable_hash, parse_env_hash, resolve_env_hash, type_env_hash, lib_env_hash)` | `ResolvedLocalType` | parse, resolve, type, lib |
| `MemberSemanticFactStore` | `(canonical, parse_stable_hash, parse_env_hash, exporter, member_name, symbol_space)` | `MemberSemanticFact` | parse, parse_stable_hash (cosmetic-invariant); skill `R28` `Member` fact |
| `MemberDisplayFactStore` | `(canonical, content_hash, parse_env_hash, exporter, member_name, symbol_space)` | `MemberDisplayFact` | parse, content_hash (cosmetic-sensitive); skill `R28` `Member` display |
| `ModuleAugmentationIndex` | `AugmentationTargetKey { project_identity, resolve_env_hash, lib_env_hash, target }` | `AugmenterSet` (fingerprinted `ModuleAugmentationIndexShape`) | resolve, lib (skill `R29` base-only) |
| `CompileOutputNode_PureContent` (`content` mode) | `CompileOutputKey { canonical, source_hash, parse_env_hash, resolve_env_hash, type_env_hash, lib_env_hash, compile_profile_hash, compiler_version, source_map_policy_hash }` | `CompileOutput` | full env + profile + source-map policy; persistable via Block 9 |
| `RouteOwnedShallowDb` | `(canonical, parse_stable_hash, parse_env_hash, resolve_env_hash, lib_env_hash, resolver_version)` | `RouteOwnedShallow` | parse, resolve, lib |
| `TypeResolutionContextDb` | `(canonical, content_hash, parse_env_hash, resolve_env_hash, type_env_hash, lib_env_hash, parser_version)` | `TypeResolutionContext` | full env + parser_version |
| `EvalEnvCacheDb` | `(canonical, content_hash, parse_env_hash)` | `EvalEnv` | parse only |
| `DependencyCacheDb` (parse-domain dep edges) | `(canonical, content_hash, parse_env_hash, resolve_env_hash)` | `ParseDepEdges` | parse, resolve |
| `FlowBodyHashNode` (Block 11) | `FlowBodyHashKey { canonical, function_symbol, parse_stable_hash, parser_version }` | `FlowBodyHashOutcome` (fail-closed enum — Block 11) | parse_stable_hash; body-hash production split from body lowering. `BudgetExceeded` routes via `CacheAdmission::ReturnOnly`; `Hash(_)` via `Cacheable`. |
| `FlowLoweredBodyNode` (Block 11) | `FlowLoweredBodyKey { canonical, function_symbol, parse_stable_hash, body_semantic_hash, parser_version }` | `FlowLoweredBody` | parse_stable_hash + `body_semantic_hash` (the latter produced UPSTREAM by `FlowBodyHashNode`). `FlowLoweredBodyNode::compute` does NOT call `compute_body_semantic_hash`. |

**Query-identity nodes (skill `R20` multi-candidate — key carries NO content/version/`fact_dep_signature`):**

> SUPERSEDED: the split `MaterializeMemoDb` / `MemberShapeCacheDb` shape caches are retired; the per-member materialiser cache is a slot inside `ShapeCacheDb` indexed by `ShapeSubject::SemanticNode` (unified plan §2.2; the static guard `block_6i_static_guards.rs::shape_cache_db_replaces_split_caches` forbids re-introduction). The `MemberShapeCacheDb` row below maps onto that `ShapeCacheDb` slot.

| Cache | Slot key (skill `R6`/H3) | Candidate discriminant | Notes |
|---|---|---|---|
| `RouteDb` (per-name) | `(scope, name_token, resolve_env_hash, lib_env_hash, project_identity)` | source canonical's `whole_hash` + observed facts | skill `R29` base-only |
| `RouteDb` (effective barrel surface) | `(barrel_canonical, resolve_env_hash, lib_env_hash, project_identity)` | barrel `whole_hash` + facts | session-view fail-closed |
| `RouteDb` (effective export set) | `(canonical, resolve_env_hash, lib_env_hash, project_identity)` | own `whole_hash` + facts | base-only |
| `MaterializeStructureDb` | `(slot_identity, projection_mode, type_env_hash, lib_env_hash, project_identity)` | source `VersionedDeclIdentity` + facts | structural materialiser |
| `RefCycleResultDb` | `(decl_slot_identity, type_env_hash, lib_env_hash, project_identity)` | source `VersionedDeclIdentity` + facts | strict self-root |
| `SemanticGraphStore` (family + relation + named-type) | `SemanticQueryKey` (post-migration — no `whole_hash` in `Instantiate` or `ResolveMacroPayload`) | per-candidate `VersionedDeclIdentity` + facts | migration |
| `ComponentMetaResultDb` | `(owner_slot_identity, options_fingerprint, type_env_hash, lib_env_hash, project_identity)` | owner `VersionedDeclIdentity` + facts | final-result cache |
| `MemberShapeCacheDb` | `(scope, member_semantic_node_id, projection_mode, type_env_hash, lib_env_hash, project_identity)` | observation `validated_at_generation` + facts | strict self-root |
| `AnalysisReadyDb` (slot) | `AnalysisSlotKey { canonical_id, project_identity }` | `AnalysisCandidate { whole_hash, scope, validated_at_generation, facts }` (bitflag-containment match) | skill `R20` multi-candidate |
| `OwnerImportSurfaceDb` | `(owner_slot_identity, resolve_env_hash, lib_env_hash, project_identity)` | owner `VersionedDeclIdentity` + facts | direct-owner-import surface |
| `ImportedRootDb` | `(scope, resolve_env_hash, lib_env_hash, project_identity)` | scope `VersionedDeclIdentity` + facts | imported-root projection |
| `AppConfigNoOverrideProofDb` | `(scope, parse_env_hash, project_identity)` | scope `VersionedDeclIdentity` + facts | no-override proof |
| `ResolvedTypeCacheDb` | `(slot_identity, type_env_hash, lib_env_hash, project_identity)` | `VersionedDeclIdentity` + facts | explicit skill `R20` multi-candidate (distinct from typed-IR resolve which is file-identity domain) |
| `CompileOutputNode_FactValidatedSession` (`session` mode) | `CompileOutputSlotKey { canonical, parse_env_hash, resolve_env_hash, type_env_hash, lib_env_hash, compile_profile_hash, compiler_version, source_map_policy_hash, project_identity }` | `CompileOutputCandidate { source_hash, versioned_decl: VersionedDeclIdentity, facts: ReadSetSignature, validated_at_generation, self_root_canonicals, value: Arc<CompileOutput> }` | full env + profile + source-map policy; slot key carries NO `source_hash` (skill `R6`/H3); memory-only (NOT persisted). |

**`AnalysisReadyDb` bitflag-containment invariant.** The slot key
`AnalysisSlotKey { canonical_id, project_identity }` carries NO
`whole_hash`. The bitflag-containment behavior lives entirely on the
`AnalysisCandidate` discriminant: a candidate admitted with
`scope = Broad` satisfies a later query for `scope = Narrow` IFF
`broad.bitflags.contains(narrow.bitflags) && broad.whole_hash ==
expected_whole_hash`. Query-identity keys stay free of `whole_hash`.
Discriminating test:
`analysis_ready_bitflag_containment::broader_scope_candidate_satisfies_narrower_query_at_same_whole_hash`.

**Per-query-family self-root validation contract.** This cutover owns
the per-query-family self-root validation contract. Each query
family's `self_root_canonicals` content:

| Query family | `self_root_canonicals` content |
|---|---|
| `MaterializeStructureDb` | base node's declaration-origin file canonical (empty for `Global` origin) |
| `RefCycleResultDb` | BFS root file canonical + every visited declaration's file canonical |
| `SemanticGraphStore` query nodes | keyed canonical for `ResolveDecl` / `TypeOf` / `Instantiate` / `ResolveMacroPayload`; file-derived origin canonical for nodeid-keyed kinds |
| `ComponentMetaResultDb` | owner canonical |
| `MemberShapeCacheDb` | scope file canonical + observation-generation anchor canonical |
| `RouteDb` per-name / barrel / effective-set | source canonical |
| `CompileOutputNode_FactValidatedSession` | source canonical |

**Sub-caches consumed by `ComponentMetaResultDb` (NOT standalone
query-identity slots — listed for inventory completeness):**

| Sub-cache | Parent query family | Role |
|---|---|---|
| `DeclarationLookupDb` | `ComponentMetaResultDb` | declaration lookup for owner resolution |
| `OwnerCollectionDb` | `ComponentMetaResultDb` | owner-set assembly for fallthrough propagation |
| `ShapeCacheDb` | `ComponentMetaResultDb` | per-shape skeleton cache |

> Retired-history note: the prepared-surface walker cluster
> (`PreparedTargetDb`, `PreparedSurfaceDb`, `PreparedMemberDb`,
> `RoutedExprSurfaceDb`) was deleted with the materializer/walker
> subgraph. `define_*` and route surfaces now resolve through the
> dispatch projectors; these DB types and their entries no longer exist.

**Caches RETIRED in Block 8 (subsumed by direct `ProjectTypeStore` ownership):**

- `CompileCacheDb` — retired. Replaced by
  `CompileOutputNode_PureContent` (content mode — artifact node,
  persistable via Block 9) AND `CompileOutputNode_FactValidatedSession`
  (session mode — query node, memory-only, fact-validated at warm hit,
  NEVER persisted). `Stateless` mode bypasses both nodes entirely.
- `DerivedRawCacheDb` — retired; the raw display string layer is
  display-only passthrough on `MemberDisplayFact`.
- `semantic_db` — retired; subsumed by `SemanticGraphStore` direct
  ownership in `ProjectTypeStore`.

**Session-view fail-closed contract (skill R29 line 745).** The
augmentation-sensitive query path returns an OBSERVABLE typed error
on a session view, NOT a "valid but non-cacheable" result. The
runtime contract:

```rust
// crates/verter_session/src/route_db.rs (post-cutover signature)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectiveExportSetError {
    SessionAugmentationNotSupported,
    // ... other observable errors
}

impl RouteDb {
    pub fn get_or_compute_effective_export_set(
        &self,
        canonical: &CanonicalId,
        view: &dyn StoreView,
    ) -> Result<std::sync::Arc<EffectiveExportSet>, EffectiveExportSetError> {
        if view.compat_token().is_session() {
            return Err(EffectiveExportSetError::SessionAugmentationNotSupported);
        }
        // base-only path: compute, admit
    }
}
```

The session caller observes the typed error and surfaces it (or
short-circuits its own pipeline) — it does NOT receive a valid
base-only `EffectiveExportSet`. `CacheAdmission::ReturnOnly` is the
WRONG shape here because it would return a valid base computation to
the session caller.

**Skill `R28` two-fact model.** `MemberPresence` (key exists) and `Member`
(key's body identity) are SEPARATE facts. Adding `Foo.b` invalidates
only consumers that observed `MemberPresence("b")` or
`Member("b", _)`; consumers that observed only `Member("a", _)` stay
warm. Block 4 preserves this verbatim through
`MemberSemanticFactStore` / `MemberDisplayFactStore` node
implementations.

**Member fact key shape.** Both `MemberSemanticFactStore` and
`MemberDisplayFactStore` carry the existing six-field key shape:
`(canonical, parse_stable_hash | content_hash, parse_env_hash,
exporter, member_name, symbol_space)`. The `parse_stable_hash` vs
`content_hash` choice distinguishes cosmetic-invariant vs
cosmetic-sensitive caches. The `symbol_space ∈ {Type, Value,
Namespace}` field is required by skill R11.

**`SemanticQueryKey` migration.** `SemanticQueryKey::Instantiate
{ base: DeclIdentity }` → `SemanticQueryKey::Instantiate { base:
ResolvedDeclSlotIdentity }`. `whole_hash` moves to candidate
`VersionedDeclIdentity` on the `MemoEntry`. Same migration for
`ResolveMacroPayload { owner: DeclIdentity }` → `ResolveMacroPayload {
owner: ResolvedDeclSlotIdentity }`. Every caller updates in the same
commit.

`ResolvedDeclSlotIdentity` is defined in
`crates/verter_session/src/semantic_query.rs:237` (existing file —
the slot-identity projection of `VersionedDeclIdentity` that drops
`whole_hash` and carries the canonical declaring file, the merged
symbol name, the symbol space discriminator, and the
`project_identity` / `type_env_hash` / `lib_env_hash` dimensions).

#### Legacy Deletions

- DELETE `DeclIdentity` usage as a key field on
  `SemanticQueryKey::Instantiate` and `ResolveMacroPayload`;
  replace with `ResolvedDeclSlotIdentity`.
- DELETE bespoke `clear_*_cache(canonical)` helpers introduced for
  query-identity caches (cross-file invalidation is lazy via
  fact-revalidation).
- DELETE redundant per-query in-flight maps that existed only because
  `cooperative_admission` was not yet uniformly applied at the
  query-node layer.

#### Verification

```
cargo test --package verter_session file_artifact_store --tests --verbose
cargo test --package verter_session route_db --tests --verbose
cargo test --package verter_session component_meta --tests --verbose
cargo test --package verter_session semantic_query --tests --verbose
cargo test --workspace --tests --verbose
cargo clippy --workspace -- -D warnings
cargo fmt --all
pnpm install --frozen-lockfile
```

#### Discriminating tests

- `crates/verter_session/tests/cache_key_runtime_guards.rs::semantic_query_keys_contain_no_content_hash_or_fact_signature`
  — fails against pre-migration
  `SemanticQueryKey::Instantiate { base: DeclIdentity { whole_hash, .. } }`.
  Walks every `SemanticQueryKey::*` variant via `syn::parse_file`;
  asserts `base` field type is `ResolvedDeclSlotIdentity` and rejects
  any `whole_hash` / `content_hash` / `fact_dep_signature` field.
- `crates/verter_session/tests/query_node_multi_candidate.rs::concurrent_overlay_variants_coexist_as_bounded_candidates`
  — inserts two overlay variants of the same `ComponentMetaResultDb`
  slot; asserts both candidates are present and each revalidates only
  under its own snapshot. Negative: the first is not overwritten.
- `crates/verter_session/tests/module_augmentation_runtime.rs::effective_export_set_rejects_session_view`
  — under base view, augmenter set computes and admits. Under session
  view, `RouteDb::get_or_compute_effective_export_set` returns
  `Err(SessionAugmentationNotSupported)`. The `ReturnOnly` shape is
  explicitly the wrong contract here.
- `crates/verter_session/tests/module_augmentation_runtime.rs::augmentation_index_population_is_incremental`
  — adding one augmenter touches one `AugmentationTargetKey` entry;
  no full-corpus rescan counter increment.
- `crates/verter_session/tests/module_augmentation_runtime.rs::augmenter_set_fingerprint_changes_on_add_remove`
  — fingerprint changes on add/remove; stable across identical sets
  in different insertion orders.
- `crates/verter_session/tests/member_two_fact_model.rs::pick_literal_key_observes_member_presence_and_member`
  — `type Foo = { a: A; b: B };` consumer picks `Foo['a']`. Editing
  `Foo.a` body invalidates; adding `Foo.b` (and not touching `Foo.a`)
  does NOT invalidate.
- `crates/verter_session/tests/exact_policy_identity_audit.rs::every_artifact_node_key_carries_required_dimensions`
  — walks every artifact-node trait impl; asserts each key struct
  carries the dimensions named in the table above.
- `crates/verter_session/tests/analysis_ready_bitflag_containment.rs::broader_scope_candidate_satisfies_narrower_query_at_same_whole_hash`
  — admit `AnalysisCandidate` with `scope = Broad`, `whole_hash = H`,
  `bitflags = {A,B,C}`. Query with `scope = Narrow`, `whole_hash = H`,
  `bitflags = {A}`. Single cache hit (`compute_call_count == 0`).
  Querying with a different `whole_hash` does NOT match. Asserts
  reflectively that `AnalysisSlotKey` has NO `whole_hash` field.
- `crates/verter_session/tests/member_fact_key_shape.rs::member_fact_store_keys_carry_six_explicit_fields`
  — walks `component_meta_caches.rs` (or wherever the keys live) via
  `syn::parse_file`; asserts each key struct exposes exactly six
  fields. A regression collapsing to a single `slot_identity` blob
  fails.
- `crates/verter_session/tests/compile_output_session_slot_key_has_no_source_hash.rs::compile_output_session_slot_key_has_no_source_hash`
  — walks `CompileOutputSlotKey` via `syn::parse_file`; asserts nine
  named fields and NO `source_hash` / `content_hash` / `whole_hash` /
  `parse_stable_hash` / `body_semantic_hash` field. Companion
  assertion: `CompileOutputCandidate` carries `source_hash`,
  `versioned_decl`, `facts`, `validated_at_generation`,
  `self_root_canonicals` (canonicals only — matching
  `FileWholeHash` lives in `facts`), and `value: Arc<CompileOutput>`.
- `crates/verter_session/tests/query_node_self_root_validation.rs` —
  one test per query family verifying warm-read revalidation AND
  post-compute publish-gate revalidation. The seven warm-read tests
  cover `MaterializeStructureDb`, `RefCycleResultDb`,
  `SemanticGraphStore` query nodes (parameterised over all 11
  `SemanticQueryKey` variants), `ComponentMetaResultDb`,
  `MemberShapeCacheDb`, `RouteDb` (three RouteDb variants), and
  `CompileOutputNode_FactValidatedSession`. The seven post-compute
  tests verify Block 2's `publish` pipeline rejects candidates whose
  self-root was edited mid-compute (per
  `RejectedSupersededSelfRoot`). Each test uses the
  `ReadSetSignature::validate_with_self_roots(ctx, &candidate.self_root_canonicals)`
  contract from
  `crates/verter_session/src/fact_signature_helpers.rs:732`, NOT any
  nonexistent `WorldSnapshot::current_whole_hash` accessor.
- `crates/verter_session/tests/session_compile_cache.rs::session_mode_warm_hit_is_under_one_millisecond`
  — cold compile under `CompileCacheMode::Session` admits a candidate
  to the in-memory multi-candidate store; warm re-request observes
  `cache_node_metrics.hit == 1` and wall-clock < 1 ms; cold elapsed
  > 1 ms. An implementation routing session-mode warm re-requests
  through the artifact-node path would exceed 1 ms.
- `crates/verter_session/tests/resolved_type_cache_db_preserved.rs::resolved_type_cache_db_remains_r20_query_node`
  — walks `ResolvedTypeCacheDb`'s definition; asserts slot key has
  the four fields `slot_identity`, `type_env_hash`, `lib_env_hash`,
  `project_identity` and NO `whole_hash` / `content_hash`. Asserts
  per-slot candidate carries `VersionedDeclIdentity` +
  `ReadSetSignature.facts`. Any regression collapsing
  `ResolvedTypeCacheDb` into typed-IR resolve fails.

#### Owning-doc updates

- `.claude/skills/type-cache-architecture/SKILL.md` — replace the
  per-cache table in "R29 / R30 / R31 exact policy identity" with the
  full enumeration above; add the artifact-node vs query-node legend.
- `docs/arch/fact-based-cache.md` — replace the per-cache key
  composition appendix with this block's enumeration, including the
  `ResolvedDeclSlotIdentity` migration row.

#### Public API mirrors

Cache identity is crate-internal.

- Rust crate: `pub(crate)` migration only.
- NAPI / WASM / protocol DTO / TS types / JS wrappers / compat: no
  change in this block.

#### Blocks blocked by this block

- B5 (`CompileOutputNode_PureContent` / `_FactValidatedSession` are
  the content/session targets).
- B6 (`compile_many` writes through artifact nodes).
- B8 (host-cache rehoming uses these nodes).
- B9 (only artifact nodes implement `PersistentArtifactNode`).
- B11 (`FlowLoweredBodyNode` artifact node mirrors this shape).

### 5. Public `CompileCacheMode` + typed downgrade signal

#### Context

Today `verter_napi::compile` exposes one ambiguous compile
entry-point. The cache-runtime hard rule from `CLAUDE.md` → "Cache
Architecture (CRITICAL)" — "public APIs expose distinct `stateless`,
`content`, and `session` semantics" — is violated because callers
cannot tell whether they are getting a stateless compile, a
content-cache compile, or a full session compile; they cannot opt out
of the heaviest path; and fast-path eligibility silently downgrades.
The block adds explicit modes, threads them through every binding
surface, and lets the caller observe an `actual_mode` that may differ
from `requested_mode` (silent downgrade becomes observable).

#### Changes

Add the typed mode:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompileCacheMode {
    Stateless,
    Content,
    Session,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DowngradeReason {
    HasExternalSrc,
    HasMacroTypeDeps,
    HasWorkspaceAlias,
    HasModuleAugmentation,
    HasBlockOverride,
    HasStyleOverride,
    HasIdeOnlyAnalysis,
    HasDevLastGood,
}

#[derive(Debug, Clone)]
pub struct CompileResult {
    pub code: std::sync::Arc<str>,
    pub source_map: Option<std::sync::Arc<str>>,
    pub errors: Vec<String>,
    pub requested_mode: CompileCacheMode,
    pub actual_mode: CompileCacheMode,
    pub downgrade_reason: Option<DowngradeReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceMapPolicy {
    Inline,
    External,
    None,
}
```

Both enums expose a `stable_hash() -> Hash16` so callers can populate
`WorldSnapshot::public_api_mode_hash` and
`WorldSnapshot::source_map_policy_hash` deterministically. The
conversion is byte-deterministic across builds and Rust versions
(blake3 over a namespaced byte representation; `std::hash::Hasher` is
forbidden — its output is unspecified):

```rust
impl CompileCacheMode {
    pub fn stable_hash(self) -> Hash16 {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"verter.compile_cache_mode.v1:");
        match self {
            Self::Stateless => hasher.update(b"stateless"),
            Self::Content   => hasher.update(b"content"),
            Self::Session   => hasher.update(b"session"),
        };
        let digest = hasher.finalize();
        let mut out = [0u8; 16];
        out.copy_from_slice(&digest.as_bytes()[..16]);
        out
    }
}

impl SourceMapPolicy {
    pub fn stable_hash(self) -> Hash16 {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"verter.source_map_policy.v1:");
        match self {
            Self::Inline   => hasher.update(b"inline"),
            Self::External => hasher.update(b"external"),
            Self::None     => hasher.update(b"none"),
        };
        let digest = hasher.finalize();
        let mut out = [0u8; 16];
        out.copy_from_slice(&digest.as_bytes()[..16]);
        out
    }
}
```

The namespaced prefixes guarantee the two enum hash spaces never
collide and give a forward-compatible version bump path. Block 5
introduces `blake3` as a regular dependency of
`crates/verter_session/Cargo.toml`; Block 9's `cas_key_hash` consumes
the same dependency.

```diff
 # crates/verter_session/Cargo.toml
 [dependencies]
+blake3 = "1"
 # ... existing deps
```

Eligibility predicate (consumed by both single-file `compile` and
batch `compile_many`): a request is `Stateless`-eligible iff the input
has no external `src` blocks, no `macro_type_deps`, no
workspace-alias-driven codegen, no module augmentation observation,
no block/style override layer, no IDE-only template/type analysis,
and no dev last-good behavior. If any condition fails, the runtime
downgrades to `Content` (or `Session` if content alone cannot
satisfy). The result reports the realized `actual_mode` and the first
failed condition as `downgrade_reason`.

Plumbing:

- `crates/verter_session/src/host_compile.rs` —
  `compile` / `compile_many` take `requested_mode: CompileCacheMode`,
  return `CompileResult` carrying `actual_mode` + `downgrade_reason`.
- `crates/verter_session/src/compile_fact_emission.rs` — fact tracer
  installed only on `Session` mode. Mode → node routing:
  - `Stateless` — bypasses every host cache.
  - `Content` — routes through `CompileOutputNode_PureContent`
    (artifact node from Block 4; persistable via Block 9).
  - `Session` — routes through
    `CompileOutputNode_FactValidatedSession` (query node from
    Block 4; memory-only). Warm hits validate the candidate's
    `ReadSetSignature.facts` against the caller's `StoreView` before
    return.
- `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs`
  — codify the eligibility predicate as
  `classify_compile_mode(input, world) -> (CompileCacheMode, Option<DowngradeReason>)`.
- `crates/verter_napi/src/lib.rs` — accept `compile_cache_mode: Option<String>`
  (`"stateless"` / `"content"` / `"session"`); default `"session"`;
  serialize `actual_mode` + `downgrade_reason` on the response.
- `crates/verter_wasm/src/lib.rs` — same surface as NAPI.
- `packages/native/index.ts` — TS binding type `CompileCacheMode` as
  a union; `compile` / `compileMany` signatures gain `cacheMode?: CompileCacheMode`.
  Return type gains `actualMode` + `downgradeReason`.
- `packages/native/index.js` — runtime unchanged; only types.
- `packages/wasm/index.ts` — mirror.
- `crates/verter_protocol/src/*` — `CompileRequestDto` adds
  `cache_mode: Option<CompileCacheMode>`; `CompileResponseDto` adds
  `actual_mode: CompileCacheMode` + `downgrade_reason: Option<DowngradeReason>`.
- `packages/types/audit.generated.ts` — regenerate via
  `node scripts/gen-corpus-audit-tests.mjs`; includes
  `StructuredAuditEvent::CompileModeDowngrade` variant + the
  `CompileCacheMode` union.
- `@verter/component-meta/compat` — reads `actualMode` from the
  underlying native call; pass-through; compat always runs against
  `session` internally.
- `packages/benchmark/src/apple-to-apple.ts` — every benchmark
  scenario records `cacheMode`, `actualMode`, `downgradeReason`.

#### Legacy Deletions

- DELETE the ambiguous single compile entry-point that hid mode
  selection.
- DELETE any silent downgrade path that did not surface
  `actual_mode`.

#### Verification

```
cargo test --package verter_session compile_mode --tests --verbose
cargo test --package verter_napi compile_mode --tests --verbose
pnpm --filter @verter/native test
cargo test --workspace --tests --verbose
cargo clippy --workspace -- -D warnings
cargo fmt --all
pnpm install --frozen-lockfile
```

#### Discriminating tests

- `crates/verter_session/tests/compile_mode_equivalence.rs::stateless_and_session_produce_byte_identical_output_for_dependency_free_sfc`
  — `let stateless_result = compile(..., Stateless); let session_result = compile(..., Session);`.
  Positive: `stateless_result.code == session_result.code` AND
  `stateless_result.source_map == session_result.source_map`
  byte-identical (the source map's `mappings` segment encodes the
  codegen transform path so a stub returning the same value by
  short-circuiting codegen would still fail).
- `crates/verter_session/tests/compile_mode_divergence.rs::content_and_session_differ_for_sfc_with_workspace_alias`
  — SFC with workspace alias requiring resolver observation. Session
  returns resolved alias type; Content returns
  `actual_mode = Session` with
  `downgrade_reason = HasWorkspaceAlias`. Content does NOT silently
  return a stale or empty alias.
- `crates/verter_session/tests/compile_mode_observable_downgrade.rs::compile_mode_observable_downgrade_reports_actual_mode`
  — calls with `requested_mode = Stateless` on input with `src`
  block. `actual_mode == Content` (or `Session`), `downgrade_reason ==
  Some(HasExternalSrc)`. Call does not panic, does not return
  `actual_mode == Stateless`.
- `packages/native/index.spec.ts::compile_cache_mode_is_explicit_and_defaults_are_stable`
  — passing `{ cacheMode: "stateless" }` returns an object with
  `actualMode` set; default (omitted) is `"session"`; invalid string
  throws a typed error.
- `packages/native/index.spec.ts::actual_mode_is_surfaced_to_js`
  — returned object's `actualMode` is one of `"stateless" | "content"
  | "session"`; not `undefined`.
- `crates/verter_session/tests/world_snapshot_mode_and_policy_divergence.rs::otherwise_identical_snapshots_diverge_on_cache_mode_and_source_map_policy`
  — sweeps the three `CompileCacheMode` variants and three
  `SourceMapPolicy` variants; asserts pairwise
  `snapshot_a.public_api_mode_hash != snapshot_b.public_api_mode_hash`
  for distinct pairs. Identical inputs hash equal
  (`Stateless.stable_hash() == Stateless.stable_hash()`). Inline-hex
  literal pin guards against a silent switch to
  `std::hash::Hasher`.

#### Owning-doc updates

- `CLAUDE.md` (Cache Architecture section) — append a "Cache modes"
  subsection naming the three modes and the public-API mirror.
- `.claude/skills/type-cache-architecture/SKILL.md` — append a
  `CompileCacheMode` synopsis under "Cache substrate" pointing at the
  three public-API cache modes and the per-mode cache-layer access
  table.
- `docs/arch/fact-based-cache.md` — append a "Per-mode cache layer
  access" table.

#### Public API mirrors

- Rust crate (`verter_session::host_compile`): `compile(request:
  CompileRequest)` + `compile_many(inputs, options: CompileBatchOptions)`.
- NAPI: `compile_cache_mode: Option<String>` on the request;
  `actual_mode: String` + `downgrade_reason: Option<String>` on the
  response.
- WASM: same shape.
- FFI shared crate (`crates/verter_ffi/src/`): adds
  `FfiCompileCacheMode` (`#[serde(rename_all = "camelCase")]` over
  `"stateless" | "content" | "session"`), `FfiDowngradeReason`,
  `ffi_compile_cache_mode_to_host`, `host_actual_mode_to_ffi`,
  `host_downgrade_reason_to_ffi`, and
  `FfiConversionError::InvalidCompileCacheMode(String)` (next to the
  existing `InvalidCompileErrorPolicy(String)` precedent).
- Protocol DTOs: `CompileCacheMode` + `DowngradeReason` enums + the
  two new fields.
- TS generated types: regenerated.
- JS wrappers / compat: pass-through.

#### Blocks blocked by this block

- B6 (`compile_many` consumes `CompileCacheMode` per-input).
- B12 (benchmarks report `cache_mode` per row).

### 6. `compile_many` as a transactional batch on a host-owned CPU pool

#### Context

`crates/verter_session/src/host_compile.rs:117`'s `compile_many` is
the hottest production path that touches the cache runtime. Two
regressions to avoid:

- The per-call Rayon pool at lines 143–147 guarantees an 8 MiB
  Windows stack (`compile_many_default_pool_has_8mib_stack`). That
  guarantee must survive — Rayon's global pool's default 1 MiB
  Windows stack is not acceptable.
- The existing per-canonical submit-wait loop is not a transaction —
  duplicate-canonical handling, source-diff dedup, parse publication,
  and admission are interleaved, defeating batch dedup.

Block 6 turns `compile_many` into a transaction over the cache
runtime + host CPU pool.

> **Status (current tree).** §6a–§6c are LANDED: the host-owned
> CPU pool (`HostCpuPool`, which on the current tree coordinates every
> host batch API) alongside the scheduler's pre-existing internal stage
> `cpu_pool` field, the construction-time
> `HostConfig::host_cpu_threads` worker sizing, the removal of the
> per-call `CompileBatchOptions.threads` option, the per-input
> `requested_mode` + classifier-owned `actual_mode`, and the atomic
> batch admission (`compile_many` → `upsert_many_with_priority` → one
> `submit_batch_atomic` + one `wait_batch`) are all on the tree. The
> §6d∪§6e finalization (this block, also LANDED) gates the compile-tier
> prefetch to `Session` and skips the empty-`macro_type_deps` collector
> setup (the *Legacy Deletions* below describe both). The per-call
> CPU-concurrency cap — `CpuConcurrencySemaphore` propagation through
> `CacheNodeDagNode` — is NOT part of Block 6; it is a **Block-7**
> design concept and is not on the tree. Until B7 lands, scheduler-side
> admission runs at the pool's default concurrency.

#### Changes

**Dual-pool design.** Block 6 introduces TWO distinct host-owned CPU
pools to eliminate the deadlock between the `compile_many` outer wait
and the scheduler's per-task CPU executor. The single-pool design
would allow a saturated scheduler CPU pool to starve `compile_many`'s
outer collect/order phase that itself blocks on scheduler-queued
parse work.

- **`HostCpuPool`** — owned by `VerterHost`. As introduced in this
  block it backed `compile_many`'s outer batch coordinator (the
  synchronous collect/order/finalise phase). A later change (the
  batch-pool deadlock fix) routes the component-meta batch coordinator
  through this **same** host pool, so in the current tree `HostCpuPool`
  is the shared coordinator pool for **every** host batch API — see
  the `HostBatchCoordinator` primitive in
  `crates/verter_session/src/host_batch_coordinator.rs`. Its workers do
  NOT execute scheduler stage work. Located at
  `crates/verter_scheduler/src/host_cpu_pool.rs` so both the
  host-side `verter_session::host_compile` and the scheduler can
  reference the type (H20 — `verter_scheduler → verter_session` is
  forbidden, so the lowest shared owner is the scheduler crate).
- **`SchedulerCpuPool`** — defined in Block 7. The host constructs
  `Arc::new(SchedulerCpuPool::new(scheduler_cpu_threads))` at startup
  and passes the Arc into `Scheduler::new`. The scheduler dispatches
  every `TaskKind::Parse` and `TaskKind::CacheNode` onto it.

Both pools build their underlying `rayon::ThreadPool` with
`.stack_size(8 * 1024 * 1024)`.

```rust
// crates/verter_scheduler/src/host_cpu_pool.rs
pub struct HostCpuPool {
    pool: rayon::ThreadPool,
}

impl HostCpuPool {
    pub fn new(num_threads: usize) -> Self {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .stack_size(8 * 1024 * 1024)
            .build()
            .expect("HostCpuPool::new failed to build rayon ThreadPool");
        Self { pool }
    }

    pub fn install<R: Send>(&self, f: impl FnOnce() -> R + Send) -> R {
        self.pool.install(f)
    }
}
```

`VerterHost` owns `Arc<HostCpuPool>` and exposes it via
`host_cpu_pool()`. The scheduler does NOT know about the host pool —
the host pool is the coordinator-only side of the dual-pool design,
constructed and stored entirely on the host. As introduced here
`compile_many` was the lone consumer of
`host.host_cpu_pool().install(...)` for its outer coordinator phase;
in the current tree the same host pool serves the outer coordinator of
**every** host batch API (component-meta batch included) via the shared
`HostBatchCoordinator`, so it is no longer `compile_many`-exclusive.
The scheduler's own CPU stage executor never touches the host pool. The
two pools never share workers, and the scheduler has no reference to
`HostCpuPool` at all.

Workers register as `CallerKind::External` in TLS via the host pool's
`start_handler`, so when the coordinator blocks on a scheduler
completion handle (via `wait_or_drive`) the host worker parks on the
condvar rather than inline-executing scheduler CPU tasks. The dual-
pool isolation eliminates the deadlock class where a saturated
scheduler CPU pool could starve `compile_many`'s outer coordinator.

Replace `compile_many` with a transaction:

```rust
pub fn compile_many(
    &self,
    inputs: Vec<CompileBatchInput>,
    options: CompileBatchOptions,
) -> Vec<CompileBatchEntry> {
    let snapshot = WorldSnapshot::from_request(
        self.resolver_context(),
        self.world_snapshot_dims(),       // host pre-computes the dim bundle
        self.overlay_identity(),          // None on the base rail; Some(_) under a session
        options.cache_mode.stable_hash(),
        options.source_map_policy.stable_hash(),
    );
    let txn = CompileBatchTxn::begin(self, snapshot, inputs, options);
    let txn = txn.dedupe_inputs();
    let txn = txn.classify_modes();             // per-input: Stateless / Content / Session
    let txn = txn.compute_source_diffs();
    let txn = txn.submit_required_parse_jobs(); // through scheduler (B7)
    let txn = txn.wait_parse_jobs();
    let txn = txn.publish_artifacts_once();     // single artifact-node admission phase
    let txn = txn.compile_outputs_through_nodes();
    let txn = txn.admit_cache_entries_once();   // single admission phase
    txn.finish() // returns Vec<CompileBatchEntry> preserving input order
}

pub struct CompileBatchInput {
    pub canonical: std::sync::Arc<str>,
    pub source: std::sync::Arc<str>,
    pub requested_mode: CompileCacheMode,
}

pub struct CompileBatchEntry {
    pub canonical: std::sync::Arc<str>,
    pub result: CompileResult,
}

pub struct CompileBatchOptions {
    pub cache_mode: CompileCacheMode,
    pub source_map_policy: SourceMapPolicy,
    pub priority: Option<Priority>,    // `verter_scheduler::Priority`
}
```

The transaction:

- preserves input order in output;
- compiles each unique canonical once;
- reports duplicate canonical/source conflicts deterministically
  (sorted by canonical id);
- performs one publish phase for cache-visible source changes;
- batches VFS edge recording and overlay notifications;
- runs its OUTER coordinator on `HostCpuPool` via
  `host.host_cpu_pool().install(...)`. Scheduler-side dispatch of every
  `TaskKind::Parse` and `TaskKind::CacheNode` runs on
  `SchedulerCpuPool`. The two pools are independent
  `rayon::ThreadPool` instances; worker sets do not intersect;
- exposes one audit envelope for the batch and per-file child spans.

`cache_mode` and `source_map_policy` flow into
`WorldSnapshot::from_request` via their `stable_hash() -> Hash16`
conversions. Callers that do not set `cache_mode` get
`CompileCacheMode::Session`; callers that do not set
`source_map_policy` get the workspace default.

**Worker-count semantics.** `compile_many` has no per-call thread
option. The `HostCpuPool` worker count is sized once at host
construction from `HostConfig::host_cpu_threads`
(`Option<usize>`; `None` resolves to
`std::thread::available_parallelism()`, `Some(0)` is normalised to
the same default, `Some(n)` for `n >= 1` caps at `n`). The pool is
reused across every `compile_many` call on the same host, so per-call
sizing is no longer reachable from the public API. `CompileBatchOptions`
carries only `priority` + `default_mode` — no `threads` / `thread_count`
/ `num_threads` field (locked by the
`compile_batch_options_has_no_thread_field` static guard in
`crates/verter_session/tests/architecture_guards.rs`). Per-call
concurrency capping on `SchedulerCpuPool` admissions (the
`CpuConcurrencySemaphore` propagation through `CacheNodeDagNode`) is a
**Block-7** design concept that is NOT on the tree; until B7 lands,
scheduler-side admission runs at the pool's default concurrency.

#### Legacy Deletions

- DELETE the per-call
  `rayon::ThreadPoolBuilder::new().num_threads(thread_count).stack_size(8 * 1024 * 1024).build()`
  block at `crates/verter_session/src/host_compile.rs:143-147`.
- DELETE per-file submit-and-wait scheduler loops inside batch
  compilation.
- DELETE per-canonical interleaved publish/admission paths inside
  `compile_many`.
- GATE the compile-tier `prefetch_compile_tier_observation_targets`
  (cross-file import-route cache + dependency `ensure_indexed_ready`
  pre-population) to `actual_mode == Session`. The prefetch only feeds
  the compile-tier fact tracer, which is installed for `Session` alone;
  `Content` / `Stateless` compile with no fact rail and produce their
  cross-file correctness independently via `compile_entry`, so running
  the prefetch for those modes was load + index work nobody records.
- SKIP the external-macro-type collector SETUP (resolver context +
  `collect_external_types_from_loaded_files`) when `macro_type_deps` is
  empty — the collector iterates only `macro_type_deps`, so it returns
  an empty result anyway. `sync_transitive_macro_type_dependencies`
  stays UNCONDITIONAL: its `replace_semantic_transitive(canonical, {})`
  clears the semantic dependency axis when the set is empty (closes
  F15).

#### Verification

```
cargo test --package verter_session host_compile --tests --verbose
cargo test --package verter_session compile_many --tests --verbose
cargo test --workspace --tests --verbose
cargo clippy --workspace -- -D warnings
cargo fmt --all
pnpm install --frozen-lockfile
```

#### Discriminating tests

- `crates/verter_session/src/host_compile_tests.rs::compile_many_uses_scheduler_owned_cpu_pool_not_local_rayon`
  — instruments `HostCpuPool` with a build-counter atomic. Positive:
  after `n` back-to-back `compile_many` calls,
  `HostCpuPool::build_count == 1`. Per-call build is detectable
  (counter > 1) and fails.
- `crates/verter_session/src/host_compile_tests.rs::compile_many_pool_has_8mib_stack`
  — 200-nested-`<div>` template runs through the host pool. Positive:
  compiles successfully. 1 MiB-stack default would panic.
- `crates/verter_session/tests/compile_many_one_pool_per_host.rs::two_back_to_back_compile_many_share_pool`
  — pool build counter is 1 across two batches.
- `crates/verter_session/tests/compile_many_transactional.rs::duplicate_canonicals_compile_once_and_report_per_input`
  — 5 inputs sharing 2 canonicals; each canonical compiles exactly
  once (`compile_one_call_count == 2`); every original input
  receives its result in original order.
- `crates/verter_session/tests/compile_many_publish_phase.rs::publish_phase_runs_once_per_batch`
  — artifact-node admission counter increments by exactly the number
  of unique canonicals, in one bursting phase.
- `crates/verter_session/tests/host_cpu_pool_lifetime.rs::scheduler_drop_does_not_invalidate_host_cpu_pool`
  — host constructs `Arc::new(HostCpuPool::new(N))`; passes
  `Arc::clone(&host_cpu_pool)` to `Scheduler::new`. After dropping
  the scheduler, `compile_many` still calls
  `host.host_cpu_pool.install(closure)` and the closure completes on
  the same pool instance. `Arc::strong_count(&host_cpu_pool) == 1`
  after the scheduler drops its clone.
- `crates/verter_session/tests/compile_many_no_pool_deadlock.rs::compile_many_with_full_scheduler_pool_does_not_deadlock`
  — saturates `SchedulerCpuPool` with `scheduler_cpu_threads`
  long-running cache-node tasks (each blocked on a manual latch).
  Invokes `compile_many` on the same host. The outer coordinator
  runs on `HostCpuPool`; `submit_required_parse_jobs` enqueues parse
  work onto the saturated pool. Releases the latches; `compile_many`
  returns within `MAX_TEST_TIMEOUT = std::time::Duration::from_secs(30)`
  (defined in `crates/verter_session/src/test_support/timeout.rs`).
- `crates/verter_session/tests/host_cpu_pool_isolation.rs::host_cpu_pool_workers_do_not_execute_scheduler_cpu_tasks`
  — instruments both pools with submit counters. A `compile_many`
  batch increments `HostCpuPool::install_count` only for the outer
  coordinator phase and `SchedulerCpuPool::submit_count` once per
  parse/cache-node task.

#### Owning-doc updates

- `CLAUDE.md` (Cache Architecture section) — append a brief reference
  to the host-owned CPU pool and 8 MiB stack guarantee.
- `.claude/skills/type-cache-architecture/SKILL.md` — note that
  `compile_many` is a transactional batch over
  `CompileOutputNode_*`.

#### Public API mirrors

- Rust crate: `compile_many(inputs, options: CompileBatchOptions)` —
  `cache_mode` already exposed via Block 5.
- NAPI: `compileMany(inputs, { cacheMode?, priority? })`.
- WASM: same shape.
- Protocol DTOs: `CompileBatchRequestDto.cache_mode` already mirrored
  in Block 5.
- TS generated types: regenerated alongside Block 5.
- JS wrappers: signatures updated.
- Compat: pass-through.

#### Blocks blocked by this block

- B12 (benchmark `compile_many` scenarios depend on the transactional
  shape).

### 7. Scheduler ↔ cache-runtime integration

> **Revised per architectural decision (codex): unified scheduler DAG +
> admission-time backpressure.** Block 7 no longer introduces a separate
> cache-node ordering substrate that coexists with the staged
> `JobIndex` / blocker path. There is ONE driver-owned `SchedulerDag`
> readiness authority for all work — file stages AND cache nodes — and
> backpressure lives at DAG admission, not at a submitter-side
> ready-queue push. See §7.0 below for the authoritative substrate
> design; the remaining §7 prose is the cache-node specialization of
> that one substrate.

#### 7.0 Unified scheduler DAG (single readiness authority)

**Decision (codex, authoritative).** Delete `JobIndex` and
`BlockerRegistry`. Model file stages (`Load → Parse → Analysis →
Artifact`) AND cache-runtime nodes in ONE driver-owned `SchedulerDag`
readiness substrate. The current staged path scans `JobIndex.entries`
for `len()` / `dequeue()` (substrate `queue.rs:180`) and scans blocker
state through `has_pending_blockers` (substrate `edges.rs:230`); a
separate cache DAG would preserve TWO ordering authorities, which
violates the project's single-substrate law. The end-state has exactly
one readiness mutator (the driver) and one node-identity dedup key
(`WorkNodeKey`).

**Generic substrate types** (these REPLACE the cache-only
`CacheNodeDag*` types — every `CacheNodeDag*` symbol named later in §7
is the cache-node specialization of the corresponding generic type
below, NOT a parallel authority):

```rust
// crates/verter_scheduler/src/node.rs (generic substrate)
pub enum WorkKind {
    Load { canonical: Arc<str> },
    Parse { canonical: Arc<str>, source: Arc<str>, file_kind: FileKind },
    Analysis { canonical: Arc<str> },
    Artifact { canonical: Arc<str>, profile_hash: u64 },
    CacheNode { cache_id: SchedulerCacheId, key_hash: u64 },
}

pub struct WorkNodeKey {
    pub canonical: Option<Arc<str>>,
    pub generation: u64,
    pub content_hash: Option<Hash16>,
    pub kind_key: WorkKindKey,
}

pub struct SchedulerDagNode {
    pub id: NodeId,
    pub key: WorkNodeKey,
    pub work: WorkKind,
    pub priority: Priority,
    pub request_context: Option<OpaqueRequestContext>,
    pub waiters: SmallVec<[CompletionSender<RequestResult>; 2]>,
}

pub struct SchedulerDagEdge {
    pub from: NodeId,
    pub to: NodeId,
    pub gate: EdgeGate,
}
```

**File progression becomes nodes.** `Load → Parse → Analysis →
Artifact(profile)`. `TargetStage::Source` is satisfied by `Parse`,
because parse publishes the `SourceSnapshot` / `IndexedReady`. File
stages are no longer ordered through `FileNode.pending_requests`; they
are `SchedulerDagNode`s with `WorkKind::Load` / `Parse` / `Analysis` /
`Artifact` and `SchedulerDagEdge`s between them, in the same
`SchedulerDag` that carries `WorkKind::CacheNode` nodes.

**Dynamic imports → driver-owned incremental DAG expansion.** `Parse`
completion is an expansion barrier. Before the `Parse → Analysis` edge
is released, the driver reads parsed/import facts, creates the required
dependency nodes, and adds edges such as `B.Parse → A.Analysis` or
`B.Analysis → A.Artifact(profile)` depending on the required fact. If an
upstream node already completed in the same generation, the new edge is
satisfied immediately; otherwise `remaining_upstream` is incremented
BEFORE downstream readiness is opened. The driver never releases a
downstream stage before the dynamically discovered upstream edges for
that stage have been added — this is the
`dynamic_import_edges_added_before_downstream_dispatch` guarantee.

**Generation invalidation.** A generation bump marks all nodes under
`(canonical, old_generation)` superseded through
`nodes_by_file_generation`. Ready lanes skip stale nodes by a generation
fence on dispatch; worker completions are ignored unless
`FileNode.generation() == node.key.generation`. There is no separate
`supersede_old_generations` `TaskKind`-scan — supersession is a
generation-keyed node sweep.

**Scan-free priority fairness (replaces priority aging).** Priority
aging is DELETED. Dispatch reads four driver-owned `VecDeque<NodeId>`
lanes (one per `Priority`) plus a small fixed deficit/credit policy.
Dispatch checks ONLY the four lanes, never the job set — there is no
linear scan of pending jobs on the dispatch path. `Critical` remains
first-class; `Background` / `Maintenance` receive bounded service
through the deficit/credit counters without any per-entry aging scan.
This is the `scheduler_dispatch_path_no_linear_job_scan` guarantee.

**Dedup identity is `WorkNodeKey`.** File-stage keys are `(canonical,
generation, stage, profile_hash?)`; cache keys are `(SchedulerCacheId,
typed_key_hash, snapshot_pin_id / view_epoch)`. Multiple request waiters
attach to the SAME node via `SchedulerDagNode.waiters` — same-stage
requests join one work node rather than spawning duplicate stage work.
This subsumes the substrate's old `pending_requests` dedupe rail for
file-stage ordering (the cache-node `DedupKey` inflight collapse that
the rest of §7 describes is the cache-node specialization of this same
join).

**STOP-gate (codex).** Block 7 MUST NOT land while `job_index` or
`BlockerRegistry` remain production ordering authorities. The guard
`scheduler_has_single_readiness_authority` fails the gate if either
type, or any `Mutex<JobIndex>` field / `has_pending_blockers` call,
survives on a non-test scheduler path.

**Decision-1 guards (registered in the §7 Discriminating tests and the
cross-block Verification):** `scheduler_has_single_readiness_authority`,
`scheduler_dispatch_path_no_linear_job_scan`,
`blocker_resolution_touches_only_out_edges`,
`dynamic_import_edges_added_before_downstream_dispatch`,
`stale_generation_nodes_never_dispatch_after_bump`,
`same_stage_requests_join_one_work_node`.

**Pool capacity is part of admission (H23).** Driver dispatch MUST NOT
call a worker-pool API that can block on a bounded queue. The old
`IoPool::execute` path uses a bounded channel and `sender.send(...)`;
when called by the driver during source dispatch it can stall readiness
processing behind disk or queue pressure. Block 7 replaces every
driver-facing worker submission with nonblocking, permit-backed APIs:
admission reserves the required CPU and I/O node budget before any
ready lane is seeded, and the driver calls only `try_submit_*` methods.
If a nonblocking pool submission unexpectedly fails at dispatch time,
the borrowed permit is returned to the `DagCapacityReservation`, the
node is parked in a driver-owned deferred lane, and the driver retries
when a completion or pool-health event makes progress possible. The
driver never waits on a channel send, condvar, or worker-pool install
call.

#### Context

`crates/verter_scheduler/src/scheduler.rs:273` exposes
`submit_request`; `:312` exposes `submit_batch` (a loop). There is no
dependency-DAG submission API and no in-flight dedupe before
scheduling for cache nodes. CPU and I/O work share submission pressure
today: source-parse jobs can land on the I/O pool (parse is CPU work),
and bounded I/O submission can block the scheduler driver. Block 7
formalises the diff per-file.

**Crate dependency invariant (H20, BLOCKING).** `verter_scheduler`
MUST NOT depend on `verter_session`. The dependency direction is
one-way: `verter_session → verter_scheduler`.
`crates/verter_scheduler/Cargo.toml` already omits `verter_session`
(verified in tree — current deps are `verter_audit`, `verter_span`,
plus framework deps `crossbeam-*`, `rayon`, `dashmap`, `parking_lot`,
`arc-swap`, `rustc-hash`). Any re-introduction is a cycle and fails
`crates/verter_scheduler/tests/no_session_dep.rs::scheduler_does_not_depend_on_verter_session`.

The cache-runtime in-flight `InflightTable` lives in
`verter_session::cache_runtime::singleflight`. The scheduler is
unaware of it. Cache-runtime callers in `verter_session` perform their
in-flight dedupe BEFORE constructing a scheduler submission. The
scheduler exposes a generic `DedupeHook` trait in
`crates/verter_scheduler/src/dedupe_hook.rs` that the calling crate
may pass into `submit_request` / `try_submit_dag` /
`submit_dag_blocking` so the scheduler can collapse joiners — but the
scheduler does not import or depend on the cache-runtime substrate.

#### Changes

**`Scheduler` surface diff:**

| Method | Today | Post-cutover |
|---|---|---|
| `submit_request(req)` | inbox + per-request `CompletionHandle` | unchanged signature; gains optional `&dyn DedupeHook` arg |
| `submit_batch(reqs)` | loop over `submit_request` | replaced by no-edge-DAG bridge over `try_submit_dag` |
| `try_submit_dag(dag) -> SubmissionResult<DagHandle>` | absent | NEW (DECISION 2 — typed admission; `Backpressure` without readiness mutation) |
| `submit_dag_blocking(dag) -> DagHandle` | absent | NEW (DECISION 2 — parks on `admission_budget_available` condvar) |
| `dedup_key_for(req) -> DedupKey` | absent | NEW (probe surface for callers) |
| `cpu_concurrency_semaphore(n) -> Arc<CpuConcurrencySemaphore>` | absent | NEW |
| `ready_queue_depth() -> usize` | absent | NEW (observability) |
| `register_resolved_deps` | unchanged | unchanged |

**Type ownership (single source of truth):**

The substrate already has two adjacent envelope concepts. To avoid
the collision the synthesis flagged:

- `driver::Submission` (`crates/verter_scheduler/src/driver.rs:13`) is
  the INBOX-level enum. Block 7 does NOT rename or shadow it, but it
  DOES change the variant set: the `BlockerResolved {…}` variant is
  DELETED (blocker resolution is now an `out_edge` decrement inside the
  driver-owned `SchedulerDag`, never an inbox message — see
  `blocker_resolution_touches_only_out_edges`), and a new
  `DagSubmitted { dag_id }` variant is ADDED (the only thing a submitter
  sends after it has acquired a typed admission budget — see §7.0 +
  DECISION 2). Post-cutover the enum is
  `Wake | NewRequest {…} | StageComplete {…} | DagSubmitted { dag_id }`.
- `job::CompletionSender<T: Clone>`
  (`crates/verter_scheduler/src/job.rs:104`) is the substrate's
  condvar-backed handle used by top-level `submit_request`. Block 7
  does NOT rename or shadow it.
- `node::CacheNodeCompletionSender` (NEW — defined below) is the
  cache-node-only completion channel built on
  `tokio::sync::oneshot`. Renamed from earlier drafts'
  `node::CompletionSender` to avoid name collision with
  `job::CompletionSender<T>` at the crate root after re-export.

Block 7's per-node envelope is `CacheNodeDagNode`. The plan does NOT
introduce a separate `ReadySubmission` struct that shadows the inbox
enum — the legacy `ReadySubmission` shape is retired. Instead, the
ready queue's element type is `Arc<CacheNodeDagNode>` — the driver
pops a ready `Arc<CacheNodeDagNode>` and dispatches it by deref'ing
into the `dispatch_cpu_task(&CacheNodeDagNode, ...)` signature.
`Arc` (rather than direct ownership) is required because the same
node must live on both the ready queue and `DagState.nodes`, and
`CacheNodeDagNode` is intentionally not `Clone` (its
`CacheNodeCompletionSender` wraps a single-use `oneshot::Sender`).
The top-level inbox continues to enqueue
`driver::Submission::NewRequest` for single-request callers
(`submit_request`); the DAG admission tail (`admit_dag`, reached via
`try_submit_dag` / `submit_dag_blocking`) wraps every incoming
`CacheNodeDagNode` in `Arc`, stores the `Arc<CacheNodeDagNode>` vector
on `DagState.nodes`, and sends `Submission::DagSubmitted { dag_id }`.
The DRIVER (the sole readiness mutator — DECISION 2) then seeds
`Arc::clone(&state.nodes[idx])` into the ready lanes for every
zero-upstream root and, thereafter, pushes each downstream node once
its upstream gates fire. The submitter never pushes into the ready
queue.

**`CacheNodeDag` envelope (verbatim — single source of truth).**

Every per-node field lives in exactly one place:
`CacheNodeDagNode.task_kind` is THE task discriminator the dispatcher
matches on. There is no separate `CacheNodeDagNode.task_kind` AND
`KeyedJob.task` — `KeyedJob` is the submission identity (canonical +
stage + content_hash + priority + generation), and the `task_kind`
lives on `CacheNodeDagNode` only.

```rust
// crates/verter_scheduler/src/job.rs (additions)
//
// `DedupKey` is the dedup identity for both scheduler-side
// `pending_requests` and consumer-side `DedupeHook::probe`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DedupKey {
    pub canonical: std::sync::Arc<str>,
    pub stage: crate::stage::TargetStage,
    pub content_hash: u64,
}

#[derive(Clone, Debug)]
pub struct KeyedJob {
    pub dedup_key: DedupKey,
    pub stage: crate::stage::TargetStage,
    pub priority: crate::stage::Priority,
    /// World generation under which this job was enqueued. Dispatch
    /// reads `node.keyed_job.generation` directly; `CacheNodeDagNode`
    /// has no `generation()` accessor.
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CacheNodeId(pub u32);

pub struct DagHandle { /* opaque — wraps Arc<DagState> */ }

pub struct DagState {
    /// `Arc<CacheNodeDagNode>` — the ready queue carries the same
    /// `Arc<CacheNodeDagNode>` element, and the driver dispatches
    /// every popped node by deref'ing the Arc into the
    /// `dispatch_cpu_task(node: &CacheNodeDagNode, ...)` signature.
    ///
    /// `CacheNodeCompletionSender` wraps its inner
    /// `tokio::sync::oneshot::Sender<CacheNodeOutcome>` in
    /// `parking_lot::Mutex<Option<...>>` precisely so the worker can
    /// `take()` the sender out of a shared-Arc borrow when it fires
    /// the completion (`oneshot::Sender::send` consumes `self`). The
    /// node is therefore never `Clone` — Arc-sharing is the only
    /// admissible way to publish the same node onto both the ready
    /// queue and the per-DAG state.
    pub nodes: Vec<std::sync::Arc<crate::node::CacheNodeDagNode>>,
    pub edges: Vec<crate::node::CacheNodeDagEdge>,
    pub readiness: parking_lot::Mutex<DagReadiness>,
    pub cancellation: crate::cancellation::CancellationToken,
}

pub struct DagReadiness {
    /// Per-node count of remaining upstream gates. A node enters the
    /// ready queue when its count drops to zero.
    pub remaining_upstream: smallvec::SmallVec<[u32; 8]>,
    /// Per-node admission disposition observed on upstream completion.
    pub upstream_dispositions: smallvec::SmallVec<[Option<crate::node::AdmissionDisposition>; 8]>,
    /// Completed-count for the `DagCompletionAggregator` to mark the
    /// DagHandle as resolved.
    pub completed: u32,
}

#[cfg(feature = "test-support")]
impl KeyedJob {
    pub fn stub() -> Self {
        Self {
            dedup_key: DedupKey::new_for_test(),
            stage: crate::stage::TargetStage::Source,
            priority: crate::stage::Priority::Background,
            generation: 0,
        }
    }
}

#[cfg(feature = "test-support")]
impl DedupKey {
    pub fn new_for_test() -> Self {
        Self {
            canonical: std::sync::Arc::from(""),
            stage: crate::stage::TargetStage::Source,
            content_hash: 0,
        }
    }
}

/// Stable 64-bit content fingerprint used as the `DedupKey.content_hash`
/// discriminator.
///
/// Substrate `Request.source` (`scheduler.rs:88`) is
/// `Option<Arc<str>>`; the dedup key needs a fixed-width hash that
/// distinguishes content variants on the same `(canonical, stage)`
/// without ballooning the inflight-map memory footprint. The
/// definition here is deliberately small and self-contained — it is
/// NOT a re-export of an existing hasher because the scheduler crate
/// must not depend on `verter_session` to compute its own dedup
/// identity (H20: scheduler is leaf relative to session).
///
/// Implementation contract:
///
/// - deterministic across processes for a given Verter build (the
///   `parser_version` envelope keys carry the hasher-version
///   discriminator — content-hash collisions across hasher upgrades
///   are caught by the parser-version axis on `FileArtifactKey`);
/// - byte-stable (no float ordering, no `RandomState`);
/// - cheap on the common case (small source files dominate the
///   submission frequency curve);
/// - returns `0` only when the caller explicitly omits source
///   (`request.source.is_none()`); `stable_content_hash("")` is a
///   nonzero canary distinct from the absence case.
///
/// The body uses `blake3::hash` truncated to the first 8 bytes
/// (the workspace already depends on `blake3` for fact-signature
/// composition; no new dependency). The XOR-with-canary disambiguates
/// the empty-string input from the `request.source.is_none()` branch
/// without paying for a separate Option discriminator inside
/// `DedupKey`.
pub fn stable_content_hash(content: &str) -> u64 {
    const EMPTY_CANARY: u64 = 0xD15E_A5ED_DEAD_BEEF;
    let digest = blake3::hash(content.as_bytes());
    let bytes = digest.as_bytes();
    let mut out = [0u8; 8];
    out.copy_from_slice(&bytes[..8]);
    let raw = u64::from_le_bytes(out);
    if content.is_empty() { raw ^ EMPTY_CANARY } else { raw }
}
```

```rust
// crates/verter_scheduler/src/node.rs (additions — single per-node envelope)
//
// Per-node executable envelope. Every field is required: no node
// enters the ready queue without all nine fields populated. The
// driver does NOT enrich nodes after submission.
pub struct CacheNodeDagNode {
    pub id: crate::job::CacheNodeId,
    pub keyed_job: crate::job::KeyedJob,
    /// THE task-kind discriminator. Lives on the node only. No
    /// duplicate field on `KeyedJob`.
    pub task_kind: crate::stage::TaskKind,
    pub dedup_key: crate::job::DedupKey,
    pub priority: crate::stage::Priority,
    pub cancellation_token: crate::cancellation::CancellationToken,
    /// SEMAPHORE HANDLE clone (NOT a pre-acquired permit). Worker
    /// dispatch acquires a fresh per-task permit from this handle
    /// immediately before executing the task; the permit drops on
    /// task completion (RAII).
    pub cpu_concurrency_semaphore: Option<std::sync::Arc<crate::cpu_concurrency::CpuConcurrencySemaphore>>,
    /// Scheduler-local opaque wrapper (NOT a session-crate type;
    /// H20). Session callers wrap their concrete `Arc<RequestContext>`
    /// in `OpaqueRequestContext(arc as Arc<dyn RequestContextLike>)`.
    pub request_context: std::sync::Arc<crate::request_context::OpaqueRequestContext>,
    /// Cache-node-only completion channel. Distinct from
    /// `crate::job::CompletionSender<T>` (which is the condvar-backed
    /// sender for top-level `submit_request`).
    pub completion: CacheNodeCompletionSender,
}

pub struct CacheNodeDagEdge {
    pub from: crate::job::CacheNodeId,
    pub to: crate::job::CacheNodeId,
    pub gate: EdgeGate,
}

pub enum EdgeGate {
    /// Downstream admits as soon as upstream completes (any outcome).
    Sequential,
    /// Downstream admits only if upstream returned
    /// `CacheAdmission::Cacheable` (admission disposition is
    /// `AdmissionDisposition::Cacheable`).
    ConditionalOnSuccess,
    /// Downstream admits only if upstream admission was non-failed
    /// (`Cacheable` or `ReturnOnly`); `Failed` short-circuits the
    /// downstream and propagates failure.
    ConditionalOnAdmission,
}

pub struct CacheNodeDag {
    pub nodes: Vec<CacheNodeDagNode>,
    pub edges: Vec<CacheNodeDagEdge>,
    pub completion_aggregator: std::sync::Arc<DagCompletionAggregator>,
}

/// Collects per-node `CacheNodeOutcome`s and, on the DAG's terminal
/// transition (completion / cancellation / shutdown), releases the
/// admitted node/edge/CPU/I/O budget back to
/// `Scheduler.admission_budget` through the single
/// `DagCapacityReservation` and calls
/// `admission_budget_available.notify_all()` (DECISION 2) so a parked
/// `submit_dag_blocking` can proceed. Holds the admitted
/// `(nodes, edges, cpu_work, io_work)` counts it must return.
pub struct DagCompletionAggregator { /* internal — wraps Arc<Mutex<Vec<CacheNodeOutcome>>> + admitted budget counts */ }

/// Cache-node-only completion channel. Wraps a
/// `tokio::sync::oneshot::Sender<CacheNodeOutcome>` in
/// `Mutex<Option<...>>` so the worker dispatch site can `take()` the
/// inner sender out of a shared `&CacheNodeDagNode` borrow.
/// `oneshot::Sender::send` consumes `self`, so a bare-Sender field
/// on `CacheNodeDagNode` would not compile against the
/// `dispatch_cpu_task(node: &CacheNodeDagNode, ...)` signature.
///
/// RENAMED from earlier drafts' `node::CompletionSender` to avoid
/// collision with the substrate's existing `job::CompletionSender<T>`
/// at the crate root after re-export.
pub struct CacheNodeCompletionSender {
    inner: parking_lot::Mutex<Option<tokio::sync::oneshot::Sender<CacheNodeOutcome>>>,
}

impl CacheNodeCompletionSender {
    pub fn new(sender: tokio::sync::oneshot::Sender<CacheNodeOutcome>) -> Self {
        Self { inner: parking_lot::Mutex::new(Some(sender)) }
    }
    pub fn send(&self, outcome: CacheNodeOutcome) -> bool {
        if let Some(sender) = self.inner.lock().take() {
            sender.send(outcome).is_ok()
        } else {
            false
        }
    }
}

/// Unified completion type for every `CacheNodeDagNode`, regardless
/// of the `TaskKind` that produced it.
pub enum CacheNodeOutcome {
    Source(Result<SourceSnapshot, crate::executor::StageError>),
    Analysis(Result<AnalysisSnapshot, crate::executor::StageError>),
    Artifact(Result<ArtifactSnapshot, crate::executor::StageError>),
    /// `TaskKind::CacheNode` completed; the host produces the
    /// concrete cache value.
    CacheNode(Result<CacheNodeValue, crate::executor::StageError>),
    /// Default-stub result used by the trait's default
    /// `execute_cache_node` body.
    Stub,
}

impl CacheNodeOutcome {
    pub fn from_source(result: Result<SourceSnapshot, crate::executor::StageError>) -> Self { Self::Source(result) }
    pub fn from_analysis(result: Result<AnalysisSnapshot, crate::executor::StageError>) -> Self { Self::Analysis(result) }
    pub fn from_artifact(result: Result<ArtifactSnapshot, crate::executor::StageError>) -> Self { Self::Artifact(result) }
    pub fn stub() -> Self { Self::Stub }

    /// Disposition the upstream gate observes on edge resolution.
    pub fn disposition(&self) -> AdmissionDisposition {
        match self {
            Self::Source(Ok(_)) | Self::Analysis(Ok(_)) | Self::Artifact(Ok(_)) => AdmissionDisposition::Cacheable,
            Self::CacheNode(Ok(v)) => v.admission,
            Self::Source(Err(_)) | Self::Analysis(Err(_)) | Self::Artifact(Err(_)) | Self::CacheNode(Err(_)) => AdmissionDisposition::Failed,
            Self::Stub => AdmissionDisposition::Cacheable,
        }
    }
}

pub struct CacheNodeValue {
    pub key_hash: u64,
    pub admission: AdmissionDisposition,
    pub payload: std::sync::Arc<dyn std::any::Any + Send + Sync>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmissionDisposition {
    Cacheable,
    ReturnOnly,
    Failed,
}

#[cfg(feature = "test-support")]
impl CacheNodeDagNode {
    pub fn stub() -> Self {
        let (sender, _receiver) = tokio::sync::oneshot::channel();
        Self {
            id: crate::job::CacheNodeId(0),
            keyed_job: crate::job::KeyedJob::stub(),
            task_kind: crate::stage::TaskKind::CacheNode {
                cache_id: crate::cache_id::SchedulerCacheId::Test,
                key_hash: 0_u64,
            },
            dedup_key: crate::job::DedupKey::new_for_test(),
            priority: crate::stage::Priority::Background,
            cancellation_token: crate::cancellation::CancellationToken::new(),
            cpu_concurrency_semaphore: None,
            request_context: std::sync::Arc::new(
                crate::request_context::OpaqueRequestContext::test_stub(),
            ),
            completion: CacheNodeCompletionSender::new(sender),
        }
    }
}
```

**`TaskKind` rewrite** (replaces the substrate
`stage.rs:32` enum verbatim; `TargetStage` derive bumped to include
`Hash` so `DedupKey` can derive `Hash`):

```rust
// crates/verter_scheduler/src/stage.rs (rewrite)
use crate::node::{AnalysisSnapshot, FileKind, SourceSnapshot};
use std::sync::Arc;

/// Internal work discriminant.
///
/// The legacy `Source` variant (which combined load + parse on the
/// I/O pool) is RETIRED. The source loader synthesises a Load → Parse
/// DAG edge. Block 7 carries `canonical` and the relevant snapshots
/// on the payload-bearing variants because substrate `SourceSnapshot`
/// has no `canonical_id()` accessor.
///
/// `TaskKind` is no longer `Copy` — payload-bearing variants carry
/// `Arc<str>` / `Arc<SourceSnapshot>` etc. Every existing
/// `TaskKind`-Copy call site in the substrate
/// (e.g. `supersede_old_generations` at `scheduler.rs:388`) becomes an
/// `Arc` clone — the behavioral consequence is the additional
/// `Arc::clone` cost at supersession; the discriminating test
/// `task_kind_clone_is_cheap_arc_clone` (below) pins the clone cost
/// at <100ns p99 against an inline literal so a regression switching
/// to deep cloning is caught.
///
/// `Eq` and `Hash` are NOT derived — the substrate's `SourceSnapshot`
/// (`crates/verter_scheduler/src/node.rs:41-91`) and
/// `AnalysisSnapshot` (`:93-128`) do NOT implement those traits (they
/// carry an `Arc<dyn SnapshotData>` field plus per-snapshot
/// `whole_hash` / `semantic_hash` / `generation` values, and the
/// substrate intentionally never derived structural equality on a
/// snapshot). `TaskKind` is therefore `Clone + Debug + PartialEq`
/// only. The behavioral consequences:
///
/// - `TaskKind` cannot be used directly as a `HashMap` / `DashMap` /
///   `HashSet` key. The scheduler's existing in-flight dedupe rail
///   (`pending_requests: DashMap<DedupKey, _>`) already keys on
///   `DedupKey` — which IS `Hash + Eq + Clone` — so production code
///   paths never key on `TaskKind`. The new code in this plan that
///   needs a `Hash + Eq` discriminator for in-flight collapsing uses
///   `DedupKey`, NOT `TaskKind`.
/// - The two `PartialEq`-only call sites are `TargetStage::is_satisfied_by`
///   (substrate `stage.rs`) and `supersede_old_generations`
///   (substrate `scheduler.rs:388`). `PartialEq` is sufficient for
///   both — neither requires `Hash` or full `Eq`.
/// - A few existing scheduler tests construct `HashSet<TaskKind>` or
///   `HashMap<TaskKind, _>` for ad-hoc assertions. Each migrates to
///   `HashMap<DedupKey, _>` (the canonical discriminator) or
///   `Vec<TaskKind>` with linear scan in the same Block 7 commit.
#[derive(Clone, Debug, PartialEq)]
pub enum TaskKind {
    /// Pure I/O — read bytes off disk. Routed to `SchedulerIoPool`.
    Load { canonical: Arc<str> },
    /// Pure CPU — tokenize/parse. Routed to `SchedulerCpuPool`.
    Parse {
        canonical: Arc<str>,
        source: Arc<str>,
        file_kind: FileKind,
    },
    /// Pure CPU — static analysis. Carries the upstream source
    /// snapshot.
    Analysis {
        canonical: Arc<str>,
        source_snapshot: Arc<SourceSnapshot>,
    },
    /// Pure CPU — compile for a specific profile.
    Artifact {
        canonical: Arc<str>,
        source_snapshot: Arc<SourceSnapshot>,
        analysis_snapshot: Arc<AnalysisSnapshot>,
        profile_hash: u64,
    },
    /// Pure CPU — cache-runtime cache-node dispatch.
    CacheNode {
        cache_id: crate::cache_id::SchedulerCacheId,
        key_hash: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Priority {
    Critical = 0,
    Interactive = 1,
    Background = 2,
    Maintenance = 3,
}

/// `TargetStage` derive bumped to include `Hash` so `DedupKey` can
/// derive `Hash` (Rust's derive is field-wise).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TargetStage {
    Source,
    Analysis,
    Artifact { profile_hash: u64 },
}

impl TargetStage {
    pub fn required_task_kind(&self, canonical: Arc<str>) -> TaskKind {
        match self {
            // `TargetStage::Source` is satisfied by the terminal
            // `Parse` of the synthesised Load → Parse DAG. Callers
            // populate `source` / `file_kind` from the inbox-side
            // submission shape.
            TargetStage::Source => unreachable!("TargetStage::Source satisfaction is handled by the Load → Parse DAG edge"),
            TargetStage::Analysis => unreachable!("TargetStage::Analysis requires source_snapshot — synthesised by driver after Parse"),
            TargetStage::Artifact { profile_hash } => unreachable!("TargetStage::Artifact requires source+analysis snapshots — synthesised by driver"),
        }
    }

    pub fn is_satisfied_by(&self, completed: &TaskKind) -> bool {
        match (self, completed) {
            (TargetStage::Source, TaskKind::Parse { .. }) => true,
            (TargetStage::Source, TaskKind::Analysis { .. }) => true,
            (TargetStage::Source, TaskKind::Artifact { .. }) => true,
            (TargetStage::Analysis, TaskKind::Analysis { .. }) => true,
            (TargetStage::Analysis, TaskKind::Artifact { .. }) => true,
            (TargetStage::Artifact { profile_hash: a }, TaskKind::Artifact { profile_hash: b, .. }) => a == b,
            _ => false,
        }
    }
}
```

The `SchedulerJobKind` enum at `stage.rs:19` is **retained**
unchanged — it discriminates non-staged component-meta batch jobs
(the existing `MetaSession::get_component_meta_batch` fan-out path).
The retained pre-existing tests in `stage.rs` continue to pass.
Block 7's new `TaskKind::CacheNode` variant is the cache-runtime
dispatch surface; it lives ALONGSIDE `SchedulerJobKind`, not
replacing it.

**Bounded ready queue:**

```rust
// crates/verter_scheduler/src/queue.rs (additions)
pub const MAX_READY_QUEUE_DEPTH: usize = 64;

/// Typed submission result. Backpressure is reported at DAG
/// ADMISSION (before any readiness mutation), not at a
/// ready-queue push. The `T` is the success payload —
/// `DagHandle` for `try_submit_dag`, `()` for single-request
/// admission probes.
pub enum SubmissionResult<T> {
    Admitted(T),
    /// Admission budget exhausted; caller chooses to block
    /// (`submit_dag_blocking`) or back off. Carries the typed
    /// capacity accounting so the caller can log / retry intelligently.
    Backpressure {
        requested: DagCapacityDemand,
        available: DagCapacityDemand,
    },
}

pub struct DagCapacityDemand {
    pub nodes: u32,
    pub edges: u32,
    pub cpu_work: u32,
    pub io_work: u32,
}
```

**Backpressure lives at DAG admission, not at a ready-queue push
(DECISION 2, codex).** The driver is the ONLY readiness mutator. A
submitter never holds `DagState.readiness.lock()` and never spins on a
ready-queue `push`. Admission is gated by a typed node/edge/CPU/I/O budget:

> SUPERSEDED: `DagAdmissionBudget` is not built; `DagCapacityBudget` / `DagCapacityReservation` is the single ledger (unified plan §2.2). `queue.rs` itself was deleted by §7 — the budget lives on the driver-owned `SchedulerDag`, not a `queue.rs` addition.

```rust
// crates/verter_scheduler/src/queue.rs (additions)
pub struct DagAdmissionBudget {
    pub max_nodes: u32,
    pub max_edges: u32,
    pub max_cpu_work: u32,
    pub max_io_work: u32,
    pub in_flight_nodes: u32,
    pub in_flight_edges: u32,
    pub in_flight_cpu_work: u32,
    pub in_flight_io_work: u32,
}

pub struct DagCapacityReservation {
    pub nodes: u32,
    pub edges: u32,
    pub cpu_work: u32,
    pub io_work: u32,
    cpu_permits: smallvec::SmallVec<[PoolPermit; 8]>,
    io_permits: smallvec::SmallVec<[PoolPermit; 8]>,
}
```

`try_submit_dag` checks `DagAdmissionBudget` under a small mutex; if
capacity is unavailable it returns
`Backpressure { requested: DagCapacityDemand, available: DagCapacityDemand }`
WITHOUT touching readiness.
`submit_dag_blocking` waits on a `parking_lot::Condvar` attached to that
budget mutex until capacity frees. After admission succeeds, the
submitter sends `Submission::DagSubmitted { dag_id }` to the inbox; the
driver owns `DagReadiness` and seeds the ready priority lanes
internally. `DagCompletionAggregator` releases the admitted node/edge
and CPU/I/O work budget through the single `DagCapacityReservation` on
completion / cancellation / shutdown and notifies the
`admission_budget_available` condvar. Worker pools do not own a second
capacity counter; driver dispatch borrows a `PoolPermit` from the
reservation and returns it through the same terminal path if dispatch
fails before the work starts.

The bounded `crossbeam_queue::ArrayQueue<Arc<CacheNodeDagNode>>` is the
DRIVER-INTERNAL ready queue — `Arc<CacheNodeDagNode>` because
`CacheNodeDagNode` is intentionally not `Clone` (its
`CacheNodeCompletionSender` wraps a single-use `oneshot::Sender` in
`Mutex<Option<...>>` so the worker can `take()` it out of a shared
borrow). The driver pops an `Arc<CacheNodeDagNode>` and dispatches it by
deref'ing into the `dispatch_cpu_task(&CacheNodeDagNode, ...)` signature.
Because admission already bounded the in-flight node count, the driver's
internal seeding cannot overflow the ready queue — there is no
submitter-visible `Err(Arc<CacheNodeDagNode>)` overflow path and no
`yield_now()` spin anywhere on a submission path. The existing
`driver::Submission` enum at `driver.rs:13` remains the inbox
discriminator and is a different type.

**Per-call CPU concurrency semaphore:**

```rust
// crates/verter_scheduler/src/cpu_concurrency.rs (NEW)
//
// Counting semaphore backed by `parking_lot::Mutex<usize>` +
// `parking_lot::Condvar`. These are the only synchronisation
// primitives `parking_lot 0.12` exports — `parking_lot::Semaphore`
// does NOT exist in that version.
use std::sync::Arc;

pub struct CpuConcurrencySemaphore {
    available: parking_lot::Mutex<usize>,
    condvar: parking_lot::Condvar,
}

impl CpuConcurrencySemaphore {
    pub fn new(capacity: usize) -> Self {
        Self {
            available: parking_lot::Mutex::new(capacity),
            condvar: parking_lot::Condvar::new(),
        }
    }

    /// Block until a permit is available, decrement the counter,
    /// return the RAII guard. The guard increments the counter and
    /// notifies one waiter on `Drop`.
    pub fn acquire(self: &Arc<Self>) -> CpuConcurrencyPermit {
        let mut available = self.available.lock();
        while *available == 0 {
            self.condvar.wait(&mut available);
        }
        *available -= 1;
        CpuConcurrencyPermit { semaphore: Arc::clone(self) }
    }
}

/// RAII permit handed back to the semaphore on drop.
///
/// This type is NOT propagated through DAG submissions. The DAG
/// carrier is `Arc<CpuConcurrencySemaphore>` (the HANDLE); the
/// per-task RAII permit is acquired by the worker dispatch site
/// IMMEDIATELY BEFORE the task body runs.
pub struct CpuConcurrencyPermit {
    semaphore: Arc<CpuConcurrencySemaphore>,
}

impl Drop for CpuConcurrencyPermit {
    fn drop(&mut self) {
        let mut available = self.semaphore.available.lock();
        *available += 1;
        self.semaphore.condvar.notify_one();
    }
}
```

`Scheduler::cpu_concurrency_semaphore(n)` returns
`Arc::new(CpuConcurrencySemaphore::new(n))`. Callers clone the Arc
onto every DAG node's `cpu_concurrency_semaphore` field; per-task
permits are acquired by the worker, not by the caller.

**Scheduler-local cancellation:**

```rust
// crates/verter_scheduler/src/cancellation.rs (NEW)
//
// Cooperative cancellation flag. Cheap, clone-able, lock-free. All
// clones share the same backing flag.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Default)]
pub struct CancellationToken {
    inner: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self { Self::default() }
    pub fn cancel(&self) { self.inner.store(true, Ordering::Release); }
    pub fn is_cancelled(&self) -> bool { self.inner.load(Ordering::Acquire) }
}
```

**Scheduler-local cache identity:**

```rust
// crates/verter_scheduler/src/cache_id.rs (NEW)
//
// Scheduler-local cache identity. Distinct namespace from
// `verter_session::capture_token::CacheId` (which is session-side);
// the rename to `SchedulerCacheId` prevents silent shadowing.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SchedulerCacheId {
    ResolvedImportFacts,
    ComponentMetaResult,
    MaterializeStructure,
    RefCycleResult,
    MemberShape,
    AugmentationTarget,
    PersistentPureArtifact,
    #[cfg(feature = "test-support")]
    Test,
}
```

`serde` is added to `crates/verter_scheduler/Cargo.toml` via
`serde = { workspace = true, features = ["derive"] }`.

**`SchedulerCpuPool` (the second of the two pools):**

```rust
// crates/verter_scheduler/src/pool.rs (replacement pool surface)
pub struct SchedulerCpuPool {
    pool: rayon::ThreadPool,
}

impl SchedulerCpuPool {
    pub fn new(num_threads: usize) -> Self {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .stack_size(8 * 1024 * 1024)
            .build()
            .expect("SchedulerCpuPool::new failed to build rayon ThreadPool");
        Self { pool }
    }

    /// Driver-safe submission. Never blocks on worker availability.
    /// The queued closure owns the admission permit until completion.
    pub fn try_submit(
        &self,
        permit: PoolPermit,
        f: impl FnOnce() + Send + 'static,
    ) -> Result<(), PoolSubmissionFailed> {
        self.pool.spawn_fifo(move || {
            let _permit = permit;
            f();
        });
        Ok(())
    }

    /// Non-driver helper for call sites that intentionally run work
    /// inline on the scheduler pool. Static guards reject this from
    /// driver dispatch paths.
    pub fn install_non_driver<R: Send>(&self, f: impl FnOnce() -> R + Send) -> R {
        self.pool.install(f)
    }
}

pub struct SchedulerIoPool {
    sender: crossbeam_channel::Sender<IoWork>,
}

impl SchedulerIoPool {
    pub fn new(num_threads: usize, queue_bound: usize) -> Self { /* ... */ }

    /// Driver-safe submission. Never blocks. The queued work owns the
    /// admission permit until completion/drop.
    pub fn try_submit(
        &self,
        permit: PoolPermit,
        work: IoWork,
    ) -> Result<(), PoolSubmissionFailed> {
        self.sender
            .try_send(work.with_permit(permit))
            .map_err(|err| match err {
                crossbeam_channel::TrySendError::Full(work) => {
                    PoolSubmissionFailed::no_capacity(work.into_permit())
                }
                crossbeam_channel::TrySendError::Disconnected(work) => {
                    PoolSubmissionFailed::shutdown(work.into_permit())
                }
            })
    }
}

/// Capability minted only by `DagCapacityReservation`.
pub struct PoolPermit { _private: () }

pub struct PoolSubmissionFailed {
    pub kind: PoolSubmitError,
    pub permit: PoolPermit,
}

pub enum PoolSubmitError {
    NoCapacity,
    Shutdown,
}
```

The legacy `cpu_pool: rayon::ThreadPool` field on `Scheduler` (at
`scheduler.rs:135`) is replaced with `scheduler_cpu_pool: Arc<SchedulerCpuPool>`.
Every internal call site that previously installed work on
`self.cpu_pool` now calls `self.scheduler_cpu_pool.try_submit(...)` from
driver dispatch or `install_non_driver(...)` from explicitly non-driver
helpers. The
existing blocking `IoPool::execute` surface at `pool.rs:17` is deleted
from every driver path and replaced with `scheduler_io_pool:
Arc<SchedulerIoPool>`. Any helper that keeps a blocking I/O submission
method is marked test-only or non-driver-only; the static guard
`driver_never_blocks_on_io_pool_send` rejects `send(...)`, blocking
`recv(...)`, `Condvar::wait`, or `install(...)` calls in
`driver.rs`, `scheduler.rs` dispatch tails, and any `dispatch_*`
function reachable from the driver.

**`StageExecutor` trait extension:**

```rust
// crates/verter_scheduler/src/executor.rs (additions)
//
// `StageExecutor` gains a fifth dispatch method `execute_cache_node`.
// The four existing methods (`execute_source`, `execute_analysis`,
// `extract_deps`, `execute_artifact`) are preserved verbatim with
// their existing default bodies.
//
// `as_any` is added to support the test-support
// `Scheduler::last_dispatched_task` downcast (see test-support
// section below). It is OBJECT-SAFE: no `where Self: Sized` bound,
// no default body. Every concrete impl provides a one-line body
// returning `self`.
pub trait StageExecutor: Send + Sync + 'static {
    fn execute_source(
        &self,
        _canonical_id: &str,
        _file_kind: crate::node::FileKind,
        content: Arc<str>,
        generation: u64,
    ) -> Result<crate::node::SourceSnapshot, StageError> {
        Ok(crate::node::SourceSnapshot::new_empty(content, generation))
    }

    fn execute_analysis(
        &self,
        _canonical_id: &str,
        _source: &crate::node::SourceSnapshot,
        generation: u64,
    ) -> Result<crate::node::AnalysisSnapshot, StageError> {
        Ok(crate::node::AnalysisSnapshot::new_empty(generation))
    }

    fn extract_deps(&self, _canonical_id: &str, _source: &crate::node::SourceSnapshot) -> ExtractedDeps {
        ExtractedDeps::default()
    }

    fn execute_artifact(
        &self,
        _canonical_id: &str,
        _source: &crate::node::SourceSnapshot,
        _analysis: &crate::node::AnalysisSnapshot,
        profile_hash: u64,
        generation: u64,
    ) -> Result<crate::node::ArtifactSnapshot, StageError> {
        Ok(crate::node::ArtifactSnapshot {
            generation,
            profile_hash,
            data: Arc::new(crate::node::EmptyData),
        })
    }

    /// Cache-node task dispatch. Returns `CacheNodeOutcome` directly
    /// (NOT `Result`-wrapped); errors live inside
    /// `CacheNodeOutcome::CacheNode(Err(_))`. Default returns
    /// `CacheNodeOutcome::stub()`; the host overrides.
    fn execute_cache_node(
        &self,
        _node: &crate::node::CacheNodeDagNode,
        _ctx: &CacheNodeDispatchCtx<'_>,
    ) -> crate::node::CacheNodeOutcome {
        crate::node::CacheNodeOutcome::stub()
    }

    /// Reflective downcast hook for test-support fixture helpers.
    /// Object-safe: NO `where Self: Sized`; NO default body. Every
    /// concrete impl provides its own body.
    fn as_any(&self) -> &dyn std::any::Any;
}

impl StageExecutor for DefaultExecutor {
    fn as_any(&self) -> &dyn std::any::Any { self }
}

/// Cache-node execution context. Materialised by the worker dispatch
/// site BEFORE invoking `execute_cache_node`. Pure non-owning borrows.
pub struct CacheNodeDispatchCtx<'a> {
    pub dedup_key: &'a crate::job::DedupKey,
    pub generation: u64,
    pub observer: Option<&'a dyn verter_audit::AuditObserver>,
    pub cancellation: &'a crate::cancellation::CancellationToken,
}

#[cfg(feature = "test-support")]
impl<'a> CacheNodeDispatchCtx<'a> {
    pub fn stub_with(
        dedup_key: &'a crate::job::DedupKey,
        cancellation: &'a crate::cancellation::CancellationToken,
    ) -> Self {
        Self { dedup_key, generation: 0, observer: None, cancellation }
    }
}
```

Every `impl StageExecutor for X` in the workspace gains the
mandatory `fn as_any(&self) -> &dyn std::any::Any { self }` body in
the same commit. Verifiable by
`grep -rE "impl StageExecutor for" crates/` — the workspace inventory
at plan-land time is:

- `crates/verter_scheduler/src/executor.rs` — `impl StageExecutor for DefaultExecutor`.
- `crates/verter_session/src/host_executor.rs:143` — `impl StageExecutor for HostStageExecutor`.
- `crates/verter_scheduler/src/scheduler.rs` — `impl StageExecutor for BlockingExecutor` (test-only).
- `crates/verter_scheduler/src/scheduler.rs` — `impl StageExecutor for ProbeExecutor` (test-only).
- `crates/verter_scheduler/src/scheduler.rs` — `impl StageExecutor for ParentAndDepProbe` (test-only).
- `crates/verter_scheduler/src/scheduler.rs` — `impl StageExecutor for PerFileProbe` (test-only).
- `crates/verter_session/tests/scheduler_audit_queue_dwell_under_load.rs` — `impl StageExecutor for SlowExecutor` (test-only).
- `crates/verter_session/tests/scheduler_audit_attributes_worker.rs` — `impl StageExecutor for PassthroughExecutor` (test-only).
- `crates/verter_session/tests/scheduler_worker_tls_propagation.rs` — `impl StageExecutor for SessionProbeExecutor` (test-only).
- Test-support `LastDispatchedTaskRecorder` (introduced below).

Without these per-file diffs, `cargo build --workspace --tests`
fails E0046 after the trait change lands. The architecture guard
`every_stage_executor_impl_provides_as_any` (Block 7 discriminating
tests) walks the workspace via `grep -rE "impl StageExecutor for"
crates/` and asserts each impl block contains a `fn as_any` method.

**Dispatch site:**

```rust
// crates/verter_scheduler/src/scheduler.rs (additions)
//
// `pub fn` at the crate root so `verter_scheduler::dispatch_cpu_task`
// resolves from the discriminating signature-witness test. The
// function lives here and is re-exported via `pub use scheduler::dispatch_cpu_task;`
// in lib.rs.
pub fn dispatch_cpu_task(
    node: &crate::node::CacheNodeDagNode,
    executor: &dyn crate::executor::StageExecutor,
    observer: Option<&dyn verter_audit::AuditObserver>,
    cancellation: &crate::cancellation::CancellationToken,
) {
    // Per-task permit acquire. Blocks if the semaphore counter is
    // zero. The acquire is per-task, not per-DAG: cloning the
    // `Arc<CpuConcurrencySemaphore>` across N nodes does NOT
    // pre-acquire N permits; only this `acquire()` call consumes a
    // permit.
    let _permit_guard = node.cpu_concurrency_semaphore
        .as_ref()
        .map(|sem| sem.acquire());

    // Materialise the dispatch context BEFORE the match. Pure
    // non-owning references.
    let dispatch_ctx = crate::executor::CacheNodeDispatchCtx {
        dedup_key: &node.dedup_key,
        generation: node.keyed_job.generation,
        observer,
        cancellation,
    };

    match &node.task_kind {
        crate::stage::TaskKind::CacheNode { .. } => {
            // `execute_cache_node` returns `CacheNodeOutcome` directly
            // (no outer `Result` wrapper).
            let outcome = executor.execute_cache_node(node, &dispatch_ctx);
            let _ = node.completion.send(outcome);
        }
        crate::stage::TaskKind::Parse { canonical, source, file_kind } => {
            let result = executor.execute_source(
                canonical.as_ref(),
                *file_kind,
                std::sync::Arc::clone(source),
                node.keyed_job.generation,
            );
            let _ = node.completion.send(crate::node::CacheNodeOutcome::from_source(result));
        }
        crate::stage::TaskKind::Analysis { canonical, source_snapshot } => {
            let result = executor.execute_analysis(
                canonical.as_ref(),
                source_snapshot,
                node.keyed_job.generation,
            );
            let _ = node.completion.send(crate::node::CacheNodeOutcome::from_analysis(result));
        }
        crate::stage::TaskKind::Artifact { canonical, source_snapshot, analysis_snapshot, profile_hash } => {
            let result = executor.execute_artifact(
                canonical.as_ref(),
                source_snapshot,
                analysis_snapshot,
                *profile_hash,
                node.keyed_job.generation,
            );
            let _ = node.completion.send(crate::node::CacheNodeOutcome::from_artifact(result));
        }
        crate::stage::TaskKind::Load { .. } => unreachable!(
            "TaskKind::Load routes through SchedulerIoPool, not dispatch_cpu_task"
        ),
    }
    // `_permit_guard` drops here — increments the semaphore counter
    // and notifies one waiter (RAII).
}
```

**Generic dedupe-hook surface:**

```rust
// crates/verter_scheduler/src/dedupe_hook.rs (NEW)
//
// The trait has NO `verter_session` / `cache_runtime` path in any
// method signature or struct field. Consumer-side dedupe runs in the
// calling crate BEFORE constructing a scheduler submission.
pub trait DedupeHook: Send + Sync {
    /// Probe whether `dedup_key` is already known to the caller's
    /// in-flight table. If `Some`, the caller blocks on the existing
    /// flight and the scheduler skips enqueue.
    fn probe(&self, dedup_key: &crate::job::DedupKey) -> Option<DedupeJoiner>;
}

/// Opaque handle the caller may use to attach a completion as a
/// joiner on an in-flight flight.
pub struct DedupeJoiner { _opaque: () }
```

`Scheduler::submit_request`, `Scheduler::try_submit_dag`, and
`Scheduler::submit_dag_blocking` accept an optional `&dyn DedupeHook`
argument. `verter_session::cache_runtime` implements `DedupeHook` over
its `InflightTable`; the scheduler crate stays unaware of the
cache-runtime substrate.

**`Scheduler` constructor:**

```rust
// crates/verter_scheduler/src/scheduler.rs (constructor rewrite)
//
// Replaces the substrate `Scheduler::new(config, source_loader) -> Arc<Self>`
// at `:169` and `Scheduler::with_executor(config, source_loader, executor) -> Arc<Self>`
// at `:178`. The new signatures take three caller-owned pool handles
// as `Arc<...>` (not borrows — `HostCpuPool` and `SchedulerCpuPool`
// are not `Clone`). Return type stays `Arc<Self>` because the driver
// lifecycle holds `Weak<Scheduler>`.
//
// `SchedulerConfig` (substrate type) is retyped by Block 7: the
// `aging: AgingConfig` field is DELETED (DECISION 1 — priority aging
// is replaced by the driver's scan-free deficit/credit lanes, so
// there is nothing to configure), and two DECISION-2 admission knobs
// are ADDED — `max_dag_nodes: u32` and `max_dag_edges: u32` (the
// `DagAdmissionBudget` ceilings). `SchedulerConfig::default()`
// populates both with the substrate's prior queue-depth heuristics.
// H23 adds `max_cpu_in_flight: u32` and `max_io_in_flight: u32`; the
// DAG admission path reserves both through one `DagCapacityReservation`
// before seeding readiness.
impl Scheduler {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(
        config: SchedulerConfig,
        source_loader: Arc<dyn SourceLoader>,
        host_cpu_pool: Arc<crate::host_cpu_pool::HostCpuPool>,
        scheduler_cpu_pool: Arc<crate::pool::SchedulerCpuPool>,
        scheduler_io_pool: Arc<crate::pool::SchedulerIoPool>,
    ) -> Arc<Self> {
        Self::with_executor(
            config,
            source_loader,
            Arc::new(DefaultExecutor),
            host_cpu_pool,
            scheduler_cpu_pool,
            scheduler_io_pool,
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_executor(
        config: SchedulerConfig,
        source_loader: Arc<dyn SourceLoader>,
        executor: Arc<dyn StageExecutor>,
        host_cpu_pool: Arc<crate::host_cpu_pool::HostCpuPool>,
        scheduler_cpu_pool: Arc<crate::pool::SchedulerCpuPool>,
        scheduler_io_pool: Arc<crate::pool::SchedulerIoPool>,
    ) -> Arc<Self> {
        let scheduler = Arc::new(Self {
            // Existing fields preserved from substrate
            // `scheduler.rs:115-163`. NOTE (DECISION 1): the
            // `job_index: Mutex<JobIndex>` field — which seeded
            // `JobIndex::new(config.aging)` and was scanned for
            // `len()` / `dequeue()` — is DELETED. So is the
            // `deferred_blocker_ids` map (BlockerRegistry-adjacent
            // state). Readiness for ALL work (file stages + cache
            // nodes) now lives in the driver-owned `SchedulerDag`.
            nodes: DashMap::new(),
            edges: EdgeManager::new(),
            inbox: SubmissionInbox::new(),
            overlay: Arc::new(OverlayMap::new()),
            source_loader,
            executor,
            tombstones: DashMap::new(),
            generation_floors: DashMap::new(),
            removal_epoch: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
            driver_handle: Mutex::new(None),
            counters: SchedulerCounters::default(),
            // NEW fields:
            host_cpu_pool,
            scheduler_cpu_pool,
            scheduler_io_pool,
            ready_queue: Arc::new(
                crossbeam_queue::ArrayQueue::new(crate::queue::MAX_READY_QUEUE_DEPTH),
            ),
            // Cache-node inflight dedupe rail, keyed on `DedupKey`
            // (distinct from the deleted file-stage
            // `FileNode.pending_requests` ordering path — DECISION 1).
            pending_requests: Arc::new(DashMap::new()),
            // DECISION 2: typed DAG admission budget + condvar. The
            // submitter reserves node/edge/CPU/I/O budget here (a small
            // mutex), NOT the readiness lock; `submit_dag_blocking`
            // parks on `admission_budget_available`.
            admission_budget: parking_lot::Mutex::new(crate::queue::DagAdmissionBudget {
                max_nodes: config.max_dag_nodes,
                max_edges: config.max_dag_edges,
                max_cpu_work: config.max_cpu_in_flight,
                max_io_work: config.max_io_in_flight,
                in_flight_nodes: 0,
                in_flight_edges: 0,
                in_flight_cpu_work: 0,
                in_flight_io_work: 0,
            }),
            admission_budget_available: parking_lot::Condvar::new(),
            dags: DashMap::new(),
            config,
        });

        // Driver-spawn lifecycle — verbatim with substrate
        // `scheduler.rs:210-221`.
        let weak_scheduler = Arc::downgrade(&scheduler);
        let receiver = scheduler.inbox.receiver.clone();
        let driver_handle = std::thread::Builder::new()
            .name("verter-scheduler-driver".to_string())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || Self::driver_loop_native(weak_scheduler, receiver))
            .expect("driver spawn must succeed at scheduler startup");
        *scheduler.driver_handle.lock() = Some(driver_handle);
        scheduler
    }

    /// Typed admission (DECISION 2 + H23). Acquires node/edge/CPU/I/O
    /// budget under the admission mutex; on success constructs the `DagState`,
    /// registers it, and sends `Submission::DagSubmitted { dag_id }`
    /// so the DRIVER seeds the ready lanes. The submitter never touches
    /// `DagState.readiness.lock()` and never spins on a ready-queue
    /// push. On budget exhaustion it returns `Backpressure` WITHOUT
    /// mutating readiness.
    pub fn try_submit_dag(
        &self,
        dag: crate::node::CacheNodeDag,
    ) -> crate::queue::SubmissionResult<crate::job::DagHandle> {
        let requested_nodes = dag.nodes.len() as u32;
        let requested_edges = dag.edges.len() as u32;
        let requested_cpu_work = dag.count_cpu_work() as u32;
        let requested_io_work = dag.count_io_work() as u32;
        let requested = crate::queue::DagCapacityDemand {
            nodes: requested_nodes,
            edges: requested_edges,
            cpu_work: requested_cpu_work,
            io_work: requested_io_work,
        };
        // Single small mutex around the admission budget — NOT the
        // readiness lock. Either we reserve the full
        // node/edge/CPU/I/O budget and mint one `DagCapacityReservation`
        // here, or we report typed backpressure and leave the DAG
        // un-admitted (no readiness mutation, no partial seeding).
        {
            let mut budget = self.admission_budget.lock();
            let available_nodes = budget.max_nodes - budget.in_flight_nodes;
            let available_edges = budget.max_edges - budget.in_flight_edges;
            let available_cpu_work = budget.max_cpu_work - budget.in_flight_cpu_work;
            let available_io_work = budget.max_io_work - budget.in_flight_io_work;
            let available = crate::queue::DagCapacityDemand {
                nodes: available_nodes,
                edges: available_edges,
                cpu_work: available_cpu_work,
                io_work: available_io_work,
            };
            if requested_nodes > available_nodes
                || requested_edges > available_edges
                || requested_cpu_work > available_cpu_work
                || requested_io_work > available_io_work
            {
                return crate::queue::SubmissionResult::Backpressure {
                    requested,
                    available,
                };
            }
            budget.in_flight_nodes += requested_nodes;
            budget.in_flight_edges += requested_edges;
            budget.in_flight_cpu_work += requested_cpu_work;
            budget.in_flight_io_work += requested_io_work;
        }
        let handle = self.admit_dag(dag);
        crate::queue::SubmissionResult::Admitted(handle)
    }

    /// Blocking admission (DECISION 2). Parks on the
    /// `admission_budget_available` condvar attached to the admission
    /// budget mutex until capacity frees, then admits. No DAG readiness
    /// lock exists on this path; the wait is a condvar park, never a
    /// `yield_now()` spin.
    pub fn submit_dag_blocking(&self, dag: crate::node::CacheNodeDag) -> crate::job::DagHandle {
        let requested_nodes = dag.nodes.len() as u32;
        let requested_edges = dag.edges.len() as u32;
        let requested_cpu_work = dag.count_cpu_work() as u32;
        let requested_io_work = dag.count_io_work() as u32;
        {
            let mut budget = self.admission_budget.lock();
            loop {
                let available_nodes = budget.max_nodes - budget.in_flight_nodes;
                let available_edges = budget.max_edges - budget.in_flight_edges;
                let available_cpu_work = budget.max_cpu_work - budget.in_flight_cpu_work;
                let available_io_work = budget.max_io_work - budget.in_flight_io_work;
                if requested_nodes <= available_nodes
                    && requested_edges <= available_edges
                    && requested_cpu_work <= available_cpu_work
                    && requested_io_work <= available_io_work
                {
                    budget.in_flight_nodes += requested_nodes;
                    budget.in_flight_edges += requested_edges;
                    budget.in_flight_cpu_work += requested_cpu_work;
                    budget.in_flight_io_work += requested_io_work;
                    break;
                }
                // Park on the condvar — releases the budget mutex while
                // waiting and is woken by `DagCompletionAggregator`'s
                // `admission_budget_available.notify_all()` on
                // completion / cancellation / shutdown.
                self.admission_budget_available.wait(&mut budget);
            }
        }
        self.admit_dag(dag)
    }

    /// Internal admission tail shared by `try_submit_dag` /
    /// `submit_dag_blocking`. Constructs the `DagState`, registers it,
    /// and signals the driver to seed the ready lanes. Called ONLY
    /// after the node/edge/CPU/I/O budget is already reserved.
    fn admit_dag(&self, dag: crate::node::CacheNodeDag) -> crate::job::DagHandle {
        let dag_id = self.allocate_dag_id();
        let node_count = dag.nodes.len();
        let mut remaining_upstream = smallvec::SmallVec::with_capacity(node_count);
        let mut upstream_dispositions = smallvec::SmallVec::with_capacity(node_count);
        for _ in 0..node_count {
            remaining_upstream.push(0u32);
            upstream_dispositions.push(None);
        }
        for edge in &dag.edges {
            remaining_upstream[edge.to.0 as usize] = remaining_upstream[edge.to.0 as usize]
                .saturating_add(1);
        }
        let cancellation = crate::cancellation::CancellationToken::new();
        // Wrap each `CacheNodeDagNode` (which is intentionally NOT
        // `Clone` — see the `CacheNodeCompletionSender` doc comment)
        // in an `Arc`. The driver-internal ready queue and
        // `DagState.nodes` share the same `Arc<CacheNodeDagNode>`
        // element; the driver pops an `Arc<CacheNodeDagNode>` and
        // dispatches it by deref'ing into the
        // `dispatch_cpu_task(&CacheNodeDagNode, ...)` signature.
        let node_arcs: Vec<std::sync::Arc<crate::node::CacheNodeDagNode>> = dag
            .nodes
            .into_iter()
            .map(std::sync::Arc::new)
            .collect();
        let state = std::sync::Arc::new(crate::job::DagState {
            nodes: node_arcs,
            edges: dag.edges,
            readiness: parking_lot::Mutex::new(crate::job::DagReadiness {
                remaining_upstream,
                upstream_dispositions,
                completed: 0,
            }),
            cancellation: cancellation.clone(),
        });
        self.dags.insert(dag_id, std::sync::Arc::clone(&state));
        // Hand the DAG to the DRIVER. The driver is the ONLY readiness
        // mutator: on receiving `DagSubmitted { dag_id }` it reads
        // `DagState.readiness`, seeds every zero-upstream root into the
        // priority lanes, and thereafter decrements `remaining_upstream`
        // on each upstream completion (admitting a downstream when its
        // count hits zero AND its `EdgeGate` is satisfied). The
        // submitter performs NO readiness lock and NO ready-queue push —
        // admission already bounded the in-flight node count, so the
        // driver's seeding cannot overflow.
        self.inbox.send(crate::driver::Submission::DagSubmitted { dag_id });
        crate::job::DagHandle::new(dag_id, state, cancellation)
    }

    pub fn dedup_key_for(&self, request: &Request) -> crate::job::DedupKey {
        // The submission identity is (canonical, target stage,
        // content_hash). The `KeyedJob` envelope owns the same
        // tuple in production — this accessor is the probe surface
        // for callers wiring an external `DedupeHook`.
        //
        // Substrate `Request.file_id` is `String`
        // (`scheduler.rs:84-85`); `DedupKey.canonical` is `Arc<str>`
        // (B7 `job.rs` shape) so dedup-key clones stay cheap inside
        // the inflight tables. We pay one `String → Arc<str>`
        // conversion at the submission boundary; every downstream
        // clone is a refcount bump.
        //
        // `request.source` is `Option<Arc<str>>`; when absent (e.g.
        // overlay-only submissions, or analysis-stage probes where
        // the loader has not been driven yet) `content_hash` is `0`
        // — joiners on the same `(canonical, stage, 0)` triple
        // coalesce, and the post-source-stage `SourceSnapshot`
        // carries the authoritative `FileWholeHash` for downstream
        // cache-key composition.
        crate::job::DedupKey {
            canonical: std::sync::Arc::from(request.file_id.as_str()),
            stage: request.target.clone(),
            content_hash: request
                .source
                .as_ref()
                .map(|src| crate::job::stable_content_hash(src.as_ref()))
                .unwrap_or(0),
        }
    }

    pub fn cpu_concurrency_semaphore(&self, n: usize)
        -> Arc<crate::cpu_concurrency::CpuConcurrencySemaphore>
    {
        Arc::new(crate::cpu_concurrency::CpuConcurrencySemaphore::new(n))
    }

    pub fn ready_queue_depth(&self) -> usize {
        self.ready_queue.len()
    }
}

pub struct Scheduler {
    // Existing fields (from substrate). DECISION 1 deletes the
    // `job_index: Mutex<JobIndex>` field (the linear-scanned staged
    // ordering authority) and the `deferred_blocker_ids` map
    // (BlockerRegistry-adjacent state). The single readiness authority
    // for ALL work is the driver-owned `SchedulerDag` (see §7.0); the
    // four priority lanes + deficit/credit policy live on the driver,
    // not on a scanned `JobIndex`.
    pub(crate) nodes: DashMap<String, Arc<FileNode>>,
    pub(crate) edges: EdgeManager,
    pub(crate) inbox: SubmissionInbox,
    pub(crate) overlay: Arc<OverlayMap>,
    pub(crate) source_loader: Arc<dyn SourceLoader>,
    pub(crate) executor: Arc<dyn StageExecutor>,
    pub tombstones: DashMap<String, u64>,
    pub generation_floors: DashMap<String, u64>,
    pub(crate) removal_epoch: AtomicU64,
    pub(crate) shutdown: AtomicBool,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) driver_handle: Mutex<Option<std::thread::JoinHandle<()>>>,
    pub(crate) counters: SchedulerCounters,
    // NEW fields:
    pub(crate) host_cpu_pool: Arc<crate::host_cpu_pool::HostCpuPool>,
    pub(crate) scheduler_cpu_pool: Arc<crate::pool::SchedulerCpuPool>,
    pub(crate) scheduler_io_pool: Arc<crate::pool::SchedulerIoPool>,
    pub(crate) ready_queue: Arc<crossbeam_queue::ArrayQueue<std::sync::Arc<crate::node::CacheNodeDagNode>>>,
    pub(crate) pending_requests: Arc<DashMap<crate::job::DedupKey, Vec<CompletionSender<RequestResult>>>>,
    // DECISION 2: typed DAG admission budget guarded by a small mutex
    // (NOT the readiness lock) + a condvar `submit_dag_blocking` parks
    // on. Released by `DagCompletionAggregator` on completion /
    // cancellation / shutdown.
    pub(crate) admission_budget: parking_lot::Mutex<crate::queue::DagAdmissionBudget>,
    pub(crate) admission_budget_available: parking_lot::Condvar,
    pub(crate) dags: DashMap<u64, Arc<crate::job::DagState>>,
    pub(crate) config: SchedulerConfig,
}
```

The legacy `cpu_pool: rayon::ThreadPool` field (substrate
`scheduler.rs:135`) is REMOVED. Every internal use of `self.cpu_pool`
(verified: `dispatch_meta_jobs` at `:381` and any driver-internal
parse closures) migrates to
`self.scheduler_cpu_pool.try_submit(...)` on driver paths and
`self.scheduler_cpu_pool.install_non_driver(...)` only on non-driver
helpers. The legacy `io_pool: IoPool`
field is replaced by `scheduler_io_pool: Arc<SchedulerIoPool>` and the
driver dispatch path uses only `SchedulerIoPool::try_submit(...)`.

`Scheduler::dispatch_meta_jobs` (the existing component-meta batch
fan-out at `:360`) is preserved verbatim with the one-line change
`self.cpu_pool.install(...)` →
`self.scheduler_cpu_pool.install_non_driver(...)`.
Behavior, counters, and the existing tests stay green.

> Update: `Scheduler::dispatch_meta_jobs` has since been DISSOLVED by
> the host-batch-coordinator change. The scheduler no longer performs
> any outer batch fan-out; the host/runtime layer owns batch fan-out on
> its coordinator pool, and the scheduler exposes only the pool-free
> `Scheduler::account_batch_submission` for per-batch submission
> accounting. There is no `cpu_pool.install` meta-batch site left to
> migrate here — this Block 7 step now applies only to the driver-
> internal parse closures, not to a `dispatch_meta_jobs` method.

**`verter_session::host_construction.rs`:**

```rust
// crates/verter_session/src/host_construction.rs (verbatim)
let host_cpu_pool: Arc<HostCpuPool> =
    Arc::new(HostCpuPool::new(num_threads_outer));
let scheduler_cpu_pool: Arc<SchedulerCpuPool> =
    Arc::new(SchedulerCpuPool::new(num_threads_scheduler));
let scheduler_io_pool: Arc<SchedulerIoPool> =
    Arc::new(SchedulerIoPool::new(io_threads, scheduler_config.max_io_in_flight));

let scheduler: Arc<Scheduler> = Scheduler::new(
    scheduler_config,
    source_loader,
    Arc::clone(&host_cpu_pool),
    scheduler_cpu_pool,
    scheduler_io_pool,
);

let host = VerterHost {
    host_cpu_pool, // host retains its own Arc clone
    scheduler,
    // ... other fields
};
```

**`HostStageExecutor::as_any` per-file diff:**

```rust
// crates/verter_session/src/host_executor.rs (addition to existing impl at :143)
impl StageExecutor for HostStageExecutor {
    // ... existing execute_source / execute_analysis / execute_artifact bodies preserved verbatim ...

    fn execute_cache_node(
        &self,
        node: &CacheNodeDagNode,
        ctx: &CacheNodeDispatchCtx<'_>,
    ) -> CacheNodeOutcome {
        // Routes through `cache_runtime::lookup<N: ArtifactNode>` or
        // `cache_runtime::query::lookup<N: QueryNode>` per the
        // `SchedulerCacheId` discriminator in `node.task_kind`.
        // The host implementation produces a `CacheNodeValue` and
        // returns it inside `CacheNodeOutcome::CacheNode(Ok(value))`.
        let (cache_id, key_hash) = match &node.task_kind {
            crate::stage::TaskKind::CacheNode { cache_id, key_hash } => (*cache_id, *key_hash),
            other => return CacheNodeOutcome::CacheNode(Err(
                crate::executor::StageError::dispatch_mismatch(other),
            )),
        };
        if ctx.cancellation.is_cancelled() {
            return CacheNodeOutcome::CacheNode(Err(crate::executor::StageError::Cancelled));
        }
        // Reconstruct the per-request `WorldSnapshot` and
        // `ResolverContext` from the host's pinned snapshot store
        // (the worker dispatch site does not own them directly; it
        // re-derives them from the request_context Arc on the node).
        let snapshot = self.host.world_snapshot_for_request(&node.request_context);
        let resolver = self.host.resolver_context_for_request(&node.request_context);
        // Match ladder — one arm per `SchedulerCacheId` variant.
        // Each arm constructs the concrete `ArtifactNode` /
        // `QueryNode` impl and routes through the cache-runtime
        // lookup. The `key_hash` is the typed-erased key — the
        // host owns the (cache_id, key_hash) → typed-key resolution
        // map that the request_context pinned at submission time.
        let outcome = match cache_id {
            verter_scheduler::SchedulerCacheId::ResolvedImportFacts => {
                let typed_key: ResolvedImportFactsKey =
                    self.host.resolve_keyed_request(cache_id, key_hash);
                let node_impl = ResolvedImportFactsNode { host: &*self.host };
                self.host.lookup_artifact_to_cache_node_value::<ResolvedImportFactsNode<'_>>(
                    &node_impl, &typed_key, &snapshot, resolver,
                )
            }
            verter_scheduler::SchedulerCacheId::ComponentMetaResult => {
                let typed_key: ComponentMetaResultKey =
                    self.host.resolve_keyed_request(cache_id, key_hash);
                let node_impl = ComponentMetaResultQueryNode { host: &*self.host };
                self.host.lookup_query_to_cache_node_value::<ComponentMetaResultQueryNode<'_>>(
                    &node_impl, &typed_key, &snapshot, resolver,
                )
            }
            verter_scheduler::SchedulerCacheId::MaterializeStructure => {
                let typed_key: MaterializeStructureKey =
                    self.host.resolve_keyed_request(cache_id, key_hash);
                let node_impl = MaterializeStructureQueryNode { host: &*self.host };
                self.host.lookup_query_to_cache_node_value::<MaterializeStructureQueryNode<'_>>(
                    &node_impl, &typed_key, &snapshot, resolver,
                )
            }
            verter_scheduler::SchedulerCacheId::RefCycleResult => {
                let typed_key: RefCycleResultKey =
                    self.host.resolve_keyed_request(cache_id, key_hash);
                let node_impl = RefCycleResultQueryNode { host: &*self.host };
                self.host.lookup_query_to_cache_node_value::<RefCycleResultQueryNode<'_>>(
                    &node_impl, &typed_key, &snapshot, resolver,
                )
            }
            verter_scheduler::SchedulerCacheId::MemberShape => {
                let typed_key: MemberShapeCacheKey =
                    self.host.resolve_keyed_request(cache_id, key_hash);
                let node_impl = MemberShapeQueryNode { host: &*self.host };
                self.host.lookup_query_to_cache_node_value::<MemberShapeQueryNode<'_>>(
                    &node_impl, &typed_key, &snapshot, resolver,
                )
            }
            verter_scheduler::SchedulerCacheId::AugmentationTarget => {
                let typed_key: AugmentationTargetKey =
                    self.host.resolve_keyed_request(cache_id, key_hash);
                let node_impl = ModuleAugmentationIndexNode { host: &*self.host };
                self.host.lookup_artifact_to_cache_node_value::<ModuleAugmentationIndexNode<'_>>(
                    &node_impl, &typed_key, &snapshot, resolver,
                )
            }
            verter_scheduler::SchedulerCacheId::PersistentPureArtifact => {
                let typed_key: CompileOutputKey =
                    self.host.resolve_keyed_request(cache_id, key_hash);
                let node_impl = CompileOutputPureContentNode { host: &*self.host };
                self.host.lookup_artifact_to_cache_node_value::<CompileOutputPureContentNode<'_>>(
                    &node_impl, &typed_key, &snapshot, resolver,
                )
            }
            #[cfg(feature = "test-support")]
            verter_scheduler::SchedulerCacheId::Test => {
                Ok(CacheNodeValue {
                    key_hash,
                    admission: AdmissionDisposition::Cacheable,
                    payload: std::sync::Arc::new(()) as std::sync::Arc<dyn std::any::Any + Send + Sync>,
                })
            }
        };
        match outcome {
            Ok(value) => CacheNodeOutcome::CacheNode(Ok(value)),
            Err(err) => CacheNodeOutcome::CacheNode(Err(err)),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}
```

**`lib.rs` re-exports** (single canonical path per type — no
`or equivalently`):

```rust
// crates/verter_scheduler/src/lib.rs (rewrite of the module declarations + re-exports)
pub mod audit_publish;
pub mod cache_id;
pub mod cancellation;
pub mod cpu_concurrency;
pub mod dedupe_hook;
pub mod driver;
pub mod edges;
pub mod executor;
pub mod host_cpu_pool;
pub mod invalidation;
pub mod job;
pub mod node;
pub mod overlay;
#[cfg(not(target_arch = "wasm32"))]
pub mod pool;
pub mod queue;
pub mod request_context;
pub mod scheduler;
pub mod source_loader;
pub mod stage;

// Re-exports — one canonical path per type.
pub use cache_id::SchedulerCacheId;
pub use cancellation::CancellationToken;
pub use cpu_concurrency::{CpuConcurrencyPermit, CpuConcurrencySemaphore};
pub use dedupe_hook::{DedupeHook, DedupeJoiner};
pub use executor::{CacheNodeDispatchCtx, DefaultExecutor, ExtractedDeps, StageError, StageExecutor};
pub use host_cpu_pool::HostCpuPool;
pub use job::{CacheNodeId, DagHandle, DedupKey, KeyedJob, RequestResult, SchedulerError};
pub use node::{
    AdmissionDisposition, AnalysisSnapshot, ArtifactSnapshot, CacheNodeCompletionSender,
    CacheNodeDag, CacheNodeDagEdge, CacheNodeDagNode, CacheNodeOutcome, CacheNodeValue,
    DagCompletionAggregator, EdgeGate, FileKind, FileNode, NodeId, SchedulerDag,
    SchedulerDagEdge, SchedulerDagNode, SourceSnapshot, WorkKind, WorkKindKey, WorkNodeKey,
};
#[cfg(not(target_arch = "wasm32"))]
pub use pool::{SchedulerCpuPool, SchedulerIoPool};
pub use queue::{DagAdmissionBudget, MAX_READY_QUEUE_DEPTH, SubmissionResult};
// `try_submit_dag` / `submit_dag_blocking` are inherent `Scheduler`
// methods (DECISION 2), reached via the `Scheduler` re-export — not
// free functions, so they are not named here.
pub use scheduler::{dispatch_cpu_task, BatchHandle, Request, Scheduler, SchedulerConfig};
pub use stage::{Priority, SchedulerJobKind, TargetStage, TaskKind};
```

`job::CompletionSender<T>` (the substrate's condvar-backed handle) is
INTENTIONALLY NOT re-exported at the crate root — it remains
reachable via `verter_scheduler::job::CompletionSender` for the
inbox-side completion plumbing it owns. The crate-root
`CacheNodeCompletionSender` re-export (from `node::`) is the
cache-node-only sender. The two types are distinct names, no
collision after re-export.

**Cargo manifest additions:**

```toml
# crates/verter_scheduler/Cargo.toml (additions)
[dependencies]
# Existing entries preserved verbatim.
crossbeam-queue = "0.3"
serde = { workspace = true, features = ["derive"] }
tokio = { version = "1", features = ["sync"] }
# `blake3` powers `job::stable_content_hash` (the `DedupKey.content_hash`
# discriminator). Scheduler is leaf relative to session, so it cannot
# reuse `verter_session`'s blake3 wrappers; the dependency lands here
# directly. Same major version as the workspace's existing pins to
# avoid duplicate compile.
blake3 = "1"

[features]
hotpath = ["dep:hotpath", "hotpath/hotpath"]
# Gates test-only `::stub()` / `::new_for_test()` / `::test_stub()`
# constructors on KeyedJob, CacheNodeDagNode, CacheNodeDispatchCtx,
# DedupKey, OpaqueRequestContext.
test-support = []
```

**Test-support fixture helpers** (gated by `feature = "test-support"`):

`Scheduler::new_for_test()`, `Scheduler::enqueue_analysis(&FileNode)`,
`Scheduler::last_dispatched_task() -> Option<(KeyedJob, TaskKind)>`,
`LastDispatchedTaskRecorder`, `NoopSourceLoader`,
`FileNode::stub_with_canonical(&str)`,
`OpaqueRequestContext::test_stub()`.

```rust
// crates/verter_scheduler/src/scheduler.rs (test-support additions)
#[cfg(feature = "test-support")]
impl Scheduler {
    pub fn new_for_test() -> Arc<Self> {
        let host_cpu_pool = Arc::new(crate::host_cpu_pool::HostCpuPool::new(1));
        let scheduler_cpu_pool = Arc::new(crate::pool::SchedulerCpuPool::new(1));
        let scheduler_io_pool = Arc::new(crate::pool::SchedulerIoPool::new(1, 8));
        let executor: Arc<dyn StageExecutor> = Arc::new(LastDispatchedTaskRecorder::new());
        let source_loader: Arc<dyn crate::source_loader::SourceLoader> =
            Arc::new(crate::source_loader::NoopSourceLoader);
        Self::with_executor(
            SchedulerConfig::default(),
            source_loader,
            executor,
            host_cpu_pool,
            scheduler_cpu_pool,
            scheduler_io_pool,
        )
    }

    pub fn enqueue_analysis(&self, file_node: &crate::node::FileNode) {
        let (_handle, sender) =
            crate::job::completion_pair::<crate::job::RequestResult>();
        let _submitted = self.submit_request(crate::scheduler::Request {
            file_id: file_node.canonical_id.clone(),
            target: crate::stage::TargetStage::Analysis,
            priority: crate::stage::Priority::Background,
            source: Some(std::sync::Arc::from("")),
            file_kind: None,
            request_context: None,
        });
        let _ = sender;

        // Drive to quiescence: poll until the recorder observes a
        // dispatch matching the requested target stage. The driver
        // dispatches Source THEN Analysis; the polling predicate
        // matches specifically on `TaskKind::Analysis { canonical, .. }`
        // so it does NOT return early on the Source dispatch.
        let fixture_canonical = file_node.canonical_id.clone();
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_millis(500);
        while std::time::Instant::now() < deadline {
            if let Some((_, task)) = self.last_dispatched_task() {
                if matches!(
                    &task,
                    crate::stage::TaskKind::Analysis { canonical, .. }
                        if canonical.as_ref() == fixture_canonical.as_str()
                ) {
                    return;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!(
            "Scheduler::enqueue_analysis: driver did not dispatch Analysis \
             task for {} within 500ms",
            fixture_canonical
        );
    }

    pub fn last_dispatched_task(&self) -> Option<(crate::job::KeyedJob, crate::stage::TaskKind)> {
        let executor_any: &dyn std::any::Any = self.executor.as_any();
        executor_any
            .downcast_ref::<LastDispatchedTaskRecorder>()
            .and_then(|recorder| recorder.recorded_task())
    }
}

#[cfg(feature = "test-support")]
pub struct LastDispatchedTaskRecorder {
    last: Arc<parking_lot::Mutex<Option<(crate::job::KeyedJob, crate::stage::TaskKind)>>>,
}

#[cfg(feature = "test-support")]
impl LastDispatchedTaskRecorder {
    pub fn new() -> Self {
        Self { last: Arc::new(parking_lot::Mutex::new(None)) }
    }

    pub fn recorded_task(&self) -> Option<(crate::job::KeyedJob, crate::stage::TaskKind)> {
        self.last.lock().clone()
    }

    fn record(&self, task: crate::stage::TaskKind) {
        *self.last.lock() = Some((crate::job::KeyedJob::stub(), task));
    }
}

#[cfg(feature = "test-support")]
impl crate::executor::StageExecutor for LastDispatchedTaskRecorder {
    fn execute_source(
        &self,
        canonical_id: &str,
        _file_kind: crate::node::FileKind,
        content: Arc<str>,
        generation: u64,
    ) -> Result<crate::node::SourceSnapshot, crate::executor::StageError> {
        self.record(crate::stage::TaskKind::Parse {
            canonical: Arc::from(canonical_id),
            source: Arc::clone(&content),
            file_kind: crate::node::FileKind::NonSfc,
        });
        Ok(crate::node::SourceSnapshot::new_empty(content, generation))
    }

    fn execute_analysis(
        &self,
        canonical_id: &str,
        source: &crate::node::SourceSnapshot,
        generation: u64,
    ) -> Result<crate::node::AnalysisSnapshot, crate::executor::StageError> {
        self.record(crate::stage::TaskKind::Analysis {
            canonical: Arc::from(canonical_id),
            source_snapshot: Arc::new(source.clone()),
        });
        Ok(crate::node::AnalysisSnapshot::new_empty(generation))
    }

    fn execute_artifact(
        &self,
        canonical_id: &str,
        source: &crate::node::SourceSnapshot,
        analysis: &crate::node::AnalysisSnapshot,
        profile_hash: u64,
        generation: u64,
    ) -> Result<crate::node::ArtifactSnapshot, crate::executor::StageError> {
        self.record(crate::stage::TaskKind::Artifact {
            canonical: Arc::from(canonical_id),
            source_snapshot: Arc::new(source.clone()),
            analysis_snapshot: Arc::new(analysis.clone()),
            profile_hash,
        });
        Ok(crate::node::ArtifactSnapshot {
            generation,
            profile_hash,
            data: Arc::new(crate::node::EmptyData),
        })
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}
```

```rust
// crates/verter_scheduler/src/source_loader.rs (test-support addition)
#[cfg(feature = "test-support")]
pub struct NoopSourceLoader;

#[cfg(feature = "test-support")]
impl SourceLoader for NoopSourceLoader {
    fn load(&self, _canonical_id: &str) -> Option<std::sync::Arc<str>> { None }
    fn exists(&self, _canonical_id: &str) -> bool { false }
    fn classify(&self, _canonical_id: &str) -> FileKind { FileKind::NonSfc }
    fn realpath(&self, _canonical_id: &str) -> Option<String> { None }
}
```

```rust
// crates/verter_scheduler/src/node.rs (test-support addition for FileNode)
#[cfg(feature = "test-support")]
impl FileNode {
    pub fn stub_with_canonical(canonical: &str) -> Self {
        Self::new(canonical.to_string(), FileKind::NonSfc)
    }
}
```

```rust
// crates/verter_scheduler/src/request_context.rs (test-support addition)
#[cfg(feature = "test-support")]
impl OpaqueRequestContext {
    pub fn test_stub() -> Self {
        use std::sync::Arc;
        struct NoOpCtx;
        impl RequestContextLike for NoOpCtx {
            fn request_id(&self) -> u64 { 0 }
            fn capture_enabled(&self) -> bool { false }
            fn on_dedup_joiner(&self, _canonical: Arc<str>, _winner_request_id: u64, _winner_audited: bool) {}
            fn record_cache_event(&self, _event: CacheEventKind) {}
            fn install_tls(self: Arc<Self>) -> Box<dyn TlsUninstall + Send> {
                struct NoUninstall;
                impl TlsUninstall for NoUninstall { fn uninstall(self: Box<Self>) {} }
                Box::new(NoUninstall)
            }
        }
        OpaqueRequestContext(Arc::new(NoOpCtx) as Arc<dyn RequestContextLike>)
    }
}
```

#### Legacy Deletions

**STOP-gate (DECISION 1, codex).** Block 7 MUST NOT land while
`job_index` or `BlockerRegistry` remain production ordering
authorities. The guard `scheduler_has_single_readiness_authority`
fails the gate if either type survives on a non-test scheduler path.

DECISION-1 deletions (the staged-ordering / blocker substrate is
replaced by the one driver-owned `SchedulerDag` — see §7.0):

- DELETE `JobIndex` (`crates/verter_scheduler/src/queue.rs`) — the
  linear-scanned (`len()` / `dequeue()` at `queue.rs:180`) staged
  ordering authority.
- DELETE `QueueEntry` and `EffectiveKey` (`JobIndex`'s entry / key
  types).
- DELETE `AgingConfig` and the `aging: AgingConfig` field on
  `SchedulerConfig` — priority aging is replaced by the driver's
  scan-free four-lane deficit/credit policy.
- DELETE the `job_index: Mutex<JobIndex>` field on `Scheduler`.
- DELETE `BlockerRegistry`, `BlockerRef`, `UnblockedJob`, and
  `has_pending_blockers` (`crates/verter_scheduler/src/edges.rs:230`)
  — blocker readiness is now an `out_edge` decrement inside
  `SchedulerDag`.
- DELETE the `Submission::BlockerResolved` inbox variant
  (`driver.rs:13`) and the `deferred_blocker_ids` map on `Scheduler`.
- DELETE file-stage ordering through `FileNode.pending_requests`;
  same-stage requests now attach as `waiters` on one
  `SchedulerDagNode` (`same_stage_requests_join_one_work_node`).

DECISION-2 deletions (backpressure moves to typed DAG admission — see
the `try_submit_dag` / `submit_dag_blocking` design):

- DELETE the `ArrayQueue::push` retry loop in `submit_dag` (the
  submitter-side ready-queue seeding loop). The driver is the only
  readiness mutator.
- DELETE every `std::thread::yield_now()` call on a scheduler
  submission path.
- DELETE the submitter-held `DagState.readiness.lock()` — no DAG
  readiness lock exists on any submission path.

Pre-existing Block 7 deletions:

- DELETE the loop body of `Scheduler::submit_batch`
  (`scheduler.rs:312`) and replace with a no-edge-DAG bridge over
  `try_submit_dag`.
- DELETE the `cpu_pool: rayon::ThreadPool` field on `Scheduler`
  (`scheduler.rs:135`); replaced by `scheduler_cpu_pool: Arc<SchedulerCpuPool>`.
- DELETE per-call CPU pool construction inside
  `Scheduler::with_executor` (substrate `:183-187`); construction
  moves to the calling crate.
- DELETE every branch in `executor.rs` (and host implementations)
  that enqueues a parse closure on the I/O pool.

#### Verification

```
cargo test --package verter_scheduler --tests --verbose
cargo test --package verter_scheduler --features test-support --tests --verbose
cargo test --package verter_session host_compile --tests --verbose
cargo test --workspace --tests --verbose
cargo clippy --workspace -- -D warnings
cargo fmt --all
pnpm install --frozen-lockfile
```

#### Discriminating tests

- `crates/verter_scheduler/tests/no_session_dep.rs::scheduler_does_not_depend_on_verter_session`
  — walks `Cargo.toml` plus every `.rs` file under
  `crates/verter_scheduler/src/**` AND
  `.claude/skills/scheduler/SKILL.md`. Positive: no `dependencies` /
  `dev-dependencies` entry mentions `verter_session`; no `use
  verter_session::*` path expression; no `verter_session::*` /
  `cache_runtime::*` / `host_construction` substring in skill prose
  (test-file path references whitelisted). A synthetic SKILL.md prose
  addition containing `verter_session::host_construction` is asserted
  to trip the guard.
- `crates/verter_scheduler/tests/dedupe_hook_trait_has_no_session_paths.rs::dedupe_hook_trait_signature_uses_no_verter_session_types`
  — parses `dedupe_hook.rs`; asserts the trait methods, the
  `DedupeJoiner` struct, and `DedupKey` carry no `verter_session` /
  `cache_runtime` path component.
- `crates/verter_scheduler/tests/cache_node_dag.rs::cache_node_dag_respects_dependencies_priority_and_cancellation`
  — 4-node DAG `A → B,C → D`. B/C do not start before A completes; D
  does not start before B AND C complete; cancellation of A
  propagates transitively.
- `crates/verter_scheduler/tests/cache_node_dag_carries_required_fields_for_executable_dispatch.rs::cache_node_dag_carries_required_fields_for_executable_dispatch`
  — walks `node.rs` and `job.rs` via `syn::parse_file`; asserts:
  - `CacheNodeDag` exposes `nodes: Vec<CacheNodeDagNode>` (the
    producer-side input shape — callers build the bare node vector;
    the scheduler wraps each into `Arc` in the `admit_dag` admission
    tail reached via `try_submit_dag` / `submit_dag_blocking`),
    `edges: Vec<CacheNodeDagEdge>`,
    `completion_aggregator: Arc<DagCompletionAggregator>`;
  - `DagState` (in `job.rs`) exposes
    `nodes: Vec<Arc<CacheNodeDagNode>>` — the bare
    `Vec<CacheNodeDagNode>` regresses to a clone-required dispatch
    path and fails this assertion;
  - `Scheduler.ready_queue` field type contains
    `ArrayQueue<Arc<CacheNodeDagNode>>` (parsed by matching the
    angle-bracketed inner type — a regression to bare
    `ArrayQueue<CacheNodeDagNode>` fails);
  - `CacheNodeDagNode` carries all nine named fields: `id`,
    `keyed_job`, `task_kind`, `dedup_key`, `priority`,
    `cancellation_token`, `cpu_concurrency_semaphore`,
    `request_context`, `completion`;
  - `CacheNodeDagNode` does NOT derive `Clone` and does NOT
    implement a `clone_for_dispatch` method — the
    `CacheNodeCompletionSender` is single-use by design;
  - `cpu_concurrency_semaphore` field type is
    `Option<Arc<CpuConcurrencySemaphore>>` (the HANDLE — NOT a
    pre-acquired permit; a regression that re-introduces
    `Option<Arc<CpuConcurrencyPermit>>` fails the type assertion);
  - `CacheNodeDagEdge` carries `from`, `to`, `gate`.
- `crates/verter_scheduler/tests/single_source_of_truth_for_task_kind.rs::task_kind_lives_on_cache_node_dag_node_not_keyed_job`
  — parses `job.rs` and `node.rs` via `syn::parse_file`; asserts
  `KeyedJob` has NO field named `task` (or `task_kind`) and
  `CacheNodeDagNode` carries exactly one `task_kind` field. A
  regression duplicating the field on both sides fails.
- `crates/verter_scheduler/tests/pool_isolation.rs::source_parse_runs_on_cpu_pool_not_io_pool`
  — instruments both pools with submit counters. A `TaskKind::Parse`
  increments `SchedulerCpuPool::submit_count` and not
  `SchedulerIoPool::submit_count`; a `TaskKind::Load` increments
  `SchedulerIoPool::submit_count`.
- `crates/verter_scheduler/tests/parse_runs_on_cpu_not_io.rs::parse_task_routes_through_scheduler_cpu_pool`
  — submits `TaskKind::Parse { canonical, source, file_kind }` with
  a recording executor wrapping both pools.
  `SchedulerCpuPool::submit_count == 1`, `SchedulerIoPool::submit_count == 0`.
- `crates/verter_scheduler/tests/scheduler_cpu_pool_distinct_from_host_cpu_pool.rs::scheduler_cpu_pool_is_not_the_same_instance_as_host_cpu_pool`
  — constructs the scheduler with both pools; submits a
  `TaskKind::CacheNode`; asserts the task ran on `SchedulerCpuPool`
  (`scheduler_cpu_pool_submit_count > 0`,
  `host_cpu_pool_install_count == 0`).
- `crates/verter_scheduler/tests/scheduler_cpu_pool_has_8mib_stack.rs::scheduler_cpu_pool_has_8mib_stack`
  — runs a 200-level recursive parse fixture through
  `SchedulerCpuPool::submit`. Completes without stack overflow.
- `crates/verter_scheduler/tests/cpu_concurrency_semaphore_caps_concurrent_cpu_tasks.rs::semaphore_capacity_4_caps_concurrent_cpu_tasks_at_4`
  — acquires `let semaphore: Arc<CpuConcurrencySemaphore> = scheduler.cpu_concurrency_semaphore(4)`;
  submits 16 cache-node tasks each carrying
  `Arc::clone(&semaphore)`; observed max concurrent CPU tasks ≤ 4.
  (Originally named `threads_option_honored.rs` when `compile_many`
  exposed a per-call `threads` option; the option has since been
  removed and `HostCpuPool` worker count is sized at host
  construction via `HostConfig::host_cpu_threads`. The semaphore
  capacity is now sourced from the scheduler config rather than a
  per-call option.)
- `crates/verter_scheduler/tests/cpu_concurrency_permit_acquired_per_task_at_dispatch.rs::cpu_concurrency_permit_acquired_per_task_at_dispatch`
  — capacity-4 semaphore, 8 long-running cache-node tasks each
  carrying `Arc::clone(&semaphore)`. Executor records
  `currently_in_flight: AtomicUsize` per-task entry/exit. After the
  latch releases and the DAG drains, `max_observed.load(...) == 4`
  (NOT 1 — a pre-acquired permit would gate all but one; NOT 8 — an
  unbounded semaphore would admit all). Cloning a single
  pre-acquired permit across 8 nodes would observe `max_observed == 8`
  and fail.
- `crates/verter_scheduler/tests/cpu_concurrency_permit_compiles.rs::cpu_concurrency_permit_compiles_and_limits_per_task_admission`
  — capacity-4 semaphore; 4 threads each acquire and hold a permit;
  5th thread's `acquire()` blocks for at least 100 ms after the
  barrier; returns within 100 ms after the test drops one of the
  initial 4 permits. Sibling trybuild fail-test
  `tests/trybuild/parking_lot_semaphore_does_not_exist.rs` asserts
  `use parking_lot::Semaphore;` fails to compile.
- `crates/verter_scheduler/tests/scheduler_new_accepts_arc_host_cpu_pool.rs::scheduler_new_accepts_arc_host_cpu_pool_without_lifetime_or_clone_violation`
  — `let scheduler: Arc<Scheduler> = Scheduler::new(config, source_loader, Arc::clone(&host_cpu_pool), Arc::new(SchedulerCpuPool::new(2)), Arc::new(SchedulerIoPool::new(2, 64)))`.
  Compiles without any `Clone` bound on `HostCpuPool`;
  `Arc::strong_count(&host_cpu_pool) >= 2`; dropping the returned
  `Arc<Scheduler>` joins the driver. Sibling trybuild fail-tests:
  `scheduler_new_borrow_form_rejected.rs` and
  `scheduler_new_self_return_rejected.rs`.
- `crates/verter_session/tests/compile_many_does_not_wait_on_its_own_pool.rs::compile_many_does_not_wait_on_its_own_pool`
  — instruments both pools with worker-affinity tracking. A
  saturating `compile_many` blocks at `wait_parse_jobs()`. The
  blocked thread reports `current_worker_pool() == HostCpuPool`. No
  worker waits for a job in its OWN pool. A single-shared-pool
  implementation would report `SchedulerCpuPool` on the blocked
  thread.
- `crates/verter_scheduler/tests/inflight_dedupe.rs::concurrent_submits_for_same_dedup_key_join_one_flight`
  — two submitters with identical `dedup_key`; the scheduler runs
  exactly one compute job (`compute_run_count == 1`); both submitters
  receive equal results.
- `crates/verter_scheduler/tests/cache_node_dag_backpressure.rs::dag_submission_blocks_or_rejects_when_admission_budget_exceeds`
  — scheduler constructed with a `DagAdmissionBudget` of
  `max_nodes = 64`. Compute completion held behind a test latch.
  `try_submit_dag` of a 64-node DAG returns
  `SubmissionResult::Admitted(_)`; a second `try_submit_dag` of any
  further node returns `SubmissionResult::Backpressure { requested,
  available }` with `available.nodes == 0` SYNCHRONOUSLY and WITHOUT
  mutating readiness (asserted: `scheduler.ready_queue_depth()`
  observed by the driver never exceeds 64, and the backpressured
  submit performed no `DagState.readiness.lock()`). A concurrent
  `submit_dag_blocking` of the overflow DAG parks on
  `admission_budget_available`. After the latch releases and the
  first DAG drains, the parked `submit_dag_blocking` wakes, admits,
  and all jobs complete with each `compute_run_count == 1`. A
  submitter-side ready-queue push loop observes a 128-deep ready
  queue (or a wedged driver) and fails.
- `crates/verter_scheduler/tests/scheduler_new_spawns_driver_thread.rs::scheduler_new_spawns_driver_with_weak_handle`
  — `scheduler.driver_handle.lock().is_some() == true` immediately
  after construction; the JoinHandle has name
  `"verter-scheduler-driver"`; dropping the returned `Arc<Scheduler>`
  joins the driver thread.
- `crates/verter_scheduler/tests/stage_executor_dispatches_cache_node.rs::dispatch_cpu_task_acquires_per_task_permit_then_calls_execute_cache_node`
  — recording `StageExecutor` overrides `execute_cache_node` to
  record `(method_name, currently_in_flight_at_entry)`. Capacity-4
  semaphore. Single `CacheNodeDagNode` with
  `task_kind = TaskKind::CacheNode { .. }`. Observed: exactly one
  entry `(method_name == "execute_cache_node", permits_taken == 1)`.
  A regression omitting the per-task acquire observes
  `permits_taken == 0`. A regression using a bare
  `executor.execute(node)` call fails to compile against the
  substrate trait.
- `crates/verter_scheduler/tests/cache_node_outcome_adapters_compile.rs::cache_node_outcome_adapters_construct_each_variant`
  — constructs one of each `CacheNodeOutcome` variant via the
  adapters using real substrate constructors:
  `from_source(Ok(SourceSnapshot::new_empty(Arc::from(""), 0)))`,
  `from_analysis(Ok(AnalysisSnapshot::new_empty(0)))`,
  `from_artifact(Ok(ArtifactSnapshot { generation: 0, profile_hash: 0, data: Arc::new(EmptyData) }))`,
  `CacheNodeOutcome::stub()`. Asserts each value matches the
  expected variant discriminant.
- `crates/verter_scheduler/tests/dispatch_cpu_task_wires_outcomes_correctly.rs::dispatch_cpu_task_compiles_against_substrate`
  — function-pointer witness:

  ```rust
  use verter_scheduler::{dispatch_cpu_task, CacheNodeDagNode, CancellationToken, StageExecutor};
  #[test]
  fn dispatch_cpu_task_compiles_against_substrate() {
      let _signature_witness: fn(
          &CacheNodeDagNode,
          &dyn StageExecutor,
          Option<&dyn verter_audit::AuditObserver>,
          &CancellationToken,
      ) = dispatch_cpu_task;
  }
  ```
- `crates/verter_scheduler/tests/dispatch_cpu_task_wires_outcomes_correctly.rs::cache_node_outcome_adapters_take_result_not_unwrapped_value`
  — pins each adapter's signature: the explicit form
  `CacheNodeOutcome::from_source(result)` compiles; the
  `Result::map(CacheNodeOutcome::from_source)` form does NOT.
- `crates/verter_scheduler/tests/dispatch_cpu_task_wires_outcomes_correctly.rs::execute_cache_node_returns_cachenodeoutcome_not_result`
  — `let outcome: CacheNodeOutcome = executor.execute_cache_node(&node, &ctx);`.
  Type annotation pins the return type as `CacheNodeOutcome` (not
  `Result<CacheNodeOutcome, StageError>`).
- `crates/verter_scheduler/tests/scheduler_skill_uses_semaphore_handle_form.rs::scheduler_skill_does_not_use_propagated_permit_form`
  — reads `.claude/skills/scheduler/SKILL.md` and asserts:
  - substring `cpu_permit:` absent;
  - substring `Option<Arc<CpuConcurrencyPermit>>` absent;
  - substring `cpu_concurrency_semaphore` present at least once;
  - substring `Arc<CpuConcurrencySemaphore>` present at least once.
- `crates/verter_scheduler/tests/driver_threads_canonical_to_analysis_variant.rs::driver_threads_canonical_to_analysis_variant`
  — enqueues `TargetStage::Analysis` against
  `FileNode::stub_with_canonical("foo.ts")`. After the driver
  resolves the upstream Parse and threads the source-snapshot into
  the downstream Analysis, the dispatched task is
  `TaskKind::Analysis { canonical, source_snapshot }` with
  `canonical.as_ref() == "foo.ts"`. A regression substituting
  `Arc::from("")` (e.g. because `SourceSnapshot` has no
  `canonical_id()` accessor) fails the equality assertion. The test
  file declares `#![cfg(feature = "test-support")]`.
- `crates/verter_scheduler/tests/host_stage_executor_has_as_any.rs::host_stage_executor_implements_as_any`
  — walks `crates/verter_session/src/host_executor.rs` via
  `syn::parse_file`; asserts the `impl StageExecutor for HostStageExecutor`
  block contains a `fn as_any(&self) -> &dyn std::any::Any` method
  body. Without the per-impl `as_any` body, `cargo build --workspace`
  fails E0046; this test pins the requirement explicitly so a
  regression that drops the body fails before workspace build.
- `crates/verter_scheduler/tests/every_stage_executor_impl_provides_as_any.rs::every_stage_executor_impl_provides_as_any`
  — walks every `.rs` file under `crates/` via
  `grep -rE "impl StageExecutor for" crates/` + `syn::parse_file`;
  enumerates every `impl StageExecutor for <T>` block and asserts
  each contains a `fn as_any(&self) -> &dyn std::any::Any` method
  body. The expected impl set at plan-land time is
  `{DefaultExecutor, HostStageExecutor, BlockingExecutor,
  ProbeExecutor, ParentAndDepProbe, PerFileProbe, SlowExecutor,
  PassthroughExecutor, SessionProbeExecutor, LastDispatchedTaskRecorder}`
  — any new impl added in a follow-up commit without an `as_any`
  body fails this test. The test does NOT hard-code the list; it
  enumerates dynamically AND asserts the minimum 10 impls are
  present.
- `crates/verter_scheduler/tests/task_kind_clone_is_cheap_arc_clone.rs::task_kind_clone_cost_is_arc_clone_not_deep_copy`
  — constructs `TaskKind::Parse { canonical: Arc::from("foo.ts"),
  source: Arc::from(string_1mib), file_kind: FileKind::NonSfc }`;
  measures `task.clone()` cost over N iterations. Assert p99 < 100ns.
  A regression switching to deep cloning of the 1 MiB source would
  observe ≫ 100ns and fail.

**DECISION-1 guards (single readiness authority — §7.0):**

- `crates/verter_scheduler/tests/single_readiness_authority.rs::scheduler_has_single_readiness_authority`
  — walks `crates/verter_scheduler/src/**` via `syn::parse_file`;
  asserts NO `JobIndex`, `QueueEntry`, `EffectiveKey`, `AgingConfig`,
  `BlockerRegistry`, `BlockerRef`, `UnblockedJob` type definition
  survives; NO `job_index` / `deferred_blocker_ids` field on
  `Scheduler`; NO `has_pending_blockers` fn; NO
  `Submission::BlockerResolved` variant. A synthetic re-introduction of
  a `job_index: Mutex<JobIndex>` field trips the guard. This is the
  STOP-gate enforcement.
- `crates/verter_scheduler/tests/dispatch_path_no_linear_scan.rs::scheduler_dispatch_path_no_linear_job_scan`
  — instruments the driver's dispatch step; asserts dispatch reads only
  the four `VecDeque<NodeId>` priority lanes (probe counts lane pops)
  and never iterates the full node/job set. A regression scanning all
  pending nodes increments the full-scan probe and fails.
- `crates/verter_scheduler/tests/blocker_resolution_out_edges_only.rs::blocker_resolution_touches_only_out_edges`
  — a 3-node DAG `A → B`, `A → C`; on `A` completion the driver
  decrements only `B` and `C` `remaining_upstream` (the out-edges of
  `A`); asserts no unrelated node's count is read or mutated.
- `crates/verter_scheduler/tests/dynamic_import_expansion.rs::dynamic_import_edges_added_before_downstream_dispatch`
  — a `Parse` node whose parsed facts reveal a dynamic import. Asserts
  the driver adds the discovered `B.Parse → A.Analysis` edge and
  increments `A.Analysis.remaining_upstream` BEFORE the `A.Parse →
  A.Analysis` edge is released — i.e. `A.Analysis` is never dispatched
  before the dynamically discovered upstream completes. A regression
  releasing `Analysis` at `Parse` completion (before expansion) lets
  `Analysis` run early and fails.
- `crates/verter_scheduler/tests/stale_generation_fence.rs::stale_generation_nodes_never_dispatch_after_bump`
  — submits work under generation N, bumps the file generation to N+1
  mid-flight; asserts every `(canonical, N)` node is skipped by the
  ready-lane generation fence and no worker completion for a generation
  N node is accepted (`FileNode.generation() == node.key.generation`
  gate). A regression dispatching the stale node fails.
- `crates/verter_scheduler/tests/same_stage_requests_join.rs::same_stage_requests_join_one_work_node`
  — two `submit_request` callers for the same `(canonical, stage,
  content_hash)` `WorkNodeKey`; asserts exactly one
  `SchedulerDagNode` is created, both completion senders attach to its
  `waiters`, and the stage executes once (`compute_run_count == 1`).

**DECISION-2 guards (admission-time backpressure):**

- `crates/verter_scheduler/tests/admission_backpressure_typed.rs::submit_dag_backpressure_is_typed_before_readiness_mutation`
  — fills the `DagAdmissionBudget`; asserts the next `try_submit_dag`
  returns `SubmissionResult::Backpressure { .. }` and that NO
  `DagState` was constructed and NO `DagState.readiness.lock()` was
  taken on the backpressured path (readiness-lock acquisition probe ==
  0 for the rejected submit).
- `crates/verter_scheduler/tests/submission_never_spins.rs::dag_submission_never_spins_or_holds_readiness_lock`
  — a recording lock-wrapper around `DagState.readiness`; asserts a
  submitter thread never acquires it and the submit path performs no
  busy-retry on the ready queue (push-attempt probe on the submit path
  == 0; all pushes happen on the driver thread).
- `crates/verter_scheduler/tests/no_yield_now_on_submission_paths.rs::scheduler_submission_paths_do_not_call_yield_now`
  — static guard: walks `scheduler.rs` / `queue.rs` / `driver.rs` via
  `syn::parse_file`; asserts no `std::thread::yield_now` (or
  `thread::yield_now`) call expression exists in any `submit_*` fn body
  or admission-tail (`admit_dag`). A re-introduced spin trips the
  guard.
- `crates/verter_scheduler/tests/blocking_submission_parks.rs::blocking_submission_parks_on_admission_condvar`
  — capacity-1 budget held by an admitted DAG behind a latch; a second
  thread calls `submit_dag_blocking` and is observed PARKED (not
  spinning — CPU-time probe ≈ 0 while waiting) on
  `admission_budget_available`; releasing the latch wakes it and it
  admits. A spin-loop implementation observes nonzero CPU time while
  waiting and fails.

**H23 guards (driver-safe worker-pool submission):**

- `crates/verter_scheduler/tests/driver_pool_submission.rs::driver_never_blocks_on_io_pool_send`
  — static guard: walks `driver.rs`, `scheduler.rs`, `pool.rs`, and
  every `dispatch_*` helper reachable from the driver; rejects
  `Sender::send`, blocking `Receiver::recv`, `Condvar::wait`,
  `ThreadPool::install`, or any blocking wrapper inside driver
  dispatch. `SchedulerCpuPool::install_non_driver` is whitelisted only
  inside its own definition and call sites proven unreachable from
  driver dispatch. The only allowed I/O submission call from driver code
  is `SchedulerIoPool::try_submit`.
- `crates/verter_scheduler/tests/driver_pool_submission.rs::driver_never_blocks_on_cpu_pool_submit`
  — instruments a saturated `SchedulerCpuPool` and dispatches a ready
  CPU node. Submission returns immediately via `spawn_fifo` while all
  workers are busy, and the driver continues processing an unrelated
  ready node. A regression that calls `ThreadPool::install` or otherwise
  waits for worker availability blocks the unrelated node and fails.
- `crates/verter_scheduler/tests/driver_pool_submission.rs::pool_capacity_reserved_before_ready_seed`
  — submits a DAG containing `Load`, `Parse`, and `CacheNode` work with
  an admission budget smaller than the DAG's CPU/I/O demand. The
  backpressured submit returns before any ready lane is seeded, and
  `ready_queue_depth()` remains unchanged. A regression that seeds
  readiness first and blocks on pool submission fails.

**Critical implementation risks (blocking):**

- **Single accounting source for DAG budget and pool permits.** Block 7
  must introduce one `DagCapacityReservation` value created during
  `try_submit_dag` / `submit_dag_blocking` admission. It owns the
  reserved node count, edge count, CPU work count, and I/O work count,
  and is moved into `DagState` / `DagCompletionAggregator`. No worker
  pool may maintain an independent "truth" that can drift from
  `DagAdmissionBudget`; worker permits are borrowed from the reservation
  and released through the same terminal path. Completion,
  cancellation, panic, supersession, and shutdown all release exactly
  once. Guards:
  `dag_capacity_reservation_is_single_accounting_source`,
  `capacity_reservation_releases_exactly_once_on_completion_cancel_panic_shutdown`,
  `pool_permit_and_dag_budget_cannot_double_release`.
- **Deferred lanes must be bounded and fair.** H23 parks nodes when
  pool capacity is unexpectedly unavailable. Those deferred lanes are
  driver-owned, bounded by the same admitted capacity, and scheduled by
  the same priority/deficit policy as ordinary ready lanes. A saturated
  stream of new critical work must not permanently starve an admitted
  deferred background node; a flood of background deferred work must not
  delay critical ready work beyond the configured deficit bound. Guards:
  `deferred_lane_is_bounded_by_admitted_capacity`,
  `deferred_lane_eventually_runs_under_sustained_cpu_saturation`,
  `critical_ready_work_not_starved_by_deferred_background_work`.
- **Concurrency-sensitive scheduler behavior needs deterministic model
  coverage.** In addition to integration tests, Block 7 adds a seeded
  model/stress suite that generates random DAGs, priorities, dynamic
  import expansions, cancellations, generation bumps, pool-capacity
  failures, and shutdowns. The model asserts: no node runs before all
  satisfied gates; stale generations never commit; each waiter completes
  once; admitted capacity returns to zero at quiescence; and no driver
  path blocks. Guards:
  `scheduler_model_random_dags_preserve_readiness_invariants`,
  `scheduler_model_capacity_returns_to_zero_at_quiescence`,
  `scheduler_model_driver_never_blocks_under_seeded_pool_failures`.

#### Owning-doc updates

- `.claude/skills/scheduler/SKILL.md` — REWRITTEN to derive from this
  block's body (see SKILL parity work in
  `.claude/skills/scheduler/SKILL.md`). Carries verbatim:
  - the H20 dependency invariant section + the
    `no_session_dep::scheduler_does_not_depend_on_verter_session`
    guard scope (Cargo.toml + every `.rs` file + this skill markdown);
  - the generic `DedupeHook` trait surface;
  - the dual-pool ownership model
    (`HostCpuPool` + `SchedulerCpuPool` distinct rayon thread pools)
    + the deadlock-free invariant ("no worker waits for a job in its
    OWN pool");
  - the `cpu_concurrency_semaphore(n) -> Arc<CpuConcurrencySemaphore>`
    handle surface + the per-task RAII permit acquisition at worker
    dispatch (NOT a pre-acquired permit propagated through the DAG);
  - the single-source-of-truth `CacheNodeDagNode` envelope with all
    nine fields including `task_kind` (no separate
    `KeyedJob.task`);
  - the `TaskKind` split (`Load` I/O / `Parse` CPU / `Analysis` /
    `Artifact` / `CacheNode`);
  - the `ready_queue_depth()` observability accessor;
  - the `CacheNodeDag` executable API (with `EdgeGate` on each edge);
  - the `Scheduler` storage shape (`Arc<HostCpuPool>` /
    `Arc<SchedulerCpuPool>` / `Arc<SchedulerIoPool>`);
  - the `Scheduler::new(...) -> Arc<Self>` constructor signature
    matching this block;
  - the `dispatch_cpu_task` 4-param signature.
  The skill MUST NOT contain any `verter_session::*` or
  `cache_runtime::*` path expression as a symbol reference.
- `.claude/skills/host-session/SKILL.md` — confirm the one-line
  pointer to `.claude/skills/scheduler/SKILL.md`.

#### Public API mirrors

- Rust crate: `pub` `try_submit_dag` + `submit_dag_blocking` +
  `CacheNodeDag` from `verter_scheduler::scheduler`; `pub`
  `SubmissionResult<T>` + `DagAdmissionBudget` from
  `verter_scheduler::queue`; `pub` re-exports per the lib.rs diff
  above.
- NAPI / WASM / protocol DTO / TS types / JS wrappers / compat: no
  exposure.

#### Blocks blocked by this block

- B6 (`compile_many` consumes B7's `CpuConcurrencySemaphore` and
  `CacheNodeDagNode` types in its DAG submission path).
- B12 (DAG bench scenarios depend on `try_submit_dag` /
  `submit_dag_blocking`).

### 8. Rehome remaining host caches + delete bespoke invalidation

#### Context

`ProjectTypeStore` (`crates/verter_session/src/project_type_store.rs:1641`)
already owns the indexed/analysis/route/semantic/owner-import/
component-meta/intrinsic stores plus the ten typed DB wrappers for
component-meta caches enumerated in Block 4. The remaining
"rehoming" work, partially complete per
`docs/arch/debt-closure/12-host-cache-rehoming.md`, is:

- delete bespoke per-call cache-invalidation paths on `VerterHost`;
- complete the `DeclIdentity` → `ResolvedDeclSlotIdentity` migration
  from Block 4 at every call site;
- record per-block file/line references for every removed path.

#### Changes

Per-file deletion + migration list:

| File | Lines | Removed code path | Replacement |
|---|---|---|---|
| `crates/verter_session/src/host_lifecycle.rs` | `269` (`pub fn clear_compile_cache(&self)`) | Per-canonical compile cache eviction | Lazy fact-revalidation through B4 query-identity nodes |
| `crates/verter_session/src/host_lifecycle.rs` | `410` (`pub fn notify_close(&self, canonical_id: &str)`) | Bespoke per-close invalidation | Snapshot-pin release; entries age out via B10 memory policy |
| `crates/verter_session/src/host_lifecycle.rs` | `358` (`pub fn configure_projects(...)`) | Bulk reset on project reconfigure | Project generation increment invalidates all caches lazily (skill `R26`-style) |
| `crates/verter_session/src/host_lifecycle.rs` | `44` (`pub fn set_workspace(&self, workspace: Arc<dyn WorkspaceAccess>)`) | Bulk reset on workspace change | Workspace generation increment; same lazy revalidation |
| `crates/verter_session/src/host_upsert.rs` | per-canonical cache-clear in upsert | Same | `parse_stable_hash` change invalidates artifact nodes; nothing else |
| `crates/verter_session/src/semantic_query.rs:1143,1282` | `DeclIdentity` in `Instantiate.base` and `ResolveMacroPayload.owner` | Block 4 migration | `ResolvedDeclSlotIdentity` |
| `crates/verter_session/src/component_meta_caches.rs` | per-DB `clear_*` helpers used as authority | Reverse-dependent eviction | Validated lazy revalidation per B4 |

After the deletions, `VerterHost` owns: configuration, workspace,
scheduler, `host_cpu_pool: Arc<HostCpuPool>`, `ProjectTypeStore`.
Nothing else with cache shape.

`docs/arch/debt-closure/12-host-cache-rehoming.md` is updated to
reflect the realized state. The file is not deleted — it remains the
binding rehoming spec for the realized inventory.

#### Legacy Deletions

- Direct host-owned result maps for compile, resolved type, eval env,
  and semantic DB cache state (replaced by `ProjectTypeStore`
  handles).
- `clear_compile_cache` / `notify_close` / `set_workspace` /
  `configure_projects` bespoke per-call invalidation lists.
- Reverse-dependent eviction loops used as correctness authority.
- `DeclIdentity` as a key field on `SemanticQueryKey::Instantiate` and
  `ResolveMacroPayload`.

#### Verification

```
cargo test --package verter_session host_lifecycle --tests --verbose
cargo test --package verter_session project_type_store --tests --verbose
cargo test --package verter_session semantic_query --tests --verbose
cargo test --workspace --tests --verbose
cargo clippy --workspace -- -D warnings
cargo fmt --all
pnpm install --frozen-lockfile
```

#### Discriminating tests

- `crates/verter_session/tests/host_has_no_cache_shape_fields.rs::verter_host_owns_no_cache_shape_field_outside_project_type_store`
  — walks `VerterHost`'s struct fields; positive: `project_type_store`
  exists and owns every cache handle; negative: no field named
  `compile_cache`, `resolved_type_cache`, `eval_env_cache`, or
  `semantic_db` at the host level outside `project_type_store`.
- `crates/verter_session/tests/no_bespoke_invalidation_lists.rs::no_call_site_drives_cache_eviction_outside_block_10_memory_policy`
  — greps for `\.clear\(\)` and `\.evict_` outside whitelisted files
  (memory policy, scheduler completion drains).
- `crates/verter_session/tests/decl_identity_not_in_query_keys.rs::semantic_query_keys_use_resolved_slot_identity_not_decl_identity`
  — asserts `base` field type is `ResolvedDeclSlotIdentity`; no
  occurrence of `DeclIdentity` inside `SemanticQueryKey::*` variants.

#### Owning-doc updates

- `docs/arch/debt-closure/12-host-cache-rehoming.md` — mark the four
  fields' rehoming as realized; cite B4 and B8 as the binding
  implementation.
- `.claude/skills/type-cache-architecture/SKILL.md` — update the host
  cache map to drop references to the deleted bespoke invalidation
  lists.
- `docs/arch/fact-based-cache.md` — append the per-file
  deletion-evidence table.

#### Public API mirrors

`ProjectTypeStore` is `pub(crate)`. The migration is invisible to
external consumers.

#### Blocks blocked by this block

- None directly. B12 (benchmarks) cross-validates that the cache
  shape on `VerterHost` is minimal post-cutover.

### 9. Persistent pure artifact cache + sealed `PersistentArtifactNode`

#### Context

The cache-runtime hard rules in `CLAUDE.md` → "Cache Architecture
(CRITICAL)" say pure artifacts persist only with complete
semantic/compiler/env/profile/plugin/source-map-policy keys, and
fact-validated semantic query results stay memory-only until their
query family has audited self-root validation and typed
non-cacheable admission. Block 9 specifies the on-disk format, the
serialization library, the file-locking discipline, the env-hash
mismatch read-side validator, the sealed-trait architecture guard
that makes semantic query nodes impossible to persist, and the
corruption-detection contract.

Block 9 also adds `WorldSnapshot::base_write_token()` to the
`WorldSnapshot` type that Block 1 introduced — the field plumbing
landed in B1, but the `BaseToken` type lives in B9 because the
sealed-trait architecture is B9's concern.

#### Changes

Sealed trait surface:

```rust
// crates/verter_session/src/cache_runtime/persistent/mod.rs (NEW)
mod sealed { pub trait Sealed {} }

pub trait PersistentArtifactNode: crate::cache_runtime::artifact::ArtifactNode + sealed::Sealed {
    fn schema_version() -> u32;
    fn cas_namespace() -> &'static str;
    fn serialize(value: &Self::Value, buf: &mut Vec<u8>);
    fn deserialize(buf: &[u8]) -> Option<Self::Value>;
}

/// Compile-time capability witness that the caller is on a base
/// (NOT overlay) view. Constructed only via
/// `WorldSnapshot::base_write_token` (returns `Some` iff
/// `overlay_identity.is_none()`). Both the trait AND `BaseToken`'s
/// only field are sealed — no outside code path can fabricate a
/// `BaseToken` from an overlay snapshot.
pub trait BaseWriteToken: sealed::Sealed {}

pub struct BaseToken {
    _marker: std::marker::PhantomData<()>,
}
impl sealed::Sealed for BaseToken {}
impl BaseWriteToken for BaseToken {}

impl crate::cache_runtime::world_snapshot::WorldSnapshot {
    pub fn base_write_token(&self) -> Option<BaseToken> {
        if self.overlay_identity.is_none() {
            Some(BaseToken { _marker: std::marker::PhantomData })
        } else {
            None
        }
    }
}

pub const PERSISTENT_SCHEMA_VERSION: u32 = 1;

pub struct PersistentCache {
    root: std::path::PathBuf,
    manifest: Manifest,
}

pub struct Manifest {
    /// Per-namespace `ManifestHeader` (read on startup; written on
    /// every put).
    pub headers: parking_lot::RwLock<std::collections::HashMap<&'static str, ManifestHeader>>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ManifestHeader {
    pub schema_version: u32,
    pub compiler_version: Hash16,
    pub project_identity: Hash16,
    pub parse_env_hash: Hash16,
    pub resolve_env_hash: Hash16,
    pub type_env_hash: Hash16,
    pub lib_env_hash: Hash16,
    pub source_map_policy_hash: Hash16,
}

impl PersistentCache {
    /// Persistent write requires a `BaseWriteToken` AND a
    /// `WorldSnapshot` so the env-hash dimensions enter the CAS key
    /// salt at write time.
    pub fn put<N: PersistentArtifactNode, T: BaseWriteToken>(
        &self,
        _token: &T,
        snapshot: &crate::cache_runtime::world_snapshot::WorldSnapshot,
        key: &N::Key,
        value: &N::Value,
    ) -> std::io::Result<()> { /* ... */ }

    /// Persistent read takes `&WorldSnapshot` so the env-hash salt
    /// enters CAS key derivation. Env-hash mismatch returns `None`
    /// (NOT `Err` — a mismatch is a miss).
    pub fn get<N: PersistentArtifactNode>(
        &self,
        snapshot: &crate::cache_runtime::world_snapshot::WorldSnapshot,
        key: &N::Key,
    ) -> Option<N::Value> { /* env-hash mismatch returns None */ }
}
```

Implementors (sealed inside `verter_session`):
`IndexedReadyNode`, `ResolvedImportFactsNode`, `TypedIrResolveNode`,
`MemberSemanticFactsNode`, `MemberDisplayFactsNode`,
`FlowBodyHashNode` (B11), `FlowLoweredBodyNode` (B11),
`CompileOutputNode_PureContent` (`content` mode only).

Query-identity nodes (`SemanticGraphStore`, `MaterializeStructureDb`,
`RefCycleResultDb`, `ComponentMetaResultDb`, `RouteDb`,
`MemberShapeCacheDb`) do NOT implement `PersistentArtifactNode` and
CANNOT — the trait is sealed.

On-disk byte format:

- **Serialization library:** `bincode 2` with configurable encoding,
  fixint encoding for `Hash16` fields, varint for length prefixes.
- **CAS path layout:**
  `<root>/<cas_namespace>/<key_hash_hex[..2]>/<key_hash_hex[2..]>.bin`
  where `key_hash_hex = cas_key_hash(snapshot, encoded_key)` — NOT a
  bare `blake3(encoded_key)`. The salt incorporates the
  `WorldSnapshot` env-hash dimensions so two distinct env
  configurations coexist on disk rather than thrashing:

  ```rust
  fn cas_key_hash(snapshot: &WorldSnapshot, encoded_key: &[u8]) -> [u8; 32] {
      let mut hasher = blake3::Hasher::new();
      hasher.update(&PERSISTENT_SCHEMA_VERSION.to_le_bytes());
      hasher.update(&snapshot.compiler_version);
      hasher.update(&snapshot.project_identity);
      hasher.update(&snapshot.parse_env_hash);
      hasher.update(&snapshot.resolve_env_hash);
      hasher.update(&snapshot.type_env_hash);
      hasher.update(&snapshot.lib_env_hash);
      hasher.update(&snapshot.source_map_policy_hash);
      hasher.update(&snapshot.public_api_mode_hash);
      hasher.update(&snapshot.plugin_versions);
      hasher.update(encoded_key);
      *hasher.finalize().as_bytes()
  }
  ```

- **Per-entry framing:** `(manifest_header, key_bytes_len, key_bytes,
  value_bytes_len, value_bytes, crc32c_of_value_bytes)`.
- **Atomic write:** `tempfile::NamedTempFile::new_in(cas_dir)` →
  write → `flush + fsync` → `persist()` (POSIX rename). Crash-safe;
  readers never observe a partial entry.
- **File locking:** per-CAS-namespace `fd_lock`-backed advisory
  exclusive lock for writes; no lock for reads (atomic writes make
  read-without-lock safe).
- **Per-entry corruption detection:** compute crc32c on `value_bytes`
  at write; verify at read; mismatch returns `None`.
- **Read-side env-hash validator:** every read decodes the manifest
  header first; if any env-hash dimension does not match the
  caller's `WorldSnapshot.<dim>`, return `None` (NOT `Err`).
- **Bounded size:** per-namespace max-bytes config; Block 10's memory
  policy extends to disk pressure via
  `PersistentCache::sweep_to_budget`.

File creation:

- `crates/verter_session/src/cache_runtime/persistent/mod.rs` —
  `PersistentArtifactNode` (sealed), `BaseWriteToken` (sealed),
  `BaseToken` (opaque, no public ctor), `PersistentCache`,
  `PERSISTENT_SCHEMA_VERSION`.
- `crates/verter_session/src/cache_runtime/persistent/cas.rs` — CAS
  path layout + atomic write + per-entry framing + crc.
- `crates/verter_session/src/cache_runtime/persistent/manifest.rs` —
  `ManifestHeader` (de)serialization + env-hash validator.
- `crates/verter_session/src/cache_runtime/persistent/tests.rs` —
  unit tests for encoder/decoder + env-hash validator.

#### Legacy Deletions

- DELETE any persistent semantic-query cache admission path before
  the semantic query audit proves complete fact signatures and
  strict self-root validation.
- DELETE legacy `FileArtifactKey::legacy` / `overlay_scoped` sentinel
  patterns wherever they appear. The typed `BaseWriteToken` capability
  witness and the multi-candidate substrate (skill `R20`) replace every use
  case.
- DELETE any overlay-write path that targets a base CAS namespace.

#### Verification

```
cargo test --package verter_session persistent --tests --verbose
cargo test --workspace --tests --verbose
cargo clippy --workspace -- -D warnings
cargo fmt --all
pnpm install --frozen-lockfile
```

#### Discriminating tests

- `crates/verter_session/tests/persistent_pure_artifacts.rs::cas_write_is_atomic_and_rejects_bad_schema_env_or_checksum`
  — writing under base produces a file whose framing parses with the
  expected schema_version and matching crc32c. Corrupting 1 byte in
  `value_bytes` causes the next read to return `None`; mismatching
  schema_version returns `None`; mismatching any env-hash returns
  `None`.
- `crates/verter_session/tests/persistent_overlay_rejection.rs::overlay_content_mode_never_writes_persistent_or_base_artifacts`
  — `WorldSnapshot` with `overlay_identity = Some(_)`;
  `snapshot.base_write_token()` returns `None`; no CAS file created.
- `crates/verter_session/tests/persistent_cache_admits_only_pure_artifacts.rs::semantic_query_db_does_not_implement_persistent_admission`
  — walks `cache_runtime/persistent/` for
  `impl PersistentArtifactNode for *Db` patterns; only whitelisted
  artifact-node impls compile. A synthetic
  `impl PersistentArtifactNode for SemanticGraphStoreNode` in a
  trybuild fail-test fixture is asserted to fail to compile (sealed
  trait).
- `crates/verter_session/tests/persistent_env_mismatch.rs::persistent_read_returns_none_on_env_hash_mismatch`
  — write under `parse_env_hash = A`; read under `parse_env_hash = B`
  returns `None`. Does not return `Err`; does not return the stale
  payload.
- `crates/verter_session/tests/persistent_two_envs_two_paths_no_collision.rs::persistent_two_envs_have_distinct_cas_paths`
  — writes `(envA, K, A_value)` then `(envB, K, B_value)` where envA
  / envB differ only in `parse_env_hash`. TWO files exist on disk
  (`walkdir` count); read with envA returns `Some(A_value)`; read
  with envB returns `Some(B_value)`; the older write is NOT
  overwritten. A bare-`blake3(encoded_key)` implementation would
  target identical CAS paths and fail.
- `crates/verter_session/tests/persistent_corruption.rs::corrupt_manifest_returns_none_and_does_not_poison_subsequent_reads`
  — corrupt entry's `get` does not return `Err`; subsequent read of
  a different intact key succeeds.
- `crates/verter_session/tests/persistent_restart.rs::pure_artifact_survives_subprocess_restart`
  — uses `TestHarness::restart_in_subprocess()` (B12). Writes
  `IndexedReady`; restarts; reads back; positive: read returns the
  same value.
- `crates/verter_session/tests/persistent_schema_pin.rs::persistent_serialization_layout_is_pinned_by_schema_version`
  — pins on-disk byte layout for each `PersistentArtifactNode`
  implementor against `crates/verter_session/tests/fixtures/persistent-schema/<node>.bin.hex`.
  Serialising a fixed in-memory value with
  `PERSISTENT_SCHEMA_VERSION = N` produces a byte sequence equal to
  the pinned snapshot. Any field reorder, any type-tag change, any
  new field NOT accompanied by a `PERSISTENT_SCHEMA_VERSION` bump
  fails.
- `crates/verter_session/tests/persistent_implementor_whitelist.rs::flow_body_hash_node_is_in_persistent_artifact_implementor_whitelist`
  — walks `cache_runtime/persistent/mod.rs` via `syn::parse_file`;
  collects every `impl PersistentArtifactNode for <T>`; asserts the
  set includes the eight named nodes (`IndexedReadyNode`,
  `ResolvedImportFactsNode`, `TypedIrResolveNode`,
  `MemberSemanticFactsNode`, `MemberDisplayFactsNode`,
  `FlowBodyHashNode`, `FlowLoweredBodyNode`,
  `CompileOutputNode_PureContent`).
- `crates/verter_session/tests/persistent_overlay_compile_error.rs::cache_overlay_snapshot_cannot_construct_base_write_token`
  — `trybuild`-style compile-fail test. A base-snapshot write site

  ```rust
  if let Some(token) = snapshot.base_write_token() {
      cache.put::<IndexedReadyNode, _>(&token, &snapshot, &key, &value);
  }
  ```

  compiles. A synthetic overlay-only site that attempts
  `cache.put::<IndexedReadyNode, _>(&BaseToken { _marker: PhantomData }, &overlay_snapshot, &key, &value)`
  FAILS TO COMPILE with an error citing the sealed `_marker` field
  (private field) AND the sealed trait bound. The trybuild
  expected-stderr snapshot asserts the diagnostic mentions
  "field `_marker` of struct `BaseToken` is private".
- `crates/verter_session/tests/base_write_token_has_no_unchecked_escape_hatch.rs::base_write_token_has_only_one_constructor_via_world_snapshot`
  — walks every `.rs` file under `crates/verter_session/src/**` via
  `syn::parse_file`, excluding `cfg(test)`-gated items. Asserts:
  - (G1) only ONE function names `BaseToken` in its return type, and
    that return type is structurally `Option<BaseToken>`
    (`WorldSnapshot::base_write_token`);
  - (G2) every `BaseToken { _marker: _ }` constructor expression in
    non-test code is inside the body of
    `WorldSnapshot::base_write_token`.

  Synthetic regressions like `pub fn base_write_token_unchecked() -> BaseToken { BaseToken { _marker: PhantomData } }`,
  `impl BaseToken { pub fn new() -> Self { … } }`, or a stray
  `BaseToken { _marker: _ }` literal in a non-test fn body all fail.

#### Owning-doc updates

- `.claude/skills/type-cache-architecture/SKILL.md` — append TWO new
  CRITICAL-headed sections:
  1. `## Sealed PersistentArtifactNode trait (CRITICAL)` — verbatim:
     persistent disk admission is restricted to
     `verter_session::cache_runtime::persistent::PersistentArtifactNode`
     whose only implementors are sealed inside `verter_session`.
     Query-identity nodes CANNOT implement the trait (compile error).
     Guard:
     `persistent_cache_admits_only_pure_artifacts::semantic_query_db_does_not_implement_persistent_admission`.
     Registry entry: `("Sealed PersistentArtifactNode trait", &["semantic_query_db_does_not_implement_persistent_admission", "flow_body_hash_node_is_in_persistent_artifact_implementor_whitelist"])`.
  2. `## Typed BaseWriteToken view gate (CRITICAL)` — verbatim:
     persistent write is gated by the sealed `BaseWriteToken` trait;
     `BaseToken` has a private `_marker: PhantomData<()>` field;
     `WorldSnapshot::base_write_token()` returns `Some(BaseToken)`
     iff `overlay_identity.is_none()`. Persistent reads accept
     `&WorldSnapshot` so env-hash salt enters CAS key derivation.
     Guards:
     `persistent_overlay_compile_error::cache_overlay_snapshot_cannot_construct_base_write_token`,
     `base_write_token_has_no_unchecked_escape_hatch::base_write_token_has_only_one_constructor_via_world_snapshot`.
     Registry entry: `("Typed BaseWriteToken view gate", &["cache_overlay_snapshot_cannot_construct_base_write_token", "overlay_content_mode_never_writes_persistent_or_base_artifacts", "base_write_token_has_only_one_constructor_via_world_snapshot"])`.
- CREATE `docs/arch/persistent-cache.md` — full architecture document
  (manifest header, CAS layout, atomic write, framing/CRC, env-hash
  validator, sealed-trait implementor list, `BaseWriteToken`
  capability model).

#### Public API mirrors

- Rust crate (`verter_session`): `pub` `PersistentArtifactNode` trait
  re-export; `pub` `BaseWriteToken` trait + opaque `BaseToken` (private
  `_marker` field; no public ctor); `pub` `WorldSnapshot::base_write_token`.
- NAPI / WASM / protocol DTO / TS types / JS wrappers / compat: no
  exposure.

#### Blocks blocked by this block

- B11 (`FlowLoweredBodyNode` implements `PersistentArtifactNode`).
- B12 (process-restart bench depends on the persistent cache).

### 10. Memory policy + cache observability + audit additions

#### Context

Today's cache layers use either count-based maps or LRU-on-count.
Skill R22 (eviction is memory-bound, not correctness-bound) and R24
(warm cache validation is counter-only — zero allocation, zero
structured payload emission per hit) plus the cache-runtime hard
rule "cache hits do not allocate audit payloads without an active
accumulator" together require weighted eviction, zero-allocation
audit emission under no-accumulator, and a test-allocator guard.
The block wires a uniform memory-policy trait into every
`ArtifactNode` and `QueryNode`, adds five typed
`StructuredAuditEvent` variants, and extends the test-allocator
guard.

#### Changes

Memory policy. The single weight-accounting shape is the
`ArtifactNode::weight_bytes` / `QueryNode::weight_bytes` method that
Block 2 already defines on both traits. Block 10 does NOT introduce a
separate `WeightedAccountable` trait — every `MemoryPolicy::admit`
call site is already typed against an `ArtifactNode` or `QueryNode`
impl, so `node.weight_bytes(&value)` is the canonical entry point.
The earlier `WeightedAccountable` abstraction was a redundant second
shape and is retired.

```rust
// crates/verter_session/src/cache_runtime/memory_policy.rs (full)
pub struct MemoryPolicy {
    pub byte_budget: std::sync::atomic::AtomicU64,
    pub current_bytes: std::sync::atomic::AtomicU64,
    pub pins: ActiveSnapshotPinRegistry,
    pub eviction_lru: arc_swap::ArcSwap<EvictionRingBuffer>,
}

/// Bounded LRU ring buffer of `(CacheEntryId, last_access_generation)`
/// used by `sweep_to_budget` to pick eviction candidates. Capacity is
/// configured via `MemoryPolicy::new`; older entries roll off the
/// ring on insert.
pub struct EvictionRingBuffer {
    entries: smallvec::SmallVec<[(CacheEntryId, u64); 1024]>,
    capacity: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SnapshotId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CacheEntryId {
    pub cache_id: verter_scheduler::SchedulerCacheId,
    pub key_hash: u64,
}

pub enum AdmissionDecision {
    Admit,
    /// Reject because the byte budget would be exceeded and no
    /// unpinned entries are available for eviction.
    Reject,
}

/// Per-snapshot pin map. Replaces an earlier `AtomicU64`-counter
/// design — a global counter cannot tell which entries are pinned by
/// which snapshot, so the eviction policy could not respect
/// overlapping active snapshots.
pub struct ActiveSnapshotPinRegistry {
    pins: parking_lot::RwLock<std::collections::HashMap<SnapshotId, std::collections::HashSet<CacheEntryId>>>,
    active_count_per_entry: dashmap::DashMap<CacheEntryId, std::sync::atomic::AtomicUsize>,
}

impl ActiveSnapshotPinRegistry {
    pub fn pin(&self, snapshot: SnapshotId, entry: CacheEntryId) { /* ... */ }
    pub fn unpin_snapshot(&self, snapshot: SnapshotId) { /* ... */ }
    pub fn is_pinned(&self, entry: CacheEntryId) -> bool { /* ... */ }
}

impl MemoryPolicy {
    /// Admit `weight_bytes` for `entry_id`. Callers compute
    /// `weight_bytes` via the owning `ArtifactNode::weight_bytes` or
    /// `QueryNode::weight_bytes` method (the substrate single source
    /// of truth — no separate `WeightedAccountable` abstraction).
    pub fn admit(&self, entry_id: CacheEntryId, weight_bytes: usize) -> AdmissionDecision { /* ... */ }
    pub fn release_pin(&self, snapshot: SnapshotId) { self.pins.unpin_snapshot(snapshot); }
    pub fn sweep_to_budget(&self) -> usize { /* returns evicted bytes; respects active pins */ }
}
```

Active snapshot pins: every in-flight request acquires per-entry
pins via `pins.pin(snapshot_id, entry_id)`; release happens via
`pins.unpin_snapshot(snapshot_id)` when the request completes.
Entries are evictable iff `pins.is_pinned(entry) == false`. With
two active snapshots pinning overlapping subsets, an entry stays
pinned until BOTH releases land.

Cache-node metrics (per-node):

```rust
// crates/verter_session/src/cache_runtime/metrics.rs (full)
#[derive(Default)]
pub struct CacheNodeMetrics {
    pub hit: std::sync::atomic::AtomicU64,
    pub miss: std::sync::atomic::AtomicU64,
    pub stale_rejection: std::sync::atomic::AtomicU64,
    pub non_admission: std::sync::atomic::AtomicU64,
    pub inflight_dedupe: std::sync::atomic::AtomicU64,
    pub compute_micros: std::sync::atomic::AtomicU64,
    pub validation_micros: std::sync::atomic::AtomicU64,
    pub stored_bytes: std::sync::atomic::AtomicU64,
    pub evicted_bytes: std::sync::atomic::AtomicU64,
    pub live_entries: std::sync::atomic::AtomicU64,
}
```

`StructuredAuditEvent` additions (verbatim):

```rust
// crates/verter_audit/src/structured_event.rs (additions)
StructuredAuditEvent::CacheNodeHit { cache_id: &'static str, key_hash: u64 } // counter-only
StructuredAuditEvent::CacheNodeMiss { cache_id: &'static str, key_hash: u64, reason: ColdMissReason }
StructuredAuditEvent::CacheNodeStaleRejection { cache_id: &'static str, reason: StaleReason }
StructuredAuditEvent::CacheNodeNonAdmission { cache_id: &'static str, reason: NonAdmissionReason }
StructuredAuditEvent::CacheNodeInflightDedupe { cache_id: &'static str }

#[derive(Debug, Clone, Copy)]
pub enum ColdMissReason {
    NotPresent,
    StaleByFactValidation,
    PinnedSnapshotExpired,
}

#[derive(Debug, Clone, Copy)]
pub enum StaleReason {
    FactSignatureDiverged,
    GenerationSuperseded,
}

/// `NonAdmissionReason` lives here — in `verter_audit`, the leaf
/// observability substrate — because the published audit envelope
/// carries it on `StructuredAuditEvent::CacheNodeNonAdmission` and
/// `verter_audit` must not depend on `verter_session` (CLAUDE.md
/// "Shared Optimized Codebase": `verter_audit` "depends only on
/// `verter_span` and has no back-edge to higher crates").
///
/// `verter_session::cache_runtime::query` re-exports it as the
/// `PublishOutcome::NotAdmitted(NonAdmissionReason)` payload — one
/// canonical definition; one symbol identity end-to-end; no DTO
/// mirroring layer needed.
#[derive(Debug, Clone, Copy)]
pub enum NonAdmissionReason {
    SignatureOverflow,
    BudgetExceeded,
    Cancellation,
    GenerationSupersession,
    IncompleteSelfRooting,
    UnresolvedProvenance,
}
```

Cold vs warm dispatch:

- `CacheNodeHit` — emitted on every warm-hit return. Allocation-free
  under no accumulator.
- `CacheNodeMiss` — emitted on cold compute entry.
- `CacheNodeStaleRejection` — emitted when warm-hit revalidation
  fails and the runtime forks to a cold rebuild.
- `CacheNodeNonAdmission` — emitted when a cold compute returns
  `CacheAdmission::ReturnOnly`.
- `CacheNodeInflightDedupe` — emitted when a caller joins an
  existing flight.

The five variants are exhaustive over the cache-runtime emission
set: every cache-node call site emits exactly one of these on each
warm/cold/stale/non-admission/dedup outcome. No new
`RequestKindPayload` variant is introduced; the audit envelope rolls
up via the existing per-request `RequestAuditRecord` accumulator.

File creation/modification:

- `crates/verter_session/src/cache_runtime/memory_policy.rs` — full
  impl above.
- `crates/verter_session/src/cache_runtime/metrics.rs` —
  `CacheNodeMetrics`.
- `crates/verter_audit/src/structured_event.rs` — add the five
  variants + the `ColdMissReason`, `StaleReason`, and
  `NonAdmissionReason` enums (canonical definitions all live here;
  `verter_session::cache_runtime::query` re-exports
  `NonAdmissionReason` via
  `pub use verter_audit::structured_event::NonAdmissionReason;` so
  publish-pipeline callers see it on the `query` path. Direction is
  one-way: `verter_session` depends on `verter_audit`, never the
  reverse — H20 leaf-substrate rule).
- `crates/verter_session/src/component_meta_audit/mod.rs` — emit the
  new variants from the component-meta cache paths.
- `crates/verter_session/src/request_context.rs` — pin/release
  primitives for active snapshots.
- `crates/verter_session/src/host_manage.rs` — host-level
  configuration for byte budget.
- `packages/benchmark/src/audit-validator.ts` — extend to assert the
  new variants appear with the expected reason taxonomy.
- `packages/types/audit.generated.ts` — regenerated via
  `node scripts/gen-corpus-audit-tests.mjs`.

#### Legacy Deletions

- DELETE count-only / LRU-only cache eviction policies wherever they
  remain.
- DELETE any audit emission path that allocates before checking
  whether a request accumulator is active.
- DELETE the `StructuredAuditEvent::Custom` escape hatch for cache
  subsystems (R23 — typed only); migrate any existing cache-side
  `Custom` events to the new typed variants.

#### Verification

```
cargo test --package verter_session memory_policy --tests --verbose
cargo test --package verter_audit --tests --verbose
cargo test --package verter_session cache_node_audit_alloc --tests --verbose
cargo test --package verter_session audit_byte_budget --features external-corpus --tests --verbose
cargo test --workspace --tests --verbose
cargo clippy --workspace -- -D warnings
cargo fmt --all
pnpm install --frozen-lockfile
```

#### Discriminating tests

- `crates/verter_session/tests/cache_memory_policy.rs::weighted_eviction_respects_active_snapshot_pins_and_byte_budget`
  — byte budget = 1 KiB; insert 10 entries weighing 200 B each; pin
  a snapshot referencing entry 3. `sweep_to_budget` evicts until
  under budget, but entry 3 survives because pinned.
- `crates/verter_session/tests/cache_runtime_pin_prevents_eviction.rs::live_request_pin_blocks_lru_eviction`
  — in-flight request pinning a snapshot blocks LRU eviction; after
  pin release, entries become eligible.
- `crates/verter_session/tests/weighted_eviction_pin_registry.rs::weighted_eviction_respects_pins_with_overlapping_snapshots`
  — two active snapshots S1, S2; S1 pins `{A, B}`; S2 pins `{B, C}`.
  Sweep with a budget that would evict all three. Union `{A, B, C}`
  all survive. Release S1: `{A}` becomes evictable; `{B, C}` remain
  pinned by S2. Release S2: all evictable. A global-counter
  implementation cannot distinguish per-snapshot ownership and
  fails.
- `crates/verter_session/tests/cache_node_audit_alloc.rs::cache_node_hit_allocates_nothing_under_no_accumulator`
  — non-gated test file (does NOT depend on `external-corpus`
  feature). Imports the allocator-counter machinery via a shared
  `crates/verter_session/src/test_support/audit_alloc_counter.rs`
  helper exposed under `#[cfg(any(test, feature = "external-corpus"))]`.
  Warm-hit `CacheNodeHit` emission under no accumulator does not
  increase the test allocator's bytes-allocated counter.
- `crates/verter_session/tests/cache_node_audit_alloc.rs::cache_node_non_admission_payload_is_typed_not_custom`
  — emission matches `StructuredAuditEvent::CacheNodeNonAdmission { .. }`
  with a typed `NonAdmissionReason`; no `Custom` variant.
- `crates/verter_session/tests/cache_node_metrics_complete.rs::every_cache_node_exposes_all_counter_dimensions`
  — walks every `ArtifactNode` / `QueryNode` impl; asserts
  `CacheNodeMetrics` exposes the ten counter dimensions.

#### Owning-doc updates

- `.claude/skills/audit-infrastructure/SKILL.md` — append the five
  new `StructuredAuditEvent` variants with their cold/warm dispatch
  contracts and the typed reason enums.
- `docs/audit-footprint/` — update the API reference index to
  include the new variants.

#### Public API mirrors

- Rust crate (`verter_audit`): `pub` variant additions + reason
  enums.
- NAPI / WASM: audit envelope JSON serialization passes the new
  variants through unchanged.
- Protocol DTOs: no DTO change (audit envelope is JSON-serialized
  via the structured event enum).
- TS generated types (`packages/types/audit.generated.ts`):
  regenerated.
- JS wrappers / compat: pass-through.

#### Blocks blocked by this block

- B12 (benchmark validator depends on the audit variants).

### 11. Native flow-return on the cache-runtime substrate

#### Context

The native flow-return plan at
`/tmp/verter-native-flow-return-coverage.md` proposes its own 6-step
on-demand pipeline for `FlowLoweredBody`, `FlowBody` fact,
`FlowLoweredBodyKey`, `FactKey::FlowBody`, and
`SemanticQueryKey::FlowReturn`. The Shared Optimized Codebase rule
(CLAUDE.md) forbids two plans claiming authority over the same
substrate. Block 11 reconciles: the cache-runtime overhaul provides
the substrate; flow-return imports the substrate; flow-return does
not redefine artifact-node mechanics.

#### Changes

**First task in this block:** move
`/tmp/verter-native-flow-return-coverage.md` →
`docs/arch/native-flow-return.md`. The move is verbatim; content
preserved, only the path changes. After the move,
`docs/arch/native-flow-return.md` is the authoritative flow-return
plan and is referenced from
`.claude/skills/type-resolution/SKILL.md`.

The caller-side wiring (Solver → FlowBodyHashNode →
FlowLoweredBodyKey → FlowLoweredBodyNode) is documented inline below
so an implementer can wire the two-stage lookup from this plan alone.

Block 11 lands flow-return as a TWO-NODE consumer of the
cache-runtime substrate. Body-hash production is split from body
lowering to break the pre-lookup hash circularity:
`FlowLoweredBodyKey.body_semantic_hash` cannot be produced inside
`FlowLoweredBodyNode::compute` because the key must be constructed
BEFORE the node is looked up. `FlowBodyHashNode` is a separate
artifact node whose Key does NOT carry `body_semantic_hash`; its
Value is the hash itself.

```rust
// crates/verter_session/src/cache_runtime/flow_body_hash.rs (NEW)

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FlowBodyHashKey {
    pub canonical: CanonicalId,
    pub function_symbol: SymbolId,
    pub parse_stable_hash: ParseStableHash,
    pub parser_version: ParserVersion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SymbolId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ParseStableHash(pub Hash16);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ParserVersion(pub u32);

/// Fail-closed body-hash outcome. The enum has TWO variants only —
/// `Hash(Hash16)` for a successful body-hash and `BudgetExceeded`
/// for an over-budget production. Invalid representations (e.g. a
/// `(hash: None, budget_exceeded: false)` product type) are
/// impossible at the type level. Callers MUST pattern-match; no
/// `.unwrap()` / `.expect()` is permitted on the outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlowBodyHashOutcome {
    Hash(Hash16),
    BudgetExceeded,
}

pub struct FlowBodyHashNode<'h> { pub host: &'h VerterHost }

impl<'h> ArtifactNode for FlowBodyHashNode<'h> {
    type Key = FlowBodyHashKey;
    type Value = FlowBodyHashOutcome;
    fn compute(
        &self,
        key: &Self::Key,
        snapshot: &WorldSnapshot,
        ctx: &mut ComputeCtx<'_>,
    ) -> CacheAdmission<Self::Value> {
        // Body-hash computation only — does NOT look up FlowLoweredBodyNode
        // through the cache runtime (no circular dependency).
        let outcome = self.host.compute_body_semantic_hash(
            &key.canonical, key.function_symbol, snapshot, ctx,
        );
        match outcome {
            FlowBodyHashOutcome::BudgetExceeded => {
                CacheAdmission::ReturnOnly(FlowBodyHashOutcome::BudgetExceeded)
            }
            FlowBodyHashOutcome::Hash(h) => {
                CacheAdmission::Cacheable(FlowBodyHashOutcome::Hash(h))
            }
        }
    }
}

// Sealing is performed inside `cache_runtime::persistent::mod` (B9) —
// the `Sealed` trait is private to that module. Block 9's whitelist
// is amended in this block to add `FlowBodyHashNode<'h>`:
//
//   // crates/verter_session/src/cache_runtime/persistent/mod.rs
//   impl<'h> sealed::Sealed for super::super::flow_body_hash::FlowBodyHashNode<'h> {}
//
// Block 11 provides only the `PersistentArtifactNode` impl below;
// it compiles because B9 has already sealed the type.
impl<'h> crate::cache_runtime::persistent::PersistentArtifactNode for FlowBodyHashNode<'h> {
    fn schema_version() -> u32 { 1 }
    fn cas_namespace() -> &'static str { "flow_body_hash" }
    fn serialize(value: &Self::Value, buf: &mut Vec<u8>) { /* bincode 2 */ }
    fn deserialize(buf: &[u8]) -> Option<Self::Value> { /* bincode 2 */ }
}
```

```rust
// crates/verter_session/src/cache_runtime/flow_lowered_body.rs (UPDATED)

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FlowLoweredBodyKey {
    pub canonical: CanonicalId,
    pub function_symbol: SymbolId,
    pub parse_stable_hash: ParseStableHash,
    pub body_semantic_hash: Hash16,
    pub parser_version: ParserVersion,
}

/// The lowered body payload. Defined in
/// `crates/verter_semantic/src/analysis/flow/` per the imported
/// flow-return plan's §2.
pub struct FlowLoweredBody { /* defined in verter_semantic */ }

pub struct FlowLoweredBodyNode<'h> { pub host: &'h VerterHost }

impl<'h> ArtifactNode for FlowLoweredBodyNode<'h> {
    type Key = FlowLoweredBodyKey;
    type Value = FlowLoweredBody;
    fn compute(
        &self,
        key: &Self::Key,
        snapshot: &WorldSnapshot,
        ctx: &mut ComputeCtx<'_>,
    ) -> CacheAdmission<Self::Value> {
        // This node does NOT call compute_body_semantic_hash.
        // body_semantic_hash is already part of `key` — produced by
        // FlowBodyHashNode at the caller boundary. Budget-exceeded
        // bodies never reach this lookup (the caller short-circuits
        // on `FlowBodyHashOutcome::BudgetExceeded` via
        // `SignatureAdmission::NonCacheable`).
        let body = self.host.lower_function_body(
            &key.canonical, key.function_symbol, ctx,
        );
        CacheAdmission::Cacheable(body)
    }
}

// Sealing performed by B9 — see the `mod.rs` whitelist amendment for
// `FlowLoweredBodyNode<'h>` (added in the same B11 commit that lands
// the impl below). The Block 9 whitelist test
// `persistent_implementor_whitelist::flow_body_hash_node_is_in_persistent_artifact_implementor_whitelist`
// enforces presence.
impl<'h> crate::cache_runtime::persistent::PersistentArtifactNode for FlowLoweredBodyNode<'h> {
    fn schema_version() -> u32 { 1 }
    fn cas_namespace() -> &'static str { "flow_lowered_body" }
    fn serialize(value: &Self::Value, buf: &mut Vec<u8>) { /* bincode 2 */ }
    fn deserialize(buf: &[u8]) -> Option<Self::Value> { /* bincode 2 */ }
}
```

Caller pipeline (verbatim — lives in
`crates/verter_session/src/project_semantic_dispatch/flow.rs`):

```rust
// 1. Look up the body hash through its own artifact node.
let hash_node = FlowBodyHashNode { host: &self };
let hash_key = FlowBodyHashKey {
    canonical, function_symbol, parse_stable_hash, parser_version,
};
let outcome: std::sync::Arc<FlowBodyHashOutcome> =
    cache_runtime::lookup::<FlowBodyHashNode>(&hash_node, &hash_key, &snapshot, ctx)?;

// 2. Pattern-match on the fail-closed enum. No `.unwrap()` /
//    `.expect()` — every variant is observed structurally.
let body_semantic_hash = match outcome.as_ref() {
    FlowBodyHashOutcome::BudgetExceeded => {
        // Budget-exceeded routes through Block 3's typed admission
        // gate; the caller publishes SignatureAdmission::NonCacheable
        // upward and FlowLoweredBodyNode is NEVER looked up for this
        // (canonical, function_symbol) — the key cannot be
        // constructed without the hash.
        return SignatureAdmission::NonCacheable;
    }
    FlowBodyHashOutcome::Hash(h) => *h,
};

// 3. Construct the FlowLoweredBodyKey with the pre-computed hash.
let flow_key = FlowLoweredBodyKey {
    canonical,
    function_symbol,
    parse_stable_hash,
    body_semantic_hash,
    parser_version,
};

// 4. Look up the lowered body (no body-hash re-computation; the key
//    fully identifies the body).
let flow_node = FlowLoweredBodyNode { host: &self };
let body: std::sync::Arc<FlowLoweredBody> =
    cache_runtime::lookup::<FlowLoweredBodyNode>(&flow_node, &flow_key, &snapshot, ctx)?;
```

`SemanticQueryKey::FlowReturn` is a query-node variant (Block 4
discipline). The query identity does NOT include
`body_semantic_hash`; the body identity lives on the candidate's
`ReadSetSignature.facts` via `FactKey::FlowBody`. Budget-exceeded
paths route through `SignatureAdmission::NonCacheable`, NOT a
side-channel boolean.

Flow-return persistent writes thread through Block 9's
`BaseWriteToken` AND `&WorldSnapshot` discipline. The cold-compute
publish path uses graceful overlay branching (no panic on overlay
snapshots):

```rust
if let Some(token) = snapshot.base_write_token() {
    persistent_cache.put::<FlowLoweredBodyNode, _>(
        &token, &snapshot, &flow_key, &body,
    );
}
// Overlay snapshots skip persistent admission silently. The result
// still admits to the memory-only CompileOutputNode_FactValidatedSession
// variant when the call site routes through compile output; raw
// flow-return calls under overlay return to the caller without
// persistent or base-token-bound admission.
```

Per-file changes:

- `docs/arch/native-flow-return.md` (moved from `/tmp/`).
- `crates/verter_semantic/src/analysis/flow/` — new directory; landed
  by the imported flow-return §2 plan; owns `FlowLoweredBody`.
- `crates/verter_session/src/project_semantic_dispatch/flow.rs` —
  new file; landed by the imported flow-return §2 plan; the caller
  pipeline above lives here.
- `crates/verter_session/src/cache_runtime/flow_body_hash.rs` (NEW)
  — `FlowBodyHashNode` artifact node + `FlowBodyHashKey` +
  `FlowBodyHashOutcome` + `SymbolId` / `ParseStableHash` /
  `ParserVersion`.
- `crates/verter_session/src/cache_runtime/flow_lowered_body.rs` —
  `FlowLoweredBodyNode` artifact node. `compute` body does NOT call
  `compute_body_semantic_hash`.
- `crates/verter_session/src/cache_runtime/persistent/mod.rs` —
  register BOTH `FlowBodyHashNode` AND `FlowLoweredBodyNode` as
  sealed `PersistentArtifactNode` implementors.
- `crates/verter_session/src/file_artifact_store.rs` — DELETE the
  proposed bespoke `flow_lowered_body_for(&key)` accessor; replace
  with `FlowLoweredBodyNode` via `cache_runtime::lookup`.
- `.claude/skills/type-resolution/SKILL.md` — link
  `docs/arch/native-flow-return.md` and note that flow-return is a
  cache-runtime consumer (two-node pipeline:
  `FlowBodyHashNode → FlowLoweredBodyNode`).

#### Legacy Deletions

- DELETE the proposed `FileArtifactStore::flow_lowered_body_for(&key)`
  bespoke API from the flow-return plan.
- DELETE any side-channel boolean for budget-exceeded admission;
  routing is through `CacheAdmission::ReturnOnly` (for the hash node)
  AND `SignatureAdmission::NonCacheable` (at the caller boundary
  that consumes the hash outcome) only.
- DELETE any inline `compute_body_semantic_hash` call from
  `FlowLoweredBodyNode::compute`.
- DELETE the `/tmp/verter-native-flow-return-coverage.md` working
  copy after the move.

#### Verification

```
cargo test --package verter_session flow_return --tests --verbose
cargo test --package verter_semantic flow --tests --verbose
cargo test --workspace --tests --verbose
cargo clippy --workspace -- -D warnings
cargo fmt --all
pnpm install --frozen-lockfile
```

#### Discriminating tests

- `crates/verter_session/tests/flow_return_artifact_node.rs::flow_lowered_body_uses_artifact_node_not_bespoke_store`
  — greps `crates/verter_session/src/file_artifact_store.rs` for the
  absence of `flow_lowered_body_for`. Positive: zero matches.
  Negative: a `compute_call_count` fixture validates
  `FlowLoweredBodyNode::compute` is invoked on cold miss.
- `crates/verter_session/tests/flow_return_overflow_routes_to_return_only.rs::flow_body_budget_exceeded_does_not_admit_cache`
  — synthetic body whose hash returns
  `FlowBodyHashOutcome::BudgetExceeded`. Caller receives a
  non-cacheable `CacheAdmission::ReturnOnly`-routed result; cache
  has zero `FlowLoweredBodyKey` entries; second cold call
  cold-rebuilds.
- `crates/verter_session/tests/flow_return_warm_stays_warm_on_whitespace_edit.rs::whitespace_only_function_body_edit_serves_warm_flow_return`
  — whitespace-only edit produces identical `body_semantic_hash`;
  artifact node hits the same `FlowLoweredBodyKey`. Warm serve;
  `compute_call_count` does not increment.
- `crates/verter_session/tests/flow_return_semantic_edit_cold_rebuilds.rs::semantic_body_edit_cold_rebuilds_only_affected_function`
  — edit `Foo.bar()` body; `Foo.bar`'s `FlowLoweredBodyKey`
  cold-rebuilds; `Foo.baz`'s key stays warm.
- `crates/verter_session/tests/flow_return_doc_moved.rs::flow_return_plan_lives_under_docs_arch`
  — `docs/arch/native-flow-return.md` exists;
  `/tmp/verter-native-flow-return-coverage.md` does not.
- `crates/verter_session/tests/flow_return_pre_lookup_hash.rs::flow_lowered_body_node_does_not_recompute_body_hash`
  — walks `cache_runtime/flow_lowered_body.rs` via `syn::parse_file`;
  the `compute` body contains a call to
  `self.host.lower_function_body(...)` and publishes
  `CacheAdmission::Cacheable`. The body contains ZERO calls to
  `compute_body_semantic_hash` AND ZERO calls to
  `cache_runtime::lookup::<FlowBodyHashNode>`.
- `crates/verter_session/tests/flow_body_hash_node_owns_hash_production.rs::flow_body_hash_node_compute_does_not_recurse_into_flow_lowered_body`
  — walks `cache_runtime/flow_body_hash.rs`'s `compute` body; ZERO
  calls to `cache_runtime::lookup::<FlowLoweredBodyNode>` and ZERO
  calls into `lower_function_body`. A call to
  `self.host.compute_body_semantic_hash(...)` appears exactly once;
  the return path pattern-matches on `FlowBodyHashOutcome::Hash(_)`
  → `CacheAdmission::Cacheable(...)` and
  `FlowBodyHashOutcome::BudgetExceeded` →
  `CacheAdmission::ReturnOnly(...)`.
- `crates/verter_session/tests/flow_body_hash_outcome_fail_closed.rs::flow_body_hash_outcome_has_no_invalid_representation`
  — walks `cache_runtime/flow_body_hash.rs` via `syn::parse_file`;
  asserts `FlowBodyHashOutcome` is an `enum` (NOT a `struct`) with
  exactly two variants `Hash(Hash16)` and `BudgetExceeded` (unit).
  ZERO field of type `Option<Hash16>` and ZERO field named
  `budget_exceeded: bool`. Walks every consumer of
  `FlowBodyHashOutcome`; ZERO `.unwrap()` / `.expect()` calls on any
  `FlowBodyHashOutcome` expression.
- `crates/verter_session/tests/flow_return_budget_exceeded_short_circuits.rs::budget_exceeded_hash_never_constructs_flow_lowered_body_key`
  — synthesises a body whose hash returns
  `FlowBodyHashOutcome::BudgetExceeded`. Caller in `flow.rs`
  pattern-matches and returns `SignatureAdmission::NonCacheable`
  without constructing a `FlowLoweredBodyKey` or calling
  `cache_runtime::lookup::<FlowLoweredBodyNode>`. The
  `FlowLoweredBodyNode` lookup count for the (`canonical`,
  `function_symbol`) pair stays at zero.
- `crates/verter_session/tests/flow_return_overlay_no_panic.rs::overlay_snapshot_flow_return_publish_does_not_panic`
  — `WorldSnapshot` with `overlay_identity = Some(_)`; drive the
  flow-return cold-compute publish path. The publish path observes
  `snapshot.base_write_token() == None`, takes the `None` branch,
  returns the computed body to the caller without panic, writes
  ZERO CAS files.

#### Owning-doc updates

- `.claude/skills/type-resolution/SKILL.md` — add a section pointing
  at `docs/arch/native-flow-return.md` and naming `FlowLoweredBodyNode`
  + `FlowBodyHashNode` as the cache-runtime consumers.
- CREATE `docs/arch/native-flow-return.md` (moved file).

#### Public API mirrors

Flow-return is invoked through existing
`host.resolve_named_symbol_with_audit` and the typeinfo API.

- Rust crate: `pub fn get_flow_return_type` consumed by `verter_lsp`
  and `@verter/component-meta`.
- NAPI / WASM: indirect (via existing `resolveNamedSymbol` / typeinfo
  paths).
- Protocol DTOs / TS types / JS wrappers / compat: no new schema;
  result projects into existing `TypeDescriptor`.

#### Blocks blocked by this block

- B12 (flow-return bench scenarios depend on the artifact-node
  implementation).

### 12. Benchmarks + regression gates with typed bench output schema

#### Context

The cache-runtime architecture requires benchmarks to report cache
mode, source-map policy, batch shape, thread count, hit count, and
fallback count — a benchmark without those dimensions is not an
architecture signal. Today's bench harness reports only mean/p99.
The block defines the typed `BenchResultRow` schema, the schema
validator, the numerical thresholds, the process-restart harness,
and the long-horizon TypeScript-corpus bench.

#### Changes

Schema:

```ts
interface BenchResultRow {
  scenario: string;
  cache_mode: "stateless" | "content" | "session";
  source_map_policy: "inline" | "external" | "none";
  batch_shape: { canonicals: number; duplicates: number };
  thread_count: number;
  cold_compute_count: number;
  cache_hit_count: number;
  cache_fallback_count: number;
  non_admission_count: number;
  mean_ms: number;
  p99_ms: number;
}
```

Validator `every_row_reports_required_cache_discriminators` walks
every `BenchResultRow` and asserts every field is populated (no
`undefined`, no NaN, no empty `scenario`). Missing-field rows fail
the bench.

Numerical thresholds:

- Warm `compile_many` p99 ≤ 1.10× of the pre-cutover baseline
  (recorded in
  `packages/benchmark/baselines/compile-runtime.json` as the first
  task of Block 12).
- Cold dependency-free `content`-mode compile p99 ≤ 1.05× of a
  direct `verter_compiler::compile_template` call.
- Process-restart bench:
  `pure_artifact_survives_subprocess_restart` measures the cost of
  subprocess restart + cache read; p99 ≤ 50ms for a 200-file
  corpus's `IndexedReady` retrieval.
- Long-horizon TypeScript-corpus bench: 5000-file vendored
  TypeScript-only corpus; `session`-mode `compile_many` p99 ≤
  baseline + 10%.

Bench scenarios (each row produced via the schema validator):

| Scenario | Cache mode | Source-map | Batch shape | Threads | Notes |
|---|---|---|---|---|---|
| `pure_sfc_stateless` | stateless | none | 1/0 | 1 | hottest path |
| `pure_sfc_content_cold` | content | inline | 1/0 | 1 | cold-miss content node |
| `pure_sfc_content_warm` | content | inline | 1/0 | 1 | warm-hit content node |
| `full_session_cold` | session | inline | 1/0 | 1 | with workspace alias |
| `full_session_warm` | session | inline | 1/0 | 1 | re-request after warm-up |
| `compile_many_unique_80` | session | inline | 80/0 | host | full unique batch |
| `compile_many_duplicates` | session | inline | 80/40 | host | dedup pressure |
| `compile_many_external_src` | session | inline | 80/0 | host | external src blocks |
| `component_meta_cold` | session | inline | 1/0 | host | vendored cm corpus |
| `component_meta_warm` | session | inline | 1/0 | host | re-request |
| `lsp_open_edit_hover` | session | inline | 1/0 | 1 | scripted LSP loop |
| `thundering_herd_one_query` | session | inline | 1/0 | 16 | 16 concurrent cold callers (H14) |
| `overlay_vs_base_isolation` | session | inline | 1/0 | host | overlay write does not corrupt base |
| `persistent_after_restart` | content | inline | 200/0 | host | process-restart |
| `typescript_only_corpus_5k` | session | none | 5000/0 | host | long-horizon, vendored |

External corpora: the 5000-file TypeScript corpus is **vendored**
under `packages/benchmark/corpora/typescript-5k/` (a curated subset
of the public TypeScript test suite). It is NOT pulled from
`.integration-tests/repos/<third-party>/`. The component-meta
cold/warm benches use the existing vendored cm corpus. Any future
bench against nuxt-ui / element-plus stays feature-gated by
`--features external-corpus` and is excluded from the default bench
run, matching the existing
`external_corpus_paths_not_present_outside_gated_tests` guard at
`crates/verter_session/tests/architecture_guards.rs:3457`.

`MAX_TEST_TIMEOUT` is defined in
`crates/verter_session/src/test_support/timeout.rs` as
`pub const MAX_TEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30)`.
Block 6's `compile_many_no_pool_deadlock` test references it; Block
12 lands it.

Files to create/update:

- `packages/benchmark/src/cache-runtime-bench.ts` — new bench driver
  emitting `BenchResultRow[]`.
- `packages/benchmark/src/cache-runtime-bench.spec.ts` — schema
  validator test.
- `packages/benchmark/src/apple-to-apple.ts` — adopt `BenchResultRow`;
  record threshold checks.
- `packages/benchmark/src/meta-ui-bench.ts` — adopt the schema.
- `packages/benchmark/baselines/compile-runtime.json` — pinned
  baselines.
- `packages/benchmark/corpora/typescript-5k/` — vendored corpus.
- `crates/verter_bench/examples/profile_host.rs` — extend to report
  cache discriminators.
- `crates/verter_bench/examples/profile_cache_runtime.rs` — new
  example.
- `crates/verter_session/src/cache_runtime/tests.rs` — internal
  bench smoke test.
- `crates/verter_session/tests/cache_runtime_architecture_guards.rs`
  — centralized arch-guard for cache-runtime invariants.
- `crates/verter_session/tests/restart_in_subprocess.rs` — implement
  `TestHarness::restart_in_subprocess()`.
- `crates/verter_session/src/test_support/timeout.rs` — `MAX_TEST_TIMEOUT`.

#### Legacy Deletions

- DELETE bench output paths that emit only mean/p99 without cache
  discriminators.
- DELETE any bench scenario that pulls from
  `.integration-tests/repos/<third-party>/` outside the
  `external-corpus` feature gate.

#### Verification

```
pnpm --filter @verter/benchmark test
pnpm --filter @verter/benchmark exec tsx packages/benchmark/src/cache-runtime-bench.ts
pnpm --filter @verter/benchmark exec tsx packages/benchmark/src/apple-to-apple.ts
cargo run --package verter_bench --example profile_host --release --features hotpath
cargo run --package verter_bench --example profile_cache_runtime --release --features hotpath
cargo test --workspace --tests --verbose
cargo clippy --workspace -- -D warnings
cargo fmt --all
pnpm install --frozen-lockfile
```

#### Discriminating tests

- `packages/benchmark/src/cache-runtime-bench.spec.ts::every_row_reports_required_cache_discriminators`
  — fails any row missing required fields. A synthetic row with
  `cold_compute_count: undefined` fails.
- `packages/benchmark/src/cache-runtime-bench.spec.ts::warm_compile_many_p99_within_baseline_threshold`
  — warm `compile_many` p99 ≤ 1.10× baseline. A synthetic injection
  that artificially slows the warm path is detected.
- `packages/benchmark/src/cache-runtime-bench.spec.ts::cold_content_p99_within_direct_compiler_threshold`
  — cold content-mode p99 ≤ 1.05× direct `verter_compiler` call.
- `crates/verter_session/tests/thundering_herd_one_query.rs::sixteen_concurrent_cold_callers_compute_once`
  — `cold_compute_count == 1` across 16 concurrent cold callers
  (B2's H14 singleflight guard exercised end-to-end through the
  cache-runtime substrate).
- `crates/verter_session/tests/restart_in_subprocess.rs::restart_helper_exists_and_round_trips_arc_value`
  — `TestHarness::restart_in_subprocess()` returns a
  `RestartedHandle` that lets a follow-up test read back a
  previously persisted artifact.

#### Owning-doc updates

- `.claude/skills/build-and-profiling/SKILL.md` — append
  `## Cache bench schema + numerical thresholds` documenting the
  `BenchResultRow` interface verbatim, the long-horizon vendored
  TypeScript-only corpus, and the four numerical thresholds.
- `.claude/skills/architecture/SKILL.md` — append a one-line pointer
  to the new build-and-profiling skill section.

#### Public API mirrors

Bench infrastructure does not cross any public binding surface.

#### Blocks blocked by this block

- None. Block 12 is the final gate.

## Legacy Deletions (cross-block summary)

The per-block `#### Legacy Deletions` subsections are authoritative
for per-block work. The list below is a cross-block summary the
implementer verifies against after the cutover lands:

- `crates/verter_session/src/cooperative_admission.rs` (file renamed
  into `cache_runtime/singleflight.rs`) — B2.
- `crates/verter_session/src/cooperative_admission_tests.rs` (file
  renamed into `cache_runtime/singleflight_tests.rs`) — B2.
- `finalise_signature_or_empty` and every helper converting overflow
  to an empty cacheable signature — B3.
- Direct `Arc::from(Vec::<FactVersionRef>::new())` constructions
  outside `ReadSetSignature::{empty, overflow}` — B3.
- `DeclIdentity` as a key field on `SemanticQueryKey::Instantiate`
  and `ResolveMacroPayload` — B4 / B8.
- Bespoke `clear_*_cache(canonical)` helpers on query-identity
  caches — B4.
- The ambiguous single compile entry-point that hid mode selection —
  B5.
- Per-call `rayon::ThreadPoolBuilder::new()...build()` in
  `compile_many` (`crates/verter_session/src/host_compile.rs:143-147`)
  — B6.
- Per-file submit-and-wait scheduler loops inside batch compilation
  — B6.
- Unconditional compile-tier `ensure_indexed_ready` prefetch for
  dependency-free compile output — B6.
- Unconditional external type collection setup when `macro_type_deps`
  is empty — B6.
- `Scheduler.cpu_pool: rayon::ThreadPool` field (substrate
  `scheduler.rs:135`) — B7.
- Per-call CPU pool construction inside
  `Scheduler::with_executor` (substrate `:183-187`) — B7.
- `Scheduler::submit_batch`'s loop body (substrate `:312`) — replaced
  by no-edge-DAG bridge over `try_submit_dag` — B7.
- `JobIndex`, `QueueEntry`, `EffectiveKey`, `AgingConfig` (+ the
  `SchedulerConfig.aging` field), and the `Scheduler.job_index:
  Mutex<JobIndex>` field — replaced by the driver-owned `SchedulerDag`
  + scan-free priority lanes (DECISION 1) — B7.
- `BlockerRegistry`, `BlockerRef`, `UnblockedJob`,
  `has_pending_blockers`, `Submission::BlockerResolved`, and the
  `Scheduler.deferred_blocker_ids` map — replaced by `SchedulerDag`
  out-edge resolution (DECISION 1) — B7.
- File-stage ordering through `FileNode.pending_requests` — replaced
  by `SchedulerDagNode.waiters` join (DECISION 1) — B7.
- The non-generic `SubmissionResult` enum and the submitter-side
  `ArrayQueue::push` retry loop + `std::thread::yield_now()` spin +
  submitter-held `DagState.readiness.lock()` in `submit_dag` —
  replaced by typed `SubmissionResult<T>` admission + condvar
  (DECISION 2) — B7.
- Direct host-owned result maps for compile, resolved type, eval
  env, and semantic DB cache state — B8.
- `clear_compile_cache`, `notify_close`, `set_workspace`,
  `configure_projects` bespoke per-call invalidation lists — B8.
- Reverse-dependent eviction loops used as correctness authority —
  B8.
- Any persistent semantic-query cache admission before the semantic
  query audit — B9.
- Legacy `FileArtifactKey::legacy` / `overlay_scoped` sentinel
  patterns — B9.
- Audit event payload allocation before checking whether a request
  accumulator exists — B10.
- `StructuredAuditEvent::Custom` escape hatch on cache subsystems —
  B10.
- `/tmp/verter-native-flow-return-coverage.md` (moved to
  `docs/arch/native-flow-return.md`) — B11.
- The proposed `FileArtifactStore::flow_lowered_body_for(&key)`
  bespoke accessor — B11.
- Bench output paths emitting only mean/p99 without cache
  discriminators — B12.

## Verification (cross-block)

The per-block `#### Verification` subsections are authoritative for
per-block work. The list below is the cross-block end-of-change
verification the implementer runs after every block lands:

```bash
cargo test --workspace --tests --verbose
cargo clippy --workspace -- -D warnings
cargo fmt --all
pnpm install --frozen-lockfile
```

After all blocks land, run the full workspace + native + benches:

```bash
cargo test --workspace --tests --verbose
pnpm test
pnpm run build:native
pnpm run build:ts
pnpm --filter @verter/native test
pnpm --filter @verter/benchmark test
pnpm --filter @verter/benchmark exec tsx packages/benchmark/src/apple-to-apple.ts
pnpm --filter @verter/benchmark exec tsx packages/benchmark/src/cache-runtime-bench.ts
cargo run --package verter_bench --example profile_host --release --features hotpath
cargo run --package verter_bench --example profile_cache_runtime --release --features hotpath
node scripts/benchmark/trace-component-corpus.mjs \
  --output-dir=tmp/cm-cache-runtime \
  --no-trace
npx tsx packages/benchmark/src/trace-check.ts \
  tmp/cm-cache-runtime \
  --strict \
  --check-expected
```

Expected outcomes:

- All workspace Rust tests pass.
- No architecture guard reports an unguarded critical cache rule.
- Scheduler single-readiness-authority guards pass (DECISION 1):
  `scheduler_has_single_readiness_authority`,
  `scheduler_dispatch_path_no_linear_job_scan`,
  `blocker_resolution_touches_only_out_edges`,
  `dynamic_import_edges_added_before_downstream_dispatch`,
  `stale_generation_nodes_never_dispatch_after_bump`,
  `same_stage_requests_join_one_work_node`. `JobIndex` /
  `BlockerRegistry` are absent from every non-test scheduler path
  (STOP-gate).
- Scheduler admission-backpressure guards pass (DECISION 2):
  `submit_dag_backpressure_is_typed_before_readiness_mutation`,
  `dag_submission_never_spins_or_holds_readiness_lock`,
  `scheduler_submission_paths_do_not_call_yield_now`,
  `blocking_submission_parks_on_admission_condvar`. No
  `std::thread::yield_now()` exists on any submission path.
- Scheduler pool-submission guards pass (H23):
  `driver_never_blocks_on_io_pool_send`,
  `driver_never_blocks_on_cpu_pool_submit`,
  `pool_capacity_reserved_before_ready_seed`. No driver path can block
  on worker-pool queue pressure.
- Scheduler critical-risk guards pass:
  `dag_capacity_reservation_is_single_accounting_source`,
  `capacity_reservation_releases_exactly_once_on_completion_cancel_panic_shutdown`,
  `pool_permit_and_dag_budget_cannot_double_release`,
  `deferred_lane_is_bounded_by_admitted_capacity`,
  `deferred_lane_eventually_runs_under_sustained_cpu_saturation`,
  `critical_ready_work_not_starved_by_deferred_background_work`,
  `scheduler_model_random_dags_preserve_readiness_invariants`,
  `scheduler_model_capacity_returns_to_zero_at_quiescence`,
  `scheduler_model_driver_never_blocks_under_seeded_pool_failures`.
- `cache-runtime-bench.spec.ts::every_row_reports_required_cache_discriminators`
  passes for every emitted row.
- Cache-mode output equivalence holds for stateless/content/session
  where their semantics overlap.
- `ReturnOnly` values are returned to callers but never admitted to
  a cache map.
- Persistent pure artifacts survive process restart and reject
  mismatched env hashes.
- Session overlays cannot populate base-only cache entries (sealed
  `BaseWriteToken` gate).
- Batch compile preserves output order and per-file diagnostics.
- Benchmark output shows cold dependency-free `content` mode avoids
  the full session overhead while `session` mode preserves semantic
  correctness.
- `pnpm install --frozen-lockfile` exits clean (CI parity).
- `cargo clippy --workspace -- -D warnings` exits clean.
- `cargo fmt --all -- --check` is a no-op.
