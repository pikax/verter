<!-- unified-charter-v2
id=PAR0
name=Parser decision, ownership, reuse, and lineage contract
phase=expansion
train=expansion.kernel
product=kernel
kind=contract
semantic_role=delivery
class=successor
predecessors=CPF1,VID0
conditional_predecessors=
owner=expansion.kernel:typed immutable universal catalog and demand-selected kernel services
conflict_domains=source_lineage,carrier_parser
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
source_refs=source:successor-expansion.md:L815
external_requirements=
activation_gate=ORC0
charter=charters/expansion-kernel/PAR0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# PAR0 — Parser decision, ownership, reuse, and lineage contract

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Parser decision, ownership, reuse, and lineage contract. The current owner is **framework-shaped host/session registries and untagged public boundaries**. The final and sole owner is **typed immutable universal catalog and demand-selected kernel services**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `crates/verter_protocol/src`, `crates/verter_identity/src`.
- Named API/data boundaries: `CarrierProfileId`, `FrameworkProfileId`, `ProjectProfileId`, `CatalogSnapshot`, `DemandPlan`, `TypeInfoRequest`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **CPF1:** exact current receipt ID and digest for “Carrier frontend registration and Vue/Svelte cutover”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **VID0:** exact current receipt ID and digest for “Orthogonal identities and exact-release law”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** make parser choice evidence-based per carrier while preventing both arbitrary parser proliferation and an omni parser.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **central framework switch**, **untagged coordinate/public identity**, **duplicate component information authority** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **PAR0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **PAR0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **PAR0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **PAR0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
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

- `source:successor-expansion.md:L815`

## Reconciled source-plan contract

**Intent:** make parser choice evidence-based per carrier while preventing both arbitrary parser proliferation and an omni parser.
**Predecessors:** `CPF1`, `VID0`.
**Subblocks:** (1) define `ParserDecision`; (2) key ownership by carrier profile + grammar epoch; (3) define safe reuse equality and cache keys; (4) define fork lineage/license/corpus recording; (5) define lossless recovery, error, fuzz, and budget obligations; (6) reserve evidence-gated HTML-family extraction.
**Acceptance:** negative fixtures reject content-hash-only reuse, TSX parser copies, framework switches in a neutral parser, and a tooling-only carrier forced through a compiler backend.
**Forbidden:** global parser family authority, “HTML-like” as a cache key, shared recovery semantics without proof, or parser selection from an unresolved framework name.
**Deletion/abort:** delete any central grammar match made obsolete by owner-local registration; rescope a vertical when its closest parser fails the pinned grammar/recovery corpus.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1917-F56A2F4EDE95

- Kind: `context`
- Source: `compiler-proposal.md:1917-1917`
- Applicability: `PAR0`
- Exact text SHA-256: `f56a2f4ede9503edbdb49b299a713a68bf1c612c66390bb4f2969888a8cc1318`

~~~~markdown
## 11.3 `PAR0`
~~~~

### SRC-COMP-L1919-241FF749AA41

- Kind: `context`
- Source: `compiler-proposal.md:1919-1919`
- Applicability: `PAR0`
- Exact text SHA-256: `241ff749aa418d8da870f3c7c3df91ffecdf78c471a6d1541aa070b14cd466c9`

~~~~markdown
Add explicit consumption of:
~~~~

### SRC-COMP-L1921-AA2D18D56BE8

- Kind: `context`
- Source: `compiler-proposal.md:1921-1921`
- Applicability: `PAR0`
- Exact text SHA-256: `aa2d18d56be88c4677ae0153b7a09ef588c982aaf5f5aae0410229d355bf2022`

~~~~markdown
- source-backed lexical surface and recovery sidecars;
~~~~

### SRC-COMP-L1922-6111E4699743

- Kind: `context`
- Source: `compiler-proposal.md:1922-1922`
- Applicability: `PAR0`
- Exact text SHA-256: `6111e4699743169f552344f304d244c164ab4559823abd4222b2559f6c7a2a3e`

~~~~markdown
- parser-owned `ParseAdmission`;
~~~~

### SRC-COMP-L1923-C868D02CC86D

- Kind: `context`
- Source: `compiler-proposal.md:1923-1923`
- Applicability: `PAR0`
- Exact text SHA-256: `c868d02cc86db5f4161d09a9fbf64fdecfc3a52f2d13f7629d1ced9752dae34a`

~~~~markdown
- direct strict path permitted to avoid full tooling-sidecar materialization;
~~~~

### SRC-COMP-L1924-070759D93A9C

- Kind: `requirement`
- Source: `compiler-proposal.md:1924-1924`
- Applicability: `PAR0`
- Exact text SHA-256: `070759d93a9c33817759c9a1d1f46fa1d71781edae44c146080cc49f0ad6314c`

~~~~markdown
- at most one authoritative parse per exact region/grammar contract;
~~~~

### SRC-COMP-L1925-C55F38C1E111

- Kind: `context`
- Source: `compiler-proposal.md:1925-1925`
- Applicability: `PAR0`
- Exact text SHA-256: `c55f38c1e111e561a88bfa1aa54608b203f16c9ef2d34e73949e0753c12664a1`

~~~~markdown
- no redundant whole-source rescans;
~~~~

### SRC-COMP-L1926-CCE66EA99721

- Kind: `context`
- Source: `compiler-proposal.md:1926-1926`
- Applicability: `PAR0`
- Exact text SHA-256: `cce66ea9972116aafaa8268e85647b6967b02a03941303f66910091a2db4f5fd`

~~~~markdown
- raw authored text source-backed;
~~~~

### SRC-COMP-L1927-3E643FBA36DA

- Kind: `context`
- Source: `compiler-proposal.md:1927-1927`
- Applicability: `PAR0`
- Exact text SHA-256: `3e643fba36da60c4ca79427d1fdd3176fbb8bc420be21d99709e447ece301097`

~~~~markdown
- dense syntax IDs separate from authored offsets and cross-revision lineage.
~~~~

### SRC-COMP-L1929-E549198EAA49

- Kind: `forbidden`
- Source: `compiler-proposal.md:1929-1929`
- Applicability: `PAR0`
- Exact text SHA-256: `e549198eaa4975376efeacc97c8720a1b30399123a1abd9fc6a8ceb4d115338e`

~~~~markdown
`PAR0` must not own `SemanticAdmission` or `CompileAdmission`.
~~~~

### SRC-EXP-L815-8126F7EC831B

- Kind: `context`
- Source: `successor-expansion.md:815-815`
- Applicability: `PAR0`
- Exact text SHA-256: `8126f7ec831ba5d945fc11937e1bd3302de24ea78a4fe60303cdaf23cfe5e2d2`

~~~~markdown
### `PAR0.md` — Parser decision, ownership, reuse, and lineage contract
~~~~

### SRC-EXP-L817-05C7B3DA3F90

- Kind: `forbidden`
- Source: `successor-expansion.md:817-822`
- Applicability: `PAR0`
- Exact text SHA-256: `05c7b3da3f90122e451eab25c9d18c74bd992a1efad199c4c822fb0edfbd13b5`

~~~~markdown
**Intent:** make parser choice evidence-based per carrier while preventing both arbitrary parser proliferation and an omni parser.
**Predecessors:** `CPF1`, `VID0`.
**Subblocks:** (1) define `ParserDecision`; (2) key ownership by carrier profile + grammar epoch; (3) define safe reuse equality and cache keys; (4) define fork lineage/license/corpus recording; (5) define lossless recovery, error, fuzz, and budget obligations; (6) reserve evidence-gated HTML-family extraction.
**Acceptance:** negative fixtures reject content-hash-only reuse, TSX parser copies, framework switches in a neutral parser, and a tooling-only carrier forced through a compiler backend.
**Forbidden:** global parser family authority, “HTML-like” as a cache key, shared recovery semantics without proof, or parser selection from an unresolved framework name.
**Deletion/abort:** delete any central grammar match made obsolete by owner-local registration; rescope a vertical when its closest parser fails the pinned grammar/recovery corpus.
~~~~
