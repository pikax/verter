<!-- unified-charter-v2
id=HWC3
name=Web Component standards model, registry analysis, and CEM
phase=expansion
train=expansion.html-wc
product=html_wc
kind=implementation
semantic_role=delivery
class=successor
predecessors=HWC2,CEF0
conditional_predecessors=
owner=expansion.html-wc:neutral HTML/Web Components vertical on TypeInfo and workspace index
conflict_domains=capability_catalog
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
release_gating=none
source_refs=source:successor-expansion.md:L1123
external_requirements=
activation_gate=ORC0
charter=charters/expansion-html-wc/HWC3.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# HWC3 — Web Component standards model, registry analysis, and CEM

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Web Component standards model, registry analysis, and CEM. The current owner is **HTML tooling and custom-element consumers**. The final and sole owner is **neutral HTML/Web Components vertical on TypeInfo and workspace index**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `crates/verter_lsp/src`, `crates/verter_protocol/src`.
- Named API/data boundaries: `HtmlFacts`, `CustomElementDeclaration`, `RegistryScope`, `CemModule`, `IndexContribution`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **HWC2:** exact current receipt ID and digest for “HTML facts, TypeInfo, authored maps, and index contributions”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **CEF0:** exact current receipt ID and digest for “Custom Element producer/consumer interoperability contract”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Revision:** 4 — supersedes the 251-charter all-verticals proposal
- **Prepared:** 2026-08-26
- **Repository basis:** program/architecture-lock at d1f3d50a948597f036868543b9bb21acacd730ff
- **Current-program condition:** maintainer work freeze; TCM0 = RESCOPE_REQUIRED; TCM1–TCM4 = LOCKED

## Acceptance IDs and discriminating proof

- **HWC3-AC1 — sole-owner proof:** add `hwc3_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **HWC3-AC2 — positive contract:** add `hwc3_publishes_exact_htmlfacts`; assert exact identities, provenance, completeness, and deterministic ordering.
- **HWC3-AC3 — incremental equivalence:** add `hwc3_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **HWC3-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
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

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three fresh distinct harness reviewers with assigned lenses, deterministic low|medium|high effort, exact task/provider/model bindings, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L1123`

## Reconciled source-plan contract

**Intent:** solely implement standards-fact projection, registry analysis, and CEM import/export over TypeInfo and the workspace index, conforming to the `CEF0` contract.
**Predecessors:** `HWC2`, `CEF0`.
**Subblocks:** (1) consume the `CEF0` standards/CEM contract; (2) project custom-element declarations, registrations, registry scopes, properties/attributes/events/slots/methods/parts/CSS custom properties from neutral or vertical-owned evidence; (3) implement `customElements.define` and statically admitted registry analysis; (4) implement declaration↔registration↔consumer association; (5) implement CEM import/export with provenance; (6) ambiguity/scoped-registry/package fixtures.
**Acceptance:** Vue/Svelte/Lit/Stencil-owned evidence can be projected into HWC3-produced standards facts without HWC3 knowing framework semantics; consumers obtain exact/partial/ambiguous results honestly; CEM round-trip preserves admitted facts and provenance under `CEF0`.
**Forbidden:** runtime execution, global registry certainty, class-inheritance heuristics as authority, or CEM-owned types.
**Deletion/abort:** migrate only neutral standards rows/adapters; shared legacy WCP schema/registry deletion belongs solely to `CEC0`; abort static reachability claims that cannot survive scoped/dynamic registry counterexamples.

## Collapsed non-authoritative subblock disposition

The recovery candidate mechanically split this source-owned atomic node into the following labels: `HWC3-A`, `HWC3-B`, `HWC3-C`, `HWC3-D`, `HWC3-E`, `HWC3-F`. They have no separate dispatch, lease, receipt, migration manifest, deletion ownership, or review standing. Their useful source-described concerns are internal RED/GREEN checklist items of **HWC3**; HWC3 alone owns the complete migration population, exactly one final deletion/cutover, and atomic acceptance. Any quoted “suggested subblock” wording in transferred source text is non-authoritative planning context.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1123-C31FCA4D3E76

- Kind: `context`
- Source: `successor-expansion.md:1123-1123`
- Applicability: `HWC3`
- Exact text SHA-256: `c31fca4d3e7678f4252f61faf8bc0ba3fa9f7728fe4b68d1da8798de92156c23`

~~~~markdown
### `HWC3.md` — Web Component standards model, registry analysis, and CEM
~~~~

### SRC-EXP-L1125-363850D1D58A

- Kind: `forbidden`
- Source: `successor-expansion.md:1125-1130`
- Applicability: `HWC3`
- Exact text SHA-256: `363850d1d58af5711089a55779375b5691c212c3285c0bae8e7b1599f5d5275f`

~~~~markdown
**Intent:** solely implement standards-fact projection, registry analysis, and CEM import/export over TypeInfo and the workspace index, conforming to the `CEF0` contract.
**Predecessors:** `HWC2`, `CEF0`.
**Subblocks:** (1) consume the `CEF0` standards/CEM contract; (2) project custom-element declarations, registrations, registry scopes, properties/attributes/events/slots/methods/parts/CSS custom properties from neutral or vertical-owned evidence; (3) implement `customElements.define` and statically admitted registry analysis; (4) implement declaration↔registration↔consumer association; (5) implement CEM import/export with provenance; (6) ambiguity/scoped-registry/package fixtures.
**Acceptance:** Vue/Svelte/Lit/Stencil-owned evidence can be projected into HWC3-produced standards facts without HWC3 knowing framework semantics; consumers obtain exact/partial/ambiguous results honestly; CEM round-trip preserves admitted facts and provenance under `CEF0`.
**Forbidden:** runtime execution, global registry certainty, class-inheritance heuristics as authority, or CEM-owned types.
**Deletion/abort:** migrate only neutral standards rows/adapters; shared legacy WCP schema/registry deletion belongs solely to `CEC0`; abort static reachability claims that cannot survive scoped/dynamic registry counterexamples.
~~~~
