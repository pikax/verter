<!-- unified-charter-v2
id=VCE0
name=Vue Custom Element producer and consumer retrofit
phase=expansion
train=expansion.vue-ce
product=vue_ce
kind=terminal
semantic_role=delivery
class=successor
predecessors=HWC3,EAK1,SKL3
conditional_predecessors=
owner=expansion.vue-ce:Vue release-profile CustomElement producer and consumer adapter
conflict_domains=vue_custom_element_producer_and_consumer_retrofit,vue_product
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
size=S
dispatchable=true
optional=false
release_gating=product
source_refs=source:successor-expansion.md:L1186
external_requirements=
activation_gate=ORC0
charter=charters/expansion-vue-ce/VCE0.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# VCE0 — Vue Custom Element producer and consumer retrofit

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Vue Custom Element producer and consumer retrofit. The current owner is **the current source owners enumerated in the VCE0 migration manifest**. The final and sole owner is **Vue release-profile CustomElement producer and consumer adapter**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_session/src`, `crates/verter_language/src`.
- Named API/data boundaries: `Vue Custom Element producer and consumer retrofit`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **HWC3:** exact current receipt ID and digest for “Web Component standards model, registry analysis, and CEM”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **EAK1:** exact current receipt ID and digest for “Vue `defineComponent` embedded-template canary”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **SKL3:** exact current receipt ID and digest for “Maintainer-ratified atomic workflow activation”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** make the accepted Vue release an explicit CE producer and consumer rather than a generic component approximation.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **the exact superseded VCE0 owner routes listed in the deletion manifest**, **the named VCE0 compatibility/fallback call sites in the zero-consumer search receipt** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **VCE0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **VCE0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **VCE0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **VCE0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete or structurally reject: **the exact superseded VCE0 owner routes listed in the deletion manifest**.
- Delete or structurally reject: **the named VCE0 compatibility/fallback call sites in the zero-consumer search receipt**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 300 production LOC, 3 production files, 1 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_session`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 3 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, `architecture-specialist`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L1186`

## Reconciled source-plan contract

**Intent:** make the accepted Vue release an explicit CE producer and consumer rather than a generic component approximation.
**Predecessors:** `HWC3`, `EAK1`, `SKL3`.
**Subblocks:** (1) prove `defineCustomElement`/`defineSSRCustomElement` roles; (2) treat `.ce.vue` and captured plugin config as mode candidates, not tag identity; (3) model CE-specific prop/attribute/event/slot/style/root behavior; (4) associate explicit registrations; (5) contribute Vue-owned evidence to HWC3, which solely projects standards facts and CEM output conforming to `CEF0`, then test TypeInfo/ComponentInfo/CEM results; (6) add template/TS IDE, diagnostic/action, source-map, and performance fixtures.
**Acceptance:** ordinary Vue component and CE build variants remain distinct; alias/re-export activation works; filename-only and userland same-spelling cases fail closed; Vue consumer `isCustomElement` policy is captured and invalidated correctly.
**Forbidden:** deriving registration from compile output, claiming runtime registration, treating `.ce.vue` as a tag declaration, vertical-owned CEM serialization, or implementing a private formatter. CE mode does not change formatter semantics; `.ce.vue` is covered by ordinary Vue syntax fixtures in `FMTV0`.
**Deletion/abort:** delete only named Vue profile rows/adapters after zero-consumer proof; shared schema/registry deletion belongs to `CEC0`; abort if the exact Vue release oracle differs from locked mode semantics.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1186-D14AAE5B09BA

- Kind: `context`
- Source: `successor-expansion.md:1186-1186`
- Applicability: `VCE0`
- Exact text SHA-256: `d14aae5b09ba9baea3410f40fb79fc02598adf83c342f08f59e4f98517a0795f`

~~~~markdown
### `VCE0.md` — Vue Custom Element producer and consumer retrofit
~~~~

### SRC-EXP-L1188-A842A59B60C6

- Kind: `forbidden`
- Source: `successor-expansion.md:1188-1193`
- Applicability: `VCE0`
- Exact text SHA-256: `a842a59b60c6d8bd37760797cf3fa29b2f29f221ee8c91bcdb3e390bf7296ecd`

~~~~markdown
**Intent:** make the accepted Vue release an explicit CE producer and consumer rather than a generic component approximation.
**Predecessors:** `HWC3`, `EAK1`, `SKL3`.
**Subblocks:** (1) prove `defineCustomElement`/`defineSSRCustomElement` roles; (2) treat `.ce.vue` and captured plugin config as mode candidates, not tag identity; (3) model CE-specific prop/attribute/event/slot/style/root behavior; (4) associate explicit registrations; (5) contribute Vue-owned evidence to HWC3, which solely projects standards facts and CEM output conforming to `CEF0`, then test TypeInfo/ComponentInfo/CEM results; (6) add template/TS IDE, diagnostic/action, source-map, and performance fixtures.
**Acceptance:** ordinary Vue component and CE build variants remain distinct; alias/re-export activation works; filename-only and userland same-spelling cases fail closed; Vue consumer `isCustomElement` policy is captured and invalidated correctly.
**Forbidden:** deriving registration from compile output, claiming runtime registration, treating `.ce.vue` as a tag declaration, vertical-owned CEM serialization, or implementing a private formatter. CE mode does not change formatter semantics; `.ce.vue` is covered by ordinary Vue syntax fixtures in `FMTV0`.
**Deletion/abort:** delete only named Vue profile rows/adapters after zero-consumer proof; shared schema/registry deletion belongs to `CEC0`; abort if the exact Vue release oracle differs from locked mode semantics.
~~~~
