<!-- unified-charter-v2
id=FMT2
name=Native JS/TS/JSX/TSX printers
phase=expansion
train=expansion.formatter
product=formatter
kind=implementation
semantic_role=delivery
class=successor
predecessors=FMT1,FCFG0
conditional_predecessors=
owner=expansion.formatter:native document algebra and carrier-composed formatter service
conflict_domains=doc
resource_class=rust-mixed
review_profile=architecture-3
gate_profile=targeted-domain
size=S
dispatchable=true
optional=false
release_gating=none
source_refs=source:successor-expansion.md:L1345
external_requirements=
activation_gate=ORC0
charter=charters/expansion-formatter/FMT2.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# FMT2 — Native JS/TS/JSX/TSX printers

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Native JS/TS/JSX/TSX printers. The current owner is **fragmented formatting adapters**. The final and sole owner is **native document algebra and carrier-composed formatter service**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `packages/language-shared/src`.
- Named API/data boundaries: `Doc`, `FormatRequest`, `FormatEdit`, `CursorMap`, `FormatterConfig`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **FMT1:** exact current receipt ID and digest for “Document algebra, renderer, edits, cursor, and maps”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **FCFG0:** exact current receipt ID and digest for “Prettier-compatible formatter configuration translator”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Revision:** 4 — supersedes the 251-charter all-verticals proposal
- **Prepared:** 2026-08-26
- **Repository basis:** program/architecture-lock at d1f3d50a948597f036868543b9bb21acacd730ff
- **Current-program condition:** maintainer work freeze; TCM0 = RESCOPE_REQUIRED; TCM1–TCM4 = LOCKED

## Acceptance IDs and discriminating proof

- **FMT2-AC1 — sole-owner proof:** add `fmt2_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **FMT2-AC2 — positive contract:** add `fmt2_publishes_exact_doc`; assert exact identities, provenance, completeness, and deterministic ordering.
- **FMT2-AC3 — incremental equivalence:** add `fmt2_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **FMT2-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_language/tests`, `packages/language-shared`.

## Deletions and forbidden designs

- Delete or structurally reject: **format-after-build string surgery**.
- Delete or structurally reject: **second semantic parser for formatting**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 300 production LOC, 3 production files, 1 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance is 0.0% unless an owning-authority amendment supplies exact replacement thresholds; after warmup, retained bytes may not increase across 100 identical requests.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_language -p verter_session`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `architecture-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three named independent reviewers with assigned lenses, model `gpt-5.6-sol`, effort `ultra`, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L1345`

## Reconciled source-plan contract

**Intent:** make Verter format embedded script contents itself using the shared frontend facts.
**Predecessors:** `FMT1`, `FCFG0`.
**Subblocks:** (1) JS printer; (2) TypeScript syntax; (3) JSX/TSX; (4) comment/trivia/recovery behavior; (5) range/cursor/edit/maps; (6) Prettier differential plus pinned oxfmt bug-evidence fixtures; (7) performance/allocation profiling.
**Acceptance:** locked `prettier-exact` cells are byte-equivalent and `verter-default` divergences are individually proven; repeated formatting is stable; OXC remains syntax owner but no external formatter runs in production.
**Forbidden:** two option vocabularies, subprocess formatting, unsupported syntax silently unchanged under a success result, or framework rules in base printers.
**Deletion/abort:** abort a compatibility cell rather than fabricate parity; unsupported cells remain truthful.

## Collapsed non-authoritative subblock disposition

The recovery candidate mechanically split this source-owned atomic node into the following labels: `FMT2-A`, `FMT2-B`, `FMT2-C`, `FMT2-D`, `FMT2-E`, `FMT2-F`, `FMT2-G`. They have no separate dispatch, lease, receipt, migration manifest, deletion ownership, or review standing. Their useful source-described concerns are internal RED/GREEN checklist items of **FMT2**; FMT2 alone owns the complete migration population, exactly one final deletion/cutover, and atomic acceptance. Any quoted “suggested subblock” wording in transferred source text is non-authoritative planning context.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1345-6BD9553F5437

- Kind: `context`
- Source: `successor-expansion.md:1345-1345`
- Applicability: `FMT2`
- Exact text SHA-256: `6bd9553f54372c327fdd484bc12d2fb106587d9191c4abc480ee0df33ecaab19`

~~~~markdown
### `FMT2.md` — Native JS/TS/JSX/TSX printers
~~~~

### SRC-EXP-L1347-A70C467A24E1

- Kind: `forbidden`
- Source: `successor-expansion.md:1347-1352`
- Applicability: `FMT2`
- Exact text SHA-256: `a70c467a24e1b3722d07005c7dec128f597bbac58bf4708beff44cd029260300`

~~~~markdown
**Intent:** make Verter format embedded script contents itself using the shared frontend facts.
**Predecessors:** `FMT1`, `FCFG0`.
**Subblocks:** (1) JS printer; (2) TypeScript syntax; (3) JSX/TSX; (4) comment/trivia/recovery behavior; (5) range/cursor/edit/maps; (6) Prettier differential plus pinned oxfmt bug-evidence fixtures; (7) performance/allocation profiling.
**Acceptance:** locked `prettier-exact` cells are byte-equivalent and `verter-default` divergences are individually proven; repeated formatting is stable; OXC remains syntax owner but no external formatter runs in production.
**Forbidden:** two option vocabularies, subprocess formatting, unsupported syntax silently unchanged under a success result, or framework rules in base printers.
**Deletion/abort:** abort a compatibility cell rather than fabricate parity; unsupported cells remain truthful.
~~~~
