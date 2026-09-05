<!-- unified-charter-v2
id=DEBT-SOURCE-LINEAGE
name=Close the open source_lineage debt
phase=rev11
train=source_lineage
product=typescript_mapper
kind=repair
semantic_role=delivery
class=foundational
predecessors=
owner=source_lineage:ratified dual-plane mapper/snapshot/oracle identity contract and its closure instrument, carried to a debt-free state
conflict_domains=source_lineage
resource_class=rust-mixed
review_profile=semantic-3
gate_profile=canonical
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
release_gating=none
external_requirements=
charter=charters/source-lineage/DEBT-SOURCE-LINEAGE.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# DEBT-SOURCE-LINEAGE — Close the open source_lineage debt

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Every open debt row in area `source_lineage` is closed. The current owner is **the ratified dual-plane mapper/snapshot/oracle identity contract and its closure instrument, carrying twenty open debt rows**. The final and sole owner is **the same contract and instrument with an empty open-debt register: each row below is either repaired in the tree it names, with the smallest discriminating evidence, or dispositioned under a decision ruling that cites that row id**. This charter accepts one repair boundary; it contains no independently dispatchable subblocks, and the twenty rows are one acceptance, not twenty.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_identity/src`, `crates/verter_session/tests/cases`.
- Repository-owned instrument surfaces: `roadmap/0.1.0-tama/tools/closure-register.mjs`, `roadmap/0.1.0-tama/tools/closure-register.pins.mjs`, `roadmap/0.1.0-tama/tools/closure-controls.mjs`, `roadmap/0.1.0-tama/closure/typescript-mapper/register.toml`, `roadmap/0.1.0-tama/decisions/2026-09-02-typescript-mapper-rescope.md`.
- Repository-owned gate-lane surface: `.github/workflows/ci.yml` (the `tama-roadmap` and `tama-controls` jobs).
- Named API/data boundaries: the register's closed claim/atom/finding/remainder/deletion-row universes and their pins; `CanonicalEncoder::field_sorted_set`; the `A-targeted-domain-green` evidence anchor.
- Mutation boundary: only the surfaces a row below names, plus the crate that root-causes a CI-red row. A CI-red fix confined to one crate and its test is inside this boundary; any wider production reach, or any second major concern combined with it, is a mandatory rescope rather than a silent widening.

## Exact predecessor contracts

- None. The area has no in-flight nodes; this block is dispatchable immediately on its own ledger row.
- External requirements: none. A maintainer decision ruling is a closure route for a row, not a precondition for starting the work.

## Source-specific scope

- **Revision:** 1
- **Prepared:** 2026-09-05
- **Repository basis:** tama_dag/DAG-source-lineage-20260905001424 at 7c7c1f66dbd4f540fe9de40205eb5d47ca627c3d
- **Current-program condition:** twenty open debt rows against the typescript-mapper rescope decision, its closure register/validator, and the CI lanes that re-run them

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **DEBT-SOURCE-LINEAGE-AC1 — sole-owner outcome:** the open-debt register for this area is empty — every row in the binding register below is closed by repair or by a cited ruling, and the deterministic governance scanner (layer 1) no longer reports any of them. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **DEBT-SOURCE-LINEAGE-AC2 — positive contract:** the instrument's derived properties the rows name — re-derived cargo counters, per-mutation refusals, exemption truth, anchor reach — remain derived, pinned, and re-checked, with `closure-register.mjs --check` and the control suite green on the final tree. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **DEBT-SOURCE-LINEAGE-AC3 — incremental equivalence:** when a CI-red repair touches incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **DEBT-SOURCE-LINEAGE-AC4 — bounded work:** when a CI-red repair touches a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: the instrument's own suite (`roadmap/0.1.0-tama/tools/closure-register.test.mjs`, `roadmap/0.1.0-tama/tools/closure-controls.test.mjs`), `crates/verter_identity/src`, `crates/verter_session/tests/cases`.

## Binding debt-row closure register

This section is the acceptance. Each row carries its id, severity, and admission exactly as raised; a row is closed only by (a) the work done in the tree the row names, with evidence proportionate to that row, or (b) a decision ruling that cites that row id, per contract F7. No row is closed by re-description, by narrowing the scanner, or by dropping it from any input.

Governance ruling rows — debt rows standing without a ruling (contract F7, deterministic scanner, governance layer 1):

- **debt_0mtnfv2v3009xes04** [P0 invariant-defect]: `decisions/2026-09-02-typescript-mapper-rescope.md` carries a debt row without a ruling — cite the decision id that deferred it, or do the work.
- **debt_0mtnfug0u008tiziq** [P0 invariant-defect]: same file and same rule as raised at b0622d27bbb7dc2038efcacf25f1f1d4c578f992.
- **debt_0mtnf7nyt005faru8** [P0 invariant-defect]: same file and same rule as raised at eeffbd444f1d0cd4891d6caa3615045eda5501a7.

CI-red true-bug rows — `CI Required` concluded failure after the review-round cap; each closed only by root cause and fix, never by a re-run:

- **debt_0mtnen6by008l7i2q** [P1 blocking-defect]: CI red after the review-round cap; a true bug still blocks.
- **debt_0mtnd1yk8000gl5dw** [P1 blocking-defect]: CI red after the review-round cap; a true bug still blocks.
- **debt_0mtncwyoo000gimc7** [P1 blocking-defect]: CI red after the review-round cap; a true bug still blocks.
- **debt_0mtncsfjz000sex88** [P1 blocking-defect]: CI red after the review-round cap; a true bug still blocks.
- **debt_0mtnc3vt8004n41h3** [P1 blocking-defect]: CI red after the review-round cap; a true bug still blocks.

Instrument rows — the register, the validator, and the lanes that re-run them:

- **debt_0mtn4q4ey001408sn** [P2 blocking-defect]: the crate-root `A-targeted-domain-green` evidence anchor (`closure/typescript-mapper/register.toml`) puts every session change on a 45-minute `tama-controls` job; anchor at the fixtures already named (`tests/cases/mod.rs` and `Cargo.toml`) and drop the subtree glob.
- **debt_0mtn4m3b00034iv95** [P2 unsupported-completeness]: the charter targeted-verification cargo record is the one cargo record whose counters and name-filters are never checked live.
- **debt_0mtmw2rb0007exvzr** [P2 unsupported-completeness]: the cargo-counter residual is closed by machinery the rescope delta adds and then discards; its disclosure is now false.
- **debt_0mtmw2raz0079meqf** [P2 unsupported-completeness]: the Tama Roadmap 5-minute budget against the new re-application cost.
- **debt_0mtmea9pe0090vj35** [P2 unsupported-completeness]: the `owed_surface` anti-laundering mechanism has zero live use, and the one obligation the shipped code demonstrably fails is routed around it — `CLM-IDENTITY` reads PROVEN / Owed 0.
- **debt_0mtmw2rb2007mbljl** [P3 unsupported-completeness]: the decision record asserts a failure mode the encoder's only other consumer structurally prevents.
- **debt_0mtmw2rb1007j9sdo** [P3 unsupported-completeness]: the control-exemption gate counts callbacks, not mutations, and never requires a refusal.
- **debt_0mtmw2ray00754dza** [P3 unsupported-completeness]: the cargo-record re-derivation residual is disclosed but not resolved by a formal DEFER ruling row.
- **debt_0mtmw2rax0073qsn3** [P3 unsupported-completeness]: the new `tama-controls` lane mirrors the whole repo per run with no measured cost bound.
- **debt_0mtmea9pg0095capn** [P3 unsupported-completeness]: the acyclicity exemption is granted per-record on a property that holds only per-case, with nothing enforcing it for cases added later.
- **debt_0mtmea9pf0093p39j** [P3 unsupported-completeness]: 16 refusal paths in the validator are never executed by any test, including two the decision record argues at length.
- **debt_0mtmea9p9008swebb** [P3 unsupported-completeness]: cargo-record transcribed counters are bound to no re-derivation (disclosed residual).

A row whose severity label reads `[?]` in the raising record keeps the severity shown above, which is the value that record carried. Discovery of a twenty-first open row in this area during the work is an amendment and a new node, not a quiet addition here.

## Deletions and forbidden designs

- Delete or structurally reject: **unruled debt rows** — every DEFER/debt statement in this area stands under a cited decision id or is done.
- Delete or structurally reject: **the crate-root evidence anchor that widens a 45-minute lane onto every session change**.
- Delete or structurally reject: **the false cargo-counter disclosure** — a residual statement whose delta both created and discarded the machinery that closes it.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Never close a row by weakening the instrument that detected it; the scanner, the validator, and their pinned universes leave this block at least as strict as they entered it.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern. A CI-red root cause that lands outside the named production surfaces is the named trigger for this rescope, not a violation of it.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing; zero rows closed without repair or cited ruling.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally. A CI-red row whose root cause cannot be reproduced on the candidate is an abort, not a re-run.

## Targeted verification

1. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
2. `node roadmap/0.1.0-tama/tools/closure-register.mjs --check`
3. `node --test roadmap/0.1.0-tama/tools/closure-register.test.mjs roadmap/0.1.0-tama/tools/closure-controls.test.mjs`
4. `node scripts/gate.mjs`
5. Run every final command in the bound `canonical` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
6. Bind the preflight evidence selection and terse rationale, row by row, in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial`, `conformance`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 2/2 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `targeted` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
