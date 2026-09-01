<!-- unified-charter-v2
id=TE3
name=Projection and key-domain selective forcing
phase=rev11
train=rev11.type-evaluation
product=rev11
kind=implementation
semantic_role=delivery
class=foundational-authority
predecessors=TE1,TA1B
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
charter=charters/rev11-type-evaluation/TE3.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# TE3 — Projection and key-domain selective forcing

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Move projection-shaped selective operators onto TE1's sealed operands so the requested
key or residual path is known before an operand is forced. `ProjectPath` remains the
canonical path query; `ProjectMember` and `IndexedAccess` remain sugar that converge on
it; `KeyOf` remains the key-domain query. A projection evaluates its key/index domain
before requesting any unrelated base surface, propagates residual path demand through
only contributing arms, and never enumerates sibling members merely to answer one key.

The final owner is the same **sole demand-selected semantic operand forcing authority
inside the existing SemanticQuery graph**. Path walking stays a non-owning navigator:
it may choose the next hop but every new semantic node, declaration resolution,
instantiation, conditional/mapped reduction, and reusable result re-enters the shared
query API. Canonical intersection/union construction remains TA1B-owned. This charter
accepts one projection/key-domain cutover and contains no independently dispatchable
subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_session/src/project_semantic_dispatch` and `crates/verter_session/src/semantic_query.rs`.
- Production files: the six named files `walk.rs`, `evaluate.rs`, `build.rs`, `raise.rs`, `lower.rs`, and `semantic_query_memo/family.rs`, plus at most two additional production files if the closed TE1 request vocabulary requires them; tests are non-production. A ninth production file exceeds the target and requires explicit rescope; the 12-file mandatory ceiling is not advance permission.
- Named boundaries: `SemanticQueryKey::{ProjectPath,ProjectMember,IndexedAccess,KeyOf}`, `PathSegment`, `IndexKey`, `ProjectionMode`, `ProjectionReductionContext`, `ReductionDemand`, `PathWalker`, `SemanticNodeData::{IndexedAccess,KeyOf}`, TE1's sealed operands/force capability, `ProjectMember`/`ProjectIndex`/`ProjectPath` origin edges, `ReadSetSignature`, and `cached_satisfies` recorded materialized points.
- Mutation boundary: operand-bearing projection keys/carriers, lowering/emission of indexed/keyof shells, path-walk demand propagation, member/key-domain evaluation, and exact tests. No whole-surface/public/native-checker/wire ownership expansion.

## Exact predecessor contracts

- **TE1:** implemented ledger row for “Sealed semantic operands and forcing boundary”; ledger presence alone satisfies the predecessor. It supplies exact content-free authored operand identity, store-local materialized handles, the request-owned complete `ProjectionReductionContext` (`mode`, `demand`, `provenance`, `merge_role`, `vue_heritage_policy`), its exactly-once unchanged combination with operand identity in query dispatch/family identity, and the one force capability. TE3 may add projection-specific request vocabulary but may not reconstruct/default/duplicate/store a second context or add a walker-owned evaluator.
- **TA1B:** implemented ledger row for “Canonical composite payload and construction-site closure”; ledger presence alone satisfies the predecessor. Intersection contributions and union-wide projection results must use TA1B's canonical construction and sealed bypass categories; no raw composite mint is allowed.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- `ProjectPath { base, path, context }` remains canonical. Intermediate segments demand `Navigate`; only the terminal uses the requested mode. An empty path remains the explicit whole-surface case rather than an accidental side effect of forcing a base.
- A member/index request first seals the segment/key-domain demand. For an indexed access, force the index/key operand sufficiently to determine literal/union/index-signature demand before requesting base members. Do not expand the base surface first.
- `KeyOf` forces only key-producing structure. It must not force object member values, nested bodies, call signatures, or unrelated declaration surfaces unless their structure is semantically necessary to decide the key domain.
- Intersection projection may perform at most one contribution-classification probe per potentially contributing arm per `(path segment, complete ProjectionReductionContext)` cold evaluation, under the existing request budget, before it knows which arms contribute. Every probe's key/selection facts are fact-traced; identical warm demand adds zero probes. A proven non-contributor is ignored, not rewritten to `never`, and only after that proof must its member-value/body/deep forcing remain zero. If contribution is undecidable, preserve an open/partial carrier and its typed completeness rather than treating the arm as absent. Union projection requires every arm to contribute but carries the same residual path into each arm without enumerating sibling keys.
- Conditional values reached during a path are opaque carrier/materialized-branch inputs under the behavior already present when TE3 lands. TE3 may preserve a residual path on an open conditional shell, but it neither selects a branch nor claims selected/dead-branch forcing semantics. Selection-before-force, infer scoping, and dead conditional branches belong only to TE2; their combined conditional-plus-projection behavior is integrated in TE4 and proved cross-family in TE5.
- Indexed-access and `keyof` carrier operands remain addressable when open/undecidable. A budget, recursion, missing dependency, or partial key-domain answer yields the existing typed carrier or degraded result under the applicable completeness rule and is `ReturnOnly` whenever incomplete; it is never fabricated as a closed empty surface.
- Content-free query identity includes exact base/operand identity, path/index, substitutions/binders, and the force request's unchanged complete five-axis `ProjectionReductionContext`, combined with operand identity exactly once by TE1. The sealed operand does not own or duplicate any context axis. Read facts and content versions remain exclusively in value-side `ReadSetSignature`/self-roots.
- **Non-enumeration proof:** projecting `Deep['wanted']['leaf']` performs zero forcing attempts, locator dereferences, substitutions, nested dispatches, semantic allocations, and semantic fact reads attributable to sibling member values/bodies. For intersections, at most one fact-traced contribution-classification probe is permitted per potentially contributing arm per `(path segment, complete ProjectionReductionContext)` cold evaluation under the existing request budget; identical warm demand adds zero probes. After an arm is proven non-contributing, its member-value/body/deep forcing counters must be zero. Parse/shallow indexing is excluded.
- Required fixtures include a wide object with a cold/invalid sibling import, an intersection with proven contributing/non-contributing arms plus an undecidable contribution arm, a union path, indexed access with a literal-union index, `keyof` over value-heavy members, an opaque open conditional shell retaining residual path, and fresh/warm/incremental edits of requested versus unrelated keys. No TE3 fixture may assert branch selection or dead conditional-branch forcing.

## Acceptance IDs and discriminating proof

- **TE3-AC1 — sole path/key forcing authority:** all `ProjectMember`/`IndexedAccess` sugar converges on canonical `ProjectPath`/key-domain families, and every new semantic hop enters the shared dispatch. Prove no walker-owned recursive resolver, key evaluator, or cache exists and that restoring whole-base-first evaluation fails a discriminating test.
- **TE3-AC2 — path precision and key-domain order:** prove intermediate `Navigate`/terminal mode behavior, index/key-domain-before-base behavior, intersection/union contribution rules, `keyof` value non-forcing, and opaque open-conditional residual-path preservation without branch selection. The non-enumeration fixture shows exact zero deep semantic work for siblings and for proven non-contributing arms, at most one fact-traced contribution probe per potentially contributing arm per `(path segment, complete ProjectionReductionContext)` cold evaluation under the request budget, and zero added probes for identical warm demand; undecidable contribution preserves open/partial carrier state.
- **TE3-AC3 — identity, validation, and incremental parity:** distinct path/index/request-context/substitution identities never alias, with one-axis tests for request-owned `mode`, `demand`, `provenance`, `merge_role`, and `vue_heritage_policy`; warm candidates validate ordinary fact signatures; requested-member edits invalidate and recompute; an unrelated sibling-body edit may conservatively reject through strict same-owner self-root validation but recomputation performs zero sibling body/deep force work and matches fresh. Partial/cancelled/budgeted work is `ReturnOnly`. Fresh and incremental results and origin edges match.
- **TE3-AC4 — bounded work:** navigation touches at most one semantic query per required hop plus terminal work; a single-key request does not enumerate the base; intersection classification performs at most one probe per potentially contributing arm per `(path segment, complete ProjectionReductionContext)` cold evaluation under the existing request budget; concurrent equal paths join existing singleflight; repeated identical warm requests add zero classification probes and cause no candidate growth. Use existing graph statistics/test hooks or exact bounded inspection.
- Every new test must discriminate a plausible whole-surface, contribution-classification, open/partial-arm, wrong-key-order, request-context-alias, or admission regression. Prefer extending/table-driving existing path tests.
- Test homes: co-located `project_semantic_dispatch` tests and `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete eager whole-base/member-surface evaluation used only to answer a narrower projection and every projection-local recursive semantic resolver; cite `ProjectPath` plus TE1 force as the replacement.
- No generic recursive demand walker, second projection graph, key-domain cache, relation authority, conditional selector, mapper evaluator, or raw union/intersection constructor.
- No `SemanticRecipeId`, closures, AST/OXC pointers, `TypeExpr` operand storage, source hashes, spans, text heuristics, arbitrary env maps, or boolean-compressed projection context.
- Never enumerate siblings for a single key, materialize member values for `keyof`, or use a partial key domain as proof of a complete empty result.
- Do not widen public `TypeInfo`, native checker, flow, relation, truthiness, canonical algebra, component-meta publication, or wire ownership.
- Do not implement or claim TE2's conditional branch selection/dead-branch cutover, TE4 mapped/generic value evaluation, or TE5 cross-family closure. Discovery of an independent outcome requires a DAG amendment.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or if a new graph/walker/cache, public/wire change, or relation/mapped policy change is required.
- Correctness budget: zero sibling member-value/body/deep work; zero deep work after non-contribution is proven; every contribution probe's key/selection facts traced; no more than one probe per potentially contributing arm per `(path segment, complete ProjectionReductionContext)` cold evaluation; zero identity aliasing, stale publication, fabricated closure, wrong origin, or warm degraded result.
- Performance budget: single-key enumeration count 0; sibling semantic counters 0; contribution probes at most one per potentially contributing arm per `(path segment, complete ProjectionReductionContext)` cold evaluation under the request budget and zero for identical warm demand; required semantic queries bounded by path length plus terminal work; zero warm-candidate growth and zero regression in equivalent selected work.

## Abort conditions

- Stop before mutation if a predecessor lacks a ledger row, path precision cannot be represented by TE1 demand/context, or live source requires a whole-surface answer to determine an ordinary known key contrary to the contract.
- Stop if the solution needs a second walker/resolver/cache, redefines conditional selection, or changes public/native/wire semantics.
- Abort on any unexplained sibling work, fresh/incremental divergence, warm partial, allocation/latency regression, or candidate growth.

## Targeted verification

1. Run discriminating path, indexed-access, `keyof`, open-conditional, cancellation, and non-enumeration cases.
2. `cargo nextest run -p verter_session -p verter_semantic`
3. Run every final command in `targeted-domain` on the stable candidate and bind TE3-AC1–AC4 evidence/rationale in the review report.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial` and `conformance`. Reviews must inspect path/key ordering, non-owning walker boundaries, open/partial behavior, content-free identity, and bounded work. P0/P1 block; unresolved P2 follows the binding policy and otherwise blocks. Any material change invalidates affected verdicts. Final acceptance requires 2/2 clean PASS reports plus `targeted` confirmation.

## Trusted implementation ledger

Before squashing or review, the implementation patch adds one `[[implemented]]` row to `authority/state/implemented.toml` with the node ID, planned squash commit message, approximate timezone-bearing date, and optional pull-request number. Row presence is authoritative; locator metadata is never validated against Git or GitHub.
