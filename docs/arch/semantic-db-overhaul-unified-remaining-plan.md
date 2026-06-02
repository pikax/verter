# Semantic-DB-Overhaul — Unified Remaining-Work Plan

This is the single authoritative plan for the **remaining** work across the
two `refactor/semantic-db-overhaul` tracks: the **cache-runtime / scheduler**
track (`cache-runtime-overhaul-plan.md`) and the **semantic-type-graph** track
(`semantic-type-graph-plan-recovered.md`). It merges and sequences the
remaining items of both plans into one ordered backlog of 16 blocks (`U0`–`U15`)
and is the doc an orchestrator drives block-by-block.

It is composed from a binding codex merge decision (§A unified sequenced
backlog, §B Block-4 ↔ `SemanticQueryKey` co-sequencing, §C
`TypeInfoGraphResultDb` admission fork, §D MOOT adoptions, §E doc structure)
plus the two per-track landed-vs-remaining analyses. **This document SUPERSEDES
the two original plans for all REMAINING work.** The originals stay as
historical / per-item detail reference only.

---

## 1. Landed-context preamble

**Current tip:** `b36e0835` on `refactor/semantic-db-overhaul` (== the B7c land
tip; clean working tree).

### What is LANDED

**Cache-runtime / scheduler track — Blocks 1–6 FULLY LANDED, B7a/b/c landed:**

- **B1** — `WorldSnapshot` request-identity type (never enters a cache key) +
  plan-vocabulary guard (H19 `no_phase_archaeology_in_production_code`).
- **B2** — `cache_runtime/` substrate: `ArtifactNode` / `QueryNode` traits +
  `CacheAdmission<V>` + `cache_runtime::lookup`. The legacy
  `cooperative_admission` module is GONE; `singleflight.rs` +
  `cooperative_admit_with_post_publish` + `ComputeAdmission` are the H14
  singleflight substrate.
- **B3** — typed `SignatureAdmission::{Cacheable, NonCacheable}` in
  `fact_signature_helpers.rs`; `ReadSetSignature.overflowed: bool` carrier;
  `NonAdmissionReason` re-exported from `verter_audit`.
- **B5** — public `CompileCacheMode` (`Stateless` / `Content` / `Session`) +
  `classify_compile_mode` wired at `virtual_file_pipeline.rs`; typed
  `DowngradeReason` / `SourceMapPolicy`.
- **B6 — Phase 6 COMPLETE** — `HostCpuPool` (§6a), `submit_batch_atomic` (§6b),
  `upsert_many_with_priority` atomic-upsert cutover (§6c, `host_upsert.rs`),
  `HostBatchCoordinator` (deadlock fix), §6d∪§6e finalization. The per-call
  `CompileBatchOptions.threads` option was REMOVED outright (concurrency is now
  construction-time `HostConfig::host_cpu_threads`).
- **§7 / §7b — unified `SchedulerDag`** (single readiness/admission/reservation
  authority, H21): 3-variant `WorkNodeIdentity`, 5-variant
  `WorkKind{Load, Parse, Analysis, Artifact, CacheNode}`, cooperative
  pump. `queue.rs` / `JobIndex` / `BlockerRegistry` /
  `Submission::BlockerResolved` DELETED.
- **B7a** — leaf substrate (additive, unwired): `cache_id.rs` (opaque
  `SchedulerCacheId(pub u64)`), `cancellation.rs`, `cpu_concurrency.rs`
  (`CpuConcurrencySemaphore` + `CpuConcurrencyPermit`), `dedupe_hook.rs`
  (`DedupeHook`), `SubmissionResult<T>`.
- **B7b** — DAG readiness lanes + weighted credit (4 priority lanes,
  deficit/credit replacing the linear scan). `DagAgingConfig` /
  `effective_priority` DELETED. `DagCapacityBudget` is the SOLE ledger.
- **B7c** — pool topology: injected `Arc<SchedulerCpuPool>` /
  `Arc<SchedulerIoPool>` + nonblocking `try_submit`; 3-pool topology
  (HostCpuPool=External, SchedulerCpuPool=CpuWorker, SchedulerIoPool=IoWorker).
  `IoPool` / `IoHandle` / blocking `execute` DELETED.

**Semantic-type-graph track — A0a CONTRACT substrate LANDED on a pre-existing
`SemanticGraphStore` foundation:**

- The **`SemanticGraphStore` / `ProjectSemanticDispatch` / `Relate`-memo**
  foundation and the **five query modes** (`Identity` / `Navigate` / `Shallow` /
  `Expanded` / `Skeleton`) pre-exist and are the SOLE query-time type resolver.
- **A0a — typeinfo wire + audit CONTRACT substrate:**
  `crates/verter_protocol/proto/verter/v1/typeinfo.proto` rewritten
  (`GraphTypeNode` 32-arm oneof, `StructuredTypeExpression` 22 arms,
  `TypeInfoGraphRequest` 7 arms, `TypeInfoRequestError` 11 variants,
  `FrameworkSurfacePayload`, capability handshake, reserved field directives);
  `verter_protocol/src/typeinfo/graph.rs` (Rust re-exports, `Graph*`-prefixed);
  `verter_audit` extended with `RequestKind::TypeInfoGraph`=9 +
  `RequestKindPayload` + closed tag enums + 3 `StructuredAuditEvent` variants +
  `KindBit::TypeInfoGraph` + batch aggregator + regenerated `audit.generated.ts`;
  `request_validation.rs` shape-only validator (closed schema-version gate +
  exhaustive structured-expression coverage); the wire-contract guards
  (taxonomy parity, byte-equal TS freshness, audit parity, request validation);
  the `Typeinfo Wire Contract` CRITICAL rule registered in `CRITICAL_RULE_GUARDS`.
- **reconcile-#5 (CF-2 reviewed-clean):** the audit footprint payload was
  reshaped from 9× `exactness_*: u32` scalar fields to one
  `exactness_counts: BTreeMap<ExactnessTag, u32>` map field.

### Known-failure baseline at the tip

The tree carries a **stable 8-failure baseline** at `b36e0835`, all
long-standing and **OUT OF SCOPE for this unified plan** (they are a Block-1.A
`fact_dep_signature` substrate migration cluster carried forward since
reconcile-#2/#3):

- `compile_tier_signature_carries_*` ×5
  (`member`, `member_presence`, `import_ref`, `route_surface`,
  `module_augmentation_index_shape`)
- `family_a_fact_validation` ×2 (`family_a_entries_carry_fact_dep_signature`,
  `family_a_warm_hit_uses_fact_validation`)
- `materialise_structure_entry_carries_dep_signature` ×1

Plus one **environment-only** non-failure: `typeinfo_ts_bindings_*` fails in a
`node_modules`-less worktree (it regenerates TS bindings via the workspace `buf`
binary) and **PASSES on the main checkout with `node_modules` present** /
post-`pnpm install`. It is not a code failure.

Every block's verification expects this exact baseline and zero NEW failures.

---

## 2. Binding decisions & MOOT adoptions

These are transcribed from the codex merge decision (§C, §D). They are binding
on every block; do not relitigate or resurrect superseded items.

### 2.1 `TypeInfoGraphResultDb` admission fork (§C)

**Decision: singleflight NOW, with NO later retarget to `submit_dag`.**

`TypeInfoGraphResultDb` admission belongs to the **cache-runtime
singleflight / fact-validation substrate**, specifically:

- `cooperative_admit_with_post_publish` (`cache_runtime/singleflight.rs`)
- `InflightTable` (`cache_runtime/singleflight.rs:213`)
- `BoundedCandidateMap` (`bounded_query_retention`)
- `FactReadSet::finalise` → `FactReadSetFinalise`
- `SignatureAdmission` (via `SignatureAdmission::from_finalise`)

B7e `submit_dag` is **scheduler execution / readiness plumbing, not a second
cache-admission authority**. The typeinfo DB must NOT be folded into the
`CacheNodeDag` / `submit_dag` design now or later. This resolves the single
biggest cross-track sequencing question: the semantic-graph execution layer
binds to the already-landed singleflight substrate and is therefore largely
PARALLEL to the remaining scheduler work (U1/U7/U9).

### 2.2 MOOT adoptions (§D — do NOT resurrect)

- **Adopt the A0a `Graph*` wire names** (`GraphTypeNode`, `GraphMergedDeclaration`,
  …) and the **single `TypeInfoGraphRequest` envelope** (7-arm oneof). The
  plan's bare-`TypeNode` / per-request-message proto naming is superseded.
  `TypeInfoRequestError` is 11 variants (A0a), not 8.
  `PredicateSubjectIdentifier` → `PredicateSubjectName` rename is landed.
- **Use `exactness_counts: BTreeMap<ExactnessTag, u32>`** (reconcile-#5 / CF-2).
  The `exactness_*: u32` scalar field list (§10 / A.17 of the semantic-graph
  plan) is MOOT.
- **Build the typeinfo DB / fence (U10) on `FactReadSet::finalise` +
  `SignatureAdmission` + `HostStoreView`.** Do NOT resurrect the retired
  `finalise_signature_or_empty` helper (it collapsed Overflow→Empty, a
  correctness defect) nor the retired request-view globals (`RequestStoreView`,
  `CURRENT_REQUEST_VIEW`, `_in_view`). The plan's "live `StoreView`" maps onto
  `HostStoreView` directly.
- **Keep the opaque `SchedulerCacheId(u64)` newtype.** Do NOT make it an enum
  (an enum leaks session cache-family semantics into the scheduler).
- **Do NOT build `DagAdmissionBudget`.** `DagCapacityBudget` /
  `DagCapacityReservation` is the single ledger.
- **Use `ShapeCacheDb`** (keyed by `ShapeSubject::SemanticNode`), NOT the retired
  split `MaterializeMemoDb` / `MemberShapeCacheDb` shape caches. The static guard
  `block_6i_static_guards.rs::shape_cache_db_replaces_split_caches` forbids
  re-introduction.
- **Treat the `DeclKey` whole-hash fix as ALREADY LANDED.** `Instantiate.base` /
  `ResolveMacroPayload.owner` already carry a content-free
  `DeclKey { canonical_id, decl_name }` (via §6c / A0a / reconcile-#4
  `to_decl_key()`). Only the further **slot-identity** refinement
  (`→ ResolvedDeclSlotIdentity`) remains, and it lands in U2. The R6 whole-hash
  violation is resolved.
- **Do NOT resurrect** `queue.rs`, `submit_batch` (the non-atomic one — 0 callers,
  deleted in §6c), `JobIndex`, `QueueEntry`, `EffectiveKey`,
  `AgingConfig`/`DagAgingConfig`, `BlockerRegistry`, `BlockerRef`,
  `Submission::BlockerResolved`, `FileNode.pending_requests`, per-call `threads`,
  or scheduler enum cache IDs. All were deleted by §7 / §6c / B7b.
- **`SubstitutionConcrete`** (semantic-graph §4.1.1) is superseded within its own
  plan by `CanonicalSubstitutionValueKey` (A.10). If U10 builds substitution keys,
  use the A.10 carrier.
- **`FlowNarrowing` / `ContextualType` placement:** A.11 (the later, authoritative
  revision) moves them OUT of `TypeNode` into a sibling `ProgramAnalysisGraph`
  (guards `type_node_contains_only_type_values` +
  `program_analysis_graph_gated_by_projection_required`). A0a's proto currently
  encodes the `GraphFlowNarrowing` / `GraphContextualType` arms INSIDE
  `GraphTypeNode` (`typeinfo.proto:206-207`) — the OLD placement. **U8 performs the
  wire move** (re-home the two arms under `ProgramAnalysisGraph`, `reserved` the
  vacated `GraphTypeNode` tags 26/27, bump `SemanticTypeGraph.schema_version`) and
  produces the `TypeInfoGraphPayload { graph, program_analysis }` shape. Do NOT
  keep the A0a inside-`GraphTypeNode` placement.
- **The legacy `evaluate_type_expression.rs` scratch-file evaluator is NOT a
  sanctioned 2nd `parse_type_annotation` exception.** It is the text-evaluator
  deleted in U12 once `StructuredTypeExpression` dispatch (U11) lands. §11 of the
  semantic-graph plan confirms no 2nd exception.

---

## 3. Cross-track dependency map

The two tracks are **largely PARALLEL**. The key structural fact (§C):
the semantic-graph EXECUTION layer (U8, U10, U11, …) binds to the
**already-landed singleflight / fact-signature substrate**, NOT to the unbuilt
scheduler cache-node DAG (`submit_dag`). So scheduler work (U1 → U7 → U9) and
semantic-graph execution (U8 → U10 → … → U15) proceed on **dependency-parallel
lanes** after the one convergence gate.

```
   U0 (semantic) ─────────┐  (U2 DEPENDS on U0; U0 lands FIRST)
   [NEXT]                  ▼
                      ┌─────────────────────────────────────────────┐
                      │  U2 = CONVERGENCE GATE                       │
   U1 (B7d, scheduler)│  (SemanticQueryKey identity shape + B4       │
   [∥ U0 and U2;      │   cache-node enumeration; highest            │
    scheduler-only] ──┤   correctness risk; one clean cutover)       │
                      └───────────────────┬─────────────────────────┘
                                          │
        ┌─────────────────────────────────┼──────────────────────────────┐
        │ CACHE-RUNTIME lane               │ SEMANTIC-GRAPH lane            │
        │                                  │                                │
   U3 (B8 invalidation) ──► depends U2     U8 (exporter) ──► depends U2     │
   U4 (B9 persistent)   ──► depends U2       (+U6 only for flow consumers)  │
   U5 (B10 mem/audit)   ──► B2, best ≥U2/U3  │                              │
   U6 (B11 flow-return) ──► depends U2,U4    U10 (DB/fence/QueryErr) ──► U8  │
   U7 (B7e submit_dag)  ──► depends U1       U11 (TypeInfoSession)   ──► U10 │
        (parallel after U1)                 U12 (FFI/TS + deletions)──► U11 │
   U9 (B7f session bridge)──► depends U7     U13 (TS session+proj)   ──► U12 │
        │                                    U14 (Vue adapter)       ──► U13 │
        └────────────────────────────────►  U15 (integration/bench/lift) ◄──┘
                                                depends on all code blocks
```

**The one hard coupling = U2** (the `SemanticQueryKey` reshape + B4 cache-node
enumeration, merged per §B). It must land before graph execution (U8+) because
the exporter dispatches the final-shape `SemanticQueryKey` variants AND because
B4 enumerates the semantic-track-owned caches (`SemanticGraphStore`,
`ComponentMetaResultDb`, `MaterializeStructureDb`, `RefCycleResultDb`,
`ShapeCacheDb`) onto the `QueryNode` substrate — doing those twice (add variants
on `DeclKey`, then re-key to slot identity) is forbidden.

**Lower-grade shared edges (additive, no hard sequencing):**

- **Audit substrate** (`verter_audit` leaf) is shared. A0a already added the
  typeinfo arms; U5 (cache-node audit events) and U5's `StructuredAuditEvent`
  additions are additive under closed-enum discipline. Land any new
  `StructuredAuditEvent` variants through one coordinated `structured_event.rs`
  edit to avoid enum-variant churn / regen races.
- **Env-hash / R21 split** is a shared invariant; already landed; no sequencing.
- **Batch / host-pool coupling** is already satisfied by `HostBatchCoordinator`
  + `HostCpuPool` (§6a). A batch typeinfo session (if ever) routes through them;
  single-request typeinfo needs no new coupling.

---

## 4. Unified block backlog (U0–U15)

Drive ONE block at a time: implement → triple review (independent reviewer +
codex) → per-block fix cycle until clean → land. Each block uses the template:
**ID / source track / scope / deps / parallelism / risk / required deletions /
guards**. Sequence is faithful to §A; do not reorder.

---

### U0 — Finish Typeinfo Contract Gaps  **(NEXT primary block)**

- **Source track:** semantic-graph (R-0a).
- **Scope:** Close the contract gaps A0a left.
  - Add the `AuditedResult<T, E>` carrier in **`crates/verter_audit/src/audited_result.rs`**
    (NOT `verter_protocol`: it is generic over `T`/`E`, which protobuf cannot
    express, and embeds the audit-substrate `RequestAuditRecord`, so it rides the
    ts-rs export into `audit.generated.ts` rather than forcing a dependency
    inversion). It does not exist anywhere in the tree (genuinely net-new, 0 hits).
    `packages/typeinfo` imports the generated TS type; there is no hand-written
    mirror.
  - **Unignore-manifest — EXTEND/RECONCILE the A0a-landed manifest, do NOT create a
    second one.** A0a already landed the manifest as a Rust test, not a doc:
    `crates/verter_session/tests/typeinfo_ignored_test_manifest.rs` +
    `tests/manifest_data/typeinfo_ignored_test_manifest_rows.rs` (363 rows, schema
    `IgnoredTestRow { file, function, substrate: TargetSubstrate, unblocker }`,
    `EXPECTED_TOTAL_IGNORED_COUNT = 363`), with ~10 backing guards. U0 EXTENDS this
    landed manifest + its guards to cover any new Phase-0a contract scope. The
    schema is already the intended one — keep the landed `substrate` /
    `unblocker` columns (do NOT introduce a competing `target_phase` /
    `target_substrate` / `unblocked_by` schema at a different path). There is no
    separate `typeinfo_tests_unignore_plan.md` doc — the manifest IS the `.rs`
    test, so the reconciliation is a no-op schema confirm.
  - Add the remaining Phase-0a static guards not covered by A0a (symbol-node
    invariants, origin-edge taxonomy, closure-bound, substitution-canonicalisation,
    request-uniformity, the schema-version closed-surface pins) — see the Guards
    list below for the exhaustive set. The manifest's own backing guards stay green
    under the reconciled schema (they are not net-new; they extend in place).
  - Adopt / verify the landed A0a wire naming (`GraphTypeNode` /
    `TypeInfoGraphRequest`) as canonical (§2.2).
- **Deps:** A0a (landed). No cross-track dep.
- **Parallelism:** Runs beside U1 (B7d) only. U2 DEPENDS on U0 and starts only
  after U0 lands (U2 is the convergence gate).
- **Risk:** small-medium (the `AuditedResult` carrier + additive guards + a
  manifest reconciliation/rename of the A0a-landed test).
- **Required deletions:** none net-new for the contract surface, BUT the
  unignore-manifest is a reconciliation, not a fresh add: do NOT create a duplicate
  manifest at a second path with a competing schema — the A0a-landed
  `typeinfo_ignored_test_manifest.rs` + its `manifest_data/` rows + its backing
  guards stay in place under the already-landed `substrate` / `unblocker`
  schema (a no-op confirm; do NOT migrate to a competing schema). `AuditedResult`
  is genuinely net-new.
- **Guards (the exhaustive Phase-0a remaining set per §R-0a / §8.0 / §A.23 — no
  silent "subset + etc."):**
  - **Dependency / projection:** `dependency_direction_one_way` (the typeinfo
    layering is one-way, machine-checked); `path_projection_mode_cascade`
    (intermediate hops `Navigate`, terminal hop the caller's mode);
    `typeinfo_request_validates_mode_present`.
  - **SymbolNode invariants:** `symbol_node_preserves_type_value_namespace_spaces`
    (`SymbolSpace = Type | Value | Namespace`, `BothTypeValue` forbidden);
    `class_dual_space_emits_two_symbols`;
    `symbol_node_preserves_resolved_decl_slot_identity` (content-free slot
    identity, not `(canonical_id, name, symbol_space)`).
  - **Origin-edge taxonomy:** `origin_edge_taxonomy_locked` (the three
    `OriginEdgeKind` enums — `verter_session` 9, `verter_audit` 10, `verter_protocol`
    10 — are pairwise consistent, differing by exactly `SharedLoadReuse`).
  - **Schema-version (contract gate already landed; these pin the closed surface):**
    `every_typeinfo_request_carries_schema_version`;
    `unknown_schema_version_shape_uniform_across_plan`;
    `typeinfo_request_error_union_is_consistent_across_sections`. (The schema-version
    RUNTIME encoders/negotiation land in U11/U12.)
  - **Request-uniformity:** `every_typeinfo_request_carries_context_or_is_exempted_with_rationale`
    (seven entry-points carry `context + closure + displayPolicy`; `listSymbols` /
    `relate` are the two NAMED exemptions); `list_symbols_is_scalar`;
    `relate_has_no_closure_field`; `every_closure_variant_has_concrete_resource_bound`.
  - **Closure-bound / substitution-canonicalisation:** the closure-bound guard and
    the substitution-canonicalisation guard (`literal_value_key_is_independent_of_wire_string_table`,
    `cycle_id_propagates_canonicalization_conflicts`).
  - **Unignore-manifest (reconciled with the A0a-landed manifest — see below):**
    the manifest's existing backing guards (`every_ignored_typeinfo_test_has_a_manifest_row`,
    `every_manifest_row_corresponds_to_a_live_ignored_test`,
    `every_manifest_row_has_non_empty_unblocker`,
    `every_manifest_row_unblocker_matches_live_ignore_reason`,
    `every_manifest_row_lists_a_valid_substrate`,
    `total_ignored_typeinfo_test_count_matches_expected`,
    `manifest_length_matches_documented_total`,
    `per_file_ignored_test_counts_match_manifest`, plus the two reason-quality
    guards), kept green under the reconciled schema.

  Plus the broader §8.0 registry "etc." entries scoped to Phase 0a
  (`r21_*` for typeinfo, `node_taxonomy_complete`, `diagnostics_only_on_typeinfo_graph_payload`,
  `proto_closed_enums_declared_not_raw_uint32`, `proto_no_duplicate_enum_declarations`,
  `wire_dtos_generated_only_from_proto`, `ts_rs_not_applied_to_wire_dtos`,
  `part_a_carries_no_phase_archaeology`). All must be discriminating (fail against
  the pre-change tree).

---

### U1 — Scheduler Dispatch Split (B7d)

- **Source track:** cache-runtime / scheduler.
- **Scope:** Bring the dispatch enum into parity with the landed DAG-layer
  `WorkKind`. Split `TaskKind::Source` → `Load` + `Parse`; add
  `TaskKind::CacheNode { cache_id, key_hash }`. Drop `Copy/Eq/Hash` from
  `TaskKind`, add `TargetStage: Hash`. Add `StageExecutor::as_any` +
  `execute_cache_node` + `dispatch_cpu_task`. Replace the
  `task_kind_for_ready_job` adapter's `unreachable!()` CacheNode arm
  (`scheduler.rs:146`) with a real dispatch arm.
- **Deps:** B7a/b/c (landed).
- **Parallelism:** Can run parallel with U0 and U2 (scheduler-only surface).
- **Risk:** medium — touches the per-task dispatch chokepoint; must not create a
  second dispatch path.
- **Required deletions:** the `task_kind_for_ready_job` adapter (its
  `unreachable!()` CacheNode arm is the placeholder this block replaces).
  `SchedulerJobKind` (component-meta batch fan-out) is **RETAINED** and must not
  alias `TaskKind`.
- **Guards:** keep `dag_arch_guards` (12/12) green; a discriminating test that
  the CacheNode dispatch arm actually routes (not `unreachable!()`); a guard that
  there is exactly one dispatch path (no second `task_kind_for_ready_job`-style
  adapter re-introduced).

---

### U2 — Semantic Key + B4 Cache-Node Convergence  **(CONVERGENCE GATE — highest correctness risk)**

- **Source track:** MERGED (semantic-graph R-1 + cache-runtime B4-completion).
  See the dedicated co-sequencing section (§5).
- **Scope (ONE clean cutover — no migrate-twice):**
  1. Finalize the **`SemanticQueryKey` identity SHAPE once** (the slot-identity
     model for every variant): migrate existing `Instantiate { base }` /
     `ResolveMacroPayload { owner }` from `DeclKey` → `ResolvedDeclSlotIdentity`
     (slot identity), AND add the **7 new variants** in that identity shape
     (`ResolveMergedDeclaration`, `ResolveModuleAugmentation`,
     `ResolveAmbientNamespace`, `ResolveOverloadSet`, `ResolveEnum`,
     `FlowNarrowingAt`, `ContextualTypeAt`). Every variant routes through
     `ProjectSemanticDispatch::execute` (the one-engine rule). This finalizes the
     identity SHAPE — later ADDITIVE variants land in that same slot-identity shape
     with NO cache re-key (notably U6's `SemanticQueryKey::FlowReturn`, B11); adding
     a later variant is additive, not a second migration.
  2. Add the matching `SemanticNodeData::{MergedDeclaration, ModuleAugmentation,
     AmbientNamespace, Class, Enum}` producers in `verter_semantic::analysis`
     (namespace / merge / class analysis) + `verter_session::semantic_query`.
     **Cross-file declaration merging lands HERE** (not deferred) per §9.5;
     `ResolveModuleAugmentation` rides `FileArtifactStore::augmentation_index`
     (landed).
  3. Enumerate the remaining B4 caches onto `ArtifactNode` / `QueryNode` against
     that same final key model: `FileArtifactStore`, `ResolvedImportFacts`,
     typed-IR resolve, `MemberSemanticFactStore`, `MemberDisplayFactStore`,
     `ModuleAugmentationIndex`, `RouteDb` (×3) + `RouteOwnedShallowDb`,
     `TypeResolutionContextDb`, `EvalEnvCacheDb`, `DependencyCacheDb`,
     `SemanticGraphStore`, `ComponentMetaResultDb`, `MaterializeStructureDb`,
     `RefCycleResultDb`, `ShapeCacheDb`, `AnalysisReadyDb`,
     `OwnerImportSurfaceDb`, `ImportedRootDb`, `AppConfigNoOverrideProofDb`,
     `ResolvedTypeCacheDb`. Add the supporting key/value types
     (`FileArtifactKey`, `ResolvedImportFactsKey`, `CompileOutputKey`,
     `CompileOutputSlotKey`, `AnalysisSlotKey`, `AnalysisCandidate`,
     `ResolvedDeclSlotIdentity`).
- **Deps:** U0; B2 + B3 (landed); pre-existing `SemanticGraphStore` /
  `ProjectSemanticDispatch` / `execute_cooperative` (landed).
- **Parallelism:** U1 (B7d) may run beside it. NOTHING downstream of the gate
  (U3, U8+) starts until U2 lands.
- **Risk:** **very large / highest correctness risk** — touches the one shared
  resolver. Declaration merging + ambient + augmentation are the hardest
  TS-fidelity cases; cross-file merge completeness (§9.5 five properties) is a
  known hard sub-item.
- **Required deletions:** none of substance at this block — but it FORBIDS adding
  the 7 variants on the `DeclKey` shape and re-keying later (that double-migration
  is the anti-pattern §B exists to prevent). Use `ShapeCacheDb`, never the retired
  split shape caches. The `DeclKey` whole-hash fix is already landed (§2.2).
- **Guards:** an H3 runtime guard
  (`cache_key_runtime_guards::semantic_query_keys_contain_no_content_hash_or_fact_signature`
  / equivalent) — query-identity keys carry no content/version hash or
  `fact_dep_signature`; a guard that every `SemanticQueryKey` variant dispatches
  through `ProjectSemanticDispatch::execute`; `shape_cache_db_replaces_split_caches`
  stays green; a cross-file merged-interfaces 5-property test (§9.5); per-variant
  producer discriminators.

---

### U3 — Delete Bespoke Invalidation (B8)

- **Source track:** cache-runtime (with semantic coupling).
- **Scope:** Route the 10 typed component-meta DB wrappers + remaining host
  caches through the U2 nodes. DELETE `component_meta_caches.rs` per-DB `clear_*`
  reverse-dependent eviction authority (replaced by validated lazy revalidation
  per B4 / skill R3). Remove `DeclIdentity` as a key field on any
  `SemanticQueryKey::*` variant.
- **Deps:** U2.
- **Parallelism:** Cache-runtime lane; can run beside U4/U5 and the semantic-graph
  lane (U8+).
- **Risk:** medium-large — correctness-sensitive (invalidation-authority change).
- **Required deletions:** `component_meta_caches.rs` per-DB `clear_*`
  reverse-dependent eviction; `DeclIdentity` from `SemanticQueryKey::*`.
- **Guards:** a guard that no `SemanticQueryKey` variant contains `DeclIdentity`;
  a guard that reverse-dependency graphs are not invalidation authority
  (validated lazy revalidation only); regression tests that same-canonical edits
  are caught by strict self-root validation and cross-file edits invalidate
  lazily through recorded facts.

---

### U4 — Persistent Pure Artifact Cache (B9)

- **Source track:** cache-runtime.
- **Scope:** Sealed `PersistentArtifactNode` trait (query nodes CANNOT persist) +
  `BaseWriteToken` / `BaseToken` capability witness +
  `PersistentCache` / `ManifestHeader` / `PERSISTENT_SCHEMA_VERSION` + CAS +
  manifest under `cache_runtime/persistent/`. Only pure content-addressed
  artifacts persist (e.g. `CompileOutputNode_PureContent`); semantic / session
  nodes stay memory-only.
- **Deps:** U2 (needs the node enumeration to know what is pure).
- **Parallelism:** Cache-runtime lane; beside U5/U6 and the semantic-graph lane.
- **Risk:** large — new on-disk format + sealed-capability type-gating.
- **Required deletions:** none (additive persistence layer); but query nodes must
  be type-level barred from `BaseWriteToken`.
- **Guards:** `cache_overlay_snapshot_cannot_construct_base_write_token`; a guard
  that only `PersistentArtifactNode` impls reach the persistent path; pure
  artifacts persist only with complete semantic/compiler/env/profile/plugin/
  source-map-policy keys.

---

### U5 — Memory Policy + Cache Audit (B10)

- **Source track:** cache-runtime.
- **Scope:** `MemoryPolicy`, `ActiveSnapshotPinRegistry` / `SnapshotId` /
  `CacheEntryId`, `EvictionRingBuffer`, `AdmissionDecision` / `ColdMissReason` /
  `StaleReason`, `CacheNodeMetrics` (single weight via
  `ArtifactNode::weight_bytes` / `QueryNode::weight_bytes` — no separate
  `WeightedAccountable`). Add `StructuredAuditEvent::CacheNode*` variants in
  `verter_audit` and emit from component-meta cache paths.
- **Deps:** B2 only (per inter-block DAG); best landed after U2/U3 (observability
  is most useful once nodes exist).
- **Parallelism:** B2-gated; can run parallel with the U4/U6 cache-runtime work
  and the semantic-graph lane.
- **Risk:** medium — audit additions are purely additive (closed-enum discipline
  on `StructuredAuditEvent`).
- **Required deletions:** none (`NonAdmissionReason` leaf already exists from the
  B3 split). Do NOT add a separate `WeightedAccountable` — weight is the single
  node method.
- **Guards:** closed-enum discipline guard on `StructuredAuditEvent`; a guard that
  cache hits do not allocate audit payloads without an active accumulator; metrics
  discriminators for cold-miss / stale / admission paths.

---

### U6 — Native Flow Return (B11)

- **Source track:** cache-runtime (with a semantic-key touch).
- **Scope:** Move `/tmp/verter-native-flow-return-coverage.md` →
  `docs/arch/native-flow-return.md` (FIRST task). Add `FlowBodyHashNode` /
  `FlowBodyHashKey` / `FlowBodyHashOutcome` (fail-closed: `BudgetExceeded` →
  `ReturnOnly`, `Hash(_)` → `Cacheable`) and `FlowLoweredBodyNode` /
  `FlowLoweredBodyKey` / `FlowLoweredBody` as B4-style nodes. Add
  `SemanticQueryKey::FlowReturn` query-node variant (routes through
  `ProjectSemanticDispatch::execute`). Body-hash production is SPLIT from body
  lowering (`FlowLoweredBodyNode::compute` must NOT call
  `compute_body_semantic_hash`).
- **Deps:** U2 + U4.
- **Parallelism:** Cache-runtime lane; beside the semantic-graph lane.
- **Risk:** large — touches `FileArtifactStore` + adds a new query-node kind.
- **Required deletions:** none of substance (the coverage doc is MOVED, not
  deleted).
- **Guards:** a guard that `FlowLoweredBodyNode::compute` does not call
  `compute_body_semantic_hash` (the split); `FlowReturn` routes through the one
  engine; fail-closed budget tests (`BudgetExceeded` → `ReturnOnly`).

---

### U7 — Scheduler Cache-Node DAG Admission (B7e)

- **Source track:** cache-runtime / scheduler.
- **Scope:** Add `CacheNodeDag`, `CacheNodeDagNode` (non-Clone; ready-queue
  element is `Arc<CacheNodeDagNode>`), `CacheNodeDagEdge` / `EdgeGate`, `KeyedJob`
  (envelope metadata), `CacheNodeOutcome` / `CacheNodeValue`,
  `CacheNodeCompletionSender`, `DagHandle`, `DagCompletionAggregator`. Implement
  `try_submit_dag(dag) -> SubmissionResult<DagHandle>` (typed `Backpressure`
  BEFORE readiness mutation, per H22) and `submit_dag_blocking` (parks on capacity
  condvar). Lower ALL cache nodes into the EXISTING `SchedulerDag` under ONE
  admission path (extend the §6b atomic admission core — NOT a parallel path).
- **Deps:** U1 (TaskKind::CacheNode + execute_cache_node); B7a (`SubmissionResult`
  / `DedupeHook` / `SchedulerCacheId`).
- **Parallelism:** Can run parallel AFTER U1, alongside the U3/U4/U5/U6
  cache-runtime work and the entire semantic-graph lane (the typeinfo DB does NOT
  ride `submit_dag` per §2.1).
- **Risk:** **very large / highest scheduler risk.** #1 stated risk: do NOT create
  a second readiness/accounting system beside `SchedulerDag` (no `ArrayQueue`, no
  `DagAdmissionBudget`, no parallel `DedupKey`). `WorkNodeIdentity` is THE dedupe
  identity. Preserve h23 capacity-reservation single-release + cooperative-pump
  invariants.
- **Required deletions:** none net-new to delete (no submitter-side `ArrayQueue` /
  `yield_now` / readiness-lock exists post-§7). `submit_dag` is net-new, NOT a
  `submit_batch` replacement (`submit_batch` was already deleted in §6c).
- **Guards:** keep `dag_arch_guards` (12/12) +
  `b7b_no_second_admission_budget_or_ready_queue` green; a guard that there is no
  second readiness structure; typed-`Backpressure`-before-mutation test (H22);
  single-release reservation test (h23).

---

### U8 — TypeInfo Graph Exporter

- **Source track:** semantic-graph (R-2).
- **Scope:** `crates/verter_session/src/typeinfo/exporter.rs` — a PURE lowering
  pass. Dispatch root semantic queries, snapshot reachable `SemanticNodeData`,
  build `node_id_map` / `symbol_id_map` / `signatures` arena / `StringTable`,
  capture `read_set` via `with_fact_tracer` + `self_root_canonicals`, stamp
  `exactness`, serialise to prost. It must NOT re-derive merge/augmentation
  structure from `IndexedReady` (it dispatches the U2 `ResolveMergedDeclaration`
  query instead).
  - **Payload shape: `TypeInfoGraphPayload { graph, program_analysis }` (A.11 —
    the authoritative revision, §2.2 MOOT).** `GraphTypeNode` carries ONLY type
    values; **flow-narrowing and contextual-type live in a sibling
    `ProgramAnalysisGraph`**, gated by a projection-required closure. The exporter
    lowers reachable type `SemanticNodeData` into the `GraphTypeNode` taxonomy and
    the flow/contextual facts into `ProgramAnalysisGraph`.
  - **Required wire move (touches the A0a wire surface → schema_version bump per
    the Typeinfo Wire Contract).** A0a currently encodes `GraphFlowNarrowing` /
    `GraphContextualType` arms INSIDE `GraphTypeNode` (`typeinfo.proto:206-207`).
    This block MOVES those two arms out of `GraphTypeNode` into
    `ProgramAnalysisGraph`: `reserved` the vacated `GraphTypeNode` field tags
    (26, 27) with their names, add the arms under the new sibling message, and
    bump `SemanticTypeGraph.schema_version` (closed-enum-discipline +
    wire-compat: field numbers never reused). Update the exporter to emit the
    A.11 placement.
  - Add the Rust `TypeNode` / `SymbolNode` / `ExpansionStatus` DTO surface beyond
    A0a's `graph.rs` re-exports.
- **Deps:** U2 (the variants it dispatches). U6 only for flow-return consumers.
- **Parallelism:** Semantic-graph lane head; runs beside the cache-runtime lane
  (U3–U7).
- **Risk:** large (wide lowering table + a wire-surface move with a schema_version
  bump); medium once U2 is correct (mechanical).
- **Required deletions:** the `GraphFlowNarrowing` / `GraphContextualType` arms are
  REMOVED from `GraphTypeNode` (re-homed under `ProgramAnalysisGraph`, their old
  tags `reserved`); legacy resolution/decoder deletions land with U12, same-phase
  as their replacements.
- **Guards:** `exporter_dispatches_resolve_merged_declaration_query` (proves the
  exporter dispatches rather than re-deriving); a guard that the exporter is pure
  (no second resolution path); node-taxonomy completeness over the `GraphTypeNode`
  kinds; `type_node_contains_only_type_values` (no flow/contextual arms inside
  `GraphTypeNode`); `program_analysis_graph_gated_by_projection_required`
  (`ProgramAnalysisGraph` populated only when the request's closure demands it).

---

### U9 — Scheduler Session Bridge (B7f)

- **Source track:** cache-runtime / scheduler.
- **Scope:** Session-side `DedupeHook` impl, opaque cache-id registry,
  `HostStageExecutor::execute_cache_node`. Wire the §6-deferred
  `CpuConcurrencySemaphore` propagation (workers acquire a fresh
  `CpuConcurrencyPermit` per CPU task at dispatch, RAII release; capacity sourced
  from scheduler config, NOT a removed per-call `threads` option). Update
  `.claude/skills/scheduler/SKILL.md` (stale re: `submit_batch` / pre-cache-node
  surface). Crosses the H20 boundary deliberately at the session edge only.
- **Deps:** U7 (cache-node DAG); B7a (`CpuConcurrencySemaphore` — landed but
  unwired).
- **Parallelism:** Cache-runtime lane tail; beside the semantic-graph lane.
- **Risk:** medium — spans `verter_scheduler` ↔ `verter_session`; keep the session
  bridge thin.
- **Required deletions:** none (wires the already-landed B7a substrate).
- **Guards:** keep `no_session_dep` (H20) green except at the sanctioned session
  edge; a discriminating test that `CpuConcurrencySemaphore` actually caps
  concurrent CPU tasks; opaque-cache-id-registry round-trip.

---

### U10 — TypeInfo DB / Fence / Degraded Store + QueryError Lowering

- **Source track:** semantic-graph (R-3 + R-4).
- **Scope:**
  - `crates/verter_session/src/typeinfo/completion_fence.rs` —
    `publish_with_retry` wrapping `InflightTable`, consuming the canonical
    `MAX_INFLIGHT_RETRIES = 3` (widen its visibility to `pub(crate)` per A.18);
    NO second retry constant.
  - `typeinfo_graph_db.rs` (`TypeInfoGraphResultDb`) — `BoundedCandidateMap` slot
    (cap 4 / budget 512), warm-hit revalidation (self-root + read-set facts),
    cold path via `cooperative_admit_with_post_publish`,
    `get_or_compute -> GetOrComputeOutcome` (A.3). Warm-only-exact admission
    (§2.6); ExactOpenGeneric warm-admissible (A.12).
  - `DegradedResultStore` (256-LRU, opt-in reads).
  - `From<QueryError> for QueryErrorDto` (§3.8 table) +
    `budget_kind_from_domain(BudgetDomain) -> BudgetKind` exhaustive match,
    against the real `BudgetExceededFailure { domain, limit, actual, context }`
    (NO `detail_name`).
- **Deps:** U8 (`cold_export`); the LANDED singleflight / `InflightTable` /
  `BoundedCandidateMap` / fact-signature substrate. **Per §2.1: built on the
  singleflight substrate, NOT on `submit_dag`.**
- **Parallelism:** Semantic-graph lane; cache-runtime lane (U7/U9) is unrelated.
- **Risk:** medium-large — the fence/admission contract is the most invariant-dense
  (warm-exact-only, 3-retry, no-partial-admit, zero-alloc warm hit).
- **Required deletions:** do NOT resurrect `finalise_signature_or_empty` — build on
  `FactReadSet::finalise` + `SignatureAdmission` (§2.2).
- **Guards:** `typeinfo_graph_warm_hit_*` (warm-exact-only); `queryerror_dto_is_lossless`
  round-trip; `budget_exceeded_*`; a guard that there is no second retry constant;
  a zero-alloc-warm-hit test (no audit payload allocation without accumulator).

---

### U11 — Native TypeInfoSession + Audit Execution

- **Source track:** semantic-graph (R-6 + R-5).
- **Scope:** `crates/verter_session/src/typeinfo/session.rs` — 8 `_with_audit`
  methods returning `AuditedResult<Arc<...>, TypeInfoRequestError>`;
  validate-before-execute; route through `TypeInfoGraphResultDb`. The §5.6
  `StructuredTypeExpression` → `SemanticQueryKey` 1:1 dispatch (replacing the
  legacy text scratch-file evaluator). Public `relate()` exposing the existing
  relation memo (§2.14). Per-entry-point cold / warm / degraded
  `StructuredAuditEvent` emission (warm = counter only),
  `TypeInfoGraphFootprintCell` in `footprint_miner.rs`, nested-record semantics
  for `expand_graph_around`, extend `wave_3_entry_points_propagate_tls` with
  typeinfo drivers. **Use `exactness_counts: BTreeMap` (§2.2), not `exactness_*`.**
  - **Schema-version RUNTIME (server side; A.5/A.6 — the runtime layer on top of
    A0a's contract GATE, all REMAINING).** Per-request schema-version ECHO:
    every `_with_audit` response carries the negotiated `schema_version` (not just
    the validator's closed-set gate). Runtime version NEGOTIATION:
    `SchemaVersionCapabilities::validated_supported_versions()` returns the
    server's advertised set (`[N, N-1, N-2]`) restricted to versions backed by a
    registered encoder; a session that negotiates `V < current` emits via the
    U12 `encode_typeinfo_payload_for_version(V, payload)` downlevel path. This is
    the session-edge consumer of the U12 encoder table — it lands together so the
    negotiation is never advertised without a backing encoder.
- **Deps:** U10; A0a request validators + `StructuredTypeExpression` proto +
  schema-version handshake gate (`SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS`) +
  `Relate` memo (all landed). The U12 downlevel encoders + `KNOWN_VARIANTS_AT_VERSION`
  table are the encoder backing for the advertised set (co-landed across U11/U12).
- **Parallelism:** Semantic-graph lane.
- **Risk:** medium-large.
- **Required deletions:** none yet (the scratch evaluator is deleted in U12).
- **Guards:** `typeinfo_exposes_relate_query`; validate-before-execute coverage;
  the audit 3-branch (cold/warm/degraded) discriminators;
  `wave_3_entry_points_propagate_tls` extended and discriminating;
  `every_typeinfo_request_carries_schema_version` (per-request echo);
  `server_supported_versions_have_encoders`
  (`capabilities.supported == validated_supported_versions()` — never advertise a
  version without a registered encoder).

---

### U12 — FFI / TS Decoder + Legacy Rust/TS Deletion

- **Source track:** semantic-graph (R-7 + R-8).
- **Scope:** `verter_napi/src/typeinfo.rs` + `verter_wasm/src/typeinfo.rs`
  (binary protobuf `Buffer` / `Uint8Array`); `packages/typeinfo/src/decode.ts`
  (pure mechanical) + new `session.ts`. Migrate the `component_meta.rs`
  `GraphBuilder` callsites to the new `TypeNode`.
  - **Schema-version downlevel ENCODERS (A.5/A.6 — REMAINING, the encoder backing
    for U11's negotiation).** Add `encode_typeinfo_payload_for_version(V, payload)`
    and the `KNOWN_VARIANTS_AT_VERSION` cumulative-exhaustive table (per-version
    `&[VariantId]` sets, A.6). Newer-only variants project to compatible
    substitutes for `v(N-1)` consumers (e.g. post-V `ExpansionStatus::ExactOpenGeneric`
    → `ExactSymbolic { reason: GenericPreserved }`); unsupported constructs degrade
    through `UnsupportedConstruct::DowngradedFromNewerSchema` →
    `UnsupportedConstruct::Unsupported { construct: "schema_skew" }` →
    `Opaque(Miss { reason: SchemaSkew })`. Each encoder is validated against
    `KNOWN_VARIANTS_AT_VERSION[target_version]` (NOT `encoder.version`).
- **Deps:** U11. Respects the `verter_ffi` thin-adapter rule.
- **Parallelism:** Semantic-graph lane.
- **Risk:** medium-large (cutover with deletions; must migrate last consumers in
  the same diff).
- **Required deletions (one clean cutover):**
  - Legacy NAPI/WASM typeinfo entries (consuming
    `EvaluateTypeExpressionRequestDto`).
  - `packages/typeinfo/src/{types, native-type-expr}.ts`.
  - `packages/component-meta/src/{type-graph*, type-expr-bridge}.ts` (8 files).
  - `verter_protocol/src/graph/builder.rs::GraphNode` / `GraphBuilder` (≈36KB) +
    `graph/schema/*` + `graph/mod.rs` re-exports; reserve the legacy proto
    `EvaluateTypeExpressionRequestDto` fields.
  - `typeinfo/{evaluate_type_expression, raise, resolve_named_symbol,
    scratch_cache, types, tests}.rs`; rename `symbol_inventory.rs` →
    `list_symbols.rs`.
- **Guards:** a find-grep guard that the deleted symbols/files are gone (e.g. a
  `RETIRED_SYMBOLS`-style guard); the scratch evaluator's `parse_type_annotation`
  use no longer exists in the resolver pipeline;
  `known_variants_at_version_rows_are_cumulative_exact_sets` (the table is
  cumulative-exhaustive); `downgrade_encoder_never_emits_variant_unknown_to_target_version`
  (each encoder validated against `KNOWN_VARIANTS_AT_VERSION[target_version]`);
  `known_variants_table_matches_proto_at_version` (CI — table regenerated from proto).

---

### U13 — Public TS Session + Projections

- **Source track:** semantic-graph (R-9 + R-10).
- **Scope:** Wire every `TypeInfoSession` method through NAPI/WASM; migrate legacy
  `resolveSymbol` / `evaluateTypeExpression` consumers to the graph API +
  `toTypeDescriptor`; TS graph helpers; nested audit.
  `packages/typeinfo/src/projections/{display, type-descriptor, json-schema, zod,
  storybook, docs}/`; cycle-id `z.lazy` memoisation; `SharedLoadReuse`
  audit-terminal skip (§7.7). `TypeDescriptor` becomes a projection target.
- **Deps:** U12.
- **Parallelism:** Semantic-graph lane.
- **Risk:** medium-large (6 projections).
- **Required deletions:** `descriptor-to-native.ts` / `native-to-descriptor.ts`
  (A.20) + compat semantic-recovery hooks; legacy entry-point names.
- **Guards:** the typed-IR-only compat guards extended to the projection packages
  (no `looksLike*` / `extract*` / `split*` on `rawType`); projection round-trip
  discriminators.

---

### U14 — Vue Framework Adapter Rebuild

- **Source track:** semantic-graph / component-meta (R-11).
- **Scope:** Rebuild `@verter/component-meta` as a thin `FrameworkSurfacePayload`
  adapter + `FrameworkAdapterRegistry` (A.15); `compat` as a projection wrapper.
  Fix the 4 known Vue mismatch cases: Popover `SlotProps<M>`, theme-alias display,
  `Button["variants"]["color"]` indexed-access, ContentSearch intersection.
- **Deps:** U13 (and transitively U1–U10).
- **Parallelism:** Semantic-graph lane tail.
- **Risk:** **large / high regression surface** — replaces the live native
  component-meta pipeline; regression risk against the existing corpus is greatest
  here.
- **Required deletions:** the legacy native-component-meta resolution path it
  replaces (cut over, do not dual-path).
- **Guards:** the 4 mismatch-case regression tests (each fails on the legacy path,
  passes on the rebuilt adapter); a guard that component-meta is a thin adapter
  (no second resolver/expander) per the native-vs-compat CRITICAL rule.

---

### U15 — Integrations, Ignored-Test Lift, Bench Schema

- **Source track:** MERGED terminal (semantic-graph Phases 6/7/8 + cache-runtime
  B12).
- **Scope:** Zod/schema client helpers; LSP hover→graph+display,
  completion→framework-surface, MCP `typeinfo.*` / `component-meta.*` tools,
  playground type explorer; lift the U0-derived majority of the typeinfo
  `#[ignore]` tests on the U0 manifest schedule (the target fraction is computed
  against the live count U0 re-derives — the A0a manifest baseline is 363 rows /
  `EXPECTED_TOTAL_IGNORED_COUNT`, distinct from the ~384 raw `#[ignore]` SITES
  before macro-family collapse; do NOT hard-code a stale absolute); Svelte/React
  STOP-gate files
  (`svelte_adapter_stop_gate.rs`, `react_adapter_stop_gate.rs`); final find-grep
  sweep. Plus the B12 typed bench schema: `BenchResultRow`
  (`packages/benchmark/src/cache-runtime-bench.ts`) reporting cache mode /
  source-map policy / batch shape / thread count / hit count / fallback count;
  vendored cm corpus benches (`component_meta_cold` / `_warm`);
  `MAX_TEST_TIMEOUT` + `test_support/timeout.rs`.
- **Deps:** all code-producing blocks (U0–U14).
- **Parallelism:** terminal — runs last.
- **Risk:** large aggregate; lower per-item risk (mostly integration + un-ignoring
  + gating).
- **Required deletions:** any remaining legacy entry-point names surfaced by the
  final sweep.
- **Guards:** the reconciled U0 unignore-manifest guards (the ~10 backing guards
  from the A0a-landed `typeinfo_ignored_test_manifest.rs`, now asserting the lifted
  count against the U0-re-derived total — `total_ignored_typeinfo_test_count_matches_expected`
  + `manifest_length_matches_documented_total` track the post-lift count, not a
  hard-coded stale absolute); `merged_interfaces_across_files` 5-property test (§9.5)
  green; Svelte/React STOP-gate guards; hermeticity guard
  (`external_corpus_paths_not_present_outside_gated_tests`) — bench corpora are
  vendored, no `.integration-tests/repos/<third-party>/`.

---

## 5. U2 in depth — B4 ↔ SemanticQueryKey co-sequencing (§B)

**Binding decision (§B): merge Block-4 and the 7-variant `SemanticQueryKey`
addition into a single block, U2. Do NOT add the 7 variants on the current
`DeclKey` shape and later migrate B4 to slot identity.** One clean cutover, no
migrate-twice.

U2 finalizes the **`SemanticQueryKey` identity SHAPE once** (the slot-identity
model). It does NOT freeze the variant LIST — later ADDITIVE variants land in this
same shape with no cache re-key (notably U6's `SemanticQueryKey::FlowReturn`, B11).
What U2 fixes once is the identity model every variant keys on:

1. **Existing variants → slot identity.** `Instantiate { base }` and
   `ResolveMacroPayload { owner }` move from the content-free
   `DeclKey { canonical_id, decl_name }` (already landed via §6c/A0a/reconcile-#4)
   to `ResolvedDeclSlotIdentity`. This is the only remaining identity refinement;
   the whole-hash R6 violation is already resolved (§2.2), so this is a
   slot-precision change, not a re-key from scratch.

2. **7 new variants in the final shape.** `ResolveMergedDeclaration`,
   `ResolveModuleAugmentation`, `ResolveAmbientNamespace`, `ResolveOverloadSet`,
   `ResolveEnum`, `FlowNarrowingAt`, `ContextualTypeAt` — each added directly in
   the slot-identity shape, each dispatched through
   `ProjectSemanticDispatch::execute` (the one-engine rule). Their
   `SemanticNodeData` producers (`MergedDeclaration`, `ModuleAugmentation`,
   `AmbientNamespace`, `Class`, `Enum`) land in the same block. Cross-file
   declaration merging is implemented here, not deferred (§9.5).

3. **Semantic + component-meta caches onto `QueryNode` / `ArtifactNode` against
   the same key model.** The query-identity caches owned by the semantic track —
   `SemanticGraphStore` (family / relation / named-type, parameterised over ALL
   `SemanticQueryKey` variants), `ComponentMetaResultDb`, `MaterializeStructureDb`,
   `RefCycleResultDb`, `ShapeCacheDb` — plus the content-addressed artifact caches
   (`FileArtifactStore`, `ResolvedImportFacts`, typed-IR resolve, member fact
   stores, `ModuleAugmentationIndex`) become `QueryNode` / `ArtifactNode` impls
   keyed by the final model. Query-identity keys carry NO content/version hash or
   `fact_dep_signature`; content-addressed keys carry `content_hash` /
   `parse_stable_hash`. The five env-hash dimensions stay split (R21);
   `lib_env_hash` enters only the caches that depend on lib data.

This is why U2 is the convergence gate and the highest-correctness-risk block:
the same caches are re-keyed exactly once, the one shared resolver gains its
final variant set in one cutover, and every downstream consumer (the exporter
U8, the typeinfo DB U10, the Vue adapter U14) builds on the final key model.

---

## 6. TypeInfoGraphResultDb admission fork (§C)

**Binding decision (§C): `TypeInfoGraphResultDb` admission is singleflight NOW,
with NO later retarget to `submit_dag`.**

The typeinfo result DB (built in U10) admits through the cache-runtime
**singleflight / fact-validation substrate**, which is already landed:

- `cooperative_admit_with_post_publish` (`cache_runtime/singleflight.rs`)
- `InflightTable` (`cache_runtime/singleflight.rs:213`); the canonical
  `MAX_INFLIGHT_RETRIES = 3` lives in `semantic_query_memo/inflight.rs:226`
- `BoundedCandidateMap` + `GlobalRetentionBudget` (`bounded_query_retention`)
- `FactReadSet::finalise` → `SignatureAdmission`
- `HostStoreView` for warm-hit revalidation

The B7e cache-node DAG (`submit_dag`, U7) is **scheduler execution / readiness
plumbing — not a second cache-admission authority.** The typeinfo DB must never
be folded into `CacheNodeDag` / `submit_dag`, now or as a future migration.

**Consequence for sequencing:** because U10's admission binds to the
already-landed singleflight path, the entire semantic-graph execution lane (U8 →
U10 → U11 → … ) is **independent of the remaining scheduler work (U1, U7, U9)**
and proceeds on its own dependency-parallel lane after the U2 gate. The only
forward-coupling the original plans worried about — retargeting the typeinfo DB
onto a future `submit_dag` — is explicitly ruled out here.

---

## 7. Verification baseline & known failures

Run after EVERY block. The crates are highly interconnected; always run the full
workspace suite, not a scoped subset.

```bash
# Rust
cargo check --workspace --tests
cargo clippy --workspace --tests -- -D warnings
cargo fmt --all -- --check

# Focused scheduler / session (every block that touches them)
cargo test -p verter_scheduler --tests
cargo test -p verter_session  --tests            # covers all integration binaries
                                                  # in one run (consolidated harness)

# Full workspace (the authoritative gate)
cargo test --workspace --tests --no-fail-fast

# TS + full build (gates wasm cfg-gating that --tests cannot catch)
pnpm install --frozen-lockfile
pnpm test
pnpm build                                        # native → lsp → wasm → ts
```

**Gate-method notes (banked from prior landings):**

- `cargo test --workspace --tests` historically SKIPS the consolidated
  `verter_session` integration binaries; the reliable session gate is
  `cargo test -p verter_session --tests` (one run covers all binaries since the
  `mod harness;` consolidation).
- `cargo nextest run` is NOT a substitute — it false-fails `verter_audit` ts-rs
  `export_bindings_*` (parallel writes to the shared `audit.generated.ts`).
- `pnpm build` is a required gate: the wasm `cfg`-gating breaks have surfaced
  ONLY under `build:wasm`, never under `cargo --tests`.
- Trust-but-verify: re-run the full gate independently of any sub-agent's
  `tail -N` summary.

**Expected outcome for every block:** the **exact 8-failure baseline** (§1) and
**ZERO new failures**. The `typeinfo_ts_bindings_*` env-only failure is a
non-failure on the main checkout (passes with `node_modules` present).

`sixteen_cold_concurrent…attribute_per_joiner_contract` was a load-flake earlier
in the chain and is now stable (fixed at `27c25a7a`); treat any recurrence under
pathological oversubscription as a known load-flake, not a new regression, and
confirm 3/3 in isolation.

---

## 8. Documentation-update map

After landing each block, update the OWNING documentation (skill that owns the
module/API; `CLAUDE.md` only if a summary or skill pointer changes; `AGENTS.md`
if skill routing changes; `docs/` for API/guide pages; inline rustdoc/JSDoc on
changed signatures). Every new CRITICAL rule lands with a static guard or a
discriminating regression test in the same change (R6 meta-guard).

| Block | Primary docs to update |
|---|---|
| **U0** | `/type-resolution` (typeinfo contract surface); the reconciled A0a-landed `tests/typeinfo_ignored_test_manifest.rs` manifest (schema notes); `/audit-infrastructure` (`AuditedResult`). |
| **U1** | `/scheduler` SKILL (TaskKind split, `execute_cache_node`); `/host-session` if dispatch surface changes. |
| **U2** | `/type-resolution` + `/type-cache-architecture` (final `SemanticQueryKey` surface, slot identity, B4 node enumeration, R21 key composition); `CLAUDE.md` project-global-cache + macro-traversal summaries; `docs/arch/fact-based-cache.md` per-cache key tables. |
| **U3** | `/type-cache-architecture` (invalidation authority, no reverse-dep eviction); `/component-meta` (cache contracts). |
| **U4** | `/type-cache-architecture` (persistent pure-artifact rules, sealed `PersistentArtifactNode`); `docs/arch/fact-based-cache.md`. |
| **U5** | `/type-cache-architecture` (memory policy, metrics); `/audit-infrastructure` (`StructuredAuditEvent::CacheNode*`). |
| **U6** | `docs/arch/native-flow-return.md` (moved here in U6); `/type-resolution` (`FlowReturn` query node); `/compiler-codegen` if flow lowering surfaces. |
| **U7** | `/scheduler` SKILL (`CacheNodeDag`, `submit_dag`, `KeyedJob`, `DagHandle`). |
| **U8** | `/type-resolution` (exporter, lowering table); `Typeinfo Wire Contract` rule pointers. |
| **U9** | `/scheduler` + `/host-session` SKILL (session bridge, `CpuConcurrencySemaphore` wiring, `execute_cache_node`). |
| **U10** | `/type-resolution` + `/type-cache-architecture` (`TypeInfoGraphResultDb`, `CompletionFence`, degraded store, QueryErrorDto lowering). |
| **U11** | `/type-resolution` (`TypeInfoSession`, `_with_audit`, `relate`); `/audit-infrastructure` (3-branch emission, footprint cell, nested records). |
| **U12** | `/architecture` (FFI surface); `/type-resolution` (decoder); legacy-deletion notes in the owning skills. |
| **U13** | `/component-meta` + `/architecture` (projections, `TypeDescriptor` as projection); `docs/` API pages. |
| **U14** | `/component-meta` (native-vs-compat, framework adapter registry, Vue surface). |
| **U15** | `/e2e-vscode-testing`, `/build-and-profiling` (bench schema), `/testing`; `/architecture` (MCP/LSP/playground integration); the unignore manifest's final counts. |

The two original plans (`cache-runtime-overhaul-plan.md`,
`semantic-type-graph-plan-recovered.md`) carry a SUPERSEDED-for-remaining-work
pointer to this doc but are otherwise unchanged (historical/detail reference).

---

## 9. Terminal acceptance checklist

The unified effort is "done" when ALL of the following hold:

- [ ] **U0–U15 all landed** (each implement → triple review → clean re-review →
  land), in §A order, with the U2 convergence gate landed before any
  graph-execution block (U8+).
- [ ] **One `SemanticQueryKey` identity shape** (slot-identity) finalized in U2 with
  all 7 U2 variants, the additive U6 `FlowReturn` variant landed in that same shape
  (no cache re-key), every variant dispatched through
  `ProjectSemanticDispatch::execute`; NO `DeclIdentity` and NO content/version
  hash or `fact_dep_signature` in any query-identity key.
- [ ] **All B4 caches** are `ArtifactNode` / `QueryNode` impls on the B2 substrate;
  bespoke reverse-dependent `clear_*` invalidation deleted; validated lazy
  revalidation is the sole invalidation rail.
- [ ] **`TypeInfoGraphResultDb`** admits through the singleflight / fact-validation
  substrate (NOT `submit_dag`), with warm-exact-only admission, the canonical
  3-retry fence, no second retry constant, and zero-alloc warm hits.
- [ ] **Scheduler** has the TaskKind split (U1), `submit_dag` cache-node DAG (U7)
  under one admission path with no second readiness/ledger structure, and the
  session bridge (U9) wiring `CpuConcurrencySemaphore`; `dag_arch_guards` and the
  B7b guards green.
- [ ] **Typeinfo session** exposes the 8 `_with_audit` methods + public `relate()`,
  validate-before-execute, with cold/warm/degraded audit + footprint cell + nested
  records using `exactness_counts: BTreeMap`; every request response echoes the
  negotiated `schema_version` and the advertised supported set is restricted to
  versions with a registered encoder (`every_typeinfo_request_carries_schema_version`,
  `server_supported_versions_have_encoders` green).
- [ ] **FFI/TS** exposes the binary-protobuf typeinfo surface; the schema-version
  downlevel encoders (`encode_typeinfo_payload_for_version` +
  `KNOWN_VARIANTS_AT_VERSION` cumulative-exhaustive table) ship with their guards
  (`known_variants_at_version_rows_are_cumulative_exact_sets`,
  `downgrade_encoder_never_emits_variant_unknown_to_target_version`); ALL legacy
  typeinfo Rust modules, the `GraphBuilder`, the scratch evaluator, and the legacy
  TS/component-meta type-graph files are DELETED (no dual path).
- [ ] **Projections** (display / type-descriptor / json-schema / zod / storybook /
  docs) ship; `TypeDescriptor` is a projection; descriptor-bridge deleted.
- [ ] **`@verter/component-meta`** is a thin `FrameworkSurfacePayload` adapter over
  the graph; the 4 known Vue mismatches are fixed; no second resolver/expander.
- [ ] **Ignored-test lift:** the U0-derived majority of typeinfo `#[ignore]` tests
  lifted per the U0 manifest schedule (target fraction computed against the
  U0-re-derived live count — A0a baseline 363 manifest rows, not a hard-coded stale
  absolute); the reconciled manifest guards assert the final post-lift counts;
  Svelte/React STOP-gate files present.
- [ ] **Bench schema:** `BenchResultRow` reports cache mode / source-map policy /
  batch shape / thread count / hit count / fallback count; cm corpus benches
  vendored and hermetic.
- [ ] **Gate green:** `cargo check` / `clippy -D warnings` / `fmt` clean;
  `verter_scheduler` + `verter_session` + full workspace = the 8-failure baseline
  and ZERO new; `pnpm test` + `pnpm build` (native → lsp → wasm → ts) fully green;
  `pnpm install --frozen-lockfile` in sync.
- [ ] **Architecture clean:** every new CRITICAL rule has a registered guard (R6
  meta-guard green); `no_phase_archaeology_in_production_code` green;
  one-engine / typed-IR-only / shallow-by-default / CodeTransform invariants
  intact.
