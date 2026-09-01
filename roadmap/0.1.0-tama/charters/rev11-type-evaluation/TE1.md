<!-- unified-charter-v2
id=TE1
name=Sealed semantic operands and forcing boundary
phase=rev11
train=rev11.type-evaluation
product=rev11
kind=implementation
semantic_role=delivery
class=foundational-authority-substrate
predecessors=B6,C1
owner=rev11.type-evaluation:the sole demand-selected semantic operand forcing authority inside the existing SemanticQuery graph
conflict_domains=semantic_authority
resource_class=rust-mixed
review_profile=architecture-3
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
charter=charters/rev11-type-evaluation/TE1.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# TE1 — Sealed semantic operands and forcing boundary

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Introduce the representation and capability boundary required for demand-selected
semantic operand evaluation, without yet changing conditional, projection, or mapped
operator semantics. The final owner is the **sole demand-selected semantic operand
forcing authority inside the existing `SemanticQueryKey` →
`ProjectSemanticDispatch::execute` → `SemanticGraphStore` graph**. An operand is either
an already materialized `SemanticNodeId` — a store/generation-local runtime handle,
never persisted or used as cross-store/authored identity — or a content-free authored
locator paired with the exact sealed lexical environment, substitution, binder, and
split-env axes needed to reproduce its authored semantic meaning. The force request,
not the operand, owns the one complete existing `ProjectionReductionContext`:
`(mode, demand, provenance, merge_role, vue_heritage_policy)`. One capability-limited
`force(operand, request_context)` boundary combines the operand identity with that
complete request context exactly once in existing query dispatch/family identity; it
never reconstructs or defaults over that context, duplicates it, or stores a second
copy. It then materializes through existing `LowerLocator`, `Instantiate`, and projection
queries. It is not a second graph, resolver, recipe language, or generic evaluation
trait.

Acceptance requires the operand carrier, forcing capability, and their identity and
admission rules to land together: an authored operand cannot be forced without its
exact lexical/binder/substitution identity; content or version data remains on ordinary
fact validation rather than in reusable identity; the materialized-node arm merges the
node's provenance/read-set roots into the produced candidate; and callers cannot dereference a
locator, resolve a declaration, instantiate a body, or project a new semantic node
through the operand API except by invoking the one forcing boundary. This charter
accepts one substrate/authority boundary and contains no independently dispatchable
subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_session/src` and the content-free locator vocabulary in `crates/verter_type_expr/src` only when an existing locator cannot express an exact typed child.
- Production files: the five named files `semantic_query.rs`, `semantic_query_memo/family.rs`, `project_semantic_dispatch/mod.rs`, `project_semantic_dispatch/locator_shape.rs`, and `project_semantic_dispatch/lower.rs`, plus at most three narrowly scoped new modules under `project_semantic_dispatch` or `semantic_query` for the sealed operand/capability. A ninth production file exceeds the target and requires explicit rescope; the 12-file mandatory ceiling is not advance permission.
- Named boundaries: `SemanticQueryKey`, `ProjectSemanticDispatch::execute`, `SemanticGraphStore::execute_cooperative`, `SemanticNodeId`, `AuthoredBodyLocator`, `TypeArgLocator`, `LowerLocator`, `Instantiate`, `ProjectPath`, the complete existing `ProjectionReductionContext::{mode,demand,provenance,merge_role,vue_heritage_policy}`, `InferBinderId`, `MapperKey`, `ReadSetSignature`, and `SignatureAdmission`.
- Required API shape: a closed operand enum and a closed force-request type carrying one complete existing `ProjectionReductionContext`; the only public-to-the-owner operation is semantically equivalent to `force(operand, request_context)`. Exactly one adapter combines sealed authored operand identity with that request-owned context in existing query dispatch/family identity. It never reconstructs or defaults any of the five context axes, duplicates them, or stores a second context. Exact names may follow repository style, but a generic trait implemented by arbitrary evaluators is forbidden.
- Mutation boundary: only the named surfaces and APIs above. This node does not change operator results, public `TypeInfo`, native-checker behavior, flow semantics, relation semantics, truthiness, canonical algebra, or wire contracts.

## Exact predecessor contracts

- **B6:** implemented ledger row for “PreparedCarrier direct batch and direct-core closure”; ledger presence alone satisfies the predecessor. It supplies the direct prepared-carrier and core closure on which locator-backed semantic operands depend. Locator consumption must remain direct and must not restore an indirect carrier batch.
- **C1:** implemented ledger row for “ModuleResolverCore convergence and non-flow semantic basis”; ledger presence alone satisfies the predecessor. It supplies the one non-flow semantic facade and module-resolution core; forcing must enter that owner rather than creating a second import, alias, or declaration resolver.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- Seal operand identity as either an existing graph node or `(authored locator, sealed lexical environment identity, substitution identity, binder identity, required split env dimensions, any non-ProjectionReductionContext identity that is truly authored operand meaning)`. The authored arm is a locator for typed syntax, never its contents. All five `ProjectionReductionContext` axes — `mode`, `demand`, `provenance`, `merge_role`, and `vue_heritage_policy` — are absent from the sealed operand and supplied together only by each force request.
- The `SemanticNodeId` arm is valid only as a store/generation-local runtime handle in the existing node-keyed family identity. It is never persisted, serialized, compared cross-store/generation, or treated as durable authored identity. Forcing that arm must merge every provenance edge, read-set fragment, and self-root required by the referenced node into the candidate being built.
- Keep reusable authored identity content-free. `whole_hash`, content hash, source bytes, spans, `fact_dep_signature`, `ReadSetSignature`, independently supplied allocation ordinals/ranking, and graph-allocation order as durable authored identity are forbidden. Existing store-local node-keyed families may key on `SemanticNodeId`. The cold locator build re-sources the live artifact; every consumed fact and self-root enters the ordinary `ReadSetSignature` and is validated on every warm read.
- Exact binder/substitution identity is semantic meaning. Same spelling under different `NodeScopeId`, `InferBinderId`, mapper binder, default/constraint frame, or substitution cannot alias. Equivalent callers with the same sealed identity converge on the same existing family memo.
- The force capability may only dispatch existing `LowerLocator`, `Instantiate`, `ProjectPath`/`ProjectMember`/`IndexedAccess`, `KeyOf`, `MappedType`, `Conditional`, relation, or canonical-algebra queries as the requested demand requires. It never recursively interprets `TypeExpr` itself and never reads OXC.
- Cancellation and budget checks occur before locator dereference and before every nested dispatch initiated by forcing. A cancelled, budget-exceeded, recursive, unstable, missing, partial, or signature-overflow force is typed and `ReturnOnly`; no result, origin edge, reverse index, or candidate may warm.
- Parse and shallow indexing remain outside the dead-operand accounting because they are owner-file preparation, not semantic forcing. The required semantic counters begin at the operand boundary.
- Required discriminating evidence: authored content-free identity equality/difference cases; store/generation-local node-handle confinement plus provenance/read-set-root merging; wrong-scope and wrong-binder non-aliasing; one-axis difference tests for every request-owned `ProjectionReductionContext` field (`mode`, `demand`, `provenance`, `merge_role`, `vue_heritage_policy`) with exactly one unchanged-context combination into query dispatch/family identity; cancellation before first locator dereference; ordinary read-set invalidation after a locator dependency changes; and a compile/capability proof that a consumer cannot force by dereferencing or dispatching through the sealed operand internals.

## Acceptance IDs and discriminating proof

- **TE1-AC1 — sealed sole-owner substrate:** all operand materialization through the new carrier reaches one capability-limited forcing boundary inside the existing semantic query graph. Prove the authored/node variants and force-request vocabulary are closed, the request alone owns one complete existing `ProjectionReductionContext`, and the boundary combines it unchanged with operand identity exactly once in query dispatch/family identity. One-axis tests must distinguish `mode`, `demand`, `provenance`, `merge_role`, and `vue_heritage_policy`. Prove no external caller can inspect authored content, mint an arbitrary environment map, reconstruct/default/duplicate/store a second context, or supply an alternate evaluator. A compiler/capability proof is preferred over a name-keyed source scanner.
- **TE1-AC2 — exact authored identity and local node handles:** prove same locator plus identical sealed scope/substitution/binder/split-env and other legitimate non-context authored identity axes dedups while one-axis differences never alias; prove `SemanticNodeId` stays store/generation-local and its provenance/read-set roots merge into the candidate; edit invalidation occurs through `ReadSetSignature`/self-root validation, not key churn. Prove a locator re-sources current content at cold compute and a warm candidate whose dependency fact changed is rejected.
- **TE1-AC3 — cancellation and partial admission:** cancellation before force causes zero locator dereferences, nested semantic dispatches, semantic allocations, and semantic fact reads; cancellation/budget/partiality discovered during force returns typed `ReturnOnly` and warms nothing. Fresh and incremental answers are identical after a dependency edit.
- **TE1-AC4 — bounded work:** one cold force performs at most one direct dereference of that selected operand's own locator and one winning cold build for its key. Transitively demanded operands/subqueries are charged and bounded independently per semantic key; they do not count against or hide inside the selected operand's direct-dereference bound. Concurrent identical callers join the existing cooperative memo; warm repetition does not grow the candidate family. Use existing semantic statistics/test hooks or bounded inspection, and prove no second in-flight table or request-local memo was added.
- Every new test must name a plausible regression boundary and fail against the pre-change capability/identity shape. Do not add prose mirrors, universal scanners, or non-discriminating quota tests.
- Test homes: `crates/verter_session/tests/cases` and co-located `project_semantic_dispatch`/`semantic_query` unit tests.

## Deletions and forbidden designs

- Delete or make unreachable any operand-specific locator dereference or semantic dispatch path introduced during migration that bypasses the sealed force capability; every deletion names the surviving authority route.
- No `SemanticRecipeId`, general recipe graph, instruction tape, recipe cache, parallel resolver, or per-consumer evaluator. There is one semantic query graph.
- No closures, function pointers, trait objects, AST/OXC references, borrowed parser arenas, `TypeExpr` bodies, source slices, spans, display text, arbitrary environment maps, or generic `Force`/`Evaluate` trait in operand storage.
- No public or wire expansion of `TypeInfo`; no native-checker, flow, relation, truthiness, canonical-algebra, module-resolution, or display-policy changes.
- No dual-running authority, compatibility fallback, source/string semantic recovery, test-only production bypass, unqualified identity, global policy boolean, or unbounded cache.
- Do not implement TE2–TE5 semantics in this node. Discovery of a second independently acceptable outcome requires a DAG amendment before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or if a new general graph/IR, public/wire change, unsafe code, or independent cache subsystem appears necessary.
- Correctness budget: zero identity aliasing, stale publication, wrong-complete result, missing dependency fact, or warm partial/cancelled result.
- Performance budget: zero additional parse or shallow-index passes; at most one direct dereference of the selected operand's own locator and one cold winner for that key, with transitive subqueries independently bounded per semantic key; zero semantic work for a force cancelled before entry; zero warm-family growth under repeated identical demand. Equivalent-work, allocation, and retained-candidate counters may not regress.

## Abort conditions

- Stop before mutation if B6 or C1 lacks an implemented ledger row, the live semantic graph cannot host the forcing boundary, or exact authored operand identity would require stored source/AST content.
- Stop if correctness would require a second resolver, recipe graph, arbitrary closure, public/wire change, or silently widened relation/truthiness/algebra authority.
- Abort on unexplained output, cancellation, allocation, latency, candidate-retention, or fresh/incremental divergence; do not record it as local residue.

## Targeted verification

1. Run the new operand identity/capability/cancellation unit and integration cases.
2. `cargo nextest run -p verter_session -p verter_semantic -p verter_type_expr`
3. Run every final command in the bound `targeted-domain` profile on the stable review candidate. Bind the selected TE1-AC1–AC4 evidence and rationale in the review report.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, and `architecture-specialist`. Review must specifically challenge second-graph risk, content-bearing identity, capability bypass, and cache/admission completeness. P0/P1 block. A P2 must have a named owner under the binding review policy or it blocks. Any material change invalidates affected verdicts. Final acceptance requires 3/3 current-round clean PASS reports plus `independent-full` confirmation.

## Trusted implementation ledger

Before squashing or review, the implementation patch adds one `[[implemented]]` row to `authority/state/implemented.toml` with the node ID, planned squash commit message, approximate timezone-bearing date, and optional pull-request number. Row presence is the implementation fact; locator metadata is never validated against Git or GitHub.
