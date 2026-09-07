<!-- unified-charter-v2
id=EMB0
name=Embedded codecs and exact authored map chains
phase=expansion
train=expansion.kernel
product=kernel
kind=contract
semantic_role=delivery
class=successor
predecessors=EAK0,ENC1
owner=expansion.kernel:typed immutable universal catalog and demand-selected kernel services
conflict_domains=mapping_geometry
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
external_requirements=
charter=charters/expansion-kernel/EMB0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# EMB0 — Embedded codecs and exact authored map chains

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Embedded codecs and exact authored map chains. The current owner is **framework-shaped host/session registries and untagged public boundaries**. The final and sole owner is **typed immutable universal catalog and demand-selected kernel services**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `crates/verter_protocol/src`, `crates/verter_identity/src`.
- Named API/data boundaries: `CarrierProfileId`, `FrameworkProfileId`, `ProjectProfileId`, `CatalogSnapshot`, `DemandPlan`, `TypeInfoRequest`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **EAK0:** implemented ledger row for “Canonical-symbol provenance and role activation”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **ENC1:** implemented ledger row for “Tagged boundary conversion convergence”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- **Normative intent:** provide reusable authored embedded-region geometry without centralizing language semantics.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Inventory evidence:** enumerate every in-scope outcome, consumer and displaced production route before validating ownership. Assign each outcome and consumer exactly one implementation owner, and each displaced production route exactly one later production-capable deletion/rejection owner, bound to its concrete DAG node, successor path and receiving acceptance criterion. This contract node proves inventory completeness and unambiguous ownership; later implementation nodes prove production deletion.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **EMB0-AC1 — ownership contract:** enumerate every in-scope outcome, consumer and displaced production route. Each outcome/consumer has exactly one implementation owner; each displaced production route has exactly one later production-capable deletion/rejection owner. Bind owners to existing DAG node IDs, valid successor paths and receiving acceptance IDs under contracts/successor-charter-quality.md. The contract-owned schema/validator must reject missing inventory members, unknown/pathless owners and conflicting assignments before this node completes; production deletion remains acceptance of the later owner.
- **EMB0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **EMB0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **EMB0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_language/tests`, `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`.

## Deletions and forbidden designs

- Inventory and assign the later deletion/rejection owner for: **central framework switch**. This node changes no production route.
- Inventory and assign the later deletion/rejection owner for: **untagged coordinate/public identity**. This node changes no production route.
- Inventory and assign the later deletion/rejection owner for: **duplicate component information authority**. This node changes no production route.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance acceptance: use the exact applicable metric rows and methodology from performance-gates.toml, the applicable MEM0 budget, or the owning ratified product catalog, under contracts/resource-and-finalization.md (L2). Exact work invariants, latency/allocation/RSS limits under their owning methodology, and bounded new-capability budgets are distinct. New capabilities and deliberate pressure policies declare bounded new work and replacement SLOs before measurement. Missing required coverage needs an owning-contract amendment before measurement; no implicit 0.0% threshold or post-hoc rebaseline applies.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_language -p verter_protocol -p verter_session`
2. Run every final command in the bound `docs-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Reconciled source-plan contract

**Intent:** provide reusable authored embedded-region geometry without centralizing language semantics.
**Predecessors:** `EAK0`, `ENC1`.
**Subblocks:** (1) consume repaired `SourceUnitId` plus stable `AttachmentId`/`RegionId` for nested lineage without a second embedded-source identity; (2) profile-declared raw-versus-cooked selection; (3) validate cooked values as Unicode scalar text, encode them as UTF-8, and return typed `NonUnicodeCookedLiteral`/missing-cooked-value partiality before parsing otherwise; (4) delimiter, escape, CRLF, indentation, and interpolation-hole policies; (5) exact authored-UTF-8 map-chain composition, attachment, and base-URI identity; (6) incremental splice/fuzz/allocation corpus including lone surrogate, surrogate pair, invalid tagged escape, line continuation, CRLF, escaped delimiter, and holes.
**Acceptance:** Vue, Angular, and Lit fixtures share codec primitives but produce profile-owned activation/grammar/hole results; JavaScript cooked values containing lone surrogates never acquire a false UTF-8 position and fail with typed partiality before embedded parsing; every admitted returned position maps exactly to authored UTF-8 bytes or declares why it cannot.
**Forbidden:** nearest-token guesses, a standalone `EmbeddedSourceId`, private UTF-16/WTF-8 offsets escaping the codec, one embedded-language parser, dynamic code execution, unbounded nesting, or a universal projection mask.
**Deletion/abort:** replace duplicate literal decoders only after byte/map equivalence; abort sharing when a language’s authored semantics cannot be represented without branches in the neutral codec.

