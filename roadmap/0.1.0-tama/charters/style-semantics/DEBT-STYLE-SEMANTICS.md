<!-- unified-charter-v2
id=DEBT-STYLE-SEMANTICS
name=Style semantics open-debt closure
phase=rev11
train=style_semantics
product=rev11
kind=repair
semantic_role=delivery
class=subsystem
predecessors=
owner=style_semantics:verter_css_syntax facts with qualified stage results and owner-specific adapters
conflict_domains=style_semantics
resource_class=rust-mixed
review_profile=semantic-3
gate_profile=targeted-domain
implementation_effort_min=medium
implementation_effort_default=high
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
charter=charters/style-semantics/DEBT-STYLE-SEMANTICS.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# DEBT-STYLE-SEMANTICS — Style semantics open-debt closure

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Close every open debt row recorded against area `style_semantics` after the `rev11.style` J-train landings. The current owner is **the post-J3 shared-plan cascade with its recorded debt rows open**. The final and sole owner is **verter_css_syntax facts with qualified stage results and owner-specific adapters**. This charter accepts one repair boundary; it contains no independently dispatchable subblocks. Acceptance is row-complete: each row in the debt register below is closed by its stated condition, and no row is closed by weakening the invariant it reports.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src`, `crates/verter_compiler/tests`, `crates/verter_css_syntax/src`, `crates/verter_bench`.
- Documentation and record surfaces: `.claude/skills/compiler-codegen/SKILL.md`, `roadmap/0.1.0-tama/charters/rev11-style/J2.md`, `roadmap/0.1.0-tama/charters/rev11-style/J3.md`.
- Named API/data boundaries: `transform_vue_css_modules`, `transform_vue_scoped_css`, `transform_vue_v_bind`, `plan_authored_v_bind_edits`, `merge_shared_stage_edits`, `PlanDisjointEdits`, `PlainCssInput`, `CssStageRequest`, `SharedVueStylePlan`, `StyleRewriteStage`, `StyleRewriteFailure`.
- Mutation boundary: only the production, documentation, and record surfaces and named API/data boundaries above; every changed path must be inside that list. Amendments to the J2/J3 charter records are limited to recording rulings on rows this register names — no other J-record content is rewritten.

## Exact predecessor contracts

- None. The area has no in-flight predecessor nodes; the debt rows target landed `rev11.style` work already recorded implemented in the ledger. Ledger presence of those landings is the historical basis, not a readiness edge.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- **Prepared:** 2026-09-05
- **Repository basis:** branch `tama_dag/DAG-style-semantics-20260905001713` cut from `main` at `7c7c1f66`
- **Debt source:** review findings recorded against the J2/J3 candidates (round 8 and later) plus post-landing completeness sweeps, as enumerated in the register below.

## Debt register and closure definitions

Thirty rows, grouped into seven closure work items. The rows were filed against the J2/J3 review candidates; the tree has moved since (later landings demoted the staged oracles to `#[cfg(test)]`-gated crate-test instruments, migrated `verter_bench`, and introduced `PlanDisjointEdits`), so a row's finding may already be satisfied in current source. A row is closed when, against the candidate's starting tree, its group's closure condition holds — because this node's patch delivers the named repair, or because the implementer verifies the condition already holds and cites the landing that delivered it. The review report carries a per-row closure statement naming which of the two closed it and the discriminating evidence. Where a row's remedy is an operator ruling on a record (groups 6 and 7), closure means the ruling is written into the named record with a reference a later reader can follow — silence or a local note outside that record does not close it.

### CWI-1 — Second CSS parse path structurally rejected (J3-AC1)

As filed: `transform_vue_css_modules` and `transform_vue_scoped_css` were production-compiled public functions calling `parse_ir` on their own bytes (`style_planner.rs` staged-oracle region), `verter_bench` (`css_identities.rs`, `new_impl_comparison.rs`) chained them, `.claude/skills/compiler-codegen/SKILL.md` sanctioned them as retained staged oracles, and `transform_vue_v_bind` carried a second v-bind edit implementation for the overlapping case. J3-AC1 requires the second CSS parse path deleted or structurally rejected; `#[doc(hidden)]` is not a visibility bar.

Closure condition: no second CSS parse path is reachable from outside `verter_compiler` in any non-test build — the staged instruments are absent or `#[cfg(test)]`-gated with crate-private visibility and no non-test compilation route (`test-support` or any ordinary feature must not compile them) — and `verter_bench` builds and runs only against the shared-plan/production route with no staged chaining. v-bind edit planning is single-sourced: exactly one implementation of the overlapping-case behavior, exercised through `plan_authored_v_bind_edits`. The skill's staged-oracle paragraph states the enforced structural rule, not a sanctioned exception. Discriminating proof: the visibility/cfg structure is shown (compile-fail or capability test pinning the rejection through the public boundary, or proof the functions no longer exist outside `#[cfg(test)]`), plus the bench targets compiling through the shared route.

- debt_0mtnainy4004ct482 [P1 invariant-defect] — public staged oracles violate the second-parse-path prohibition; skill line ~464 sanctioned them against J3's own deletions.
- debt_0mtn7yvpw002ojigr [P2 invariant-defect] — same boundary: `pub fn` oracles remained, `verter_bench` chained them.
- debt_0mtn4sebn001ihab0 [P2 invariant-defect] — same boundary: staged transforms remained a public second parse path with zero production callers.
- debt_0mtn01utz000solx0 [P3 unsupported-completeness] — the J3 deletion "second CSS parse path" was only partially discharged; two public per-stage transforms survived.
- debt_0mtmfd4k0001m3t69 [P2 unsupported-completeness] — `transform_vue_v_bind` duplicated v-bind edit planning outside the shared helper.
- debt_0mtmdem97004ngjas [P2 unsupported-completeness] — two independent v-bind edit implementations remained for the overlapping case.

### CWI-2 — Plain-CSS gate stamps the true stage (J3-AC2 identities)

As filed: `PlainCssInput::try_new` always recorded `StyleRewriteStage::PostPreprocessScoping`, and `CssStageRequest::gated` forwarded that failure unchanged, so `<style module lang="scss">` with `scoped=false` reported a scoping refusal for a modules-only block. Publication still cleared (fail-closed holds); the stage identity is wrong.

Closure condition: the durable design holds — `try_new` is stage-free admission; `gated` stamps `PostPreprocessModules` when the request is a modules block and `PostPreprocessScoping` otherwise. A test through the public compile boundary discriminates that a modules-only plain-CSS refusal names the modules stage.

- debt_0mtn7yvpx002qlk20 [P2 invariant-defect] — modules-only refusal stamped as the scoping stage.
- debt_0mtn4sebo001mxrd5 [P2 invariant-defect] — same finding, independent round.

### CWI-3 — v-bind refusal output policy is single-sourced

As filed: `plan_authored_v_bind_edits` success with non-disjoint edits took `SharedVueStylePlan::refused` to `ClearsOutput` and dropped modules/scoped output, while the sibling `Err` arm recorded `KeepsOutput` and continued; the documented v-bind policy is keep-authored-bytes; and the discriminating test constructed a fake authored outcome without running the cascade.

Closure condition: one truth — either the overlapping-edits refusal keeps authored output, consistent with the documented policy and the sibling arm, or the policy document is corrected to state the clearing rule with the reason; code and document must state the same rule. The discriminating test exercises the real cascade end-to-end through the public boundary rather than a constructed outcome, and pins whichever rule is declared true.

- debt_0mtn7yvpy002upmsm [P2 invariant-defect] — overlapping v-bind edits clear output while other v-bind failures keep it; test never runs the cascade.

### CWI-4 — Shared-plan merge correctness is structural or accurately fenced

As filed: `merge_shared_stage_edits` saw authored-span intersection only; its correctness rested on a disjoint-`prior` precondition and on replacement text not resubmitting tokens another stage would rewrite — documented, not structural. Shared-vs-staged equivalence was held only by the fixture corpus (`a_shared_plan_matches_running_the_stages_one_after_the_other`), which could pass vacuously on overlap-prone fixtures, lacked an at-rule-nested selector family, and left named merge branches untested; the skill overstated the fail-closed property for future stage additions; and the `Arc<str>` shared-code API elided no copy on any production path while adding an unmeasured O(n) compare.

Closure condition: the disjointness precondition is structural — a type-carried invariant (`PlanDisjointEdits` or equivalent) that a caller cannot construct dishonestly and that every merge re-establishes over its composition — and each merge branch named below is either covered by direct discriminating coverage (including the mutation recipes that fail when the branch is removed) or provably unreachable under that structure; "reachable and untested" is not closable. The disjointness coupling the Insert arms rely on is stated where the arms use it. The replacement-text neutrality assumption is either fenced (the merge or a later gate refuses when a replacement could resubmit another stage's tokens) or stated in the skill as the exact remaining trusted condition — no stronger. The equivalence corpus contains an at-rule-nested selector family, and at least one fixture fails the sweep when its overlap checking is removed (anti-vacuity). The skill's fail-closed statement names precisely what a future stage addition must do to stay fail-closed. The `Arc<str>` shared-code API is either removed or kept with a recorded measurement of the win it exists for.

- debt_0mtn7yvpx002sn0kv [P2 required-acceptance] — equivalence fixture-held, not structural; replacement-text token risk.
- debt_0mtn4sebo001kisgt [P2 required-acceptance] — same finding, independent round.
- debt_0mtmhy1t9008izzjg [P3 unsupported-completeness] — unguarded assumption about replacement-text content.
- debt_0mtn01utz000qugn1 [P3 unsupported-completeness] — disjoint-`prior` precondition documented but not structural.
- debt_0mtmuse75004ozr5g [P3 unsupported-completeness] — Insert arm correct only because `prior` is disjoint, coupling unstated.
- debt_0mtmmgud3003r36rx [P3 unsupported-completeness] — equivalence sweep can pass vacuously on the overlap-prone fixtures it was written for.
- debt_0mtmfd4k2001u0ahc [P3 unsupported-completeness] — later-Insert-inside-prior-overwrite branch untested.
- debt_0mtmfd4k1001q59zq [P3 unsupported-completeness] — Insert-discard branch untested.
- debt_0mtmdemay004urtqo [P3 unsupported-completeness] — one conflict-resolution branch untested.
- debt_0mtn01utz000ozg63 [P3 unsupported-completeness] — corpus claims every rewritten construct family but has no at-rule-nested selector family.
- debt_0mtmmgud0003ouyso [P3 unsupported-completeness] — skill overstates the shared plan's fail-closed property for future stage additions.
- debt_0mtmhy1t7008cf2v6 [P2 unsupported-completeness] — `Arc<str>` shared-code API elides no copy on any production path and adds an unmeasured O(n) compare.

### CWI-5 — Comments, diagnostics, and claims state the shared plan

As filed: `StyleRewriteFailure::to_diagnostic` and `VueStyleFacts::input_pulls_in_unparsed_bytes` docs still described per-stage rewritten-byte parses that shared planning does not perform, so a later change could restore `CascadeStageSpaces` from the comments; allocator-canary test diagnostics carried node-id markers (`J1_STYLE_PLANNER_ALLOC`, `J1_ALLOC_CONVERGED`, `J1_SLOTTED_ARENA`, …) against the no-roadmap-archaeology rule; and a recorded `:global` empty-argument panic claim had no corresponding diff.

Closure condition: no doc comment on the named items describes a per-stage rewritten-byte parse; allocator-canary diagnostics use durable pipeline names (grep for node-id markers in test diagnostics is empty); the `:global` empty-argument panic claim has its corresponding diff or the record is corrected to match what landed.

- debt_0mtnainy3004a8xp0 [P3 optional-improvement] — stale per-stage-parse comments at `style_planner.rs` ~129.
- debt_0mtn7yvpy002wjd95 [P3 invariant-defect] — allocator canary eprints still use node-id markers (`allocator_canaries.rs` ~144).
- debt_0mtmfd4k1001o45ca [P3 unsupported-completeness] — `:global` empty-argument panic claim has no corresponding diff.

### CWI-6 — J2's STYLE-REFUSAL-SPAN-PRECISION deferral gets its ruling

J2 recorded a deferral request (`charters/rev11-style/J2.md` "Requested DEFER — STYLE-REFUSAL-SPAN-PRECISION", ~line 273): block-wide refusal anchors after any earlier stage rewrite, proposed owner a rewrite-space→authored map, `Ruling reference: none yet`. J3's shared plan removed the premise entirely: `shared_plan_refusal_keeps_its_authored_anchor_after_prior_edits` holds the authored anchor through the public `compile()` boundary on exactly the proposed fixture (`.a { color: v-bind(tone); }\n}\n` with CSS Modules requested, stray `}` surfaced at `(refusal_start, refusal_start + 1)`). Three debt rows report the same gap from different rounds: the deferral is satisfied but still recorded open.

Closure condition: edit the J2 deferral section to record the ruling — resolved by J3's shared plan, ruling reference filled with J3's ledger row, the row marked closed — without altering the surrounding J2 record. This is the one permitted J2-record mutation.

- debt_0mtnainy30048vhak [P3 required-acceptance] — deferral resolved by J3 but left open in the J2 charter.
- debt_0mtnainy20042b4do [P3 required-acceptance] — proposed acceptance already delivered by `shared_plan_refusal_keeps_its_authored_anchor_after_prior_edits`; row still open.
- debt_0mtnainy2004437s2 [P3 required-acceptance] — same section still reads `Ruling reference: none yet`.

### CWI-7 — J3 charter surfaces tell the truth; the out-of-charter commit gets a ruling

J3's "Concrete surfaces and APIs" names `crates/verter_compiler/src/framework`, which does not exist; the J3 production change landed entirely in `crates/verter_compiler/src/style_planner.rs`, a path on none of J3's declared surfaces. Separately, the J3 branch carried an out-of-charter commit below the review base — `f4d755241 feat(play): migrate WASM fixture capture to typed compile request` (`packages/playground/scripts/capture-wasm-carrier-fixtures.mjs`, +68/−11), now in `main` history and never reviewed under the J3 lens.

Closure condition: amend J3's charter record so its production-surface list names the surfaces the landing actually touched (the `style_planner.rs` path; the non-existent `src/framework` subtree removed), and record an operator disposition for `f4d755241` — ratify as a tooling-only fixture-capture change outside J3's production surfaces, or revert — inside the same amendment. Both are record corrections owned by this node; neither reopens J3's implementation fact. This is the one permitted J3-record mutation.

- debt_0mtnainy1003yu84j [P3 required-acceptance] — J3 names the non-existent `crates/verter_compiler/src/framework` surface.
- debt_0mtnainy10040jb8m [P3 required-acceptance] — the only production file J3 changed is on none of its declared surfaces.
- debt_0mtnainy300460693 [P3 required-acceptance] — out-of-charter commit `f4d755241` below the review base, unreviewed under the J3 lens.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **DSS-AC1 — row-complete closure:** every row in the register is closed by its group's condition — repaired in this patch or verified already satisfied against the starting tree with a citation of the landing that delivered it — and the review report carries a per-row closure statement naming its discriminating evidence. A row closed by deleting a discriminating test, weakening the J3 invariant it reports, or rewording a record without the underlying repair is not closed.
- **DSS-AC2 — invariant preservation:** after closure, J3-AC1 (no second CSS parse path), J3-AC2 (exact stage identities), and the single-sourced v-bind output policy are still enforced — existing discriminating tests kept green or replaced by strictly stronger ones through the public compile boundary.
- **DSS-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority. Shared-plan style planning inside one compile request is expected to be not-applicable; state which authority bounds that claim.
- **DSS-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Any bench migration in CWI-1 must not reintroduce a second parse on a production route. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_compiler/tests`, `crates/verter_css_syntax/tests`, `crates/verter_bench`.

## Deletions and forbidden designs

- Delete or structurally reject: **public staged CSS parse oracles** (the second CSS parse path, however spelled).
- Delete or structurally reject: **wrong stage identity on plain-CSS gate refusals**.
- Delete or structurally reject: **duplicate v-bind edit planning for the overlapping case**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Never close a register row by weakening the invariant it reports, deleting its discriminating test, or leaving the precondition the row names still trusted-but-unenforced.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves a register row's premise outright (the named symbol no longer exists and no equivalent does — then close its rows by citing the landing that removed it), an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_css_syntax`
2. `cargo check -p verter_bench --all-targets`
3. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
4. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial`, `conformance`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 2/2 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `targeted` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
