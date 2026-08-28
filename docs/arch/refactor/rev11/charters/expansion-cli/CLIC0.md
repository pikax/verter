<!-- unified-charter-v2
id=CLIC0
name=Registered carrier `compile` command
phase=expansion
train=expansion.cli
product=cli
kind=adapter
semantic_role=delivery
class=successor
predecessors=CLI1,CPF1
conditional_predecessors=
owner=expansion.cli:one `verter` application service with thin command adapters
conflict_domains=cli_application
resource_class=ts-heavy
review_profile=architecture-3
gate_profile=ts-domain
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
source_refs=source:successor-expansion.md:L1511
external_requirements=
activation_gate=ORC0
charter=charters/expansion-cli/CLIC0.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# CLIC0 — Registered carrier `compile` command

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Registered carrier `compile` command. The current owner is **separate package launchers and command-local project logic**. The final and sole owner is **one `verter` application service with thin command adapters**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `packages/binary-launcher`, `packages/verter-lsp`, `packages/verter-tsc`, `crates/verter_mcp_server/src`.
- Named API/data boundaries: `ApplicationServices`, `SelectionPlan`, `Reporter`, `WriteTransaction`, `CommandCapability`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **CLI1:** exact current receipt ID and digest for “Shared application services, selection, invocation, reporters”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **CPF1:** exact current receipt ID and digest for “Carrier frontend registration and Vue/Svelte cutover”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Revision:** 4 — supersedes the 251-charter all-verticals proposal
- **Prepared:** 2026-08-26
- **Repository basis:** program/architecture-lock at d1f3d50a948597f036868543b9bb21acacd730ff
- **Current-program condition:** maintainer work freeze; TCM0 = RESCOPE_REQUIRED; TCM1–TCM4 = LOCKED

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **CLIC0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **CLIC0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **CLIC0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **CLIC0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `packages/binary-launcher/cli.spec.ts`, `crates/verter_mcp_server/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **command-local semantic engine**.
- Delete or structurally reject: **non-atomic multi-file writes**.
- Delete or structurally reject: **ambiguous offset encoding**.
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

1. `pnpm --filter @verter/binary-launcher test`
2. Run every final command in the bound `ts-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 3 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, `architecture-specialist`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L1511`

## Reconciled source-plan contract

**Intent:** route compilation only to optional Verter-owned compiler backends while keeping tooling-only carriers first-class.
**Predecessors:** `CLI1`, `CPF1`.
**Subblocks:** (1) resolve exact carrier/backend capability; (2) route Vue/Svelte SFC compilation; (3) return normalized `Supported | FutureSeparateTrain | NotApplicable`; (4) write output/map manifests atomically; (5) project/reference/watch selection; (6) differential/cancellation/performance tests.
**Acceptance:** Vue/Svelte preserve admitted compiler bytes/maps; Astro returns `FutureSeparateTrain`; HTML/MDX and other non-compiler carriers return `NotApplicable`; tooling availability is unaffected.
**Forbidden:** compiler stubs for every carrier, runtime ownership, treating tooling support as compilation, or generic “unsupported” that loses disposition.
**Deletion/abort:** migrate old compile shells only after parity; abort any backend without source-map and atomic-output guarantees.

## Collapsed non-authoritative subblock disposition

The recovery candidate mechanically split this source-owned atomic node into the following labels: `CLIC0-A`, `CLIC0-B`, `CLIC0-C`, `CLIC0-D`, `CLIC0-E`, `CLIC0-F`. They have no separate dispatch, lease, receipt, migration manifest, deletion ownership, or review standing. Their useful source-described concerns are internal RED/GREEN checklist items of **CLIC0**; CLIC0 alone owns the complete migration population, exactly one final deletion/cutover, and atomic acceptance. Any quoted “suggested subblock” wording in transferred source text is non-authoritative planning context.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1961-AF01A6A69146

- Kind: `context`
- Source: `compiler-proposal.md:1961-1961`
- Applicability: `CLIC0`
- Exact text SHA-256: `af01a6a69146c52bdc4084095c59b70e6f18d8c2302fb4ddbd2963a56dc3c34b`

~~~~markdown
## 11.7 `CLIC0`
~~~~

### SRC-COMP-L1963-77E358A8D925

- Kind: `deletion`
- Source: `compiler-proposal.md:1963-1963`
- Applicability: `CLIC0`
- Exact text SHA-256: `77e358a8d92569bb4d248f062ac55e97e5e853e8bf199cec9d95eabac5ed46f4`

~~~~markdown
`CLIC0` consumes the CCA2 `CompileArtifactSet` and exact runtime-compiler capability. It remains able to expose existing Vue/Svelte compilers before V2, through temporary adapters. VCP6/SCP6 later delete those adapters without changing CLI command semantics.
~~~~

### SRC-COMP-L1965-F05E79FB27F2

- Kind: `context`
- Source: `compiler-proposal.md:1965-1965`
- Applicability: `CLIC0`
- Exact text SHA-256: `f05e79fb27f2c9cb052d0d8384b6ecfc475c9d0964f7335b53bdf3384b8e3d42`

~~~~markdown
The command exposes:
~~~~

### SRC-COMP-L1967-02562180FDCE

- Kind: `context`
- Source: `compiler-proposal.md:1967-1971`
- Applicability: `CLIC0`
- Exact text SHA-256: `02562180fdce9de240d838e7176971298a21a500f9a31689a3a1b8d1fff51886`

~~~~markdown
```text
Supported
FutureSeparateTrain
NotApplicable
```
~~~~

### SRC-COMP-L1973-180203AC91A2

- Kind: `requirement`
- Source: `compiler-proposal.md:1973-1973`
- Applicability: `CLIC0`
- Exact text SHA-256: `180203ac91a2dc538a01168dce667dc4c336dae87528ca2f197ccab9ddfa6556`

~~~~markdown
and exposes `Optimized` only when its capability is actually accepted.
~~~~

### SRC-COMP-L1975-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1975-1975`
- Applicability: `CLIC0`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~

### SRC-EXP-L1511-7DD5F0DE8A40

- Kind: `context`
- Source: `successor-expansion.md:1511-1511`
- Applicability: `CLIC0`
- Exact text SHA-256: `7dd5f0de8a40d84f770de5c1fbc8f24c18bfb33c8efe3041293210fcdf3907e7`

~~~~markdown
### `CLIC0.md` — Registered carrier `compile` command
~~~~

### SRC-EXP-L1513-7C69D751D4A7

- Kind: `forbidden`
- Source: `successor-expansion.md:1513-1518`
- Applicability: `CLIC0`
- Exact text SHA-256: `7c69d751d4a7f2c274778f311dc6b899c37293eae06d9059fdf83ab2acf6c015`

~~~~markdown
**Intent:** route compilation only to optional Verter-owned compiler backends while keeping tooling-only carriers first-class.
**Predecessors:** `CLI1`, `CPF1`.
**Subblocks:** (1) resolve exact carrier/backend capability; (2) route Vue/Svelte SFC compilation; (3) return normalized `Supported | FutureSeparateTrain | NotApplicable`; (4) write output/map manifests atomically; (5) project/reference/watch selection; (6) differential/cancellation/performance tests.
**Acceptance:** Vue/Svelte preserve admitted compiler bytes/maps; Astro returns `FutureSeparateTrain`; HTML/MDX and other non-compiler carriers return `NotApplicable`; tooling availability is unaffected.
**Forbidden:** compiler stubs for every carrier, runtime ownership, treating tooling support as compilation, or generic “unsupported” that loses disposition.
**Deletion/abort:** migrate old compile shells only after parity; abort any backend without source-map and atomic-output guarantees.
~~~~
