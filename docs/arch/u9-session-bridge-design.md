# U9 — Scheduler Cache-Node Lowering (LOCKED)

**Status:** LOCKED. **Verdict: U9 is NOT a "session bridge." Honestly scoped against the tree it is
(a) finishing and hardening the half-built cache-node DAG edge so a cache→cache dependency is LIVE and
SOUND (success release + failure propagation + cycle rejection), and (b) DELETING four dead B7a leaf
primitives per the U7 §6 standing-honesty condition. No registry, no `DedupeHook`, no `CpuConcurrencySemaphore`,
no host back-edge, and no production cache materializer are built — there is no consumer that needs them, and
the central one (`DedupeHook`) is unbuildable under R6.** The core `WorkNodeIdentity::CacheNode` arm survives
U9 only as a now-correct mechanism with a test-only proof, under a hard deletion re-gate (§7).

This is the design-gate outcome for plan row `U9 | session bridge (B7f); ← U1 + B7a` and supersedes the U9
block body in `docs/arch/semantic-db-overhaul-unified-remaining-plan.md` (§U9). It honors — and does not
re-open — `docs/arch/u7-scheduler-submit-dag-decision.md` (LOCKED): the multi-node `submit_dag` envelope stays
deferred/CUT-by-default; U9 closes the reachability gap via single-node lowering into the existing
`SchedulerDag::submit`.

Process: source-verified recon → two independent mandate-led panelists (codex + claude-work) attacked a
strawman → an independent claude-work adjudicator that did NOT rubber-stamp, opened every cited `file:line`,
ran the consumer/cycle/failure greps itself, confirmed every panel finding (two with deeper evidence), added
two new defects, and made the fork call. Artifacts:
`/tmp/mom/U9/{STRAWMAN.md,PANEL.md,codex-panel-out.txt,adjudicator-report.md}`.

---

## 1. Context

U1 landed the cache-node substrate (`TaskKind::CacheNode`, `WorkNodeIdentity::CacheNode`,
`DepKey::CacheNode`, the `execute_cache_node` executor surface, the panic-guarded dispatch arm,
`cache_node_terminal_counts`). B7a landed the leaf primitives (`CpuConcurrencySemaphore`/`Permit`,
`CancellationToken`, opaque `SchedulerCacheId`, `DedupeHook`/`DedupeJoiner`/`NoDedupeHook`,
`SubmissionResult`). U7 DEFERRED the multi-node envelope and tasked U9 with closing the cache-node
reachability gap via single-node lowering (`u7-...-decision.md` §3.4) while discharging the §6 standing-honesty
condition: *a B7a primitive U9 does not consume is re-gated for DELETION at the U9 outcome.*

The gate's job was to decide the airtight mechanism — and, first, whether a real consumer exists at all.

---

## 2. Ground truth (source-verified @ `8d5566a3`)

- `StageExecutor::execute_cache_node(&self, cache_id: SchedulerCacheId, key_hash: Hash16, view_epoch: u64,
  snapshot_pin_id: PinId, cancellation: &CancellationToken) -> Result<(), StageError>` — default = loud typed
  error (`executor.rs:138`). The ONLY overrides on the tree are THREE **test-only** impls inside the scheduler
  crate (`scheduler.rs:5798/5969/5995`). `HostStageExecutor` (`host_executor.rs:129`) does NOT override it and
  holds NO host back-edge (only `config` + `workspace`); it is constructed BEFORE `VerterHost` exists
  (`host_construction.rs:79`).
- The dispatch arm (`scheduler.rs:~196-265`) runs `execute_cache_node` panic-guarded; `Ok(Ok(()))`→
  `complete(identity)`, `Ok(Err)`/panic→`cancel(identity)`. THREE `debug_assert!(newly_ready/stranded.is_empty())`
  (`scheduler.rs:237/249/263`) encode "cache nodes are terminal."
- `SchedulerDag::complete(identity)` (`dag.rs:1499`) and `cancel(identity)` (`dag.rs:1525`) ALREADY release/
  strand waiters keyed on `DepKey::from_identity` **identity-agnostically**, ALREADY call
  `refresh_ready_membership` to put a freed waiter into its dispatch lane (`dag.rs:1514/1560`), and ALREADY
  release the parked reservation exactly-once by value (`reservation.release()` consumes `self`,
  `dag.rs:1494/1578`). ⇒ the cache→cache **success** edge is one assert-lift + one `Wake` away from live.
- **Failure path is NOT built for cache nodes.** `cancel()` strands waiters but attaches NO `FailedDepRecord`
  (`dag.rs:1544`). The file-stage failure chokepoints are structurally closed to cache nodes:
  `terminalize_failure` is `unreachable!()` for `TaskKind::CacheNode` (`scheduler.rs:4678`), and
  `execute_stage_on_worker` (which reads `failed_blocker_deps`) is `unreachable!()` for `CacheNode`
  (`scheduler.rs:4868`). The `failed_blocker_deps` forbid at `scheduler.rs:4793` is the file-stage path's
  guard. ⇒ "lift the 4793 forbid → consumer gets a `DependencyFailed` short-circuit" is FALSE: cache nodes
  never reach that chokepoint.
- **Cache-edge cycle detection is absent.** `SchedulerDag::submit` (`dag.rs:1117`) does zero cycle checking.
  The only cycle logic, `dep_reaches_owner` (`dag.rs:1276`), is a pre-admission filter for macro-type deps that
  walks ONLY `DepKey::FileStage{Analysis}` edges — "CacheNode edges are not traversed" (`dag.rs:1268`). A
  `B→A, A→B` cache mis-submission silently deadlocks.
- **Lossy identity.** `WorkNodeIdentity::CacheNode.key_hash: Hash16` (`dag.rs:132/167`) is a 16-byte projection
  of the cache key `K`. The scheduler dedups on `WorkNodeIdentity` (`dag.rs:1130`); singleflight dedups on the
  full typed `K` (`InflightTable<K>`, `cache_runtime/singleflight.rs:211`; `QueryFlightKey<K>` incl. the
  store-view compat token, `node.rs:99`). `key_hash` is NOT an injection of `K`.
- **Cache coordination authority is singleflight.** `cooperative_admit_with_post_publish` + `InflightTable<K>`
  + `ReadSetSignature` fence + `MAX_INFLIGHT_RETRIES = 3` (in `semantic_query_memo/inflight.rs:226`, NOT in
  `cache_runtime/singleflight.rs`). `compile_output_node.rs` runs cold-build inline under singleflight.
- **No consumer, tree-wide and committed.** `WorkNodeIdentity::CacheNode` / `DepKey::CacheNode` /
  `WorkKind::CacheNode` are never constructed outside `crates/verter_scheduler`. `submit_cache_node` does not
  exist. Zero `verter_session/src` consumers of `execute_cache_node`/`SchedulerCacheId`/`DedupeHook`/
  `CpuConcurrencySemaphore`/`CancellationToken`/`SubmissionResult`. The only result DB, `TypeInfoGraphResultDb`
  (U10), is PERMANENTLY singleflight-bound (plan §2.1, "NO retarget to `submit_dag`"; U9/U7 lane "unrelated").
  Batch CPU work runs on `HostCpuPool`, "never on the scheduler stage `cpu_pool`". There is no third workload
  category a scheduler cache-node would serve.
- Bare `&VerterHost as &dyn ResolverContext` PANICS/guards in `store_view()` (`resolver_context.rs`): production
  resolver-tier work MUST construct `HostResolverContext::new(host, &view, overlay)` at a request boundary.

---

## 3. The fork — decision

**α — full bridge (registry + `DedupeHook` + `CpuConcurrencySemaphore` + host back-edge + production
materializer): REJECTED.**
- No named consumer exists, tree-wide or in the committed plan; the only result DB is permanently
  singleflight-bound (§2). Building the bridge against a non-existent consumer is the "manufacture a need to
  look productive" mandate FAIL that U7 §3.1 already rejected for JUSTIFY-NOW.
- Independently fatal: `DedupeHook::probe(identity: &WorkNodeIdentity)` is handed only the **lossy** identity
  yet must consult a `K`-keyed `InflightTable<K>`. Recovering `K` needs either a `key_hash→K` side-index (a
  second, collision-prone identity map) or enum-ifying `SchedulerCacheId`/`key_hash` (forbidden by R6/§2.1). And
  `probe` runs BEFORE scheduler admission, so a probing caller never reaches the scheduler's `WorkNodeIdentity`
  dep-merge — making the hook a PARALLEL dedup authority. The bridge is **unbuildable soundly**.
- A production materializer on a `cpu_pool` worker thread has no live request and hits the `store_view()` panic;
  there is no clean request-bound-context-on-worker story without a much larger retarget.

**γ — delete the entire cache-node substrate (reverse U7 §3.2 KEEP): clears its bar for the LEAF set; held in
reserve for the CORE.** U7 §3.2 rejected CUT-ALL on the premise "U9 must consume it." That premise has expired
(U9 names no consumer; the only result DB is permanently singleflight). Under Parsimony, landed-unwired-forever
is dead code. γ is therefore applied IN FULL to the leaf primitives (§5 table). It is NOT applied to the CORE
arm now because U7 §3.4 explicitly tasked U9 with *closing* (not deleting) the gap, and closing it is cheap
correctness hardening of an already-present mechanism — but the core is placed under a hard deletion re-gate
(§7).

**β — minimal reachable + delete unused primitives: SELECTED, sharpened.** Make the existing cache-node edge
live and SOUND; delete the four dead leaf primitives. The CORE arm is kept ONLY because the DAG layer already
expresses `DepKey::CacheNode` edges and `complete()`/`cancel()` already release them — the current state is a
*latent defect* (the dispatch asserts forbid the release the DAG performs; `cancel()` propagates no failure;
`submit` has no cycle guard), and closing it routes through the existing admission core so
`b7b_no_second_admission_budget_or_ready_queue` stays green by construction. This honors U7 §3.4 (single-node
lowering, envelope deferred) and discharges U7 §6 (delete unconsumed primitives).

---

## 4. Locked decisions

### Decision 1 — Bridge / consumer
**No session bridge is built.** No `submit_cache_node` host entry, no cache-node registry
(`SchedulerCacheId → Arc<dyn CacheNodeFamily>`), no `HostStageExecutor::execute_cache_node` override, no host
back-edge (`OnceLock<Weak<VerterHost>>` / `Arc::new_cyclic` / shared-registry-Arc), no request-bound materializer
context. The only `execute_cache_node` impl that runs is a **test-only** `StageExecutor` exercising the cache
arm (joining the three already on the tree). The plan's "crosses the H20 boundary at the session edge" scope is
DROPPED — there is no session edge to cross; the `no_session_dep` (H20) guard stays green with no sanctioned
edge added.

### Decision 2 — Single-node lowering (U7 §3.4, exact) + the net-new soundness work
A cache→cache cluster is expressed as N single `SchedulerDag::submit` calls, each carrying
`WorkNodeIdentity::CacheNode{…}` and `deps: vec![DepKey::CacheNode{…}]`. NO `submit_dag` envelope, NO second
admission core, NO second readiness/accounting structure, NO parallel dedup key — routes through the existing
`SchedulerDag::submit` admission core (the file-stage request wrappers are file-stage-specific and are NOT
reused); `WorkNodeIdentity` stays THE single dedupe identity.

The success path is the easy 10%; the failure path + cycle guard are the load-bearing 90%. "Lift the
assertions" is INSUFFICIENT. U9 must:
1. **Lift** the three cache-arm asserts (`scheduler.rs:237/249/263`); when `complete()`/`cancel()` returns a
   non-empty `newly_ready`/`stranded`, send ONE `Submission::Wake` to re-pump (the existing mechanism,
   `scheduler.rs:1447/4698`). Do NOT "requeue `newly_ready`" — the freed waiter is ALREADY in its lane via
   `refresh_ready_membership` (`dag.rs:1514/1560`); the return Vec is informational and the file-stage path
   discards it.
2. **Cache failed-dep fanout (net-new):** on a cache-node failure/cancel, record a
   `FailedDepRecord{ dep_key: DepKey::CacheNode{…}, cause }` for each released cache-waiter, plus a persistent
   `terminal_dep_failures` entry under the `CacheNode` key for the admission-time race (mirroring
   `insert_terminal_dep_failure`, `scheduler.rs:4661`). This is cache-arm-local — it MUST NOT reuse
   `terminalize_failure`/`execute_stage_on_worker` (both `unreachable!()` on `CacheNode`).
3. **Cache dependent short-circuit (net-new):** in the **cache dispatch arm**, before calling
   `execute_cache_node`, if the node carries a `CacheNode` `failed_blocker_deps` entry, terminalize it as
   `DependencyFailed` WITHOUT executing.
4. **Cache cycle guard (net-new):** a bounded cache-edge reachability check at the cache submit chokepoint
   (or inside `submit` when the identity/deps are `CacheNode`) returning a typed error on a would-be
   `A↔…↔A` cache cycle. `dep_reaches_owner` walks only Analysis edges and cannot be reused as-is.

The `4793` forbid is relaxed to admit `CacheNode` keys ONLY into the cache-arm path — a `CacheNode` `DepKey`
must NEVER flow into the file-stage chokepoint (file stages never depend on cache nodes).

### Decision 3 — Dedup / cancellation / cycle / completion-fence
- **Dedup:** core-work dedup is the scheduler's existing `WorkNodeIdentity` merge in `submit`. Cache **result**
  dedup/fence stays 100% in singleflight (`InflightTable<K>` + `cooperative_admit_with_post_publish` +
  `ReadSetSignature`, `MAX_INFLIGHT_RETRIES=3` in `semantic_query_memo/inflight.rs:226`). **`DedupeHook` is
  deleted, not bridged** — the `WorkNodeIdentity(Hash16) → InflightTable<K>` bridge is non-recoverable
  (lossy) and pre-admission (a parallel authority), i.e. unsound under R6/§2.1.
- **Cancellation:** no production cancellation path is wired (no production submitter). The retained test proof
  may pass a trivial local cancel flag; the rich `CancellationToken` leaf primitive is deleted (§5).
- **Cycle:** the net-new bounded cache-edge check in Decision 2 #4; no general new cycle engine for file stages.
- **Completion fence:** unchanged and untouched by U9 — it lives entirely in singleflight. Cancelled /
  superseded / budget-exceeded results route through the existing `ReturnOnly` admission and are NOT
  warm-promoted. The scheduler adds NOTHING to the fence.

### Decision 4 — Consistency with landed CRITICAL invariants
- **ONE shared admission core:** single-node lowering routes through the existing `SchedulerDag::submit`
  admission core; `WorkNodeIdentity` stays the single dedupe identity. No `DagAdmissionBudget`, no parallel
  ready queue, no parallel dedup key. `b7b_no_second_admission_budget_or_ready_queue` stays green.
- **ONE pool:** cache nodes dispatch on the existing scheduler `cpu_pool`; the host-owned `HostCpuPool`
  coordinator contract is unchanged; NO second pool. (`CpuConcurrencySemaphore` — a redundant second
  concurrency cap with no consumer — is deleted, §5.)
- **Canonical dependency cache rule, R6/R21, ReadSetSignature authority, immutable `Arc` payloads:** untouched
  — U9 adds no cache-key surface. `SchedulerCacheId` stays the opaque `u64` identity field (no enum-ification).
- **Resolver-context seal:** untouched — no resolver-tier cache materialization is wired, so the
  `store_view()` request-bound-context contract is never engaged.

---

## 5. Per-primitive KEEP / DELETE (discharges U7 §6)

| Primitive | Verdict | Rationale (source) |
|---|---|---|
| `DedupeHook` / `DedupeJoiner` / `NoDedupeHook` (`dedupe_hook.rs`) | **DELETE** | No consumer; the `WorkNodeIdentity(key_hash) → InflightTable<K>` bridge is unsound under R6 (lossy `Hash16`, non-recoverable) and probes pre-admission (a parallel dedup authority). The scheduler already dedups core work on `WorkNodeIdentity`; singleflight already dedups results on `K`. No bridge is needed or buildable. |
| `SubmissionResult<T>{Admitted,DedupeJoined,Backpressured}` | **DELETE** | No consumer; `submit` returns `SubmissionToken`. `DedupeJoined` is meaningless once `DedupeHook` is gone; `Backpressured` has no producer. |
| `CpuConcurrencySemaphore` / `CpuConcurrencyPermit` (`cpu_concurrency.rs`) | **DELETE** | No consumer (only a guard *comment* references it). Redundant second concurrency limiter — file-stage CPU work is already capped by the cpu_pool capacity ledger (`scheduler.rs:4251`), batch work by `HostCpuPool`. A second cap on a non-existent cache-CPU workload is dead. |
| `CancellationToken` (rich form, `cancellation.rs`) | **DELETE** | Threaded ONLY into `execute_cache_node`; file-stage arms ignore the param. No production cache submitter ⇒ no production cancellation. The retained test proof uses a trivial local if it needs one. |
| `SchedulerCacheId(u64)` (`cache_id.rs`) | **KEEP as identity field ONLY** | It is the `cache_id` field of `WorkNodeIdentity::CacheNode`/`DepKey::CacheNode` — part of the core DAG identity; survives iff the core arm survives; stays opaque `u64` (R6). The "opaque cache-id **registry**" is NOT built. |
| CORE: `WorkNodeIdentity::CacheNode` / `WorkKind::CacheNode` / `DepKey::CacheNode` / `from_identity` / `execute_cache_node` / `cache_node_terminal_counts` / dispatch arm | **KEEP + complete + harden** (under the §7 re-gate) | Already present; closing the half-built edge (success + failure + cycle) is consumer-independent correctness. |

The plan's U9 "Required deletions: none" is WRONG and is corrected to this list (§8).

---

## 6. Worked example — cache→cache single-node lowering (success / failure / cycle)

Two cache nodes, B depends on A. Submitted as two single-node `submit` calls (no envelope):

1. `submit(WorkNodeIdentity::CacheNode{A}, WorkKind::CacheNode, prio, deps=[])` → admitted, dispatch-ready.
2. `submit(WorkNodeIdentity::CacheNode{B}, WorkKind::CacheNode, prio, deps=[DepKey::CacheNode{A}])` →
   admitted; `deps_remaining={A}`; parked in `waiters[DepKey::CacheNode{A}]`. **Before admission, the cycle
   guard (Dec 2 #4) checks A does not transitively depend on B; a `B↔A` submission returns a typed cycle error
   — no silent deadlock.**

**Success:** A dispatched on `cpu_pool` → `execute_cache_node(A)` → (test executor) returns `Ok(())`. Dispatch
arm: `newly_ready = complete(&A.identity)`; `complete` already removed `DepKey::CacheNode{A}` from B's
`deps_remaining`, emptied it, and ran `refresh_ready_membership(B)` putting B in its lane. The arm sends one
`Submission::Wake` (the **lifted assert** — previously `debug_assert!(newly_ready.is_empty())` fired). The pump
drains B; B dispatched → reads A's now-warm fence-validated result through A's family singleflight entry.

**Failure:** A dispatched → returns `Ok(Err(_))` (or panics; both caught). Dispatch arm: `cancel(&A.identity)`
records `FailedDepRecord{DepKey::CacheNode{A}, cause}` for the released waiter B (Dec 2 #2), returns
`stranded=[B]`, sends one `Wake`. B becomes dispatch-ready; the cache-arm short-circuit (Dec 2 #3) sees B's
`CacheNode` `failed_blocker_deps` entry and terminalizes B as `DependencyFailed` WITHOUT calling
`execute_cache_node(B)` — never dispatching over the dead prerequisite.

---

## 7. Hard re-gate on the CORE cache-node arm (standing honesty)

The `WorkNodeIdentity::CacheNode` arm survives U9 ONLY as a now-correct mechanism with a test-only proof. It has
no production consumer and none is committed (U10's result DB is permanently singleflight-bound). **If the next
semantic-graph / cache-runtime block lands without a committed production cache-node submitter, the entire
`CacheNode` arm (`TaskKind::CacheNode` / `WorkNodeIdentity::CacheNode` / `DepKey::CacheNode` /
`WorkKind::CacheNode` / `execute_cache_node` / `cache_node_terminal_counts` / the dispatch arm) is re-gated
for deletion (full γ).**
Keeping it past that point would be a reachable second cache-coordination authority — a standing violation of
"one shared admission core" — tolerated now only because closing the half-built edge is cheap correctness, not
because anything needs it. This strengthens U7 §5 ("a cache→cache edge alone does NOT re-gate the envelope"):
the edge is served by single-node lowering AND has no consumer.

---

## 8. R6 discipline + guards + plan edits

**No new CLAUDE.md `(CRITICAL)` heading.** U9 is deferral/correctness wiring, not a new discriminating-now
rule. The load-bearing invariant ("no second cache-admission authority") is ALREADY guarded by
`b7b_no_second_admission_budget_or_ready_queue` (`crates/verter_scheduler/tests/dag_arch_guards.rs:389`); the
new cache-edge correctness is guarded by the implementation block's characterization test, NOT by a premature
static guard for unbuilt surface.

**Discriminating test the implementation MUST land (B1):** a test-only `StageExecutor` submits A, then B with
`deps=[DepKey::CacheNode{A}]`, asserting all three axes — each FAILS pre-change, PASSES post-change:
- (i) **release** — A succeeds → B is released and dispatched (pre-change: the lifted assert fires in debug /
  B never wakes);
- (ii) **failure-propagate** — A fails → B terminalizes `DependencyFailed`, never dispatching over the dead dep
  (pre-change: B dispatches over the failed prerequisite);
- (iii) **cycle-reject** — `B↔A` submission → typed cycle error, no deadlock (pre-change: hangs).

This test is NOT a synthetic-consumer demo (which would be a Stub-Prevention violation); it characterizes a real
latent defect in an already-test-exercised mechanism.

**Plan edits** (`docs/arch/semantic-db-overhaul-unified-remaining-plan.md`, §U9 row + block):
- U9 status → `DESIGN-LOCKED` pointing to this doc.
- "Required deletions: none" → the §5 DELETE list (`DedupeHook`/`DedupeJoiner`/`NoDedupeHook`,
  `SubmissionResult`, `CpuConcurrencySemaphore`/`Permit`, rich `CancellationToken`).
- The "session-side `DedupeHook` impl, opaque cache-id registry, `CpuConcurrencySemaphore` propagation, crosses
  H20 at the session edge" scope is superseded: no bridge, no registry, no semaphore, no session edge.

---

## 9. Implementation mini-DAG (4 sub-blocks, scheduler-lane)

- **B1 — cache-edge correctness in the DAG** (scheduler-local): lift the three cache-arm asserts → `Wake`; add
  `cancel()` `FailedDepRecord` for `DepKey::CacheNode` + persistent `terminal_dep_failures`; add the cache-arm
  pre-execute `DependencyFailed` short-circuit; relax the 4793 forbid to admit `CacheNode` keys ONLY into the
  cache path. Lands with the discriminating three-axis characterization test (§8).
- **B2 — cache-edge cycle guard** at the cache submit chokepoint (typed error). May fold into B1; kept distinct
  because it is new logic, not an assert lift.
- **B3 — DELETE the leaf-bridge set** (§5 table): `dedupe_hook.rs`, `cpu_concurrency.rs`, `SubmissionResult`,
  rich `CancellationToken` — together with their sibling test files (`dedupe_hook` tests,
  `cpu_concurrency_semaphore.rs`, `cancellation_token.rs`), which fail to compile once the types are gone;
  keep `SchedulerCacheId` as the opaque identity field. Update
  `dag_arch_guards` / `architecture_guards` comments that name deleted symbols; keep `no_parking_lot_semaphore`
  and `no_session_dep` green.
- **B4 — skill refresh:** `/scheduler` (stale `submit_batch` / "not yet implemented `TaskKind`" prose; record
  cache nodes are reachable-but-consumer-less and on a deletion re-gate; B7a leaf primitives deleted),
  `/host-session` (no session bridge was added).

Dependency order: B1 → B2 (or B1+B2 together) → B3 → B4. B3 must follow B1/B2 so the deletions don't strand the
correctness work; B4 documents the landed state.

---

## 10. Invariants preserved (never weakened)

- ONE shared admission core (`b7b_no_second_admission_budget_or_ready_queue` green); `WorkNodeIdentity` the
  single dedupe identity; result dedup/fence stays in singleflight.
- ONE pool (scheduler `cpu_pool` + host `HostCpuPool` coordinator); no second pool; the redundant
  `CpuConcurrencySemaphore` cap deleted.
- Canonical dependency cache rule, R6/R21, ReadSetSignature validity authority, immutable `Arc` payloads,
  resolver-context seal — all untouched (U9 adds no cache-key surface and wires no resolver-tier materialization).
- U7 not re-opened: the multi-node `submit_dag` envelope stays deferred/CUT-by-default; §5(A)/(B) never
  triggered.
