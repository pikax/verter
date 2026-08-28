<!-- unified-charter-v2
id=SCP7
name=Svelte Default compiler product terminal
phase=compiler
train=compiler.svelte-compiler
product=svelte_compiler
kind=terminal
semantic_role=convergence
class=compiler
predecessors=SCP6,CPER2,BR0
conditional_predecessors=
owner=compiler.svelte-compiler:Svelte-owned Default compiler cells over shared compiler substrate
conflict_domains=compiler_execution,svelte_product
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
release_gating=product
source_refs=source:compiler-proposal.md:L1736
external_requirements=
activation_gate=ORC0
charter=charters/compiler-svelte-compiler/SCP7.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# SCP7 — Svelte Default compiler product terminal

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Svelte Default compiler product terminal. The current owner is **Svelte runtime emitter and assembly paths**. The final and sole owner is **Svelte-owned Default compiler cells over shared compiler substrate**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src/framework/svelte`, `crates/verter_svelte_conformance`, `packages/svelte-runtime-tests`.
- Named API/data boundaries: `SvelteSemanticSnapshot`, `SvelteCompilePlan`, `SvelteTarget`, `SvelteArtifactSet`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **SCP6:** exact current receipt ID and digest for “Svelte assembly, artifacts, host integration, and atomic cutover”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **CPER2:** exact current receipt ID and digest for “Shared compiler physical-execution and zero-work terminal”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **BR0:** exact current receipt ID and digest for “Post-L4 successor product promotion”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** independently promote the Svelte V2 default compiler after cumulative correctness, maps, CSS, performance, memory and deletion proof.
- **Suggested predecessors:** SCP6, CPER2.
- **Required evidence:** exact SCP0 matrix; client/server/module runtime and hydration; style scoping/pruning with no false negatives; maps; direct/prepared/managed and incremental/fresh equivalence; multi-target sharing; cold/warm/batch/RSS/cancellation; zero o
- **Acceptance:** all cells pass on one exact candidate and the experimental compiler is deleted.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **SCP7-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **SCP7-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **SCP7-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **SCP7-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_svelte_conformance/tests`, `packages/svelte-runtime-tests/test`.

## Deletions and forbidden designs

- Delete or structurally reject: **legacy Svelte emitter route**.
- Delete or structurally reject: **per-target prerequisite duplication**.
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

1. `cargo nextest run -p verter_compiler -p verter_svelte_conformance`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 3 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, `architecture-specialist`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1736`

## Reconciled source-plan contract

**Intent:** independently promote the Svelte V2 default compiler after cumulative correctness, maps, CSS, performance, memory and deletion proof.

**Suggested predecessors:** `SCP6`, `CPER2`.

**Required evidence:** exact `SCP0` matrix; client/server/module runtime and hydration; style scoping/pruning with no false negatives; maps; direct/prepared/managed and incremental/fresh equivalence; multi-target sharing; cold/warm/batch/RSS/cancellation; zero old compiler/matcher/session authorities; `Default = Supported`, `Optimized = FutureSeparateTrain`.

**Acceptance:** all cells pass on one exact candidate and the experimental compiler is deleted.

**Forbidden:** terminal implementation fixes, speed waivers, or enabling `Optimized`.

**Deletion/abort:** findings return to exact owners; terminal deletes nothing beyond verification.

---

# 10. Post-framework non-release convergence and future gates

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1736-1F7E486197E5

- Kind: `context`
- Source: `compiler-proposal.md:1736-1736`
- Applicability: `SCP7`
- Exact text SHA-256: `1f7e486197e5a3f051b39d9e1efba980a847a267b54b0cfe013b24df4827086d`

~~~~markdown
## `SCP7.md` — Svelte Default compiler product terminal
~~~~

### SRC-COMP-L1738-74C94AE59914

- Kind: `deletion`
- Source: `compiler-proposal.md:1738-1738`
- Applicability: `SCP7`
- Exact text SHA-256: `74c94ae59914bf0ef46d5384845b034a1844d34ed0bfd181f8dac6c2a56fcf86`

~~~~markdown
**Intent:** independently promote the Svelte V2 default compiler after cumulative correctness, maps, CSS, performance, memory and deletion proof.
~~~~

### SRC-COMP-L1740-0B133A4248B5

- Kind: `context`
- Source: `compiler-proposal.md:1740-1740`
- Applicability: `SCP7`
- Exact text SHA-256: `0b133a4248b55523ab7a6735dd533c668ea971f1cc7cec20b1027d9423a16783`

~~~~markdown
**Suggested predecessors:** `SCP6`, `CPER2`.
~~~~

### SRC-COMP-L1742-50E831C5E3C7

- Kind: `acceptance`
- Source: `compiler-proposal.md:1742-1742`
- Applicability: `SCP7`
- Exact text SHA-256: `50e831c5e3c7b90b2c42dbf8887ecfa4dd5e3e7522392bc15ab3a81e4f25681b`

~~~~markdown
**Required evidence:** exact `SCP0` matrix; client/server/module runtime and hydration; style scoping/pruning with no false negatives; maps; direct/prepared/managed and incremental/fresh equivalence; multi-target sharing; cold/warm/batch/RSS/cancellation; zero old compiler/matcher/session authorities; `Default = Supported`, `Optimized = FutureSeparateTrain`.
~~~~

### SRC-COMP-L1744-33441C3B8518

- Kind: `deletion`
- Source: `compiler-proposal.md:1744-1744`
- Applicability: `SCP7`
- Exact text SHA-256: `33441c3b851855acb9b11e10835bdd5ddce307d5693878c5e843dff468d1d19e`

~~~~markdown
**Acceptance:** all cells pass on one exact candidate and the experimental compiler is deleted.
~~~~

### SRC-COMP-L1746-033CF0B9A45F

- Kind: `forbidden`
- Source: `compiler-proposal.md:1746-1746`
- Applicability: `SCP7`
- Exact text SHA-256: `033cf0b9a45fb9a834ae5153153439051f920e1e1010911898df02216c20e027`

~~~~markdown
**Forbidden:** terminal implementation fixes, speed waivers, or enabling `Optimized`.
~~~~

### SRC-COMP-L1748-CF952B0C451B

- Kind: `deletion`
- Source: `compiler-proposal.md:1748-1748`
- Applicability: `SCP7`
- Exact text SHA-256: `cf952b0c451be7f38305763fe00103c0e382335d88214d2b8fe75e7d9fdf83cc`

~~~~markdown
**Deletion/abort:** findings return to exact owners; terminal deletes nothing beyond verification.
~~~~

### SRC-COMP-L1750-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1750-1750`
- Applicability: `SCP7`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
