<!-- unified-charter-v2
id=TIF0
name=TypeInfo query/selector and authority-composition contract
phase=expansion
train=expansion.kernel
product=kernel
kind=contract
semantic_role=delivery
class=successor
predecessors=DEM0,ENC1
conditional_predecessors=
owner=expansion.kernel:typed immutable universal catalog and demand-selected kernel services
conflict_domains=style_semantics,semantic_authority
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
source_refs=source:successor-expansion.md:L896
external_requirements=
activation_gate=ORC0
charter=charters/expansion-kernel/TIF0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# TIF0 — TypeInfo query/selector and authority-composition contract

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

TypeInfo query/selector and authority-composition contract. The current owner is **framework-shaped host/session registries and untagged public boundaries**. The final and sole owner is **typed immutable universal catalog and demand-selected kernel services**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `crates/verter_protocol/src`, `crates/verter_identity/src`.
- Named API/data boundaries: `CarrierProfileId`, `FrameworkProfileId`, `ProjectProfileId`, `CatalogSnapshot`, `DemandPlan`, `TypeInfoRequest`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **DEM0:** exact current receipt ID and digest for “Selection, two-stage activation, and demand planning”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **ENC1:** exact current receipt ID and digest for “Tagged boundary conversion convergence”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** establish the canonical public TypeInfo façade while consuming—not redefining—the accepted Rev11 observation/runtime identity law.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **central framework switch**, **untagged coordinate/public identity**, **duplicate component information authority** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **TIF0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **TIF0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **TIF0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **TIF0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_language/tests`, `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **central framework switch**.
- Delete or structurally reject: **untagged coordinate/public identity**.
- Delete or structurally reject: **duplicate component information authority**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_language -p verter_protocol -p verter_session`
2. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 3 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, `architecture-specialist`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L896`

## Reconciled source-plan contract

**Intent:** establish the canonical public TypeInfo façade while consuming—not redefining—the accepted Rev11 observation/runtime identity law.
**Predecessors:** `DEM0`, `ENC1`.
**Subblocks:** (1) import `CertifiedTypeEngineBinding`, `InputBasisId`/`TypeObservationBasis`, generic `QueryIdentity`, `ResultContractId`, and `SemanticFlightKey` from the accepted owner; (2) define position/file-name/project-name/workspace-name selectors with source-revision versus captured-view bases; (3) define TypeInfo-specific operation descriptors and canonical equality material; (4) define owner-routed native/TS/composed operation plans; (5) define authority/provenance/completeness/candidate/ambiguity/budget DTOs with actual completeness only in result provenance; (6) bind observation caching/invalidation to the accepted runtime/G2 law.
**Acceptance:** native-only, TypeScript-authoritative, composed framework+TS, ambiguous, stale-backend, changed-map, and missing-input fixtures produce distinct truthful results; TypeInfo adds no generic flight/key authority and no downstream block redefines it.
**Forbidden:** field-wise winner merging, native recreation of authoritative TS facts, first-match name search, provider handles in DTOs, or the index acting as checker.
**Deletion/abort:** supersede broad `TypeProvider`-shaped public requests after all consumers move; abort if an operation lacks exactly one ratified execution owner.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L896-11D230A0FF28

- Kind: `context`
- Source: `successor-expansion.md:896-896`
- Applicability: `TIF0`
- Exact text SHA-256: `11d230a0ff285a47969f1832743585e99da8b6e9c70ff62ed7e262dc2b7b0e48`

~~~~markdown
### `TIF0.md` — TypeInfo query/selector and authority-composition contract
~~~~

### SRC-EXP-L898-B956C69BA783

- Kind: `forbidden`
- Source: `successor-expansion.md:898-903`
- Applicability: `TIF0`
- Exact text SHA-256: `b956c69ba78371e3768362caae746f4de33969f1c959b6463cda263af9e82bee`

~~~~markdown
**Intent:** establish the canonical public TypeInfo façade while consuming—not redefining—the accepted Rev11 observation/runtime identity law.
**Predecessors:** `DEM0`, `ENC1`.
**Subblocks:** (1) import `CertifiedTypeEngineBinding`, `InputBasisId`/`TypeObservationBasis`, generic `QueryIdentity`, `ResultContractId`, and `SemanticFlightKey` from the accepted owner; (2) define position/file-name/project-name/workspace-name selectors with source-revision versus captured-view bases; (3) define TypeInfo-specific operation descriptors and canonical equality material; (4) define owner-routed native/TS/composed operation plans; (5) define authority/provenance/completeness/candidate/ambiguity/budget DTOs with actual completeness only in result provenance; (6) bind observation caching/invalidation to the accepted runtime/G2 law.
**Acceptance:** native-only, TypeScript-authoritative, composed framework+TS, ambiguous, stale-backend, changed-map, and missing-input fixtures produce distinct truthful results; TypeInfo adds no generic flight/key authority and no downstream block redefines it.
**Forbidden:** field-wise winner merging, native recreation of authoritative TS facts, first-match name search, provider handles in DTOs, or the index acting as checker.
**Deletion/abort:** supersede broad `TypeProvider`-shaped public requests after all consumers move; abort if an operation lacks exactly one ratified execution owner.
~~~~

### SRC-LEGACY-TRANSFER-C22A814BC030

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:33-38`
- Applicability: `TIF0`, `TCM3`
- Exact text SHA-256: `9e83775264a7035283aecc867eda7cb16c9950e0154c5054ec36a5befc4f8a3d`

~~~~markdown
### LEGACY-TRANSFER-C22A814BC030

- Original path: `docs/arch/followups/u0-typeinfo-scratch-fact-validation.md`; Git blob: `c22a814bc0303053f2dc0ba0a98237ee49f60e42`; exact source SHA-256: `93b3ff687a3ad6bcb0944d5a5a45f2c0b811ad2ae444e54fa4d8dbe84fa1f160`.
- Exact retained source: `sources/legacy-architecture-transfers/followups/u0-typeinfo-scratch-fact-validation.md`.
- Applicable authority: `TIF0`, `TCM3`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-B69FCDB81AA0

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:194-199`
- Applicability: `TIF0`, `H1`
- Exact text SHA-256: `15ff8b6fd1b28b2666d1c1a79da33f869a1bc58195676457ed3a7b5a3d95e0c1`

~~~~markdown
### LEGACY-TRANSFER-B69FCDB81AA0

- Original path: `docs/arch/future/typeinfo-inplace-package-edit-supersession.md`; Git blob: `b69fcdb81aa094599f7739b309cb4eef21513052`; exact source SHA-256: `9cc6959d5c9a418d4551a5e8078903b824437fdefcbee85ab1026b6319d7e1d5`.
- Exact retained source: `sources/legacy-architecture-transfers/future/typeinfo-inplace-package-edit-supersession.md`.
- Applicable authority: `TIF0`, `H1`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-5CE6EF3AD364

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:579-584`
- Applicability: `TIF0`, `TIF1`
- Exact text SHA-256: `876f31646a347665cadfe2d45b1eb20aa06b9e600b09a1fc4f1335f39f727b1c`

~~~~markdown
### LEGACY-TRANSFER-5CE6EF3AD364

- Original path: `docs/arch/typed-ir-cutover/compat-heuristic-mapping.md`; Git blob: `5ce6ef3ad3646b58b83c924384e9e23959414163`; exact source SHA-256: `0821e46260a13eed33e7419b1dc7fd8c176b72bb151e871f3a4a9efd02532b03`.
- Exact retained source: `sources/legacy-architecture-transfers/typed-ir-cutover/compat-heuristic-mapping.md`.
- Applicable authority: `TIF0`, `TIF1`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~
