<!-- unified-charter-v2
id=HWCI0
name=HTML/WC IDE and LSP capabilities
phase=expansion
train=expansion.html-wc
product=html_wc
kind=implementation
semantic_role=delivery
class=successor
predecessors=HWC2,HWC3,COX0,PUB0
conditional_predecessors=
owner=expansion.html-wc:neutral HTML/Web Components vertical on TypeInfo and workspace index
conflict_domains=lsp_publication,carrier_parser
resource_class=rust-mixed
review_profile=architecture-3
gate_profile=targeted-domain
size=S
dispatchable=true
optional=false
release_gating=none
source_refs=source:successor-expansion.md:L1141
external_requirements=
activation_gate=ORC0
charter=charters/expansion-html-wc/HWCI0.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# HWCI0 — HTML/WC IDE and LSP capabilities

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

HTML/WC IDE and LSP capabilities. The current owner is **HTML tooling and custom-element consumers**. The final and sole owner is **neutral HTML/Web Components vertical on TypeInfo and workspace index**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `crates/verter_lsp/src`, `crates/verter_protocol/src`.
- Named API/data boundaries: `HtmlFacts`, `CustomElementDeclaration`, `RegistryScope`, `CemModule`, `IndexContribution`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **HWC2:** exact current receipt ID and digest for “HTML facts, TypeInfo, authored maps, and index contributions”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **HWC3:** exact current receipt ID and digest for “Web Component standards model, registry analysis, and CEM”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **COX0:** exact current receipt ID and digest for “Per-profile editor participation and coexistence”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **PUB0:** exact current receipt ID and digest for “Versioned public request/result and capability truth”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Revision:** 4 — supersedes the 251-charter all-verticals proposal
- **Prepared:** 2026-08-26
- **Repository basis:** program/architecture-lock at d1f3d50a948597f036868543b9bb21acacd730ff
- **Current-program condition:** maintainer work freeze; TCM0 = RESCOPE_REQUIRED; TCM1–TCM4 = LOCKED

## Acceptance IDs and discriminating proof

- **HWCI0-AC1 — sole-owner proof:** add `hwci0_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **HWCI0-AC2 — positive contract:** add `hwci0_publishes_exact_htmlfacts`; assert exact identities, provenance, completeness, and deterministic ordering.
- **HWCI0-AC3 — incremental equivalence:** add `hwci0_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **HWCI0-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
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
- Performance budget: equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance is 0.0% unless an owning-authority amendment supplies exact replacement thresholds; after warmup, retained bytes may not increase across 100 identical requests.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_language -p verter_lsp -p verter_session`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `architecture-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three named independent reviewers with assigned lenses, model `gpt-5.6-sol`, effort `ultra`, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L1141`

## Reconciled source-plan contract

**Intent:** deliver the applicable interactive language operations without lint, formatter, or public-binding ownership.
**Predecessors:** `HWC2`, `HWC3`, `COX0`, `PUB0`.
**Subblocks:** (1) completion/hover/signature/document symbols; (2) definition/references/rename; (3) document links/colors/folding/selection; (4) semantic tokens/inlay/code-lens cells where applicable; (5) component auto-import and consumer navigation from bounded index evidence; (6) positive/negative/stale/cancellation/map/coexistence suites.
**Acceptance:** every applicable LSP row has exact fixtures and truthful registration; no-op handlers are absent; capability masks withdraw only overlaps; unassociated `.html` receives no TypeScript projection.
**Forbidden:** fabricated route/runtime results, unbounded workspace search, formatter edits, lint diagnostics, or hidden delegation to another extension.
**Deletion/abort:** delete displaced HTML IDE handlers after consumer parity; rescope any operation without exact authored mapping.

## Collapsed non-authoritative subblock disposition

The recovery candidate mechanically split this source-owned atomic node into the following labels: `HWCI0-A`, `HWCI0-B`, `HWCI0-C`, `HWCI0-D`, `HWCI0-E`, `HWCI0-F`. They have no separate dispatch, lease, receipt, migration manifest, deletion ownership, or review standing. Their useful source-described concerns are internal RED/GREEN checklist items of **HWCI0**; HWCI0 alone owns the complete migration population, exactly one final deletion/cutover, and atomic acceptance. Any quoted “suggested subblock” wording in transferred source text is non-authoritative planning context.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1141-82790005655C

- Kind: `context`
- Source: `successor-expansion.md:1141-1141`
- Applicability: `HWCI0`
- Exact text SHA-256: `82790005655c2db4828ff5cbfa4814c15c651ed08c9b9c5c036b0488b26d5058`

~~~~markdown
### `HWCI0.md` — HTML/WC IDE and LSP capabilities
~~~~

### SRC-EXP-L1143-A9C6F83FF38B

- Kind: `forbidden`
- Source: `successor-expansion.md:1143-1148`
- Applicability: `HWCI0`
- Exact text SHA-256: `a9c6f83ff38b1adcd92e861cee29dd19a45826d14bb31b405715cf4d2802a5a7`

~~~~markdown
**Intent:** deliver the applicable interactive language operations without lint, formatter, or public-binding ownership.
**Predecessors:** `HWC2`, `HWC3`, `COX0`, `PUB0`.
**Subblocks:** (1) completion/hover/signature/document symbols; (2) definition/references/rename; (3) document links/colors/folding/selection; (4) semantic tokens/inlay/code-lens cells where applicable; (5) component auto-import and consumer navigation from bounded index evidence; (6) positive/negative/stale/cancellation/map/coexistence suites.
**Acceptance:** every applicable LSP row has exact fixtures and truthful registration; no-op handlers are absent; capability masks withdraw only overlaps; unassociated `.html` receives no TypeScript projection.
**Forbidden:** fabricated route/runtime results, unbounded workspace search, formatter edits, lint diagnostics, or hidden delegation to another extension.
**Deletion/abort:** delete displaced HTML IDE handlers after consumer parity; rescope any operation without exact authored mapping.
~~~~
