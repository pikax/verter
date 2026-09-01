<!-- unified-charter-v2
id=TE4
name=Mapped and generic selective forcing
phase=rev11
train=rev11.type-evaluation
product=rev11
kind=implementation
semantic_role=delivery
class=foundational-authority
predecessors=TE2,TE3
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
charter=charters/rev11-type-evaluation/TE4.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# TE4 — Mapped and generic selective forcing

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Apply TE1–TE3 selective forcing to mapped types, generic instantiation, and utility
operators. A mapped/generic query first determines the demanded key domain and then
substitutes and forces only value operands needed for those keys. A single-key
`ProjectPath` through a mapped type does not enumerate the full mapped surface. Dead
keys and dead conditional/operator operands are neither substituted nor instantiated.
Mapper classification remains a structural, content-free lowering fact and must not
materialize the mapper's value body just to choose `Identity` versus `Computed`.

`SemanticQueryKey::MappedType`, `Instantiate`, `IndexedAccess`, `KeyOf`, and
`ProjectPath` remain the only reusable query families for this work. Built-in
utilities use the same machinery as user-authored helpers. The open-key-domain
carrier-stop remains authoritative: an open/unknown key domain preserves its carrier;
a closed-key/open-value mapped type may expose demanded keys with shallow values.
This charter accepts one mapped/generic evaluation cutover and contains no
independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_session/src/project_semantic_dispatch`, `crates/verter_session/src/semantic_query.rs`, and existing prepared-declaration/binder facts under `crates/verter_semantic/src/analysis/type_solver` only as needed to preserve exact identity.
- Production files: exactly the eight named files `semantic_query.rs`, `build.rs`, `walk.rs`, `raise.rs`, `lower.rs`, `locator_shape.rs`, `semantic_query_memo/family.rs`, and `mapper_binder_registry.rs`; tests are non-production. `evaluate.rs` is outside this exact target inventory and requires ninth-file rescope if actually needed. A ninth production file exceeds the target and requires explicit rescope; the 12-file mandatory ceiling is not advance permission.
- Named boundaries: `SemanticQueryKey::{MappedType,Instantiate,ProjectPath,IndexedAccess,KeyOf,Conditional}`, `MapperKey`, `MapperKind`, mapper `parameter_node`, `InstantiateKey`, `InstantiateContext`, `ResolvedDeclSlotIdentity`, `SubstitutionCanonicalHash`, `InferBinderId`, type-parameter defaults/constraints, `prepared_instantiation_key_domain_is_closed`, `mapped_type_is_open_or_unknown`, `mapped_type_key_domain_is_open_or_unknown`, `materialize_mapped_member_value_for_key`, TE1 force, and TA1B canonical construction reached transitively through TE2/TE3.
- Mutation boundary: demand-aware mapped/generic key/value forcing, exact identity/context, mapper classification, carrier-stop integration, and tests. No new utility, public/native/wire, relation, truthiness, or display authority.

## Exact predecessor contracts

- **TE2:** implemented ledger row for “Conditional selective forcing”; ledger presence alone satisfies the predecessor. It supplies select-before-force conditional semantics, exact infer scoping, open conditional suspension/residual projection, distributivity ownership, and dead-branch proof used by conditional mapper values.
- **TE3:** implemented ledger row for “Projection and key-domain selective forcing”; ledger presence alone satisfies the predecessor. It supplies canonical residual-path propagation, key-domain-before-base forcing, single-key non-enumeration, and content-free projection identity.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- A mapped whole-surface demand may enumerate a proven closed key domain. A `ProjectPath`/`ProjectMember` for one known key forces the mapping only for that key and does not call whole-surface member enumeration as a prerequisite.
- For each demanded key, bind the exact mapper parameter identity, apply the exact substitution environment, then force the value operand under the caller's residual demand/context. Different `MapperKey`, binder, modifier, remap, default, constraint, or substitution identities never alias.
- Dead mapped keys are not substituted, instantiated, conditionally selected, related, lowered by locator, allocated, or fact-read. A dropped `as`-remap key is dead after the remap decision; its value operand is never forced.
- A generic declaration body is lowered at most once per declaration/content through the existing locator/`Instantiate` path. Different concrete argument environments reuse the parameterized body but keep distinct semantic instantiation identities. Missing arguments use declared defaults only under exact verified binder order; constraint/default reads are fact-traced.
- Mapper classification (`MapperKind`) operates on structural carrier shape/binder identity only. It must not execute `Instantiate`, `Conditional`, `IndexedAccess`, relation, or locator dereference and must not enumerate source keys or materialize the value body. `Computed` classification defers evaluation to demanded-key force.
- Preserve L1 carrier-stop semantics across every entrance. Open/unknown key production stays a deferred `Mapped`/`InstantiationRef`; closed-key/open-value permits demanded-key enumeration with shallow deferred values. Budget exhaustion is unknown/open and never falls through into eager Expanded materialization.
- Built-in `Partial`, `Required`, `Readonly`, `Pick`, `Omit`, `Record`, and other applicable utilities route through the same typed key-domain/mapper/instantiate queries. No name/text pattern fast path may produce different nodes, origins, facts, modes, or cache behavior.
- **Mapped single-key non-enumeration proof:** for `MappedWide['wanted']`, attributable semantic counters for every unrelated key/value must be exactly zero: forcing attempts, locator dereferences, substitutions, nested dispatches/relation reads, semantic allocations, and semantic fact reads. Parse/shallow indexing is excluded. The demanded key may perform only the work its value and remap require.
- Cancellation checks run before key enumeration, before each substitution/force, and before admission. Cancelled, budgeted, recursive, partial, or unstable work is `ReturnOnly`; a partial per-key result never warms a complete mapped/generic family candidate.
- Required fixtures include wide mapped types, key remaps that drop keys, conditional mapper values with infer, forwarded/defaulted/constrained type parameters, closed-key/open-value and open-key carriers, userland/builtin equivalence, and fresh/warm/incremental edits.

## Acceptance IDs and discriminating proof

- **TE4-AC1 — demanded-key-only evaluation:** a single-key mapped/generic projection returns the same semantic answer as whole-surface-then-project while satisfying exact zero-work counters for unrelated keys and dropped remaps. A mutation restoring full enumeration or eager value substitution must fail.
- **TE4-AC2 — exact generic and mapper semantics:** prove binder/default/constraint/substitution/remap/modifier identity is exact; dead operands are neither substituted nor instantiated; mapper classification performs zero semantic dispatch/materialization; conditional/infer values preserve TE2 scoping; open/closed carrier-stop behavior and builtin/userland equivalence remain intact.
- **TE4-AC3 — validation and fresh/incremental parity:** every demanded default/constraint/body/import fact enters the ordinary `ReadSetSignature`; dead-key semantic dependency facts do not. An ordinary same-owner dead-key edit may conservatively reject through strict self-root validation; recomputation must still perform zero dead-key semantic work and match fresh. Fresh and incremental outputs, origins, completeness, and candidate behavior match after demanded-key, dead-key, default, constraint, and remap edits. Partial/cancelled/budgeted work remains `ReturnOnly`; no finer-grained self-root scheme is introduced.
- **TE4-AC4 — bounded work and retention:** body lowering occurs at most once per declaration/content; substitutions and nested dispatches scale with demanded keys, not source width; identical instantiated demands join the existing family memo; repeated warm demands produce no candidate growth and obey the family cap.
- Every new test must discriminate a plausible full-enumeration, binder-alias, eager-classification, carrier-stop, or admission regression. Prefer table-driving existing mapped/generic suites.
- Test homes: co-located `project_semantic_dispatch` tests, `crates/verter_session/tests/cases`, and narrowly relevant `verter_semantic` prepared-declaration tests.

## Deletions and forbidden designs

- Delete mapped/generic routes that enumerate every key or substitute every value before a narrower requested key is known; cite TE3 key demand plus TE1 force as the replacement.
- Delete mapper classifiers that inspect a materialized value result; classification consumes structural carriers/binder identity only.
- No second mapper evaluator, generic instantiator, utility resolver, conditional selector, relation authority, recipe graph, or recursive demand walker.
- No `SemanticRecipeId`, closures, AST/OXC references, stored `TypeExpr` recipes, arbitrary env maps, source/content hashes, spans, display text, or name-matched utility semantics in reusable identity.
- Never use unknown/open as closed, enumerate an open key domain, substitute a dead key, force a remap-dropped value, or let partial per-key work warm a whole-surface candidate.
- Do not widen public `TypeInfo`, native checker, flow, relation, truthiness, canonical algebra, component-meta publication, or wire ownership.
- Do not implement TE5 closure/deletion beyond routes directly displaced by this cutover. Discovery of an independent outcome requires a DAG amendment.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or if a second instantiation/mapper subsystem, public/wire change, or broad relation/type-system rewrite is required.
- Correctness budget: zero dead-key semantic work, binder/substitution aliasing, open-as-closed evaluation, stale publication, wrong-complete result, or warm degraded candidate.
- Performance budget: single-key mapped enumeration count 1 demanded key and 0 unrelated keys; unrelated semantic counters exactly 0; declaration body lower count at most 1 per content; no warm growth; equivalent selected-work allocation/latency may not regress.

## Abort conditions

- Stop before mutation if either predecessor lacks a ledger row, exact mapper/binder identity cannot ride the existing keys, or demanded-key evaluation requires a whole-surface materialization under live APIs.
- Stop if correct behavior requires a new generic resolver, recipe graph, relation policy, public/wire change, or weakening the L1 carrier-stop.
- Abort on any dead-key work, mapper-classification dispatch, identity collision, fresh/incremental divergence, warm partial, candidate growth, or unexplained performance regression.

## Targeted verification

1. Run discriminating mapped single-key, remap, conditional/infer, generic default/constraint, carrier-stop, cancellation, and retention cases.
2. `cargo nextest run -p verter_session -p verter_semantic`
3. Run every final command in `targeted-domain` on the stable candidate and bind TE4-AC1–AC4 evidence/rationale in the review report.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial` and `conformance`. Reviews must inspect key-scaled work, exact generic identity, classifier purity, carrier stops, builtin parity, and admission/retention. P0/P1 block; unresolved P2 follows the binding policy and otherwise blocks. Any material change invalidates affected verdicts. Final acceptance requires 2/2 clean PASS reports plus `targeted` confirmation.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Row presence is authoritative; locator metadata is never validated against Git or GitHub.
