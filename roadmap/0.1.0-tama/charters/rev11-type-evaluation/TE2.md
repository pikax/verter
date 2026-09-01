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

## Independently acceptable outcome

Make conditional evaluation decide through the existing relation/conditional authority
before forcing branch operands. `SemanticQueryKey::Conditional` remains the canonical
conditional dispatch family and `ProjectSemanticDispatch::conditional_branch_selection`
remains the one branch-selection oracle; TE2 changes its operand boundary from four
already-materialized branch nodes to TE1's sealed operands plus the force request's one
complete existing `ProjectionReductionContext` (`mode`, `demand`, `provenance`,
`merge_role`, `vue_heritage_policy`). The force boundary combines operand identity with
that unchanged request context exactly once in existing query dispatch/family identity;
the operands do not own, reconstruct, default, duplicate, or store a second context.
When selection is true or false, only the winning branch is forced. When infer bindings
are produced, they exist only in the selected true operand's exact binder/substitution
frame. When selection is open, the conditional stays suspended and a requested residual
projection is pushed into both branch operands without forcing unrelated branch roots.

The existing conditional dispatch continues to own distributivity. TE2 does not add a
second distributivity planner, assignability check, or relation policy. A genuine
query-root `Expanded` demand preserves the current semantic contract: an open conditional
materializes both branches because both are the requested full result; a decided
conditional still forces only the selected branch. This charter accepts one complete
conditional cutover and contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_session/src/project_semantic_dispatch` and `crates/verter_session/src/semantic_query.rs`.
- Production files: the six named files `lower.rs`, `build.rs`, `evaluate.rs`, `walk.rs`, `relation.rs`, and `semantic_query_memo/family.rs`, plus at most two additional production files if the closed TE1 request vocabulary requires them; tests are non-production. A ninth production file exceeds the target and requires explicit rescope; the 12-file mandatory ceiling is not advance permission.
- Named boundaries: `SemanticQueryKey::Conditional`, `SemanticNodeData::Conditional`, `ProjectSemanticDispatch::build_conditional`, `conditional_branch_selection`, `conditional_infer_route`, `SemanticQueryKey::Relate`, `InferenceSession`, `InferBinderId`, `ProjectionReductionContext`, `ProjectPath`, `ConditionalSelect`, `InferBind`, TE1's sealed operand and force capability, `ReadSetSignature`, and `SignatureAdmission`.
- Mutation boundary: conditional operand representation, conditional family identity, lowering/structural emission, conditional build/evaluation/path-walk integration, and exact tests. No public `TypeInfo`, native-checker, flow-product, relation-policy, truthiness, canonical-algebra, or wire change.

## Exact predecessor contracts

- **TE1:** implemented ledger row for “Sealed semantic operands and forcing boundary”; ledger presence alone satisfies the predecessor. TE1 supplies the content-free authored operand identity, store-local materialized handle arm, the request-owned complete five-axis `ProjectionReductionContext`, its exactly-once unchanged combination into query dispatch/family identity, and the sole force capability. TE2 may specialize the closed force-request vocabulary but may not reconstruct/default/duplicate/store a second context or bypass it.
- **D3C:** implemented ledger row for “Product worklist cutover”; ledger presence alone satisfies this architect-mandated ordering edge because D3C is the ledger-visible completion/ordering fence for the atomic D3R/D3I/D3P/D3C landing. TE2 continues to consume the pre-existing shared `SemanticQueryKey::Relate`, `InferenceSession`, and `InferBinderId` authorities. It consumes no D3R nominal `Identity`/`Comparable` outcome, no D3I `FlowBinding` identity, and no D3P/D3C flow, product, worklist, admission, or budget API, including `FlowProductStore`, `FlowReturn`, and `FlowDischargeReport`.
- **TA1B:** implemented ledger row for “Canonical composite payload and construction-site closure”; ledger presence alone satisfies the predecessor. Every distributed or suspended conditional composite produced here uses the sealed canonical algebra route; no raw derived union/intersection constructor returns.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- Replace the live eager shape in `lower.rs` where `true_type` and `false_type` are lowered before `SemanticQueryKey::Conditional` dispatch. The query key must carry sealed operands; the force request supplies the one complete existing five-axis `ProjectionReductionContext`, which TE1 combines unchanged with operand identity exactly once in query dispatch/family identity. Neither operand nor a second field reconstructs, defaults, duplicates, or stores that request context; source contents and version hashes remain absent from identity.
- Force the check and extends operands only as far as the existing selection oracle requires. Every nested relation read re-enters `SemanticQueryKey::Relate`; the O(tag) prefilter remains internal to that authority and never becomes a second answer.
- **Decided true:** force only the true operand under the committed infer bindings. The false operand is dead. Infer declarations/references keep exact `InferBinderId`; the selected true operand alone receives the extended substitution frame. No binding leaks to the false operand, an outer same-name binder, or a sibling conditional.
- **Decided false:** force only the false operand in the original environment. The true operand and every infer body beneath it are dead.
- **Open/deferred:** preserve a `SemanticNodeData::Conditional` shell. For a non-empty `ProjectPath`, propagate only the residual path/demand into each branch operand and preserve the check/extends operands as the suspended decision inputs. Do not whole-expand either branch merely to project a leaf.
- **Genuine root Expanded:** preserve the current query-mode contract. If selection stays open and the caller requested the full root, both branch operands are demanded because both belong to the result. This is not classified as dead-operand work. Decided root Expanded still touches only the winner.
- Existing conditional distributivity remains in `build_conditional`; distributive arms call the same conditional family and canonical algebra. TE2 adds no pre-expansion distributor or request-local branch walker.
- **Dead-operand proof:** for the losing branch of a decided conditional, attributable semantic counters must be exactly zero: forcing attempts, locator dereferences, substitutions, nested dispatches (including relation reads), semantic allocations/interns/origin edges, and semantic fact reads. Dead operands add no semantic dependency facts. Parse and shallow indexing are explicitly excluded.
- Cancellation is checked before check/extends force, before branch force, and before admission. Any cancellation, budget, recursive/unknown relation, or partial nested force is typed `ReturnOnly` unless the correct semantic result is an intentionally suspended complete carrier under existing rules; no degraded candidate warms.
- Required fixtures include: false branch containing an unresolved import and allocation-heavy generic; true branch containing the same for false selection; nested same-name infer binders; open conditional `['a']['b']` projection; distributive conditional over a union; repeated fresh/warm/incremental execution; and root `Expanded` preservation.

## Acceptance IDs and discriminating proof

- **TE2-AC1 — select before branch force:** a decided true/false conditional returns the existing semantic answer while the losing branch satisfies the full zero-work proof. A mutation that restores eager branch lowering must fail. The one `Conditional` family and one relation authority remain structurally evident.
- **TE2-AC2 — infer, distributivity, and open projection:** prove infer bindings are exact and scoped only to the selected true operand; existing distributive behavior and origin edges are preserved; an open conditional with a residual path projects that path through both branches without whole-surface enumeration; genuine root `Expanded` semantics remain unchanged.
- **TE2-AC3 — admission and incremental equivalence:** fresh and incremental results, completeness, origin, and bytes match after edits to check, winning branch, and formerly losing branch. Conditional family tests one-axis-distinguish the request's `mode`, `demand`, `provenance`, `merge_role`, and `vue_heritage_policy` without an operand-owned or reconstructed context. A dead operand adds no semantic dependency facts and performs no semantic work, but an ordinary same-owner edit may conservatively reject the candidate through strict self-root validation. After any such rejection, recomputation must match fresh and must still record zero dead-operand semantic work. When selection flips, the new winner is forced and its current facts are observed. Cancelled/budgeted/partial work is `ReturnOnly` and never warm; this node does not weaken or refine strict self-root architecture.
- **TE2-AC4 — bounded work:** relation selection runs at most once per check-relevant substitution class; a decided conditional forces one branch; an open residual-path request forces only branch work necessary for that path; concurrent identical conditionals join the existing family memo; repeated warm requests do not grow candidates.
- Every new test must discriminate a plausible eager-forcing, infer-leak, residual-projection, or admission regression. Reuse/table-drive existing conditional and infer fixtures where possible.
- Test homes: co-located `project_semantic_dispatch` tests and `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete the eager conditional branch-lowering/materialization route and every conditional-only bypass that forces branch bodies before selection; each deletion names TE1 force plus `build_conditional` as the replacement.
- No second relation engine, truthiness classifier, distributivity planner, branch recipe graph, recursive demand walker, or conditional cache.
- No `SemanticRecipeId`, closures, AST pointers, `TypeExpr` operands, arbitrary env maps, source hashes, spans, or display text in conditional operand/query identity.
- Never infer through an unselected branch, substitute a losing branch, or read semantic facts from a dead branch. Never treat parse/shallow indexing as proof of semantic forcing.
- Never weaken genuine root `Expanded`, the five query modes, canonical algebra, existing origin taxonomies, or relation/inference-session ownership.
- No public/wire, native-checker, flow-product, call-resolution, truthiness, display, or component-meta policy expansion.
- Do not implement TE3–TE5 scope. A second independently acceptable outcome requires a DAG amendment.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or if relation/distributivity ownership, public/wire shape, or a general recipe graph must change.
- Correctness budget: zero losing-branch semantic work, infer leakage, stale publication, wrong branch, wrong-complete result, or warm degraded result.
- Performance budget: decided conditional branch-forcing count exactly 1; dead-branch counters exactly 0; no additional relation selection per equivalent substitution class; zero warm-candidate growth; no allocation/latency regression for equivalent selected work.

## Abort conditions

- Stop before mutation if any predecessor lacks an implemented ledger row, conditional selection cannot occur without first materializing both branches under the existing relation authority, or exact infer scoping cannot be represented by TE1 identity.
- Stop if preserving distributivity requires a second planner or if root `Expanded` semantics would change.
- Abort on any dead-branch semantic activity, infer leakage, stale/warm partial, candidate growth, or unexplained output/performance divergence.

## Targeted verification

1. Run discriminating conditional, infer, distributivity, projection, cancellation, and dead-operand counter cases.
2. `cargo nextest run -p verter_session -p verter_semantic`
3. Run every final command in `targeted-domain` on the stable candidate and bind TE2-AC1–AC4 evidence/rationale in the review report.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial` and `conformance`. Reviews must inspect the cumulative conditional cutover, infer scoping, dead-operand counters, root/open behavior, and authority non-duplication. P0/P1 block; unresolved P2 follows the binding policy and otherwise blocks. Any material change invalidates affected verdicts. Final acceptance requires 2/2 clean PASS reports plus `targeted` confirmation.

## Trusted implementation ledger

Before squashing or review, the implementation patch adds one `[[implemented]]` row to `authority/state/implemented.toml` with the node ID, planned squash commit message, approximate timezone-bearing date, and optional pull-request number. Row presence is authoritative; locator metadata is never validated against Git or GitHub.
