<!-- unified-charter-v2
id=VIM0
name=Vertical conformance manifest schema
phase=expansion
train=expansion.kernel
product=kernel
kind=contract
semantic_role=delivery
class=successor
predecessors=CAT0,PAR0,DEM0
conditional_predecessors=
owner=expansion.kernel:typed immutable universal catalog and demand-selected kernel services
conflict_domains=carrierprofileid
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
source_refs=source:successor-expansion.md:L986
external_requirements=
activation_gate=ORC0
charter=charters/expansion-kernel/VIM0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# VIM0 — Vertical conformance manifest schema

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Vertical conformance manifest schema. The current owner is **framework-shaped host/session registries and untagged public boundaries**. The final and sole owner is **typed immutable universal catalog and demand-selected kernel services**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `crates/verter_protocol/src`, `crates/verter_identity/src`.
- Named API/data boundaries: `CarrierProfileId`, `FrameworkProfileId`, `ProjectProfileId`, `CatalogSnapshot`, `DemandPlan`, `TypeInfoRequest`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **CAT0:** exact current receipt ID and digest for “Immutable typed catalog snapshot and static registration”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **PAR0:** exact current receipt ID and digest for “Parser decision, ownership, reuse, and lineage contract”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **DEM0:** exact current receipt ID and digest for “Selection, two-stage activation, and demand planning”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** encode all architecture obligations once per exact vertical release.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **central framework switch**, **untagged coordinate/public identity**, **duplicate component information authority** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **VIM0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **VIM0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **VIM0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **VIM0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
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

- `source:successor-expansion.md:L986`

## Reconciled source-plan contract

**Intent:** encode all architecture obligations once per exact vertical release.
**Predecessors:** `CAT0`, `PAR0`, `DEM0`.
**Subblocks:** (1) `vertical.toml` identity/geometry/ownership/version; (2) `capabilities.toml`; (3) `rules/*.toml`; (4) `oracles.lock`; (5) `fixtures/manifest.toml`; (6) schema for parser lineage, activation stages, embeddings, maps, TypeInfo roles, CE dispositions, coexistence, public surfaces, budgets, compiler state, deletions, and forbidden dependencies.
**Acceptance:** manifests for current Vue/Svelte and synthetic HTML, React, Lit, Angular, and project-profile cases validate; versions arrays, untyped extension bags, missing CE rows, and compiler/parser conflation fail structurally.
**Forbidden:** manifest-owned implementation logic, dynamic plugins, prose-only capability rows, or self-selected performance gates.
**Deletion/abort:** generated artifacts become consumers, not co-authorities; rescope schema when a representative geometry needs an untyped escape hatch.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1939-DC752EC1C6B2

- Kind: `context`
- Source: `compiler-proposal.md:1939-1939`
- Applicability: `VIM0`
- Exact text SHA-256: `dc752ec1c6b25b500f8d7058a17524c7849ec76e005c166663519b5c48389121`

~~~~markdown
## 11.6 `VIM0` / `VIM1`
~~~~

### SRC-COMP-L1941-907C128AA707

- Kind: `context`
- Source: `compiler-proposal.md:1941-1941`
- Applicability: `VIM0`
- Exact text SHA-256: `907c128aa707ae8c9f02c800fe9a05c0070a61a4b49be4a328b1053a0a0c1365`

~~~~markdown
Add compiler manifest cells for:
~~~~

### SRC-COMP-L1943-59D0655A3B2C

- Kind: `deletion`
- Source: `compiler-proposal.md:1943-1957`
- Applicability: `VIM0`
- Exact text SHA-256: `59d0655a3b2c56e78ecf6113640ddc57601193845f17d03acae78c7c8a2ede26`

~~~~markdown
```text
CompilePolicy::Default support
CompilePolicy::Optimized disposition
DefaultCompilationContractId
framework semantic epoch
targets and target-specific options
style prerequisites and external-stage cells
artifact roles/relations
map contracts
strict compile admission
work-ledger and performance budgets
old compiler deletion owner
custom-block disposition
host integration cells
```
~~~~

### SRC-COMP-L1959-9F97A96D854B

- Kind: `requirement`
- Source: `compiler-proposal.md:1959-1959`
- Applicability: `VIM0`
- Exact text SHA-256: `9f97a96d854bc2d505a08c88f1e19a12cea195822404c05340599549e2126998`

~~~~markdown
The manifest must reject a tooling vertical that implicitly claims compiler support.
~~~~

### SRC-EXP-L986-623904B73A02

- Kind: `context`
- Source: `successor-expansion.md:986-986`
- Applicability: `VIM0`
- Exact text SHA-256: `623904b73a024ea1eaaf31a42d3c73a8c9674990f78ebf5350963b259bb77c2e`

~~~~markdown
### `VIM0.md` — Vertical conformance manifest schema
~~~~

### SRC-EXP-L988-4888FFCF6C7A

- Kind: `forbidden`
- Source: `successor-expansion.md:988-993`
- Applicability: `VIM0`
- Exact text SHA-256: `4888ffcf6c7af8cc018fd849c1554d6b9f5274de5f5a5354a0274221ba9c9fbc`

~~~~markdown
**Intent:** encode all architecture obligations once per exact vertical release.
**Predecessors:** `CAT0`, `PAR0`, `DEM0`.
**Subblocks:** (1) `vertical.toml` identity/geometry/ownership/version; (2) `capabilities.toml`; (3) `rules/*.toml`; (4) `oracles.lock`; (5) `fixtures/manifest.toml`; (6) schema for parser lineage, activation stages, embeddings, maps, TypeInfo roles, CE dispositions, coexistence, public surfaces, budgets, compiler state, deletions, and forbidden dependencies.
**Acceptance:** manifests for current Vue/Svelte and synthetic HTML, React, Lit, Angular, and project-profile cases validate; versions arrays, untyped extension bags, missing CE rows, and compiler/parser conflation fail structurally.
**Forbidden:** manifest-owned implementation logic, dynamic plugins, prose-only capability rows, or self-selected performance gates.
**Deletion/abort:** generated artifacts become consumers, not co-authorities; rescope schema when a representative geometry needs an untyped escape hatch.
~~~~
