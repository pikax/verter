<!-- unified-charter-v2
id=FMTH0
name=Native neutral-HTML formatter
phase=expansion
train=expansion.formatter
product=formatter
kind=implementation
semantic_role=delivery
class=successor
predecessors=FMT1D,FMT1E,FCFG0,HWC2,PUB0,PER0
owner=expansion.formatter:neutral HTML printer and HTML format-view contribution
conflict_domains=carrier_parser,doc
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
charter=charters/expansion-formatter/FMTH0.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMTH0 — Native neutral-HTML formatter

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Implement the neutral-HTML printer and HTML-specific `AuthoredFormatView` contribution while consuming, never redefining, the shared formatter substrate. The sole owner is **neutral HTML printer and HTML format-view contribution**.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_formatter/src/printers/html/**` and `crates/verter_language/src/formatter/html/**`; dispatch binds exact paths/symbols.
- Owned boundaries: one typed `NeutralHtmlFormatterContribution` carrying the HTML authored-view contribution, element/attribute/text/comment/raw-text printer rules, malformed HTML recovery policy, and HTML `Doc` production expected by FMT3C.
- Consumed boundaries: `Doc`, `FormatEditSet`, `FormatPositionMap`, `FormatSelection`, `CursorMap`, and the generic surface-neutral `PUB0` envelope. Formatter-specific protocol DTOs remain FMT4P; `FormatterService`, carrier selection, embedded-demand planning, result aggregation, and cache admission remain exclusively FMT3C.
- Mutation boundary: the typed private HTML view/printer contribution only. No service implementation, service registration, session/LSP route, capability, public DTO, conversion, or carrier consumer changes.

## Exact predecessor contracts

- **FMT1D:** implemented ledger row for “Authored range expansion and edit containment”; ledger presence alone satisfies the predecessor.
- **FMT1E:** implemented ledger row for “Cursor projection and bias geometry”; ledger presence alone satisfies the predecessor.
- **FCFG0:** implemented ledger row for “Prettier-compatible formatter configuration translator”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **HWC2:** implemented ledger row for “HTML facts, TypeInfo, authored maps, and index contributions”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **PUB0:** implemented ledger row for “Versioned public request/result and capability truth”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **PER0:** implemented ledger row for “Cache/backend identity, cancellation, budgets, and zero work”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- **Revision:** 4 — supersedes the 251-charter all-verticals proposal
- **Prepared:** 2026-08-26
- **Repository basis:** program/architecture-lock at d1f3d50a948597f036868543b9bb21acacd730ff
- **Current-program condition:** readiness follows the current DAG and implemented ledger; historical source-plan freeze/lock labels are not active dispatch instructions.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **FMTH0-AC1 — private-owner outcome:** one typed neutral-HTML view/printer contribution exists for later FMT3C consumption, no formatter-service implementation or registration exists, no Vue/Svelte branch exists, and no live formatter route/capability/consumer changes.
- **FMTH0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **FMTH0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **FMTH0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_language/tests`, `packages/language-shared`.

## Deletions and forbidden designs

- Delete only a source-confirmed private neutral-HTML printer prototype under the two owned surfaces after all private consumers migrate; FMT0 currently names no such prototype, so the default deletion population is empty.
- Delete no SFC/shared/LSP route or adapter. Structurally forbid format-after-build string surgery and a second semantic parser inside this contribution.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages; split before exceeding any M-node quality ceiling.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: meet FMT0's HTML fixture work/allocation/stack/latency/cancellation/zero-work thresholds with zero parser invocations.

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

**Intent:** implement the neutral HTML full/range printer on the already-locked formatter substrate before any SFC composition.
**Predecessors:** `FMT1D`, `FMT1E`, `FCFG0`, `HWC2`, `PUB0`, `PER0`.
**Subblocks:** (1) HTML format view and authored trivia; (2) element/attribute/text/comment/raw-text printers; (3) malformed/recovery islands; (4) conformance through the shared range/cursor/edit/`FormatPositionMap` substrate without reimplementation; (5) Prettier differential and idempotence corpus; (6) typed `NeutralHtmlFormatterContribution` construction plus printer-local performance/cancellation tests for later FMT3C consumption.
**Acceptance:** locked exact cells are byte-equivalent, divergences are predeclared, repeated formatting stabilizes, malformed retained bytes and every edit map exactly, and no Vue/Svelte branch exists.
**Forbidden:** delegating to Prettier/oxfmt, Vue parser semantics, whole-file replacement when smaller edits are proven, or deleting an SFC formatter path.
**Deletion/abort:** delete only an exact private neutral-HTML prototype named before dispatch; delete no route. Abort a compatibility cell rather than fabricate parity.
