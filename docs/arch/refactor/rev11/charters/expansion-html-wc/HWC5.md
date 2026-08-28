<!-- unified-charter-v2
id=HWC5
name=Neutral HTML/WC conformance, performance, and Experimental terminal
phase=expansion
train=expansion.html-wc
product=html_wc
kind=terminal
semantic_role=delivery
class=successor
predecessors=HWC4,PER0,VIM1
conditional_predecessors=
owner=expansion.html-wc:neutral HTML/Web Components vertical on TypeInfo and workspace index
conflict_domains=carrier_parser,performance_evidence
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
source_refs=source:successor-expansion.md:L1177
external_requirements=
activation_gate=ORC0
charter=charters/expansion-html-wc/HWC5.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# HWC5 — Neutral HTML/WC conformance, performance, and Experimental terminal

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Neutral HTML/WC conformance, performance, and Experimental terminal. The current owner is **HTML tooling and custom-element consumers**. The final and sole owner is **neutral HTML/Web Components vertical on TypeInfo and workspace index**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `crates/verter_lsp/src`, `crates/verter_protocol/src`.
- Named API/data boundaries: `HtmlFacts`, `CustomElementDeclaration`, `RegistryScope`, `CemModule`, `IndexContribution`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **HWC4:** exact current receipt ID and digest for “HTML/WC read-only product convergence”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **PER0:** exact current receipt ID and digest for “Cache/backend identity, cancellation, budgets, and zero work”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **VIM1:** exact current receipt ID and digest for “Deterministic manifest compiler and conformance generator”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** independently decide whether neutral HTML and standards Web Components are a truthful first-class Experimental vertical.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **legacy shared custom-element registry**, **framework-local HTML fact authority** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **HWC5-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **HWC5-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **HWC5-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **HWC5-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_language/tests`, `crates/verter_lsp/tests`, `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete or structurally reject: **legacy shared custom-element registry**.
- Delete or structurally reject: **framework-local HTML fact authority**.
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

1. `cargo nextest run -p verter_language -p verter_lsp -p verter_session`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 3 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, `architecture-specialist`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L1177`

## Reconciled source-plan contract

**Intent:** independently decide whether neutral HTML and standards Web Components are a truthful first-class Experimental vertical.
**Predecessors:** `HWC4`, `PER0`, `VIM1`.
**Subblocks:** (1) validate manifest/capability/test matrices; (2) standards/oracle differential; (3) fresh/incremental/cancellation/Unicode/map suite; (4) cold/warm/equivalent-work/RSS/zero-work benchmarks; (5) publish per-operation/per-surface maturity and `NotApplicable` compiler disposition; (6) exact-candidate three-lane review and ratification.
**Acceptance:** every neutral locked cell passes or retains its originally locked unsupported disposition; HTML/WC can promote without Vue/Svelte CE retrofit completion; no global-release dependency is created.
**Forbidden:** broad “HTML compatible” claims, scoped/dynamic registry completeness, hidden CLI parity, or fixing implementation in the terminal.
**Deletion/abort:** unsuccessful cells return to owners; parser-architecture failure reopens `PAR0/HWC1`, not an exception in the terminal.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1177-5E7BA83440CB

- Kind: `context`
- Source: `successor-expansion.md:1177-1177`
- Applicability: `HWC5`
- Exact text SHA-256: `5e7ba83440cb619bcd71647976b47c8b3d2d4967e1b97fa9bd945add28af10b2`

~~~~markdown
### `HWC5.md` — Neutral HTML/WC conformance, performance, and Experimental terminal
~~~~

### SRC-EXP-L1179-05B5E758AE58

- Kind: `forbidden`
- Source: `successor-expansion.md:1179-1184`
- Applicability: `HWC5`
- Exact text SHA-256: `05b5e758ae583fbd995779668ef4485bd84b22090e39cdc52cc8ac099a170af0`

~~~~markdown
**Intent:** independently decide whether neutral HTML and standards Web Components are a truthful first-class Experimental vertical.
**Predecessors:** `HWC4`, `PER0`, `VIM1`.
**Subblocks:** (1) validate manifest/capability/test matrices; (2) standards/oracle differential; (3) fresh/incremental/cancellation/Unicode/map suite; (4) cold/warm/equivalent-work/RSS/zero-work benchmarks; (5) publish per-operation/per-surface maturity and `NotApplicable` compiler disposition; (6) exact-candidate three-lane review and ratification.
**Acceptance:** every neutral locked cell passes or retains its originally locked unsupported disposition; HTML/WC can promote without Vue/Svelte CE retrofit completion; no global-release dependency is created.
**Forbidden:** broad “HTML compatible” claims, scoped/dynamic registry completeness, hidden CLI parity, or fixing implementation in the terminal.
**Deletion/abort:** unsuccessful cells return to owners; parser-architecture failure reopens `PAR0/HWC1`, not an exception in the terminal.
~~~~
