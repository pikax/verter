<!-- unified-charter-v2
id=TE2
name=Conditional selective forcing
phase=rev11
train=rev11.type-evaluation
product=rev11
kind=implementation
semantic_role=delivery
class=foundational-authority
predecessors=TE1,D3C,TA1B
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

# TE2 — Conditional selective forcing

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Ratified rescope

This contract is the RESCOPE ratified by the architect ruling recorded in
`decisions/2026-09-09-te2-conditional-operand-abort.md` (2026-09-11). The
original contract moved the `SemanticQueryKey::Conditional` operand boundary
onto TE1 sealed authored operands and required a zero-intern dead-operand
proof over the losing branch's whole lifetime. Three source-verified
properties of the landed tree put that outside this node's boundary: forcing
an authored `ConditionalTrue` operand whose sibling `extends` declares an
`infer` re-lowers the whole enclosing conditional through the binder-crossing
locator route; TE1 identity has no conditional-`infer` binder frame; and
several conditional positions have no locator spelling without a
`verter_type_expr` vocabulary change. The excluded scope is owned by `TE2B`
and is not silently absorbed here.

Dead-operand accounting under this contract is attributable to the decided
conditional's own evaluation at the dispatch boundary. Lowering the
parameterized declaration body once per declaration content and the
substitution descend of an open shell are outside this node's claim; they are
`TE2B`'s subject.

## Independently acceptable outcome

Conditional evaluation decides through the existing relation/conditional
authority before any branch operand is forced, over the materialized branch
operands the `SemanticQueryKey::Conditional` family carries.
`SemanticQueryKey::Conditional` remains the canonical conditional dispatch
family and `ProjectSemanticDispatch::conditional_branch_selection` remains the
one branch-selection oracle; it consumes only `check` and `extends`, and every
relation read re-enters `SemanticQueryKey::Relate`.

A decided conditional's value is the winning branch node. The losing branch is
dead at the dispatch boundary: it receives no forcing attempt, no nested
dispatch, no relation read, no origin edge, and no semantic fact read, and it
contributes no semantic dependency fact — neither a read-set fact nor an
observed self-root — to the result. When infer bindings are produced they are
substituted only into the selected true branch under exact `InferBinderId`
identity; no binding reaches the false branch, an outer same-name binder, or a
sibling conditional. When selection is open, a `SemanticNodeData::Conditional`
shell is preserved and a residual `ProjectPath` is pushed into both branch
operands through the shared `ProjectPath` family without whole-surface
enumeration. A genuine query-root `Expanded` demand keeps the current contract:
an open conditional materializes both branches because both are the requested
result; a decided conditional still yields only the winner.

Existing conditional dispatch continues to own distributivity. A distributed
conditional's dependency facts follow the per-member selections: the parent
roots directly on the distributive `check`, the resolved union surface it
distributed over, and `extends`, and inherits branch facts from the per-member
`SemanticQueryKey::Conditional` reads. When every member selects the same
branch, the other branch contributes no dependency fact. This charter accepts
one conditional dependency-fact cutover at the dispatch boundary and contains
no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_session/src/project_semantic_dispatch`.
- Production files: `build.rs`; tests are non-production and live in `project_semantic_dispatch/tests.rs`.
- Named boundaries: `SemanticQueryKey::Conditional`, `SemanticNodeData::Conditional`, `ProjectSemanticDispatch::build_conditional`, `conditional_branch_selection`, `conditional_infer_route`, `distributive_check_union_members`, `observed_self_roots_from_nodes`, `SemanticQueryKey::Relate`, `InferBinderId`, `ConditionalSelect`, `InferBind`, `ReadSetSignature`, and `SignatureAdmission`.
- Mutation boundary: the observed self-roots and dependency facts of decided and distributed conditional results, and exact tests. No key or family-identity change, no lowering change, no public `TypeInfo`, native-checker, flow-product, relation-policy, truthiness, canonical-algebra, or wire change.

## Exact predecessor contracts

- **TE1:** implemented ledger row for “Sealed semantic operands and forcing boundary”; ledger presence alone satisfies the predecessor. This node does not consume the force capability or mint an operand; it keeps the materialized `SemanticNodeId` operands the conditional family already carries. TE1's authored vocabulary is consumed by `TE2B`, not here.
- **D3C:** implemented ledger row for “Product worklist cutover”; ledger presence alone satisfies this ordering edge. This node consumes only the pre-existing shared `SemanticQueryKey::Relate`, `InferenceSession`, and `InferBinderId` authorities and no D3R/D3I/D3P/D3C flow, product, worklist, admission, or budget API.
- **TA1B:** implemented ledger row for “Canonical composite payload and construction-site closure”; ledger presence alone satisfies the predecessor. The distributed per-member union is produced through the sealed canonical algebra route; no raw derived composite constructor returns.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- `build_conditional` roots a DECIDED result (relation selection to `True`/`False`, and the binding-producing infer selection, which is always decided true) on `check`, `extends`, and the WINNER only. The deferred shell publishes both branch references in its value and keeps the four-node root set.
- The distributive early return roots on `check`, the resolved union surface returned by `distributive_check_union_members`, and `extends`; the branch files arrive as member-observed facts through the memo's nested fact rail. A global alias over a file-scoped union still roots the union's file.
- Staleness in the other direction is impossible: the memo key carries both branch node ids, so an edit that changes the losing branch mints a different id and a different key rather than serving a stale hit. The change removes rejections of values that never read the rejecting file and adds no reuse of a value whose inputs moved.
- Selection, infer substitution into the true branch, the open shell, the walker's residual-path push into both branches, root `Expanded`, and distributivity keep their existing behavior; existing cancellation and `ReturnOnly` rails apply unchanged.

## Acceptance IDs and discriminating proof

- **TE2-AC1 — select before branch force at the dispatch boundary:** a decided true/false conditional returns the winning branch while the losing branch receives no force, nested dispatch, relation read, origin edge, or fact read, and contributes no self-root. Restoring the four-node root set on a decided result must fail `decided_conditional_roots_only_on_check_extends_and_the_winner`; the decided-false leg mirrors the decided-true leg so the proof cannot pass by dropping one fixed branch. One `Conditional` family and one relation authority remain structurally evident (`closed_conditional_selects_and_emits_edges`, `closed_conditional_does_not_materialise_losing_branch_body`).
- **TE2-AC2 — infer, distributivity, and open projection:** infer bindings substitute only into the selected true branch through the shared oracle (`bare_infer_extends_selects_true_through_the_shared_oracle` and the existing infer suites); a distributed conditional's dependency facts follow member selections for all-true, all-false, mixed, and open member sets, for a raw union check and a global alias over it (`distributed_conditional_dependencies_follow_member_selections`); an open conditional with a residual path projects that path through both branches through the walker; root `Expanded` semantics are unchanged.
- **TE2-AC3 — admission and incremental equivalence:** a warm repeat of the distributed parent hits without rebuilding members and yields the same read set; strict self-root validation is neither weakened nor refined; cancelled, budgeted, or partial work stays `ReturnOnly` and never warms.
- **TE2-AC4 — bounded work:** selection runs once per conditional dispatch and consumes only `check`/`extends`; a decided conditional forces zero branches at the boundary because the winner node id is returned as-is; repeated warm requests do not grow candidates beyond the existing family cap.
- Every test discriminates a plausible dead-branch-root, infer-leak, or admission regression; existing conditional and infer fixtures are reused.
- Test homes: co-located `project_semantic_dispatch` tests and `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- The unconditional four-node observed-self-root set on the decided and distributed arms is deleted; the replacement is the selection-observed root set in `build_conditional`.
- No second relation engine, truthiness classifier, distributivity planner, branch recipe graph, recursive demand walker, or conditional cache.
- No `SemanticRecipeId`, closures, AST pointers, `TypeExpr` operands, arbitrary env maps, source hashes, spans, or display text in conditional query identity.
- Never infer through an unselected branch, substitute a losing branch at the boundary, or read semantic facts from a dead branch. Never treat parse/shallow indexing as proof of semantic forcing.
- Never weaken genuine root `Expanded`, the five query modes, canonical algebra, existing origin taxonomies, or relation/inference-session ownership.
- Do not implement `TE2B`, TE4, or TE5 scope here: no lowering-site or instantiation-site change, no authored branch operand, no projection-context axis in `FamilyKey::Conditional`.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or if relation/distributivity ownership, public/wire shape, or a general recipe graph must change.
- Correctness budget: zero losing-branch dispatch-boundary work, infer leakage, stale publication, wrong branch, wrong-complete result, or warm degraded result.
- Performance budget: zero warm-candidate growth; no allocation/latency regression for equivalent selected work.

## Abort conditions

- Stop before mutation if any predecessor lacks an implemented ledger row or if a decided result could serve a stale hit after a losing-branch edit under the existing key shape.
- Abort on any dead-branch dispatch-boundary activity, infer leakage, stale/warm partial, candidate growth, or unexplained output/performance divergence.

## Targeted verification

1. Run the discriminating conditional root, infer, distributivity, and walker projection cases.
2. `cargo nextest run -p verter_session -p verter_semantic`
3. Run every final command in `targeted-domain` on the stable candidate and bind TE2-AC1–AC4 evidence/rationale in the review report.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial` and `conformance`. Reviews must inspect the dependency-fact cutover, infer scoping, root/open behavior, and authority non-duplication. P0/P1 block; unresolved P2 follows the binding policy and otherwise blocks. Any material change invalidates affected verdicts. Final acceptance requires 2/2 clean PASS reports plus `targeted` confirmation.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Row presence is authoritative; locator metadata is never validated against Git or GitHub.
