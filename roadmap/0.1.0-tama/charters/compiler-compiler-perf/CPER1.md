<!-- unified-charter-v2
id=CPER1
name=Compiler work ledger and lifetime attribution
phase=compiler
train=compiler.compiler-perf
product=compiler_perf
kind=implementation
semantic_role=delivery
class=compiler
predecessors=CPER0,CMP0
owner=compiler.compiler-perf:phase/owner-labeled equivalent-work ledger
conflict_domains=compiler_execution,performance_evidence
resource_class=rust-mixed
review_profile=concurrency-3
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
charter=charters/compiler-compiler-perf/CPER1.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CPER1 — Compiler work ledger and lifetime attribution

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Compiler work ledger and lifetime attribution. The current owner is **unattributed compiler work and benchmark-only totals**. The final and sole owner is **phase/owner-labeled equivalent-work ledger**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src`, `crates/verter_bench/benches`, `crates/verter_audit/src`.
- Named API/data boundaries: `CompilerWorkLedger`, `WorkKind`, `OwnerPhase`, `AllocationClass`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **CPER0:** implemented ledger row for “Compiler equivalent-work and oracle genesis lock”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **CMP0:** implemented ledger row for “Compiler request, policy, compatibility, and identity contract”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- **Intent:** make compiler work, memory, and reuse mechanically observable with negligible disabled overhead.
- **Problem:** time measurements alone cannot catch extra traversals, reparses, allocations, or unrequested semantic/style work.
- **Compatibility closure:** remove the retained legacy work-attribution view when `CompilerWorkLedger`, `WorkKind`, and `OwnerPhase` become the sole production attribution boundary.
- **Solution and architecture decisions:**
- Implement a versioned CompileWorkLedger covering at least:

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **CPER1-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **CPER1-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **CPER1-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **CPER1-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_bench`, `crates/verter_compiler/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **unlabeled work counters**.
- Delete or structurally reject: **wall-clock-only acceptance**.
- Delete or structurally reject: **retained legacy work-attribution view over staged compiler events**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_bench`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `concurrency-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `concurrency-lifetime`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch adds one `[[implemented]]` row to `authority/state/implemented.toml` with the node ID, planned squash commit message, approximate date with timezone, and optional pull-request number. Row presence is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Reconciled source-plan contract

**Intent:** make compiler work, memory, and reuse mechanically observable with negligible disabled overhead.

**Problem:** time measurements alone cannot catch extra traversals, reparses, allocations, or unrequested semantic/style work.

**Solution and architecture decisions:**

Implement a versioned `CompileWorkLedger` covering at least:

```text
parse.full_source_scans
parse.region_scans[grammar]
parse.bytes[grammar]
parse.expression_attempts
parse.authoritative_expression_parses
parse.downstream_reparses
parse.raw_source_copy_bytes
parse.semantic_normalization_bytes

semantic.fact_families_demanded
semantic.facts_produced
semantic.fact_reads
semantic.binding_lookups
semantic.dependency_sets
semantic.dependency_edges
semantic.provenance_entries

structure.nodes_materialized
structure.regions
structure.topology_nodes
structure.source_sized_visits
structure.regional_visits
structure.graph_visits

style.blocks
style.selector_plans
style.index_builds
style.candidate_nodes
style.predicate_tests
style.combinator_hops
style.match_yes_maybe_no
style.pruned_rules
style.witnesses_materialized

planning.target_entries
planning.effect_nodes
planning.effect_edges
planning.multi_target_shared_prerequisites

emission.segments
emission.source_slice_bytes
emission.generated_bytes
emission.copy_bytes
emission.allocations
emission.map_segments

reuse.candidates
reuse.validated
reuse.rejected_by_basis
reuse.recomputed

memory.allocated_by_lifetime
memory.peak_by_lifetime
memory.retained_by_product
concurrency.tasks_spawned
concurrency.cancellation_waste
```

**Suggested predecessors:** `CPER0`, `CMP0`.

**Normative source decomposition:** instrumentation schema, leaf counters, memory/lifetime hooks, deterministic export, disabled-overhead benchmark, architecture gate integration.

**Acceptance:** counters are deterministic for equivalent single-thread work, attributable to named capabilities, stable-schema versioned, and cheap when disabled; strict valid compilation reports zero lossless-sidecar and downstream-reparse work.

**Forbidden:** counters becoming semantic authority, string-heavy per-node tracing in production, timing-based correctness, or a metric without an owner and definition.

**Deletion/abort:** remove superseded ad hoc compiler telemetry only after parity; abort counters whose disabled cost exceeds the prelocked budget.

---
