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

### Scope: the target is FULL TypeScript-checker parity

**The TARGET is full TypeScript-checker-grade type parity** — relation /
assignability, inference (candidate accumulation / priority / fixation /
contextual-callback / reverse-mapped), measured variance, control-flow narrowing,
conditional / mapped / template-literal reduction, overload resolution, and the
cross-engine recursion that binds them. This is an **honest multi-person-year
scope**, not a flow-return patch. The bounded near-term lifts this plan sequences —
the **363-row ledger** plus the Vue-macro / component-meta / IDE surfaces — are the
**first measurable increments** toward that target, not the target itself.

**The 363-row ledger tracks COVERAGE / wiring, NOT semantic tsc-parity.** A green
363 proves every row has an owner block, an executable proof, and a wired mechanism
(coverage completeness — "detects un-wired"). It does **not** prove the engine
agrees with `tsc`/`tsgo` on the SEMANTICS of those families (semantic completeness —
"detects wrong"). Semantic tsc-parity is gated separately by the **differential
`tsgo`-parity oracle** (§3.2 / parent `native-typeinfo-parity.md` §6.3), baselined
at each hard phase's rescope gate. State `363-green` and "tsc-parity" as two
distinct claims; never read one as the other.

### Native-typeinfo-parity doc set (indexed here)

The native TypeScript-parity typeinfo architecture is documented as one parent
plus four children. This unified plan is their **sequencing authority** and
indexes the whole set at the owning U-blocks below; each subplan links back here
and to the parent architecture via its standard subplan header.

| File | Owns | Owning U-block(s) |
|---|---|---|
| `docs/arch/native-typeinfo-parity.md` | **Parent architecture** — engine architecture, capability map, query/fact authority, the per-block contract template, the two-table manifest ledger, the git/CI landing protocol, the guards index | spans `U0`–`U15`; indexed at U0/U2/U6/U15 |
| `docs/arch/native-typeinfo-parity-u2-reducers.md` | U2 reducer / relation / utility / indexed / mapped / template / class / enum / module / JSX foundations + the U0 manifest/ledger substrate | U0, U2 |
| `docs/arch/native-flow-return.md` | U6 flow chapter — the demand-sliced `ReturnPathPeeker` (two-frontier model) and the flow IR | U6 |
| `docs/arch/native-typeinfo-parity-cache-export-session.md` | U3 / U8 / U10 / U11 / U12 / U13 — cache/fact model (U3), wire-surface closure (U8), result DB (U10), public relation/session (U11), exporter (U12), projection (U13) | U3, U8, U10, U11, U12, U13 |
| `docs/arch/native-typeinfo-parity-adapters-final-lift.md` | U14 / U15 — framework adapters, integrations, final lift | U14, U15 |

The foundation those documents build on is
`docs/arch/semantic-type-graph-plan-recovered.md` (graph / wire / cache
foundation).

### Stage5 cutover doc set (indexed here)

The Stage5+6 compiler-macro cutover (port `verter_compiler` macro lowering off the
parser `resolve_type/` OXC resolver onto the shared dispatch, deliver the
compiler-owned `ResolvedMacroSurfaces` DTO produced by the host, make parser macro
parsing spans-only, then delete the legacy rail) is documented as one plan. This
unified plan is their **sequencing authority** — the Stage5
blocks `S5.B1`–`S5.B12` are sequenced into the cross-plan order below (§3), with the
shared macro-surface gate `S5.B5` landing AFTER the typeinfo convergence gate U2.

| File | Owns | Cross-plan blocks |
|---|---|---|
| `docs/arch/stage5-cutover-plan.md` | **The Stage5+6 cutover plan** — macro DTO boundary, `resolve_macro_surfaces_for`, runtime-constructor relocation, parser-spans-only cutover, legacy `resolve_type/` deletion | owns `S5.B1`–`S5.B12` |

### Goto-definition overhaul doc set (indexed here)

The go-to-definition overhaul (typed `EmitOp` IDE-codegen mapping substrate, the
`SfcComponentAnchor` on `IndexedReady`, the reconciled `CompileSnapshotId`, and the
single definition engine) is documented as one plan plus two detail/reference docs.
This unified plan is their **sequencing authority** — the goto blocks `G.P1`–`G.P6/7`
are sequenced into the cross-plan order below (§3), with `G.P3` landing after U2,
`G.P4` after U3, and the `CompileSnapshotId` reconciliation + name disambiguation
stated there (§3.1.2).

| File | Owns | Cross-plan blocks |
|---|---|---|
| `docs/arch/goto-definition-overhaul-plan.md` | **The phased goto-definition plan (rev 2)** — phase contracts, file:line references, legacy deletions | owns `G.P1`–`G.P6/7` |
| `docs/arch/goto-definition-architecture-decision.md` | Binding architecture (detail/reference) — the V3-substrate decision, the `EmitOp` taxonomy, the one-definition-engine end-state. **NOT a sequencing authority.** | — |
| `docs/arch/goto-definition-review-findings.md` | Round-1 review findings + binding OQ resolutions (detail/reference). **NOT a sequencing authority.** | — |

`docs/arch/native-checker.md` is a **sibling follow-up** (non-parity): native
TypeScript-grade diagnostics as a LATER layer over the SAME resolver
(`SemanticQueryKey → ProjectSemanticDispatch::execute → SemanticGraphStore`),
sequenced AFTER the U0–U15 typeinfo-parity blocks. It is NOT part of the 363-row
parity scope and does not change the U-blocks; it only consumes the seams the parity
blocks reserve (`SemanticQueryValue::DiagnosticAnalysis`, the `Check*` query names,
the `ExecutableRegionId` region abstraction, the `ProgramAnalysisContributor` seam).
It is **RESCOPE-GATE-REQUIRED (effort weight HIGH, §3.3): deep design produced at
its own rescope session before its `Check*` queries implement** — its
diagnostics-algorithm depth + the per-family `tsgo`-oracle baseline are produced at
the gate, depending on the parity blocks (U2/U6) it layers over.

---

## 1. Landed-context preamble

**Branch:** `refactor/semantic-db-overhaul`. This plan does NOT pin a "current tip"
SHA — the branch advances under a concurrent workstream, so any named SHA goes stale
the moment it is written. Instead, the recent history is recorded as HISTORICAL
WAYPOINTS for orientation only: `aba794d9` was the native-typeinfo full-TS-parity
doc-set land (the U0–U15 parent + four children); `cec9cfa4` (a v-on-handler
closing-suffix compiler fix) and `0a68332c` (a serena-memories commit) are later
commits on top of it; `51a49a35` (the prior scheduler-dispatch tip) and `b36e0835`
(the B7c land point) are older references still. None of these is a "current
authority" SHA. **Binding rule:** this plan does not run the workspace and does not
assert a test count at any specific commit — the live baseline AND the live tip are
RE-DERIVED at each block's entry gate against the then-current `HEAD` (per the env
re-home rule, §3.1.4), never pinned to a frozen SHA (see "Known-failure baseline"
below).

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

### Known-failure baseline (recorded at the B7c land; re-derived at implementation)

The recorded historical baseline is the **long-standing 8-failure cluster recorded
at the B7c land (`b36e0835`)** — all long-standing and **OUT OF SCOPE for this
unified plan** (a Block-1.A `fact_dep_signature` substrate migration cluster carried
forward since reconcile-#2/#3):

- `compile_tier_signature_carries_*` ×5
  (`member`, `member_presence`, `import_ref`, `route_surface`,
  `module_augmentation_index_shape`)
- `family_a_fact_validation` ×2 (`family_a_entries_carry_fact_dep_signature`,
  `family_a_warm_hit_uses_fact_validation`)
- `materialise_structure_entry_carries_dep_signature` ×1

This is the **historical reference point** as of `b36e0835`, NOT a verified count at
the current tip. The tip has advanced well past `b36e0835` (through the
scheduler-dispatch work and the `aba794d9` typeinfo doc-set landing), and this plan
does NOT itself run the workspace — so implementation **re-derives the live baseline
at each block's entry gate at the then-current tip** (the separate later effort), and
treats the cluster above as the recorded starting set to reconcile against, not a
frozen count to assert at any specific commit.

Plus one **environment-only** non-failure: `typeinfo_ts_bindings_*` fails in a
`node_modules`-less worktree (it regenerates TS bindings via the workspace `buf`
binary) and **PASSES on the main checkout with `node_modules` present** /
post-`pnpm install`. It is not a code failure.

Every block's verification expects ZERO NEW failures over the baseline re-derived at
that block's entry gate (at the then-current tip), not a count frozen to an old commit.

### Block status ledger

Status derived from the landed-context above + git evidence on
`refactor/semantic-db-overhaul`; re-derive at each block's entry gate (per the binding
rule above, no frozen SHA is pinned). `RESCOPE-GATE-PENDING` = the block's deep design
is produced at its rescope session (§3.2) before it implements.

| Block | Status | Note |
|---|---|---|
| X0 | LANDED | the §3.1 cross-plan integration + doc-set index is present |
| G.P1 | LANDED | typed `PositionMapper` coords (`verter_span`) |
| G.P2 | LANDED | `EmitOp` IDE-emit substrate; `resolve_prefixed_expr` retired |
| G.P3 | NOT-STARTED | `SfcComponentAnchor` on `IndexedReady`; 0 src hits; ← U2 |
| G.P4 | NOT-STARTED | `CompileSnapshotId`; 0 src hits; ← U3 |
| G.P5 | NOT-STARTED | `navigation::definition` engine; 0 src hits; ← G.P4 |
| G.P6/7 | NOT-STARTED | route all nav surfaces + delete legacy arbitration; ← G.P5 |
| S5.B1 | LANDED | `parse_checker_text_to_type_expr` deleted (in RETIRED list) |
| S5.B3 | LANDED | `ResolvedMacroSurfaces` DTO present (`compile/macro_dto.rs`) |
| S5.B4 | LANDED | `runtime_constructors_from_type_expr` (`typeinfo/adapters/vue/runtime_ctor.rs`) |
| S5.B5 | RESCOPE-GATE-PENDING | macro-surface gate; `resolve_macro_surfaces_for` not wired; compat matrix is a gate deliverable; ← U2 |
| S5.B6–B12 | NOT-STARTED | consumer switch + parser-spans-only + legacy `resolve_type/` deletion; ← S5.B5 |
| U0 | PARTLY-LANDED | `AuditedResult` carrier landed; host-API wiring + two-table ledger extension remain |
| U1 | PARTLY-LANDED | `TaskKind` split + `CacheNode` + executor surface landed; `dispatch_cpu_task` + cache-node dispatch wiring remain |
| U2.QUERY_VALUE_DOMAIN | NOT-STARTED | the typed `SemanticQueryValue` value-domain layer (convergence gate) |
| U2.RELATION_INFER | RESCOPE-GATE-PENDING | the relation/inference/variance core; ← U2.QUERY_VALUE_DOMAIN |
| U2 (other reducers) | NOT-STARTED | indexed/mapped/utilities/class/enums/module-aug/JSX sub-blocks |
| U3 | NOT-STARTED | canonical fact-signature model; ← U8 + U2 + U6 |
| U4 | NOT-STARTED | persistent (B9); ← U2 |
| U5 | NOT-STARTED | mem/audit (B10); ← B2, best ≥ U2 |
| U6 | RESCOPE-GATE-PENDING | cross-engine flow-return recursion; ← U2.RELATION_INFER |
| U7 | RESCOPE-GATE-PENDING | scheduler `submit_dag` (justify-or-cut); ← U1 |
| U8 | NOT-STARTED | wire-surface closure; ← U6 + S5.B12 |
| U9 | NOT-STARTED | session bridge (B7f); ← U7 |
| U10 | NOT-STARTED | result DB; ← U3 + U8 |
| U11 | NOT-STARTED | public relation/session; ← U12 |
| U12 | NOT-STARTED | exporter; ← U10 |
| U13 | NOT-STARTED | projection; ← U12 |
| U14 | NOT-STARTED | Vue adapter; ← U13 + U11 |
| U15 | NOT-STARTED | integration/bench/final lift; ← all code blocks |
| native-checker | RESCOPE-GATE-PENDING | sibling/future diagnostics layer (out of 363 scope); own rescope gate |

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
  correctness defect) nor the retired request-view globals (`CURRENT_REQUEST_VIEW`
  and the `_in_view` signature surface — both retired). The plan's "live
  `StoreView`" maps onto `HostStoreView` directly: build the fence on
  `HostStoreView`, NOT on the request-bound `resolver_core::RequestStoreView`
  (a live `pub(crate)` overlay-chaining `StoreView` impl that exists for the
  request-overlay path, not the fence).
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
  deleted in U12 once `StructuredTypeExpression` dispatch (U12) lands — the
  request-decode/dispatch home (`resolve_named_symbol.rs`) that obsoletes the
  text-evaluator is U12 itself (U11 DEPENDS on U12, so U12 cannot gate its own
  deletion on U11). §11 of the semantic-graph plan confirms no 2nd exception.

---

## 3. Cross-track dependency map

The two tracks are **largely PARALLEL**. The key structural fact (§C):
the semantic-graph EXECUTION layer (U8 wire closure, U3 cache/fact model, U10 result
DB, U12 exporter, U11/U13, …) binds to the **already-landed singleflight /
fact-signature substrate**, NOT to the unbuilt scheduler cache-node DAG
(`submit_dag`). So scheduler work (U1 → U7 → U9) and semantic-graph execution
(U8 → U3 → U10 → U12 → U11/U13 → … → U15) proceed on **dependency-parallel lanes**
after the one convergence gate.

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
   U4 (B9 persistent)   ──► depends U2     U8 (wire closure)  ──► U2 + U6    │
   U5 (B10 mem/audit)   ──► B2, best ≥U2     (keystone of the subplan)       │
   U6 (B11 flow-return) ──► depends U2,U4    U3.CACHE_FACT_MODEL ──► U8       │
   U7 (B7e submit_dag)  ──► depends U1          (+U2,U6; after wire closure)  │
        (parallel after U1)                 U10 (result DB)  ──► U3 + U8      │
   U9 (B7f session bridge)──► depends U7     U12 (exporter)   ──► U10         │
        │                                    U11 (relation/session) ──► U12   │
        │                                    U13 (projection) ──► U12         │
        │                                    U14 (Vue adapter)  ──► U13 + U11 │
        └────────────────────────────────►  U15 (integration/bench/lift) ◄──┘
                                                depends on all code blocks
```

(Semantic-graph lane chain, per the revised subplan
`docs/arch/native-typeinfo-parity-cache-export-session.md`:
**U8 wire-closure → U3 cache/fact model → U10 result-DB → U12 exporter → {U11
relation/session, U13 projection}**. `U3.CACHE_FACT_MODEL` lands AFTER the wire
closure (its typed admission produces values whose wire shape U8 closes) and the
result DB (U10) + exporter (U12) admit through its rails. U8 is the keystone — the
wire surface every later block reads/writes — and is the sole hard prerequisite the
parity lane shares with the cache-runtime lane via U2/U6.)

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

### 3.1 Cross-plan integration — Stage5 + goto-definition folded in

Two further streams land under this same unified sequencing authority alongside the
typeinfo U-blocks: the **Stage5+6 compiler-macro cutover** (`S5.B1`–`S5.B12`, owned
by `docs/arch/stage5-cutover-plan.md`) and the **go-to-definition overhaul**
(`G.P1`–`G.P6/7`, owned by `docs/arch/goto-definition-overhaul-plan.md`). They are
folded in per a HIGH-CONFIDENCE codex-architect cross-plan sequencing decision. The
typeinfo dep-map above is unchanged; this section ADDS the `S5.*` and `G.*` edges and
the one mandatory shared gate that binds Stage5 and typeinfo together.

#### 3.1.1 The shared macro-surface gate (THE crux)

**Macro-resolution end-state (one path, one normalizer).** Vue macro resolution has
exactly ONE semantic path: `ResolveMacroPayload → ProjectSemanticDispatch::execute →
SemanticGraphStore`, followed by ONE shared macro normalizer. `VueMacroElements`,
`HostResolvedNamedTypeKey`, `SemanticQueryKey::ResolvedNamedType`, the parser
`NamedTypeCache` adapter, the graph named-type indexes, and the entire parser
`resolve_type/` OXC resolver ALL retire. This is the concrete realization of
CLAUDE.md's **Macro-Type-Traversal Rule** ("a macro is at bottom a single type lookup
plus thin normalisation — `shared_resolve(type) + normalise`"); Stage5 `S5.B5` and the
typeinfo blocks **jointly own** this cutover — it is NOT a standalone compiler cleanup.

**`S5.B5` is a SHARED macro-surface gate that lands AFTER typeinfo U2.** U2 finalizes
the `ResolveMacroPayload` identity (the slot-identity shape), so `S5.B5` cannot land
before it. **`S5.B5` is also RESCOPE-GATE-REQUIRED (§3.2): its normalizer
COMPATIBILITY MATRIX is a rescope-gate deliverable** — the full
valid-empty-vs-unresolved / visibility / defaults / `native_props` / public-surface
filtering / emits / slots / options / expose matrix, proved at the gate to lose **no**
semantic distinction versus the legacy rail BEFORE `S5.B11` deletes the fallback
(that proof is the gate's output, not asserted in this prose). Ownership at the gate
splits cleanly:

- **Typeinfo owns the semantic surface + the normalization contract.** The shared
  macro surface and its normalization (props / emits / slots / options / expose) are
  the typeinfo semantic authority, resolved through the one shared dispatch.
- **Stage5 owns the compiler-owned `ResolvedMacroSurfaces` DTO + `VerterHost::resolve_macro_surfaces_for`.**
  `ResolvedMacroSurfaces` is a **structural projection of the shared normalization**
  produced by the host for the compiler — it is **NOT a second resolver**, NOT a
  second normalizer, and NOT the `SemanticGraphStore` artifact. Typeinfo `U14` later
  consumes the **same** normalized surface structurally and **must not rebuild macro
  meaning** (no parallel surface/expander in the adapter).

**`native_props` + class-member VISIBILITY ownership.** The semantic visibility of
class/members belongs to **typeinfo surface ownership** — specifically the U2 class /
member surfaces (`ResolveClassSurface`) and the published projection (U13). Stage5
carries that visibility through `ResolvedMacroSurfaces` and is responsible for
**re-sourcing the FFI / public `native_props` carrier BEFORE `S5.B11`**. `S5.B11` is
**BLOCKED until that re-source is proved** (the carrier must demonstrably reflect the
shared visibility before the legacy rail is deleted).

**HARD GATE: `S5.B11`/`S5.B12` MUST complete BEFORE any typeinfo `U8+` work.** Do NOT
build the new typeinfo wire / result / export / projection stack (U8, U10, U12, U11,
U13, …) around a sidecar (`VueMacroElements` / `HostResolvedNamedTypeKey` /
`resolve_type/`) that is scheduled for deletion. The biggest correctness risk is the
B5 normalizer **silently losing semantic distinctions** while replacing the old rail —
especially **valid-empty vs unresolved**, **non-public visibility**, **`native_props`**,
**defaults**, and **public-surface filtering**. Once `S5.B11` deletes the fallback,
that class of regression is hard to recover without reintroducing a second path — so
the gate makes the post-U2 macro-surface cutover MANDATORY before any `U8+` typeinfo
work or Stage5 legacy deletion completes.

**Docs-updated deliverable on `S5.B11`.** CLAUDE.md's project-global-cache description
(the `VueMacroElements` / `HostResolvedNamedTypeKey` / `NamedTypeCache` paragraph) and
the `/type-cache-architecture` + `/type-resolution` skills are updated to the
one-`ResolveMacroPayload`-path end-state by `S5.B11`'s `Docs updated:` step **when
S5.B11 lands** — tracked here as a deliverable, NOT pre-edited now (pre-editing the
current-state authority to describe unbuilt state would make it lie about the live
code, exactly as §8's intentional-divergence note governs the typeinfo blocks).

#### 3.1.2 Goto-definition dependency edges + the `SnapshotId` reconciliation

- **`G.P1` → `G.P2` — LANDED on this branch** — the typed `PositionMapper`
  coordinates (`SourceByteRange` / `GeneratedByteRange` / `LspPosition` / `TsPosition`
  in `crates/verter_span/src/lib.rs`) and the typed `EmitOp` IDE-codegen mapping
  substrate (`crates/verter_compiler/src/ide/template/emit.rs`) + the
  collapsed-overwrite emit-site fixes landed via the `db1e2db8`/`3b4c1e3c`/`01dd889d`/
  `b437232f`/`47184ca3`/`b1611270`/`cec9cfa4` line (the retired `resolve_prefixed_expr`
  producer is gone). This is mapper / codegen substrate work with **NO typeinfo
  dependency**; it parallels the early lane.
- **`G.P3` lands AFTER U2** — it adds `SfcComponentAnchor` onto `IndexedReady`, the
  shared post-parse substrate U2 finalizes (`G.P3 ← U2`).
- **`G.P4` waits for U3's canonical fact-signature model** (`G.P4 ← U3`), and the
  goto/nav validity token — `CompileSnapshotId` — **is RECONCILED** to the compile
  warm-hit fact-validated identity:

  > `CompileSnapshotId` = `Hash128(compile warm-hit identity)` = `semantic_hash` + the
  > style/content override hashes + the compile profile + the parser/compiler/version
  > dimensions + the **canonicalized `ReadSetSignature` facts** (the cross-file
  > fact signature).

  The goto plan's `Hash128(prefix || generated_tsx_bytes)` (a TSX byte hash) is **NOT
  the authority**: a TSX byte hash may ride as **debug / collision metadata**, but the
  **validity token MUST match the compile-slot fact-validated identity** — otherwise
  the id is stale-stable under cross-file dependency edits (`semantic_hash`
  covers own-content only). `.ts` / `.js` / `.d.ts` real-source targets have no IDE
  TSX and use **`TargetSourceContext.source_hash`**, NOT a TSX `CompileSnapshotId`.
  Mismatch ⇒ DROP (do not recompute).

  **Name disambiguation (avoids a `verter_session` compile-time clash):** `G.P4`'s
  nav-validity token is named **`CompileSnapshotId(u128)`**, distinct from U5's net-new
  cache-entry-pin **`SnapshotId(u64)`** (the `ActiveSnapshotPinRegistry` id in
  `cache_runtime/{metrics,memory_policy}.rs`). Both are net-new in `verter_session`, so
  the goto/nav identity carries the `Compile` prefix throughout the goto doc set; U5's
  `SnapshotId` stays as-is.
- **`G.P5` + the merged `G.P6/7`** wait on `G.P4`; once U3 has landed they may run
  **parallel with typeinfo `U10`–`U13`**.
- **One-engine guard for navigation.** Definition / references / rename stay **ZERO
  typed-IR dispatch** (navigation is not type resolution). `type-definition` MAY use
  `ProjectSemanticDispatch` **only in the one explicitly allowed type-definition
  module** — a guard enforces no type-resolution leak into the other nav surfaces.

#### 3.1.3 The concrete cross-plan ORDER

```
X0  re-home + docs index  (this integration itself — §3.1)
       │
       ▼
parallel-safe early:   G.P1 → G.P2   ∥   S5.B1   ∥   U0   ∥   U1
       │
       ▼
U2  = CONVERGENCE GATE  (finalizes ResolveMacroPayload identity + the slot-identity shape)
       │
       ├──────────────────────────── after U2 ────────────────────────────┐
       │                                                                    │
  Stage5 macro chain (shared gate):                                         │
       S5.B5 (macro-surface GATE)                                           │
         → S5.B6 / S5.B7 / S5.B8 → S5.B9 → S5.B10 → S5.B11 → S5.B12         │
                                                                            │
  parallel after U2:                                                        │
       U4 → U6        U5        scheduler U7 → U9        G.P3               │
                                                                            │
  U8  ONLY after  U6 + S5.B12   ◄── HARD GATE (no U8+ around the sidecar)   │
       │                                                                    │
       ▼                                                                    │
  U3   (canonical fact-signature model)                                     │
       │                                                                    │
       ├──────────────────────────── after U3 ────────────────────────────┤
       │                                                                    │
  goto tail:   G.P4 → G.P5 → G.P6/7     ∥     U10 → U12 → U11/U13 → U14 → U15
```

**Cross-plan dep-map edges (added to the typeinfo dep-map above; the whole graph
stays ACYCLIC).** The typeinfo edges are unchanged; these are the ADDED `S5.*` / `G.*`
edges:

- `S5.B5 ← U2` (the macro-surface gate cannot land before U2 finalizes
  `ResolveMacroPayload` identity).
- `S5.B6/B7/B8 ← S5.B5`, `S5.B9 ← S5.B6/B7/B8`, `S5.B10 ← S5.B9`,
  `S5.B11 ← S5.B10` (and `S5.B11` additionally BLOCKED on the proved `native_props`
  re-source), `S5.B12 ← S5.B11`.
- `U8 ← {U6, S5.B12}` — U8 keeps its typeinfo prereqs (U2 + U6) AND additionally
  waits on `S5.B12` so the new wire/result/export stack is never built around the
  deleted sidecar. (This is the one new typeinfo-side edge; it adds a Stage5 prereq
  to U8, it does not change the U8↔U2/U6 edges.)
- `G.P2 ← G.P1` (early, no typeinfo dep); `G.P3 ← U2`; `G.P4 ← U3`;
  `G.P5 ← G.P4`; `G.P6/7 ← G.P5`.

Because `S5.B5 ← U2`, `U8 ← S5.B12`, `G.P3 ← U2`, and `G.P4 ← U3` all point from a
later block to an earlier one in the order above, no cycle is introduced; the combined
graph remains a DAG.

#### 3.1.4 Env re-home rule

The three streams' historical artifacts are **HISTORICAL ONLY**: the `D:/` worktrees,
the branches (`fix/goto-definition-overhaul`, `s5/b5`), the SHAs (`0b3a63894`,
`380967beb`, `63d682f69`), and the `D:/tmp/*` live-state artifacts. The **live
authority** is this repository on `refactor/semantic-db-overhaul`.

Goto `G.P1`/`G.P2` have since **landed on this branch** (typed `PositionMapper`
coordinates + the `EmitOp` substrate — see §3.1.2). Stage5 `S5.B5` is **NOT landed
here**: the `ResolvedMacroSurfaces` DTO struct exists (`crates/verter_compiler/src/compile/macro_dto.rs`),
but `resolve_macro_surfaces_for`, the cutover, and the legacy `resolve_type/` +
`VueMacroElements` / `HostResolvedNamedTypeKey` deletions are not done — re-implement /
re-land it from the plan against live HEAD (S5.B5 is RESCOPE-GATE-PENDING, gate after
U2). A cherry-pick is a
**patch-extraction technique after review**, never proof of correctness or sequencing.
The live-branch tests / guards / docs + the unified-plan block acceptance govern. All
three streams land under this unified plan's **git/CI landing model**: branch per
block → green CI gate → three-reviewer LAND → squash-merge with a trailer (e.g.
`Typeinfo-Block:` / the Stage5 / goto equivalent), exactly as §1 / U0 describe for the
typeinfo blocks.

---

## 3.2 Big-phase rescope gate (pre-implementation algorithm-depth gate)

The full-tsc-parity target (Scope, above) is multi-person-year, and its hardest
subsystems **cannot be designed to algorithm depth up-front**: a hard phase's deep
design depends on PRIOR phases having landed and on the substrate they expose. So a
**BIG PHASE is deliberately NOT pre-specified to algorithm depth in this plan.**
Instead, **before** a big phase implements, the orchestrator runs a **RESCOPE
SESSION** that produces that phase's deep design against the then-current tree.

**This is the resolution of the "under-designed core" finding.** The hard algorithmic
cores are **intentionally deferred to their rescope gate, not hand-waved.** Where a
block contract currently says only "route through `Relate`" / "route through the
reducers" / "route through the one engine" for a HARD core, that phrasing states the
one-engine *wiring* constraint — it is **not** the algorithm. The deep algorithm for a
RESCOPE-GATE-REQUIRED phase is produced at its rescope session, not asserted here.

**The rescope session (autonomous, minimal-token).** A planner drafts the phase's
algorithm-depth design; a **rescope review panel — 1 Claude Code + 2 codex, all under
the best-architecture / breaking-changes-allowed / be-honest mandate** (the same loop
that converged this plan) — iterates WITH the planner until best-possible-architecture
is reached AND feasibility is confirmed. The orchestrator drives this **fully
autonomously** and does the **minimal work to keep token usage low**: it does not
re-litigate already-converged phases, and it scopes the panel to the one phase. The
session produces:

- **(a) the phase's algorithm-depth design** — **executable-pseudocode depth**: the
  real data structures, the actual algorithm, and — wherever the design is recursive
  or iterative — a **real termination / convergence proof** (e.g. a monotone
  measure + a well-founded bound, or an SCC-discharge argument showing
  non-convergence never yields a WARM admission). A proof is NOT "route through
  `Relate`" prose, and guard NAMES are NOT a design.
- **(b) the differential `tsgo`-parity oracle baseline** for the phase's families
  (§3.3) — the per-family divergence budget the phase's acceptance is gated on.
- **(c) the phase's discriminating guards** — fail-today fixtures (fail against the
  pre-change tree, pass after).
- **(d) any rescope of SUBSEQUENT phases** the design reveals (the gate may re-shape
  later phases' contracts).
- **(e) the rules update — at the gate, BEFORE the phase implements (NOT the
  implementer's job).** Once best-architecture is reached AND before the phase's
  implementing agent starts, the **rescope orchestrator** updates `CLAUDE.md` (its
  `(CRITICAL)` rule set) plus the relevant `.claude/skills/*` with the phase's NEW
  `(CRITICAL)` rules and their named R6 guards. The implementing agent must have those
  rules ALREADY present in `CLAUDE.md` / the skills (loaded in its context) by the time
  it codes the phase, so it implements WITH the rules in mind rather than
  discovering/adding them mid-block. **R6 coupling (keeps the meta-guard green):** each
  new `(CRITICAL)` rule is committed together with its REGISTERED guard — a guard name
  plus a `CRITICAL_RULE_GUARDS` registry entry — at this gate→implementation boundary,
  so `every_critical_rule_in_docs_has_registered_guard`
  (`crates/verter_session/tests/critical_rules_have_guards.rs`) is satisfied the moment
  the rule appears; the guard's DISCRIMINATING test then lands with the phase's FIRST
  implementation block (the rule + a registered guard appear in the same change, and the
  test goes green as that first block lands).

**This rules-update-at-gate step is DISTINCT from the already-converged
orchestration-layer "current-state authority / Docs-updated-deferred" note** (§3.1.1's
`S5.B11` deliverable + §8's intentional-divergence note): that deferral applies to
ALREADY-CONVERGED orchestration-layer changes, where pre-editing the current-state
authority to describe unbuilt code would make it lie about the live tree, so the doc
update is tracked as a land-time `Docs updated:` deliverable. For a RESCOPE-GATE big
phase the rule update happens AT the gate (just before that phase implements) precisely
because the implementer must CODE AGAINST the new rule — the rule is a design input the
phase consumes, not a post-hoc description of code that already shipped.

**Two DISTINCT gates.** The **rescope panel (per-big-phase, PRE-implementation)** and
the **three-reviewer LAND panel (per-block, at block-done, §11.12 of
`native-typeinfo-parity.md`)** are different gates at different stages. The rescope
gate produces the design *before* the phase's blocks are written; the LAND panel
authorizes the merge of each finished block. Both are **additive to** the git/CI
landing model — a phase still lands block-by-block via branch → green CI →
three-reviewer LAND → squash-merge **after** its rescope gate has produced the design.
The rescope panel is also distinct from §14.1's codex-architect fork gate (which
resolves an UNFORESEEN fork DURING a block); the rescope gate is the SCHEDULED
pre-design of a known-hard phase.

**RESCOPE-GATE-REQUIRED phases** (each tagged in its block contract below with a
one-line "deep design produced at its rescope session (depends on `<prior>`)" note;
effort weights in §3.3):

- **`U2.RELATION_INFER`** — the relation / assignability + inference-session +
  measured-variance core (TS's hardest subsystem). Rescope gate **after the U2
  foundation (`U2.QUERY_VALUE_DOMAIN`) lands**. Design depth: the assignability
  algorithm + the recursion-id / `isDeeplyNestedType`-style **termination proof**
  (non-convergence never yields a WARM admission) + the inference
  candidate-accumulation / priority / fixation / contextual-callback-loop /
  reverse-mapped algorithm + the marker-probe variance iteration. (Parent
  `native-typeinfo-parity.md` §§4.0–4.2 already seeds this core — the marker-probe
  fixed-point, the coinductive-SCC assumption protocol, the `CheckerTransaction` /
  `InferenceSession` substrate; the rescope gate hardens that seed to
  executable-pseudocode depth + the oracle baseline, it does not start from zero.)
- **`U6`** (the cross-engine cycle / termination:
  `FlowReturn` ↔ `ResolveCall` ↔ narrowing ↔ `ContextualTypeAt`, the
  `CheckerReentryGraph`, and flow narrowing as a dataflow fixed-point). Rescope gate
  **after `U2.RELATION_INFER`**. Design depth: runtime SCC detection +
  provisional-result-during-discharge + a stability decision + **fail-closed on
  non-convergence**; the narrowing **JOIN ALGEBRA** for conflicting predicates + the
  loop fixed-point. **The 8 `U6.NARROW_*` sub-blocks re-cast as 1 hard
  lattice-substrate block + 7 cheap additions** (front-load the hard one — see the U6
  block contract).
- **The native checker** (`docs/arch/native-checker.md`) — the sibling/future
  diagnostics layer. **RESCOPE-GATE-REQUIRED before its `Check*` queries implement**
  (it is out of the 363-row parity scope, but its `Check*` algorithm-depth design is
  produced at its own rescope session).
- **`U7`** (the scheduler cache-node DAG, `submit_dag`) — **RESCOPE-GATE-REQUIRED with
  an explicit "justify against MEASURED workload or CUT" question**: a model-checked
  DAG for an I/O-bound LSP already served by singleflight is an over-engineering risk.
  The rescope gate must either justify the DAG against a measured workload or cut it.
- **`S5.B5`** (the shared macro-surface gate) — **RESCOPE-GATE-REQUIRED**: its
  normalizer **compatibility matrix** is a rescope-gate deliverable — the full
  valid-empty-vs-unresolved / visibility / defaults / `native_props` / public-surface
  filtering / emits / slots / options / expose matrix, proved to lose **no** semantic
  distinction versus the legacy rail before `S5.B11` deletes the fallback.

### 3.2.1 Rescope-gate deliverable ledger (the SEMANTIC-CORE design, produced at each gate)

Two rounds of independent architecture-only review converged on one finding: the
**substrate frame is near-ceiling** (one-engine dispatch, fact-validated cache, the R21
env-hash split + the R6 rule↔guard coupling, demand-driven scoping, wire purity), but the
**SEMANTIC CORE is still a named seam, not a design** — "route through `Relate`" / "route
through the reducers" states the one-engine WIRING constraint, not the algorithm. The
first pass (codex ~8, claude ~7) seeded **#1–#8**; a **second deep architecture-only pass
(codex + claude, 2026-06-03) re-confirmed the substrate frame near-ceiling (~9)** and
surfaced further semantic-core gaps, recorded below as **#9–#22**. Per the owner's
defer-to-gate process these gaps are **NOT designed here**; each is captured as an
explicit NAMED deliverable its rescope gate MUST produce (to algorithm /
executable-pseudocode depth, with proofs), so the gaps are assigned, not lost or
hand-waved. State plainly: **this expanded ledger IS the semantic-core design,
intentionally produced at each phase's rescope gate; the substrate frame is already
near-ceiling, and the plan does not pretend the core is designed yet.**

**Already credited (NOT gaps).** The second pass re-surfaced two items the plan already
owns: the differential `tsgo`-parity oracle (§3.4 + ledger **#8**) and the
`CheckerReentryGraph` (§3.2 `U6` + parent `native-typeinfo-parity.md`). Both are credited
as present, not re-opened as deliverables.

The relation / cycle pair (**#1 + #2**) is **resolved EARLY at the U2 foundation gate**
because it shapes the substrate every later phase consumes — it is not deferred to the
phase that happens to need it last.

- **`U2.RELATION_INFER` gate** (the highest-leverage; resolved FIRST, substrate-shaping
  — gate runs after `U2.QUERY_VALUE_DOMAIN` lands):
  - **#1 — Relation-caching category (the deepest gap).** Decide EXPLICITLY whether
    `Relate` is a **TRANSIENT intra-check relation cache** (keyed on the type-id pair +
    relation-kind, like tsc's per-check relation cache) or a **PERSISTENT cross-request
    cache**. **The current plan folds `Relate` into the persistent fact-validated
    query-identity cache; that folding is FLAGGED PROVISIONAL pending this gate's
    decision.** If the gate confirms persistent, it MUST PROVE the cache identity fully
    captures the live comparison stack + the in-flight inference context (the
    `InferenceContextKey` fingerprint must be complete) — otherwise the choice is
    unsoundness (a warm hit reuses a relation decided under different assumptions) or a
    perf collapse (the identity is so wide it never hits).
  - **#2 — Coinductive cycle discharge as a first-class
    `ProjectSemanticDispatch::execute` PRIMITIVE.** A cross-query reentry / assumption
    stack — spanning `FlowReturn` / `ResolveCall` / `FlowNarrowingAt` /
    `ContextualTypeAt` / `Relate` — with a provisional-value protocol PLUS an explicit
    "**when a provisional result becomes STABLE and is therefore ADMISSIBLE**" rule. This
    is a property of the ONE engine, NOT per-family bolt-ons: it replaces / unifies the
    per-family stand-ins (`RefCycleResultDb` + the flow depth-sentinel). **#1 and #2 are
    ONE design problem** — the reentry / assumption stack is exactly what decides whether
    a relation result is stable enough to admit — so the gate designs them TOGETHER.
  - **#5 — Relation-proof structural shape.** A derivation witness / reason code, so
    `Relate` is the sole assignability authority with a DEFINED public payload, not a
    name on a side table. (Feeds the `RelationPayload` + payload-side `relation_proofs`
    proof table the U8 wire surface already names.)
  - **Cache-admission algebra** (codex). An exact, impossible-to-misread
    publish / no-publish matrix over the full discriminant set: `Cacheable` /
    `ReturnOnly` / overflow / budget-exhaustion / unresolved-provenance /
    incomplete-self-rooting / generation-supersession / overlay-only → which states
    PUBLISH a warm entry and which return-only.
  - **#10 — Request-consistency model.** ONE request view held consistent across every
    nested query, so a parent and its sub-queries observe one immutable snapshot. The
    reentry / assumption stack of **#2** IS this request-consistency frame; the gate
    designs them together.
  - **#13 — Cycle semantics OUTSIDE `Relate`.** ONE cycle + depth policy table covering
    aliases, mapped types, conditional types, template-literal types, recursive object
    types, projection, and display — extending the **#2** coinductive primitive beyond
    relation so every recursive walk shares one termination policy.
  - **#20 — Strict-family semantics.** Reducers and assignability must BRANCH on
    `strictNullChecks` (and the strict family), not merely KEY a cache on it: the result
    differs under strict-on vs strict-off, so the difference is computed, not just
    isolated.
- **`U2.QUERY_VALUE_DOMAIN` gate:**
  - **#3 — Formalize the `ProjectionDemand × EvalPolicy` lattice.** The partial order,
    the join / meet, the dominance + backfill rules, the satisfaction relation PROVEN,
    with worked examples for the important pairs (`Identity` / `Navigate` / `Shallow` /
    `Expanded` / `Skeleton`, path projection, carrier stops, generic-open) — the full
    lattice algebra, NOT just the five presets.
  - **Tabular `SemanticQueryKey → SemanticQueryValue → wire-projection`** (codex). Per
    key: identity fields, env-hash dimensions, value domain, fact domains read / written,
    allowed demand dimensions, cache family, producer, projection target.
  - **#14 — Canonical display policy.** Display is a PROJECTION over typed values, never
    a second resolver: one rule for how every family's typed value renders to its display
    string. Ties the **#3** lattice and the U13 published projection.
  - **#18 — Error-tolerance / resolution over broken / mid-edit code.**
    **Flagged EARLY — HIGHEST FUNCTIONAL RISK.** The editor steady-state is half-written
    source, so every query must produce a useful result over syntactically / semantically
    broken input. This gap may warrant escalation to a U0 / foundation treatment rather
    than staying inside this gate.
  - **#21 — Module-resolution matrix implementation.** Conditional `exports`, `node16`,
    `paths`, and symlink resolution, proven against the resolver / import-graph layer.
    Foundation-level: it is a resolver / import-graph deliverable that may rescope U0.
  - **#22 — Error-type / `any` / `never` propagation lattice (worth-checking).** How the
    error type, `any`, and `never` propagate and absorb through reducers — ties the **#3**
    lattice.
- **`U3` / `U6` gates — #4 Fact-model completeness for the new fact domains.** PROVE the
  `ReadSetSignature` read-set tracer CAPTURES the footprints of
  `FactDomain::ProgramAnalysis` (a flow slice = control flow + EVERY narrowed symbol's
  type) and of relation results (the full structural reachability of BOTH related types),
  OR add the missing fact kinds — else warm hits go stale SILENTLY (the worst failure
  mode). (`#4` straddles U3's fact model and U6's `FlowSlice` / `FactDomain::ProgramAnalysis`
  domain.)
- **`native-checker` gate** (NEW sub-heading; `docs/arch/native-checker.md`):
  - **#15 — Hard typeinfo↔checker negative boundary.** Define the line typeinfo NEVER
    crosses: it does NOT compute whole-body diagnostics, does NOT run a full
    assignment-check (the public `relate` is the sole exception), does NOT compute
    control flow beyond the demanded slices, and does NOT perform checker error-recovery.
    Pairs with **#19** (reason-to-exist / authority boundary) — together they fix what
    native OWNS versus what it DEFERS to the running TS.
- **`U7` gate — #6 Parsimony pass.** Justify the custom DAG scheduler (deficit / credit
  lanes, model-checked liveness) + the persistent on-disk cache against the MEASURED
  I/O-bound LSP workload, or CUT them — concentrate sophistication in the resolver + the
  cache. (Same justify-or-cut question already on the `U7` tag; #6 binds it to a measured
  workload.)
- **Architecture-wide (substrate deliverable, stated once):**
  - **#7 — One unified soundness statement.** Define ONCE what "**a warm hit is valid**"
    means uniformly across EVERY query family + fact domain, and require every cache to
    uphold that single invariant — versus the implicit distributed R1–R31 + per-family
    rules.
  - **#8 — The differential `tsgo`-parity oracle as a DESIGNED structural surface** (tie
    to §3.4). Divergence from the oracle is observable BY CONSTRUCTION (a structural
    surface the engine emits), not ONLY via an external test harness.
  - **#9 — Unified degradation taxonomy.** ONE model for how every query degrades —
    miss / partial / fallback / unresolved — so every family reports the same degradation
    shape. Couples the FFI / client-compat surface (**#12**) and per-family fallback.
  - **#11 — Formal lib / intrinsics authority + TS-version pinning.** Define WHO owns
    `lib.d.ts` and the intrinsics, pinned to a specific TS version. Ties `lib_env_hash`
    and the pinned-`tsgo` oracle.
  - **#12 — FFI / client-compat ownership.** The downgrade is defined per wire variant,
    and no semantic distinction is lost across the NAPI / WASM / TS clients. Couples the
    degradation taxonomy (**#9**) and ties the U8 wire surface.
  - **#16 — Per-family performance + latency contracts.** Each query family's cost shape
    AND fallback shape, expressed as an interactive-latency SLO (ms — p50 / p95 on the
    MISS path; an LSP cannot gate on fallback-entry-COUNT). Each rescope gate emits its
    families' contract; ties §3.3 + §3.4.
  - **#17 — `lib.d.ts`-scale ambient cost (worth-checking).** The cost of carrying
    full-scale ambient lib declarations — ties the lib-authority (**#11**) and per-family
    perf (**#16**) deliverables.
  - **#19 — Reason-to-exist / authority boundary vs the TS already running.**
    **Flagged FOUNDATIONAL — the deepest strategic gap, root of the scope problem.** Why
    native reimplements what `tsgo` already serves; what native OWNS; where it DEFERS.
    Pairs with the typeinfo↔checker boundary (**#15**).

## 3.3 Effort / risk weights on the iceberg blocks (independent of row count)

Row count is **not** a difficulty signal — the hardest cores carry few rows. The
scheduler must treat the following **effort/risk weight as a scheduling input DISTINCT
from row count** (it sizes the rescope-gate session and the per-block fix-cycle budget,
it does not change the DAG order):

| Block | Rows | Effort / risk weight | Why |
|---|---|---|---|
| `U2.RELATION_INFER` | ~20 | **EXTREME** | 20 rows = the WHOLE TS checker core (relation + inference + variance). Row count vastly understates it. |
| `U6` (cross-engine recursion) | (flow chapter) | **EXTREME** | `FlowReturn`↔`ResolveCall`↔narrowing↔contextual fixed-point + the narrowing join algebra; non-convergence must fail closed. |
| native checker (`Check*`) | 0 (out of scope) | **HIGH** | net-new diagnostics layer over the same resolver; its own rescope gate. |
| `U3` (cache / fact model) | ~3 | **HIGH** | 3 rows but a concurrency-sensitive cache-eviction rewrite (per-family adaptive cap + invalid-first/LRU-by-valid-hit + global ceiling, invalidation-authority change). |
| `U7` (`submit_dag`) | — | **HIGH (justify-or-cut)** | highest scheduler risk; over-engineering candidate — rescope-gated against measured workload. |

The rest of the backlog is moderate/low weight at its stated row count. The weight is
advisory to the orchestrator (session sizing + review depth), **not** a re-ordering of
the acyclic dep-map (§3).

## 3.4 Differential tsgo-parity oracle (the semantic-correctness gate)

The semantic-correctness gate for the hard families is the **differential `tsgo`-parity
oracle harness** (owned in full by parent `native-typeinfo-parity.md` §6.3; this is the
sequencing/process pointer): run a corpus — a **TS-conformance slice + property-generated
type fixtures** — through **Verter AND the pinned `tsgo`**, diff the **STRUCTURED**
results, and gate on a **per-family divergence budget**.

- **It replaces the 363 proxy as the SEMANTIC gate.** 363-green proves wiring/coverage
  completeness (every row owned + executably proven); the oracle proves the engine
  agrees with `tsgo` on the families' SEMANTICS. The oracle is **produced / baselined at
  each hard phase's rescope gate (§3.2(b))** and gates THAT phase's acceptance — it
  converts the behavioral guards from "detects un-wired" to "detects **wrong**."
- **Per-family parity-coverage target shape:** "N TS-conformance cases per reducer
  family; divergence budget M per family" (each hard phase's rescope gate names its own
  concrete N / M for its families against the pinned `tsgo`).
- This is **distinct from** the §6.2 / U15 Verter-vs-`tsgo` PERFORMANCE benches (those
  gate cost-shape / fallback-count; this gates semantic agreement). Both run the same
  pinned `tsgo`.

---

## 4. Unified block backlog (U0–U15)

Drive ONE block at a time: implement → triple review (independent reviewer +
codex) → per-block fix cycle until clean → land. Each block uses the template:
**ID / source track / scope / deps / parallelism / risk / required deletions /
guards**. Sequence is faithful to §A; do not reorder.

---

### X0 — Cross-plan integration (this edit)

- **Scope:** the env re-home rule (§3.1.4), the three doc-set index clusters (native-typeinfo-parity / Stage5 / goto-definition, indexed in §0) wiring all subplans to this sequencing authority, and the back-references from the goto/stage5 docs to this plan.
- **Deps:** none (this is the integration edit itself; it gates nothing and is gated by nothing).
- **Done-when:** §3.1 cross-plan integration (incl. §3.1.1–§3.1.4) is present, and all 4 goto/stage5 docs (the 3 goto docs + the Stage5 cutover plan) back-reference this unified plan as their sequencing authority.

---

### U0 — Finish Typeinfo Contract Gaps  **(NEXT primary block)**

- **Source track:** semantic-graph (R-0a).
- **Parity docs:** parent `docs/arch/native-typeinfo-parity.md` (manifest ledger
  §10, git/CI landing protocol §§11–14) + child
  `docs/arch/native-typeinfo-parity-u2-reducers.md` (the U0 manifest/ledger
  foundation block, `U0.MANIFEST_SUBSTRATE`).
- **Scope:** Close the contract gaps A0a left.
  - The `AuditedResult<T, E>` carrier ALREADY EXISTS in
    **`crates/verter_audit/src/audited_result.rs`** (the type, its `ts-rs`
    `audit.generated.ts` export, and the `ok`/`err` constructors) — NOT
    `verter_protocol`: it is generic over `T`/`E`, which protobuf cannot express,
    and embeds the audit-substrate `RequestAuditRecord`, so it rides the ts-rs
    export rather than forcing a dependency inversion. `packages/typeinfo` imports
    the generated TS type; there is no hand-written mirror. REMAINING: wire the
    host entry-points that hand it back (the `AuditedResult` host API that U6's
    flow surface and the typeinfo result blocks consume).
  - **Unignore-manifest — EXTEND the A0a-landed manifest into the two-table
    ledger end-state, do NOT create a second one.** A0a already landed the manifest
    as a Rust test, not a doc:
    `crates/verter_session/tests/typeinfo_ignored_test_manifest.rs` +
    `tests/manifest_data/typeinfo_ignored_test_manifest_rows.rs` (363 rows,
    `EXPECTED_TOTAL_IGNORED_COUNT = 363`), with ~10 backing guards. U0 keeps that
    same in-repo `.rs` test as the single ledger (there is no separate
    `typeinfo_tests_unignore_plan.md` doc and no second manifest at a competing
    path) and EXTENDS it to the **two-table ledger** the parent architecture
    requires (`native-typeinfo-parity.md` §10):
    - **`IgnoredTestRow`** holds EXACTLY the 363 ignored test-site rows — count-guarded
      at 363 and bijective with the source `#[ignore]`s — with the extended row schema
      (`substrate: TargetSubstrate`, `capability`, `organ`, `owning_u_block`,
      `block_id`, `semantic_queries`, `proof: ProofRequirement`, `status: IgnoreStatus`,
      `unblocker`).
    - **`AdditionalProofRow`** is a SEPARATE coverage-only table holding the
      **closed exactly-7** new/additional coverage fixtures (e.g. the JSX no-new-key
      submatrix) — one block per row, pinned by `additional_proof_row_table_holds_exactly_7_rows`.
      It is EXCLUDED from the ignored-count + bijection guards (it carries no
      `IgnoreStatus`) but still requires a `ProofRequirement` + a coverage-table entry;
      a fixture corresponding to an existing ignored row stays an `IgnoredTestRow`
      (never duplicated). It is NOT an open/expandable bucket — the table is closed at
      seven.
    - **`IgnoreStatus { Ignored, Lifted { block_id } }`** is the per-`IgnoredTestRow`
      lifecycle — BINARY, with no tracked in-flight `Verifying` / lease state (the
      "in-flight, not-yet-landed" state of a block is simply its unmerged branch;
      branch/CI is the only acceptance transaction — `native-typeinfo-parity.md`
      §10.1 / §11.1). `ProofRequirement` + the GENERATED proof registry + the row-test
      wrapper make "every row resolves to an executable proof its own test consumes"
      mechanical; the U0 row-exact capability→mechanism→proof coverage table
      (`native-typeinfo-parity.md` §10.4 / §10.4.1) DEFINES completeness.
    Do NOT migrate to a competing `target_phase` / `target_substrate` / `unblocked_by`
    schema at a different path — the landed `substrate` / `unblocker` columns are
    preserved and extended in place. See `native-typeinfo-parity.md` §10 for the full
    two-table ledger schema, the proof model, and the exact-363 count/bijection
    contract (cited, not duplicated here).
  - **Landing protocol — git/CI, no tracked cursor (U0 deliverable).** Typeinfo
    parity blocks land through git/CI, not a tracked orchestration cursor: branch per
    block → green CI (the full Rust+JS workspace gate + the coverage/proof/required/DAG
    guards) → three-reviewer LAND (branch-protection required approval) → squash-merge
    with a `Typeinfo-Block: <block-id>` trailer. Git is the transaction log, branch
    protection is the accept gate, and `git revert` is rollback; resume is derived from
    the manifest status + the merged trailers + the prereq DAG (no lease / WAL / revision
    CAS / receipt). There is NO `.cutover-state.typeinfo_parity` namespace, no two-namespace
    TOML schema, no namespaced xtask, and no crash-recovery machinery to build. The
    SEPARATE legacy top-level `.cutover-state` cutover cursor (the broader-plan execution
    cursor) is unrelated and is neither migrated nor reset. See `native-typeinfo-parity.md`
    §§11–14 for the git/CI landing protocol (cited, not duplicated).
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
- **Risk:** small-medium (the `AuditedResult` host-API wiring over the
  already-landed carrier + additive guards + a manifest reconciliation/rename of
  the A0a-landed test).
- **Required deletions:** none net-new for the contract surface, BUT the
  unignore-manifest is an EXTENSION of the A0a-landed manifest into the two-table
  ledger end-state, not a fresh add: do NOT create a duplicate manifest at a second
  path with a competing schema — the A0a-landed `typeinfo_ignored_test_manifest.rs`
  + its `manifest_data/` rows + its backing guards stay in place and grow the
  `IgnoredTestRow` schema + the separate `AdditionalProofRow` table + `IgnoreStatus`
  / `ProofRequirement` in place (preserving the landed `substrate` / `unblocker`
  columns; do NOT migrate to a competing `target_phase` / `target_substrate` /
  `unblocked_by` schema). The `AuditedResult` carrier already exists in
  `verter_audit`; only its host-API wiring remains.
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
    guards), kept green under the reconciled schema. Plus the EXTENDED two-table-ledger
    guards: `ignored_test_row_table_holds_exactly_363_rows` (binding-total + table
    disjointness), `additional_proof_row_table_holds_exactly_7_rows` (the closed
    coverage-only table), and the git/CI landing-protocol DAG guard
    `typeinfo_parity_block_dag_is_acyclic_and_consumed_keys_and_mechanisms_are_prereqs`
    (`native-typeinfo-parity.md` §§10.1, 11.5). `EXPECTED_TOTAL_IGNORED_COUNT` is the
    DERIVED live `count(IgnoredTestRow where status == Ignored)`, never a frozen
    hand-maintained constant.

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
  `WorkKind`. The `TaskKind` enum split (`Load` + `Parse` + `Analysis` +
  `Artifact` + `CacheNode { cache_id, key_hash }`), the dropped `Copy/Eq/Hash`
  (it derives `Clone, PartialEq, Debug`), `TargetStage`, and the
  `StageExecutor::as_any` + `execute_cache_node` trait surface are ALREADY
  PRESENT (`crates/verter_scheduler/src/stage.rs:60`, `executor.rs:122`/`:138`).
  REMAINING: add `dispatch_cpu_task` and wire a real session-side cache-node
  dispatch (the `execute_cache_node` default impl returns a "not implemented"
  `StageError` requiring override). The `task_kind_for_ready_job` adapter is
  ALREADY GONE; the surviving `unreachable!()` in `admit_work` (`scheduler.rs`)
  is the correct end-state (`Parse` is intrinsic to the source stage and
  `CacheNode` is admitted into the DAG directly by the cache layer, never
  through file-stage admission), not a placeholder to replace.
- **Deps:** B7a/b/c (landed).
- **Parallelism:** Can run parallel with U0 and U2 (scheduler-only surface).
- **Risk:** medium — touches the per-task dispatch chokepoint; must not create a
  second dispatch path.
- **Required deletions:** none — the `task_kind_for_ready_job` adapter this block
  was scoped to delete is ALREADY GONE (0 src hits). `SchedulerJobKind`
  (component-meta batch fan-out) is **RETAINED** and must not alias `TaskKind`.
- **Guards:** keep `dag_arch_guards` (12/12) green; a discriminating test that
  the CacheNode dispatch arm actually routes (not `unreachable!()`); a guard that
  there is exactly one dispatch path (no second `task_kind_for_ready_job`-style
  adapter re-introduced).

---

### U2 — Semantic Key + B4 Cache-Node Convergence  **(CONVERGENCE GATE — highest correctness risk)**

- **Source track:** MERGED (semantic-graph R-1 + cache-runtime B4-completion).
  See the dedicated co-sequencing section (§5).
- **Parity docs:** parent `docs/arch/native-typeinfo-parity.md` (query-key
  taxonomy §2, typed `SemanticQueryValue` value-domain §3) + child
  `docs/arch/native-typeinfo-parity-u2-reducers.md` (the U2 reducer / relation /
  utility / indexed / mapped / template / class / enum / module / JSX blocks).
- **Sub-block decomposition (per the child's block graph).** U2 lands as the
  foundation block **`U2.QUERY_VALUE_DOMAIN`** (the typed `SemanticQueryValue`
  value-domain layer + the seven U2 keys + the three U2-landed added keys + the
  finalized slot-identity SHAPE for the U6-landed `FlowReturn` / `ResolveCall` + the
  `ProjectionDemand` / `EvalPolicy` demand lattice with the five mode names as
  PRESETS + the per-block `SemanticQueryKeySpec` table) followed by the keystone
  reducer **`U2.RELATION_INFER`** (the `Relate` engine + coinductive-SCC discharge +
  relation-owned `InferBind`; the `CheckerTransaction` + `InferenceSession` +
  `InferenceInfo` substrate the `InferenceContextKey` fingerprints lands HERE, not in
  the value-domain block) — **RESCOPE-GATE-REQUIRED (effort weight EXTREME, §3.3):
  deep design produced at its rescope session (depends on `U2.QUERY_VALUE_DOMAIN`).**
  The assignability + inference-session + measured-variance algorithm (the whole TS
  checker core), the `isDeeplyNestedType`-style termination proof (non-convergence
  never WARM-admits), and the per-family `tsgo`-oracle baseline are produced at the
  gate, not asserted in this index; parent §§4.0–4.2 seed it (marker-probe
  fixed-point + coinductive SCC + the session substrate). The
  "route-through-`Relate`" phrasing elsewhere is the one-engine WIRING constraint,
  NOT this phase's algorithm. — and the parallel reducer sub-blocks `U2.INDEXED_ACCESS`,
  `U2.MAPPED_TEMPLATE`, `U2.UTILITIES`, `U2.CLASS_SURFACES`, `U2.ENUMS`,
  `U2.MODULE_AUGMENTATION`, `U2.JSX_FOUNDATIONS`. The parent U2 token is the
  aggregate over every sub-block. The child doc owns the per-sub-block contracts;
  this index lists the names only.
- **Scope (ONE clean cutover — no migrate-twice):**
  0. Land the typed **`SemanticQueryValue`** value-domain layer over the one shared
     dispatch/admission substrate (`{ TypeNode, ProgramAnalysis, DeclarationAnalysis,
     OverloadSet, Relation }`, extended in U6 with `{ FlowReturn, ResolvedCall }`;
     the reserved NON-LIVE `DiagnosticAnalysis(CheckResult)` arm + `Check*` query
     names stay reserved-not-live for the native-checker sibling), every variant
     mapped to exactly ONE value domain (parent §3); and the `ProjectionDemand` /
     `EvalPolicy` **demand lattice** as the PRIMARY demand + cache-identity dimension,
     with the five mode names (`Identity` / `Navigate` / `Shallow` / `Expanded` /
     `Skeleton`) as public presets over it (`Skeleton` = `generic_open =
     TypeParamShells` + carrier-stop, not a special mode) and cache satisfaction by
     the lattice dominance relation, NOT mode-enum order (parent §2.10). The
     satisfaction-relation EXACTNESS gating lands in U10; the lattice DEFINITION
     lands here.
  1. Finalize the **`SemanticQueryKey` identity SHAPE once** (the slot-identity
     model for every variant): migrate existing `Instantiate { base }` /
     `ResolveMacroPayload { owner }` from `DeclKey` → `ResolvedDeclSlotIdentity`
     (slot identity), AND add the **7 U2 variants** in that identity shape
     (`ResolveMergedDeclaration`, **`ResolveDeclarationAugmentation`**,
     `ResolveAmbientNamespace`, `ResolveOverloadSet`, `ResolveEnum`,
     `FlowNarrowingAt`, `ContextualTypeAt`). The seventh variant is the
     **generalized** augmentation key: the former `ResolveModuleAugmentation` slot
     is broadened to `ResolveDeclarationAugmentation { target: Module | Global,
     context: DeclarationAnalysisContext }` so module AND global
     declaration-environment-mutation facts share ONE concrete `SemanticQueryKey`
     identity (resolving to `SemanticQueryValue::DeclarationAnalysis`, never a
     `GraphTypeNode` arm). This is an existing-slot generalization, NOT a sixth U2
     variant — the slot count stays **seven** and the added-key count stays exactly
     **five** (below). Every variant routes through
     `ProjectSemanticDispatch::execute` (the one-engine rule). This finalizes the
     identity SHAPE — later ADDITIVE variants land in that same slot-identity shape
     with NO cache re-key; adding a later variant is additive, not a second
     migration.
     - **Added query keys (exactly five beyond the seven; parent §2.3).** Register
       the five new `SemanticQueryKey` variants in the final slot-identity shape,
       each dispatched through `ProjectSemanticDispatch::execute`:
       - **`ResolveClassSurface`** (lands here, U2) — instance/static heritage with
         generic substitution, member-demand aware (`{ decl_slot, type_args, side,
         demand, context: ClassSurfaceContext }`). Generic `ResolveClass` folds in.
       - **`ApparentType`** (lands here, U2) — primitive/array/constrained-generic
         apparent member lookup via lib facts (`{ base, demand, context:
         ApparentTypeContext }`). Named `ApparentType`, not `GetApparentType`.
       - **`TemplateLiteralReduce`** (lands here, U2) — template-literal distribution,
         intrinsics, and `infer` splitting (`{ pattern, args, context:
         TemplateLiteralReduceContext }`).
       - **`FlowReturn`** (lands in **U6**, additive in this same shape, no re-key) —
         the demand-sliced return/body flow query (`{ function_slot,
         normalized_type_args, context, demand, input }`).
       - **`ResolveCall`** (lands in **U6** with the U2 `ResolveOverloadSet` key as a
         dependency) — reusable call resolution with its own cache identity
         (`{ callee, call_kind, receiver_this, args, explicit_type_args,
         contextual_result, policy, context }`); `CallResolve` is PROMOTED to
         first-class, not folded.
       Each added key carries an explicit per-key `*Context` (split env hashes only,
       R21; no content/version hash or `fact_dep_signature`, R6) and a
       no-cross-context-warm-hit guard. The existing `Relate` key is upgraded in the
       same shape (full relation identity, parent §2.7) — an existing-key upgrade,
       not a sixth added key.
  2. Add the matching producers in `verter_semantic::analysis` (namespace / merge /
     class analysis) + `verter_session::semantic_query`: the type-value
     `SemanticNodeData::{MergedDeclaration, AmbientNamespace, Class, Enum}` producers
     plus the `DeclarationAnalysisGraph` (module + global augmentation) facts that
     `ResolveDeclarationAugmentation` resolves to — declaration/environment-mutation
     facts are NOT smuggled into `GraphTypeNode` (parent §1.3, §3).
     **Cross-file declaration merging lands HERE** (not deferred) per §9.5;
     `ResolveDeclarationAugmentation` rides `FileArtifactStore::augmentation_index`
     (landed), its `AugmentationTargetKey` derived from `DeclarationAnalysisContext`
     at execution time.
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

### U3 — Cache / Fact Model (`U3.CACHE_FACT_MODEL`)

- **Source track:** semantic-graph (cache/fact model end-state, with cache-runtime
  coupling).
- **Parity docs:** child `docs/arch/native-typeinfo-parity-cache-export-session.md`
  (`U3.CACHE_FACT_MODEL` — owns U3 / U8 / U10 / U11 / U12 / U13: facts, wire,
  exporter, DB, session, projections), under parent
  `docs/arch/native-typeinfo-parity.md` (PART 1 §6, the Cache Architecture rules,
  the Canonical Dependency Cache Rule). INDEX entry — the child doc owns the full
  block contract; it is cited, not restated.
- **Scope (the cache / fact-model end-state):** land the typed-admission +
  per-budget non-admission + route-fact-validation rails the result DB (U10), the
  exporter (U12), and the session surfaces (U11) all admit through. Concretely:
  the typed three-layer `BudgetExceeded` non-admission for every hot reducer budget
  (`RelationBudget` / `KeyspaceBudget` / `CallResolutionBudget` / `FlowSliceBudget` /
  apparent-type member-demand), each routing through `ReturnOnly`; the multi-candidate
  `FamilySlots` admission substrate with **per-family adaptive `candidate_cap()` +
  invalid-first/LRU-by-valid-hit eviction + a global memory ceiling** (replacing the
  uniform `FAMILY_SLOT_CANDIDATE_CAP = 4` FIFO); the five-dimension env-hash split
  (R21) with `lib_env_hash` entering only the layers whose value depends on lib data;
  content-free `DeclKey` query-identity keys (R6) with `ReadSetSignature.facts` as the
  sole validity rail; and route-fact validation / invalidation (selected-leaf-edit
  propagation, barrel-route redirect + prior-leaf drop, package-source-change
  invalidation, single-materialization collapse, VFS-authoritative). The B8
  bespoke-invalidation deletion is a COMPONENT of this fact model: DELETE
  `component_meta_caches.rs` per-DB `clear_*` reverse-dependent eviction authority
  (replaced by validated lazy revalidation per B4 / skill R3), and remove
  `DeclIdentity` as a key field on any `SemanticQueryKey::*` variant — these are not
  a competing block. Lifts the three `cross_file.rs` `CrossFileResolution`
  route-demand rows.
- **Deps:** **`U8.WIRE_SURFACE_CLOSURE`** (the typed admission this block enforces
  produces values whose wire shape U8 closes — it lands AFTER the wire closure) +
  U2 (parent, the reducers it bounds/admits) + U6 (parent, the flow solver it
  bounds/admits). NOT parallel-to-U8.
- **Parallelism:** Semantic-graph lane, after U8 (the cache/fact rail the result DB
  + exporter + session ride on); runs beside the cache-runtime lane (U4/U5/U7/U9).
- **Risk:** medium-large — correctness-sensitive (typed-admission + invalidation-
  authority change).
- **Required deletions:** the uniform `FAMILY_SLOT_CANDIDATE_CAP = 4` constant +
  FIFO candidate eviction (→ per-family `candidate_cap()` + invalid-first/LRU-by-
  valid-hit eviction + global ceiling); any boolean/sentinel/side-channel cache
  admission (→ the typed `ComputeAdmission` enum); any bundled `project_config_hash`
  (→ the R21 five-dimension split); any content/version hash or `fact_dep_signature`
  on a query-identity KEY (→ version rooting on the value, R6); `component_meta_caches.rs`
  per-DB `clear_*` reverse-dependent eviction; `DeclIdentity` from `SemanticQueryKey::*`;
  any route-fact revalidation that is file-hash-only.
- **Guards (per the child's named set):** `relation_budget_exceeded_admits_nothing`,
  `keyspace_budget_exceeded_admits_nothing`, `call_resolution_budget_exceeded_admits_nothing`,
  `apparent_type_budget_exceeded_admits_nothing`; `program_analysis_fact_domain_validates_flow_slice`
  (the fourth closed `FactDomain::ProgramAnalysis` dispatch home); `cache_candidate_cap_is_per_family_not_uniform`;
  `family_eviction_prefers_invalid_then_lru_valid_hit`; `cache_keys_cover_ts_jsx_moduleresolution_decorator_lib_dimensions`;
  `instantiation_depth_policy_in_identity_and_facts`; `persistent_caches_never_admit_overlay_only_results`;
  `architecture_minimizes_fallback_entry_not_fallback_cost`. A guard that no
  `SemanticQueryKey` variant contains `DeclIdentity`; a guard that reverse-dependency
  graphs are not invalidation authority (validated lazy revalidation only); regression
  tests that same-canonical edits are caught by strict self-root validation and
  cross-file edits invalidate lazily through recorded facts.

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
- **Deps:** B2 only (per inter-block DAG); best landed after U2 (observability is
  most useful once the cache nodes are enumerated).
- **Parallelism:** B2-gated; can run parallel with the U4/U6 cache-runtime work
  and the semantic-graph lane (U8/U3+).
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
- **Parity docs:** child `docs/arch/native-flow-return.md` (the U6 flow chapter —
  the demand-sliced `ReturnPathPeeker` two-frontier model + the flow IR), under
  parent `docs/arch/native-typeinfo-parity.md`. The flow-return coverage detail
  lives in this in-repo doc and the parent's coverage table (§10.4 / §10.4.1), never
  a scratch/temp artifact.
- **Scope:** The demand-sliced flow architecture is documented at
  `docs/arch/native-flow-return.md` (parent §5 owns the cross-cutting contract). The
  keystone sub-block `U6.FLOW_RETURN_SUBSTRATE` lands the four artifacts: the
  arena-free `FunctionBodySkeleton` (no type lowering, under `IndexedReady`), the
  sparse per-function **`FunctionFlowGraph`** built ONCE from the skeleton with typed
  edge classes (value-def / path-write / eval-effect / narrowing-predicate /
  control-region / closure-escape / loop-summary / try/finally-override; no
  build-time type lowering), the **`ReturnPathPeeker`** graph demand PLANNER
  (the slice = graph reachability from `(return_site | expression_site,
  projection_path, EvalPolicy)`, the two-frontier rule as edge classes — not a
  procedural CFG walk), and the slice substrate `FlowSliceHashNode` /
  `FlowSliceLoweredBodyNode` / `FlowSliceIR` as B4-style cache-runtime nodes
  (`ArtifactNode` impls under U4, NOT bespoke `FileArtifactStore` side maps). Add the
  additive `SemanticQueryKey::FlowReturn` AND `SemanticQueryKey::ResolveCall` query
  nodes — each as enum variant + `SemanticQueryKeySpec` row + dispatch behavior
  together, in the U2-finalized slot-identity shape with NO cache re-key, routed
  through `ProjectSemanticDispatch::execute` (parent §2.4 / §2.5). Flow node / fact
  identity is rooted by the per-function **`flow_body_stable_hash`** — body-SENSITIVE,
  cosmetic-INSENSITIVE — NOT the decl-skeleton `parse_stable_hash` (so
  `return { b: 1 }` and `return { b: 2 }` key distinct slices). The `FlowSlice` fact
  lives in the fourth closed **`FactDomain::ProgramAnalysis`** domain, validated
  fail-closed by `StoreView::validates_program_analysis_domain`. Slice-hash
  production is SPLIT from slice lowering (`FlowSliceLoweredBodyNode::compute` must
  NOT call the slice-hash producer). **The narrowing surface re-casts as ONE hard
  lattice-substrate block + SEVEN cheap per-mechanism additions** (front-load the
  hard one): the **`FlowFrame` branch-fact lattice substrate + the JOIN ALGEBRA for
  conflicting predicates** is the single hard block (its narrowing join / meet for
  conflicting predicates is the design difficulty); once that lattice exists, the
  seven per-mechanism sub-blocks (`U6.NARROW_TYPEOF` / `_EQUALITY` / `_TRUTHINESS` /
  `_IN` / `_INSTANCEOF` / `_DISCRIMINATED` / `_SUBSTITUTION`, plus the
  invalidation/preserve frame they share) are **cheap additions** that each fill in
  the facts their mechanism carries onto the SAME lattice — no second flow structure.
  The remaining sub-blocks (`U6.PREDICATE_ASSERTION`, `U6.CALL_RESOLVE`,
  `U6.CONTEXTUAL_CALLBACK`, `U6.VALUE_INFERENCE`, `U6.ASYNC_GENERATOR`,
  `U6.CROSS_FILE`, `U6.LOOP_CLOSURE`) add their edge classes to the same graph. The
  child doc + parent §5 / §10.4.1 own the per-sub-block contracts and the row
  partition; this index does not restate them. The reserved `ExecutableRegionId` / `ExecutableRegionKind::Function` region
  abstraction + the `ProgramAnalysisContributor` injection seam are NON-LIVE / future
  (reserved-not-built; the native-checker sibling consumes them later).
- **Deps:** U2 + U4.
- **Parallelism:** Cache-runtime lane; beside the semantic-graph lane.
- **RESCOPE-GATE-REQUIRED (effort weight EXTREME, §3.3): deep design produced at its
  rescope session (depends on `U2.RELATION_INFER`).** The cross-engine
  cycle/termination — `FlowReturn` ↔ `ResolveCall` ↔ narrowing ↔ `ContextualTypeAt`,
  the `CheckerReentryGraph`, and flow narrowing as a dataflow fixed-point — is
  designed at the gate to executable-pseudocode depth: runtime SCC detection +
  provisional-result-during-discharge + a stability decision + **fail-closed on
  non-convergence**, the narrowing JOIN ALGEBRA, the loop fixed-point, and the
  per-family `tsgo`-oracle baseline. The block contract below names the artifacts /
  guards; the recursion algorithm + its termination proof are the gate's deliverable,
  not asserted here.
- **Risk:** large — adds the `FunctionFlowGraph` + the `FlowReturn` / `ResolveCall`
  query nodes + the slice cache-runtime nodes.
- **Required deletions:** the legacy `type_eval_build.rs` lightweight return scanner
  (`infer_return_type` / `infer_expression_type` / `collect_return_types` /
  `extract_object_literal_as_type` / `append_spread_array_element_types` — the
  arena-borrowing OXC walker) is cut over to the demand-sliced resolver (child
  Legacy-deletions); no second flow engine survives.
- **Guards:** `flow_slice_lowered_body_does_not_compute_slice_hash` (the hash-then-
  lower split); `flow_slice_keys_on_body_sensitive_hash_not_parse_stable_hash`;
  `function_flow_graph_built_once_per_function_skeleton`;
  `flow_slice_is_graph_reachability_not_procedural_walk`;
  `flow_graph_effect_edges_stay_live_past_value_writes`;
  `flow_slice_budget_exceeded_admits_nothing` (fail-closed `BudgetExceeded` →
  `ReturnOnly`); `FlowReturn` / `ResolveCall` route through the one engine.

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
- **Parallelism:** Can run parallel AFTER U1, alongside the U4/U5/U6
  cache-runtime work and the entire semantic-graph lane (U8/U3+; the typeinfo DB
  does NOT ride `submit_dag` per §2.1).
- **RESCOPE-GATE-REQUIRED (effort weight HIGH / justify-or-cut, §3.3): deep design
  produced at its rescope session (depends on U1).** The gate must **justify the
  cache-node DAG against a MEASURED workload, or CUT it** — a model-checked DAG for
  an I/O-bound LSP already served by singleflight is an over-engineering risk (the
  typeinfo DB explicitly does NOT ride `submit_dag` per §2.1, narrowing the workload
  that would justify it). The block contract below stands only if the rescope gate
  confirms the workload; otherwise the gate re-scopes/cuts it.
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

### U8 — Wire-Surface Closure

- **Source track:** semantic-graph (R-2).
- **Parity docs:** child `docs/arch/native-typeinfo-parity-cache-export-session.md`
  (`U8.WIRE_SURFACE_CLOSURE` — the keystone block of that subplan), under parent
  `docs/arch/native-typeinfo-parity.md` (wire-surface purity §§1.3–1.5, the
  `TypeInfoGraphPayload` shape, the Typeinfo Wire Contract). This is an INDEX entry —
  the child doc owns the full block contract; it is cited, not restated.
- **Scope (whole-surface wire-purity end-state, NOT the exporter):** reconcile the
  ENTIRE public proto surface with the moved-concept end-state under the Typeinfo Wire
  Contract. Introduce `TypeInfoGraphPayload { graph, program_analysis,
  declaration_surfaces, diagnostics, diagnostic_directives, relation_proofs }`, the
  sibling `ProgramAnalysisGraph { flow_narrowings, contextual_types }` and
  `DeclarationAnalysisGraph { module_augmentations, global_augmentations }`, the
  `RelationPayload` + payload-side `relation_proofs` proof table; retire-and-`reserved`
  every relocated/retired `GraphTypeNode` arm (flow-narrowing 26 / contextual-type 27 /
  relation-proof 28 / module-augmentation 23 / global-augmentation 25), the
  `SemanticTypeGraph.diagnostics` (9) and `GraphTypeParameter.no_infer` (9) fields;
  migrate every public `SemanticTypeGraph` embedding (`TypeInfoGraphResponse.graph`,
  `FrameworkSurfacePayload.graph`) additively to a `TypeInfoGraphPayload` carrier at a
  fresh tag; bump `SemanticTypeGraph.schema_version`; extend
  `SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS` + the closed-set validator; regenerate the
  byte-equal TS bindings. The exporter that POPULATES this shape is U12; this block
  only closes the shape every later block reads/writes.
- **Deps:** U2 (parent) + U6 (parent) + **S5.B12** — the wire surface carries the U2
  type-value arms and the U6 flow/relation facts, so it closes only once those producers
  exist; AND S5.B12 must be done first (the HARD GATE `U8 ← {U6, S5.B12}` of §3.1.1 /
  §3.1.3: do NOT build the new wire/result/export stack around the `VueMacroElements` /
  `HostResolvedNamedTypeKey` / `resolve_type/` sidecar that S5.B11/B12 delete). It does
  NOT depend on the exporter (U12) or the result DB (U10) — it is their prerequisite.
- **Parallelism:** Semantic-graph lane head (the keystone every other cache/export
  block — including U3 — depends on); runs beside the cache-runtime lane (U1/U4–U7/U9).
- **Risk:** large (whole-proto closure + a schema_version bump + byte-equal TS
  regeneration); mechanical once the moved-concept homes are fixed.
- **Required deletions:** the relocated `GraphTypeNode` arms (26/27/28/23/25),
  `SemanticTypeGraph.diagnostics` (9), `GraphTypeParameter.no_infer` (9) — retired +
  `reserved`, never reused; the `SemanticTypeGraph graph = 1` / `FrameworkSurfacePayload.graph = 4`
  server-populated embeddings (retired/`reserved` or downgrade-only, replaced by fresh
  `TypeInfoGraphPayload` carriers). No field number is reused.
- **Guards:** `graph_type_node_oneof_contains_only_type_value_arms`,
  `no_non_type_value_smuggled_into_graph_type_node`,
  `typeinfo_wire_surface_has_no_retired_concept_fields`,
  `flow_contextual_facts_not_graph_type_nodes`,
  `program_analysis_graph_exposes_flow_contextual_queries`, `relation_proofs_not_graph_type_nodes`,
  `no_infer_not_type_parameter_metadata`, `diagnostics_only_on_typeinfo_graph_payload`,
  `all_public_semantic_type_graph_embeddings_are_payload_wrapped`; plus the four
  wire-contract guards (proto/TS oneof parity, byte-equal TS freshness, audit parity,
  request validation).

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

### U10 — Result DB + Mode/Demand Exactness

- **Source track:** semantic-graph (R-3 + R-4).
- **Parity docs:** child `docs/arch/native-typeinfo-parity-cache-export-session.md`
  (`U10.RESULT_DB`), under parent `docs/arch/native-typeinfo-parity.md` (§§5–6, the
  fact-based cache architecture). INDEX entry — the child doc owns the full contract.
- **Scope:**
  - `TypeInfoGraphResultDb` on `ProjectTypeStore` — the query-identity final-result
    cache handing out immutable `Arc<TypeInfoGraphPayload>`, validating each candidate's
    `ReadSetSignature.facts` against the live `StoreView` on every warm hit, cold-admitting
    through singleflight (`cooperative_admit_with_post_publish`), with the multi-candidate
    `FamilySlots` storage under the per-family adaptive cap + invalid-first/LRU-by-valid-hit
    eviction + global memory ceiling (the U3 substrate). Its key excludes content/version
    hashes + `fact_dep_signature` (R6) and includes `lib_env_hash` (R21). The
    completion-fence `publish_with_retry` consumes the canonical `MAX_INFLIGHT_RETRIES = 3`
    (NO second retry constant); overlay-only results never populate this base/persistent DB.
  - The mode / demand / expansion-boundary **EXACTNESS** gating over the U2 reducers,
    stated over the `ProjectionDemand` / `EvalPolicy` demand lattice (parent §2.10) of
    which the five mode names are public presets: `Identity` returns the alias decl
    identity; `Shallow` exposes one shell level (operator carriers stay `Ref`);
    `Expanded` `keyof T` is bounded to T's shallow member-name surface; `Navigate` runs
    the intermediate hops; `Skeleton` is the `TypeParamShells` + carrier-stop preset.
    `Pick`/`Omit`/inline/local/imported projection is path-precise (no unpicked/excluded/
    unselected branch loaded). Cache satisfaction/backfill is by the demand-lattice
    dominance relation, NOT mode-enum order.
- **Deps:** U3 (the typed-admission + `ReadSetSignature` validity rail this DB admits
  through) + U8 (the closed `TypeInfoGraphPayload` shape it caches) + U2/U6 parents
  (the reducers + flow solver it gates). **Per §2.1: built on the singleflight
  substrate, NOT on `submit_dag`.**
- **Parallelism:** Semantic-graph lane; cache-runtime lane (U7/U9) is unrelated.
- **Risk:** medium-large — the fence/admission contract is invariant-dense (warm-exact-only,
  3-retry, no-partial-admit, zero-alloc warm hit) and the mode/demand exactness is the
  gate that confirms the U2 reducers are path-precise at the published boundary.
- **Required deletions:** any final-result cache keyed on content/version hash or
  `fact_dep_signature` (R6); any `Identity`/`Shallow`/`Expanded` path that returns the
  alias body / reduces a `Pick<…>` member body / walks member bodies into the keyspace;
  any eager sibling materialization during projection. Do NOT resurrect
  `finalise_signature_or_empty` — build on `FactReadSet::finalise` + `SignatureAdmission`.
- **Guards:** `cache_satisfaction_is_demand_lattice_not_enum_order` (lands here); the
  U3 multi-candidate-substrate guards exercised through `TypeInfoGraphResultDb`
  (`cache_candidate_cap_is_per_family_not_uniform`, `family_eviction_prefers_invalid_then_lru_valid_hit`,
  `persistent_caches_never_admit_overlay_only_results` — must not regress); the
  demand-lattice DEFINITION guards `query_modes_are_presets_over_projection_demand_eval_policy`
  / `skeleton_is_typeparamshells_plus_carrier_stop_not_special_mode` (defined at U2,
  depended on here); a guard that there is no second retry constant; a zero-alloc warm-hit
  test (no audit payload allocation without accumulator).

---

### U11 — Public Relation / Session Surfaces + Audit Execution

- **Source track:** semantic-graph (R-6 + R-5).
- **Parity docs:** child `docs/arch/native-typeinfo-parity-cache-export-session.md`
  (`U11.PUBLIC_RELATION_SESSION`), under parent `docs/arch/native-typeinfo-parity.md`
  (§4, PART 2 §§10–11, the audit infrastructure). INDEX entry — the child doc owns the
  full contract.
- **Scope (the session boundary the exporter feeds):**
  - Public `relate()` returning the public `RelationPayload` (outcome / inference
    bindings / relation proof + typed `BudgetExceeded`), the proof carried off the
    type-values surface via the payload-side proof table — NOT the bare tri-state
    `RelationResult`. `relate` is the sole assignability authority.
  - The request-footprint attachment pipeline on every audited typeinfo resolver path
    when `footprint_capture=true` (the host-audit passive-observer footprint miner):
    the footprint reports the requested import / projected indexed-access members
    precisely, excludes unprojected branches, and stays attached on warm reads.
  - End-to-end edit-cycle cache invalidation at the session boundary over the U3
    route-fact rails: selected-leaf edit flips the surface; unselected-sibling edit
    keeps the warm cache (zero VFS reads / zero RouteDb misses, footprint attached);
    barrel edit redirects the route + drops the prior leaf from the V2 footprint;
    augmentation-patch edit surfaces the augmented shape; in-place package edit flips
    the surface.
  - The session execution surface: the `_with_audit` methods returning
    `AuditedResult<Arc<...>, TypeInfoRequestError>`, validate-before-execute, routed
    through `TypeInfoGraphResultDb`; per-entry-point cold / warm / degraded
    `StructuredAuditEvent` emission (warm = counter only), the
    `TypeInfoGraphFootprintCell` in `footprint_miner.rs`, nested-record semantics for
    `expand_graph_around`, `wave_3_entry_points_propagate_tls` extended with the
    typeinfo drivers. **Use `exactness_counts: BTreeMap` (§2.2), not `exactness_*`.**
  - **Schema-version RUNTIME (server side; A.5/A.6 — the runtime layer on top of A0a's
    contract GATE).** Per-request schema-version ECHO (every response carries the
    negotiated `schema_version`); runtime NEGOTIATION via
    `SchemaVersionCapabilities::validated_supported_versions()` restricted to versions
    backed by a registered encoder; a `V < current` session emits via the U12
    `encode_typeinfo_payload_for_version(V, payload)` downlevel path (co-landed with U12
    so negotiation is never advertised without a backing encoder).
- **Deps:** U12 (the exporter whose `TypeInfoGraphPayload` / `RelationPayload` this
  surface returns) + U3 (the route-fact rails footprint/invalidation observe) + U8 +
  U2/U6 parents. A0a request validators + `StructuredTypeExpression` proto + the
  schema-version handshake gate + `Relate` memo are landed.
- **Parallelism:** Semantic-graph lane.
- **Risk:** medium-large.
- **Required deletions:** the bare tri-state `RelationResult` on the public path
  (replaced by `RelationPayload`); any resolver path that records the scratch/owner
  footprint without attributing projected imported members precisely; any cache
  participant that invalidates on an unreferenced barrel sibling edit.
- **Guards:** `relate_query_value_carries_relation_proof_and_budget_state`,
  `relation_proofs_not_graph_type_nodes`,
  `typeinfo_relate_payload_exposes_relation_proof_without_graph_type_node` (kept green at
  the session boundary); validate-before-execute coverage; the audit 3-branch
  (cold/warm/degraded) discriminators; `wave_3_entry_points_propagate_tls` extended and
  discriminating; `every_typeinfo_request_carries_schema_version` (per-request echo);
  `server_supported_versions_have_encoders` (never advertise a version without a
  registered encoder).

---

### U12 — TypeInfo Graph Exporter

- **Source track:** semantic-graph (R-2 + R-7).
- **Parity docs:** child `docs/arch/native-typeinfo-parity-cache-export-session.md`
  (`U12.EXPORTER`), under parent `docs/arch/native-typeinfo-parity.md` (wire-surface
  purity §§1.3–1.5, §3, the Typed-IR-Only Resolver Rule). INDEX entry — the child doc
  owns the full contract.
- **Scope (the request → graph projection; a thin projection of the engine's typed
  results, NOT a second resolver):** `crates/verter_session/src/typeinfo/{surface,raise,
  evaluate_type_expression,resolve_named_symbol}.rs` — project the engine's typed
  `SemanticQueryValue` results into the closed `TypeInfoGraphPayload`: type values onto
  the closed `GraphTypeNode` type-value allowlist, flow/contextual facts onto
  `ProgramAnalysisGraph`, declaration/environment facts onto `DeclarationAnalysisGraph`,
  diagnostics onto their side tables, relation proofs onto the payload-side
  `relation_proofs` table by opaque proof id, and the `RelationPayload` for public
  `relate`. No non-type value is materialised as a `GraphTypeNode` arm; the exporter does
  NO query-time resolution (it dispatches the U2 queries — e.g. `ResolveMergedDeclaration`
  — rather than re-deriving merge/augmentation structure from `IndexedReady`). It
  publishes the payload into `TypeInfoGraphResultDb` (U10) via cooperative admission.
  The Rust→wire encoding helpers live in `crates/verter_protocol/src/typeinfo/graph.rs`.
  - **Schema-version downlevel ENCODERS (A.5/A.6 — the Rust encoder backing for U11's
    negotiation, co-landed across U11/U12).** `encode_typeinfo_payload_for_version(V,
    payload)` + the `KNOWN_VARIANTS_AT_VERSION` cumulative-exhaustive table (per-version
    `&[VariantId]` sets). Newer-only variants project to compatible substitutes for
    `v(N-1)` consumers (e.g. post-V `ExpansionStatus::ExactOpenGeneric` → `ExactSymbolic
    { reason: GenericPreserved }`); unsupported constructs degrade through
    `UnsupportedConstruct::DowngradedFromNewerSchema` → `…::Unsupported { construct:
    "schema_skew" }` → `Opaque(Miss { reason: SchemaSkew })`. Each encoder is validated
    against `KNOWN_VARIANTS_AT_VERSION[target_version]` (NOT `encoder.version`).
- **Deps:** U10 (the result DB the exporter's payload is admitted into) + U8 (the closed
  payload shape it populates) + U3 + U2/U6 parents. The session surfaces (U11) and the
  TS projection (U13) consume the exporter's output.
- **Parallelism:** Semantic-graph lane.
- **Risk:** large (wide lowering table); mechanical once U2 is correct (the exporter is a
  thin projection).
- **Required deletions (one clean cutover, with their replacements):**
  - Any exporter path that emits flow / contextual / relation-proof / module-or-global
    augmentation facts as `GraphTypeNode` arms (relocated to the payload side tables).
  - Any exporter path that re-resolves a type at projection time (the one-resolver rule).
  - The legacy scratch text-evaluator (`typeinfo/{evaluate_type_expression,
    scratch_cache}.rs`) once `StructuredTypeExpression` dispatch (U12) lands — the
    `StructuredTypeExpression`→`SemanticQueryKey` decode that obsoletes the
    text-evaluator lands in U12 itself, via the `resolve_named_symbol.rs` request
    entrypoint (U11 DEPENDS on U12 per the U11 child `Prerequisites: U12.EXPORTER`, so
    U12 cannot gate its own deletion on U11). The text-evaluator BODY is gutted from
    `evaluate_type_expression.rs` while the TYPED per-node raise/evaluate path in that
    same file SURVIVES (policed by
    `evaluate_type_expression_does_not_call_parse_type_annotation`); `scratch_cache.rs`
    is deleted outright. The `verter_protocol` `GraphBuilder` / `graph/schema/*` /
    `graph/mod.rs` re-exports
    (≈36KB) with the legacy proto `EvaluateTypeExpressionRequestDto` fields `reserved`;
    rename `symbol_inventory.rs` → `list_symbols.rs`. (The legacy NAPI/WASM typeinfo
    entries + the pure-TS decoder/component-meta type-graph files are deleted in U13,
    same clean cutover.)
- **Guards:** the U8 wire-purity guards over the exporter's OUTPUT
  (`no_non_type_value_smuggled_into_graph_type_node`,
  `program_analysis_graph_exposes_flow_contextual_queries`, `relation_proofs_not_graph_type_nodes`,
  `all_public_semantic_type_graph_embeddings_are_payload_wrapped` — kept green); a guard
  that the exporter is pure (no second resolution path; the scratch evaluator's
  `parse_type_annotation` use no longer exists in the resolver pipeline);
  `known_variants_at_version_rows_are_cumulative_exact_sets`,
  `downgrade_encoder_never_emits_variant_unknown_to_target_version`,
  `known_variants_table_matches_proto_at_version` (CI — table regenerated from proto).

---

### U13 — Published Projection (GraphTypeNode + TS TypeDescriptor)

- **Source track:** semantic-graph (R-9 + R-10).
- **Parity docs:** child `docs/arch/native-typeinfo-parity-cache-export-session.md`
  (`U13.PROJECTION`), under parent `docs/arch/native-typeinfo-parity.md` (§1, §8, the
  Typed-IR-Only Resolver Rule, the Component-Meta Native vs Compat Rule). INDEX entry —
  the child doc owns the full contract.
- **Scope (the published-surface block U14/U15 + host-backed consumers read):**
  - The closed `GraphTypeNode` type-value projection of every U2 type value on the
    published `TypeInfoSurface` (only the allowlist arms appear; no non-type value in the
    projection — the moved-off facts are read from the relocated side tables).
  - The TS `TypeDescriptor` projection consuming the wire payload STRUCTURALLY:
    `packages/type-ir/src/type-ir.ts` (the `TypeDescriptor` schema — any missing variant
    added here, never recovered through text); `packages/component-meta/src/{type-graph-decode,
    type-graph-proto-decode}.ts` + `compat/native-projection.ts` (structural decode of the
    wire `TypeInfoGraphPayload`); the compat interop (`compat/checker.ts`,
    `compat/schema.ts`) reading `prop.type` (`TypeDescriptor`) for every semantic decision
    (`prop.rawType` display-only; type-role classification structural, not identifier
    suffix). The FFI binary-protobuf surface (`verter_napi/src/typeinfo.rs` +
    `verter_wasm/src/typeinfo.rs`, `Buffer` / `Uint8Array`) + the public TS session
    (`packages/typeinfo/src/{session,index}.ts`) + the projection packages
    (`projections/{display, type-descriptor, json-schema, zod, storybook, docs}/`, cycle-id
    `z.lazy` memoisation, `SharedLoadReuse` audit-terminal skip) wire through. `TypeDescriptor`
    becomes a projection target.
- **Deps:** U12 (the exporter whose `TypeInfoGraphPayload` this projection decodes) + U8
  (the closed wire shape it reads).
- **Parallelism:** Semantic-graph lane.
- **Risk:** medium-large (the structural decode + 6 projections); this block owns ZERO
  `IgnoredTestRow`s (a substrate block — every published type-value row stays owned by its
  computing U2/U6 block per §10.4.1).
- **Required deletions (one clean cutover):** any published projection / TS compat branch
  that drives a semantic decision from `prop.rawType` / a raw/display string (`looksLike*`
  / `extract*` / `normalize*` / `split*` / `strip*` / `prefer*` / `repairOpaque*`) or from
  an identifier-name suffix (`name.ends_with("Props")`); the descriptor-bridge
  (`descriptor-to-native.ts` / `native-to-descriptor.ts`, A.20) + compat semantic-recovery
  hooks; the legacy NAPI/WASM typeinfo entries (consuming `EvaluateTypeExpressionRequestDto`),
  `packages/typeinfo/src/{types, native-type-expr}.ts`, and `packages/component-meta/src/{type-graph*,
  type-expr-bridge}.ts` (the 8 legacy type-graph files); any AST/source fallback or second
  TS resolver/expander; legacy entry-point names.
- **Guards:** the `@verter/component-meta` no-rawtype-reads contract
  (`packages/component-meta/tests/no-rawtype-reads.spec.ts`) extended to the projection
  packages; the published-surface constants parity
  (`crates/verter_audit/tests/published_surface_constants_match_ts_port.rs`); the U8
  wire-purity guards kept green over the published surface;
  `capability_rows_map_to_expected_query_fact_mechanisms` (no published-surface row mapped
  to U13); projection round-trip discriminators.

---

### U14 — Vue Framework Adapter Rebuild

- **Source track:** semantic-graph / component-meta (R-11).
- **Parity docs:** child `docs/arch/native-typeinfo-parity-adapters-final-lift.md`
  (owns U14 / U15 — framework adapters, integrations, final lift), under parent
  `docs/arch/native-typeinfo-parity.md`.
- **Scope:** Rebuild `@verter/component-meta` as a thin `FrameworkSurfacePayload`
  adapter + `FrameworkAdapterRegistry` (A.15); `compat` as a projection wrapper.
  Fix the 4 known Vue mismatch cases: Popover `SlotProps<M>`, theme-alias display,
  `Button["variants"]["color"]` indexed-access, ContentSearch intersection. Lifts
  the single `MacroResolution` row (`Parameters<NonNullable<T['slot']>>[0]` slot-payload
  extraction routed through the shared `IndexedAccess` / `NonNullable` / `Parameters`
  reductions + the U6 call signature).
- **Deps:** U13 + U11 (the published projection + the public session surface the
  adapter consumes; and transitively U2/U6/U8/U10/U12).
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
- **Parity docs:** child `docs/arch/native-typeinfo-parity-adapters-final-lift.md`
  (the U14 / U15 framework-adapter + integrations + final-lift blocks), under parent
  `docs/arch/native-typeinfo-parity.md` (terminal acceptance §10.5 / §12).
- **Scope:** Zod/schema client helpers; LSP hover→graph+display,
  completion→framework-surface, MCP `typeinfo.*` / `component-meta.*` tools,
  playground type explorer; lift **EVERY one of the 363 `IgnoredTestRow`s to
  `Lifted`** on the U0 manifest schedule — **zero remaining parity `#[ignore]`s**.
  The ONLY permitted residual `#[ignore]`s are the registered Svelte/React
  STOP-gate files (`svelte_adapter_stop_gate.rs`, `react_adapter_stop_gate.rs`),
  which are NOT among the 363. The binding total is exactly 363 `IgnoredTestRow`s
  (count-guarded + bijective with the source `#[ignore]`s; the ~384 figure is the
  raw `#[ignore]` SITES before macro-family collapse, and `AdditionalProofRow`
  coverage fixtures are excluded from the count); do NOT hard-code a stale absolute,
  and do NOT settle for a majority/fraction — terminal acceptance requires all 363
  `Lifted` (parent §10.5 / §12 `all_typeinfo_parity_rows_lifted_except_stop_gates`).
  Plus the B12 typed bench schema: `BenchResultRow`
  (`packages/benchmark/src/cache-runtime-bench.ts`) reporting cache mode /
  source-map policy / batch shape / thread count / hit count / fallback count;
  vendored cm corpus benches (`component_meta_cold` / `_warm`);
  `MAX_TEST_TIMEOUT` + `test_support/timeout.rs`. Plus the PART 1 §6.2
  **performance-contract benches as perf-regression-gated TERMINAL ACCEPTANCE**
  (not merely the functional gate): the **Verter-vs-TS/tsgo fixtures**
  (`packages/benchmark/src/verter-vs-tsgo-bench.ts`) running Verter and TS/tsgo over
  the SAME in-scope semantic queries (component-meta resolution, projected typeinfo,
  IDE hover/completion, selected member expansion off a large surface, the
  `ReturnType<typeof f>["b"]` demand-slice case), each reported via `BenchResultRow`;
  and the **per-family fallback-bound benches** (`test_support/perf_contract.rs`)
  declaring each query family's (`FlowReturn` / `ResolveCall` / `Relate` /
  `Instantiate` / `Conditional` / `MappedType` / `ResolveClassSurface` /
  `ApparentType` / `TemplateLiteralReduce` / the projection·demand-lattice families)
  fallback-count bound on the vendored corpus and FAILING the bench when a family
  exceeds it. The reserved native-checker seams (`SemanticQueryValue::DiagnosticAnalysis`,
  the `Check*` names, `ExecutableRegionId`, `ProgramAnalysisContributor`) stay
  reserved-not-live; the sibling `docs/arch/native-checker.md` is the LATER non-parity
  layer over the same resolver (NOT part of the U0–U15 parity scope).
- **Deps:** all code-producing blocks (U0–U14).
- **Parallelism:** terminal — runs last.
- **Risk:** large aggregate; lower per-item risk (mostly integration + un-ignoring
  + gating).
- **Required deletions:** any remaining legacy entry-point names surfaced by the
  final sweep.
- **Guards:** the extended two-table-ledger guards (the backing guards on the
  A0a-landed `typeinfo_ignored_test_manifest.rs`): `ignored_test_row_table_holds_exactly_363_rows`
  (binding-total + table disjointness), the exact-363 count/bijection guards
  (`total_ignored_typeinfo_test_count_matches_expected` +
  `manifest_length_matches_documented_total` track the DERIVED live
  `count(status == Ignored)` row state, not a hard-coded stale absolute and not a
  tracked-cursor/lease transaction), and the terminal
  **`all_typeinfo_parity_rows_lifted_except_stop_gates`** (asserting every one of the
  363 `IgnoredTestRow`s is `Lifted` and the only residual `#[ignore]`s are the
  registered Svelte/React STOP-gates); `merged_interfaces_across_files` 5-property
  test (§9.5) green; Svelte/React STOP-gate guards
  (`svelte_adapter_stop_gate_is_registered_out_of_scope`,
  `react_adapter_stop_gate_is_registered_out_of_scope`); the perf-contract guards
  `bench_result_row_reports_cache_mode_sourcemap_batch_thread_hit_fallback` +
  `architecture_minimizes_fallback_entry_not_fallback_cost` (the PART 1 §6.2
  governing-rule guard — per-family fallback ENTRY count under bound, warm path
  O(validate); perf-regression-gated terminal acceptance); `no_vacuous_parent_u_block_landing`;
  hermeticity guard (`external_corpus_paths_not_present_outside_gated_tests`) — bench
  corpora are vendored, no `.integration-tests/repos/<third-party>/`.

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

2. **7 U2 variants in the final shape.** `ResolveMergedDeclaration`,
   **`ResolveDeclarationAugmentation`**, `ResolveAmbientNamespace`,
   `ResolveOverloadSet`, `ResolveEnum`, `FlowNarrowingAt`, `ContextualTypeAt` —
   each added directly in the slot-identity shape, each dispatched through
   `ProjectSemanticDispatch::execute` (the one-engine rule). The seventh variant is
   the **generalized** augmentation key: the former `ResolveModuleAugmentation` is
   broadened to `ResolveDeclarationAugmentation { target: Module | Global, context:
   DeclarationAnalysisContext }` so module AND global declaration-environment-mutation
   facts share ONE concrete identity (parent `native-typeinfo-parity.md` §2.1–§2.2).
   This is an existing-slot generalization, **not a sixth U2 variant** — the slot
   count stays seven. The type-value `SemanticNodeData` producers
   (`MergedDeclaration`, `AmbientNamespace`, `Class`, `Enum`) land in the same block
   alongside the `DeclarationAnalysisGraph` augmentation facts (module + global) that
   `ResolveDeclarationAugmentation` resolves to. Cross-file declaration merging is
   implemented here, not deferred (§9.5).

   **Five added keys beyond the seven (parent §2.3) — register them in this same
   slot-identity shape.** `ResolveClassSurface`, `ApparentType`, and
   `TemplateLiteralReduce` land HERE (U2); `FlowReturn` and `ResolveCall` land in U6
   as additive variants in the identical shape (no cache re-key). `Relate` is an
   existing-key upgrade to its full relation identity (parent §2.7), not a sixth
   added key. The added-key count is exactly five; every key routes through
   `ProjectSemanticDispatch::execute`, carries an explicit R21/R6-clean per-key
   `*Context`, and resolves to its correct `SemanticQueryValue` value domain (type
   keys → `TypeNode`; `ResolveDeclarationAugmentation` → `DeclarationAnalysis`;
   `FlowReturn` / `ResolveCall` / `Relate` → their typed result domains) — no
   non-type value is smuggled into `GraphTypeNode`.

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
final variant set in one cutover, and every downstream consumer (the wire-surface
closure U8, the result DB U10, the exporter U12, the Vue adapter U14) builds on the
final key model.

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
U3 → U10 → U12 → U11/U13 → … ) is **independent of the remaining scheduler work
(U1, U7, U9)** and proceeds on its own dependency-parallel lane after the U2 gate. The only
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

**Expected outcome for every block:** **ZERO new failures** over the baseline
RE-DERIVED at that block's entry gate at the then-current tip (§1) — not a count
frozen to the old `b36e0835` land point. The recorded historical reference is the
8-failure cluster at `b36e0835`; implementation re-derives the live baseline at each
block's entry (this plan does not run the workspace). The `typeinfo_ts_bindings_*`
env-only failure is a non-failure on the main checkout (passes with `node_modules`
present).

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

**Intentional current-state-authority divergence (NOT pre-edited).** `CLAUDE.md`
(the Cache Architecture (CRITICAL) rule) and the `/type-cache-architecture` skill
currently describe the LIVE uniform `FAMILY_SLOT_CANDIDATE_CAP = 4` + FIFO eviction
model, and the `/type-resolution` skill currently treats the five query modes as the
primary cache/mode model (the mode-rank model). These describe the code as it exists
TODAY and are deliberately NOT pre-edited by this plan — making the current-state
authority docs describe unbuilt state would make them lie about the live code. They are
updated to the per-family-adaptive `candidate_cap()` + invalid-first/LRU-by-valid-hit
eviction + global memory ceiling (U3) and to the `ProjectionDemand` / `EvalPolicy`
demand-lattice with the modes as presets (U2.QUERY_VALUE_DOMAIN / U10) by the owning
blocks' `Docs updated` steps WHEN those blocks land. The divergence is therefore an
intentional, tracked deliverable, not an oversight.

| Block | Primary docs to update |
|---|---|
| **U0** | `/type-resolution` (typeinfo contract surface); the reconciled A0a-landed `tests/typeinfo_ignored_test_manifest.rs` manifest (schema notes); `/audit-infrastructure` (`AuditedResult`). |
| **U1** | `/scheduler` SKILL (TaskKind split, `execute_cache_node`); `/host-session` if dispatch surface changes. |
| **U2** | `/type-resolution` + `/type-cache-architecture` (final `SemanticQueryKey` surface, slot identity, B4 node enumeration, R21 key composition); `CLAUDE.md` project-global-cache + macro-traversal summaries; `docs/arch/fact-based-cache.md` per-cache key tables. |
| **U3** | `/type-cache-architecture` (invalidation authority, no reverse-dep eviction; **the per-family adaptive `candidate_cap()` + invalid-first/LRU-by-valid-hit eviction + global memory ceiling, replacing the uniform `FAMILY_SLOT_CANDIDATE_CAP = 4` FIFO**); **`CLAUDE.md`** (the Cache Architecture (CRITICAL) `FAMILY_SLOT_CANDIDATE_CAP` / FIFO-eviction text → the per-family-adaptive cap model); `/component-meta` (cache contracts); `docs/arch/fact-based-cache.md` (multi-candidate `FamilySlots` section — already current). |
| **U4** | `/type-cache-architecture` (persistent pure-artifact rules, sealed `PersistentArtifactNode`); `docs/arch/fact-based-cache.md`. |
| **U5** | `/type-cache-architecture` (memory policy, metrics); `/audit-infrastructure` (`StructuredAuditEvent::CacheNode*`). |
| **U6** | `docs/arch/native-flow-return.md` (moved here in U6); `/type-resolution` (`FlowReturn` query node); `/compiler-codegen` if flow lowering surfaces. |
| **U7** | `/scheduler` SKILL (`CacheNodeDag`, `submit_dag`, `KeyedJob`, `DagHandle`). |
| **U8** | `/type-cache-architecture` (wire-payload notes — `TypeInfoGraphPayload` / `ProgramAnalysisGraph` / `DeclarationAnalysisGraph` placement); `Typeinfo Wire Contract` rule pointers; amend `docs/arch/semantic-type-graph-plan-recovered.md` stale wire wording. |
| **U9** | `/scheduler` + `/host-session` SKILL (session bridge, `CpuConcurrencySemaphore` wiring, `execute_cache_node`). |
| **U10** | `/type-resolution` (`TypeInfoGraphResultDb`, `CompletionFence`; **the Query Mode Contract → the `ProjectionDemand` / `EvalPolicy` demand-lattice with the five modes as presets + the demand-lattice dominance satisfaction relation, replacing the mode-rank/enum-order model**) + `/type-cache-architecture` (result-DB membership on `ProjectTypeStore`). The `U2.QUERY_VALUE_DOMAIN` block lands the demand-lattice DEFINITION; U10 lands the satisfaction relation and updates the skill's mode-contract text. |
| **U11** | `/type-resolution` (public `relate` → `RelationPayload`, `_with_audit`, the session surface); `/audit-infrastructure` (3-branch emission, footprint-attachment pipeline, footprint cell, nested records); `/component-meta` if the public relation surface contract changes. |
| **U12** | `/type-resolution` (the exporter + lowering table; the exporter is a thin projection, not a resolver); `/architecture` (FFI surface); `/type-cache-architecture` exporter/payload notes; legacy-deletion notes in the owning skills. |
| **U13** | `/component-meta` (native-vs-compat, the structural `TypeDescriptor` projection) + `/architecture` (projections); `/type-resolution` (typed-schema-contract notes); `@verter/type-ir` schema docs; `docs/` API pages. |
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
  all 7 U2 variants, the additive U6 `FlowReturn` / `ResolveCall` variants landed in that same shape
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
- [ ] **Ignored-test lift:** ALL 363 `IgnoredTestRow`s are `Lifted` (zero remaining
  parity `#[ignore]`s) per the U0 manifest schedule — NOT a majority/fraction. The
  binding total is exactly 363 (A0a baseline, count-guarded + bijective; not a
  hard-coded stale absolute and not the ~384 raw `#[ignore]` SITES). The only
  permitted residual `#[ignore]`s are the registered Svelte/React STOP-gate files
  (not among the 363). The extended two-table-ledger guards assert it
  (`ignored_test_row_table_holds_exactly_363_rows` +
  `all_typeinfo_parity_rows_lifted_except_stop_gates`); Svelte/React STOP-gate files
  present.
- [ ] **Bench schema:** `BenchResultRow` reports cache mode / source-map policy /
  batch shape / thread count / hit count / fallback count; cm corpus benches
  vendored and hermetic.
- [ ] **Gate green:** `cargo check` / `clippy -D warnings` / `fmt` clean;
  `verter_scheduler` + `verter_session` + full workspace = ZERO new failures over the
  baseline re-derived at this block's entry gate (§1 records the historical 8-failure
  cluster at `b36e0835`; the live baseline is re-derived at the then-current tip);
  `pnpm test` + `pnpm build` (native → lsp → wasm → ts) fully green;
  `pnpm install --frozen-lockfile` in sync.
- [ ] **Architecture clean:** every new CRITICAL rule has a registered guard (R6
  meta-guard green); `no_phase_archaeology_in_production_code` green;
  one-engine / typed-IR-only / shallow-by-default / CodeTransform invariants
  intact.
