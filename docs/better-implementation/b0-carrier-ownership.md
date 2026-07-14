# B0 — Canonical Carrier Ownership + Readiness Transaction: best-implementation design reference

Status: **design reference (deferred)** — not a committed plan step. This document
captures the best long-term design for the carrier ownership + readiness + carrier-sync
layer, as identified by the independent `codex-sol` architecture leg during the B0 confirm.
It exists to inform later hardening/ownership work; it does not authorize or schedule that
work. Editorial framing added by the workstream is marked **[editorial]**; everything else
is a faithful restatement of the `codex-sol` best-design output.

Provenance and source pointers are listed in [§6](#6-provenance).

---

## 1. Context

**What B0 landed (the increment, `ac10607d8`).** B0 is *canonical carrier ownership +
readiness transaction*: a single membership evaluator (`StaticMembershipSpec` /
`ConfiguredMembership` in `crates/verter_workspace/src/membership.rs`), a typed
`ConfiguredOwnerResolution { Unique(id) | Ambiguous(ids) | None }` produced by one
evaluator in `crates/verter_workspace/src/workspace_snapshot.rs` and consumed by the
scanner, the snapshot, and the resolver alike, a resolver mapping (`None → NoProject`,
`Ambiguous → CarrierOwnershipResolution::Ambiguous { candidates, cause }`,
`Unique → ProjectBinding`) in `crates/verter_session/src/external_ts/resolver.rs`,
readiness gated by a `ProviderReadyReceipt`, and a per-source transaction coordinator
(`admit_owned` / `settle` / `advance_barrier`) admitting carrier state.

The layer must make three failures **structurally impossible**:

- **(a)** a genuine tsconfig **overlap** collapsing into the same state as "no project";
- **(b)** the workspace scanner and the live resolver **disagreeing** on which project owns a carrier;
- **(c)** readiness being exposed **before** the carrier is actually publishable (companions published, `projectFileName` registered, project verified).

**Why B0 landed as an increment (land-with-deferral).** The independent confirm rated B0
**CORE-LANDABLE-WITH-DEFERRAL** (RESOLVED: yes): the branch is strictly safer than pre-B0
— ownership collapse is removed on the canonical serve path, admission now rejects several
stale commits, and read-time configured-project selection fails closed. It passes the
correctness/non-regression, not-shallow, and no-stubs bars (BAR 1/2/3). The residuals cause
*missing or degraded service*, not *restored ambiguous ownership or inferred-project
serving*; reverting the coordinator wholesale would reopen already-fixed cases. The standing
directive is to land the correct, non-regressing increment and track the better design as
feed-forward, rather than block landing on a larger rework.

**Why this document exists.** B0 does **not** meet BAR 4 (*best implementation*). The
independent `codex-sol` leg returned `BEST-LONG-TERM-DESIGN: no` with a materially better
design and `REOPEN-RECOMMENDATION: yes`, and the confirm first-hand verified one real
(corner-case) correctness residual it identified. This document itemizes that best design so
the deferred work has a durable design of record. **[editorial]** The increment remains the
authority for what ships today; this is the target the deferred rows in
[`../arch/lsp-workstream-ledger.md`](../arch/lsp-workstream-ledger.md) converge on.

**Tracked residuals referenced below.**

- **P1 — BAR-4 best-design (whole).** The materially-better design as a whole; the reason BAR 4 is not met.
- **P2 — mixed-cycle `None`-collapse.** `crates/verter_workspace/src/workspace_snapshot.rs:160-213` (`configured_owner_resolution_for_file`). A verified corner case (see [§2.1](#21-candidate-preserving-ownership-resolution) / [§2.2](#22-maximal-scc-pruning-over-a-formal-domination-relation)).
- **P2 — F3 divergent selector.** `crates/verter_lsp/src/background_drain.rs:294-305` + `crates/verter_lsp/src/provider_sync.rs:355-367` (a live use of `nearest_config_for_path`).
- **P3 — F2 cold-snapshot serve.** `crates/verter_lsp/src/tsgo/project_binding.rs:169-172` (`PresentSnapshotAuthoritative`).

---

## 2. The best design, itemized

Each item states **Today** (what the B0 increment does), the **Ideal** (the `codex-sol`
target), **Why better**, and the **Residual closed**.

### 2.1 Candidate-preserving ownership resolution

- **Today.** `configured_owner_resolution_for_file` prunes co-claiming configs by pairwise
  filtering (discard solution-style aggregators; discard strict-ancestor candidates and
  referenced non-leaves when a unique deeper leaf exists), collapsing to
  `ConfiguredOwnerResolution::None` when pruning removes everything. Candidate discovery and
  ownership policy are fused in one pruning pass, so a policy edge can erase an overlap that
  was genuinely nonempty.
- **Ideal.** Separate candidate **discovery** from ownership **policy**. Preserve every
  matching configured project in a `ConfiguredOwnerCandidates` set; **never return `None`
  when that set was nonempty.** Either treat every multi-candidate set as `Ambiguous`, or
  expose a distinct `Preferred { owner, all_candidates, basis }`. A reference edge is **not**
  proof that a co-claiming project is merely an aggregator — a genuine solution config with
  `files: []` already claims nothing, and explicit co-membership must remain observable
  unless a separately specified TypeScript-compatible selection policy resolves it.
- **Why better.** Makes failure **(a)** — overlap collapsing to "no project" — *structurally*
  impossible rather than best-effort: the headline B0 invariant becomes an invariant of the
  type, not a property of the pruning order. Preserving the candidate set also retains the
  warm information a later selection policy needs, instead of discarding it at resolution
  time.
- **Residual closed.** **P1 (BAR-4)** and the discovery half of **P2 (mixed-cycle
  `None`-collapse)**.

### 2.2 Maximal-SCC pruning over a formal domination relation

- **Today.** Pruning is pairwise, guarded by a strongly-connected-component check that blocks
  only a **pure reference cycle**. A **mixed** domination cycle defeats it: candidate A is
  dropped by deeper-root B (ancestor rule), B is dropped by a transitive reference to C
  (reference rule), and C is dropped by a reference to A (reference rule) — with an
  **acyclic** reference graph and three or more co-claiming configs, every candidate is
  removed and the result collapses to `None`. This is the exact corner the confirm verified
  first-hand (triple-confirmed: first-hand + `codex-sol` + `codex-terra`).
- **Ideal.** If dominance-based selection is retained, compute **maximal SCCs over a formally
  defined domination relation** and never allow pruning to remove every candidate. Dominance
  becomes a total operation on the candidate set whose fixed point is a nonempty set of
  maximal (undominated) candidates.
- **Why better.** Eliminates the ambiguity-erasing `None`-collapse that B0's headline claims
  to remove but does not structurally enforce. Correctness on a rare-but-real topology
  without falling back to lexical/scan-order winner selection (which B0 correctly rejects).
- **Residual closed.** **P2 (mixed-cycle `None`-collapse, `workspace_snapshot.rs:160-213`).**

### 2.3 Materialized-file-set as cache-only

- **Today.** `ConfiguredMembership::contains` uses **materialized** membership whenever the
  materialized set is nonempty and falls back to **static** matching when it is empty — so an
  empty set is an overloaded sentinel and the materialized set acts as a competing matcher.
  A stale scan can therefore change `matches(path)` and exclude a carrier the static spec
  covers.
- **Ideal.** Make `StaticMembershipSpec` the **unconditional** path-membership authority.
  Replace the empty-set sentinel with an explicit `MaterializationState`; the materialized
  file set may only **accelerate enumeration**, never change `matches(path)`.
- **Why better.** A stale or still-warming scan can no longer exclude newly-created, open, or
  VFS-only carriers — the enumeration cache is a pure performance accelerator, decoupled from
  the correctness of membership. This also removes a source of failure **(b)** on the
  membership surface (static authority is the single truth; the cache cannot disagree with
  it).
- **Residual closed.** **P1 (BAR-4)** — closes the confirm's residual that
  `ConfiguredMembership::contains` contradicts the claimed enumeration-cache-only design.

### 2.4 Remove `nearest_config_for_path` owner inference

- **Today.** `ProjectResolver::nearest_config_for_path` (and legacy
  `ProjectGraph::owner_for_file`) is a divergent, collapsing, non-reference-pruned selector
  used in a **live** reconciliation-skip path (`background_drain.rs:294-305`,
  `provider_sync.rs:355-367`). `WorkspaceSnapshot` and `ProjectResolver` therefore apply
  **different** ownership pruning, and a production path still infers an owner from a bare
  path. The canonical external-TS serve path re-resolves through
  `configured_owner_resolution` and fails closed, so the consequence today is stale
  provider-path **sync** config (degraded), not a wrong serve.
- **Ideal.** Remove carrier-owner inference from `nearest_config_for_path` and
  `ProjectGraph::owner_for_file` entirely. Project-sensitive resolution should require a
  `ProjectId` / `ProjectBinding`; owner-independent resolution (nearest config, for
  non-ownership purposes) should be a **separate** API. Represent bootstrap-versus-ready as a
  typed **published-view** variant rather than a caller-supplied boolean.
- **Why better.** Restores "one evaluator consumed by all" on the *sync* path too, closing
  failure **(b)** (scanner/resolver disagreement) where it currently survives. A path can no
  longer be turned into an owner without a binding, and the bootstrap/ready distinction stops
  riding on an easily-mis-set boolean.
- **Residual closed.** **P2 (F3 divergent selector, `background_drain.rs:294-305` +
  `provider_sync.rs:355-367`).** **[editorial]** As a *design* item this belongs to the
  ownership-model rework (R-OWN); as a *live divergent-selector* removal it is tracked in the
  carrier-sync-concurrency hardening block (R-HARDEN, F3).

### 2.5 Separate `CarrierServeBlocker` from `Ambiguous`

- **Today.** Companion collisions and same-stem rune conflicts are represented by folding the
  carrier into the `Ambiguous` ownership state, conflating *serving eligibility* with
  *ownership*: a uniquely-owned carrier that merely cannot be served right now is
  indistinguishable from a genuinely ambiguous one.
- **Ideal.** Keep the ownership taxonomy pure — `Pending | None | Unique | Ambiguous` — and
  represent companion collisions and rune conflicts as a separate `CarrierServeBlocker`, so
  ownership, serving eligibility, and readiness remain three independent axes.
- **Why better.** Correctness and observability: a serve-blocked-but-uniquely-owned carrier
  keeps its true ownership and can recover cleanly once the blocker clears, instead of being
  reported as an ownership ambiguity it does not have. Diagnostics and readiness reason over
  the correct axis.
- **Residual closed.** **P1 (BAR-4)** — taxonomy purity / clean ownership–serving–readiness
  separation.

### 2.6 Per-source async transaction actor

- **Today.** Readiness admission is split across independent synchronization domains — a
  reconciler, a barrier, and a synchronous admission gate (the B0-added `admit_owned` /
  `settle` / `advance_barrier` coordinator, which is explicitly partial hardening). A
  synchronous gate cannot fence detached state across provider `await`s: the confirm's
  round-5 findings record that `convert_to_unresolved` clones, mutates, awaits, then
  raw-inserts (detached mutation across `await`); that inserting into a one-shot-drained set
  is not requeue semantics (an ineffective retry after refusal); and that the admission-class
  guard is static-enforcement debt whose call sites must remain review-audited. Each of these
  is fail-closed and non-regressing today, but not structurally prevented.
- **Ideal.** Replace the split reconciler/barrier/admission machinery with **one per-source
  async transaction actor** driven by a monotonic transaction sequence. The actor captures
  the ownership-snapshot identity and the authoritative source revision, compiles,
  stages/publishes companions, registers the exact `projectFileName`, **verifies the
  companions are members of that configured Program**, and atomically installs ready state —
  **revalidating after every `await` and compensating on supersession**.
- **Why better.** Fences the detached-mutate-across-`await` pattern by construction, gives
  real live-retry (requeue) semantics for a superseded source, and guarantees ready state is
  never installed torn or stale. This is the structural close of failure **(c)** (readiness
  before publishable) on the admission side, and it turns findings 1–4's review-audited
  invariants into enforced ones.
- **Residual closed.** **P1 (BAR-4)** and the round-5 findings 1–4 tracked under the
  carrier-sync-concurrency hardening block (R-HARDEN).

### 2.7 `VerifiedProjectLease` replacing the unverified `ProviderReadyReceipt`

- **Today.** Readiness is gated by a `ProviderReadyReceipt { source_revision,
  project_generation, binding, companions, provider_generation }` that is **not proof** of
  publishability: tsserver registration explicitly performs **no** project verification,
  `ProviderGeneration` merely repeats the ownership generation, `source_revision` means
  different things across engines, and readiness can be minted after a contentless open
  without verifying that the configured project exists and contains the companion. The
  `PresentSnapshotAuthoritative` cold-snapshot serve (`project_binding.rs:169-172`) is a
  symptom: an OWNED-native baseline is served from a present-but-cold snapshot during
  bootstrap (transient/degraded; the external-TS result path itself stays fail-closed).
- **Ideal.** Have backends return a **non-cloneable `VerifiedProjectLease`** carrying the
  real provider-session generation, project identity, manifest/snapshot generation, and
  **verified companion fingerprints**. Mint readiness **only** from that lease.
- **Why better.** Readiness becomes minted from a verified witness that the configured
  Program actually contains the companions at a known provider-session generation — the
  structural close of failure **(c)** on the *verification* side, capturing provider
  restart/session identity that the current receipt lacks and removing the cross-engine
  `source_revision` ambiguity. The `PresentSnapshotAuthoritative` cold serve resolves because
  a cold snapshot can no longer mint a lease.
- **Residual closed.** **P1 (BAR-4)** and **P3 (F2 cold-snapshot serve,
  `project_binding.rs:169-172`).**

---

## 3. Residual design risks (codex-sol)

Faithful restatement of the `codex-sol` `RESIDUAL-DESIGN-RISKS` block — the concrete risks
that remain in the B0 increment until the design above lands:

- `configured_owner_resolution_for_file` can still collapse an overlap to `None` (the mixed
  domination cycle: ancestor A dropped by deeper B, B dropped by reference to C, C dropped by
  reference to A, even when the reference graph itself is acyclic).
- `ConfiguredMembership::contains` uses materialized membership whenever the set is nonempty
  but static matching when empty, contradicting the claimed enumeration-cache-only design.
- `WorkspaceSnapshot` and `ProjectResolver` apply different ownership pruning, while
  production paths still call `nearest_config_for_path`.
- Tsserver readiness is minted after a contentless open but without verifying that the
  configured project exists and contains the companion.
- Receipt `source_revision` has different meanings across engines, and provider
  restart/session identity is absent.
- The source transaction remains split across independent synchronization domains; the code
  itself records detached-mutation and same-source concurrency gaps.
- Environment dimensions are fetched through a separate live host lookup rather than from the
  same captured ownership snapshot / project id, permitting torn binding provenance.
- Framework descriptors and registry-derived extensions compose well for N frameworks, but
  per-adapter required-companion completeness and TypeScript membership parity still need
  oracle tests.

**Reopen recommendation (codex-sol):** `yes` — rework membership authority,
candidate-preserving ownership selection, and the per-source verified readiness transaction
before treating the three failures as structurally impossible.

---

## 4. Tracked residual → design-item → ledger-row map

| Residual | Site | Design item | Ledger row |
|---|---|---|---|
| **P1** — BAR-4 best-design (whole) | — | §2.1–§2.7 | R-OWN + R-HARDEN |
| **P2** — mixed-cycle `None`-collapse | `workspace_snapshot.rs:160-213` | §2.1, §2.2 | R-OWN |
| **P2** — F3 divergent selector | `background_drain.rs:294-305` + `provider_sync.rs:355-367` | §2.4 | R-HARDEN (F3); design in R-OWN |
| **P3** — F2 cold-snapshot serve | `project_binding.rs:169-172` | §2.7 | R-HARDEN (F2 envelope) + R-F2 |
| round-5 findings 1–4 | carrier-sync admission | §2.6 | R-HARDEN |

**[editorial]** The ownership-model items (§2.1, §2.2, §2.3, §2.5) land under **R-OWN**; the
concurrency/readiness-transaction items (§2.6, §2.4/F3) land under **R-HARDEN**; the
cold-snapshot serve (§2.7/F2) is covered by the R-HARDEN carrier-sync block (whose acceptance
bar lists F2) and tracked narrowly as **R-F2**. `VerifiedProjectLease` (§2.7) underpins both
the readiness transaction (R-HARDEN) and the R-F2 cold-serve close.

---

## 5. What is *not* claimed

**[editorial]** To keep this reference honest about the increment:

- B0 as landed is correct, additive, and non-regressing — strictly safer than pre-B0. This
  document does not describe a defect that ships broken today; the residuals are
  missing/degraded service or verified corner cases, all fail-closed.
- Nothing here is a committed plan step. The design lands only through the tracked ledger
  rows, on the standing serial-implementation cadence, not as an implied follow-up to B0.
- The itemization is faithful to `codex-sol`; where the workstream added grouping,
  ledger-row mapping, or framing, it is marked **[editorial]**.

---

## 6. Provenance

- **Primary source.** `codex-sol` (`gpt-5.6-sol`, reasoning effort `xhigh`) BAR-4 best-design
  output — the independent architecture leg of the B0 confirm. Question posed under the
  CODEX-ARCHITECT MANDATE: *is the landed design the best long-term architecture for
  canonical carrier ownership + a readiness transaction, or does a materially better design
  exist?* Verdict: `BEST-LONG-TERM-DESIGN: no`; `REOPEN-RECOMMENDATION: yes`.
- **Confirm context.** The independent B0 confirm report (VERDICT: ISSUES-FOUND — a
  "best-impl-bar-not-met + verified corner-case residual" verdict, **not** a fail-open/broken
  verdict): BAR 1/2/3 PASS, BAR 4 NOT MET, with the P2 mixed-cycle `None`-collapse verified
  first-hand and F2/F3 withdrawn as fail-closed/deferred.
- **Land-with-deferral ruling.** Independent landability consult: **CORE-LANDABLE-WITH-
  DEFERRAL** (RESOLVED: yes) — land the increment now, do not enter another point-fix round,
  and open one dedicated carrier-sync-concurrency block covering async
  serialization/versioning, live-retry scheduling, guard enforcement, and F2, gated on
  adversarial race tests at the block level.
- **Feed-forward tracking.** [`../arch/lsp-workstream-ledger.md`](../arch/lsp-workstream-ledger.md)
  — rows **R-HARDEN**, **R-OWN**, **R-F2**.
