# Provider document-feeding architecture — design (PROVISIONAL, post-release review)

> **Status: PROVISIONAL — not approved for implementation.** The root cause below is a strongly-
> supported HYPOTHESIS, not yet empirically proven against the real failing corpus. Gate before any
> implementation: a real-world before/after proof (reproduce on the real corpus + a throwaway dirty
> fix that actually restores intellisense). If the dirty fix does not fix the real corpus, this
> design is void and the cause is re-investigated. After that proof, the design is RE-SENT to the
> architect to rescope with the empirical findings, then the decision to implement (or not) is the
> product owner's. Saved here for post-release review and possible rescope.
>
> Confidentiality: the motivating project is a private third-party monorepo — never named here;
> referred to structurally ("a real Vue monorepo"). Committed tests must be hermetic + synthetic;
> the real-corpus reproduction is scratch/uncommitted (cannot be shared).

## 1. Reasons for the change (why this exists)

A real Vue monorepo produced **no TypeScript intellisense** on the file the user was actively
editing, even after the configured-project-selection fix made that package's files sync. Observed
on **both** managed tsgo AND tsserver.

Root cause (hypothesis, pending the empirical gate above), confirmed from source + the user's real
LSP log:
- The active document's IDE companion (`.vue.tsx`) reaches `sync_coordinator: SYNC_DONE` and the
  session settles, yet hover returns `no ide_context` — the companion **never actually reached the
  TypeScript provider**, while dozens of OTHER components' companions (opened by the background
  codebase sweep) did.
- **Priority inversion / starvation:** the background sweep floods the single provider connection on
  the same Interactive lane as real editor opens, with no scheduler arbitrating. The active-doc
  open is wrapped in a **1-second timeout** (`sync_orchestration.rs:1186-1200`) that, under sweep
  saturation, **abandons the companion** and only re-flags it — it never commits.
- **Stranding:** a commit superseded under load requeues into a set whose only drain runs **once per
  init generation** (`background_init.rs:360`) — a documented one-shot-drain limitation
  (`carrier_sync.rs:939-942`), so the active companion is never re-established for the session.
- `SYNC_DONE` is logged unconditionally (`sync_coordinator.rs:482`) — it attests "the pass ran," not
  "the companion is live."
- It is **scale-dependent** (a small synthetic fixture serves; the real project floods), which is
  why it "works, then regresses" and recurred for a long time — the tests never exercised the flood.

Why the sweep exists (historical, ruled on): the "sweep every companion into the provider" model was
a deliberate bet on tsgo's capacity to hold very large programs. The bet's premise about tsgo was
right; its **topology** was wrong — it conflated engine program-*capacity* with the Interactive
lane's *scheduling headroom*. Both engines starved. Keep the payoff, move breadth to the right layer.

## 2. The design (Desired-State Feeding) — the architect's recommendation

**Verter stops narrating file events at the engine and converges the engine onto a declared document
set.** Verter sweeps/indexes in its OWN substrate (resolver, cache, shallow inventory, real-TSX) and
holds ALL THREE providers to the **active/demand set only** (the product owner's "sweep in Verter,
hold the providers" option — adopted). Per provider connection: one `ProviderDocLedger` (identity +
role + generation, not bytes) + one single-writer `ProviderDocReconciler` (the SOLE wire-writer); the
five current wire-writers all become ledger writers. Sync verbs become module-private (structural
guard).

Roles → the existing but sweep-unused priority lanes:
- **P0 ActiveDoc** (focused companion) — Interactive; a **retained obligation** (no 1s abandon, no
  one-shot drain; preempts).
- **P1 OpenSet** (other editor-open carriers + live buffers) — Interactive; small.
- **P2 DemandClosure** (the carrier-import closure of P0/P1 computed by **Verter's own resolver** +
  per-query subjects + on-demand breadth batches) — Normal/Background; exact, cancellable.
- **Never fed:** real disk `.ts/.tsx/.js` not open, node_modules contents, build output — the
  engine's project system / plugin store owns them.

The one correction to "just feed demand and let the project system load the rest": Verter's tsgo
companions are **off-disk** overlays the engine cannot pull from disk, so the demand set must be
**closure-precise** (Verter pushes exactly the import closure), else de-flooding trades the
starvation bug for a cross-file regression. This is Verter's edge — pushing precisely what the query
needs vs. waiting for the engine to discover it.

Three provider models (shared substrate, forked policy): **tsserver** — membership via the plugin
store (already correct), only P0/P1 wire-open, P2 via store publications; **managed tsgo** — lease
every tsconfig once (`updateSnapshot({openProjects})`) so the engine owns the whole real-file
program, P0/P1/P2 didOpen by lane, on-demand breadth *spikes* (find-refs/rename/check-project via
Verter's reverse-dependency graph, then LRU-retire) instead of a standing flood; **shared tsgo** —
guest: inject P0∪P1∪P2 of the current demand only (structurally type-limited so sweep/warm output is
unrepresentable as an inject candidate), repo-scale ops route to the managed engine, all citizenship
rails unchanged.

**No heartbeat** (ruled structural): crash detection already exists (B12); Interactive load is
O(open set + closure) — machine-independent; breadth is self-clocked (serial, ack-paced). A polled
stats endpoint is observability-only, never a feed-control input.

## 3. What breaks under demand-only, and how Verter recovers it

Nothing needs a standing broad provider warm. Hover/def/completion/signature/semantic-tokens/inlay/
active-diagnostics/cross-file-diagnostics-among-open = P0 + P2 closure. Workspace symbols = Verter's
shallow inventory (native, no provider). Auto-import = tsserver store / tsgo Verter-side export index.
**The one genuine broad case — find-all-references / rename across un-opened carriers** — is a
bounded on-demand injection spike over Verter's reverse-dependency candidate set (provider precision,
LRU retention), NOT a standing warm. Tradeoff: first-use latency of breadth features (mitigated by
exact candidates + tsgo speed + LRU), observable via the corpus gate, with an opt-in default-OFF warm
as the escape lever. The Verter-side sweep (`WorkspaceIndexer`) survives — it powers these substrate
answers + the tsserver store; it just stops being a provider feed.

## 4. CORRECTNESS-REQUIRED (if implemented)
1. Demand-driven on all three routes; the sweep issues zero provider opens; real disk TS / node_modules never fed.
2. Closure-precise P2 from Verter's resolver (de-flood without it = cross-file regression).
3. One ledger + one single-writer reconciler = the sole wire-writer; the five current writers become ledger writers; verbs module-private (structural).
4. Retained-obligation active-doc sync: delete the 1s timeouts; tracked in-flight + per-path serialization; steady-state convergence replaces the pending set + one-shot drain.
5. Total role→lane mapping; background work structurally cannot ride the Interactive lane.
6. Shared: inject eligibility type-limited to demand roles; `inject_all_dirty` → demand-scoped; guest rails preserved.
7. Truthful attestation: convergence events only on receipt-admitted commit, per document.
8. Existing fail-closed gates unchanged (capture / receipt / shadow-safety / BoundProject-only / no-poison).
9. Self-clocked breadth; no hardcoded flood constant; no heartbeat as load control.
10. Breadth features recovered (substrate + bounded on-demand injection); one such feature in the acceptance suite.
11. One-clean-cutover migration with the deletion list in the same change; no dual feeding path survives.
12. The acceptance contract (§6) is the definition of done.

## 5. BETTER-DESIGN (follow-ups, not gates)
Project-first tsgo leases; content-bearing `updateSnapshot` ingress when upstream lands it; on-demand
breadth-spike machinery; opt-in idle warm (default OFF, tests pass with it off); polled stats endpoint
(observability only); tsserver `updateOpen` batching; rate-limited store-publish; contents-cache
audit; LRU cap on open docs; shared editor-latency guest metric; `configure_paths` dedupe; convergence
events to the audit substrate.

## 6. Acceptance / test contract (hermetic committed + real-corpus scratch)
Committed tests are HERMETIC + synthetic (a parameterized corpus generator, N ∈ {200,1000,2000},
realistic component-import trees, + a CONSTRAINED-resource config with injected per-op provider
latency so the flood reproduces deterministically on any machine). The **committed reproduction must
fail RED because of the active-doc-open TIMEOUT/starvation now, and pass after the proper fix.** The
real-corpus reproduction (against the private monorepo) is the empirical PROOF and stays
scratch/uncommitted.
1. RED-first saturation repro (deterministic seam): init→sweep→open one carrier→hover ⇒ capture=None (gate C) today; GREEN target: active doc converges, hover answers within a bound INDEPENDENT of N; opens-before-first-hover ≤ |open set|+|closure|+ε; zero Interactive ops from background work.
2. No-flood negative: startup at N=2000 issues no O(N) Interactive opens per route.
3. Supersession/strand regression: edit storm under congestion → converged surface matches final content, no re-init; convergence event fires only on admit.
4. Closure+breadth correctness: hover through a never-opened imported carrier resolves; find-refs across un-opened carriers via the substrate spike returns the full set.
5. Three routes non-vacuous (CI, real providers): each runs 1–4; shared asserts inject-count ≤ demand bound + editor-latency probe within budget.
6. Respawn convergence: kill engine mid-session → active doc re-converges FIRST (order-asserted op log).
7. Capacity/no-heartbeat: all pass on the constrained config with the stats endpoint disabled and (managed) warm OFF; the interactive latency bound must not vary with machine class.

## 7. Open validation gate (blocks implementation)
- [ ] Reproduce the bug on the real corpus with real providers (both tsgo + tsserver), trace shows gate C.
- [ ] A throwaway dirty fix (de-flood / active-priority) actually restores intellisense on the real corpus (before/after captured). YES ⇒ hypothesis proven. NO ⇒ this design is void; re-investigate.
- [ ] Re-send to the architect to rescope this design with the empirical findings.
- [ ] Product-owner decision to implement (or not).
