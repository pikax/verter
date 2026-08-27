<!-- unified-charter-v2
id=CEJ0
name=Vue/Svelte Custom Element interoperability soak join
phase=expansion
train=expansion.custom-elements
product=custom_elements
kind=soak
semantic_role=delivery
class=successor
predecessors=CEC0
conditional_predecessors=
owner=expansion.custom-elements:standards model plus framework-specific producer/consumer adapters
conflict_domains=customelementdeclaration,vue_product,svelte_product
resource_class=rust-mixed
review_profile=semantic-3
gate_profile=targeted-domain
size=M
dispatchable=true
optional=false
release_gating=non_release
source_refs=source:successor-expansion.md:L1213
external_requirements=
activation_gate=ORC0
charter=charters/expansion-custom-elements/CEJ0.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# CEJ0 — Vue/Svelte Custom Element interoperability soak join

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Vue/Svelte Custom Element interoperability soak join. The current owner is **shared legacy Web Component schema/registry**. The final and sole owner is **standards model plus framework-specific producer/consumer adapters**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_protocol/src`, `crates/verter_session/src`.
- Named API/data boundaries: `CustomElementDeclaration`, `CustomElementRegistration`, `CemModule`, `FrameworkCeAdapter`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **CEC0:** exact current receipt ID and digest for “Shared legacy Web Component schema/registry cutover”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** continuously falsify standards interoperability across the two existing framework producers without coupling either release to the other.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **legacy shared CE schema**, **unqualified global registry** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

- **CEJ0-AC1 — sole-owner proof:** add `cej0_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **CEJ0-AC2 — positive contract:** add `cej0_publishes_exact_customelementdeclaration`; assert exact identities, provenance, completeness, and deterministic ordering.
- **CEJ0-AC3 — incremental equivalence:** add `cej0_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **CEJ0-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_language/tests`, `crates/verter_protocol/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **legacy shared CE schema**.
- Delete or structurally reject: **unqualified global registry**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance is 0.0% unless an owning-authority amendment supplies exact replacement thresholds; after warmup, retained bytes may not increase across 100 identical requests.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_language -p verter_protocol -p verter_session`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `semantic-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three named independent reviewers with assigned lenses, model `gpt-5.6-sol`, effort `ultra`, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L1213`

## Reconciled source-plan contract

**Intent:** continuously falsify standards interoperability across the two existing framework producers without coupling either release to the other.
**Predecessors:** `CEC0`.
**Subblocks:** (1) same-tag/different-registry fixtures; (2) Vue producer consumed by Svelte/HTML; (3) Svelte producer consumed by Vue/HTML; (4) props/attrs/events/slots/CSS/CEM differential; (5) incremental/stale/package ambiguity and performance soak; (6) publish findings to exact owners.
**Acceptance:** both independently accepted terminals interoperate through HWC3-produced facts conforming to `CEF0`, with no framework knowledge in standards implementation; a soak failure creates a non-invalidating follow-up/version proposal by default and does not revoke a released terminal.
**Forbidden:** implementation fixes in the join, release serialization, shared Vue/Svelte special cases, or runtime-reachability claims.
**Deletion/abort:** delete nothing; reopening an accepted owner requires a separate maintainer impact decision with mechanically computed affected receipts; otherwise findings block only future stabilization that opts into the follow-up.

## 11. Sequential architecture falsification slices

These blocks are deliberately thin and initially unshipped. Each proves parse/extract, activation, exact authored maps, one TypeInfo contribution, one type-aware IDE request, one safe lint/action, formatter-view/map feasibility through the shared `FMT1` private harness, one `PUB0` surface-neutral path, zero-work behavior, and a vertical-specific counterexample. Proof code cannot register public LSP/CLI capabilities or survive as a second product authority. They do not imply “full support.”

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1213-948B2054540E

- Kind: `context`
- Source: `successor-expansion.md:1213-1213`
- Applicability: `CEJ0`
- Exact text SHA-256: `948b2054540e753fa38b1d21aaf8d392acdd98485bdac46f20263ee6b6cab3e8`

~~~~markdown
### `CEJ0.md` — Vue/Svelte Custom Element interoperability soak join
~~~~

### SRC-EXP-L1215-7627A3F60437

- Kind: `forbidden`
- Source: `successor-expansion.md:1215-1220`
- Applicability: `CEJ0`
- Exact text SHA-256: `7627a3f6043726a845da9dbd465758950923626019f1016adafabca98216412c`

~~~~markdown
**Intent:** continuously falsify standards interoperability across the two existing framework producers without coupling either release to the other.
**Predecessors:** `CEC0`.
**Subblocks:** (1) same-tag/different-registry fixtures; (2) Vue producer consumed by Svelte/HTML; (3) Svelte producer consumed by Vue/HTML; (4) props/attrs/events/slots/CSS/CEM differential; (5) incremental/stale/package ambiguity and performance soak; (6) publish findings to exact owners.
**Acceptance:** both independently accepted terminals interoperate through HWC3-produced facts conforming to `CEF0`, with no framework knowledge in standards implementation; a soak failure creates a non-invalidating follow-up/version proposal by default and does not revoke a released terminal.
**Forbidden:** implementation fixes in the join, release serialization, shared Vue/Svelte special cases, or runtime-reachability claims.
**Deletion/abort:** delete nothing; reopening an accepted owner requires a separate maintainer impact decision with mechanically computed affected receipts; otherwise findings block only future stabilization that opts into the follow-up.
~~~~
