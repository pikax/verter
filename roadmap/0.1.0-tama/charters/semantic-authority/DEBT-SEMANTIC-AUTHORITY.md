<!-- unified-charter-v2
id=DEBT-SEMANTIC-AUTHORITY
name=Close the open semantic-authority debt ledger
phase=rev11
train=semantic_authority
product=rev11
kind=repair
semantic_role=delivery
class=foundational-authority-repair
predecessors=E1,TE1
owner=semantic_authority:close every open semantic-authority debt row left by the in-flight E1 and TE1 candidates
conflict_domains=semantic_authority
resource_class=rust-mixed
review_profile=architecture-3
gate_profile=targeted-domain
implementation_effort_min=low
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
charter=charters/semantic-authority/DEBT-SEMANTIC-AUTHORITY.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# DEBT-SEMANTIC-AUTHORITY — Close the open semantic-authority debt ledger

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Close every open debt row recorded against the `semantic_authority` conflict domain by the in-flight E1 and TE1 candidates. A row is closed only by a recorded disposition: a landed remedy with discriminating evidence, a recorded architect amendment that ratifies what the row says was never ratified, or a recorded ruling that names the later DAG node which owns the remedy. Prose claims without executed evidence do not close rows. The final owner is the one account of the semantic-authority debt ledger in which no listed row remains open. This node repairs and records; it does not extend the operand forcing authority, add a second semantic graph, or rewrite the non-budget text of any other charter.

Acceptance is exactly: each row listed below is closed.

## Debt ledger and closure criteria

Every row below carries its ledger id, severity, and admission as filed. Rows re-filed in a later review round against the same underlying defect are marked as re-reports; one closure record citing every listed id closes them together. Six closure groups (A–F) define what "closed" means; a row closes exactly once against one group.

### Group A — record the TE1 mandatory-rescope amendment (or cut the surface)

- debt_0mtnbc7t3004mcrui [P1 required-acceptance] — both mandatory-rescope ceilings exceeded with no recorded rescope.
- debt_0mtnbc7t3004oh58t [P2 required-acceptance] — production size exceeds the charter mandatory-rescope line without an amendment.
- debt_0mtnbc7t3004qr37v [P1 required-acceptance] — mandatory rescope ceiling breached with no architect amendment.
- debt_0mtnbc7t2004epvcs [P1 required-acceptance] — mandatory rescope ceiling breached with no amendment.
- debt_0mtnbc7t2004ifyyu [P1 invariant-defect] — TE1 rescope ceiling still breached, unamended, going into round 2.
- debt_0mtnbc7t1004aelhd [P1 required-acceptance] — TE1 rescope ceiling breached without recorded ratification.
- debt_0mtnbc7t00046imbs [P1 invariant-defect] — TE1 rescope threshold breached without ratification.
- debt_0mtnbc7t2004kemwv [P1 invariant-defect] — delta touches files outside TE1's declared production/test-home boundary.

Closure: an additive, dated architect amendment is recorded in the TE1 charter's "Budgets and mandatory rescope" section naming the measured production population that actually landed (`crates/verter_session/src/project_semantic_dispatch/semantic_operand.rs`, `operand.rs`, `decl_body_memo/locator_deref.rs`, the `semantic_query_memo` plumbing, `host_resolve_type_audit.rs`, `host_test_force.rs`, `locator_identity.rs`, and the test-home escape `semantic_query_memo/tests.rs`) and either ratifying that population as one coherent boundary or naming the surface cut. No other TE1 text is rewritten. Alternatively the unnamed surface is cut and the rows close as landed remedies. The amendment records the honest measured file and LOC counts; it may not retroactively invent smaller numbers.

### Group B — make the required check green

- debt_0mtnh2fua0078p30a [P1 blocking-defect] — CI red: required check `CI Required` concluded failure after the review-round cap; a true bug still blocks.

Closure: the true bug behind the red required check is diagnosed, fixed, and the check concludes success on the final candidate. Closing this row by re-running alone, without the diagnosis, is forbidden.

### Group C — execute the mandatory controls and record them

- debt_0mtmp5cxk0028ou6v [P0 unsupported-completeness] — three mandatory mutation recipes were not executed.
- debt_0mtmuft3z003vd1xy [P1 unsupported-completeness] — no executed gate or mutation-recipe evidence exists for the candidate.
- debt_0mtmp5cxi0023flp3 [P1 unsupported-completeness] — gate and mutation-recipe controls could not execute for lack of disk space.

Closure: the three mandatory mutation recipes are executed with recorded outcomes, and the gate runs against the final candidate in an environment with adequate resources; both transcripts are part of the evidence pack. An environment failure (for example disk exhaustion) does not close a row; re-execution elsewhere does.

### Group D — complete the evidence pack

- debt_0mtnkhdy400a543x8 [P1 unsupported-completeness] — discovery artifact omits rule and guard changes.
- debt_0mtnbc7t2004gc5j1 [P2 unsupported-completeness] — review evidence diff omits the two commits that fix the P0 mutation-recipe findings (`32701adc0` cancellation preflight, `a8c1456d6` merge-role family axis).
- debt_0mtmp5cxh001zvxgw [P2 unsupported-completeness] — re-report of the same omitted-commits defect.
- debt_0mtnbc7t1004cno7s [P3 unsupported-completeness] — diff truncated before full review.
- debt_0mtmdwiug007wk039 [P3 unsupported-completeness] — re-report of the same truncation.

Closure: the evidence pack for the final candidate contains the complete, untruncated diff — explicitly including both single-line fix commits named above — and a discovery artifact that includes the rule and guard changes. The pack states its own byte count and file count so truncation is detectable.

### Group E — land the technical completeness remedies

- debt_0mtn7qcmh001j9wsn [P2 unsupported-completeness] — interned locator navigator is not exhaustive over `TypeBodyPathStep`.
- debt_0mtmp5cxj00250bso [P2 unsupported-completeness] — `navigate_type_body`'s `needs_ancestor` gate only inspects the path's starting node, not nested `Mapped`/`Conditional` reached mid-path.
- debt_0mtmdwiux0082n55e [P2 unsupported-completeness] — partial/cancel-during-force tests do not prove nested candidates stay cold.
- debt_0mtmuft400040fwme [P2 unsupported-completeness] — TE1-AC4's "no second in-flight table or request-local memo" is unproven and arguably contradicted.

Closure: the navigator is exhaustive over `TypeBodyPathStep` (every variant handled or a typed refusal with a discriminating test); the ancestor-need gate accounts for `Mapped`/`Conditional` reached mid-path; tests prove nested candidates stay cold under partial and cancel-during-force; and the TE1-AC4 claim is either proven with existing semantic statistics or bounded inspection hooks, or the contradicting second table/request-local memo is removed. Each remedy names the regression boundary it now fails-then-passes.

### Group F — record the call-site ruling

- debt_0mtnbc7t10048awt6 [P3 unsupported-completeness] — no production call site for the new forcing boundary yet.
- debt_0mtmdwiuf007rbps5 [P3 unsupported-completeness] — re-report of the same missing call site.

Closure: a recorded ruling that TE2–TE5 are the declared demand consumers of the forcing boundary (the TE1 charter already frames consumer demand as landing separately), or a production call site lands inside this node's own budget. The ruling names the successor node ids; it does not promise code this node does not contain.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_session/src` and, only where the `TypeBodyPathStep` vocabulary demands it, `crates/verter_type_expr/src`.
- Roadmap surfaces: an additive dated amendment inside `charters/rev11-type-evaluation/TE1.md` limited to its "Budgets and mandatory rescope" section, and this node's own closure/evidence record.
- Test homes: `crates/verter_session/tests/cases` and co-located `project_semantic_dispatch`/`semantic_query`/`decl_body_memo` unit tests, as ratified for TE1 by the Group A amendment.
- Mutation boundary: only the named surfaces above. This node does not change operator results, public `TypeInfo`, native-checker behavior, flow semantics, relation semantics, truthiness, canonical algebra, or wire contracts, and does not implement TE2–TE5 semantics.

## Exact predecessor contracts

- **E1:** in-flight "TypeExpr component-meta graph protocol consumer closure" candidate; its filed evidence-completeness and call-site debt rows are closed here. Ledger presence alone satisfies the predecessor; it supplies the consumer-side surface the rulings must stay consistent with.
- **TE1:** in-flight "Sealed semantic operands and forcing boundary" candidate; all remaining rows are filed against its charters and code. Ledger presence alone satisfies the predecessor; it supplies the forcing boundary whose budget amendment, evidence, and completeness remedies this node records and lands.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Acceptance IDs and discriminating proof

- **DSA-AC1 — every row dispositioned:** the closure record lists all 23 row ids above, each with its group, the evidence pointer, and the round in which it was filed; none remains open. Re-reports are closed by the same record that closes their original.
- **DSA-AC2 — rescope ratification is real:** the TE1 amendment is additive, dated, names the honest measured production population and the ratified or cut boundary escape, and leaves every other TE1 section byte-identical; the escaped files are either ratified in the amendment or reverted.
- **DSA-AC3 — required check green:** the diagnosis and fix for the `CI Required` failure are named and the check concludes success on the final candidate.
- **DSA-AC4 — executed controls:** mutation-recipe and gate transcripts exist, are complete, and cover the final candidate; the evidence pack diff is untruncated and contains both fix commits; the discovery artifact includes the rule and guard changes.
- **DSA-AC5 — technical proofs discriminate:** each Group E remedy lands with a test that names its regression boundary and fails against the pre-change shape; no quota or prose-mirror tests are counted as proof.
- **DSA-AC6 — rulings name owners:** the Group F ruling cites TE2–TE5 as the declared demand consumers, or the call site lands; either way no row is closed by an unowned claim.
- Every new test must name a plausible regression boundary and fail against the pre-change shape. Do not add prose mirrors, universal scanners, or non-discriminating quota tests.

## Deletions and forbidden designs

- Close rows only by the three dispositions defined above; a row may not be closed by re-filing, downgrading severity, or silent drop. Every closure names the surviving evidence or ruling route.
- No rewriting of any charter text outside the single additive TE1 budget amendment; no editing of other trains' DAG entries; no renumbering or reordering of any ledger.
- No second semantic graph, recipe language, generic evaluation trait, public/wire expansion, dual-running authority, or test-only production bypass enters through a fix. The Group B fix is a defect repair inside the existing forcing boundary, not an authority change.
- The CI-red row may not be closed by disabling the check, widening the gate profile, or marking the lane non-required.
- Do not implement TE2–TE5 semantics here; discovery of a second independently acceptable outcome requires a DAG amendment before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or if a new general graph/IR, public/wire change, unsafe code, or independent cache subsystem appears necessary.
- This node measures its own production mutation against these ceilings exactly as it holds TE1 to its own; exceeding a ceiling without a recorded amendment here would re-create the defect this node exists to close.
- Correctness budget: zero rows closed without recorded evidence; zero regressions in the E1/TE1 surfaces; the required check green.

## Abort conditions

- Stop before mutation if E1 or TE1 lacks an implemented ledger row.
- Stop before recording the Group A amendment if the honest measured population cannot be named; stop before cutting surface if the cut would orphan a consumer.
- Abort if the Group B diagnosis shows the bug's fix requires public/wire changes or operator-result changes — that is a new DAG node, not a debt closure; record it and stop.
- Abort on unexplained output, cancellation, allocation, latency, or fresh/incremental divergence in the affected surfaces; do not record it as local residue.

## Targeted verification

1. Run the new Group E discriminating tests and the existing E1/TE1 operand identity/capability/cancellation suites.
2. `cargo nextest run -p verter_session -p verter_semantic -p verter_type_expr`
3. Execute the mandatory mutation recipes and the bound `targeted-domain` commands on the stable review candidate; bind the DSA-AC1–AC6 evidence and the complete closure record in the review report.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, and `architecture-specialist`. Review must specifically challenge evidence-free closures, amendment honesty (measured numbers, additivity, no other charter text touched), the CI-green diagnosis, and completeness-proof discrimination. P0/P1 block. A P2 must have a named owner under the binding review policy or it blocks. Any material change invalidates affected verdicts. Final acceptance requires 3/3 current-round clean PASS reports plus `independent-full` confirmation, and the accepted state has no open row in this ledger.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Locator metadata is never validated against Git or GitHub.
