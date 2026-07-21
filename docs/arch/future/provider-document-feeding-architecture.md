# Provider document-feeding architecture — design v3 (production-ready candidate)

> **Status: PROVISIONAL — v3 revises v2 to close all 8 CORRECTNESS-REQUIRED findings of the
> un-primed adversarial review; awaiting the product-owner implement-or-not decision.** The cause
> is EMPIRICALLY CONFIRMED (§1a). v2's thesis (scheduler-first, de-flood gated) is preserved; v2's
> load-bearing gap (no child-synchronized completion on any route — §1b) is closed by the fence
> (§3). Not approved for implementation. Design only.
>
> Confidentiality: the motivating project is a private third-party monorepo — never named here.
> Committed tests are hermetic + synthetic; the real-corpus reproduction is scratch/uncommitted.
>
> All file:line references verified against `verter-release-clean` (read-only) at review time.

## 0. v2 → v3 delta

- **§3 (NEW, load-bearing):** per-route child-synchronized ack/fence + a bounded unfenced-coverage
  window W. This — not the scheduler alone — is what makes "bounded, N-independent active latency"
  true and is the Phase-A anti-over-feed bound. Closes CR-1.
- **§6 (NEW):** explicit liveness boundary — every reconciler await terminates via exactly one
  named sensor; end-to-end wedge-recovery chain; anti-respawn-storm argument. Closes CR-2.
- **§11 (REWRITTEN):** the stress seam moves to provider-side serial processing; assertions become
  wire-op-log counts; wedged-provider case added; per-landing applicability table; the full
  per-route contract (tsserver T1–T8, managed +G1–G4, shared S1–S4); shared gets the full proof
  set. Closes CR-3, CR-7, CR-8.
- **§8 (NEW):** quantified Phase-A per-route over-feed bounds; a fired Landing-2 fallback is
  tracked debt, never a terminal state. Closes CR-4.
- **§4/§5 (EXTENDED):** complete wire-writer inventory with fold-in/exempt disposition; the API
  companion becomes a first-class ledger `content_source.kind`; direct-import companions stay in
  the Active slot in Landing 1. Closes CR-5.
- **§11.2 (RE-SPECIFIED):** the de-flood regression guard is split into a pre-landing
  characterization of the old binary plus two committed post-cutover structural guards. Closes CR-6.

Preserved from v2 unchanged in substance: §1 (root cause), §1a (empirical matrix), the
scheduler-first / de-flood-gated thesis, the two-landing correctness-ordered migration.

## 1. Reasons for the change

A real Vue monorepo produced **no TypeScript intellisense** on the actively edited file — on
**both** managed tsgo AND tsserver — even after the project-selection fix made its files sync.
Root cause (confirmed): the active document's IDE companion never reaches a committed, capturable
surface under the background codebase-sweep flood ("gate C" = the fail-closed capture at
`crates/verter_lsp/src/server/sync_orchestration.rs:1873-1925` — `current_snapshot(..)?` +
`authorizes_carrier_ide_capture` refusal returns `None`). The active open/sync uses a **1-second
timeout** that abandons the companion under saturation (`sync_orchestration.rs:1186-1200`; the
timeout arm requeues to a set, `:1331-1337` → `needs_ide_sync.insert`); there is **no
active-vs-sweep priority scheduler**; a superseded commit is stranded by a **one-shot drain** that
runs only at init (`background_init.rs:360` → `background_drain.rs:88`; self-described "one-shot
drain — a known limitation", `external_ts/carrier_sync.rs:939-942`); `SYNC_DONE` is logged
unconditionally at pass end (`sync_coordinator.rs:482`; `TypeProviderSyncComplete` likewise fires
without per-document commit attestation, `sync_coordinator.rs:470-481`,
`background_init.rs:517-523`). Scale-dependent: a small fixture serves; the real project floods —
which is why it "works then regresses" and why tests never caught it.

The sweep-every-companion model was a deliberate bet on tsgo's capacity. The bet's premise about
tsgo was right; its **topology** was wrong — it conflated engine program-capacity with the
interactive lane's scheduling headroom. Both engines starved.

## 1a. Empirical proof (real binary, real providers, real corpus, both routes)

- **CAUSE CONFIRMED.** Gate-C starvation reproduces on both managed tsgo AND tsserver under an
  aggressive carrier flood (trace `has_sync_state=false`).
- **De-flood ALONE REGRESSES.** tsserver aggressive-flood, identical probe: **baseline 13 gate-C
  failures → de-flood-the-sweep 27 → retain-active-open-60s 24 → both 30.** ROOT REASON: the
  sweep's provider-sync is an **accidental commit-net** — it re-commits the companions the
  abandoned 1s open dropped. Remove/throttle it → more starvation.
- **The `_background` verbs are DORMANT** — default aliases of the Interactive verbs
  (`crates/verter_type_runtime/src/traits.rs:247-273`); tsserver has NO override; tsgo has real
  priority lanes (`tsgo/ipc.rs:516`, `:3669-3682`) but the sweep uses the Interactive defaults. No
  real foreground-before-background scheduler exists on the shared connection.
- (Matrix from the scratch real-corpus run; not re-verifiable hermetically — accepted as input,
  consistent with the code structure above.)

## 1b. The wire fact v2 missed: sync ops complete at LOCAL ENQUEUE (grounds §3)

On every route, the document-sync verbs' futures resolve when the frame is accepted by a
**Verter-side queue**, not when the child has processed it:

- **Managed/shared tsgo:** `didOpen`/`didChange`/`didClose` are LSP notifications delivered via
  `try_notify_with_priority` → `try_send` onto a bounded stdin lane
  (`crates/verter_type_runtime/src/tsgo/ipc.rs:721-759`); `deliver_document_sync` commits the
  local version/content ledger in the same step as an ACCEPTED ENQUEUE (`ipc.rs:826-891`). Lane
  capacity is 1024 per lane (`:161`), writer batch caps 5/3 (`:163-166`). A full lane returns
  `Err("channel full")` (`:743-750`).
- **tsserver:** `open`/`updateOpen`/`close` are sent via `command_no_response`
  (`crates/verter_type_runtime/src/tsserver/ipc.rs:257-289`; `updateOpen` sites `:1314`, `:1341`,
  touch `:1451`, `close` `:1423`) — a sequence-numbered request with NO pending entry registered;
  the future resolves on `stdin_tx.send` acceptance (`:282-285`). The protocol defines a response
  for `updateOpen`; the client currently discards it.

Consequences v2's design words did not survive:

1. "One in-flight active op", "strictly serial coverage", and "receipt-admitted" pace only
   **Verter-side dispatch**. The child eagerly drains stdin into its internal inbox, which is
   **unbounded**: the provider-side backlog ahead of the active companion and the subsequent hover
   is unbounded by the scheduler.
2. In Landing 1 (flood volume deliberately unchanged) the v2 scheduler moves starvation from gate
   C (client-side capture — which WILL go green) to provider **answer latency** — still
   O(backlog), N-dependent. The user's symptom changes shape, not size.
3. **Hang-sensor gap:** only tracked requests feed hang detection (tsserver consecutive request
   timeouts → `HANG_THRESHOLD = 3` → `crash_notify`, `tsserver/ipc.rs:119`, `:229-240`; tsgo
   request timeout → `note_hang_failure`, `tsgo/ipc.rs:700`). Sync verbs never feed it. The tsgo
   writer-stall watchdog (`WRITER_STALL_TIMEOUT_SECS = 10`, progress-aware per accepted byte,
   `tsgo/ipc.rs:168-191`) trips only when the child stops READING. A wedged-while-reading child
   (reads frames, never answers) under sync-only traffic starves silently. v2 additionally deleted
   the 1s timeouts — removing the only (bad) sensor without naming a replacement.
4. **No anti-over-feed bound exists anywhere in Phase A** without a child-synchronized completion
   event.

**In-tree precedent that the fix is sound:** the shared-tsgo relay ALREADY implements a
child-synchronized fence — `CarrierInjectionChannel::sync_overlay`
(`crates/verter_tsgo_api/src/relay.rs:366-396`) issues a `textDocument/diagnostic` REQUEST after
`didOpen`/`didChange`; LSP in-order processing on one connection means an answered (or
JSON-RPC-error-answered) later request attests all prior notifications applied (`relay.rs:340-359`);
bounded by `CARRIER_SYNC_BARRIER_TIMEOUT = 10s`, fail-closed (`relay.rs:157-162`). Proven ordered:
`relay_tests.rs:1801` (`injected_didopen_precedes_sync_barrier`). §3 generalizes this pattern.

## 2. Corrected thesis (v3)

**Two load-bearing mechanisms, neither sufficient alone:**

1. **The ActivePriorityScheduler** (v2 §2, preserved): reconciler-level slot arbitration,
   provider-neutral, above the wire. Bounds *Verter-side selection*: the active companion is always
   selected before coverage, retained (never abandoned), re-driven until converged.
2. **The fence (§3, new):** per-route child-synchronized completion + a bounded unfenced-coverage
   window. Bounds *provider-side backlog*: at most W unfenced coverage ops can precede any
   interactive op in the child's inbox.

De-flooding remains a SECONDARY optimization GATED on the scheduler+fence already covering the
active document (§1a: de-flood alone regresses). The dormant `_background` lanes remain demoted to
a BETTER-DESIGN optimization — priority is by reconciler selection, not by lane, so it works
identically on tsserver (no lanes) and tsgo.

## 3. The fence — per-route child-synchronized completion + coverage window (CR-1)

### 3.1 Per-route completion event ("fence-ack")

- **tsserver — track the `updateOpen` response (free ack).** Consolidate document lifecycle onto
  `updateOpen` (its `openFiles`/`changedFiles`/`closedFiles` arrays cover open/update/close; the
  protocol answers it — unlike bare `open`), and send it through the response-tracked path
  (register a pending entry keyed by seq, as `request_with_timeout` does at
  `tsserver/ipc.rs:171-172`, instead of `command_no_response`). Fence-ack = the `updateOpen`
  response. Zero additional wire traffic.
- **Managed tsgo — barrier request per coverage op/batch.** `didOpen`/`didChange` are
  notifications; after each coverage op (or batch, §3.2) the reconciler issues a cheap in-order
  REQUEST on the same connection — the same op the shared relay uses (`textDocument/diagnostic`,
  result discarded; a completed JSON-RPC error response also attests in-order consumption,
  `relay.rs:350-359`). Fence-ack = the barrier's completed round-trip.
- **Shared tsgo — already fenced.** Every injection drives through the ordered per-carrier state
  machine (`verter_lsp/src/tsgo/shared.rs:850` `inject_carrier` → `drive_carrier`;
  `carrier_did_open_synced`/`carrier_did_change_synced`, `shared.rs:820-843`) whose `*_synced`
  verbs embed `sync_overlay` (`relay.rs:366-396`). v3 requires no new mechanism here — only that
  the ledger records the barrier result as the fence-ack, and the per-call fail-closed timeout
  contract (guest degrades to OWNED baseline) stays unchanged.

The fence-ack is the ONLY transition to ledger `engine_state = Fenced` (§4.1), the only state that
fires the per-document convergence event, and therefore the only state `SYNC_DONE` /
`typeProviderSyncComplete` can attest.

### 3.2 The coverage-window invariant

> **INVARIANT (anti-over-feed, Phase A and forever): at most W unfenced coverage ops are
> outstanding per connection at any time; at most 1 unfenced active op. W ∈ [1,4]; default W = 2;
> the acceptance suite runs at W = 1 and W = 4.**

- W counts OPS, not batches. Batching (BD: one barrier per K ops) is admitted only while
  `outstanding + K ≤ W`.
- The reconciler will not enqueue a coverage op while the window is full; it parks on the
  fence-ack event (§6 sensor 3), not on a retry loop.
- Respawn replay is coverage-classed and fenced under the same W (G4) — a fresh child is never
  re-flooded.
- Interactive queries need no additional fence: they are tracked requests processed in-order
  behind at most W+1 sync ops.

### 3.3 What the window buys (and rules out)

- **Bounded, N-independent active latency — now TRUE:** provider-side work ahead of an interactive
  op ≤ (W unfenced coverage ops) + (1 active op) + (1 barrier). With provider per-op cost δ:
  answer latency ≤ (W+2)·δ + ε, identical at N=200 and N=2000 (asserted, §11.4 T3).
- **Anti-over-feed:** the child's inbox can never hold more than W+2 Verter-fed sync frames; the
  1024-slot Rust lanes become belt-and-suspenders (the reconciler's outstanding ops ≪ capacity,
  so `Err(channel full)` is structurally unreachable on the single-writer path — §6 sensor 2
  covers it anyway).
- **Hang sensing restored on the sync path:** every fence is a tracked round-trip; its detection
  timeout feeds the same consecutive-failure counter as query timeouts (§6 sensor 3) — the sensor
  v2 deleted without replacement.
- **Cross-lane overtake ruled out** when the BD lane-routing lands: a query on doc P is issued
  only after P's fence-ack, so the "didChange for a document the child never opened" panic class
  ("overlay not found", `tsgo/ipc.rs:813-815`) cannot be reintroduced by lane reordering (G2).

## 4. Desired-state ledger

### 4.1 Entry shape and lifecycle

Per connection, keyed by provider path:
`{provider_path, role, content_source {id, kind}, generation, engine_state, bound_project}`.

- `content_source.kind ∈ {CarrierIde, CarrierApi, CarrierDecl, Shadow, NonCarrier}` — **the API
  companion is first-class (CR-5c):** today it exists only as `needs_deferred_sync`'s product
  ("Queue deferred API sync", `sync_orchestration.rs:1321-1322`) and the `did_open` prewarm
  (`lifecycle.rs:570-576` `sync_imported_carrier_api_lightweight`); deleting the store without the
  kind would strand API sync. `Decl` entries preserve the compile-time exclusion from generic
  closers (`provider_sync.rs:18-33`, `NonDeclProviderPathKind`) by role typing.
- `engine_state ∈ {NotSent, Enqueued, Fenced, Failed}`. `Enqueued → Fenced` happens ONLY on
  fence-ack (§3.1). Supersession = generation bump; the entry stays dirty and is re-picked —
  steady-state convergence replaces the one-shot drain. Respawn ⇒ all `NotSent` ⇒ converge
  active-first (§6).
- All writers upsert/retract desired entries only; **the reconciler is the SOLE wire-writer**
  (structural: verbs module-private to the reconciler).

### 4.2 Complete writer inventory (CR-5b) — fold in or exempt, no survivors

| Current writer (source) | Disposition |
|---|---|
| Interactive open/sync path (`sync_orchestration.rs` `ensure_current_file_synced`, timeouts `:1186-1200`) | FOLD IN: Active-role upsert; 1s timeouts deleted (§6 sensor 1 replaces them). |
| Sweep provider-feed (`workspace_scanner.rs:824` `sync_file_to_provider`) | FOLD IN: Coverage-role upserts (Phase A: same blanket set — net preserved, subordinated). |
| Watched-file resync (`workspace_scanner.rs:577` `resync_non_carrier_file`) | FOLD IN: NonCarrier-role upsert on disk change. |
| `sync_coordinator` pass | FOLD IN: ledger upserts; per-document convergence attestation moves to the reconciler (deletes unconditional `SYNC_DONE` `sync_coordinator.rs:482` and unconditional `TypeProviderSyncComplete` `background_init.rs:517-523`). |
| `MembershipReconciler` (`external_ts/membership_reconciler.rs:606`) | FOLD IN as the tsserver membership ARM of the one reconciler (it is already a single-writer actor with per-source serialization; its ledger becomes membership metadata on the entries). |
| `force_reopen_current_file_in_type_provider` (`sync_orchestration.rs:1341`) | FOLD IN: a generation-bumped Reopen upsert; the reconciler's per-document serialization subsumes the repair lease. Deleted as a wire path. |
| Stale-path closers ×3 (`background_drain::close_stale_provider_paths`, `sync_coordinator::close_stale_paths`, `workspace_scanner::close_stale_paths` — `provider_sync.rs:20-22`) | FOLD IN: staleness = ledger retraction; the reconciler issues the fenced close. Deleted as wire paths — a surviving close racing the reconciler's open is exactly the untracked-state class this kills. |
| `configure_paths` (`type_provider/lazy_managed.rs:680-689`, `tsgo/composite.rs:1243`, `extension_provider.rs:1312`) | FOLD IN as connection-scoped config rows (not per-doc); reconciler dedupes: ≤ 1 configure op per owner config per generation (G3). |
| Background diagnostics pulls (`background_init.rs:501`, `:556`) | EXEMPT from the document ledger (reads, not doc-state writes) but SCHEDULED: they consume connection bandwidth, so they run in the Coverage/Demand slot behind active work. |
| Resilient replay caches (`verter_type_runtime/src/resilient.rs:101-110` `CachedFile`/`CachedPathConfig`; `lazy_managed.rs:185`, `:212` `replay`) | DELETE: respawn ⇒ ledger all-`NotSent` re-convergence is the single desired-state authority. The resilient actor keeps kill/backoff/respawn (`resilient.rs:431` `max_restarts`, `:449` `spawn_crash_monitor`); it loses its parallel content cache. |
| Requeue/replay stores: `pending_snapshot_provider_sync` (`:1317-1320`) + one-shot drain (`background_drain.rs:88`, init call `background_init.rs:360`), `needs_ide_sync` (`:1329`, `:1336`), `needs_deferred_sync` (`:1322`) | DELETE: subsumed by ledger dirtiness + generation supersession (`needs_deferred_sync` → `CarrierApi`-kind entries). |

Rule: any wire-writing path not in this table is a defect; the landing's structural confinement
(verbs private to the reconciler module) makes a new one unrepresentable outside review.

## 5. Roles → slots (CR-5a)

| Role | Slot | Landing |
|---|---|---|
| P0 Active (focused companion) | Active | 1 |
| P1 Open (other open carriers) | Active | 1 |
| **P1.5 ImportNeighbors (direct-import companions + API surfaces of the open set)** | Active | **1** |
| P2 DemandClosure (resolver-computed carrier-import closure) | Coverage/Demand | 2 |
| P3 Coverage (blanket ex-sweep) | Coverage/Demand | 1 (Phase A), replaced in 2 |
| Warm (managed only, default OFF) | Coverage/Demand | opt-in |

**P1.5 is the CR-5a fix:** today `did_open` promotes the opened file AND each imported carrier
(`server/lifecycle.rs:561-567` — `signal_priority` per import id; `workspace_scanner.rs:49`,
dir-sibling promotion `:560-570`) and prewarms imported carrier APIs (`lifecycle.rs:570-576`).
v2's Landing 1 deleted `signal_priority` while P2 only arrives in Landing 2 — with blanket-order
Coverage that regresses cross-file hover vs today. v3: the ledger's P1.5 role carries the
import-neighbor promotion from Landing 1. (`signal_priority`'s provider-feed effect is subsumed;
its host-side scan-order promotion — which orders host compilation, no wire ops — is retained.)
Landing P2 itself in Landing 1 was considered and rejected: it widens Landing 1's blast radius
with resolver-closure machinery that the Landing-2 gate exists to prove.

Never fed, unchanged: real on-disk `.ts/.js` not open in the editor, `node_modules` contents,
build output (engine/store owns those). Role→slot is one exhaustive match.

## 6. Liveness boundary (CR-2) — every await names its sensor

### 6.1 Sensor table

| # | Await point | Sensor | On fire |
|---|---|---|---|
| 1 | Interactive request awaiting per-document convergence | The request's OWN deadline (existing per-request budget) | Fail closed to the caller as today; the ledger obligation PERSISTS (entry stays dirty; scheduler keeps re-driving). Never holds the connection or repair lease — this is why it is not the measured RETAIN-60s regression (§1a). |
| 2 | Reconciler wire enqueue | Bounded lane channel. tsgo `try_send` returns `Err(Full)` (`tsgo/ipc.rs:743-750`); tsserver `stdin_tx.send` awaits capacity (`tsserver/ipc.rs:282-285`) | On `Err(Full)`: PARK on a capacity-available event from the writer task (watermark notify), never busy-retry, never drop the obligation. Structurally near-unreachable: the single writer holds ≤ W+2 outstanding vs 1024 capacity (§3.3). The writer itself sits under the progress-aware stall watchdog (`tsgo/ipc.rs:168-191`) covering a child that stops READING. |
| 3 | Fenced sync op awaiting fence-ack | **Transport DETECTION timeout** — generous (same order as `CARRIER_SYNC_BARRIER_TIMEOUT = 10s`, `relay.rs:162`) and **progress-aware**: the window resets on ANY inbound child traffic (any response or notification), so it measures total silence, not congestion | **Detection ≠ abandon.** The obligation persists (entry returns to re-drivable state); the strike feeds the SAME consecutive-failure counter as query timeouts (tsserver `HANG_THRESHOLD = 3`, `tsserver/ipc.rs:119`, `:229-240`; tsgo `note_hang_failure`, `tsgo/ipc.rs:700`) → `crash_notify` → respawn. This is the ONLY sensor that catches a wedged-while-READING child during sync-only traffic (the writer-stall watchdog needs 10s of zero bytes ACCEPTED and does not trip while the child reads). Shared route: the existing per-call fail-closed barrier timeout contract stays (guest degrades to OWNED baseline, `relay.rs:361-365`); the reconciler-side obligation persists identically. |
| 4 | Respawn | Bounded restarts + backoff (`resilient.rs:431` `max_restarts`; crash monitor `:449`) | All entries → `NotSent`; event-driven re-convergence ACTIVE-FIRST; replay is coverage-classed and FENCED under W (G4) — the fresh child is never re-flooded into the same wedge. |
| 5 | Reconciler loop | Event-driven: wakes on ledger upsert, fence-ack, capacity-available, deadline expiry, respawn. **Progress-monotone:** each iteration sends ≥ 1 op, admits ≥ 1 commit, or blocks on an event | A `Superseded` admission refusal re-picks ONLY with a strictly-newer generation — no hot loop against a refusing admit gate. Grounded: `admit_owned` returns `Superseded` and the gate itself records a content transition so the requeue mints a strictly-newer key (`external_ts/carrier_sync.rs:929-943`). Content resolution (compile of the companion) is bounded by Verter's existing compile/projection fuses. |
| 6 | Convergence event delivery | Events are keyed `(document, generation, transport-epoch)` | No ABA across respawn or transport re-establishment: a waiter for gen G/epoch E ignores a converged (G, E′) — precedent: epoch-keyed injection markers, `tsgo/overlay_core.rs:634-641`. |

### 6.2 End-to-end wedge-recovery chain

Wedged child (reads, never answers) → fence/query detection timeouts (sensor 3; ≤ T_detect each)
→ consecutive failures reach `HANG_THRESHOLD` (bounded: ≤ HANG_THRESHOLD · T_detect of silence) →
`crash_notify` → resilient kill/backoff/respawn (sensor 4; ≤ `max_restarts`) → ledger all-`NotSent`
→ active-first fenced re-convergence → interactive requests that failed closed during the wedge
re-drive on their next issue against a converging active set. Every stage is bounded; no stage
abandons the desired state.

### 6.3 Anti-respawn-storm

(a) Progress-aware detection: a congested-but-progressing child under Phase-A flood volume is
answering *something* (interactive queries, earlier fences), so the silence window keeps
resetting — congestion is never misdiagnosed as a wedge. (b) Fenced replay: post-respawn inflight
is capped at W+2 ops, so the respawned child cannot be pushed back into saturation by replay
itself. (c) Bounded respawns with backoff (`max_restarts`). (d) Active-first replay converges the
user-visible surface before coverage resumes. (Precedent for storm-breaking by cooldown:
`MEMBERSHIP_RECOVERY_COOLDOWN`, `tsserver/ipc.rs:121-130`.)

## 7. Three provider models

- **tsserver (CR):** membership via plugin store (the `MembershipReconciler` becomes the tsserver
  arm of the one reconciler); Active/Open/P1.5 are the only interactive wire opens; Coverage
  (Phase A) = today's sweep set subordinated behind Active — this fixes the tsserver regression
  WITHOUT de-flooding. **Fence = the tracked `updateOpen` response (§3.1).** Correctness relies
  entirely on reconciler slot gating + the window (tsserver `_background` is a dormant alias,
  `traits.rs:247-273`). Phase-A open-documents cap per §8. Phase B: Coverage → store publications
  + P2, gated on §11.3.
- **Managed tsgo (CR+BD):** Active/Open/P1.5 Interactive; Coverage/P2 in the Coverage slot;
  **fence = the barrier request (§3.1)**. BD: routing the Coverage slot onto the REAL tsgo
  Background lane (`tsgo/ipc.rs:3669-3682`) — the lanes prioritize only at the stdin writer, so
  child-side priority still needs the fence; adopting it carries G2 (no query on P before P's
  fence-ack; the overtake hazard is the "overlay not found" class, `tsgo/ipc.rs:813-815`). BD:
  lease all tsconfigs (`openProjects`) so the engine owns the real-file graph; on-demand breadth
  spikes (find-refs/rename/check-project via Verter's reverse-dep graph + whole-program `--api`,
  LRU-retire) — the correct home for the tsgo-scale bet. Dedupe per-file `configure_paths` (G3).
- **Shared tsgo (CR):** demand-only FROM DAY ONE — it has NO accidental net to preserve (a blanket
  net there is the guest-flood hazard); inject = Active ∪ Open ∪ DemandClosure of current demand;
  `inject_all_dirty` (`tsgo/overlay_core.rs:642`) becomes demand-scoped; inject eligibility
  type-limited so Coverage/Warm are unrepresentable; all guest rails unchanged (write-gate
  allowlist, shadow-safety, epoch replay `overlay_core.rs:634-641`); repo-scale ops route to the
  managed engine. Already fenced (§3.1). Hard caps per §8. De-flood is correctness-required HERE,
  gated everywhere else.

## 8. Phase-A over-feed bounds — quantified, per route (CR-4)

Declared constants. Values are landing-tunable defaults; the EXISTENCE and ENFORCEMENT of each
bound is acceptance-tested (the suite pins enforcement at the configured value, not the value
itself), and each is asserted from the wire-op log (§11.0).

| Bound | tsserver | managed tsgo | shared tsgo |
|---|---|---|---|
| Unfenced coverage window W | 2 (tested 1 and 4) | 2 (tested 1 and 4) | 1 (the existing per-carrier `*_synced` barrier is per-op) |
| Total open provider documents | `TSSERVER_OPEN_DOCS_CAP = 128` companions, LRU-retire Coverage; Active ∪ Open ∪ P1.5 pinned (never evicted). Justification: unbounded open-doc growth is itself the over-feed class; the review reports a practical cliff near ~500 concurrent opens at scale (empirical input, no in-repo source) — the cap sits well below with margin, and the N=2000 corpus forces eviction churn through the gate. Declared Phase-A ALTERNATIVE (choose ONE at landing, same M3 gate): tsserver Coverage rides store publications only (zero coverage wire-opens; wire opens = Active ∪ Open ∪ P1.5). | `MANAGED_OPEN_DOCS_CAP = 512` overlays (the engine owns real files via project config; the cap bounds Verter-fed overlays), LRU-retire Coverage; Active set pinned | `SHARED_LIVE_OVERLAYS_CAP = 128` total live injections; Active set pinned |
| Per-demand-event injection | n/a | n/a | `SHARED_INJECT_HARD_CAP = 32` per demand event (constant). Over cap ⇒ **visible degrade**: truncate closure, inject per-query subjects only, surface partial-closure status — never silently unbounded (barrel imports make closure unbounded otherwise) |
| Closure computation | n/a (Phase A blanket) | n/a (Phase A blanket) | DemandClosure computation runs under Verter's existing per-request projection/compile fuses; budget-exceeded ⇒ the same visible degrade |

**Landing-2 fallback is debt, not a terminal state (CR-4):** if the Landing-2 gate fires its
fallback (blanket Coverage retained because P2+breadth missed something), that is a tracked
correctness-DEBT row per the repo's finding-disposition rule — named owner (the feeding-block
owner), resolution gate no later than plan close / next release freeze, the acceptance ID of the
missed coverage, and the measurement that fired it. The permanent "opt-in default-ON lever"
formulation of v2 §3 is retracted: the lever may exist as the mitigation while the debt row is
open, but the no-over-feed end-state (§8 bounds + P2) remains the committed target on all three
routes.

## 9. Migration — correctness-ordered, two landings, coverage never lost mid-flight

**Landing 1 (correctness core):** ledger (§4) + ActivePriorityScheduler + **fence (§3 — Landing-1
scope: the anti-over-feed bound must exist from the first landing)** as the sole wire-writer; the
COMPLETE §4.2 inventory folds in (not just v2's five); the sweep upserts Coverage-role entries
(net preserved, subordinated) — **NO de-flood/throttle; flood volume unchanged**; P1.5 keeps
import-neighbor promotion live (CR-5a). Deletions in the same landing: the two 1s timeout wraps
(`sync_orchestration.rs:1186-1200`), `pending_snapshot_provider_sync` + one-shot drain + init call
(`:1317-1320`, `background_drain.rs:88`, `background_init.rs:360`), `needs_ide_sync` (`:1329`,
`:1336`), `needs_deferred_sync` (`:1322` — replaced by `CarrierApi` ledger entries),
`force_reopen` as a wire path (`:1341`), the three stale-path closer wire paths
(`provider_sync.rs:20-22`), the resilient/lazy-managed replay caches (`resilient.rs:101-110`,
`lazy_managed.rs:185`/`212`), the unconditional `SYNC_DONE` (`sync_coordinator.rs:482`) and
unconditional `TypeProviderSyncComplete` (`background_init.rs:517-523`) — per-document fence-acked
attestation replaces both. **Proof gate to land: §11.3's Landing-1 column, headline M1 = 0 on all
three routes.**

**Landing 2 (de-flood optimization, GATED on Landing 1's proof):** replace blanket Coverage with
closure-precise P2 + on-demand breadth (managed) / store publications (tsserver); scanner →
`WorkspaceIndexer` (keeps FS-walk + host upsert/compile/set_import_dependencies + tsserver store
inputs; deletes `follow_node_modules_deps` + the blanket provider-feed). **Proof gate:** §11.3's
Landing-2 column — M1 stays 0 with coverage-role OFF, cross-file/breadth features correct, flood
volume drops; a fired fallback opens the §8 debt row. Each landing is single-wire-writer (no dual
path). A single atomic landing is acceptable only if it carries both proofs.

## 10. Landing-1 proof metrics — pinned (CR-7)

Three separated metrics, replacing v2's conflated "gate-C count goes DOWN":

- **M1 — active capture (gate C):** count of interactive requests refused at
  `capture_provider_request_surface` (`sync_orchestration.rs:1873-1925`) during the scripted
  probe, after each probe step's convergence await. **Seam target: ZERO** (exact op-log count, not
  "down"). Baseline procedure pinned: same generated corpus (fixed seed), same scripted probe
  sequence, same seam δ, recorded pre-landing on the old binary (§11.2a).
- **M2 — active answer latency:** wall-clock ONLY as a function of the seam's δ: every probe hover
  answers ≤ (W+2)·δ + ε, asserted EQUAL at N=200 and N=2000 (N-independence is the claim; ε
  enumerated: fence + config + query cost).
- **M3 — non-active coverage commit rate:** fraction (and op-log count) of Coverage-role entries
  reaching `Fenced` by end of run. Landing 1 must not reduce M3's terminal value vs baseline —
  the "coverage preserved" claim gate C cannot measure (its beneficiaries are non-active
  companions). Landing 2's "P2 + breadth commits what the net committed that mattered" becomes
  falsifiable as: M3 restricted to the demanded closure = 100% AND the §11.1(6) cross-file feature
  assertions stay green with coverage-role OFF.

## 11. Acceptance — the stress test as proof (CR-3, CR-6, CR-8)

### 11.0 The seam (CR-3a) and the op log (CR-3b)

- **Seam placement: provider-side serial processing.** A transport-shaped mock provider per route
  consumes frames strictly IN ORDER with a configurable per-op delay δ, and answers a request only
  after every prior frame is consumed — modeling the child's eager single-threaded inbox. It sits
  BELOW the production transport code (consumes the exact frames the production writer emits;
  precedent: the tsserver harness that mirrors production framing, `tsserver/ipc_tests.rs:1244+`,
  and the mockable `GatedWireSink` seam, `relay.rs:126-155`). A Rust-side WRITE-delay seam is
  forbidden as the proof seam: it lets the mock inbox absorb everything instantly, so RED cannot
  go red and GREEN proves nothing.
- **Wire-op log:** every provider-bound op records `(op, path, role, slot, generation,
  enqueue-ts, fence-ack-ts)`. All bounds are asserted as **op-log counts** (deterministic,
  machine-independent); wall-clock appears only in δ-derived bounds (M2, T3).
- Corpus: hermetic synthetic generator, N ∈ {200, 1000, 2000}, realistic import trees, fixed
  seed; constrained-resource config; scripted probe sequences.

### 11.1 Criteria (all three routes unless marked)

1. **RED-first starvation repro:** flood + open one carrier + hover ⇒ on the pre-landing code the
   probe records gate-C misses (RED goes red BECAUSE the seam is provider-side); post-landing
   GREEN: M1 = 0, M2 bound holds.
2. De-flood regression guard — re-specified, §11.2.
3. Coverage-preserving proof: M3 no-regression at Landing 1 (blanket), M3-on-demanded-set = 100%
   at Landing 2.
4. No-abandon/no-strand under an edit storm: final surface byte-matches final content; a
   `Superseded` admission is followed by convergence at a strictly-newer generation; op-log shows
   zero entries left unfenced-and-unretried.
5. No-pipe-hold: an interactive request never holds the connection/repair lease for its deadline;
   a concurrent hover on a second document converges within its own ≤ (W+2)·δ + ε bound (true
   claim per review: per-doc awaits overlap; the bound scales with |dirty active docs| × per-op,
   not with N).
6. Closure/breadth correctness: hover through a never-opened imported carrier resolves; find-refs
   across un-opened carriers returns the full set. (Landing 1: via P1.5 for direct imports +
   blanket Coverage for transitive, with the coverage-commit allowance; Landing 2: via P2/breadth
   with coverage OFF.)
7. **Wedge case (CR-3c), §11.3 row W.**
8. Respawn convergence: active re-converges FIRST; replay fenced (G4); ≤ `max_restarts`.
9. No-heartbeat/capacity: all criteria pass on the constrained config with any stats endpoint
   absent and (managed) Warm OFF. Heartbeats are never load control; the §6 sensors are the only
   liveness inputs.

### 11.2 The de-flood regression guard — re-specified (CR-6)

v2 §10.2 ("de-flooding WITHOUT the scheduler must RAISE gate C") is not durably implementable: it
requires the pre-cutover path, which the clean cutover deletes (keeping it = forbidden dual path).
Replaced by:

- **(a) Pre-landing characterization (recorded, temporary):** the §1a matrix (13→27→24→30) plus a
  scripted re-run of baseline / de-flood / retain toggles on the OLD binary, recorded into the
  landing evidence. Not a committed test; it dies with the old binary.
- **(b) Committed post-cutover structural guards:**
  1. **Coverage-role-OFF + scheduler-ON keeps M1 = 0** (op-log: zero active gate-C misses with
     zero Coverage-role wire ops) — Landing 2's real gate: active correctness must never depend on
     the blanket net.
  2. **Structural no-abandon:** with an arbitrarily slow (huge-δ) provider, the op-log shows the
     active obligation re-driven until fenced — the entry never leaves the ledger unconverged, and
     no code path exists that deletes an unconverged entry on a timer (the 1s-abandon class is
     structurally gone; sensor-1 deadlines fail the REQUEST closed, never the OBLIGATION).

### 11.3 Per-landing applicability (CR-3d)

| Criterion | Landing 1 | Landing 2 |
|---|---|---|
| 11.1(1) GREEN: M1 = 0, M2 bound | REQUIRED | holds |
| 11.2(a) old-binary characterization | REQUIRED (pre-landing) | n/a |
| 11.2(b)(1) coverage-OFF gate | — | REQUIRED |
| 11.2(b)(2) structural no-abandon | REQUIRED | holds |
| 11.1(3) M3 no-regression (blanket) | REQUIRED | superseded by M3-on-demand |
| 11.1(4) edit storm | REQUIRED | holds |
| 11.1(5) no-pipe-hold | REQUIRED | holds |
| 11.1(6) closure/breadth | REQUIRED (P1.5 + blanket allowance) | REQUIRED (coverage OFF) |
| 11.1(7) wedge | REQUIRED | holds |
| 11.1(8) respawn active-first, fenced replay | REQUIRED | holds |
| Flood volume drop | forbidden (volume unchanged) | REQUIRED |
| §8 caps enforced (W, doc caps, shared hard cap) | REQUIRED | REQUIRED |

### 11.4 Per-route contract (CR-8; op-log counts unless marked wall-clock-on-seam)

**tsserver — T1–T8:**
- T1 Startup at N=2000 issues 0 interactive-slot opens outside Active ∪ Open ∪ P1.5.
- T2 Opens-before-first-hover ≤ |open| + |P1.5 closure| + ε (ε enumerated: fences + config ops).
- T3 Between the active-bump enqueue and the active fence-ack, ≤ W+1 non-active wire ops appear in
  the provider-consumed log; hover answers ≤ (W+2)·δ + ε wall-clock-on-seam — SAME bound at N=200
  and N=2000.
- T4 Wire-open documents ≤ `TSSERVER_OPEN_DOCS_CAP` at all times; overflow → LRU-retire coverage
  (or the store-publication variant records zero coverage opens); never unbounded.
- T5 Edit storm: final provider surface byte-matches final content; convergence events fire only
  on fence-acked admission; zero abandon paths exercised.
- T6 Wedge (reads-but-won't-answer): every interactive op answers or fail-closes ≤ its deadline;
  detection trips via fenced-sync/query timeouts (§6 sensor 3); ≤ R respawns; active re-converges
  FIRST; a δ-slow-but-answering provider triggers ZERO respawns (progress-aware discrimination).
- T7 No-pipe-hold: concurrent hover on a second document converges within its own ≤ (W+2)·δ + ε.
- T8 (Landing 2) Coverage-OFF keeps M1 = 0, 11.1(6) green, total opens = demand-set size.

**Managed tsgo — T1–T8 plus:**
- G1 Fence semantics: a query issued on the last coverage document immediately after its fence-ack
  succeeds (the ack really attests child-side application).
- G2 (carried by the BD lane-routing when adopted): no query on P before P's fence-ack — the
  cross-lane overtake is the "overlay not found" class (`tsgo/ipc.rs:813-815`).
- G3 ≤ 1 configure op per owner config per generation.
- G4 Respawn replay fenced: the active fence-ack precedes the (W+1)th replay op in the consumed
  log.

**Shared tsgo — full proof set (not inject-count only):**
- S1 Inject-count ≤ `SHARED_INJECT_HARD_CAP` per demand event; over-cap degrades VISIBLY to
  partial closure + per-query subjects (asserted from the op log + the surfaced status).
- S2 Zero Coverage/Warm injections: type-unrepresentable (compile-time) AND runtime op-log zero.
- S3 RED-first starvation/convergence on the relay, edit storm, and editor-tsgo-restart epoch
  replay converging ACTIVE-FIRST (epoch markers per `overlay_core.rs:634-641`); wedge case per T6
  with the relay's fail-closed barrier contract (`relay.rs:361-365`) preserved.
- S4 Editor-latency probe stays within budget during a maximum inject burst (guest never starved).

### 11.5 Execution proof (CI)

All three routes are non-vacuous on CI: each suite proves execution per the repo's "Verification
Must Prove Execution" rule (independently discovered inventory, non-zero executed work, negative
controls that are themselves proven to have applied). Skip-as-pass is forbidden: a route whose
harness is unavailable FAILS the gate, never silently passes. Seam-based suites are the proof;
real-provider runs are additional evidence and never substitute for the deterministic assertions.

## 12. CORRECTNESS-REQUIRED (v3)

1. Build the ActivePriorityScheduler (reconciler-level slot arbitration; active preempts +
   retained; independent of the dormant lanes); delete the 1s sync timeouts.
2. **Build the fence: per-route child-synchronized completion (tracked `updateOpen` response /
   barrier request / existing relay barrier) + the ≤ W unfenced-coverage window (§3). Landing-1
   scope.**
3. One ledger + one single-writer reconciler = sole wire-writer; the COMPLETE §4.2 inventory folds
   in (incl. force_reopen, the three stale closers, configure-paths, replay caches); verbs
   module-private; the API companion is a first-class `content_source.kind`.
4. Preserve the sweep's coverage as a subordinated Coverage role; no flood-volume reduction until
   the Landing-2 gate is proven. De-flood must never raise M1.
5. Retention lives in the ledger re-drive, never a lengthened interactive await; requests await
   convergence events bounded by their own deadline and never hold the connection/repair lease.
6. Truthful convergence attestation: per-document, fence-acked only; delete the unconditional
   `SYNC_DONE` and unconditional `typeProviderSyncComplete`.
7. **The §6 liveness boundary is normative: every reconciler await terminates via exactly one
   named sensor; fence detection feeds hang detection; detection never abandons obligations;
   Superseded re-picks require a strictly-newer generation; convergence events are
   (document, generation, epoch)-scoped.**
8. **Phase-A over-feed bounds exist and are enforced on every route (§8): W, per-route open-doc
   caps, the shared hard cap + visible degrade. A fired Landing-2 fallback is a tracked debt row,
   never a terminal state.**
9. Active slot includes P1.5 import neighbors from Landing 1 (no cross-file-hover regression
   window).
10. Shared tsgo demand-only from day one; inject eligibility type-limited; guest rails unchanged.
11. Existing fail-closed gates unchanged (capture, admit_owned, shadow-safety, BoundProject-only,
    no-poison).
12. No heartbeat as load control; the §6 sensors (which include the existing crash rails) are the
    only liveness inputs; any stats endpoint is observability-only (tests pass with it absent).
13. Structural confinement (sole-writer visibility, exhaustive role→slot match, injectable-role
    type split); no name-keyed scanner guards.
14. Migration order is a correctness constraint (§9); the §11 acceptance contract — including the
    per-route T/G/S sets, the per-landing table, and the seam definition — is the definition of
    done.

## 13. BETTER-DESIGN

Track tsserver responses generally (free acks beyond `updateOpen`); fence batching (one barrier
per K ops within W); Coverage slot onto the real tsgo Background lane once fenced (carries G2);
rate-limited tsserver store publish; lazy respawn replay (re-add coverage on demand); per-
connection feed-pressure gauge on the audit substrate (observability-only); idle-gated coverage
admission (latency nicety); tsgo project-first leases + whole-program check-project; on-demand
breadth-spike machinery; content-bearing updateSnapshot when upstream lands; opt-in managed Warm
(default OFF); contents-cache audit; shared editor-latency guest metric; convergence events onto
the audit substrate.

## 14. Decision gate (blocks implementation)

- [x] Reproduce on the real corpus, both routes — trace shows gate C. (DONE)
- [x] Empirically test the naive fix — de-flood/retain-alone REGRESS (13→27/24/30). (DONE)
- [x] Rescope with the empirical findings (v2). (DONE)
- [x] Adversarial review of v2 → 8 CRs. (DONE)
- [x] v3 closes all 8 CRs (this document).
- [ ] **Product-owner decision to implement (or not).**
- [ ] If yes: commit the hermetic RED reproduction (provider-side seam, §11.0) + the structural
  no-abandon guard FIRST; record the old-binary characterization (§11.2a); then Landing 1 (proven
  per §11.3 column 1), then Landing 2 (gated).

## 15. CR-closure map

| CR | Closed by | How |
|---|---|---|
| CR-1 | §3 (grounded by §1b) | Per-route fence-ack (tracked `updateOpen` response; tsgo barrier request generalizing the in-tree `sync_overlay` precedent, `relay.rs:366-396`; shared already fenced) + the ≤ W unfenced-coverage window; Landing-1 scope (§9, CR-2 in §12); makes M2's N-independent bound true and is the Phase-A anti-over-feed bound. |
| CR-2 | §6 | Six named sensors covering every reconciler await; fence detection-not-abandon feeding `HANG_THRESHOLD`; capacity-park on `Err(Full)`; strictly-newer-generation re-pick; epoch-scoped events; end-to-end wedge chain (§6.2) + anti-respawn-storm (§6.3). |
| CR-3 | §11.0, §11.1(7), §11.3 | Seam at provider-side serial processing (write-delay seam forbidden); assertions as wire-op-log counts, wall-clock only on δ; wedge case (T6/S3); per-landing applicability table. |
| CR-4 | §8 | Quantified per-route bounds: W, `TSSERVER_OPEN_DOCS_CAP` (+ declared store-publication alternative), `MANAGED_OPEN_DOCS_CAP`, `SHARED_INJECT_HARD_CAP` + visible degrade; fired fallback = tracked debt row with owner + resolution gate. |
| CR-5 | §5 (a), §4.2 (b), §4.1 (c) | P1.5 import neighbors in the Active slot from Landing 1 (preserving `lifecycle.rs:561-567` promotion); complete writer inventory with fold-in/exempt dispositions incl. force_reopen, stale closers, configure-paths, diag pulls, replay caches; `CarrierApi` as a first-class `content_source.kind`. |
| CR-6 | §11.2 | Pre-landing old-binary characterization (recorded, temporary) + committed post-cutover guards: coverage-OFF ⇒ M1 = 0, and the structural no-abandon test; no dual path retained. |
| CR-7 | §10 | Three separated metrics M1/M2/M3; M1 seam target = ZERO (exact count); pinned environment, probe, seed, and baseline procedure; M3 makes "coverage preserved" and the Landing-2 gate falsifiable. |
| CR-8 | §11.4, §11.3, §11.5 | Shared route gets the full proof set (S1–S4 incl. RED-first, edit storm, epoch replay active-first, wedge, hard cap + degrade); all three routes non-vacuous with execution proof; skip-as-pass forbidden. |
