<!-- unified-charter-v2
id=NCK0
name=Native diagnostic authority and parity-certification constitution
predecessors=UAK1,D8,E4,G2,TCM3,TIF1,LRA0,PUB0
conditional_predecessors=
phase=expansion
train=expansion.native-checker
product=native_checker
kind=constitution
semantic_role=delivery
class=successor
owner=expansion.native-checker:sole native semantic diagnostic authority and certified-family cutover
conflict_domains=semantic_authority,diagnostic_action_service,public_protocol
resource_class=docs-light
gate_profile=docs-domain
review_profile=architecture-3
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
dispatchable=true
optional=false
release_gating=none
source_refs=source:successor-dag-amendment.md:L1,source:legacy-arch-reconciliation.md:L1
external_requirements=
activation_gate=ORC0
charter=charters/expansion-native-checker/NCK0.md
size=M
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCK0 — Native diagnostic authority and parity-certification constitution

Authority state is derived at dispatch. The canonical CLI validates the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

## Independently acceptable outcome

Ratify the native semantic checker constitution: one diagnostic authority over the existing resolver, a typed diagnostic result model, a family and feature-slice certification law, and an atomic provider-to-native cutover protocol. This block changes authority and contracts only; it does not implement checker execution.

The current owner is **fragmented parser diagnostics, framework-specific checks, lint registration, provider diagnostics, LSP merge logic, and legacy Native Checker prose**. The final and sole owner is **the native checker product constitution, with semantic facts owned by their existing resolver and diagnostic evaluation owned by expansion.native-checker**.

## Concrete surfaces and APIs

- Production surfaces: `docs/arch/refactor/rev11/sources/legacy-arch-reconciliation.md`, `crates/verter_identity/src`, `crates/verter_protocol`, `crates/verter_diagnostics`, `crates/verter_actions`, `crates/verter_semantic`, `crates/verter_session`, `crates/verter_type_runtime`, `crates/verter_lsp`.
- Pack production inventory:
- `docs/arch/refactor/rev11/sources/legacy-arch-reconciliation.md` and generated authority catalogs
- `crates/verter_identity/src` for stable diagnostic, family, rule, and certification identities
- `crates/verter_protocol` for the future public diagnostic batch contract owned with PUB0
- `crates/verter_diagnostics`, `crates/verter_actions`, `crates/verter_semantic`, and `crates/verter_session` as future implementation owners
- `crates/verter_type_runtime` and `crates/verter_lsp` only for certified observation and cutover boundaries, never native semantic computation

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.
- Named API/data boundaries:
- `DiagnosticOrigin`, `DiagnosticFamilyId`, `DiagnosticFeatureSliceId`, and `DiagnosticRuleId`
- `DiagnosticAuthorityState::{External, ObserveNative, CertifiedNative, Disabled}`
- `DiagnosticCertification` and immutable family certification receipts
- `DiagnosticBasis`, `DiagnosticCompleteness`, and typed operational outcomes
- `CorrectionOverlayEntry` as test and certification data, not a runtime compatibility mode
- `DiagnosticDedupKey` and the law that one family/profile/slice has one publishing authority

## Exact predecessor contracts

- **UAK1:** exact current receipt ID and digest for “Universal-tooling constitution and program split”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **D8:** exact current receipt ID and digest for “U6 convergence and complete-result admission proof”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **E4:** exact current receipt ID and digest for “Reclaimable semantic storage and scoped interning”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **G2:** exact current receipt ID and digest for “FlightCell-owned same-key production”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **TCM3:** exact current receipt ID and digest for “TypeScript semantic capability closure (dormant until TCM4)”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **TIF1:** exact current receipt ID and digest for “TypeInfo-first ComponentInfo and component-meta cutover”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **LRA0:** exact current receipt ID and digest for “Profile-scoped diagnostics, lint, fixes, and actions”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **PUB0:** exact current receipt ID and digest for “Versioned public request/result and capability truth”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

### Binding architecture

- Exactly one resolver remains authoritative for symbols, types, relation, calls, overloads, contextual typing, and flow. The checker evaluates diagnostic rules over those facts and may not recompute them.
- Diagnostic classes remain distinct: parser/recovery, native semantic, framework semantic, lint, external provider, and project/configuration diagnostics share a public shape but not authority or suppression rules.
- Certification and cutover occur at `profile + diagnostic family + semantic feature slice`; never at a vague project-wide percentage or one global boolean.
- External TypeScript is the oracle and fallback owner for uncertified families. Native checker execution never invokes tsserver or tsgo to decide a native result.
- The resolver has one correctness behavior. A reviewed correction overlay records conceded TypeScript bugs for certification; no user-facing compat mode or cache-key spec dimension exists.
- Every diagnostic carries authored provenance, exact input basis, completeness, and optional proof/fix references. Identity-less side effects are forbidden.
- Shadow observation is non-publishing. Promotion to CertifiedNative atomically suppresses the external family before native publication becomes visible.
- No monolithic CheckProgram cache entry is allowed. Program checks are coordinators over scoped region, file, and project-rule queries.

### Internal subblocks

#### NCK0-SB1 - Diagnostic ownership matrix

**Independently testable outcome:** Every diagnostic class, family, and surface has a named owner and no overlapping publication authority.

**Architecture:**

- Define the authority matrix across parser, semantic checker, framework adapters, lint, external provider, and configuration/project services.
- Define which owner may create proof references, suppressions, related locations, and fixes.
- Require a stable family and feature-slice identity for every diagnostic capable of cutover.

**Expected changes:**

- Add the matrix and machine-readable catalog schema to Rev11 authority.
- Map existing diagnostics and legacy Native Checker clauses to the new classes.
- Reject an uncategorized diagnostic at registration and publication boundaries.

**Discriminating proof:**

- A planted duplicate owner or uncategorized diagnostic must fail the catalog validator.
- The generated ownership table must be byte-deterministic and complete against registered diagnostics.

#### NCK0-SB2 - Typed result and operational outcome law

**Independently testable outcome:** Diagnostic results cannot collapse cancellation, stale state, missing inputs, or unsupported capability into empty success.

**Architecture:**

- Specify complete, NeedInputs, unsupported, cancelled, stale, and superseded outcomes.
- Specify that partial diagnostic batches are ReturnOnly and never warm-admitted as complete.
- Separate result completeness from an empty diagnostic vector.

**Expected changes:**

- Amend PUB0 result vocabulary and LRA0 diagnostic provenance requirements.
- Reserve the native checker query result domain without adding live query keys yet.

**Discriminating proof:**

- Mutation tests must prove empty-complete differs from NeedInputs, cancelled, stale, and unsupported.
- Serialization round trips must preserve basis and completeness exactly.

#### NCK0-SB3 - Family and feature-slice taxonomy

**Independently testable outcome:** The checker can be implemented and certified in bounded slices rather than one train-sized parity claim.

**Architecture:**

- Define required diagnostic families and a stable feature-slice namespace.
- Permit a family to contain many independently generated NCF nodes.
- Define terminal criteria as manifest completeness, not a hand-maintained percentage.

**Expected changes:**

- Bind the family manifest schema and generated-node policy.
- Define split and merge rules for slices without renumbering published identities.

**Discriminating proof:**

- A missing required slice or duplicate slice identity must fail generation.
- A manifest reorder must not change generated node identity or evidence keys.

#### NCK0-SB4 - Certification and correction-overlay constitution

**Independently testable outcome:** Native parity can be certified against TypeScript without placing TypeScript on the runtime query path or implementing bug-for-bug modes.

**Architecture:**

- Separate recomputable oracle snapshots from review-gated correction overlays.
- Require issue/evidence, semantic rationale, affected slices, and expiry review for each correction.
- Disallow production access to oracle values except static explanatory issue metadata explicitly approved by PUB0.

**Expected changes:**

- Amend TCM3 certification inputs and source atoms.
- Define deterministic canonicalization of provider diagnostics before comparison.

**Discriminating proof:**

- Planting a runtime provider callback, compat-mode query field, or unreviewed overlay must fail a critical guard.
- Recomputing an unchanged oracle corpus must produce byte-identical snapshots.

#### NCK0-SB5 - Atomic authority transition law

**Independently testable outcome:** A family can move from external ownership to native ownership without duplicates, gaps, or stale mixed publication.

**Architecture:**

- Define External, ObserveNative, CertifiedNative, and Disabled transitions.
- Bind transitions to exact profile, provider epoch, native implementation receipt, and certification receipt.
- Require latest-basis publication and cancellation of superseded observation work.

**Expected changes:**

- Amend COX0/LRA0/PUB0 transition and publication contracts.
- Define rollback only to the previous certified authority receipt, never to an implicit fallback.

**Discriminating proof:**

- State-machine tests must reject illegal transitions and mixed-epoch batches.
- A planted double-publication path must fail before user-visible output.

#### NCK0-SB6 - Critical guard and source-transfer index

**Independently testable outcome:** The constitution is mechanically tied to durable source atoms and named guards before legacy docs are deleted.

**Architecture:**

- Name guards for one resolver, no runtime oracle callback, no compat mode, exact authority, typed outcomes, and no monolithic program cache.
- Bind legacy Native Checker requirements to exact NCK targets and digests.

**Expected changes:**

- Register requirement atoms in `legacy-arch-reconciliation.md`.
- Add the future guard names to the authority catalog; implementation nodes activate them with code.

**Discriminating proof:**

- The legacy disposition validator must refuse deletion if any atom lacks a target charter.
- A renamed or removed guard without an amendment must fail authority validation.

### Identity, invalidation, and publication

- Diagnostic identity is independent of message wording and source position; it is rooted in family, rule, semantic subject, authored anchor identity, profile, and exact input basis.
- Severity and presentation are policy fields; they do not change semantic diagnostic identity or cache identity unless the rule itself branches on policy.
- A certified family result must name the facts and environment dimensions it read. Provider epoch enters only observation/cutover identity, never native semantic computation.
- Diagnostic ordering is deterministic: primary authored location, family ID, rule ID, semantic subject identity, then stable tie-breaker.
- Fixes are references to authored edit intents owned by LRA0/LSO7, not opaque text edits embedded in semantic facts.

### Migration and cutover

- Land this constitution and source atoms before deleting `docs/arch/native-checker.md`.
- Do not activate native query keys or publish native semantic diagnostics in NCK0.
- Classify every existing diagnostic producer and record unknown cases as blocking migration debt, not inferred ownership.
- Update successor DAG and existing contract charters in one amendment so no interim authority contradiction exists.

### Consumers and unlocks

- Unlocks NCK1 and all later native checker implementation.
- Provides the diagnostic authority contract consumed by CLI2, LSO8, COX0, LRA0, and PUB0 amendments.
- Defines the promotion law used by generated NCF family nodes.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **NCK0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or reproduced defect that existing evidence does not discriminate.
- **NCK0-AC2 — positive contract:** the named API/data boundary preserves exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **NCK0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm; otherwise bind a terse not-applicable rationale.
- **NCK0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, provider, filesystem, network, allocation, copy, or retained-candidate work; otherwise bind a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; do not add implementation mirrors, duplicate permutations, or universal test quotas.
- Test homes: `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`, `packages/typescript-plugin/src`, and the exact generated vertical fixture selected by this node.


### Pack-specific proof obligations

- **NCK0-AC-AUTHORITY:** generated ownership and transition tables cover every registered diagnostic origin and reject overlap.
- **NCK0-AC-ONE-ENGINE:** static guard text and architecture tests reject any checker semantic resolver surface.
- **NCK0-AC-CERTIFICATION:** correction-overlay and oracle rules are exact and contain no runtime compatibility path.
- **NCK0-AC-LEGACY:** every durable clause from `native-checker.md` has a digest-bound disposition.
- **NCK0-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **NCK0-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **NCK0-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **NCK0-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Deletions and forbidden designs

- Delete the legacy Native Checker prose only after all durable clauses are digest-bound to NCK0-NCK8 and generated-family authority.
- Delete any proposed checker-specific resolver or TypeScript compatibility-mode design from live authority.
- Delete ambiguous claims that a green coverage ledger alone proves TypeScript semantic parity.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

- A checker-private type walker, relation engine, overload resolver, flow engine, symbol table, or module resolver.
- Runtime tsserver/tsgo calls from a native Check query.
- One global native-checker enabled boolean used as a substitute for family/slice authority.
- Diagnostics stored as GraphTypeNode arms or identity-less side products.
- A monolithic whole-program cache entry or eager workspace check on an interactive leaf request.
- Permanent duplicate native and provider diagnostics hidden by message-text deduplication.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

- Delete or structurally reject every compatibility path that would preserve a second owner after cutover.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing, or unauthorized executable work.
- Performance budget: when preflight identifies touched authority or a hot path, use the ratified replacement SLO and equivalent-work counters below; otherwise performance evidence is not applicable and no soak is invented solely to populate evidence.

- The constitution must declare zero hidden work for Disabled and External-only native paths.
- Certification cost is test/offline work and must not enter runtime latency budgets.
- Target ceiling: 0 production LOC, 0 production files, and 0 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Abort conditions

- Abort if any diagnostic family cannot name a sole semantic fact owner and a sole publishing owner.
- Abort if certification requires generated TypeScript text to become semantic truth rather than oracle input.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

1. `programctl validate-authority --module expansion-native-checker` and source-coverage validation.
1. Schema tests for diagnostic family, authority state, result outcome, and correction overlay catalogs.
1. Negative mutations for duplicate owner, runtime provider callback, compat-mode field, and unclassified legacy clause.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired round handle; the complete gate command list; 3 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, `architecture-specialist`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-dag-amendment.md:L1`
- `source:legacy-arch-reconciliation.md:L1`

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-SUCCESSOR-DAG-AMENDMENT

- Kind: `context`
- Source: `successor-dag-amendment.md:1-1`
- Applicability: `EPR0`, `EPR1`, `EPR2`, `EPR3`, `EPR4`, `EPR5`, `EPR6`, `LSO0`, `LSO1`, `LSO10`, `LSO2`, `LSO3`, `LSO4`, `LSO5`, `LSO6`, `LSO7`, `LSO8`, `LSO9`, `NCF-AT-CYCLE`, `NCF-AT-QUERY`, `NCF-AT-REDUCE`, `NCF-BD-DUP`, `NCF-BD-INIT`, `NCF-BD-SCOPE`, `NCF-CF-CONTEXT`, `NCF-CF-THIS`, `NCF-CF-VAR`, `NCF-CO-CALL`, `NCF-CO-INFER`, `NCF-CO-OVER`, `NCF-FD-CFLOW`, `NCF-FD-DEF`, `NCF-FD-NARROW`, `NCF-JD-DEC`, `NCF-JD-JS`, `NCF-JD-JSDOC`, `NCF-JF-JSX`, `NCF-JF-SVELTE`, `NCF-JF-VUE`, `NCF-MP-AUG`, `NCF-MP-MODULE`, `NCF-MP-PROJECT`, `NCF-OC-HERIT`, `NCF-OC-MEM`, `NCF-OC-MERGE`, `NCF-RO-ASSIGN`, `NCF-RO-EXCESS`, `NCF-RO-OPER`, `NCK0`, `NCK1`, `NCK2`, `NCK3`, `NCK4`, `NCK5`, `NCK6`, `NCK7`, `NCK8`, `NCKF0`
- Exact text SHA-256: `9413cba2563db3ebfda5614b0ecd45ba6757581a4f7a20da7341ed2b3dc1d128`

~~~~markdown
# Rev11 legacy-architecture reconciliation and successor charter pack
~~~~

### SRC-LEGACY-NCK-AUTH-001

- Kind: `requirement`
- Source: `legacy-arch-reconciliation.md:15-21`
- Applicability: `NCK0`, `NCK3`, `NCF-AT-CYCLE`, `NCF-AT-QUERY`, `NCF-AT-REDUCE`, `NCF-BD-DUP`, `NCF-BD-INIT`, `NCF-BD-SCOPE`, `NCF-CF-CONTEXT`, `NCF-CF-THIS`, `NCF-CF-VAR`, `NCF-CO-CALL`, `NCF-CO-INFER`, `NCF-CO-OVER`, `NCF-FD-CFLOW`, `NCF-FD-DEF`, `NCF-FD-NARROW`, `NCF-JD-DEC`, `NCF-JD-JS`, `NCF-JD-JSDOC`, `NCF-JF-JSX`, `NCF-JF-SVELTE`, `NCF-JF-VUE`, `NCF-MP-AUG`, `NCF-MP-MODULE`, `NCF-MP-PROJECT`, `NCF-OC-HERIT`, `NCF-OC-MEM`, `NCF-OC-MERGE`, `NCF-RO-ASSIGN`, `NCF-RO-EXCESS`, `NCF-RO-OPER`
- Exact text SHA-256: `654de84730f7d32a408348bb81ee224674ac622afb1ecc93af88a782992a7825`

~~~~markdown
### NCK-AUTH-001 — One resolver, separate diagnostic authority

- Diagnostics may evaluate shared symbol/type/relation/call/flow/context/module/project facts.
- No checker-private resolver, type walker, relation engine, overload resolver, flow engine, symbol table, module resolver, or project graph exists.
- Semantic fact ownership remains with Rev11 authorities; diagnostic evaluation is owned by `expansion.native-checker`.
- Targets: `NCK0`, `NCK3`, every generated `NCF-*` charter.
- Source: `docs/arch/native-checker.md`, blob `3e96bf48ec481e97b9fd3067041e21099d194944`.
~~~~

### SRC-LEGACY-NCK-QUERY-001

- Kind: `requirement`
- Source: `legacy-arch-reconciliation.md:23-29`
- Applicability: `NCK0`, `NCK2`, `NCK7`
- Exact text SHA-256: `9372ba4833d43deb3133fe48c881dff958d1009effc4094bda39c522c1d28ec4`

~~~~markdown
### NCK-QUERY-001 — Scoped first-class diagnostic queries

- Diagnostic results are first-class query values, not `GraphTypeNode` arms or identity-less side products.
- Primitive operations are region/file/project-rule/expression scoped.
- Whole-program checking is a bounded coordinator/stream, not a monolithic cache key.
- Complete-only cache admission applies.
- Targets: `NCK0`, `NCK2`, `NCK7`.
~~~~

### SRC-LEGACY-NCK-CERT-001

- Kind: `requirement`
- Source: `legacy-arch-reconciliation.md:46-52`
- Applicability: `NCK0`, `NCK4`, `NCF-AT-CYCLE`, `NCF-AT-QUERY`, `NCF-AT-REDUCE`, `NCF-BD-DUP`, `NCF-BD-INIT`, `NCF-BD-SCOPE`, `NCF-CF-CONTEXT`, `NCF-CF-THIS`, `NCF-CF-VAR`, `NCF-CO-CALL`, `NCF-CO-INFER`, `NCF-CO-OVER`, `NCF-FD-CFLOW`, `NCF-FD-DEF`, `NCF-FD-NARROW`, `NCF-JD-DEC`, `NCF-JD-JS`, `NCF-JD-JSDOC`, `NCF-JF-JSX`, `NCF-JF-SVELTE`, `NCF-JF-VUE`, `NCF-MP-AUG`, `NCF-MP-MODULE`, `NCF-MP-PROJECT`, `NCF-OC-HERIT`, `NCF-OC-MEM`, `NCF-OC-MERGE`, `NCF-RO-ASSIGN`, `NCF-RO-EXCESS`, `NCF-RO-OPER`
- Exact text SHA-256: `fd3208a2cd7d054e945f223cf15017477ad01323bc7f6330a4c29f8924f68837`

~~~~markdown
### NCK-CERT-001 — Oracle outside runtime

- TypeScript/tsgo is a pinned oracle and residual owner, never called by native query-time checker evaluation.
- Native resolver behavior is single-spec.
- Clear TypeScript bugs are represented by review-gated correction-overlay data, not a compatibility mode or cache-key dimension.
- Targets: `NCK0`, `NCK4`, generated `NCF-*` nodes.
- Related source: legacy TypeScript compatibility model.
~~~~

### SRC-LEGACY-TRANSFER-3E96BF48EC48

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:320-325`
- Applicability: `NCK0`, `NCK1`, `NCK2`, `NCK3`, `NCK4`, `NCK5`, `NCK6`, `NCK7`, `NCK8`, `NCKF0`
- Exact text SHA-256: `9e49f7ccc41ffb05aa34dea9d059d333873074c8f55d5c84b5438285a90737e2`

~~~~markdown
### LEGACY-TRANSFER-3E96BF48EC48

- Original path: `docs/arch/native-checker.md`; Git blob: `3e96bf48ec481e97b9fd3067041e21099d194944`; exact source SHA-256: `2a7124d22a468e005faad16b43bf2d64a5472e3bea30bb39f436c2f33b1cde06`.
- Exact retained source: `sources/legacy-architecture-transfers/native-checker.md`.
- Applicable authority: `NCK0`, `NCK1`, `NCK2`, `NCK3`, `NCK4`, `NCK5`, `NCK6`, `NCK7`, `NCK8`, `NCKF0`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~
