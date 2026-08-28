<!-- unified-charter-v2
id=HWC4
name=HTML/WC read-only product convergence
phase=expansion
train=expansion.html-wc
product=html_wc
kind=convergence
semantic_role=convergence
class=successor
predecessors=FMTH0,HWCI0,HWCL0,HWCP0
conditional_predecessors=
owner=expansion.html-wc:neutral HTML/Web Components vertical on TypeInfo and workspace index
conflict_domains=carrier_parser
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
source_refs=source:successor-expansion.md:L1168
external_requirements=
activation_gate=ORC0
charter=charters/expansion-html-wc/HWC4.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# HWC4 — HTML/WC read-only product convergence

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

HTML/WC read-only product convergence. The current owner is **HTML tooling and custom-element consumers**. The final and sole owner is **neutral HTML/Web Components vertical on TypeInfo and workspace index**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `crates/verter_lsp/src`, `crates/verter_protocol/src`.
- Named API/data boundaries: `HtmlFacts`, `CustomElementDeclaration`, `RegistryScope`, `CemModule`, `IndexContribution`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **FMTH0:** exact current receipt ID and digest for “Native neutral-HTML formatter”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **HWCI0:** exact current receipt ID and digest for “HTML/WC IDE and LSP capabilities”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **HWCL0:** exact current receipt ID and digest for “HTML/WC diagnostics, lint, fixes, and code actions”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **HWCP0:** exact current receipt ID and digest for “HTML/WC public-surface adapters”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** revalidate formatter, IDE, lint/action, and public work on one cumulative candidate without becoming an implementation owner.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **legacy shared custom-element registry**, **framework-local HTML fact authority** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

- **HWC4-AC1 — sole-owner proof:** add `hwc4_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **HWC4-AC2 — positive contract:** add `hwc4_publishes_exact_htmlfacts`; assert exact identities, provenance, completeness, and deterministic ordering.
- **HWC4-AC3 — incremental equivalence:** add `hwc4_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **HWC4-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
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

- `source:successor-expansion.md:L1168`

## Reconciled source-plan contract

**Intent:** revalidate formatter, IDE, lint/action, and public work on one cumulative candidate without becoming an implementation owner.
**Predecessors:** `FMTH0`, `HWCI0`, `HWCL0`, `HWCP0`.
**Subblocks:** (1) regenerate exact capability/test matrices; (2) run cross-operation transaction and map tests; (3) verify one owner per diagnostic/edit/fact; (4) verify per-surface maturity and compiler disposition; (5) run fresh/incremental/cancellation/coexistence suites; (6) independent exact-candidate reviews.
**Acceptance:** all locked cells pass on the same tree; HTML formatting, fixes, and refactors remain distinct transactions; the gate contains no implementation fix.
**Forbidden:** repairing code in the join, treating a CLI adapter as available, or lowering an owner’s locked criteria.
**Deletion/abort:** delete nothing; any finding returns to its exact owner and invalidates convergence.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1168-F2A5600CA757

- Kind: `requirement`
- Source: `successor-expansion.md:1168-1168`
- Applicability: `HWC4`
- Exact text SHA-256: `f2a5600ca757e2a6ae901acf51284445375bb7f7bf78b037ba583aa21c832fdb`

~~~~markdown
### `HWC4.md` — HTML/WC read-only product convergence
~~~~

### SRC-EXP-L1170-F18B13ECB502

- Kind: `forbidden`
- Source: `successor-expansion.md:1170-1175`
- Applicability: `HWC4`
- Exact text SHA-256: `f18b13ecb502b1f9d49168bc13c434aa94670917b490524e2d6fffd3b7d1dc21`

~~~~markdown
**Intent:** revalidate formatter, IDE, lint/action, and public work on one cumulative candidate without becoming an implementation owner.
**Predecessors:** `FMTH0`, `HWCI0`, `HWCL0`, `HWCP0`.
**Subblocks:** (1) regenerate exact capability/test matrices; (2) run cross-operation transaction and map tests; (3) verify one owner per diagnostic/edit/fact; (4) verify per-surface maturity and compiler disposition; (5) run fresh/incremental/cancellation/coexistence suites; (6) independent exact-candidate reviews.
**Acceptance:** all locked cells pass on the same tree; HTML formatting, fixes, and refactors remain distinct transactions; the gate contains no implementation fix.
**Forbidden:** repairing code in the join, treating a CLI adapter as available, or lowering an owner’s locked criteria.
**Deletion/abort:** delete nothing; any finding returns to its exact owner and invalidates convergence.
~~~~
