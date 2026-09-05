<!-- unified-charter-v2
id=CVO4
name=Owner-driven probe promotion join
phase=compiler
train=compiler.validation-observability
product=validation_observability
kind=convergence
semantic_role=convergence
class=compiler
predecessors=CVO1,CVO1S,CVO2,CVO3,CPER0,CPER2,VCP7,SCP7
owner=compiler.validation-observability:test-only validation and observability lane
conflict_domains=validation_observability,performance_evidence
resource_class=rust-mixed
review_profile=semantic-3
gate_profile=targeted-domain
implementation_effort_min=low
implementation_effort_default=medium
review_effort_min=medium
review_effort_default=medium
verification_effort_min=medium
verification_effort_default=medium
confirmation_effort_min=medium
confirmation_effort_default=medium
size=S
dispatchable=true
optional=false
release_gating=non_release
external_requirements=
charter=charters/compiler-validation-observability/CVO4.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CVO4 — Owner-driven probe promotion join

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

An explicit, selective review join over accumulated canaries, known failures, skips, and benchmark observations once the owning DAG contracts are available, producing promotion decisions only through the owning authorities. The current owner is **unreviewed accumulated probe and observation state**. The final and sole owner is **the owner-driven promotion join record**. This charter accepts one boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: none.
- Named API/data boundaries: the promotion join record `crates/verter_validation_probe/join/<date>.toml` — per probe, its current state, candidate transition (`skip -> canary`, `canary -> known-fail`, `known-fail -> gate`, `canary -> gate`) or `deferred`, deciding authority (`Authority` plus the contract section relied on; never a roadmap identifier), and rationale; the join validator `crates/verter_validation_probe/src/join.rs` (`JoinRecord::validate(manifest, observations)`), which proves total inventory — every manifest entry from CVO1/CVO1S and every `ObservationArtifact` row id appears exactly once in the record — that every non-deferred entry cites an authority and section, and that every transition is one of the four allowed; `validate-probe-authorities.mjs` (CVO0) then proves each cited authority's owning node is implemented and covers the probe's framework and dimension, so a promotion to `gate` cannot rest on an unimplemented or wrong-framework authority; updated probe-state manifest entries.
- Mutation boundary: the join record, the validator, and manifest entries; production LOC is zero.

## Exact predecessor contracts

- **CVO1:** implemented ledger row for "Pinned vue-benchmarks external workload probe lane"; supplies the accumulated probe states. Ledger presence alone satisfies the predecessor. Its locator metadata remains non-authoritative.
- **CVO1S:** implemented ledger row for "Pinned svelte-benchmarks external workload probe lane"; supplies the accumulated Svelte probe states. Ledger presence alone satisfies the predecessor. Its locator metadata remains non-authoritative.
- **CVO2:** implemented ledger row for "Non-gating CI benchmark observation artifacts"; supplies correctness-labeled observations. Ledger presence alone satisfies the predecessor. Its locator metadata remains non-authoritative.
- **CVO3:** implemented ledger row for "Critical false-green controls for validation machinery"; guarantees the machinery behind the states being promoted is proven. Ledger presence alone satisfies the predecessor. Its locator metadata remains non-authoritative.
- **CPER0:** implemented ledger row for "Compiler equivalent-work and oracle genesis lock"; the equivalent-work methodology that would make any numeric promotion meaningful. Ledger presence alone satisfies the predecessor. Its locator metadata remains non-authoritative.
- **CPER2:** implemented ledger row for "Shared compiler physical-execution and zero-work terminal"; supplies only the execution and equivalent-work constraints a numeric adoption must satisfy. It decides nothing about behavior acceptance; VCP7, SCP7, or the case's named semantic owner alone does. Ledger presence alone satisfies the predecessor. Its locator metadata remains non-authoritative.
- **VCP7:** implemented ledger row for "Vue Default compiler product terminal"; the Vue semantic authority convergence that determines whether probed Vue behaviors are part of Verter's accepted contract. Ledger presence alone satisfies the predecessor. Its locator metadata remains non-authoritative.
- **SCP7:** implemented ledger row for "Svelte Default compiler product terminal"; the Svelte semantic authority convergence that determines whether probed Svelte behaviors are part of Verter's accepted contract. Ledger presence alone satisfies the predecessor. Its locator metadata remains non-authoritative.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- **Intent:** convert accumulated evidence into explicit state transitions without inventing acceptance.
- **Problem:** without a join, canaries rot: XPASS candidates stay invisible, skips outlive their missing prerequisites, and observations never become actionable evidence.
- **Solution and architecture decisions:**
  - review every canary, known-fail, skip, XPASS candidate, and benchmark observation accumulated by CVO1/CVO2;
  - transitions are explicit and selective: `skip -> canary`, `canary -> known-fail`, `known-fail -> gate`, `canary -> gate`;
  - no automatic promotion merely because a case happens to pass once; the owning semantic node (VCP7 for Vue, SCP7 for Svelte, or the case's named owner) decides whether behavior is accepted; the owning performance node decides whether a numeric observation becomes a baseline or regression gate;
  - cases whose owners remain later in the DAG remain deferred; CVO4 does not require every historical canary to become green;
  - deferred-failure policy stays in force: classify -> associate owner -> canary/known-fail/skip -> continue; a test failure alone does not override DAG ownership.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence selection that actually discriminates the touched contract. Existing behavioral coverage, static validation, canonical gates, and bounded inspection are valid when accompanied by a terse rationale.

- **CVO4-AC1 — sole-owner outcome:** every promotion in the join record names its deciding owner and evidence; zero promotions decided by CI convenience or pass-streak heuristics.
- **CVO4-AC2 — positive contract:** `JoinRecord::validate` passes on the published record and fails closed on two planted negatives kept as fixtures: a record omitting one manifest entry, and a `known-fail -> gate` promotion with no deciding authority; `validate-probe-authorities.mjs` refuses a promotion citing an unimplemented owner or a wrong-framework authority (its own fixtures, owned by CVO0). After the join, every probe-state manifest entry is internally consistent — gates are enforced as gates, canaries keep their expected classes, skips still name owners, XPASS candidates are either promoted with an owner or left `deferred` with a reason.
- **CVO4-AC3 — incremental equivalence:** not applicable; the join owns no incremental, cache, cancellation, or publication authority.
- **CVO4-AC4 — bounded work:** not applicable; the join owns no hot path.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; CVO3's controls already prove state-transition machinery. Do not add duplicate permutations.
- Test homes: `crates/verter_validation_probe/tests/cases/join.rs` via the crate's single `tests/main.rs`.

## Deletions and forbidden designs

- Delete or structurally reject: **pass-streak or automatic promotion rules**.
- Delete or structurally reject: **gate demotion done silently to keep CI green**.
- Never let this join expand into implementing deferred behavior, weakening a gate without an owner decision, or converting observations into thresholds without the performance authority.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero promotion without a named deciding owner; zero silent class change.
- Performance budget: not applicable; numeric adoption remains with the performance authority.

## Abort conditions

- Stop before mutation if an owning authority contract needed for a candidate decision is not yet available; leave the candidate deferred rather than guessing.
- Abort the candidate if the join would require a compiler semantic change or a new threshold; those belong to their owning nodes.

## Targeted verification

1. `cargo nextest run -p verter_validation_probe` and `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. The two planted negatives are the TDD artifact for the validator.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial`, `conformance`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `targeted` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Reconciled source-plan contract

**Intent:** review accumulated probe and observation state after the relevant authorities converge and record explicit, owner-driven transitions.

**Problem:** without a join, XPASS candidates and stale skips never become actionable, and observations never reach the authorities that could adopt them.

**Solution and architecture decisions:**

- enumerate every accumulated canary, known-fail, skip, and observation;
- allowed transitions: `skip -> canary`, `canary -> known-fail`, `known-fail -> gate`, `canary -> gate`;
- deciding owners: semantic authority for behavior acceptance, performance authority for numeric baselines/gates;
- deferred cases stay deferred; no automatic promotion; no gate weakening without an owner.

**Suggested predecessors:** `CVO1`, `CVO1S`, `CVO2`, `CVO3` for the evidence; `VCP7` and `SCP7` for behavior acceptance; `CPER0` and `CPER2` for numeric adoption constraints.

**Normative source decomposition:**

1. **CVO4-A — Evidence inventory.** Every accumulated state and observation enumerated.
2. **CVO4-B — Owner consultation record.** Per candidate: deciding owner, contract basis, decision.
3. **CVO4-C — Manifest updates.** Applied transitions with rationale; deferred entries left explicit.
4. **CVO4-D — Join record publication.** The review artifact summarizing promotions and deferrals.

**Acceptance:** every transition names a deciding owner and evidence; deferred behavior stays deferred with owners; nothing gates or un-gates implicitly.

**Forbidden:** automatic promotion, threshold invention, scope expansion into implementing deferred behavior.

**Deletion/abort:** delete any heuristic promotion residue; abort on a missing owning contract and leave the candidate deferred.
