<!-- unified-charter-v2
id=CPER2
name=Shared compiler physical-execution and zero-work terminal
phase=compiler
train=compiler.compiler-perf
product=compiler_perf
kind=convergence
semantic_role=convergence
class=compiler
predecessors=CMP4,CPER1
owner=compiler.compiler-perf:phase/owner-labeled equivalent-work ledger
conflict_domains=compiler_execution,performance_evidence
resource_class=docs-light
review_profile=semantic-3
gate_profile=docs-domain
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
release_gating=contract
external_requirements=
charter=charters/compiler-compiler-perf/CPER2.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CPER2 — Shared compiler physical-execution and zero-work terminal

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Shared compiler physical-execution and zero-work terminal. The current owner is **unattributed compiler work and benchmark-only totals**. The final and sole owner is **phase/owner-labeled equivalent-work ledger**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src`, `crates/verter_bench/benches`, `crates/verter_audit/src`.
- Named API/data boundaries: `CompilerWorkLedger`, `WorkKind`, `OwnerPhase`, `AllocationClass`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **CMP4:** implemented ledger row for “Segmented emission, qualified artifacts, assembly, and host integration”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **CPER1:** implemented ledger row for “Compiler work ledger and lifetime attribution”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- **Intent:** verify the common compiler substrate before framework V2 trains depend on it.
- **Problem:** clean contracts can still produce physically materialized multi-pass implementations, hidden map work, or memory regressions.
- **Solution and architecture decisions:** run architecture-budget tests and canaries over the accepted common engine.
- **Required laws:**

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **CPER2-AC1 — sole-owner outcome:** every required outcome and consumer is covered by its implemented owning ancestor, and each required displaced route is already deleted or structurally rejected by that owner. Check the complete inventory, ancestor paths, acceptance criteria and executed evidence; missing, conflicting or residual ownership fails this verifier and returns to the predecessor owner. A named future owner cannot satisfy closure, and this node adds no missing production mechanism.
- **CPER2-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **CPER2-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **CPER2-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_bench`, `crates/verter_compiler/tests`.

## Deletions and forbidden designs

- Verify the owning predecessor has deleted or structurally rejected: **unlabeled work counters**. This node changes no production route.
- Verify the owning predecessor has deleted or structurally rejected: **wall-clock-only acceptance**. This node changes no production route.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance acceptance: use the exact applicable metric rows and methodology from performance-gates.toml, the applicable MEM0 budget, or the owning ratified product catalog, under contracts/resource-and-finalization.md (L2). Exact work invariants, latency/allocation/RSS limits under their owning methodology, and bounded new-capability budgets are distinct. New capabilities and deliberate pressure policies declare bounded new work and replacement SLOs before measurement. Missing required coverage needs an owning-contract amendment before measurement; no implicit 0.0% threshold or post-hoc rebaseline applies.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_bench`
2. Run every final command in the bound `docs-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial`, `conformance`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 2/2 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `targeted` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Reconciled source-plan contract

**Intent:** verify the common compiler substrate before framework V2 trains depend on it.

**Problem:** clean contracts can still produce physically materialized multi-pass implementations, hidden map work, or memory regressions.

**Solution and architecture decisions:** run architecture-budget tests and canaries over the accepted common engine.

**Required laws:**

- no redundant authoritative parse of the same exact region/grammar product;
- no semantic raw-source searching after parse;
- no compiler-local duplicate framework analysis;
- no lossless/recovery allocation in valid strict compilation;
- no per-node dynamic target dispatch;
- no map work when maps are disabled;
- no client effect planning for server-only targets;
- unknown facts cannot enable optimization;
- raw source copy bytes are zero for representation ownership;
- incremental/prepared reuse validates exact basis.

**Budgets:** node sizes, source-sized visits, region/graph visits, allocations, bytes/lifetime, emission copies, map segments, cancellation waste, and disabled instrumentation overhead.

**Suggested predecessors:** `CMP4`, `CPER1`.

**Normative source decomposition:** strict-path canary, maps/no-maps canary, server/client demand canary, multi-target sharing canary, memory/RSS soak, exact-candidate architecture review.

**Acceptance:** all laws pass mechanically; every budget has a pinned value and equivalent-work basis; no implementation fix is made inside the terminal candidate.

**Forbidden:** changing gates after measurement, treating “one pass” as a universal law, or accepting unexplained extra work because wall time remains noisy.

**Deletion/abort:** findings return to `CMP0`–`CMP4` or `CPER1`; this terminal deletes nothing.

---

