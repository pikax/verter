# Scheduler Lifecycle Unification (UNIFY)

**Status:** ratified design, not yet implemented.
**Ruling:** `GB4-S0-DEFER-2026-07-26` (architecture seat, confidence 0.94 UNIFY / 0.97 land-with-defer).
**Owner block:** the UNIFY cutover block.
**Debt row:** [`SCHED-UNIFY-LIFECYCLE-ATOMICITY`](#debt-row-sched-unify-lifecycle-atomicity).

---

## 1. Context

Four consecutive review rounds against the scheduler each closed a real defect and each revealed
another of the same family. Every one of them reduces to a single sentence:

> Work is admitted for a generation the file has already left, or a waiter is left holding an
> identity nothing will ever complete.

The defects were real and the fixes were correct — both review seats confirmed each landed fix
sound — but the *method* did not converge, because the underlying condition was never removed.

### 1.1 The two mechanisms, and which one closed

Splitting the findings by mechanism rather than by round is what makes the shape visible:

| Mechanism | Where it appeared | Status |
|---|---|---|
| **M1 — admission after retirement** | round 0 (Source→Analysis publish), round 1 (`admit_pending_artifacts`), round 2 (`remove()` sweep gap) | **CLOSED, structurally.** A per-canonical retirement floor consulted by `SchedulerDag::submit`, the sole `by_identity` insert. Both seats independently enumerated every writer and confirmed exactly one insertion primitive and no alternate ready-node constructor. It has not recurred. |
| **M2 — captured authority state crosses the lock boundary and is then trusted** | round 1 (completion witness re-derived by lookup), round 3 (prepared request resumes post-delete), round 4 (prepared request resumes **mid**-delete) | **NOT CLOSED.** Each fix closed the specific instant demonstrated; the next review found an adjacent instant in the same seam. |

Rounds 3 and 4 are the same function at successively finer time granularity: post-delete, then
mid-delete. That is window-level whack-a-mole, and the next finer instant is available to whoever
looks next.

### 1.2 The generating condition

`Scheduler.nodes` (node liveness) and `SchedulerDag` (admission state) are **two authorities with
independent transition points.** Every independent transition point is a window. There is no finite
list of windows to close — there is one condition to remove.

The round-4 finding is the sharpest evidence: the liveness predicate is *transiently true inside
`remove()`'s own window*. `remove()` installs floor `G+1` and completes its cancellation sweep, then
— before `nodes.remove()` unpublishes the `FileNode` — an admission takes the DAG lock, observes the
node still published at the same incarnation, passes the hand-written crossing gate, bumps to `G+1`,
and `submit` accepts because `G+1` is exactly the floor and the comparison is `<`. `remove()` then
unpublishes and signals waiters but never cancels that post-sweep identity. A later dequeue reserves
capacity, finds no `FileNode`, and skips without cancelling ⇒ a leaked admission permit **in release**. In DEBUG
builds the dispatch skip's `debug_assert` fires FIRST, so the observable
symptom there is a PANIC, not a leak — a reader who meets that panic
should not have to rediscover that it is this residual.

Note the two facts that make a narrower fix insufficient:

- **Generation cannot separate a detached node from a legitimate re-add.** Bumping a detached node
  lands its generation on exactly `last_gen + 1`, which is precisely where a legitimate re-add
  arrives. `<=` would break re-add; `<` admits the detached node. **Only liveness separates them** —
  and liveness is exactly what is transiently wrong inside the window.
- **Atomic `remove()` alone does not close it.** The seat's decisive evidence, which the prior
  analysis did not have: *"`prepare_request` still performs its tombstone check and publishes
  through `nodes.entry` OUTSIDE the DAG lock, and auto-ingest has comparable node-ensure paths;
  making only `remove()`'s current deletion atomic would leave those split-phase publications
  needing the next witness."* There are at least three split-phase publishers, not one.

### 1.3 Why not an owned type-state token

An owned admission token was considered and **rejected**: *"An owned type-state token cannot prove
revocable liveness after lock release; its correctness would still require epoch revalidation,
whereas a lock-scoped capability makes removal unable to interleave."* Liveness here is *revocable*,
so a capability that outlives the lock can only ever be re-validated, never trusted — which
reproduces the same class one level up.

The related instinct that the captured `Arc` should not exist at all was upheld; it is realised
through a guard-borrowed capability rather than an owned token.

---

## 2. Intent Contract

**Actor / problem.** The scheduler must never dispatch, reserve capacity for, or park a waiter on
work belonging to a file version that no longer exists. Today it can, because two authorities decide
that jointly without a shared transaction.

**Required observable outcomes.**

- A file admission either happens against a live, current incarnation or does not happen at all.
- Removal, re-add, re-home, and invalidation are each a single atomic transition; no observer can
  see a half-applied lifecycle change.
- Every waiter reaches a terminal state. A refused or retired admission never leaves a parked group.
- Every reserved CPU / IO / aggregate permit is released, whether the work ran, was cancelled, or
  was refused.
- An immediate legitimate re-add after a removal remains admissible.

**Forbidden observable outcomes.**

- An admitted identity whose canonical has no published `FileNode` (cache nodes exempt).
- A dispatch skip that returns without cancelling and without releasing its reservation.
- Any file identity reaching the DAG insertion primitive without a lock-scoped live capability.
- Any liveness decision made from a value read outside the lock that governs the transition.

**Authority / fallback order.** The lifecycle table is the single authority for liveness and
admissibility. `FileNode` remains readable lock-free but is **payload only, never liveness
authority** — an immutable snapshot, not a decision input. There is no fallback path: an admission
that cannot obtain the capability does not occur.

**Material bounds.** Lock-free `FileNode` payload reads are preserved, so hot read paths do not
regress. The lifecycle table holds one row per canonical — the same order as `nodes` itself and as
the existing `generation_floors` — so it adds no new growth class. Admission work stays inside the
existing single critical section; no new lock is introduced and no lock is held across executor
calls or inbox sends.

---

## 3. Changes

### 3.1 The lifecycle table

A per-canonical lifecycle row owned **beside `SchedulerDag`, under the existing mutex**:

```
Live    { node, incarnation, generation }
Retired { floor, removal_epoch }
```

Every lifecycle event transitions that row atomically within one critical section: `remove`,
re-add, re-home, `invalidate`, admission, and completion publication.

### 3.2 Lock-scoped live capability

File admission requires a **private, guard-borrowed live capability** and must occur **before that
guard is released**. Because the capability cannot outlive the lock, a removal cannot interleave
between the liveness proof and the admission — the property an owned token could not provide.

`SchedulerDag::submit` no longer admits file identities; it retains only the non-file
(cache-node) path. File admission is reachable only through the capability-bearing entry point.

### 3.3 Payload / authority split

`FileNode` reads stay lock-free and are explicitly demoted to payload. No liveness, admissibility,
or generation decision may be taken from a lock-free read.

### 3.4 Publication sites folded into the transaction

The split-phase publishers named by the seat all move inside the transaction:

- `prepare_request`'s tombstone check and its `nodes.entry` publication.
- The auto-ingest node-ensure paths.
- `remove()`'s unpublish step.

### 3.5 Carried forward — the reset / clear-all shape

`scheduler.rs:2040-2050` (the reset / clear-all path) has the **identical split-phase shape**:
`nodes.remove` in a loop outside the DAG lock, with `dag.lock()` taken afterwards. It may be
self-healing today, because `dag.lock().clear()` immediately afterwards would absorb anything
admitted in the gap — this is recorded as a *shape*, not an asserted live defect. **Neither review
seat saw this path.** UNIFY subsumes it, and it must not be lost in the cutover: the reset path
transitions the same lifecycle rows and must do so through the same transaction.

---

## 4. Legacy Deletions

Carried verbatim from the ruling's SCOPE. Each must be **deleted**, not wrapped, flagged, or
preserved behind a branch:

- `PreparedRequest.node`
- `prepared_incarnation`
- `_captured_node`
- the hand-written crossing gate
- standalone mutable `nodes` publication
- standalone tombstone / generation-floor authority
- the duplicate DAG retirement-floor representation
- direct file use of `SchedulerDag::submit`
- debug-assert-only dispatch skips

The last one matters beyond hygiene: a `debug_assert`-only skip means release builds take the same
skip **silently**, which is how the original leak class stayed invisible. Under UNIFY the skip
either cannot occur or is a handled outcome, never an assertion.

---

## 5. Verification

### 5.1 Test contract (from the ruling)

**A. Structural guard.** No file identity may reach the DAG insertion primitive without a
lock-scoped live capability. This is a *structural* guard — a type/visibility property, not a
name-keyed source scanner (`CLAUDE.md` → landed-scanner bar).

**B. Deterministic ordering matrix.** Over **every admission origin** — request, Source completion,
pending Artifact, auto-ingest — crossed with the lifecycle transitions, asserting:

1. **Handle terminality** — every waiter reaches a terminal state; none parks.
2. **No surviving retired identity, waiter, or blocker-index entry.**
3. **Permit counts return to zero** — CPU, IO, and aggregate — **after an actual dequeue**, not
   merely after admission. A permit is reserved at dequeue, so a test that never dequeues cannot
   observe the leak this class produces.

### 5.2 Class-level invariant (prescription-independent)

> **For every identity in `by_identity`, the canonical must have a published `FileNode`
> (cache nodes exempt).**

Asserted after every lifecycle operation. An orphaned admission is exactly "admitted identity with
no live node", so this catches **every member of the class regardless of which instant produced it**,
including instants nobody enumerated. It holds under UNIFY and would have held under either narrower
prescription, which makes it safe to ratify independently of the cutover.

This is the guard that answers the question a per-window test cannot: *if the defect were present,
would this check see it?* A window test can only see the window it was written for — which is
precisely how rounds 3 and 4 each passed their own regression test while the adjacent instant stayed
open.

### 5.3 Acceptance

**`SCHED-UNIFY-A1`** — deterministic test
**`remove_and_file_admission_share_one_lifecycle_transaction`**: the forced
post-sweep / pre-unpublish race cannot admit work or retain a permit, **while an immediate
legitimate re-add remains admissible.** Both halves are required — a fix that closes the race by
refusing re-adds is not acceptance, it is a regression.

### 5.4 Gate

Full workspace suite per `CLAUDE.md` → End-of-change Checks: `node scripts/gate.mjs`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`, plus `pnpm test`
for any TypeScript surface touched.

---

## 6. Convergence analysis (recorded, from the round-4 implementer analysis)

Preserved because the ruling reached the same conclusion **independently**, which is materially
stronger evidence than agreement with a handed answer. The seat's phrasing: *"the series is
converging diagnostically toward a lifecycle transaction, but the present mechanism is not
converging to a closed invariant."*

- **Converging:** severity and count. Blast radius fell hang → hang-plus-leak → leak-only with the
  waiter correctly woken; findings per round ran 3 → 2 → 1 → 1. M1 closed and stayed closed.
- **Not converging:** method, on M2. Rounds 3 and 4 are the same seam at finer granularity.
  Decreasing severity should **not** be read as the shape being right.

**Recorded implementer failure mode, because it recurred and is generalisable:** the round-3
regression test covered only post-delete resume because it was scoped to *the instance the finding
described* rather than to *the structure containing it* — the same error as scoping a `by_identity`
enumeration to `dag.rs` instead of the whole module. Both times a reviewer found the unscoped part.
This is itself an argument for §5.2's invariant over additional per-window tests.

**The transferable rule from this train:**

> **The test for "by construction" is an enumeration, not an intuition.**

The one place the claim held (the retirement floor) held only because every writer was enumerated
and exactly one was found. Two other "structural" claims made in this train — "no public API
widening" and "unusable by construction" — were **false and withdrawn**, both being a convention
plus a lint.

---

## Debt row: `SCHED-UNIFY-LIFECYCLE-ATOMICITY`

| Field | Value |
|---|---|
| **Row** | `SCHED-UNIFY-LIFECYCLE-ATOMICITY` |
| **Disposition** | `DEFER` |
| **Ruling reference** | `GB4-S0-DEFER-2026-07-26` |
| **Durable owner** | the UNIFY cutover block (this plan) |
| **Resolution gate** | blocking UNIFY acceptance, **no later than plan close** |
| **Acceptance ID** | `SCHED-UNIFY-A1` |
| **Named test** | `remove_and_file_admission_share_one_lifecycle_transaction` |

**Residual being carried.** `remove()` is not atomic across `nodes` and `SchedulerDag`. Between the
cancellation sweep and `nodes.remove()`, an admission can observe the node still published at the
same incarnation, pass the crossing gate, and be admitted at exactly the retirement floor. The
identity is never cancelled; a later dequeue reserves capacity, finds no `FileNode`, and skips
without cancelling ⇒ **a leaked admission permit in RELEASE builds.** In DEBUG builds the
dispatch skip's `debug_assert` fires first, so the symptom is a PANIC rather than a leak. The
waiter itself *is* woken by `signal_file_shutdown` in both, so this residual is a **capacity leak
(release) / assertion panic (debug), never a hang.**

**Why it was landed rather than held.** A second seat diff-audited every production hunk against
`main` and found **no state in which the branch is worse**: the floor rejects only generations
`< floor`, cache work is exempt, current-incarnation/current-generation work proceeds, legitimate
re-adds start above the removed generation, and the anti-over-refusal controls hold. Decisive
finding: **"the residual remove race is a strict subset of a window already present on `main`."**
Holding would knowingly leave the reproduced hang-plus-leak live while the cutover is not imminent.

**Not to be closed by:** a further per-window gate at an admission site. Four rounds of evidence say
that closes an instant and reveals the next one. It closes only at `SCHED-UNIFY-A1`.
