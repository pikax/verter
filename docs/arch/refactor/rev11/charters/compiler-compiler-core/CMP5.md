<!-- unified-charter-v2
id=CMP5
name=Provisional shared compiler-core contract lock
phase=compiler
train=compiler.compiler-core
product=compiler_core
kind=convergence
semantic_role=convergence
class=compiler
predecessors=CMP4,CPER2
conditional_predecessors=
owner=compiler.compiler-core:data-oriented common compiler substrate with framework-native planning
conflict_domains=compiler_execution
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
release_gating=contract
source_refs=source:compiler-proposal.md:L1123
external_requirements=
activation_gate=ORC0
charter=charters/compiler-compiler-core/CMP5.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# CMP5 — Provisional shared compiler-core contract lock

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Provisional shared compiler-core contract lock. The current owner is **framework compiler emitters and per-node target dispatch**. The final and sole owner is **data-oriented common compiler substrate with framework-native planning**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src`, `crates/verter_semantic/src`.
- Named API/data boundaries: `CompileRequest`, `CompilerPolicy`, `DemandSet`, `RegionId`, `EmissionSegment`, `ArtifactQualifier`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **CMP4:** exact current receipt ID and digest for “Segmented emission, qualified artifacts, assembly, and host integration”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **CPER2:** exact current receipt ID and digest for “Shared compiler physical-execution and zero-work terminal”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** ratify the common compiler machinery as ready for independent framework implementations without claiming universal compiler semantics.
- **Problem:** framework trains need a stable substrate, but the substrate must remain falsifiable and must not become a release join for unrelated tooling.
- **Solution and architecture decisions:** read-only convergence over CMP0–CMP4 and CPER2, including dependency firewalls and shared-mechanics-only review.
- **Suggested predecessors:** CMP4, CPER2.

## Acceptance IDs and discriminating proof

- **CMP5-AC1 — sole-owner proof:** add `cmp5_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **CMP5-AC2 — positive contract:** add `cmp5_publishes_exact_compilerequest`; assert exact identities, provenance, completeness, and deterministic ordering.
- **CMP5-AC3 — incremental equivalence:** add `cmp5_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **CMP5-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_compiler/tests`, `crates/verter_vue_conformance/tests`, `crates/verter_svelte_conformance/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **dynamic dispatch inside node loops**.
- Delete or structurally reject: **whole-tree materialization fallback**.
- Delete or structurally reject: **unqualified artifact assembly**.
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

1. `cargo nextest run -p verter_compiler -p verter_vue_conformance -p verter_svelte_conformance`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `architecture-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three fresh distinct harness reviewers with assigned lenses, deterministic low|medium|high effort, exact task/provider/model bindings, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1123`

## Reconciled source-plan contract

**Intent:** ratify the common compiler machinery as ready for independent framework implementations without claiming universal compiler semantics.

**Problem:** framework trains need a stable substrate, but the substrate must remain falsifiable and must not become a release join for unrelated tooling.

**Solution and architecture decisions:** read-only convergence over `CMP0`–`CMP4` and `CPER2`, including dependency firewalls and shared-mechanics-only review.

**Suggested predecessors:** `CMP4`, `CPER2`.

**Normative source decomposition:** authority graph review, data-layout review, demand/zero-work review, artifact/map review, framework-leakage adversarial fixtures, exact-digest ratification.

**Acceptance:** Vue and Svelte implementation locks can be written without changing common authority boundaries; no shared type contains framework semantics; compiler core remains optional to tooling verticals.

**Forbidden:** implementing framework behavior, promoting a universal IR, or making future compiler support implicit from tooling support.

**Deletion/abort:** findings reopen the smallest common owner; this block deletes nothing.

---

# 8. Vue Default compiler train

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1123-1E7A2A9BC572

- Kind: `context`
- Source: `compiler-proposal.md:1123-1123`
- Applicability: `CMP5`
- Exact text SHA-256: `1e7a2a9bc5724414cf605e3fbbad8ebc80a06efb9ec8d32456a37d0fa27302eb`

~~~~markdown
## `CMP5.md` — Provisional shared compiler-core contract lock
~~~~

### SRC-COMP-L1125-947D7B6172FB

- Kind: `context`
- Source: `compiler-proposal.md:1125-1125`
- Applicability: `CMP5`
- Exact text SHA-256: `947d7b6172fb0c6daa316c62f66345c027e9ecfef45ae22086dfe0ef34da0892`

~~~~markdown
**Intent:** ratify the common compiler machinery as ready for independent framework implementations without claiming universal compiler semantics.
~~~~

### SRC-COMP-L1127-1685BD20D6DD

- Kind: `forbidden`
- Source: `compiler-proposal.md:1127-1127`
- Applicability: `CMP5`
- Exact text SHA-256: `1685bd20d6dd8f12e93758e8970418e7d6442ffeee9feef34b9509fa21c8bb11`

~~~~markdown
**Problem:** framework trains need a stable substrate, but the substrate must remain falsifiable and must not become a release join for unrelated tooling.
~~~~

### SRC-COMP-L1129-44ED2E9B169E

- Kind: `requirement`
- Source: `compiler-proposal.md:1129-1129`
- Applicability: `CMP5`
- Exact text SHA-256: `44ed2e9b169e1a500556f7cd3393814bd880688ffbd5722eef24342c2c844a57`

~~~~markdown
**Solution and architecture decisions:** read-only convergence over `CMP0`–`CMP4` and `CPER2`, including dependency firewalls and shared-mechanics-only review.
~~~~

### SRC-COMP-L1131-3EC53651DC33

- Kind: `context`
- Source: `compiler-proposal.md:1131-1131`
- Applicability: `CMP5`
- Exact text SHA-256: `3ec53651dc33e7604f863419df3418a1a068c1cb597891666c29698b3e4a07bb`

~~~~markdown
**Suggested predecessors:** `CMP4`, `CPER2`.
~~~~

### SRC-COMP-L1133-415EEDFE70FC

- Kind: `context`
- Source: `compiler-proposal.md:1133-1133`
- Applicability: `CMP5`
- Exact text SHA-256: `415eedfe70fc38646dd5168b09b078d497cbd20e062b52a33d187ef626a1b078`

~~~~markdown
**Suggested subblocks:** authority graph review, data-layout review, demand/zero-work review, artifact/map review, framework-leakage adversarial fixtures, exact-digest ratification.
~~~~

### SRC-COMP-L1135-328E38A6E61B

- Kind: `acceptance`
- Source: `compiler-proposal.md:1135-1135`
- Applicability: `CMP5`
- Exact text SHA-256: `328e38a6e61bb0ac37e80cd791f006a2c6528ce6586d8e0b3fa83c85aed562bb`

~~~~markdown
**Acceptance:** Vue and Svelte implementation locks can be written without changing common authority boundaries; no shared type contains framework semantics; compiler core remains optional to tooling verticals.
~~~~

### SRC-COMP-L1137-6355ED2D6373

- Kind: `forbidden`
- Source: `compiler-proposal.md:1137-1137`
- Applicability: `CMP5`
- Exact text SHA-256: `6355ed2d63738e54f6a35da0a93769abba38e4e93558763b70e09702b9cd2387`

~~~~markdown
**Forbidden:** implementing framework behavior, promoting a universal IR, or making future compiler support implicit from tooling support.
~~~~

### SRC-COMP-L1139-83E805F3C3D4

- Kind: `deletion`
- Source: `compiler-proposal.md:1139-1139`
- Applicability: `CMP5`
- Exact text SHA-256: `83e805f3c3d4bc84cdae368bb9c829ced106887c9334075bb70340a00ee70f3d`

~~~~markdown
**Deletion/abort:** findings reopen the smallest common owner; this block deletes nothing.
~~~~

### SRC-COMP-L1141-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1141-1141`
- Applicability: `CMP5`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
