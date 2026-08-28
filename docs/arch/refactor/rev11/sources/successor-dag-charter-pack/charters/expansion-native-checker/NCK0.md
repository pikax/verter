<!-- unified-charter-v2
id=NCK0
name=Native diagnostic authority and parity-certification constitution
phase=expansion
train=expansion.native-checker
product=native_checker
kind=constitution
semantic_role=delivery
class=successor
predecessors=UAK1,D8,E4,G2,TCM3,TIF1,LRA0,PUB0
conditional_predecessors=
owner=expansion.native-checker:sole native semantic diagnostic authority and certified-family cutover
conflict_domains=semantic_authority,diagnostic_action_service,public_protocol
resource_class=docs-light
review_profile=architecture-3
gate_profile=docs-domain
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
charter=charters/expansion-native-checker/NCK0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCK0 - Native diagnostic authority and parity-certification constitution

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Ratify the native semantic checker constitution: one diagnostic authority over the existing resolver, a typed diagnostic result model, a family and feature-slice certification law, and an atomic provider-to-native cutover protocol. This block changes authority and contracts only; it does not implement checker execution.

The current owner is **fragmented parser diagnostics, framework-specific checks, lint registration, provider diagnostics, LSP merge logic, and legacy Native Checker prose**. The final and sole owner is **the native checker product constitution, with semantic facts owned by their existing resolver and diagnostic evaluation owned by expansion.native-checker**.

## Architectural role and end state

NCK0 prevents the checker from becoming a second type system. It defines the ownership boundary between semantic fact production, diagnostic evaluation, framework contributions, external oracle certification, lint, publication, and fixes. Every later NCK and generated NCF node must be mechanically derivable from this constitution.

## Expected production surfaces

- `docs/arch/refactor/rev11/sources/legacy-arch-reconciliation.md` and generated authority catalogs
- `crates/verter_identity/src` for stable diagnostic, family, rule, and certification identities
- `crates/verter_protocol` for the future public diagnostic batch contract owned with PUB0
- `crates/verter_diagnostics`, `crates/verter_actions`, `crates/verter_semantic`, and `crates/verter_session` as future implementation owners
- `crates/verter_type_runtime` and `crates/verter_lsp` only for certified observation and cutover boundaries, never native semantic computation

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `DiagnosticOrigin`, `DiagnosticFamilyId`, `DiagnosticFeatureSliceId`, and `DiagnosticRuleId`
- `DiagnosticAuthorityState::{External, ObserveNative, CertifiedNative, Disabled}`
- `DiagnosticCertification` and immutable family certification receipts
- `DiagnosticBasis`, `DiagnosticCompleteness`, and typed operational outcomes
- `CorrectionOverlayEntry` as test and certification data, not a runtime compatibility mode
- `DiagnosticDedupKey` and the law that one family/profile/slice has one publishing authority

## Exact predecessor contracts

- **UAK1:** consume the universal-tooling constitution and product split so the checker is a successor product rather than an amendment hidden inside Rev11 finalization.
- **D8:** consume complete shared flow, call, contextual, and relation result admission; incomplete flow facts may not be relabeled as checker results.
- **E4:** consume reclaimable semantic storage and scoped interning so checker results cannot retain the whole project graph.
- **G2:** consume same-key singleflight ownership and ReturnOnly admission laws for checker query families.
- **TCM3:** consume the certified TypeScript semantic capability and observation identity contract; external TypeScript is oracle/fallback authority, never native query-time computation.
- **TIF1:** consume the TypeInfo-first public semantic contract and component metadata cutover.
- **LRA0:** consume diagnostic, rule, suppression, action, and authored-fix ownership boundaries.
- **PUB0:** consume the versioned public result/outcome vocabulary and truthful capability law.

External custody: none beyond the package activation boundary.

## Binding architecture

- Exactly one resolver remains authoritative for symbols, types, relation, calls, overloads, contextual typing, and flow. The checker evaluates diagnostic rules over those facts and may not recompute them.
- Diagnostic classes remain distinct: parser/recovery, native semantic, framework semantic, lint, external provider, and project/configuration diagnostics share a public shape but not authority or suppression rules.
- Certification and cutover occur at `profile + diagnostic family + semantic feature slice`; never at a vague project-wide percentage or one global boolean.
- External TypeScript is the oracle and fallback owner for uncertified families. Native checker execution never invokes tsserver or tsgo to decide a native result.
- The resolver has one correctness behavior. A reviewed correction overlay records conceded TypeScript bugs for certification; no user-facing compat mode or cache-key spec dimension exists.
- Every diagnostic carries authored provenance, exact input basis, completeness, and optional proof/fix references. Identity-less side effects are forbidden.
- Shadow observation is non-publishing. Promotion to CertifiedNative atomically suppresses the external family before native publication becomes visible.
- No monolithic CheckProgram cache entry is allowed. Program checks are coordinators over scoped region, file, and project-rule queries.

## Internal subblocks

### NCK0-SB1 - Diagnostic ownership matrix

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

### NCK0-SB2 - Typed result and operational outcome law

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

### NCK0-SB3 - Family and feature-slice taxonomy

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

### NCK0-SB4 - Certification and correction-overlay constitution

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

### NCK0-SB5 - Atomic authority transition law

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

### NCK0-SB6 - Critical guard and source-transfer index

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

## Data, identity, invalidation, and publication laws

- Diagnostic identity is independent of message wording and source position; it is rooted in family, rule, semantic subject, authored anchor identity, profile, and exact input basis.
- Severity and presentation are policy fields; they do not change semantic diagnostic identity or cache identity unless the rule itself branches on policy.
- A certified family result must name the facts and environment dimensions it read. Provider epoch enters only observation/cutover identity, never native semantic computation.
- Diagnostic ordering is deterministic: primary authored location, family ID, rule ID, semantic subject identity, then stable tie-breaker.
- Fixes are references to authored edit intents owned by LRA0/LSO7, not opaque text edits embedded in semantic facts.

## Migration and cutover

- Land this constitution and source atoms before deleting `docs/arch/native-checker.md`.
- Do not activate native query keys or publish native semantic diagnostics in NCK0.
- Classify every existing diagnostic producer and record unknown cases as blocking migration debt, not inferred ownership.
- Update successor DAG and existing contract charters in one amendment so no interim authority contradiction exists.

## Deletions

- Delete the legacy Native Checker prose only after all durable clauses are digest-bound to NCK0-NCK8 and generated-family authority.
- Delete any proposed checker-specific resolver or TypeScript compatibility-mode design from live authority.
- Delete ambiguous claims that a green coverage ledger alone proves TypeScript semantic parity.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- A checker-private type walker, relation engine, overload resolver, flow engine, symbol table, or module resolver.
- Runtime tsserver/tsgo calls from a native Check query.
- One global native-checker enabled boolean used as a substitute for family/slice authority.
- Diagnostics stored as GraphTypeNode arms or identity-less side products.
- A monolithic whole-program cache entry or eager workspace check on an interactive leaf request.
- Permanent duplicate native and provider diagnostics hidden by message-text deduplication.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **NCK0-AC-AUTHORITY:** generated ownership and transition tables cover every registered diagnostic origin and reject overlap.
- **NCK0-AC-ONE-ENGINE:** static guard text and architecture tests reject any checker semantic resolver surface.
- **NCK0-AC-CERTIFICATION:** correction-overlay and oracle rules are exact and contain no runtime compatibility path.
- **NCK0-AC-LEGACY:** every durable clause from `native-checker.md` has a digest-bound disposition.
- **NCK0-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **NCK0-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **NCK0-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **NCK0-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- The constitution must declare zero hidden work for Disabled and External-only native paths.
- Certification cost is test/offline work and must not enter runtime latency budgets.
- Target ceiling: 0 production LOC, 0 production files, and 0 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if any diagnostic family cannot name a sole semantic fact owner and a sole publishing owner.
- Abort if certification requires generated TypeScript text to become semantic truth rather than oracle input.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `programctl validate-authority --module expansion-native-checker` and source-coverage validation.
1. Schema tests for diagnostic family, authority state, result outcome, and correction overlay catalogs.
1. Negative mutations for duplicate owner, runtime provider callback, compat-mode field, and unclassified legacy clause.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Unlocks NCK1 and all later native checker implementation.
- Provides the diagnostic authority contract consumed by CLI2, LSO8, COX0, LRA0, and PUB0 amendments.
- Defines the promotion law used by generated NCF family nodes.

## Source reconciliation

- `docs/arch/native-checker.md` blob `3e96bf48ec481e97b9fd3067041e21099d194944`.
- `docs/arch/native-typeinfo-parity.md` and the D/E/G/TCM authority it was partially absorbed into.
- `docs/arch/ts-compat-two-mode-model.md` durable single-spec/correction-overlay decision.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.
