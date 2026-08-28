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
conditional_predecessors=
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
source_refs=source:compiler-proposal.md:L1088
external_requirements=
activation_gate=ORC0
charter=charters/compiler-compiler-perf/CPER2.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# CPER2 — Shared compiler physical-execution and zero-work terminal

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Shared compiler physical-execution and zero-work terminal. The current owner is **unattributed compiler work and benchmark-only totals**. The final and sole owner is **phase/owner-labeled equivalent-work ledger**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src`, `crates/verter_bench/benches`, `crates/verter_audit/src`.
- Named API/data boundaries: `CompilerWorkLedger`, `WorkKind`, `OwnerPhase`, `AllocationClass`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **CMP4:** exact current receipt ID and digest for “Segmented emission, qualified artifacts, assembly, and host integration”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **CPER1:** exact current receipt ID and digest for “Compiler work ledger and lifetime attribution”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** verify the common compiler substrate before framework V2 trains depend on it.
- **Problem:** clean contracts can still produce physically materialized multi-pass implementations, hidden map work, or memory regressions.
- **Solution and architecture decisions:** run architecture-budget tests and canaries over the accepted common engine.
- **Required laws:**

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **CPER2-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **CPER2-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **CPER2-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **CPER2-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
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
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_bench`
2. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial`, `conformance`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 2/2 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `targeted` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 2 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1088`

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

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1088-07CC4EC112C8

- Kind: `context`
- Source: `compiler-proposal.md:1088-1088`
- Applicability: `CPER2`
- Exact text SHA-256: `07cc4ec112c8fd08de92ba875894d5406f2442909db46b99c0cd7825b19791f7`

~~~~markdown
## `CPER2.md` — Shared compiler physical-execution and zero-work terminal
~~~~

### SRC-COMP-L1090-33D9290AB28C

- Kind: `context`
- Source: `compiler-proposal.md:1090-1090`
- Applicability: `CPER2`
- Exact text SHA-256: `33d9290ab28ca9977113cf45dc2d225e951b61e7a152ca10d56a4ccc1759302f`

~~~~markdown
**Intent:** verify the common compiler substrate before framework V2 trains depend on it.
~~~~

### SRC-COMP-L1092-FF883E0BE0E7

- Kind: `context`
- Source: `compiler-proposal.md:1092-1092`
- Applicability: `CPER2`
- Exact text SHA-256: `ff883e0be0e7ee87237871f7e97c6b67a0333d5e18c8144ec0f3f8be7f347114`

~~~~markdown
**Problem:** clean contracts can still produce physically materialized multi-pass implementations, hidden map work, or memory regressions.
~~~~

### SRC-COMP-L1094-FDB918AF6ED7

- Kind: `context`
- Source: `compiler-proposal.md:1094-1094`
- Applicability: `CPER2`
- Exact text SHA-256: `fdb918af6ed73ef8e76ef9e62d822e2c2253ab495d4f9cac35173a11fe32433e`

~~~~markdown
**Solution and architecture decisions:** run architecture-budget tests and canaries over the accepted common engine.
~~~~

### SRC-COMP-L1096-C431D79C12CA

- Kind: `requirement`
- Source: `compiler-proposal.md:1096-1096`
- Applicability: `CPER2`
- Exact text SHA-256: `c431d79c12cacfd2aab7cfa590a62c039c8c5d73339482f2c930dd0c84ee9f75`

~~~~markdown
**Required laws:**
~~~~

### SRC-COMP-L1098-C2215B4978ED

- Kind: `requirement`
- Source: `compiler-proposal.md:1098-1098`
- Applicability: `CPER2`
- Exact text SHA-256: `c2215b4978ed7fdb0bb61707e065578e1ea540f0eabe64904f4cadb0d1b3bb5f`

~~~~markdown
- no redundant authoritative parse of the same exact region/grammar product;
~~~~

### SRC-COMP-L1099-C49170602AE1

- Kind: `context`
- Source: `compiler-proposal.md:1099-1099`
- Applicability: `CPER2`
- Exact text SHA-256: `c49170602ae123bb0519157fdaa73648d3ebad220a116b788dd82d75c1152eda`

~~~~markdown
- no semantic raw-source searching after parse;
~~~~

### SRC-COMP-L1100-787D7D1F13EF

- Kind: `context`
- Source: `compiler-proposal.md:1100-1100`
- Applicability: `CPER2`
- Exact text SHA-256: `787d7d1f13efe706ad0cd0d6b98a46ca87a000d07586d4d22edd0810a3e02fa5`

~~~~markdown
- no compiler-local duplicate framework analysis;
~~~~

### SRC-COMP-L1101-0620B5C8A49E

- Kind: `context`
- Source: `compiler-proposal.md:1101-1101`
- Applicability: `CPER2`
- Exact text SHA-256: `0620b5c8a49ef354346456b696325d0e426058f0e6a1911fa99a057095f9f3d8`

~~~~markdown
- no lossless/recovery allocation in valid strict compilation;
~~~~

### SRC-COMP-L1102-6AAD62A8AE1A

- Kind: `context`
- Source: `compiler-proposal.md:1102-1102`
- Applicability: `CPER2`
- Exact text SHA-256: `6aad62a8ae1ab60bcac2da8a6c1e44f508deab221d2df95632ef2fdab41af41c`

~~~~markdown
- no per-node dynamic target dispatch;
~~~~

### SRC-COMP-L1103-ADA7285CE3CB

- Kind: `context`
- Source: `compiler-proposal.md:1103-1103`
- Applicability: `CPER2`
- Exact text SHA-256: `ada7285ce3cb4ad35ab6e64272296dea1cf23a1791a715257193ca76dfe9ffbd`

~~~~markdown
- no map work when maps are disabled;
~~~~

### SRC-COMP-L1104-05577BF9C1E7

- Kind: `requirement`
- Source: `compiler-proposal.md:1104-1104`
- Applicability: `CPER2`
- Exact text SHA-256: `05577bf9c1e76c6dbf7cdb74909a6ed94b45744070fa4135ad35c7b1a6f523d9`

~~~~markdown
- no client effect planning for server-only targets;
~~~~

### SRC-COMP-L1105-C3225DE17E1A

- Kind: `context`
- Source: `compiler-proposal.md:1105-1105`
- Applicability: `CPER2`
- Exact text SHA-256: `c3225de17e1aca5876a18958fe87d22cf3cdde29d4212fcab22f6c45cc3884bd`

~~~~markdown
- unknown facts cannot enable optimization;
~~~~

### SRC-COMP-L1106-A0C37630581B

- Kind: `context`
- Source: `compiler-proposal.md:1106-1106`
- Applicability: `CPER2`
- Exact text SHA-256: `a0c37630581b036a9f911e9121a8ff79bb025aa8a60aecf3323b716be065afe5`

~~~~markdown
- raw source copy bytes are zero for representation ownership;
~~~~

### SRC-COMP-L1107-553EAEB4A1E2

- Kind: `requirement`
- Source: `compiler-proposal.md:1107-1107`
- Applicability: `CPER2`
- Exact text SHA-256: `553eaeb4a1e21696e7000010f195342f339301e35160d5a4dcd657bfd4feded0`

~~~~markdown
- incremental/prepared reuse validates exact basis.
~~~~

### SRC-COMP-L1109-37C00B96FE9F

- Kind: `context`
- Source: `compiler-proposal.md:1109-1109`
- Applicability: `CPER2`
- Exact text SHA-256: `37c00b96fe9f6b76122af3b7cf5c12e8eb61de585c2ef2ac0a04a820086a84c7`

~~~~markdown
**Budgets:** node sizes, source-sized visits, region/graph visits, allocations, bytes/lifetime, emission copies, map segments, cancellation waste, and disabled instrumentation overhead.
~~~~

### SRC-COMP-L1111-900049A6469D

- Kind: `context`
- Source: `compiler-proposal.md:1111-1111`
- Applicability: `CPER2`
- Exact text SHA-256: `900049a6469d27cf62ce6cc6f262f40abda6347d8bdcd35633facda73caaf971`

~~~~markdown
**Suggested predecessors:** `CMP4`, `CPER1`.
~~~~

### SRC-COMP-L1113-24EBCDF1E578

- Kind: `context`
- Source: `compiler-proposal.md:1113-1113`
- Applicability: `CPER2`
- Exact text SHA-256: `24ebcdf1e57895b38d3b9b8dbeeb85368b423087734942505bcb6f7f8e0d0f7e`

~~~~markdown
**Suggested subblocks:** strict-path canary, maps/no-maps canary, server/client demand canary, multi-target sharing canary, memory/RSS soak, exact-candidate architecture review.
~~~~

### SRC-COMP-L1115-506B0A604FD0

- Kind: `acceptance`
- Source: `compiler-proposal.md:1115-1115`
- Applicability: `CPER2`
- Exact text SHA-256: `506b0a604fd09eab1eead772eb1446e40bf941f28868e548483572ef361b4535`

~~~~markdown
**Acceptance:** all laws pass mechanically; every budget has a pinned value and equivalent-work basis; no implementation fix is made inside the terminal candidate.
~~~~

### SRC-COMP-L1117-12BA562A1F66

- Kind: `forbidden`
- Source: `compiler-proposal.md:1117-1117`
- Applicability: `CPER2`
- Exact text SHA-256: `12ba562a1f66218181e5d80ea62526e9a725d0f52a839e19e44980541235220e`

~~~~markdown
**Forbidden:** changing gates after measurement, treating “one pass” as a universal law, or accepting unexplained extra work because wall time remains noisy.
~~~~

### SRC-COMP-L1119-46822256486E

- Kind: `deletion`
- Source: `compiler-proposal.md:1119-1119`
- Applicability: `CPER2`
- Exact text SHA-256: `46822256486e853673e534082bcbe88d2f60cb2439c7eca31dc02b8234843b4b`

~~~~markdown
**Deletion/abort:** findings return to `CMP0`–`CMP4` or `CPER1`; this terminal deletes nothing.
~~~~

### SRC-COMP-L1121-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1121-1121`
- Applicability: `CPER2`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
