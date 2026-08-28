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
conditional_predecessors=
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
source_refs=source:compiler-proposal.md:L875
external_requirements=
activation_gate=ORC0
charter=charters/compiler-compiler-perf/CPER1.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# CPER1 — Compiler work ledger and lifetime attribution

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Compiler work ledger and lifetime attribution. The current owner is **unattributed compiler work and benchmark-only totals**. The final and sole owner is **phase/owner-labeled equivalent-work ledger**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src`, `crates/verter_bench/benches`, `crates/verter_audit/src`.
- Named API/data boundaries: `CompilerWorkLedger`, `WorkKind`, `OwnerPhase`, `AllocationClass`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **CPER0:** exact current receipt ID and digest for “Compiler equivalent-work and oracle genesis lock”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **CMP0:** exact current receipt ID and digest for “Compiler request, policy, compatibility, and identity contract”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** make compiler work, memory, and reuse mechanically observable with negligible disabled overhead.
- **Problem:** time measurements alone cannot catch extra traversals, reparses, allocations, or unrequested semantic/style work.
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
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_bench`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `concurrency-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `concurrency-lifetime`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 3 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, `concurrency-lifetime`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L875`

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

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L875-574040412B24

- Kind: `context`
- Source: `compiler-proposal.md:875-875`
- Applicability: `CPER1`
- Exact text SHA-256: `574040412b2438b134a34e657fd2d4e332f5d0e397397caf61bdcf01093214a9`

~~~~markdown
## `CPER1.md` — Compiler work ledger and lifetime attribution
~~~~

### SRC-COMP-L877-B28AAC0B8060

- Kind: `context`
- Source: `compiler-proposal.md:877-877`
- Applicability: `CPER1`
- Exact text SHA-256: `b28aac0b8060fbf2f826dbe432bd760ac3cac768907066895a6a7e08c26e2afd`

~~~~markdown
**Intent:** make compiler work, memory, and reuse mechanically observable with negligible disabled overhead.
~~~~

### SRC-COMP-L879-3B9425DB7788

- Kind: `context`
- Source: `compiler-proposal.md:879-879`
- Applicability: `CPER1`
- Exact text SHA-256: `3b9425db77881b62444bbf7ceceb13a49b827c16c38996e9b17d0019b9f19fed`

~~~~markdown
**Problem:** time measurements alone cannot catch extra traversals, reparses, allocations, or unrequested semantic/style work.
~~~~

### SRC-COMP-L881-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:881-881`
- Applicability: `CPER1`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L883-DD4BC4803A40

- Kind: `context`
- Source: `compiler-proposal.md:883-883`
- Applicability: `CPER1`
- Exact text SHA-256: `dd4bc4803a4035fd012625477f05720e287f896316031015f4dd604a60652c88`

~~~~markdown
Implement a versioned `CompileWorkLedger` covering at least:
~~~~

### SRC-COMP-L885-5CA372367AD2

- Kind: `context`
- Source: `compiler-proposal.md:885-893`
- Applicability: `CPER1`
- Exact text SHA-256: `5ca372367ad2bfa8696286e8d04c67ecd023e0fb535a25c0764457c54598bcc8`

~~~~markdown
```text
parse.full_source_scans
parse.region_scans[grammar]
parse.bytes[grammar]
parse.expression_attempts
parse.authoritative_expression_parses
parse.downstream_reparses
parse.raw_source_copy_bytes
parse.semantic_normalization_bytes
~~~~

### SRC-COMP-L895-612D858E47A9

- Kind: `requirement`
- Source: `compiler-proposal.md:895-901`
- Applicability: `CPER1`
- Exact text SHA-256: `612d858e47a9e1350e24db1d8271e32c30186e495d0a4f48072c2570d2e083fe`

~~~~markdown
semantic.fact_families_demanded
semantic.facts_produced
semantic.fact_reads
semantic.binding_lookups
semantic.dependency_sets
semantic.dependency_edges
semantic.provenance_entries
~~~~

### SRC-COMP-L903-F56CC0F8C6AF

- Kind: `context`
- Source: `compiler-proposal.md:903-908`
- Applicability: `CPER1`
- Exact text SHA-256: `f56cc0f8c6af7ec2bcaaf071da2d7951ff0cbad23ec5a48a747a0fe46dadc7b8`

~~~~markdown
structure.nodes_materialized
structure.regions
structure.topology_nodes
structure.source_sized_visits
structure.regional_visits
structure.graph_visits
~~~~

### SRC-COMP-L910-849B4BA66528

- Kind: `context`
- Source: `compiler-proposal.md:910-918`
- Applicability: `CPER1`
- Exact text SHA-256: `849b4ba66528a2a40d2d5af168fb284c08bce2afbf77dd41f87d9233a6a164a9`

~~~~markdown
style.blocks
style.selector_plans
style.index_builds
style.candidate_nodes
style.predicate_tests
style.combinator_hops
style.match_yes_maybe_no
style.pruned_rules
style.witnesses_materialized
~~~~

### SRC-COMP-L920-0DFA09081CD6

- Kind: `context`
- Source: `compiler-proposal.md:920-923`
- Applicability: `CPER1`
- Exact text SHA-256: `0dfa09081cd615d3104138d5fe6871e7551143a1959c9793f1ac4fdc8a51e9e5`

~~~~markdown
planning.target_entries
planning.effect_nodes
planning.effect_edges
planning.multi_target_shared_prerequisites
~~~~

### SRC-COMP-L925-6B84D73803AE

- Kind: `context`
- Source: `compiler-proposal.md:925-930`
- Applicability: `CPER1`
- Exact text SHA-256: `6b84d73803ae6cff0b706b7fcc8bea6648e72613870587e7d28aaf17850f4cf2`

~~~~markdown
emission.segments
emission.source_slice_bytes
emission.generated_bytes
emission.copy_bytes
emission.allocations
emission.map_segments
~~~~

### SRC-COMP-L932-5FAB78C6E566

- Kind: `context`
- Source: `compiler-proposal.md:932-935`
- Applicability: `CPER1`
- Exact text SHA-256: `5fab78c6e566c4eaef6fc08a4ea0d9265b75cd15c2a98c7b0c7b9bdc57f7eab4`

~~~~markdown
reuse.candidates
reuse.validated
reuse.rejected_by_basis
reuse.recomputed
~~~~

### SRC-COMP-L937-29D0B743873A

- Kind: `context`
- Source: `compiler-proposal.md:937-942`
- Applicability: `CPER1`
- Exact text SHA-256: `29d0b743873a1f9812df5b6009c6271e90bc9f3da659b14013d328ab81aa7702`

~~~~markdown
memory.allocated_by_lifetime
memory.peak_by_lifetime
memory.retained_by_product
concurrency.tasks_spawned
concurrency.cancellation_waste
```
~~~~

### SRC-COMP-L944-2DDF91B0A790

- Kind: `context`
- Source: `compiler-proposal.md:944-944`
- Applicability: `CPER1`
- Exact text SHA-256: `2ddf91b0a790d8023643bda2318b3cb6d8fb6f990fdc2c75aabf07d847282f34`

~~~~markdown
**Suggested predecessors:** `CPER0`, `CMP0`.
~~~~

### SRC-COMP-L946-D500CCA55222

- Kind: `context`
- Source: `compiler-proposal.md:946-946`
- Applicability: `CPER1`
- Exact text SHA-256: `d500cca55222b4a4ba5cb015e34a867bb128989370550e5e32119b1465a19d3c`

~~~~markdown
**Suggested subblocks:** instrumentation schema, leaf counters, memory/lifetime hooks, deterministic export, disabled-overhead benchmark, architecture gate integration.
~~~~

### SRC-COMP-L948-AA5DA2A2F4C7

- Kind: `acceptance`
- Source: `compiler-proposal.md:948-948`
- Applicability: `CPER1`
- Exact text SHA-256: `aa5da2a2f4c7bc8d993425fb2129552a6051404da1884a9dba1eec380c411696`

~~~~markdown
**Acceptance:** counters are deterministic for equivalent single-thread work, attributable to named capabilities, stable-schema versioned, and cheap when disabled; strict valid compilation reports zero lossless-sidecar and downstream-reparse work.
~~~~

### SRC-COMP-L950-6FCE8FDECDD9

- Kind: `forbidden`
- Source: `compiler-proposal.md:950-950`
- Applicability: `CPER1`
- Exact text SHA-256: `6fce8fdecdd971ea290345561cf9c7ee0047f7d136b58c15e2f8ee830c1a4495`

~~~~markdown
**Forbidden:** counters becoming semantic authority, string-heavy per-node tracing in production, timing-based correctness, or a metric without an owner and definition.
~~~~

### SRC-COMP-L952-E333D370B088

- Kind: `deletion`
- Source: `compiler-proposal.md:952-952`
- Applicability: `CPER1`
- Exact text SHA-256: `e333d370b08834abb36c519b500596df2aca9ca1b7073e427177e4ecb99822a0`

~~~~markdown
**Deletion/abort:** remove superseded ad hoc compiler telemetry only after parity; abort counters whose disabled cost exceeds the prelocked budget.
~~~~

### SRC-COMP-L954-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:954-954`
- Applicability: `CPER1`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
