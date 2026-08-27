<!-- unified-charter-v2
id=HWC2
name=HTML facts, TypeInfo, authored maps, and index contributions
phase=expansion
train=expansion.html-wc
product=html_wc
kind=implementation
semantic_role=delivery
class=successor
predecessors=HWC1,TIF1,IDX0
conditional_predecessors=
owner=expansion.html-wc:neutral HTML/Web Components vertical on TypeInfo and workspace index
conflict_domains=mapping_geometry,carrier_parser,semantic_authority
resource_class=rust-mixed
review_profile=architecture-3
gate_profile=targeted-domain
size=S
dispatchable=true
optional=false
release_gating=none
source_refs=source:successor-expansion.md:L1114
external_requirements=
activation_gate=ORC0
charter=charters/expansion-html-wc/HWC2.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# HWC2 — HTML facts, TypeInfo, authored maps, and index contributions

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

HTML facts, TypeInfo, authored maps, and index contributions. The current owner is **HTML tooling and custom-element consumers**. The final and sole owner is **neutral HTML/Web Components vertical on TypeInfo and workspace index**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `crates/verter_lsp/src`, `crates/verter_protocol/src`.
- Named API/data boundaries: `HtmlFacts`, `CustomElementDeclaration`, `RegistryScope`, `CemModule`, `IndexContribution`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **HWC1:** exact current receipt ID and digest for “Neutral HTML parser adoption and HWC carrier cutover”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **TIF1:** exact current receipt ID and digest for “TypeInfo-first ComponentInfo and component-meta cutover”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **IDX0:** exact current receipt ID and digest for “Atomic semantic contributions and workspace index”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** turn the syntax product into neutral authored semantics usable by multiple overlays.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **legacy shared custom-element registry**, **framework-local HTML fact authority** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

- **HWC2-AC1 — sole-owner proof:** add `hwc2_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **HWC2-AC2 — positive contract:** add `hwc2_publishes_exact_htmlfacts`; assert exact identities, provenance, completeness, and deterministic ordering.
- **HWC2-AC3 — incremental equivalence:** add `hwc2_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **HWC2-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
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

- `source:successor-expansion.md:L1114`

## Reconciled source-plan contract

**Intent:** turn the syntax product into neutral authored semantics usable by multiple overlays.
**Predecessors:** `HWC1`, `TIF1`, `IDX0`.
**Subblocks:** (1) element/attribute/text/comment/namespace/ID/class/selector facts; (2) document symbol and authored-region identities; (3) exact authored syntax/source maps; (4) neutral TypeInfo roles for elements/attributes without pretending DOM runtime values are known; (5) atomic index contributions for IDs/classes/assets/links/components; (6) incremental invalidation and bounded query tests.
**Acceptance:** Alpine/HTMX/Angular test overlays consume the same neutral facts without parser branches; definitions/renames of static IDs and class/selector relationships are exact where admitted; ambiguous dynamic values remain incomplete.
**Forbidden:** Angular/Alpine/HTMX rules in neutral facts, TypeScript projection of generic `.html` without project-context proof, runtime DOM inference, or lossy map recovery.
**Deletion/abort:** remove any copied Vue semantic fact types; rescope facts that require a framework owner.

## Collapsed non-authoritative subblock disposition

The recovery candidate mechanically split this source-owned atomic node into the following labels: `HWC2A`, `HWC2B`, `HWC2C`, `HWC2D`, `HWC2Z`. They have no separate dispatch, lease, receipt, migration manifest, deletion ownership, or review standing. Their useful source-described concerns are internal RED/GREEN checklist items of **HWC2**; HWC2 alone owns the complete migration population, exactly one final deletion/cutover, and atomic acceptance. Any quoted “suggested subblock” wording in transferred source text is non-authoritative planning context.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1114-E73342732814

- Kind: `context`
- Source: `successor-expansion.md:1114-1114`
- Applicability: `HWC2`
- Exact text SHA-256: `e733427328144aba4f3c8bc03cad92edbc6be9dce26a37c394c85c35fe762ef7`

~~~~markdown
### `HWC2.md` — HTML facts, TypeInfo, authored maps, and index contributions
~~~~

### SRC-EXP-L1116-039486240A9A

- Kind: `forbidden`
- Source: `successor-expansion.md:1116-1121`
- Applicability: `HWC2`
- Exact text SHA-256: `039486240a9ad04b8af75749f1c73e84d4fbbf68fbf044c39499ddb813fb4bdd`

~~~~markdown
**Intent:** turn the syntax product into neutral authored semantics usable by multiple overlays.
**Predecessors:** `HWC1`, `TIF1`, `IDX0`.
**Subblocks:** (1) element/attribute/text/comment/namespace/ID/class/selector facts; (2) document symbol and authored-region identities; (3) exact authored syntax/source maps; (4) neutral TypeInfo roles for elements/attributes without pretending DOM runtime values are known; (5) atomic index contributions for IDs/classes/assets/links/components; (6) incremental invalidation and bounded query tests.
**Acceptance:** Alpine/HTMX/Angular test overlays consume the same neutral facts without parser branches; definitions/renames of static IDs and class/selector relationships are exact where admitted; ambiguous dynamic values remain incomplete.
**Forbidden:** Angular/Alpine/HTMX rules in neutral facts, TypeScript projection of generic `.html` without project-context proof, runtime DOM inference, or lossy map recovery.
**Deletion/abort:** remove any copied Vue semantic fact types; rescope facts that require a framework owner.
~~~~
