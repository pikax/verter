<!-- unified-charter-v2
id=NCK2
name=Incremental diagnostic query and result domain
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor
predecessors=NCK1,G2,H3,PUB0
conditional_predecessors=
owner=expansion.native-checker:sole native semantic diagnostic authority and certified-family cutover
conflict_domains=semantic_authority,semantic_cache_store,public_protocol
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
size=M
dispatchable=true
optional=false
release_gating=none
source_refs=live:docs/arch/refactor/rev11/sources/legacy-arch-reconciliation.md
external_requirements=
activation_gate=ORC0
charter=charters/expansion-native-checker/NCK2.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCK2 - Incremental diagnostic query and result domain

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Implement the native checker query/result substrate: scoped Check query keys, typed DiagnosticBatch results, exact contexts and read sets, same-key production, reclaimable stores, aggregation, and public result conversion. This block implements no broad TypeScript diagnostic catalogue.

The current owner is **reserved or absent Check query names, ad hoc diagnostic vectors, LSP-owned aggregation, and provider-oriented result assumptions**. The final and sole owner is **ProjectSemanticDispatch and SemanticGraphStore-backed scoped diagnostic query families with typed outcomes and bounded storage**.

## Architectural role and end state

NCK2 makes diagnostics first-class semantic query values while preserving the one resolver. It supplies the execution, caching, cancellation, and aggregation substrate that NCK3 rules and generated NCF slices use.

## Expected production surfaces

- `crates/verter_session/src/semantic_query` for keys, contexts, specs, dispatch, and query-family admission
- `crates/verter_semantic` for diagnostic query values, bases, stores, and proof references
- `crates/verter_diagnostics` for immutable Diagnostic and DiagnosticBatch core types
- `crates/verter_protocol` and public adapters jointly with PUB0
- `crates/verter_session/tests` for spec-table generation, cache, cancellation, and incremental proofs

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `SemanticQueryKey::{CheckRegion, CheckFile, CheckProjectRule, CheckExpression}`
- `SemanticQueryValue::DiagnosticBatch` and `SemanticQueryValueTag::DiagnosticBatch`
- `CheckRegionContext`, `CheckFileContext`, `CheckProjectRuleContext`, `CheckExpressionContext`
- `DiagnosticBatch`, `DiagnosticBatchBasis`, `DiagnosticBatchOutcome`, `DiagnosticAggregate`
- `DiagnosticQueryStore`, per-family retention policy, and same-key FlightCell ownership
- `DiagnosticQuerySpec` generated alongside the enum/spec table

## Exact predecessor contracts

- **NCK1:** consume exact region identity and typed contribution ingress.
- **G2:** consume FlightCell-owned same-key production, cancellation, and admission laws.
- **H3:** consume exact-basis foreground settlement and stale-safe background publication semantics.
- **PUB0:** consume versioned typed public outcomes and capability truth.

External custody: none beyond the package activation boundary.

## Binding architecture

- CheckRegion is the primary semantic unit; CheckFile aggregates exact region and file-rule results; CheckProjectRule owns only genuinely project-scoped rules.
- CheckExpression is demand-sliced and interactive; it may not trigger whole-file checking unless the declared rule requirements demand it.
- There is no CheckProgram memo family. Workspace checking is an application coordinator over files/project rules with cancellation and progress.
- Query keys contain content-free identity and exact environment dimensions; result values are version-rooted by recorded facts.
- Only complete, current results admit. Cancelled, superseded, budget-exceeded, partial, or NeedInputs results are ReturnOnly.
- Storage is per project/profile/family, reclaimable, and bounded. Diagnostic payloads do not retain semantic arenas unnecessarily.

## Internal subblocks

### NCK2-SB1 - Query taxonomy and spec-table integration

**Independently testable outcome:** Every live Check key has one generated spec row and dispatch arm.

**Architecture:**

- Add scoped keys and tags together with value-domain, admission, allowed demand, environment dimensions, and cross-context guard.
- Keep project rules separate from file/region execution.
- Reserve future finer keys only through an amendment; do not add speculative dead variants.

**Expected changes:**

- Update enum, spec generator, artifact, tag set, and dispatch exhaustiveness in one change.
- Register critical guards for enum/spec/dispatch equality.

**Discriminating proof:**

- Enum-spec-dispatch triangulation fails on a planted missing row or dispatch arm.
- No Check key resolves to TypeNode or GraphTypeNode.

### NCK2-SB2 - Exact query contexts and identities

**Independently testable outcome:** Cross-project, profile, environment, and source revisions cannot warm-hit incorrectly.

**Architecture:**

- Define per-key contexts with only semantically relevant parse, resolve, type, lib, and project dimensions.
- Include region/file/project-rule identity and diagnostic family/slice identity.
- Keep content hashes on value/read-set rooting, not as substitutes for semantic identity.

**Expected changes:**

- Implement family key construction and minimal-dimension characterization tests.
- Add cross-context no-warm-hit guards.

**Discriminating proof:**

- Mutation matrix changes each identity axis independently and proves correct hit/miss behavior.
- Benched minimality rejects unnecessary dimensions that cause false misses.

### NCK2-SB3 - DiagnosticBatch core value domain

**Independently testable outcome:** A query result carries immutable diagnostics, basis, completeness, read set, and operational outcome.

**Architecture:**

- Define compact diagnostic records with stable IDs, primary/related authored anchors, proof refs, and fix-intent refs.
- Separate semantic diagnostic identity from localized message rendering.
- Use compact interned strings/IDs and avoid retaining full semantic nodes.

**Expected changes:**

- Implement core types and deterministic canonical ordering.
- Add public DTO conversion behind PUB0 schema/version gates.

**Discriminating proof:**

- Layout/size tests and deterministic serialization tests.
- Empty-complete, NeedInputs, cancelled, stale, and unsupported remain distinguishable.

### NCK2-SB4 - Same-key production and admission

**Independently testable outcome:** Concurrent identical checks compute once and only complete current results enter warm storage.

**Architecture:**

- Use the existing FlightCell/singleflight family runtime.
- Bind cancellation, deadline, budget, supersession, and provider-independent semantic basis.
- Record exact facts read by the rule executor.

**Expected changes:**

- Implement producer ownership, waiter behavior, admission probe, and ReturnOnly paths.
- Instrument compute, wait, cancellation, and admission counters.

**Discriminating proof:**

- Concurrency tests prove one producer and deterministic waiter results.
- Poison tests prove a cancelled/partial producer cannot populate the cache.

### NCK2-SB5 - Bounded diagnostic storage and reclamation

**Independently testable outcome:** Repeated checks do not create unbounded per-file, per-family, or per-revision retention.

**Architecture:**

- Define per-family retention and generation replacement.
- Store compact values detached from temporary semantic arenas.
- Evict superseded profile/source generations and release proof references safely.

**Expected changes:**

- Add DiagnosticQueryStore under the existing project semantic store ownership.
- Add memory counters and explicit teardown/epoch transition behavior.

**Discriminating proof:**

- Long-churn tests plateau RSS/retained bytes.
- A deleted/closed project releases all checker storage and contributor snapshots.

### NCK2-SB6 - File/project aggregation and public conversion

**Independently testable outcome:** Aggregators compose scoped results without becoming another semantic engine or cache authority.

**Architecture:**

- CheckFile joins exact region and file-rule batches; workspace coordination stays above query storage.
- Deduplicate only by stable diagnostic identity and authority, never message text.
- Propagate the least complete outcome and exact contributing bases.

**Expected changes:**

- Implement deterministic aggregation helpers and public DTO conversion.
- Leave publication arbitration to NCK6.

**Discriminating proof:**

- Fresh versus incremental aggregate equality across reordered region completion.
- A mixed stale/current input is rejected rather than published as current.

## Data, identity, invalidation, and publication laws

- A Check key may read semantic facts but may not mutate or create a second semantic node authority.
- DiagnosticBatch basis includes project/profile/source/region or rule identity plus canonical fact read-set signature.
- Message localization and editor formatting occur after semantic identity/cache computation.
- Aggregation is deterministic and non-cache-authoritative unless represented by its own exact scoped query key.
- CheckExpression has strict budgets and cannot admit a result that omitted a required rule input.

## Migration and cutover

- Introduce query values dormant until NCK3 registers rule executors; no user-visible native diagnostic publication in this block.
- Migrate ad hoc semantic diagnostic vectors only where exact identity and basis can be preserved; leave parser/lint/provider classes with their owners.
- Delete duplicate checker cache prototypes in the same candidate that routes their final consumer.

## Deletions

- Delete identity-less semantic diagnostic side channels displaced by DiagnosticBatch.
- Delete any ad hoc whole-file or whole-project checker cache introduced during prototyping.
- Delete duplicate per-feature same-key coordination for checker queries.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- CheckProgram as a monolithic memoized key.
- Diagnostic data embedded in TypeNode/GraphTypeNode or public TypeInfo graph nodes.
- Message text or source range as the sole diagnostic identity.
- Caching cancelled, partial, stale, budget-exceeded, or NeedInputs outcomes as complete.
- A query context that bundles an opaque project_config_hash instead of exact dimensions.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **NCK2-AC-SPEC:** Check enum, tags, generated spec table, dispatch, and value tags are exactly equal.
- **NCK2-AC-ADMISSION:** only complete current batches warm-admit under same-key production.
- **NCK2-AC-MEMORY:** repeated revisions and project teardown prove bounded retention.
- **NCK2-AC-NO-PROGRAM-CACHE:** static guard rejects monolithic CheckProgram memo authority.
- **NCK2-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **NCK2-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **NCK2-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **NCK2-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Warm identical CheckRegion requests perform zero parse, index walk, semantic recomputation, provider work, and diagnostic allocation beyond result sharing.
- CheckFile aggregation is linear in returned scoped results, not project size.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `cargo nextest run -p verter_semantic -p verter_session -p verter_diagnostics -p verter_protocol`.
1. Query-key spec generation/diff and cross-context mutation matrix.
1. Concurrent same-key, cancellation poison, incremental/fresh, and long-churn memory tests.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Unlocks NCK3 rule execution and NCK4 certification harness integration.
- Provides typed result contracts consumed by NCK6, CLI2, LSO8, and public surfaces.

## Source reconciliation

- `docs/arch/native-checker.md` checker query layer and typed result requirements.
- `docs/arch/fact-based-cache.md` query identity/admission laws transferred through G1/G2/E4.
- Live `SemanticQueryKeySpec` table and D8 complete-result contract.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.
