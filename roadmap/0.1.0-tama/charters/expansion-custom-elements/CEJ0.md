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
owner=expansion.custom-elements:cross-framework standards-interoperability evidence
conflict_domains=customelementdeclaration,vue_product,svelte_product
resource_class=rust-mixed
review_profile=semantic-3
gate_profile=targeted-domain
implementation_effort_min=medium
implementation_effort_default=medium
review_effort_min=medium
review_effort_default=medium
verification_effort_min=medium
verification_effort_default=medium
confirmation_effort_min=medium
confirmation_effort_default=medium
size=M
dispatchable=true
optional=false
release_gating=non_release
external_requirements=
charter=charters/expansion-custom-elements/CEJ0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CEJ0 — Vue/Svelte Custom Element interoperability soak join

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Vue/Svelte Custom Element interoperability soak join. The accepted `CEC0` standards model and framework adapters remain the production authority. This node owns only **cross-framework standards-interoperability evidence** over those accepted terminals. It accepts one complete soak matrix and contains no production migration, cutover, or independently dispatchable implementation outcome.

## Concrete surfaces and APIs

- Evidence surfaces: `crates/verter_language/tests`, `crates/verter_protocol/tests`, `crates/verter_session/tests/cases`, and their existing Custom Element fixtures.
- Observed API/data boundaries: `CustomElementDeclaration`, `CustomElementRegistration`, `CemModule`, `FrameworkCeAdapter`.
- Mutation boundary: test, fixture, benchmark, and evidence bytes only. Production source and the observed APIs are read-only subjects of the soak.

## Exact predecessor contracts

- **CEC0:** implemented ledger row for “Shared legacy Web Component schema/registry cutover”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- **Normative intent:** continuously falsify standards interoperability across the two existing framework producers without coupling either release to the other.
- **Atomic boundary:** one complete Vue-producer/Svelte-consumer, Svelte-producer/Vue-consumer, and plain-HTML differential matrix over the accepted `CEC0` model. Findings are evidence; they do not mutate or revoke an accepted terminal in this node.
- **Evidence boundary:** cover same-tag/different-registry identity, props/attributes/events/slots/CSS/CEM fidelity, package ambiguity, incremental/fresh equivalence, stale publication, and bounded-work behavior. A production fix or independently acceptable implementation outcome requires its own amended node before mutation.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **CEJ0-AC1 — complete producer/consumer matrix:** both framework producers must be consumed by the other framework and plain HTML without framework knowledge entering the standards implementation; every required cell runs against pinned fixtures and oracles.
- **CEJ0-AC2 — standards fidelity:** same-tag/different-registry identity and props/attributes/events/slots/CSS/CEM observations preserve exact identities, provenance, completeness, and deterministic ordering.
- **CEJ0-AC3 — incremental equivalence:** incremental and fresh observations are identical, and cancelled, stale, ambiguous-package, or partial outcomes cannot publish or warm reusable state.
- **CEJ0-AC4 — bounded work:** existing counters, bounded inspection, or benchmarks prove that the matrix introduces no hidden duplicate parse, resolve, emit, copy, allocation, or retained candidate. A failure is published to the exact production owner; it is not repaired here.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_language/tests`, `crates/verter_protocol/tests`.

## Deletions and forbidden designs

- Delete no production source, schema, registry, route, or accepted evidence in this soak node.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages. Test, fixture, benchmark, and evidence changes must still fit one reviewable soak matrix.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance acceptance: use the exact applicable metric rows and methodology from performance-gates.toml, the applicable MEM0 budget, or the owning ratified product catalog, under contracts/resource-and-finalization.md (L2). Exact work invariants, latency/allocation/RSS limits under their owning methodology, and bounded new-capability budgets are distinct. New capabilities and deliberate pressure policies declare bounded new work and replacement SLOs before measurement. Missing required coverage needs an owning-contract amendment before measurement; no implicit 0.0% threshold or post-hoc rebaseline applies.

## Abort conditions

- Stop before mutation if `CEC0` is not accepted, the required cross-framework matrix cannot remain test/evidence-only, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_language -p verter_protocol -p verter_session`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial`, `conformance`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 2/2 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `targeted` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Reconciled source-plan contract

**Intent:** continuously falsify standards interoperability across the two existing framework producers without coupling either release to the other.
**Predecessors:** `CEC0`.
**Subblocks:** (1) same-tag/different-registry fixtures; (2) Vue producer consumed by Svelte/HTML; (3) Svelte producer consumed by Vue/HTML; (4) props/attrs/events/slots/CSS/CEM differential; (5) incremental/stale/package ambiguity and performance soak; (6) publish findings to exact owners.
**Acceptance:** both independently accepted terminals interoperate through HWC3-produced facts conforming to `CEF0`, with no framework knowledge in standards implementation; a soak failure creates a non-invalidating follow-up/version proposal by default and does not revoke a released terminal.
**Forbidden:** implementation fixes in the join, release serialization, shared Vue/Svelte special cases, or runtime-reachability claims.
**Deletion/abort:** delete nothing; reopening an accepted owner requires a separate maintainer impact decision with mechanically computed affected receipts; otherwise findings block only future stabilization that opts into the follow-up.

## 11. Sequential architecture falsification slices

Formatter-view/map feasibility is not a CEJ0 acceptance requirement. `PRF0`, which converges both CEJ0 and FMT1C, owns the sequential private-harness proof and any formatter/map counterexample. CEJ0 remains limited to the cross-framework standards-interoperability soak described above.
