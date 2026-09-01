<!-- unified-charter-v2
id=TE5
name=Selective forcing convergence and bypass deletion
phase=rev11
train=rev11.type-evaluation
product=rev11
kind=implementation
semantic_role=delivery
class=foundational-authority-closure
predecessors=TE4
owner=rev11.type-evaluation:the sole demand-selected semantic operand forcing authority inside the existing SemanticQuery graph
conflict_domains=semantic_authority,semantic_cache_store
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
charter=charters/rev11-type-evaluation/TE5.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# TE5 — Selective forcing convergence and bypass deletion

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Close the train as one structurally unique authority. Every selective operator operand
— conditional, `ProjectPath`/`ProjectMember`, indexed access, `keyof`, mapped type,
generic instantiation, and applicable utility — is sealed and forced only through the
TE1 boundary inside the existing `SemanticQueryKey` →
`ProjectSemanticDispatch::execute` → `SemanticGraphStore` graph. All eager or ad hoc
selective-operator bypasses are deleted or made unrepresentable. No second graph,
resolver, instruction/recipe layer, recursive demand walker, request-local memo, or
consumer-specific evaluator remains selectable.

TE1–TE4 already own the family-specific identity, satisfaction, validation,
cancellation/admission, and retention behavior needed by their operators. TE5 does not
create or redesign that cache system. Its implementation task is the bounded structural
route switch: delete residual eager/ad hoc entrances, make the TE1 capability the only
selectable semantic-evaluation entrance, and enforce across families that the already-
owned contracts remain connected — recorded materialized `(path, point)` satisfaction,
ordinary fact/self-root validation, cancellation before force/admission, `ReturnOnly`
partiality, bounded family retention, fresh/incremental parity, and zero warm growth.
This implementation authority-closure must finish before D8 certifies complete-result
admission. The charter accepts one cutover outcome and contains no independently
dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_session/src` only; TE5 consumes the semantic carrier/locator types landed by TE1–TE4 without changing their owners.
- Production files: at most these eight existing files — `semantic_query.rs`, `semantic_query_memo/family.rs`, and `project_semantic_dispatch/{mod,lower,build,evaluate,walk,raise}.rs` — plus non-production tests. Touching a ninth production file exceeds the target and requires explicit rescope; the 12-file mandatory ceiling is not advance permission.
- Named boundaries: TE1's sealed operand and force capability; `SemanticQueryKey` selective families; `ProjectSemanticDispatch::execute`; `SemanticGraphStore::execute_cooperative`; `FamilyKey`; `cached_satisfies`; recorded materialized points; `ProjectionMode`; `ProjectionReductionContext`; `ReadSetSignature::validate_with_self_roots`; `StoreViewValidationToken`; `SignatureAdmission`; `ReturnOnly`; per-family `candidate_cap`; candidate eviction/promotion; `SemanticGraphStats`.
- Mutation boundary: residual route switch/bypass deletion, capability/exhaustiveness enforcement, and cross-family assertions that the family-specific identity/satisfaction/validation/admission/retention work already owned by TE1–TE4 remains connected. TE5 adds no new family key, candidate store, admission rail, retention policy, or cache subsystem. Public/native/wire/flow/relation/truthiness/algebra/display meanings remain unchanged.

## Exact predecessor contracts

- **TE4:** implemented ledger row for “Mapped and generic selective forcing”; ledger presence alone satisfies the predecessor. Through TE4's transitive closure, TE5 receives TE1's sealed force capability, TE2's select-before-force conditional semantics and infer scoping, TE3's path/key-domain selective forcing, TA1B's canonical composites, and TE4's demanded-key-only mapped/generic semantics. TE5 closes and enforces those outcomes; it does not redesign them.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- Inventory every production entrance that can evaluate a conditional, projection, indexed access, `keyof`, mapped type, generic instantiation, or utility operand. Classify it exhaustively as structural carrier emission, TE1 force, or a carrier-semantics exception that performs no semantic evaluation. Every semantic-evaluation entrance outside TE1 force is deleted.
- Enforcement is structural: closed enums/capabilities and exhaustive wildcard-free dispatch where Rust can enforce it, plus compile-fail/capability proof at crate boundaries. A spelled function/path grep is not the primary authority proof and cannot legitimize a residual bypass.
- Cross-family enforcement verifies the identities already landed by TE1–TE4: authored identities are content-free and complete over semantic subject, locator, exact scope/binder/substitution/default/constraint identity and split env axes; the materialized `SemanticNodeId` arm remains a store/generation-local runtime handle in existing node-keyed families and merges its provenance/read-set roots. The force request alone owns the complete existing `ProjectionReductionContext` (`mode`, `demand`, `provenance`, `merge_role`, `vue_heritage_policy`), which the TE1 boundary combines unchanged with operand identity exactly once in query dispatch/family identity; no operand or route reconstructs, defaults, duplicates, or stores a second context. Source bytes, spans, content/version hashes, `fact_dep_signature`, independently supplied allocation ordinals/ranking, and `ReadSetSignature` remain forbidden in durable authored keys.
- Candidate reuse requires `cached_satisfies` over a **recorded materialized `(path, point)` actually produced** by the candidate and `ReadSetSignature.validate_with_self_roots` against the caller's live view. Nominal slot/mode, enum rank, `validated_at_generation`, or bare generation equality is never a validity/satisfaction oracle.
- Backfill remains directional and evidence-limited. A whole surface may backfill only members/paths it actually materialized; a narrow result never claims siblings or whole-surface completion; `Shallow` never satisfies `Navigate` by rank. `Skeleton` remains separate absent a typed equivalence proof.
- Cancellation is checked before every force and immediately before root/member candidate admission. Cancelled, budgeted, unstable, recursive, missing/partial, signature-overflow, or fact-invalid work is `ReturnOnly`; it publishes no candidate, origin edge, reverse index, or deferred member batch. No post-cancellation member backfill is allowed.
- Per-family retention remains bounded (`candidate_cap`, including the existing larger inference/substitution-heavy families). At cap, valid publication follows existing invalid-first then least-recently-validated-hit eviction. Identical warm requests do not append candidates; concurrent equal cold requests share the existing family flight.
- **Unified dead-operand proof:** for every dead conditional branch, unrelated projection sibling member/value, proven non-contributing intersection arm, unrelated mapped key, and remap-dropped value, attributable deep semantic counters are exactly zero for forcing attempts, locator dereferences, substitutions, nested value/body dispatches or relation reads, semantic allocations/origin writes, and semantic dependency fact reads. Intersection contribution selection may perform at most one contribution-classification probe per potentially contributing arm per `(path segment, complete ProjectionReductionContext)` cold evaluation under the existing request budget; every probe's key/selection facts are visible and fact-traced, identical warm demand adds zero probes, and an undecidable arm preserves open/partial carrier state. Only after non-contribution is proven does the zero deep-work rule apply. Parse/shallow indexing is excluded.
- Fresh and incremental parity covers output node/bytes, origin edges, completeness/partial classes, read-set validation, candidate count, and semantic-work counters. Dead operands add no semantic dependency facts and no semantic work. An ordinary same-owner edit may nevertheless conservatively reject a candidate through strict self-root validation; recomputation must match fresh and still show zero dead-operand deep work. Changing selection/demand must force the newly live operand under current facts. TE5 neither weakens nor refines strict self-root architecture.
- The architecture inventory must also prove no direct TE edge or public dependency is needed at G1, E2, G2, or native-checker surfaces: D8 is the convergence consumer and those nodes inherit TE5 transitively.

## Acceptance IDs and discriminating proof

- **TE5-AC1 — structural one-authority closure:** the exhaustive entrance inventory has zero semantic-evaluation bypasses; compile/capability enforcement makes a new external bypass unrepresentable and wildcard-free internal dispatch forces classification. Prove no second graph/resolver/recipe/demand walker/cache exists and no new cache/admission subsystem was added. A mutation restoring one representative eager bypass must fail.
- **TE5-AC2 — semantic and dead-work closure:** conditional, projection, indexed/keyof, mapped/generic, utility, infer, open conditional projection, root `Expanded`, and mapped single-key fixtures preserve their required answers and origins. The unified matrix proves all seven attributable deep-work classes are zero for dead branches/siblings/keys and proven non-contributing arms; intersection classification is bounded to at most one fact-traced probe per potentially contributing arm per `(path segment, complete ProjectionReductionContext)` cold evaluation under the request budget, identical warm demand adds zero probes, and an undecidable arm stays open/partial.
- **TE5-AC3 — identity, satisfaction, validation, and admission enforcement:** cross-family tests prove TE1–TE4's complete authored identity/store-local node-handle rules, one-axis distinction of request-owned `mode`, `demand`, `provenance`, `merge_role`, and `vue_heritage_policy`, recorded-point satisfaction, live fact/self-root rejection, cancellation-before-force/admission, and `ReturnOnly` degraded outcomes remain connected after bypass deletion. No operand or route reconstructs, defaults, duplicates, or stores a second `ProjectionReductionContext`. A same-owner dead edit may reject through strict self-roots; recomputation matches fresh and performs zero dead-operand semantic work. TE5 does not introduce a new family key, validity oracle, candidate store, admission rail, or retention policy.
- **TE5-AC4 — bounded concurrency and retention:** one cold winner per key, cooperative join without a second wait graph, exact family caps, invalid-first/LRU-valid eviction behavior, zero repeated-warm candidate growth, bounded origin/reverse-index publication, and no semantic-work/allocation regression for equivalent demanded work.
- Every new test must discriminate a concrete bypass, identity, satisfaction, cancellation, retention, or dead-work regression. Prefer a table-driven cross-operator matrix plus focused compile/capability proof; do not add prose mirrors or universal name scanners.
- Test homes: `crates/verter_session/tests/cases`, co-located semantic query/dispatch tests, and existing source-policy architecture tests only if a structural compile proof cannot cover an in-crate boundary.

## Deletions and forbidden designs

- Delete every eager selective-operator path that dereferences, lowers, substitutes, instantiates, relates, projects, enumerates, or allocates an operand outside TE1 force. Each deletion names the surviving shared query route.
- Delete any TE1–TE4 migration shim, dual operand form that remains selectable, request-local memo, private in-flight table, nominal/rank satisfaction shortcut, or generation-only validation shortcut encountered on the bounded route-switch surface; family-specific cache/admission improvements outside that route switch remain owned by TE1–TE4 rather than being recreated here.
- No `SemanticRecipeId`, general recipe graph, bytecode/instruction tape, closures, function pointers, trait-object evaluator, arbitrary generic force trait, AST/OXC reference, stored `TypeExpr`, arbitrary env map, source text/hash/span identity, or consumer-owned recursive walker.
- No second relation, distributivity, truthiness, canonical-algebra, mapper, generic-instantiation, projection, route, cache, or native-checker authority.
- Never warm cancelled, budgeted, unstable, recursive, missing, signature-overflow, fact-invalid, or partial work; never backfill work a candidate did not materialize; never exceed or silently grow a family cap.
- Do not add direct TE predecessor edges to G1, E2, G2, or native-checker nodes. Their dependency is transitive through D8. Do not change D2B's implementation scope; TA1B and TA2 remain required predecessors, and D2B cannot return to implemented until both are implemented.
- Do not widen public `TypeInfo`, native checker, flow semantics, relation policy, truthiness, canonical algebra, display, component-meta, or wire contracts.

## Budgets and rescope

- Planning reference: 800 production LOC, 8 production files, 2 related crates/packages; the named eight-file mutation surface remains the expected footprint.
- Numeric rescope signal: 1,500 production LOC or 12 files. Crossing it, or touching a ninth file, requires a scope-coherence investigation under `contracts/sizing.md`, not automatic rescope.
- Architect rescope remains mandatory when the candidate spans 3 unrelated crates/packages, or closure requires a new graph/cache/IR, a public/wire change, unsafe/concurrency redesign, or semantic change outside TE1–TE4.
- Correctness budget: zero bypasses, dead-operand semantic work, identity aliasing, stale/unsatisfied reuse, wrong-complete result, or warm degraded work.
- Performance budget: dead-operand counters exactly 0; one cold winner per key; repeated warm candidate delta 0; all family sizes at or below `candidate_cap`; equivalent demanded-work parse, dispatch, allocation, latency, and retained-origin counts may not regress.

## Abort conditions

- Stop before mutation if TE4 lacks an implemented row, the entrance inventory cannot fit one review context, or one forcing authority cannot be structurally enforced without changing ratified operator meaning.
- Stop if closure would require a second graph/resolver/cache, a new cache/admission mechanism rather than enforcement of TE1–TE4's landed contracts, public/native/wire expansion, direct successor edges, or changes to D2B's unrelated implementation scope.
- Abort on any residual selectable bypass, dead-work counter, stale/nominal-only reuse, warm partial, candidate growth, concurrency divergence, or unexplained output/performance regression.

## Targeted verification

1. Run the cross-operator dead-work, identity, recorded-point satisfaction, cancellation, fresh/incremental, concurrency, and candidate-retention suites plus capability/compile proof.
2. `cargo nextest run -p verter_session -p verter_semantic -p verter_workspace`
3. Run every final command in `targeted-domain` on the stable candidate and bind TE5-AC1–AC4 evidence/rationale in the review report.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, and `architecture-specialist`. Reviews must inspect the full entrance/deletion inventory, structural enforcement, identity/satisfaction/validation/admission, cancellation, concurrency, retention, dead-work matrix, and successor topology. P0/P1 block; unresolved P2 follows the binding policy and otherwise blocks. Any material change invalidates affected verdicts. Final acceptance requires 3/3 clean PASS reports plus `independent-full` confirmation.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Row presence is authoritative; locator metadata is never validated against Git or GitHub.
