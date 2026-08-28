<!-- unified-charter-v2
id=CPER3
name=Cross-framework compiler soak and equivalent-work study
phase=compiler
train=compiler.compiler-perf
product=compiler_perf
kind=soak
semantic_role=delivery
class=compiler
predecessors=VCP7,SCP7
conditional_predecessors=
owner=compiler.compiler-perf:phase/owner-labeled equivalent-work ledger
conflict_domains=compiler_execution,performance_evidence
resource_class=rust-mixed
review_profile=semantic-3
gate_profile=targeted-domain
implementation_effort_min=medium
implementation_effort_default=medium
review_effort_min=medium
review_effort_default=medium
verification_effort_min=medium
verification_effort_default=medium
confirmation_effort_min=medium
confirmation_effort_default=medium
size=M
dispatchable=true
optional=false
release_gating=non_release
source_refs=source:compiler-proposal.md:L1781
external_requirements=
activation_gate=ORC0
charter=charters/compiler-compiler-perf/CPER3.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# CPER3 — Cross-framework compiler soak and equivalent-work study

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Cross-framework compiler soak and equivalent-work study. The current owner is **unattributed compiler work and benchmark-only totals**. The final and sole owner is **phase/owner-labeled equivalent-work ledger**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src`, `crates/verter_bench/benches`, `crates/verter_audit/src`.
- Named API/data boundaries: `CompilerWorkLedger`, `WorkKind`, `OwnerPhase`, `AllocationClass`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **VCP7:** exact current receipt ID and digest for “Vue Default compiler product terminal”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **SCP7:** exact current receipt ID and digest for “Svelte Default compiler product terminal”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** measure the mature common engine and both compilers under long-running, mixed, multi-target, incremental and concurrent workloads.
- **Problem:** independent product benchmarks do not expose shared-engine RSS, allocator, scheduler, cache or mixed-workspace pathologies.
- **Solution and architecture decisions:** non-release soak covering:
- mixed Vue/Svelte batches;

## Acceptance IDs and discriminating proof

- **CPER3-AC1 — sole-owner proof:** add `cper3_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **CPER3-AC2 — positive contract:** add `cper3_publishes_exact_compilerworkledger`; assert exact identities, provenance, completeness, and deterministic ordering.
- **CPER3-AC3 — incremental equivalence:** add `cper3_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **CPER3-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
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
- Performance budget: equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance is 0.0% unless an owning-authority amendment supplies exact replacement thresholds; after warmup, retained bytes may not increase across 100 identical requests.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_bench`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `semantic-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three fresh distinct harness reviewers with assigned lenses, deterministic low|medium|high effort, exact task/provider/model bindings, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1781`

## Reconciled source-plan contract

**Intent:** measure the mature common engine and both compilers under long-running, mixed, multi-target, incremental and concurrent workloads.

**Problem:** independent product benchmarks do not expose shared-engine RSS, allocator, scheduler, cache or mixed-workspace pathologies.

**Solution and architecture decisions:** non-release soak covering:

- mixed Vue/Svelte batches;
- client/server or VDOM/SSR/Vapor multi-target sharing;
- maps/no maps;
- direct/prepared/managed execution;
- edit storms, cancellation and stale-result rejection;
- long-session RSS plateau and idle CPU;
- small-file batching and large-component thresholds;
- selector direct/indexed thresholds;
- output/runtime/map equivalence.

**Suggested predecessors:** `VCP7`, `SCP7`.

**Acceptance:** no unbounded growth, cross-framework cache collision, duplicated prerequisite work, or throughput regression hidden by parallelism; every result retains exact correctness basis.

**Forbidden:** using the soak as a global release gate or changing accepted product criteria in the join.

**Deletion/abort:** findings create bounded owner follow-ups; non-release.

---

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1781-C8169BF2453F

- Kind: `context`
- Source: `compiler-proposal.md:1781-1781`
- Applicability: `CPER3`
- Exact text SHA-256: `c8169bf2453fad8ea0a29ed7f4e20e44de4a3a85482f2e151e87ee2f4c8ba209`

~~~~markdown
## `CPER3.md` — Cross-framework compiler soak and equivalent-work study
~~~~

### SRC-COMP-L1783-EB613B74AB42

- Kind: `context`
- Source: `compiler-proposal.md:1783-1783`
- Applicability: `CPER3`
- Exact text SHA-256: `eb613b74ab42ffabd2d75030bc8b11ede0e4cf4a999a843cd7cb872c352d901c`

~~~~markdown
**Intent:** measure the mature common engine and both compilers under long-running, mixed, multi-target, incremental and concurrent workloads.
~~~~

### SRC-COMP-L1785-14488F6A61DF

- Kind: `context`
- Source: `compiler-proposal.md:1785-1785`
- Applicability: `CPER3`
- Exact text SHA-256: `14488f6a61df476d4df239b7bce9b172680fb8d5ac2630ee91c47705b48d94e9`

~~~~markdown
**Problem:** independent product benchmarks do not expose shared-engine RSS, allocator, scheduler, cache or mixed-workspace pathologies.
~~~~

### SRC-COMP-L1787-743589F4F472

- Kind: `context`
- Source: `compiler-proposal.md:1787-1787`
- Applicability: `CPER3`
- Exact text SHA-256: `743589f4f47213c45f6d7d2d028407bf10b76cf8cd6409b5ef33e2d9b6a50aca`

~~~~markdown
**Solution and architecture decisions:** non-release soak covering:
~~~~

### SRC-COMP-L1789-FDF935CB17F8

- Kind: `context`
- Source: `compiler-proposal.md:1789-1789`
- Applicability: `CPER3`
- Exact text SHA-256: `fdf935cb17f8a14909e0be6dbcabfc1da8d4916f47f7afad9970e67c174fc081`

~~~~markdown
- mixed Vue/Svelte batches;
~~~~

### SRC-COMP-L1790-53DBE7DE406E

- Kind: `context`
- Source: `compiler-proposal.md:1790-1790`
- Applicability: `CPER3`
- Exact text SHA-256: `53dbe7de406ed0edcbed1e357114a8c288394c6e2c1ef47aa03ba03419a139e9`

~~~~markdown
- client/server or VDOM/SSR/Vapor multi-target sharing;
~~~~

### SRC-COMP-L1791-A6286F39B38B

- Kind: `context`
- Source: `compiler-proposal.md:1791-1791`
- Applicability: `CPER3`
- Exact text SHA-256: `a6286f39b38b66cd3c73b1be60c4e790aa1116de26064007e6b8f31dcb5ee5eb`

~~~~markdown
- maps/no maps;
~~~~

### SRC-COMP-L1792-B6B532F355F7

- Kind: `context`
- Source: `compiler-proposal.md:1792-1792`
- Applicability: `CPER3`
- Exact text SHA-256: `b6b532f355f7d6c17b7ebec8f2482a855358d0cf6a61381d44bee196774c9b7d`

~~~~markdown
- direct/prepared/managed execution;
~~~~

### SRC-COMP-L1793-2639400E56A4

- Kind: `context`
- Source: `compiler-proposal.md:1793-1793`
- Applicability: `CPER3`
- Exact text SHA-256: `2639400e56a40c86b1f941d3e47002468cf73e3dc953cff4178bec1dad6ae0b0`

~~~~markdown
- edit storms, cancellation and stale-result rejection;
~~~~

### SRC-COMP-L1794-E397EEC55D52

- Kind: `context`
- Source: `compiler-proposal.md:1794-1794`
- Applicability: `CPER3`
- Exact text SHA-256: `e397eec55d526a9ac7054ea025279a9db5fa73f80470be3a9873aff54d2f892a`

~~~~markdown
- long-session RSS plateau and idle CPU;
~~~~

### SRC-COMP-L1795-561A971AFD16

- Kind: `context`
- Source: `compiler-proposal.md:1795-1795`
- Applicability: `CPER3`
- Exact text SHA-256: `561a971afd16db2ab4232e2fdf9b8c88756ea531f15c9a9d88653c7676c1ce9d`

~~~~markdown
- small-file batching and large-component thresholds;
~~~~

### SRC-COMP-L1796-3AEDB7786370

- Kind: `context`
- Source: `compiler-proposal.md:1796-1796`
- Applicability: `CPER3`
- Exact text SHA-256: `3aedb7786370abac194c17d92ebf757209bd6435f800a9adbf70c04ccea1a4ec`

~~~~markdown
- selector direct/indexed thresholds;
~~~~

### SRC-COMP-L1797-2D9EEE937EC0

- Kind: `context`
- Source: `compiler-proposal.md:1797-1797`
- Applicability: `CPER3`
- Exact text SHA-256: `2d9eee937ec04a3c5c73df9f404434622e4975cb3a5864f7a9202d67153faeda`

~~~~markdown
- output/runtime/map equivalence.
~~~~

### SRC-COMP-L1799-158CAE019A6E

- Kind: `context`
- Source: `compiler-proposal.md:1799-1799`
- Applicability: `CPER3`
- Exact text SHA-256: `158cae019a6e0aeae206398211c47dc2309486847ebf015600af00666f1f3ab0`

~~~~markdown
**Suggested predecessors:** `VCP7`, `SCP7`.
~~~~

### SRC-COMP-L1801-24C34468BB0F

- Kind: `acceptance`
- Source: `compiler-proposal.md:1801-1801`
- Applicability: `CPER3`
- Exact text SHA-256: `24c34468bb0fcb18c8ad2913d13495585a89f5d9f9101312f02b3475c0fc72b7`

~~~~markdown
**Acceptance:** no unbounded growth, cross-framework cache collision, duplicated prerequisite work, or throughput regression hidden by parallelism; every result retains exact correctness basis.
~~~~

### SRC-COMP-L1803-8EBFC1010245

- Kind: `forbidden`
- Source: `compiler-proposal.md:1803-1803`
- Applicability: `CPER3`
- Exact text SHA-256: `8ebfc10102454b0bbcd19485cf91b5e67dc0b5d3d419f641575feb7b465186a9`

~~~~markdown
**Forbidden:** using the soak as a global release gate or changing accepted product criteria in the join.
~~~~

### SRC-COMP-L1805-27CC19CC78E6

- Kind: `deletion`
- Source: `compiler-proposal.md:1805-1805`
- Applicability: `CPER3`
- Exact text SHA-256: `27cc19cc78e6c4a751c3dfab93465fc2a22aebacc2ff823eb060e344be66de5e`

~~~~markdown
**Deletion/abort:** findings create bounded owner follow-ups; non-release.
~~~~

### SRC-COMP-L1807-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1807-1807`
- Applicability: `CPER3`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
