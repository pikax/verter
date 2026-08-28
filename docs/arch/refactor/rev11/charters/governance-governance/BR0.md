<!-- unified-charter-v2
id=BR0
name=Post-L4 successor product promotion
phase=governance
train=governance.governance
product=governance
kind=genesis
semantic_role=delivery
class=successor
predecessors=
conditional_predecessors=
owner=governance.governance:static DAG authority plus immutable receipts and round-bound evidence
conflict_domains=program_authority
resource_class=docs-light
review_profile=semantic-3
gate_profile=docs-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=S
dispatchable=true
optional=false
release_gating=external
source_refs=source:successor-expansion.md:L752,source:orchestration-findings.md:L1422
external_requirements=maintainer_rev11_repair_freeze_lift,maintainer_successor_genesis
activation_gate=ORC0
charter=charters/governance-governance/BR0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# BR0 — Post-L4 successor product promotion

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Post-L4 successor product promotion. The current owner is **mutable orchestration ledger and mixed authority/evidence**. The final and sole owner is **static DAG authority plus immutable receipts and ephemeral leases**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `docs/arch/refactor/rev11`.
- Named API/data boundaries: `activation receipt`, `acceptance receipt`, `lease`, `dispatch packet`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **Direct DAG predecessors:** none. This is a source-canonical entry; its external requirements remain mandatory and are not predecessor substitutes.
- **External custody maintainer_rev11_repair_freeze_lift:** require the exact immutable static slot at dispatch and the finalized-candidate-bound authorization before evidence or acceptance.
- **External custody maintainer_successor_genesis:** require the exact immutable static slot at dispatch and the finalized-candidate-bound authorization before evidence or acceptance.

## Source-specific scope

- Deliver exactly “Post-L4 successor product promotion” as the independently acceptable boundary; no neighboring authority is included.
- `BR0` is unavailable until external custody verifies both exact slots: `maintainer_rev11_repair_freeze_lift` and the distinct post-L4 `maintainer_successor_genesis`. No earlier, private, or locally minted authorization is reusable.
- BR0 is a shared prerequisite, not a join of product completion: each product terminal remains independently promotable once its own predecessors and BR0 are accepted. One product's receipt is never a predecessor of another product merely to enforce promotion policy.
- Negative controls must prove no `release_gating = "product"` node and no complete non-canary expansion node is READY or ACCEPTED before BR0, while two otherwise-independent product terminals can become READY concurrently after BR0.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **BR0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **BR0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **BR0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **BR0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `docs/arch/refactor/rev11/fixtures`.

## Deletions and forbidden designs

- Delete or structurally reject: **mutable READY ledger**.
- Delete or structurally reject: **resource-capacity DAG edge**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `node docs/arch/refactor/rev11/tools/validate-negative-controls.mjs`
2. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial`, `conformance`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 2/2 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `targeted` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 2 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L752`
- `source:orchestration-findings.md:L1422`

## Reconciled source-plan contract

**Intent:** create the only legal, immutable basis for the successor through two machine-validated external authorities; `BR0` cannot exist or become READY under only the repair-scoped freeze lift.
**Predecessors:** none inside this proposal. Receipt A names the maintainer’s Rev11 repair-scoped freeze lift and accepted amendment. Receipt B is a distinct post-L4 maintainer decision authorizing creation, ratification, and dispatch of the successor genesis block plus the named successor scope. The genesis record also names accepted TCM0–TCM4, SourceUnitId repair, K3/L1/L2 revalidation, L4, final commit/tree, and clean-state identities.
**Subblocks:** (1) define `successor-genesis.toml` with separate repair and successor-authority receipts; (2) validate amendment/TCM/SourceUnitId/ADR/UTF-8 observation-identity receipts and live edges; (3) verify `TCM4→K3→L1→L2→L4` plus every identity-repair invalidation/revalidation edge; (4) bind activation/deletion, backend, coordinate, performance, charter, ADR, and ruling digests; (5) after L4, capture successor authorization, re-hash the final commit/tree, and publish the authority index; (6) make the ledger reject creation/READY when either authority or any field/digest is absent, overbroad, or stale.
**Acceptance:** the validator reconstructs every cited identity from the accepted tree, proves TCM/identity repairs upstream of L4, distinguishes the two maintainer decisions, and records exact amendment invalidation closure; no blocking/open claim is presented as accepted.
**Forbidden:** using repair authority to dispatch successor work, treating a stored ruling as ratified, manually setting `BR0` READY, or using a worktree/branch other than the accepted integration identity.
**Deletion/abort:** supersede every old proposal premise tied to `323bc7f…`; abort if the freeze is not explicitly lifted for this amendment or Rev11 reaches L4 without activated-TCM soak/performance evidence.

## Collapsed non-authoritative subblock disposition

The recovery candidate mechanically split this source-owned atomic node into the following labels: `BR0P`. They have no separate dispatch, lease, receipt, migration manifest, deletion ownership, or review standing. Their useful source-described concerns are internal RED/GREEN checklist items of **BR0**; BR0 alone owns the complete migration population, exactly one final deletion/cutover, and atomic acceptance. Any quoted “suggested subblock” wording in transferred source text is non-authoritative planning context.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L752-AF4390650776

- Kind: `context`
- Source: `successor-expansion.md:752-752`
- Applicability: `BR0`
- Exact text SHA-256: `af439065077685814b7b84c2b5b799b867da289b3acc9f31a38074fd7a49730e`

~~~~markdown
### `BR0.md` — Accepted Rev11/TCM successor handoff
~~~~

### SRC-EXP-L754-2541E7577AE5

- Kind: `forbidden`
- Source: `successor-expansion.md:754-759`
- Applicability: `BR0`
- Exact text SHA-256: `2541e7577ae5c9fdb20cf649f32536832e148caa73ed64c57f4b504f4c9b5093`

~~~~markdown
**Intent:** create the only legal, immutable basis for the successor through two machine-validated external authorities; `BR0` cannot exist or become READY under only the repair-scoped freeze lift.
**Predecessors:** none inside this proposal. Receipt A names the maintainer’s Rev11 repair-scoped freeze lift and accepted amendment. Receipt B is a distinct post-L4 maintainer decision authorizing creation, ratification, and dispatch of the successor genesis block plus the named successor scope. The genesis record also names accepted TCM0–TCM4, SourceUnitId repair, K3/L1/L2 revalidation, L4, final commit/tree, and clean-state identities.
**Subblocks:** (1) define `successor-genesis.toml` with separate repair and successor-authority receipts; (2) validate amendment/TCM/SourceUnitId/ADR/UTF-8 observation-identity receipts and live edges; (3) verify `TCM4→K3→L1→L2→L4` plus every identity-repair invalidation/revalidation edge; (4) bind activation/deletion, backend, coordinate, performance, charter, ADR, and ruling digests; (5) after L4, capture successor authorization, re-hash the final commit/tree, and publish the authority index; (6) make the ledger reject creation/READY when either authority or any field/digest is absent, overbroad, or stale.
**Acceptance:** the validator reconstructs every cited identity from the accepted tree, proves TCM/identity repairs upstream of L4, distinguishes the two maintainer decisions, and records exact amendment invalidation closure; no blocking/open claim is presented as accepted.
**Forbidden:** using repair authority to dispatch successor work, treating a stored ruling as ratified, manually setting `BR0` READY, or using a worktree/branch other than the accepted integration identity.
**Deletion/abort:** supersede every old proposal premise tied to `323bc7f…`; abort if the freeze is not explicitly lifted for this amendment or Rev11 reaches L4 without activated-TCM soak/performance evidence.
~~~~

### SRC-LEGACY-TRANSFER-A57450E4B7D4

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:460-465`
- Applicability: `BR0`
- Exact text SHA-256: `18f448b4e290684fb32916db700b07a407f4cfd126b73c9b9aeb101a40b020b7`

~~~~markdown
### LEGACY-TRANSFER-A57450E4B7D4

- Original path: `docs/arch/release-state.md`; Git blob: `a57450e4b7d4125ad0f11a1cf76d925022bcca23`; exact source SHA-256: `1187d2acf0a99b0447227f8e05c863a3e2630333ac7f1f9c2b18f7430b12a3aa`.
- Exact retained source: `sources/legacy-architecture-transfers/release-state.md`.
- Applicable authority: `BR0`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~
