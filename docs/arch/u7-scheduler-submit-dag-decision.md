# U7 — Scheduler `submit_dag` Justify-or-Cut Decision (LOCKED)

**Status:** LOCKED. **Verdict: DEFER the multi-node `submit_dag` envelope to U9; KEEP the
landed cache-node substrate + leaf primitives; close the cache-node reachability gap at U9
via single-node lowering into the existing `SchedulerDag::submit`.**

This is the rescope-gate outcome for the plan row
`U7 | scheduler submit_dag (justify-or-cut); ← U1`
(`docs/arch/semantic-db-overhaul-unified-remaining-plan.md`). It supersedes the U7 block
contract in that plan; the U7 §-block and the `U7`/`U9` dep edges are updated to point
here.

Process: source-verified recon → two independent panelists (codex `gpt-5.5` xhigh +
claude-work opus), mandate-led → an independent claude-work adjudicator that did NOT
rubber-stamp the panel and supplied a load-bearing correction. Panel artifacts:
`/tmp/mom/U7/{PANEL.md,codex-panel-out.txt,claude-panel-report.md,adjudicator-report.md}`.

---

## 1. Context

`submit_dag` (atomic cache-node DAG admission) was the highest-risk scheduler block and
was tagged **RESCOPE-GATE-REQUIRED / "justify against a MEASURED workload or CUT it"**
(plan §3.3, §U7, #6 Parsimony pass): a model-checked multi-node DAG for an I/O-bound LSP
already served by singleflight is a textbook over-engineering candidate. This gate
decides JUSTIFY / CUT / DEFER from the **code as it actually stands**, not from the plan's
status column or the `/scheduler` skill (both of which are stale — see §2).

---

## 2. Ground truth (source-verified; the skill AND the MoM brief were both stale)

**LANDED on the tree (U1 + the B7a leaf substrate):**

- `TaskKind` (`stage.rs:60`) is the FULL expanded set `Load`/`Parse`/`Analysis`/
  `Artifact`/`CacheNode`. *(The `/scheduler` skill says these are "not yet implemented" —
  the skill is STALE and must be refreshed at U9 per its own §U9 scope note.)*
- `WorkNodeIdentity::CacheNode { cache_id, key_hash, view_epoch, snapshot_pin_id }`
  (`dag.rs:165`) + `WorkKind::CacheNode` (`dag.rs:190`).
- **`DepKey::CacheNode { cache_id, key_hash, view_epoch, snapshot_pin_id }`**
  (`dag.rs:397`) + `DepKey::from_identity` mapping `WorkNodeIdentity::CacheNode →
  DepKey::CacheNode` (`dag.rs:433`). `complete(identity)` releases waiters keyed on the
  dep key **identity-agnostically** (`dag.rs:1499`), and a raw-DAG test gates work on a
  `DepKey` (`dag_tests.rs:206`). **⇒ a cache-node→cache-node *result* dependency edge is
  already expressible at the raw `SchedulerDag` layer.** It is **NOT** yet live end-to-end:
  the production scheduler *dispatch* path asserts cache nodes are terminal — zero waiter
  release on completion/typed-failure/panic (`scheduler.rs:237`/`249`/`263`, comment
  "CacheNode identities are not used as DepKey prerequisites") and `DepKey::CacheNode`
  forbidden in `failed_blocker_deps` (`scheduler.rs:4793`). So making the edge live requires
  the dispatch-local scheduler work U9 owns (§3.4 #2), not the envelope.
- `StageExecutor::execute_cache_node` (`executor.rs:138`) — default body returns a loud
  typed `StageError` ("not implemented by this StageExecutor"). There is **NO host
  override in `verter_session`** (grep of `crates/verter_session/src/` for
  `execute_cache_node`/`SchedulerCacheId`/`DedupeHook`/`SubmissionResult`/
  `CpuConcurrencySemaphore`/`CancellationToken` → zero hits). ⇒ **cache-node execution is
  plumbed but UNREACHABLE in production.**
- `dispatch_ready_job_to_executor` → `execute_cache_node` routing, panic-guarded with
  reservation release (`scheduler.rs:181`/`215`).
- **`SchedulerDag::submit(identity, kind, priority, deps: Vec<DepKey>, request_context)`**
  (`dag.rs:1117`) — the internal DAG **already implements the genuinely hard scheduling
  core**: 3-case dedup (pre-dispatch dep+priority+winner-context merge / in-flight join /
  cancelled-tombstone reject), the correct subtle rule that an in-flight node's
  prerequisite set is immutable (`let _ = deps;`, `dag.rs:1161`), priority inheritance via
  `min`-merge, dep gating via `deps_remaining` + `waiters` reverse-index, and cache-node
  terminal accounting (`cache_node_terminal_counts`, `dag.rs:780`).
- `submit_request` / `submit_batch_atomic` (`scheduler.rs:1214`/`1281`) — atomic
  `NewRequestBatch` admission under ONE `dag.lock()` via the shared admission core
  (`prepare_request` + `admit_prepared_under_lock` + deferred `DedupJoinerEvent`). *(The
  non-atomic `submit_batch` that fanned N separate submissions was DELETED; the guard
  `crates/verter_scheduler/tests/dag_arch_guards.rs` §6c keeps `pub fn submit_batch(`
  absent — only the atomic batch API survives.)*
- `SchedulerCpuPool` (`pool.rs:101`); the B7a leaf primitives, all landed and
  **UNWIRED** (zero `verter_session` consumers): `CpuConcurrencySemaphore`/`Permit`,
  `CancellationToken`, the opaque `SchedulerCacheId(u64)` newtype, `DedupeHook`/
  `DedupeJoiner`/`NoDedupeHook`, `SubmissionResult<T>` `{Admitted, DedupeJoined,
  Backpressured}`.

**NOT on the tree (grep-verified — no definitions; only two test doc-comments name them
as future work):**

- `submit_dag`, `try_submit_dag`, `submit_dag_blocking`, `CacheNodeDag`,
  `CacheNodeDagNode`, `CacheNodeDagEdge`, `EdgeGate`, `KeyedJob`, `DagHandle`,
  `DagCompletionAggregator`, `CacheNodeOutcome`.
- **The MoM brief's premise that `KeyedJob`/`CacheNodeDagNode`/`submit_dag` are
  "landed-but-unwired" is FALSE.** Only the B7a leaf primitives above are landed-unwired.

**Public `Request` is file-centric** (`scheduler.rs:559`: `Request { file_id, target:
TargetStage, priority, source, file_kind, request_context }`). No field can carry a
`WorkNodeIdentity::CacheNode`. ⇒ **`submit_batch_atomic` CANNOT submit a cache node.** The
MoM brief's CUT premise ("`submit_batch_atomic` + existing DAG-via-NewRequestBatch already
cover every real need") is therefore **false as stated** — they cover *file* batches, not
cache nodes.

**Existing typed cache nodes do not use the scheduler DAG.** The compile-output cache
nodes (`verter_session/src/cache_runtime/compile_output_node.rs`) coordinate via inline
cold-build + content-addressing + `super::singleflight::InflightTable`, NOT
`SchedulerDag`. Plan §2.1 PERMANENTLY routes the biggest candidate
(`TypeInfoGraphResultDb`) to singleflight with **"NO later retarget to `submit_dag`"**,
and the whole semantic-graph lane (U8→U3→U10→U12) binds singleflight, not `submit_dag`.

---

## 3. Verdict: DEFER (envelope un-built), KEEP substrate, close gap via single-node lowering

### 3.1 JUSTIFY-NOW — rejected
No workload on the tree needs **atomic multi-cache-node admission**. The largest candidate
is permanently singleflight-bound (§2.1). The only forward consumer, U9, has not specified
its cache-node dependency topology. A JUSTIFY here would be manufacturing a need to look
productive — a mandate FAIL, and exactly the over-engineering §3.3 / #6 warn against. There
is no measured workload in this gate, so the Parsimony bar cannot be cleared.

### 3.2 CUT-ALL (delete the landed substrate) — rejected
The `WorkNodeIdentity::CacheNode` / `WorkKind::CacheNode` / `DepKey::CacheNode` /
`execute_cache_node` surface / `cache_node_terminal_counts` / router wiring, and the B7a
leaf primitives, are the **owner-layer mechanism U9 must consume** to close the
reachability gap and run cache nodes at all. They live in the one `SchedulerDag`, use
`WorkNodeIdentity` as the single dedupe identity, and are named U9 scope (plan §U9:
session-side `DedupeHook`, opaque cache-id registry, `HostStageExecutor::execute_cache_node`,
`CpuConcurrencySemaphore` propagation). Deleting them re-opens a gap U9 closes regardless
and forces re-derivation — wrong decisiveness. **Keep them** (subject to §6).

### 3.3 DEFER the envelope — confirmed
Hold `CacheNodeDag` / `submit_dag` / `try_submit_dag` / `submit_dag_blocking` / `KeyedJob` /
`CacheNodeDagEdge` / `EdgeGate` / `DagHandle` / `DagCompletionAggregator` / `CacheNodeOutcome`
**un-built**. The hard scheduling core they would wrap is **already landed** in
`SchedulerDag::submit`; the envelope is a thin outer-API shape, not the hard part. Building
it now is speculative generality against an unknown topology — the higher-risk path (a wrong
`EdgeGate`/aggregator shape ossifies or gets rebuilt). Defer the *decision to build it at
all* to U9, under the strict re-gate condition in §5.

### 3.4 The minimal path U9 takes instead (single-node lowering)
U9 closes the `execute_cache_node` reachability gap with:

1. a thin public submission entry that carries a `WorkNodeIdentity::CacheNode` (its
   `SchedulerCacheId` + `key_hash` + `view_epoch` + `snapshot_pin_id`) and routes through
   the **existing** `SchedulerDag::submit` / atomic-admission core — multi-cache-node
   dependency clusters are expressed as N single-node `submit()` calls with
   `deps: vec![DepKey::CacheNode{…}]` edges, which the raw `SchedulerDag` already gates
   correctly at the DAG layer;
2. **lifting the cache-node-terminal assertion in scheduler *dispatch*** so the cache→cache
   edge becomes live end-to-end. Today `dispatch_ready_job_to_executor`'s cache-node arm
   hard-asserts cache-node completion/typed-failure/panic each release zero waiters
   (`scheduler.rs:237`/`249`/`263`) and that `DepKey::CacheNode` never appears in
   `failed_blocker_deps` (`scheduler.rs:4793`) — encoding the current invariant that cache
   nodes are terminal. U9 replaces those terminal assertions with real cache-node
   waiter-release on `complete()` plus cache-node failure/cancel propagation to dependents.
   This is dispatch-local wiring of an edge the DAG layer already supports — it adds no
   second admission authority, no second readiness/accounting structure, and no parallel
   dedupe key;
3. the host `HostStageExecutor::execute_cache_node` override (the real materialiser);
4. the session-side `DedupeHook` impl + opaque cache-id registry + `CpuConcurrencySemaphore`
   propagation (wiring the already-landed B7a primitives).

This honors the §U7 **"lower ALL cache nodes into the EXISTING `SchedulerDag` under ONE
admission path"** invariant and the §2.1 **"not a second cache-admission authority"**
boundary **by construction** — single-node lowering introduces no new admission authority,
no second readiness/accounting structure, and no parallel dedupe key.

---

## 4. The load-bearing correction (why a cache→cache *result* edge does NOT re-gate)

The panel proposed re-gating the envelope the moment U9 surfaces a **cache-node→cache-node
result edge**. The adjudicator refuted this against source, and it is verified here:
`DepKey::CacheNode` (`dag.rs:397`) + `DepKey::from_identity` (`dag.rs:433`) +
identity-agnostic waiter release in `complete()` (`dag.rs:1499`) — exercised by a raw-DAG
test (`dag_tests.rs:206`) — mean that

> submit cache node A; submit cache node B with `deps: vec![DepKey::CacheNode{…A}]`; B
> dispatches only after A completes

**is mechanically expressible at the raw `SchedulerDag` layer with no envelope.** It is
**NOT** yet live end-to-end: the production scheduler *dispatch* path currently asserts
cache nodes are terminal — cache-node completion/typed-failure/panic release zero waiters
(`scheduler.rs:237`/`249`/`263`) and `DepKey::CacheNode` is forbidden in
`failed_blocker_deps` (`scheduler.rs:4793`). Making the edge live therefore requires the
dispatch-local U9 work in §3.4 #2 (replace the terminal assertions with real cache-node
waiter-release + failure propagation) — **NOT** the multi-node envelope. A cache→cache
result edge is therefore **necessary-but-NOT-sufficient** to justify the envelope: once
those dispatch assertions are lifted it is served by single-node lowering over an edge the
DAG layer already gates.

What the multi-node envelope *actually* adds over N single-node `submit()` calls is a
strictly narrower set, only one element of which is a candidate for a genuine *correctness*
requirement:

1. **All-or-none atomic admission of a whole cluster** (no observable half-admitted graph)
   — the only candidate correctness property, and even it merely *extends* the
   already-landed atomic-admission core (`submit_batch_atomic` already admits a multi-item
   batch under one `dag.lock()` for file requests) to cache-node identities; it does not
   invent a new mechanism.
2. **Aggregate completion handle** (`DagCompletionAggregator`) — caller convenience,
   buildable over per-node `SubmissionToken`s.
3. **Graph-scoped cancellation** — cancel a cluster as a unit; convenience over per-node
   `CancellationToken`s, unless a *measured* need shows per-node cancel is insufficient.

---

## 5. Re-gate condition + prerequisites (default = permanent CUT)

`submit_dag` (the multi-node envelope) may be re-gated for JUSTIFY **only** at the U9
rescope gate, and **only** if U9 surfaces ONE of the following, proven against U9's
committed cache-node topology:

- **(A) All-or-none atomic admission as a correctness property** — a cluster where a
  partially-admitted graph is observable or incorrect, that per-node `submit()` +
  `DepKey::CacheNode` gating **cannot** provide; OR
- **(B) A measured-necessary graph-scoped completion/cancellation** that per-node
  `SubmissionToken`s + `CancellationToken`s cannot cheaply provide, demonstrated against a
  representative cold/warm LSP workload (duplicate work avoided, queue dwell, p95/p99
  latency, lock contention).

**A cache-node→cache-node result edge ALONE does NOT re-gate** — it is served by
`DepKey::CacheNode` + single-node lowering once U9 lifts the terminal-cache dispatch
assertions (§3.4 #2), which is ordinary dispatch wiring, not envelope work. **Absent a
proven (A) or (B), the envelope is CUT permanently** and this decision + the plan are
updated to record it.

Prerequisites that must be on the tree / specified before the re-gate can even be
evaluated (these are ordinary U9 work, not envelope work):

1. U9's concrete cache-node consumer set: cache family, `SchedulerCacheId` registry
   mapping, `key_hash` derivation, `view_epoch` / `snapshot_pin_id` semantics.
2. The committed dependency topology: node-count distribution, cache→cache and
   cache→file/artifact edges, cycle policy.
3. `HostStageExecutor::execute_cache_node` override landed (cache nodes actually run).
4. Session-side `DedupeHook` impl + opaque cache-id registry + `CpuConcurrencySemaphore`
   propagation wired (proves the leaf primitives' shape under real use — the envelope API,
   if ever built, must match it).

**Re-gate trigger (precise):** evaluate (A)/(B) at the U9 rescope gate once prerequisites
1–4 are met. No multi-node `CacheNodeDag`/`submit_dag` public API is built before that
evaluation passes.

---

## 6. Invariants preserved (never weakened) + standing honesty condition

- **ONE shared admission core.** Single-node lowering routes through the existing
  `SchedulerDag::submit` / `admit_prepared_under_lock` core. **No second scheduler
  admission path, no second readiness/accounting structure, no `DagAdmissionBudget`, no
  parallel `DedupKey`.** `WorkNodeIdentity` remains THE single dedupe identity. This is
  already mechanically guarded by
  `crates/verter_scheduler/tests/dag_arch_guards.rs::b7b_no_second_admission_budget_or_ready_queue`
  (and the `dag_arch_guards` set); **a future `submit_dag`, if ever re-gated, MUST extend
  the §6b atomic core and keep that guard green — it must not parallel it.**
- **Scheduler-owned `cpu_pool` + host-owned `HostCpuPool` coordinator contract** unchanged;
  cancellation/dedup/cycle semantics unchanged (cycle handling stays the existing DAG's;
  no new cycle engine).
- **R6 / R21 unaffected.** The scheduler is not a cache-key surface; `SchedulerCacheId`
  stays the opaque `u64` newtype (plan §2.1 forbids enum-ifying it). No env-hash or
  query-identity-key change.
- **Standing honesty condition (landed-unwired must not become permanent).** The B7a leaf
  primitives are justified ONLY as U9 substrate. If U9 ships without consuming a given
  primitive, **that primitive is re-gated for DELETION at the U9 outcome** — landed-unwired
  -forever is dead code, not architecture. This applies to each of `CancellationToken`,
  `CpuConcurrencySemaphore`/`Permit`, `SchedulerCacheId`, `DedupeHook`/`DedupeJoiner`/
  `NoDedupeHook`, `SubmissionResult<T>`.

### R6 discipline (this gate)
No new CLAUDE.md `(CRITICAL)` heading is added: this is a deferral decision, not a new
discriminating-now rule. The single load-bearing invariant the decision rests on ("no
second cache-admission authority / no second readiness structure") is **already** covered
by the live guard `b7b_no_second_admission_budget_or_ready_queue`. No premature guard is
landed for the unbuilt envelope. When/if the envelope is re-gated and built at U9, its own
block lands the typed-`Backpressure`-before-mutation (H22) and single-release-reservation
(h23) guards named in the (now-superseded) §U7 contract.

---

## 7. Plan edits made by this gate

In `docs/arch/semantic-db-overhaul-unified-remaining-plan.md`:

- **U7 row** → status `DEFERRED-TO-U9`, pointing to this decision doc.
- **U9 dep edge** re-pointed from `← U7 (cache-node DAG)` to `← U1 (CacheNode substrate +
  executor surface) + B7a (leaf primitives)`. The dangling `U9 ← U7` edge is a correctness
  defect once the envelope is deferred: it silently re-implies the envelope must exist
  before U9 — exactly the speculative build this gate rejects. U9 depends on the landed
  substrate, NOT on a built envelope.

The §U7 block body retains its design sketch but is annotated as superseded by this
decision (the envelope is built only on a §5 re-gate pass).
