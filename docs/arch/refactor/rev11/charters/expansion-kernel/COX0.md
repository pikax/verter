<!-- unified-charter-v2
id=COX0
name=Per-profile editor participation and coexistence
phase=expansion
train=expansion.kernel
product=kernel
kind=cutover
semantic_role=delivery
class=successor
predecessors=DEM0,IDX0
conditional_predecessors=
owner=expansion.kernel:typed immutable universal catalog and demand-selected kernel services
conflict_domains=carrierprofileid
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
source_refs=source:successor-expansion.md:L932
external_requirements=
activation_gate=ORC0
charter=charters/expansion-kernel/COX0.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# COX0 — Per-profile editor participation and coexistence

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Per-profile editor participation and coexistence. The current owner is **framework-shaped host/session registries and untagged public boundaries**. The final and sole owner is **typed immutable universal catalog and demand-selected kernel services**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `crates/verter_protocol/src`, `crates/verter_identity/src`.
- Named API/data boundaries: `CarrierProfileId`, `FrameworkProfileId`, `ProjectProfileId`, `CatalogSnapshot`, `DemandPlan`, `TypeInfoRequest`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **DEM0:** exact current receipt ID and digest for “Selection, two-stage activation, and demand planning”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **IDX0:** exact current receipt ID and digest for “Atomic semantic contributions and workspace index”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Revision:** 4 — supersedes the 251-charter all-verticals proposal
- **Prepared:** 2026-08-26
- **Repository basis:** program/architecture-lock at d1f3d50a948597f036868543b9bb21acacd730ff
- **Current-program condition:** maintainer work freeze; TCM0 = RESCOPE_REQUIRED; TCM1–TCM4 = LOCKED

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **COX0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **COX0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **COX0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **COX0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_language/tests`, `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **central framework switch**.
- Delete or structurally reject: **untagged coordinate/public identity**.
- Delete or structurally reject: **duplicate component information authority**.
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

1. `cargo nextest run -p verter_language -p verter_protocol -p verter_session`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 3 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, `architecture-specialist`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L932`

## Reconciled source-plan contract

**Intent:** allow Verter to stand down interactively while retaining explicitly requested workspace semantics.
**Predecessors:** `DEM0`, `IDX0`.
**Subblocks:** (1) public `auto|disabled|workspace|full` presets with clearer UI aliases evaluated during implementation; (2) effective `Disabled|WorkspaceOnly|Full`; (3) abstract per-profile, per-document-selector capability ownership mask; (4) editor-host extension observation via generated descriptor data; (5) dynamic register/unregister, diagnostic clearing, cancellation, and epoch transitions; (6) formatter-only, diagnostics-only, navigation-only, workspace-only, and full zero-work/audit tests.
**Acceptance:** installing/enabling a conflicting test extension under `auto` withdraws only overlapping capabilities while unrelated hover/completion/navigation/formatting remain available; `workspace` contributes only demanded bounded semantics; explicit `full` wins; Rust receives capability masks but no extension IDs.
**Forbidden:** file-extension heuristics in core, “workspace” publishing diagnostics/actions, mode changes serving stale results, or hidden processing in `disabled`.
**Deletion/abort:** remove old global on/off gates and per-framework client branches; abort if an LSP capability cannot be dynamically or truthfully withdrawn.

## Collapsed non-authoritative subblock disposition

The recovery candidate mechanically split this source-owned atomic node into the following labels: `COX0-A`, `COX0-B`, `COX0-C`, `COX0-D`, `COX0-E`, `COX0-F`. They have no separate dispatch, lease, receipt, migration manifest, deletion ownership, or review standing. Their useful source-described concerns are internal RED/GREEN checklist items of **COX0**; COX0 alone owns the complete migration population, exactly one final deletion/cutover, and atomic acceptance. Any quoted “suggested subblock” wording in transferred source text is non-authoritative planning context.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L932-680571E04059

- Kind: `context`
- Source: `successor-expansion.md:932-932`
- Applicability: `COX0`
- Exact text SHA-256: `680571e0405904cfa5a20e494db631937699df56d99c9148704f2373f939e5aa`

~~~~markdown
### `COX0.md` — Per-profile editor participation and coexistence
~~~~

### SRC-EXP-L934-49D0FFCCD051

- Kind: `forbidden`
- Source: `successor-expansion.md:934-939`
- Applicability: `COX0`
- Exact text SHA-256: `49d0ffccd051ec993e9ab56c9ddc6589e17bd96b34fcf7747df34a3a052a7b5e`

~~~~markdown
**Intent:** allow Verter to stand down interactively while retaining explicitly requested workspace semantics.
**Predecessors:** `DEM0`, `IDX0`.
**Subblocks:** (1) public `auto|disabled|workspace|full` presets with clearer UI aliases evaluated during implementation; (2) effective `Disabled|WorkspaceOnly|Full`; (3) abstract per-profile, per-document-selector capability ownership mask; (4) editor-host extension observation via generated descriptor data; (5) dynamic register/unregister, diagnostic clearing, cancellation, and epoch transitions; (6) formatter-only, diagnostics-only, navigation-only, workspace-only, and full zero-work/audit tests.
**Acceptance:** installing/enabling a conflicting test extension under `auto` withdraws only overlapping capabilities while unrelated hover/completion/navigation/formatting remain available; `workspace` contributes only demanded bounded semantics; explicit `full` wins; Rust receives capability masks but no extension IDs.
**Forbidden:** file-extension heuristics in core, “workspace” publishing diagnostics/actions, mode changes serving stale results, or hidden processing in `disabled`.
**Deletion/abort:** remove old global on/off gates and per-framework client branches; abort if an LSP capability cannot be dynamically or truthfully withdrawn.
~~~~
