<!-- unified-charter-v2
id=VST0
name=Vue framework style semantics and scope plan
phase=compiler
train=compiler.vue-style
product=vue_style
kind=implementation
semantic_role=delivery
class=compiler
predecessors=VCP1,J4
owner=compiler.vue-style:Vue-owned style semantics over canonical CSS facts
conflict_domains=style_semantics,semantic_authority,performance_evidence
resource_class=rust-mixed
review_profile=semantic-3
gate_profile=targeted-domain
implementation_effort_min=medium
implementation_effort_default=medium
review_effort_min=medium
review_effort_default=medium
verification_effort_min=medium
verification_effort_default=medium
confirmation_effort_min=medium
confirmation_effort_default=medium
size=M
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-vue-style/VST0.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# VST0 — Vue framework style semantics and scope plan

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Vue framework style semantics and scope plan. The current owner is **Vue style scope planning**. The final and sole owner is **Vue-owned style semantics over canonical CSS facts**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src/framework/vue`, `crates/verter_css_syntax/src`.
- Named API/data boundaries: `VueStylePlan`, `ScopeId`, `SelectorQuery`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **VCP1:** implemented ledger row for “Canonical Vue semantic authority convergence”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **J4:** implemented ledger row for “Dialect preprocessor formatter recovery contract”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- **Intent:** consume J-owned style products and produce canonical Vue-specific style facts once.
- **Problem:** Vue style meaning can be extracted inside compiler/session code, external processing stages can be ambiguous, and template/style scope identity can diverge.
- **Solution and architecture decisions:**
- consume StyleSyntaxIr and J neutral facts only;

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **VST0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **VST0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **VST0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **VST0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_compiler/tests`, `crates/verter_css_syntax/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **Vue-local CSS parser**.
- Delete or structurally reject: **string selector matching**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_css_syntax`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial`, `conformance`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 2/2 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `targeted` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Reconciled source-plan contract

**Intent:** consume J-owned style products and produce canonical Vue-specific style facts once.

**Problem:** Vue style meaning can be extracted inside compiler/session code, external processing stages can be ambiguous, and template/style scope identity can diverge.

**Solution and architecture decisions:**

- consume `StyleSyntaxIr` and J neutral facts only;
- own Vue meaning for `v-bind()`, `:deep`, `:global`, `:slotted`, scoped selectors/keyframes, CSS Modules semantic exposure, and framework diagnostics;
- convert style expressions to source-backed `ExprId`/binding/dependency facts through the canonical Vue semantic authority;
- create one `VueComponentScopePlan` consumed by template, style, SSR and metadata paths;
- consume exact stage-qualified external preprocessor/PostCSS results and compose maps;
- perform no native Sass/Less/Stylus execution;
- do not implement selector-to-template matching in this block.

**Suggested predecessor:** `VCP1`.

**Normative source decomposition:** J integration, Vue selector/directive facts, CSS-variable expressions, scope/keyframe plan, CSS Modules semantic facts, external-stage/map integration.

**Acceptance:** no compiler/session raw CSS scan remains for migrated facts; template/style scope identity cannot disagree; preprocess-dependent work is exact `NeedInputs`; maps compose across all admitted stages; no second CSS grammar exists.

**Forbidden:** CSS reparsing, compiler-owned style semantics, opaque “processed CSS” strings, native preprocessors, or selector pruning.

**Deletion/abort:** delete replaced Vue style scanners/extractors after parity; stop if stage ordering cannot be proven.

---

