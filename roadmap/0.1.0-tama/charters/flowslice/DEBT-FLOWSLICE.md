<!-- unified-charter-v2
id=DEBT-FLOWSLICE
name=Close the flowslice open-debt ledger
phase=rev11
train=flowslice
product=rev11
kind=repair
semantic_role=delivery
class=foundational
predecessors=D3R
owner=flowslice:sole debt-closure authority over the flowslice relation and flow substrate
conflict_domains=flowslice
resource_class=rust-mixed
review_profile=architecture-3
gate_profile=targeted-domain
implementation_effort_min=medium
implementation_effort_default=medium
review_effort_min=high
review_effort_default=high
verification_effort_min=medium
verification_effort_default=medium
confirmation_effort_min=medium
confirmation_effort_default=medium
size=M
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/flowslice/DEBT-FLOWSLICE.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# DEBT-FLOWSLICE — Close the flowslice open-debt ledger

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Close every open debt row recorded against the flowslice area — the shared nominal relation substrate landed by D3R (`RelationKind::Identity`/`Comparable` over `ProjectSemanticDispatch`, `crates/verter_session/src/project_semantic_dispatch/relation.rs` and its consumers) and the D3 atomic quad it belongs to. The inventory below lists twenty-four open rows raised across four review rounds of the D3R candidate (PR #472 and its successors). Acceptance is exactly: **each row closed**, and nothing else is independently acceptable.

A row closes by exactly one recorded disposition, stated per row in the landing evidence:

1. **fixed-here** — the remedy lands in this candidate (production fix, prose correction, structural guard, or discriminating test), with the smallest evidence set that actually discriminates the row's claimed boundary.
2. **verified-stale** — the row is re-verified against the tree that exists; either the claimed risk no longer exists (recorded with the discriminating evidence that proves it) or the debt record is corrected to describe the real tree instead of the one the row describes.
3. **owned-forward** — the remedy is the declared job of a named later DAG node; the row closes here only through the corrections the row itself demands plus a finding carry-forward record (`issue`, `severity`, `owner`) binding the remaining remedy to that node.

No row may be left open, deferred without a named owner, or closed by deleting its evidence. The non-goals are as binding as the goal: this node does not implement TA1A's structural comparator, does not teach `tag_level_disjoint` the nominal axis, does not move intersection-collapse policy out of TA1A, does not implement D3I/D3P/D3C content, and does not implement `Subtype`/`StrictSubtype`. Current and final owner is the sole flowslice debt-closure authority over the same `crates/verter_session` surfaces D3R owns; this charter accepts one authority boundary and contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_session/src` only — `project_semantic_dispatch/relation.rs`, `project_semantic_dispatch/canonical_algebra.rs` (claim corrections), `project_semantic_dispatch/flow_return.rs`, and `semantic_query.rs` or the `typeof`-carrier mint sites only where a row demands marking or a bounded-chase fix.
- Named API/data boundaries: `RelationKind`, `SemanticQueryKey::TypeOf` and its consumers, `RelateMemoKey`, the `typeof` carrier mint sites, and the nominal carrier `value_root`.
- Mutation boundary: only the surfaces a row names; every changed path must be inside both this charter's surface and the acquired `flowslice` conflict domain; sibling ownership is excluded.
- Test homes: `crates/verter_semantic/tests`, `crates/verter_session/tests/cases`.

## Exact predecessor contracts

- **D3R:** implemented ledger row for “Nominal relation authority”; ledger presence alone satisfies the predecessor. D3R's own charter and the D3 scope ruling (`decisions/2026-08-30-rev11-flow-d3-split.md`) require D3R to land only inside the one atomic D3R/D3I/D3P/D3C candidate; the atomic-landing rows below verify that quad and this node implements none of the other three. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** none; tooling does not validate external state.

## Open-debt inventory and row closure

Every row below must close. Group A rows close only against a completed atomic quad landing; group B rows are TA1A-owned algebra scope; group C rows close fixed-here or verified-stale.

### Group A — atomic D3 landing (verified against the landed quad; not implemented here)

These five rows all record one defect: the D3 scope ruling requires D3R, D3I, D3P, and D3C to land as ONE atomic multi-node candidate with none merging independently, while the D3R review candidate (PR #472, branch `tama_dag/D3R`) transitioned only D3R's row, left D3C/D3I/D3P pending, was neither squashed nor reviewed as one atomic candidate, and carried unrelated commits.

- **debt_0mtnfukco009cng50** [P1, required-acceptance] (round 2): candidate is neither atomic nor squashed. Closure: all four ledger rows (`implemented.toml` D3R/D3I/D3P/D3C) are transitioned inside ONE candidate and review ran against its squash.
- **debt_0mtnfukcn0098y344** [P1, required-acceptance] (round 2): required atomic D3 landing is missing. Closure: the same completed quad fact — D3I, D3P, and D3C rows are no longer pending.
- **debt_0mtnfukcn009agoi9** [P1, required-acceptance] (round 2): the atomic-landing ruling is unsatisfiable by the D3R PR alone. Closure: the ruling is satisfied by the landed quad, or — if the maintainer re-rules the landing composition — by a recorded successor decision that supersedes `decisions/2026-08-30-rev11-flow-d3-split.md` and each row is re-pointed at it; silence is not a disposition.
- **debt_0mtnfukcm0096optl** [P2, required-acceptance] (round 2): landing composition violates the atomic ruling and carries unrelated commits (`b71832360` “Exact style identity and owner-domain reuse” and the J2 style commit). Closure: the quad's cumulative diff contains no commit outside the four nodes' charters; the unrelated commits are absent from the reviewed squash.
- **debt_0mtnfukck0094x1ib** [P3, required-acceptance] (round 1): atomic quad landing still pending. Closure: same completed-quad fact as above.

If the quad has not landed atomically when this node's review runs, these rows stay open and acceptance fails; this node must not implement D3I/D3P/D3C content to force them shut.

### Group B — TA1A-owned algebra scope (owned-forward to TA1A)

- **debt_0mtnh71z3009ih18j** [P3, future-scope] (round 4): canonical algebra's `tag_level_disjoint` was never taught the nominal axis (`canonical_algebra.rs:920-965` has no `TypeOfNominal` arms), so the “sole proven-disjoint oracle” prose overstates the algebra. Closure: do NOT teach the algebra the nominal axis here; correct the `relation.rs:4470-4473` sole-oracle claim to record that `relation.rs` now maintains the strictly stronger proven-disjoint oracle, and bind the algebra work to TA1A by a carry-forward record (owner `TA1A`).
- **debt_0mtncyezo001ro46r** [P2, future-scope] (round 3): `checker_intersection_collapse` (`relation.rs:514`) is a second walk that re-reads the pair after `Comparable` already decided. Closure: keep the payload, do NOT extend the class function here; intersection-collapse policy stays TA1A's alone; carry-forward record (owner `TA1A`).

### Group C — coverage, audit, and bounded-work rows (fixed-here or verified-stale)

- **debt_0mtnh71z10098jp09** [P2, unsupported-completeness]: mixed-kind disjoint-oracle coverage is 2 of ~15 fallthrough-governed pairs. Closure: table-driven discriminating coverage for the fallthrough-governed mixed-kind pairs, or a recorded per-pair disposition naming why the pair cannot produce a decided result (an undecided pair must remain a typed gap and must not warm).
- **debt_0mtn5vxk8002zz2tw** [P3, unsupported-completeness]: the carrier-classification invariant is review-held, not compiler-held; three unaudited `typeof` consumers were audited as low-risk in review only. Closure: a structural guard makes the classification invariant compiler-held (a new unaudited consumer fails a test, not a reviewer), and the three consumers' low-risk audit is recorded as named evidence.
- **debt_0mtn5vxk7002xc5jv** [P3, unsupported-completeness]: most open debt rows are stale and overstate remaining risk. Closure: every row is re-verified against the tree that exists; stale claims are corrected or closed verified-stale with the evidence that discriminates them.
- **debt_0mtn5vxk5002sdno8** [P2, unsupported-completeness]: critical regression recipes do not prove their claimed consumer boundaries. Closure: each named recipe actually exercises the consumer boundary it claims, or its claim is rewritten to what it does prove.
- **debt_0mtn5vxk1002htn4w** [P3, unsupported-completeness]: the pending-path nominal walker assertion is non-discriminating. Closure: strengthen the assertion so it fails when the pending-path walker regresses, or delete it with a recorded rationale; a mirror that cannot fail closes nothing.
- **debt_0mtmn85g0006ci6cl** [P2, unsupported-completeness]: the Svelte named-member export is an unaudited consumer of the changed `TypeOf` answer and emits into a generated public declaration. Closure: audit recorded plus a discriminating fixture over the generated public declaration output.
- **debt_0mtmn85fz0069ugv2** [P2, unsupported-completeness]: AC4's bounded-work proof has a named hole — a qualified `typeof` over a NON-unique `symbol` member pays an uncached declaration chase. Closure: bound the chase (cache/memo or structural preclusion) or prove it bounded with an equivalent-work counter on that exact path.
- **debt_0mtmn85fx0063w0n7** [P3, unsupported-completeness]: two of the five debt rows on record are stale and describe a tree that no longer exists. Closure: verified-stale with the discriminating evidence, or the rows are corrected to the current tree.
- **debt_0mtmn85ft005vev8g** [P3, unsupported-completeness]: comparability member descent is unbounded in depth where the displaced classifier was one level, and the tsc divergence is unpinned below depth 1. Closure: a fixture pins the tsc-comparable behavior at member depth ≥ 2, and either the descent is bounded or the depth-divergence is recorded as the pinned contract.
- **debt_0mtmn85ft005xccgn** [P3, unsupported-completeness]: the recorded class-static bound omits inherited statics. Closure: the recorded bound is corrected to include inherited statics and a fixture pins it.
- **debt_0mtmn85fr005s3qyj** [P2, unsupported-completeness]: the unreduced-operand fixture cannot observe the dangerous direction it documents. Closure: the fixture is made to observe that direction (it fails when the dangerous behavior returns) or the documented claim is removed.
- **debt_0mtmn85fq005qy4j9** [P2, unsupported-completeness]: the Svelte named-member `typeof` consumer is an uncovered instance of the wrong-OPEN class the candidate fixed and pinned for Vue. Closure: the Svelte counterpart of the Vue pin lands here.
- **debt_0mtmdrq0h006uflb5** [P2, unsupported-completeness]: three `SemanticQueryKey::TypeOf` consumers outside the amendment's audited six are neither examined nor covered. Closure: each of the three is examined and either covered by a discriminating test or recorded as low-risk with named evidence.
- **debt_0mtmdrq0g006oc2c3** [P3, unsupported-completeness]: the path walker's fail-closed change is outside the nominal axis and untested. Closure: a discriminating test proves the path walker fails closed on the changed path.
- **debt_0mtmdrq0f006ltlli** [P2, unsupported-completeness]: the “ONE mint site” invariant is not enforced — two production producers still mint unmarked `typeof` carriers for a unique-symbol root. Closure: the single mint site is enforced (marking plus structural guard) so neither producer can mint an unmarked carrier without a test failing.
- **debt_0mtma23gz00d907by** [P3, unsupported-completeness]: the nominal carrier rewrites `value_root` to the declaring root; renamed-import display/provenance is untested. Closure: a renamed-import fixture pins the displayed root and provenance.
- **debt_0mtma23gv00cvvvbt** [P2, unsupported-completeness]: AC4 (bounded work) is unmet for the hot paths this candidate actually changed. Closure: equivalent-work evidence on exactly those changed hot paths — no hidden duplicate parse, resolve, declaration chase, or retained candidate.

## Source-specific scope

- Deliver exactly the closure of the twenty-four rows above; no neighboring authority is included and no row's remedy may be satisfied by enlarging another node's charter.
- Landing: this node is a single-node candidate and lands independently of the D3 quad's atomic ruling — it rides no quad and no other node rides it.
- A discovered second independently acceptable outcome (for example, a row whose only honest remedy is production behavior change beyond the surfaces above) requires an amendment and a new DAG node before mutation.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, static validation, and bounded inspection are valid when accompanied by a terse rationale.

- **DEBT-AC1 — every row closed:** the landing evidence records one disposition per row id (fixed-here, verified-stale, or owned-forward with carry-forward) and the group A rows close only against the completed atomic quad fact; an open, ownerless, or evidence-free row fails acceptance.
- **DEBT-AC2 — no authority enlargement:** TA1A's comparator/collapse ownership, the D3I/D3P/D3C charters, `Relate` as the sole query tag, and pending `Subtype`/`StrictSubtype` are all intact after the candidate; every group B row is closed without implementing TA1A's job.
- **DEBT-AC3 — incremental equivalence:** rows touching the mint sites, `value_root`, the declaration chase, or memoization prove incremental equals fresh and no undecided result warms; otherwise record a terse not-applicable rationale tied to the untouched authority.
- **DEBT-AC4 — bounded work:** the two AC4 rows (`debt_0mtmn85fz0069ugv2`, `debt_0mtma23gv00cvvvbt`) close with equivalent-work evidence on the named hot paths; do not add counters or a soak for rows that do not name one.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_semantic/tests`, `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete or correct only what a row names: the overstated sole-oracle prose (`relation.rs:4470-4473`), the unenforced mint-site invariant, the unbounded declaration chase, and the non-discriminating assertions/fixtures the rows identify.
- Never teach `tag_level_disjoint` the nominal axis, extend `checker_intersection_collapse`, or move any canonical-algebra decision here — those are TA1A's alone.
- Never implement D3I/D3P/D3C content, `Subtype`, `StrictSubtype`, a public `Unknown` payload, a second unique-symbol identity type, or a second relation query family.
- Never route around the sole dispatch (`ProjectSemanticDispatch` / `execute_function_return_source`), and never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Never close a row by weakening its claim, deleting its evidence, or re-pointing it at a node that does not exist in the roadmap.

## Budgets and rescope

- Planning reference: 800 production LOC, 8 production files, 2 related crates/packages (ruling expectation: the bulk is discriminating tests, guards, and prose corrections inside `verter_session`; production change is small).
- Numeric rescope signal: 1,500 production LOC or 12 files. Crossing it requires a scope-coherence investigation under `contracts/sizing.md`, not automatic rescope.
- Architect rescope remains mandatory when the candidate spans 3 unrelated crates/packages, or combines public/wire, unsafe, concurrency, or lifetime work with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: the two AC4 rows close with equivalent-work counters increasing by 0 on the named hot paths; wall/allocation/RSS regression allowance is 0.0% there. Otherwise performance evidence is not applicable; do not create counters or a soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.
- Abort if closing a row honestly requires implementing another node's chartered scope; that row needs an amendment, not a workaround.

## Targeted verification

1. `cargo nextest run -p verter_semantic -p verter_session`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection, the per-row disposition table, and a terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's
predeclared row in `authority/state/implemented.toml` from `status = "pending"`
to `status = "implemented"` with the planned squash commit message, approximate
date with timezone, and optional pull-request number. The transitioned row is the
implementation fact. Commit metadata is a loose locator only and is never resolved or
validated against Git or GitHub. Reviewers inspect the squashed candidate patch without
SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
