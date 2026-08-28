<!-- unified-charter-v2
id=SCP3
name=Svelte client Default compiler
phase=compiler
train=compiler.svelte-compiler
product=svelte_compiler
kind=implementation
semantic_role=delivery
class=compiler
predecessors=SCP2,SST2
conditional_predecessors=
owner=compiler.svelte-compiler:Svelte-owned Default compiler cells over shared compiler substrate
conflict_domains=cli_application,compiler_execution,svelte_product
resource_class=rust-mixed
review_profile=public-3
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
source_refs=source:compiler-proposal.md:L1624
external_requirements=
activation_gate=ORC0
charter=charters/compiler-svelte-compiler/SCP3.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# SCP3 — Svelte client Default compiler

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Svelte client Default compiler. The current owner is **Svelte runtime emitter and assembly paths**. The final and sole owner is **Svelte-owned Default compiler cells over shared compiler substrate**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src/framework/svelte`, `crates/verter_svelte_conformance`, `packages/svelte-runtime-tests`.
- Named API/data boundaries: `SvelteSemanticSnapshot`, `SvelteCompilePlan`, `SvelteTarget`, `SvelteArtifactSet`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **SCP2:** exact current receipt ID and digest for “Compact Svelte compiler structure and canonical template topology”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **SST2:** exact current receipt ID and digest for “Svelte style-match facts and adaptive matcher cutover”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** implement client compilation from canonical semantics, topology, and style facts using demanded dependency/effect relations.
- **Problem:** transform/code generation can rediscover semantics, build multiple intermediate forms, and allocate broadly distributed object state.
- **Solution and architecture decisions:**
- monomorphic Svelte+client executor;

## Acceptance IDs and discriminating proof

- **SCP3-AC1 — sole-owner proof:** add `scp3_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **SCP3-AC2 — positive contract:** add `scp3_publishes_exact_sveltesemanticsnapshot`; assert exact identities, provenance, completeness, and deterministic ordering.
- **SCP3-AC3 — incremental equivalence:** add `scp3_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **SCP3-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_svelte_conformance/tests`, `packages/svelte-runtime-tests/test`.

## Deletions and forbidden designs

- Delete or structurally reject: **legacy Svelte emitter route**.
- Delete or structurally reject: **per-target prerequisite duplication**.
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

1. `cargo nextest run -p verter_compiler -p verter_svelte_conformance`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `public-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three fresh distinct harness reviewers with assigned lenses, deterministic low|medium|high effort, exact task/provider/model bindings, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1624`

## Reconciled source-plan contract

**Intent:** implement client compilation from canonical semantics, topology, and style facts using demanded dependency/effect relations.

**Problem:** transform/code generation can rediscover semantics, build multiple intermediate forms, and allocate broadly distributed object state.

**Solution and architecture decisions:**

- monomorphic Svelte+client executor;
- demand-only dependency sets, effects, DOM operations, hydration, bindings, actions, transitions and animations;
- sparse/graph target state indexed by Svelte compiler identities;
- consume `SST2` match/scope facts once;
- segmented emission and no-map specialization;
- no server plan or module compiler work.

**Suggested predecessors:** `SCP2`, `SST2`.

**Normative source decomposition:** static skeleton/DOM plan, reactive dependency/effects, blocks/snippets/components, directives/runtime operations, hydration, emission/maps/conformance.

**Acceptance:** locked client runtime/hydration/CSS/maps pass; no raw-source structural decisions; no duplicated style matching; target graph sizes and visits meet budgets.

**Forbidden:** source-text transform heuristics, full reactive AST, server target state, or universal target operations.

**Deletion/abort:** old client path deleted only at `SCP6` after parity.

---

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1624-3FABEB11D004

- Kind: `context`
- Source: `compiler-proposal.md:1624-1624`
- Applicability: `SCP3`
- Exact text SHA-256: `3fabeb11d004e5801fb32c15dd8d8ce1e24560eb7fec26a30682ca78739ea089`

~~~~markdown
## `SCP3.md` — Svelte client Default compiler
~~~~

### SRC-COMP-L1626-93DA131910DB

- Kind: `context`
- Source: `compiler-proposal.md:1626-1626`
- Applicability: `SCP3`
- Exact text SHA-256: `93da131910db6571e9f0b969335b2ba1d5b7e83c2941b795b14147844f819c6b`

~~~~markdown
**Intent:** implement client compilation from canonical semantics, topology, and style facts using demanded dependency/effect relations.
~~~~

### SRC-COMP-L1628-5DF21F6D9AFF

- Kind: `context`
- Source: `compiler-proposal.md:1628-1628`
- Applicability: `SCP3`
- Exact text SHA-256: `5df21f6d9aff1d4428f56789dc49334b57417ad7e3883f545ce6fa7478301637`

~~~~markdown
**Problem:** transform/code generation can rediscover semantics, build multiple intermediate forms, and allocate broadly distributed object state.
~~~~

### SRC-COMP-L1630-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:1630-1630`
- Applicability: `SCP3`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1632-189F91E7AD4D

- Kind: `context`
- Source: `compiler-proposal.md:1632-1632`
- Applicability: `SCP3`
- Exact text SHA-256: `189f91e7ad4d8c6d41ec7c94334c38a9572e970e7b17c8d7eb28cc134a0c453c`

~~~~markdown
- monomorphic Svelte+client executor;
~~~~

### SRC-COMP-L1633-3D9F68364AD3

- Kind: `requirement`
- Source: `compiler-proposal.md:1633-1633`
- Applicability: `SCP3`
- Exact text SHA-256: `3d9f68364ad337ce5d8ca5d69e9dcd3a2fe05b46b1e37d8b7868dc64e5031430`

~~~~markdown
- demand-only dependency sets, effects, DOM operations, hydration, bindings, actions, transitions and animations;
~~~~

### SRC-COMP-L1634-3E155B8B7B57

- Kind: `context`
- Source: `compiler-proposal.md:1634-1634`
- Applicability: `SCP3`
- Exact text SHA-256: `3e155b8b7b57017896a14993a1924cd46f8b8cd70dc5a6e23cc95235372f44eb`

~~~~markdown
- sparse/graph target state indexed by Svelte compiler identities;
~~~~

### SRC-COMP-L1635-2FBE8FF83A2A

- Kind: `context`
- Source: `compiler-proposal.md:1635-1635`
- Applicability: `SCP3`
- Exact text SHA-256: `2fbe8ff83a2a93a7e28e13589afb9a1f0504c2ab252fe395c85bf7d44ed3fbd3`

~~~~markdown
- consume `SST2` match/scope facts once;
~~~~

### SRC-COMP-L1636-89AA798AB514

- Kind: `context`
- Source: `compiler-proposal.md:1636-1636`
- Applicability: `SCP3`
- Exact text SHA-256: `89aa798ab5143c3a563d153ca99beea2f0214435433f2d4be36b4f550810286c`

~~~~markdown
- segmented emission and no-map specialization;
~~~~

### SRC-COMP-L1637-1CCD8BCE09E3

- Kind: `context`
- Source: `compiler-proposal.md:1637-1637`
- Applicability: `SCP3`
- Exact text SHA-256: `1ccd8bce09e3258b817d19fa6ccbb635d8a12a134823ccff84727dcbc98682bf`

~~~~markdown
- no server plan or module compiler work.
~~~~

### SRC-COMP-L1639-E5ED001D2593

- Kind: `context`
- Source: `compiler-proposal.md:1639-1639`
- Applicability: `SCP3`
- Exact text SHA-256: `e5ed001d25934b7b6527d41b3a63d1dad6b3836f3f80f5fcb65503d87eca6f57`

~~~~markdown
**Suggested predecessors:** `SCP2`, `SST2`.
~~~~

### SRC-COMP-L1641-146CE25935F6

- Kind: `context`
- Source: `compiler-proposal.md:1641-1641`
- Applicability: `SCP3`
- Exact text SHA-256: `146ce25935f6220f634079ab331a7bc0b6bb3904217f43e0b53515b8858e69e1`

~~~~markdown
**Suggested subblocks:** static skeleton/DOM plan, reactive dependency/effects, blocks/snippets/components, directives/runtime operations, hydration, emission/maps/conformance.
~~~~

### SRC-COMP-L1643-F19916D3BDBD

- Kind: `acceptance`
- Source: `compiler-proposal.md:1643-1643`
- Applicability: `SCP3`
- Exact text SHA-256: `f19916d3bdbdfd0f02f5f8495dee14d92c199ddb56fc15932be5ec400c4e7359`

~~~~markdown
**Acceptance:** locked client runtime/hydration/CSS/maps pass; no raw-source structural decisions; no duplicated style matching; target graph sizes and visits meet budgets.
~~~~

### SRC-COMP-L1645-33116DA99520

- Kind: `forbidden`
- Source: `compiler-proposal.md:1645-1645`
- Applicability: `SCP3`
- Exact text SHA-256: `33116da99520d1dd6b5aef1ef93b71f6c3917cda51b445347c8d6dcd1b3acd78`

~~~~markdown
**Forbidden:** source-text transform heuristics, full reactive AST, server target state, or universal target operations.
~~~~

### SRC-COMP-L1647-FE5CE18F8C3D

- Kind: `deletion`
- Source: `compiler-proposal.md:1647-1647`
- Applicability: `SCP3`
- Exact text SHA-256: `fe5ce18f8c3d5fccfef49be5d7714938b908421d67f02f005266330af336e0f5`

~~~~markdown
**Deletion/abort:** old client path deleted only at `SCP6` after parity.
~~~~

### SRC-COMP-L1649-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1649-1649`
- Applicability: `SCP3`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
