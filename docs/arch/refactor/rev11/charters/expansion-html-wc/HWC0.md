<!-- unified-charter-v2
id=HWC0
name=HTML + standards Custom Elements implementation lock
phase=expansion
train=expansion.html-wc
product=html_wc
kind=lock
semantic_role=delivery
class=successor
predecessors=UAI0,UAO0,UAP0,UAM0,SKL3
conditional_predecessors=
owner=expansion.html-wc:neutral HTML/Web Components vertical on TypeInfo and workspace index
conflict_domains=carrier_parser
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
source_refs=source:successor-expansion.md:L1096
external_requirements=
activation_gate=ORC0
charter=charters/expansion-html-wc/HWC0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# HWC0 — HTML + standards Custom Elements implementation lock

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

HTML + standards Custom Elements implementation lock. The current owner is **HTML tooling and custom-element consumers**. The final and sole owner is **neutral HTML/Web Components vertical on TypeInfo and workspace index**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `crates/verter_lsp/src`, `crates/verter_protocol/src`.
- Named API/data boundaries: `HtmlFacts`, `CustomElementDeclaration`, `RegistryScope`, `CemModule`, `IndexContribution`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **UAI0:** exact current receipt ID and digest for “Identity, carrier, parser, and coordinate contract lock”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **UAO0:** exact current receipt ID and digest for “Activation, observation, TypeInfo, index, and performance contract lock”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **UAP0:** exact current receipt ID and digest for “Capability, coexistence, rule/action, formatter, and public contract lock”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **UAM0:** exact current receipt ID and digest for “Manifest, validator, and governance contract lock”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **SKL3:** exact current receipt ID and digest for “Maintainer-ratified atomic workflow activation”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** freeze the first architecture project’s exact standards epochs, corpora, capabilities, exclusions, and numeric gates before implementation.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **legacy shared custom-element registry**, **framework-local HTML fact authority** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **HWC0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **HWC0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **HWC0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **HWC0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_language/tests`, `crates/verter_lsp/tests`, `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete or structurally reject: **legacy shared custom-element registry**.
- Delete or structurally reject: **framework-local HTML fact authority**.
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

1. `cargo nextest run -p verter_language -p verter_lsp -p verter_session`
2. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 3 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, `architecture-specialist`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L1096`

## Reconciled source-plan contract

**Intent:** freeze the first architecture project’s exact standards epochs, corpora, capabilities, exclusions, and numeric gates before implementation.
**Predecessors:** `UAI0`, `UAO0`, `UAP0`, `UAM0`, `SKL3`.
**Subblocks:** (1) pin HTML living-standard/WPT subset, DOM/tree/recovery oracle, accessibility/reference data, CEM schema, and browser-standards sources; (2) record a corpus-backed `ParserDecision` and exact Vue-fork lineage/license if `ForkAndSpecialize` wins; (3) lock TypeInfo/CE/index/LSP/lint/format/public cells; (4) lock separate neutral HTML, Vue CE, and Svelte CE outcomes; (5) lock performance/zero-work/RSS budgets and surface maturity; (6) obtain exact-digest reviews and ratification.
**Acceptance:** every cell has an owner, observable oracle, pass/fail rule, unsupported outcome, and fixture; no criterion is chosen after implementation.
**Forbidden:** calling this “copy and paste,” promising all browser runtime behavior, global-registry assumptions, or using Vue output as the neutral HTML oracle.
**Deletion/abort:** no code change; abort or rescope when the proposed Vue-parser fork cannot meet standards recovery without becoming a Vue-branch matrix.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1096-F6772306D225

- Kind: `context`
- Source: `successor-expansion.md:1096-1096`
- Applicability: `HWC0`
- Exact text SHA-256: `f6772306d225d954cc7f260803cc1dbeedb57fb9505309d0c869338f2d837482`

~~~~markdown
### `HWC0.md` — HTML + standards Custom Elements implementation lock
~~~~

### SRC-EXP-L1098-6D8D41B7422E

- Kind: `forbidden`
- Source: `successor-expansion.md:1098-1103`
- Applicability: `HWC0`
- Exact text SHA-256: `6d8d41b7422ebda883d1e92b95f1f4126d40692aceb9c37f56becb16cc8a9946`

~~~~markdown
**Intent:** freeze the first architecture project’s exact standards epochs, corpora, capabilities, exclusions, and numeric gates before implementation.
**Predecessors:** `UAI0`, `UAO0`, `UAP0`, `UAM0`, `SKL3`.
**Subblocks:** (1) pin HTML living-standard/WPT subset, DOM/tree/recovery oracle, accessibility/reference data, CEM schema, and browser-standards sources; (2) record a corpus-backed `ParserDecision` and exact Vue-fork lineage/license if `ForkAndSpecialize` wins; (3) lock TypeInfo/CE/index/LSP/lint/format/public cells; (4) lock separate neutral HTML, Vue CE, and Svelte CE outcomes; (5) lock performance/zero-work/RSS budgets and surface maturity; (6) obtain exact-digest reviews and ratification.
**Acceptance:** every cell has an owner, observable oracle, pass/fail rule, unsupported outcome, and fixture; no criterion is chosen after implementation.
**Forbidden:** calling this “copy and paste,” promising all browser runtime behavior, global-registry assumptions, or using Vue output as the neutral HTML oracle.
**Deletion/abort:** no code change; abort or rescope when the proposed Vue-parser fork cannot meet standards recovery without becoming a Vue-branch matrix.
~~~~
