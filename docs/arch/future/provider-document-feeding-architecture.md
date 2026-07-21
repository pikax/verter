# Provider document-feeding architecture — design v2 (RESCOPED, decision-ready)

> **Status: PROVISIONAL — rescoped after the empirical proof; awaiting the product-owner
> implement-or-not decision.** The cause is EMPIRICALLY CONFIRMED and the design was rescoped when
> the real-world proof refuted v1's central premise (de-flood is NOT the fix — it regresses). Not
> approved for implementation. Saved for review + the decision.
>
> Confidentiality: the motivating project is a private third-party monorepo — never named here.
> Committed tests are hermetic + synthetic; the real-corpus reproduction is scratch/uncommitted.

## 1. Reasons for the change (why this exists)

A real Vue monorepo produced **no TypeScript intellisense** on the file the user was actively
editing — on **both** managed tsgo AND tsserver — even after the project-selection fix made its
files sync. Root cause (confirmed): the active document's IDE companion never reaches a committed,
capturable surface on the provider under the background codebase-sweep flood ("gate C"). The active
open uses a **1-second timeout** that abandons the companion under saturation; there is **no
active-vs-sweep priority scheduler**; a superseded commit is stranded by a **one-shot drain** that
only runs at init; `SYNC_DONE` is logged unconditionally (attests "the pass ran," not "the companion
is live"). It is scale-dependent (a small fixture serves; the real project floods), which is why it
"works then regresses" and recurred for a long time — the tests never exercised the flood.

The sweep-every-companion model was a deliberate bet on tsgo's capacity. The bet's premise about
tsgo was right; its **topology** was wrong — it conflated engine program-capacity with the
interactive lane's scheduling headroom. Both engines starved.

## 1a. Empirical proof (real binary, real providers, real corpus, both routes)

- **CAUSE CONFIRMED.** Gate-C starvation reproduces on both managed tsgo AND tsserver under an
  aggressive carrier flood (trace `has_sync_state=false`).
- **De-flood ALONE REGRESSES.** tsserver aggressive-flood, identical probe: **baseline 13 gate-C
  failures → de-flood-the-sweep 27 → retain-active-open-60s 24 → both 30.** ROOT REASON: the sweep's
  provider-sync is an **accidental commit-net** — it re-commits the companions the abandoned 1s open
  dropped. Remove/throttle it → more starvation.
- **The `_background`/`Lane::Background` verbs are DORMANT** — default `open_file` aliases
  (`verter_type_runtime/src/traits.rs:247-261`); tsserver has NO override; tsgo has real lanes
  (`ipc.rs:3668`) but the sweep uses the Interactive default verbs. No real foreground-before-
  background scheduler exists on the shared connection.

## 2. Corrected thesis + the ActivePriorityScheduler (CR)

**The load-bearing fix is a real active-vs-sweep priority scheduler that commits and re-drives the
active companion with priority until served, while PRESERVING (then explicitly replacing) the
coverage the sweep provides as the accidental net. De-flooding is a SECONDARY optimization GATED ON
that scheduler already covering the active doc.**

The scheduler is **built at the reconciler (provider-neutral, above the wire)** — NOT a re-wiring of
the dormant lanes. One per provider connection, two logical slots over the single connection:
- **Active slot (P0/P1):** the focused + open-set companions. Preempting and retained; re-drives the
  active companion until receipt-admitted (never abandons at a timeout). At most one active op
  in-flight; always selected before any coverage/demand op.
- **Coverage/Demand slot:** everything else, strictly serial, yields the connection to the active slot
  (never starts an op while active work is pending/in-flight).

Correctness properties, each countering a measured failure: **no abandon** (retained ledger
obligation, delete the 1s timeout); **priority by selection not by lane** (arbitrated in Rust at the
reconciler — works on tsserver which has no lanes, and tsgo; the dormant/real lanes are demoted to a
BD optimization); **retention without holding the pipe** (retention lives in the LEDGER re-drive, NOT
a lengthened interactive await — the crucial difference from the measured RETAIN-60s regression,
which serialized the pipe); **bounded active latency** (≈ one op, independent of coverage depth or
repo size).

## 3. Coverage preservation — the accidental net becomes a subordinated Coverage role (CR)

The sweep's `sync_file_to_provider` STOPS being an independent wire-writer and becomes a **ledger
writer** declaring a **Coverage role** (blanket companion presence), driven by the reconciler BELOW
the active slot.
- **Phase A (correctness landing):** Coverage = the same blanket set the sweep enumerates today — the
  net is PRESERVED but subordinated. **No de-flood in Phase A; flood volume unchanged.** The fix is
  that the active companion is committed with priority and never abandoned, so gate C drops *despite*
  the flood. This is exactly the config the regression matrix never tested (it toggled de-flood/retain
  WITHOUT a working scheduler).
- **Phase B (optimization, GATED):** replace blanket Coverage with **closure-precise P2** (carrier-
  import closure of the open set, resolver-computed) + on-demand breadth spikes — flood volume finally
  drops. Gate: must be PROVEN that P2 + breadth commits at least what the net committed that mattered
  (gate C stays ≤ Phase A; cross-file features correct). If a gap is found, blanket Coverage stays as
  an opt-in default-ON lever. De-flood is never allowed to raise gate C.

## 4. Desired-state ledger (CR)
Per connection, keyed by provider path: `{provider_path, role, content_source (id+kind, not bytes),
generation, engine_state, bound_project}`. All writers upsert/retract desired entries only; the
reconciler is the SOLE wire-writer (structural). Collapses the FIVE current wire-writers (interactive
path, sweep, `sync_coordinator`, `resync_non_carrier_file`, `MembershipReconciler`) and the four
requeue/replay stores (`pending_snapshot_provider_sync`, `needs_ide_sync`, `needs_deferred_sync`,
lazy-managed replay list) into one authority. Supersession = generation bump; the entry stays dirty
and is re-picked — **steady-state convergence replaces the one-shot drain**. Respawn ⇒ all NotSent ⇒
converge active-first.

## 5. Roles → slots (CR)
Active (P0 focused companion) + Open (P1 other open carriers) → Active slot. DemandClosure (P2) +
Coverage (blanket ex-sweep) + optional Warm (default OFF, managed only) → Coverage/Demand slot.
Never fed: real disk `.ts/.js` not open, node_modules contents, build output (engine/store owns).
Role→slot is one exhaustive match.

## 6. Retained obligation, reconciled with the RETAIN-60s regression (CR)
On a gate-C miss the interactive request bumps the Active ledger entry and **awaits a convergence
event bounded by its own deadline**, then fails closed as today — but the obligation persists and the
scheduler keeps re-driving it in the background. The request NEVER holds the connection/repair lease
for a long deadline (this is why it ≠ the measured RETAIN-60s that serialized the pipe). `SYNC_DONE`/
`typeProviderSyncComplete` moves to the reconciler, fires only on receipt-admitted convergence per
document (deletes the unconditional log).

## 7. Three provider models
- **tsserver (CR):** membership via plugin store (`MembershipReconciler` becomes the tsserver arm of
  the one scheduler); Active/Open = the only wire opens; Coverage (Phase A) = current sweep behavior
  subordinated behind Active — **this is exactly what fixes the tsserver regression WITHOUT
  de-flooding**. Correctness relies entirely on the reconciler slot gating (tsserver `_background` is
  a dormant alias). Phase B: Coverage → store publications + P2, gated on gate-C non-regression.
- **managed tsgo (CR+BD):** Active/Open Interactive, Coverage/P2 the Coverage slot; routing the
  Coverage slot onto the REAL tsgo Background lane (`ipc.rs:3668`) is a BD tightening. BD: lease all
  tsconfigs (`openProjects`) so the engine owns the real-file graph; on-demand breadth spikes
  (find-refs/rename/check-project via Verter's reverse-dep graph + whole-program `--api`, LRU-retire)
  — the correct home for the tsgo-scale bet. Dedupe per-file `configure_paths` (CR).
- **shared tsgo (CR):** demand-only FROM DAY ONE — it has NO accidental net to preserve (a blanket
  net there is the guest-flood hazard); inject = Active∪Open∪DemandClosure of current demand;
  `inject_all_dirty` → demand-scoped; inject eligibility type-limited so Coverage/Warm are
  unrepresentable; all guest rails unchanged; repo-scale ops route to the managed engine. (De-flood is
  correctness-required HERE, gated everywhere else.)

## 8. Migration — correctness-ordered, two landings, coverage never lost mid-flight (CR)
**Landing 1 (correctness core):** introduce ledger + ActivePriorityScheduler as the sole wire-writer;
the five writers become ledger writers; the sweep upserts **Coverage-role** entries (net preserved,
subordinated) — **NO de-flood/throttle**; delete the two 1s timeout wraps, `pending_snapshot_provider_sync`
+ its one-shot drain + init call, `needs_ide_sync`/`needs_deferred_sync`, `signal_priority`, the
unconditional `SYNC_DONE`. **Proof gate to land:** gate-C count under the flood goes DOWN vs baseline
on both routes.
**Landing 2 (de-flood optimization, GATED on Landing 1's proof):** replace blanket Coverage with
closure-precise P2 + on-demand breadth (managed) / store publications (tsserver); scanner →
`WorkspaceIndexer` (keeps FS-walk + host upsert/compile/set_import_dependencies + tsserver store
inputs; deletes `follow_node_modules_deps` + the blanket provider-feed). **Proof gate:** gate-C stays
≤ Landing 1 AND cross-file/breadth features correct AND flood volume drops; else blanket Coverage
remains an opt-in default-ON lever. Each landing is single-wire-writer (no dual path). A single atomic
landing is acceptable only if it carries both proofs.

## 9. CORRECTNESS-REQUIRED (rescoped)
1. Build the ActivePriorityScheduler (reconciler-level slot arbitration; active preempts + retained; NOT dependent on the dormant lanes); delete the 1s sync timeouts.
2. Preserve the sweep's coverage as a subordinated Coverage role; do NOT reduce flood volume until #4 is proven.
3. One ledger + one single-writer reconciler = sole wire-writer; the five writers + four requeue/replay stores collapse in; verbs module-private.
4. De-flood is GATED, not required: P2 + breadth may replace blanket Coverage only after measurement shows gate C ≤ scheduler-only (Landing 1) and cross-file/breadth stay correct. De-flood must never raise gate C.
5. Retention in the ledger (re-drive), NOT a lengthened interactive await; the request awaits a convergence event bounded by its deadline, never holds the connection/repair lease.
6. Truthful convergence attestation (receipt-admitted, per document); delete the unconditional SYNC_DONE.
7. Migration order is a correctness constraint: scheduler+coverage lands + is proven before de-flood; coverage never lost mid-flight (or both proofs in one atomic landing).
8. Shared tsgo demand-only from day one; inject eligibility type-limited; guest rails preserved.
9. Existing fail-closed gates unchanged (capture, admit_owned, shadow-safety, BoundProject-only, no-poison).
10. No heartbeat as load control; crash rails (B12) are the only liveness sensors; any stats endpoint is observability-only (tests pass with it absent).
11. Structural confinement (sole-writer visibility, exhaustive role→slot match, injectable-role type split); no name-keyed scanner guards.
12. The §10 acceptance contract is the definition of done.

## 10. Acceptance (rescoped — regression now a first-class guard)
Hermetic synthetic corpus generator (N ∈ {200,1000,2000}, realistic import trees) + constrained-
resource config + a deterministic provider-saturation seam; real providers on CI for all routes.
1. RED-first starvation repro (both routes): flood + open one carrier + hover ⇒ gate C today; GREEN: active converges, hover answers within an N-independent bound.
2. **REGRESSION GUARD (mandatory):** de-flooding/retaining WITHOUT the scheduler must RAISE gate C vs baseline — guards a naive future change that removes the net before the scheduler covers it.
3. **COVERAGE-PRESERVING PROOF (mandatory):** the final design drives gate C DOWN vs baseline on both routes (not merely "active served" but "active served AND coverage preserved"); Landing 1 asserts it with blanket Coverage, Landing 2 asserts it holds after P2/breadth replaces it.
4. No-abandon/no-strand under an edit storm; Superseded followed by convergence.
5. No-pipe-hold: the interactive request does not hold the connection/repair lease for its deadline; concurrent interactive requests under flood don't serialize.
6. Closure/breadth correctness: hover through a never-opened imported carrier resolves; find-refs across un-opened carriers returns the full set.
7. Three routes non-vacuous (managed tsgo, tsserver, shared relay); shared asserts inject-count ≤ demand bound + editor-latency probe within budget.
8. Respawn convergence: active re-converges FIRST.
9. No-heartbeat/capacity: all pass on the constrained config with stats disabled and (managed) Warm OFF.

## 11. BETTER-DESIGN
Coverage slot onto the real tsgo Background lane; real tsserver background lanes if reconciler one-op
preemption proves insufficient; tsgo project-first leases + whole-program check-project; on-demand
breadth-spike machinery; content-bearing updateSnapshot when upstream lands; opt-in managed Warm
(default OFF); polled stats endpoint (observability only); tsserver updateOpen batching; LRU cap on
open docs; contents-cache audit; configure_paths dedupe; shared editor-latency guest metric;
convergence events to the audit substrate.

## 12. Decision gate (blocks implementation)
- [x] Reproduce the bug on the real corpus, both routes — trace shows gate C. (DONE)
- [x] Empirically test the naive fix — de-flood/retain-alone REGRESS (13→27/24/30). (DONE — refuted v1; drove the rescope.)
- [x] Re-send to the architect to rescope with the empirical findings. (DONE — this v2.)
- [ ] **Product-owner decision to implement (or not).**
- [ ] If yes: commit the hermetic RED reproduction + the regression guard FIRST (fail-red-on-timeout), then Landing 1 (proven gate-C-down), then Landing 2 (gated).
