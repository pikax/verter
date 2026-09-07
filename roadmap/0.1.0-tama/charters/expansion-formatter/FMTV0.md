<!-- unified-charter-v2
id=FMTV0
name=Vue whole-document formatter contribution
phase=expansion
train=expansion.formatter
product=formatter
kind=implementation
semantic_role=delivery
class=successor
predecessors=FMT2TX,FMTCS0,FMTCL0,CPF1
owner=expansion.formatter:Vue whole-document printer and composition contribution
conflict_domains=doc,vue_product
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
size=M
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/expansion-formatter/FMTV0.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMTV0 — Vue whole-document formatter contribution

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Compose the Vue outer/template/script/style/custom-block printer contributions into a complete private carrier formatter without changing the shared production route. The sole owner is **Vue whole-document printer and composition contribution**; shared renderer/edit/map/range/cursor authorities remain upstream, FMT3C owns private service composition, and FMT3 owns the later route cutover/deletion.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_formatter/src/printers/vue/**` and `crates/verter_language/src/formatter/vue/**`.
- Owned boundaries: Vue outer/template format-view and printer contributions plus private block composition.
- Consumed boundaries: shared `Doc`, edits, maps, range/cursor geometry, script/style printers, and configuration.
- Mutation boundary: private Vue printer/view/composition contribution only; no production routing or old-route deletion occurs.

## Exact predecessor contracts

- **FMT2TX:** supplies the complete accepted JavaScript/TypeScript/JSX/TSX private contribution by joining the independently accepted FMT2T and FMT2X branches.
- **FMTCS0:** supplies the accepted SCSS extension and transitive CSS base.
- **FMTCL0:** supplies the accepted Less extension and transitive CSS base.
- **CPF1:** implemented ledger row for “Carrier frontend registration and Vue/Svelte cutover”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- **Revision:** 4 — supersedes the 251-charter all-verticals proposal
- **Prepared:** 2026-08-26
- **Repository basis:** program/architecture-lock at d1f3d50a948597f036868543b9bb21acacd730ff
- **Current-program condition:** readiness follows the current DAG and implemented ledger; historical source-plan freeze/lock labels are not active dispatch instructions.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **FMTV0-AC1 — sole-owner outcome:** one private Vue printer/composition contribution exists and no production route or legacy formatter consumer changes in this node. Prefer existing type, capability, dependency, compiler, or static enforcement.
- **FMTV0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **FMTV0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **FMTV0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_language/tests`, `packages/language-shared`.

## Deletions and forbidden designs

- The private contribution structurally rejects **format-after-build string surgery** and a **second semantic parser for formatting**; it deletes no old production route.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages; split before exceeding any M-node quality ceiling.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: meet FMT0's Vue work/allocation/stack/latency/cancellation/zero-work thresholds with zero parser invocations.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_language -p verter_session`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Reconciled source-plan contract

**Intent:** compose the script and style printers with Vue-owned SFC/template syntax and custom blocks.
**Predecessors:** `FMT2TX`, `FMTCS0`, `FMTCL0`, `CPF1`.
**Subblocks:** (1) Vue outer/block layout; (2) Vue template/directive/interpolation printing via the Vue owner; (3) script/style/custom-block composition; (4) syntactic `.ce.vue` and generic/custom block fixtures without CE semantic dependency; (5) conformance through shared range/cursor/edit/map geometry plus idempotence; (6) publish the private Vue contribution to FMT3C without routing or deletion.
**Acceptance:** every admitted Vue block can be formatted by one private native contribution; unsupported custom blocks are truthful; full/range output and maps pass locked corpora; production consumers remain unchanged for FMT3C/FMT3.
**Forbidden:** external block formatters, neutral HTML containing Vue branches, deleting Svelte/HTML routes, or temporary dual authority.
**Deletion/abort:** delete no old Vue formatter or route; FMT3 owns that cutover. Abort on authored-map ambiguity or unexplained Prettier-cell divergence.

## Collapsed non-authoritative subblock disposition

The internal checklist remains one Vue-owned private carrier contribution. It has no route, public adapter, deletion, or promotion authority; FMT3C consumes it and FMT3 alone owns the later route cutover.
