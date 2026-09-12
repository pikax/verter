<!-- unified-charter-v2
id=TE2
name=Conditional branch forcing at the lowering and instantiation sites
phase=rev11
train=rev11.type-evaluation
product=rev11
kind=implementation
semantic_role=delivery
class=foundational-authority
predecessors=TE1,D3C,TA1B,TE3
owner=rev11.type-evaluation:the sole demand-selected semantic operand forcing authority inside the existing SemanticQuery graph
conflict_domains=semantic_authority
resource_class=rust-mixed
review_profile=semantic-3
gate_profile=targeted-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=L
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/rev11-type-evaluation/TE2.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# TE2 — Conditional branch forcing at the lowering and instantiation sites

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Origin

This charter is the implementation ratification of TE2. The landed cutover
selects before lowering a branch at the authored site and defers losing-branch
substitution at instantiation, over materialized runtime handles. PR #526
already delivered dispatch-boundary selection-observed dependency facts; this
node preserves that baseline. Authored identity cannot carry a
conditional-`infer` frame, a binder-crossing locator dereference re-lowers
the enclosing conditional, and some positions have no locator spelling: those
limits remain binding and this node does not reverse them.

`TE2B` is not a node. Predecessors are TE1, D3C, TA1B, and TE3. The eager
lowering-site branch intern and the unconditional four-subtree substitution
descend are deleted by this implementation.

## Independently acceptable outcome

At the authored lowering site, a conditional whose selection is decided once
`check` and `extends` are lowered never lowers its losing branch: the branch
records zero interns, zero locator dereferences, zero nested dispatches
(including import and declaration resolution reached through lowering), zero
origin edges, and zero semantic fact reads. Selection runs through the one
oracle, `ProjectSemanticDispatch::conditional_branch_selection`, and infer
bindings minted from the `extends` pattern are visible only to the selected
true branch's lowering environment. An open selection lowers both branches and
dispatches the existing family so the `SemanticNodeData::Conditional` shell is
interned exactly as today; a distributive check that resolves to a union at
lowering time lowers a branch only if some member selects it, through the
existing `build_conditional` distributor and never through a second one.

At the instantiation site, the losing branch of a conditional decided after
substitution is neither substituted, traversed, nor re-dispatched, and no
selection runs earlier than the existing on-demand evaluation point:
instantiating a declaration whose body contains a conditional that nobody
demands performs no relation read for it. Lowering the parameterized body once
per declaration content, with both branches, remains the TE4 lower-once
contract and is not dead-operand work.

`SemanticQueryKey::Conditional` remains the one conditional family and
`conditional_branch_selection` the one oracle. Branch operands stay
materialized `SemanticNodeId` handles or a sealed content-free pairing of a
materialized handle with the pending positional substitution frame — the same
shape TE1's runtime-node arm plus positional substitution axis already seals,
and the shape TE4's demanded-key value forcing must reuse. Authored locator
operands are not required for this outcome. This charter accepts one
lowering-plus-instantiation cutover and contains no independently
dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_session/src/project_semantic_dispatch` and `crates/verter_session/src/semantic_query.rs`.
- Production files: `lower.rs`, `build.rs`, `substitute.rs`, `evaluate.rs`, `semantic_query.rs`, and `semantic_query_memo/family.rs`, plus small consumer updates as the sealed substitution pairing requires; tests are non-production. File, LOC, and package counts are sizing suggestions, not abort or rescope gates (maintainer direction, 2026-09-12). Correct handling in dispatch, projection, display, and other existing consumers takes precedence over the suggested inventory.
- Named boundaries: `SemanticQueryKey::Conditional`, `SemanticNodeData::Conditional`, `ProjectSemanticDispatch::build_conditional`, `conditional_branch_selection`, `conditional_infer_route`, `distributive_check_union_members`, `lower_type_expr_with_infer_factory`, `substitute_semantic_type_param`, `evaluate_deferred_semantic_node`, `SemanticQueryKey::Relate`, `InferBinderId`, `ConditionalSelect`, `InferBind`, TE1's materialized-handle operand arm and positional substitution axis, `ReadSetSignature`, and `SignatureAdmission`.
- Mutation boundary: lowering-site selection order, instantiation-site branch substitution deferral, conditional family identity only as far as a sealed pending-substitution frame requires, and exact tests. No public `TypeInfo`, native-checker, flow-product, relation-policy, truthiness, canonical-algebra, or wire change.

## Exact predecessor contracts

- **TE1:** implemented ledger row for sealed semantic operands and forcing boundary; it supplies the runtime-node arm and positional substitution identity used by the sealed pending pairing.
- **D3C:** implemented ledger row for the flow product worklist cutover; existing semantic dispatch and admission ownership remain unchanged.
- **TA1B:** implemented ledger row for canonical opaque composite payloads; all composite construction continues through that authority.
- **Existing TE2 dispatch boundary (#526):** already delivered selection before branch forcing, infer scoping, open-shell residual projection, distributivity ownership, and selection-observed dependency facts. This is preserved baseline behavior, not a predecessor or acceptance of the remaining cutover.
- **TE3:** implemented ledger row for “Projection and key-domain selective forcing”; ledger presence alone satisfies the predecessor. It supplies the force-request projection vocabulary and residual-path propagation that a forced winner is projected under; TE2 adds no walker-owned evaluator.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- **Lowering site.** After `check` and `extends` are lowered (the `extends` lowering mints the `Infer` declarations exactly as today), consult `conditional_branch_selection` before lowering either branch. Decided true: lower only `true_type` under the infer-extended environment and substitute the returned bindings through the shared commit path `build_conditional` uses today, so `ConditionalSelect`/`InferBind` origin edges and the decision counters have one owner. Decided false: lower only `false_type` in the original environment. Open: lower both and dispatch the family unchanged. The decided path may bypass the family memo only because its result is memoized by the enclosing `LowerLocator` and no other construction site can form that key without the losing node id; record this as the deletion of the eager route, naming `conditional_branch_selection` plus the shared commit path as the replacement.
- **Instantiation site.** The `Conditional` arm of substitution substitutes `check` and `extends` and then defers branch substitution rather than descending into both branches. The admissible representation is a content-free pairing of the materialized branch handle with the pending positional substitution frame (`(param_node, arg_node)` pairs, store-local like every node-keyed identity), carried in the conditional family identity so equivalent instantiations converge and so the on-demand decision in `evaluate.rs` forces and substitutes only the winner. A stored `TypeExpr`, closure, AST pointer, arbitrary env map, or source hash is not admissible. Selection must not run during substitution; the decision point stays the on-demand evaluator.
- **Distributivity.** A pending-substitution shell whose substituted check resolves to a union distributes through the existing `build_conditional` distributor; per-member keys share the pending frame and each member forces only its winner.
- **Dead-operand proof.** For the losing branch of a conditional decided at the lowering site: forcing attempts, locator dereferences, substitutions, nested dispatches (including relation, import, and declaration reads), semantic allocations/interns/origin edges, and semantic fact reads are exactly zero, and no dependency fact is added. For the losing branch of a conditional decided after instantiation: substitutions, traversals, nested dispatches, origin edges, and fact reads after the decision are exactly zero; the one parameterized-body intern per declaration content is excluded. Parse and shallow indexing are excluded.
- Cancellation is checked before `check`/`extends` lowering, before branch lowering, before the deferred force of a winner, and before admission. Cancelled, budgeted, recursive/unknown-relation, or partial work is typed `ReturnOnly` unless the correct result is an intentionally suspended complete carrier; no degraded candidate warms.
- Required fixtures include: a non-generic decided-true conditional whose false branch contains an unresolved import and an allocation-heavy generic; the mirror decided-false case; nested same-name infer binders decided at lowering; a generic conditional instantiated with a concrete argument whose losing branch contains an unresolved import, asserting zero post-decision substitution and no relation read until the conditional is demanded; a distributive conditional over a union under a pending substitution; repeated fresh/warm/incremental execution; and root `Expanded` preservation.

## Acceptance IDs and discriminating proof

- **TE2-AC1 — lowering-site selection before branch lowering:** a decided conditional lowered from authored syntax returns the existing semantic answer while the losing branch satisfies the full zero-work proof; a mutation restoring eager branch lowering must fail; one family and one oracle remain structurally evident.
- **TE2-AC2 — instantiation-site deferral:** after instantiation with a concrete argument, the losing branch is not substituted, traversed, or re-dispatched; no selection runs before the conditional is demanded; infer bindings remain exact and scoped to the selected true branch; distributive behavior, origin edges, open residual-path projection, and genuine root `Expanded` semantics are unchanged.
- **TE2-AC3 — admission and incremental equivalence:** fresh and incremental results, completeness, origin, and bytes match after edits to the check, the winning branch, and the formerly losing branch at both sites; a pending-substitution identity one-axis-distinguishes its frame without content or version data; cancelled/budgeted/partial work is `ReturnOnly` and never warm; strict self-root architecture is neither weakened nor refined.
- **TE2-AC4 — bounded work:** a decided conditional lowers or forces exactly one branch at each site; relation selection runs at most once per check-relevant substitution class; equivalent pending-substitution demands join the existing family memo; repeated warm requests do not grow candidates.
- Every new test must discriminate a plausible eager-lowering, eager-selection, infer-leak, residual-projection, or admission regression; reuse and table-drive existing conditional and infer fixtures.
- Test homes: co-located `project_semantic_dispatch` tests and `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete the eager lowering-site branch lowering and the unconditional four-subtree substitution descend for decided conditionals; each deletion names `conditional_branch_selection` plus `build_conditional`/the on-demand evaluator as the replacement.
- No second relation engine, truthiness classifier, distributivity planner, branch recipe graph, recursive demand walker, or conditional cache.
- No `SemanticRecipeId`, closures, AST pointers, `TypeExpr` operands, arbitrary env maps, source hashes, spans, or display text in conditional operand/query identity.
- No selection during substitution and no eager relation read for an undemanded conditional.
- No authored-operand cutover of the conditional family, no conditional-`infer` binder frame on TE1 authored identity, and no `verter_type_expr` locator vocabulary change: the ratified authored-identity limitations remain binding. No successor node may be introduced.
- No projection-context axis in `FamilyKey::Conditional` unless a branch force becomes context-dependent inside the family; conditional reduction is context-free today and the winner is projected under the consumer's own `ProjectPath` context.
- Do not implement TE4 or TE5 scope. Do not split this remaining contract into a successor node.

## Suggested sizing and semantic boundaries

- Suggested size: 800 production LOC, 8 production files, 2 related crates/packages. Additional required pairing consumers are permitted; report the actual footprint.
- LOC, file, and package numbers are not abort conditions. Stop if relation/distributivity ownership, public/wire shape, a general recipe graph, or a TE1 vocabulary change beyond the sealed pending-substitution pairing must change.
- Correctness budget: zero losing-branch semantic work at both sites, infer leakage, eager selection, stale publication, wrong branch, wrong-complete result, or warm degraded result.
- Performance budget: decided conditional branch-lowering count exactly 1 at the lowering site; post-decision losing-branch substitution count exactly 0; no relation read for an undemanded conditional; zero warm-candidate growth; no allocation/latency regression for equivalent selected work.

## Abort conditions

- Stop before mutation if any named predecessor lacks an implemented ledger row, if the lowering-site decision cannot reuse `conditional_branch_selection` without a second selector, or if the pending-substitution pairing cannot be sealed content-free on TE1's runtime-node arm plus positional substitution axis.
- Stop if preserving distributivity requires a second planner, if root `Expanded` semantics would change, or if the instantiation-site outcome would require selection during substitution.
- Abort on any dead-branch semantic activity, eager selection, infer leakage, stale/warm partial, candidate growth, or unexplained output/performance divergence.

## Targeted verification

1. Run discriminating lowering-site, instantiation-site, infer, distributivity, projection, cancellation, and dead-operand counter cases.
2. `cargo nextest run -p verter_session -p verter_semantic`
3. Run every final command in `targeted-domain` on the stable candidate and bind TE2-AC1–AC4 evidence/rationale in the review report.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial` and `conformance`. Reviews must inspect the lowering-site order, the substitution deferral, infer scoping, dead-operand counters, root/open behavior, and authority non-duplication. P0/P1 block; unresolved P2 follows the binding policy and otherwise blocks. Any material change invalidates affected verdicts. Final acceptance requires 2/2 clean PASS reports plus `targeted` confirmation.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Row presence is authoritative; locator metadata is never validated against Git or GitHub.
