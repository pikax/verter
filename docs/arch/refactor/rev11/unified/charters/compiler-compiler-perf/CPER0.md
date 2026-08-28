<!-- unified-charter-v2
id=CPER0
name=Compiler equivalent-work and oracle genesis lock
phase=compiler
train=compiler.compiler-perf
product=compiler_perf
kind=lock
semantic_role=delivery
class=compiler
predecessors=CPF1,PAR0,PER0,CCA2
conditional_predecessors=
owner=compiler.compiler-perf:phase/owner-labeled equivalent-work ledger
conflict_domains=compiler_execution,performance_evidence
resource_class=docs-light
review_profile=architecture-3
gate_profile=docs-domain
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
source_refs=source:compiler-proposal.md:L778
external_requirements=
activation_gate=ORC0
charter=charters/compiler-compiler-perf/CPER0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# CPER0 — Compiler equivalent-work and oracle genesis lock

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Compiler equivalent-work and oracle genesis lock. The current owner is **unattributed compiler work and benchmark-only totals**. The final and sole owner is **phase/owner-labeled equivalent-work ledger**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src`, `crates/verter_bench/benches`, `crates/verter_audit/src`.
- Named API/data boundaries: `CompilerWorkLedger`, `WorkKind`, `OwnerPhase`, `AllocationClass`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **CPF1:** exact current receipt ID and digest for “Carrier frontend registration and Vue/Svelte cutover”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **PAR0:** exact current receipt ID and digest for “Parser decision, ownership, reuse, and lineage contract”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **PER0:** exact current receipt ID and digest for “Cache/backend identity, cancellation, budgets, and zero work”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **CCA2:** exact current receipt ID and digest for “Compiler artifact, assembly, style-stage, and host boundary”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** freeze correctness-equivalent compiler baselines and measurement methodology before the shared compiler engine or framework compilers mutate the workload.
- **Problem:** wall-clock numbers without output/runtime/map equivalence conceal invalid work and cannot distinguish architecture improvement from omitted functionality.
- **Solution and architecture decisions:**
- lock exact repository revisions, framework releases, target/options, corpora, runtime validators, map modes, diagnostic contracts, thread counts, machine classes, cold/warm/cache states, and RSS methodology;

## Acceptance IDs and discriminating proof

- **CPER0-AC1 — sole-owner proof:** add `cper0_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **CPER0-AC2 — positive contract:** add `cper0_publishes_exact_compilerworkledger`; assert exact identities, provenance, completeness, and deterministic ordering.
- **CPER0-AC3 — incremental equivalence:** add `cper0_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **CPER0-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_bench`, `crates/verter_compiler/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **unlabeled work counters**.
- Delete or structurally reject: **wall-clock-only acceptance**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance is 0.0% unless an owning-authority amendment supplies exact replacement thresholds; after warmup, retained bytes may not increase across 100 identical requests.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_bench`
2. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `architecture-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three fresh distinct harness reviewers with assigned lenses, deterministic low|medium|high effort, exact task/provider/model bindings, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L778`

## Reconciled source-plan contract

**Intent:** freeze correctness-equivalent compiler baselines and measurement methodology before the shared compiler engine or framework compilers mutate the workload.

**Problem:** wall-clock numbers without output/runtime/map equivalence conceal invalid work and cannot distinguish architecture improvement from omitted functionality.

**Solution and architecture decisions:**

- lock exact repository revisions, framework releases, target/options, corpora, runtime validators, map modes, diagnostic contracts, thread counts, machine classes, cold/warm/cache states, and RSS methodology;
- use each framework’s `DefaultCompilationContractId`, not byte similarity alone;
- retain upstream compilers as primary differential references while permitting prelocked Verter default divergences;
- capture current Verter phase/work/allocation/RSS baselines even when correctness is incomplete, but do not rank an invalid result as faster equivalent work;
- define the compiler-work-ledger schema and stable counter names;
- lock benchmark-failure and rebaseline governance before implementation direction is observed.

**Suggested predecessors:** successor `CPF1`, `PAR0`, `PER0`; accepted Revision 11 bridge and J train are external genesis receipts.

**Normative source decomposition:**

1. **CPER0-A — Corpus and behavior matrix.** Pin target/product equivalence cells for Vue and Svelte, including CSS and maps.
2. **CPER0-B — Runtime/hydration/output validators.** Ensure generated code is executed or otherwise semantically validated where applicable.
3. **CPER0-C — Work-ledger schema.** Define parse, semantic, style, planning, emission, maps, reuse, concurrency, and memory counters.
4. **CPER0-D — Baseline capture.** Capture direct, prepared, managed, single-target, multi-target, maps/no-maps, cold/warm and batch data.
5. **CPER0-E — Noise and machine policy.** Lock repetitions, outlier handling, CPU topology, memory collection, and reproducibility metadata.
6. **CPER0-F — Independent review.** Challenge equivalence, hidden work, and corpus representativeness before any numeric gate is accepted.

**Acceptance:** every comparison can prove equivalent requested work; invalid compiler outputs are identified rather than ranked; the baseline can attribute why work ran; no numeric target is invented from architectural optimism.

**Forbidden:** performance claims from microbenchmarks alone, mutable corpora, output-size-only correctness, unrecorded cache/thread state, or changing thresholds after candidate results are known.

**Deletion/abort:** no production deletion; repair a defective baseline and rerun both sides rather than weakening the contract.

---

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

No clause targets this file directly. Applicable contract clauses are selected by the validated `applicable_nodes` ledger and embedded verbatim in cold packets.
