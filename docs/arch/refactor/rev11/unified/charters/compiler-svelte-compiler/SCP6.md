<!-- unified-charter-v2
id=SCP6
name=Svelte assembly, artifacts, host integration, and atomic cutover
phase=compiler
train=compiler.svelte-compiler
product=svelte_compiler
kind=cutover
semantic_role=delivery
class=compiler
predecessors=SCP3,SCP4,SCP5,SST2
conditional_predecessors=
owner=compiler.svelte-compiler:Svelte-owned Default compiler cells over shared compiler substrate
conflict_domains=compiler_execution,host_service_graph,svelte_product
resource_class=rust-mixed
review_profile=public-3
gate_profile=targeted-domain
size=M
dispatchable=true
optional=false
release_gating=none
source_refs=source:compiler-proposal.md:L1709
external_requirements=
activation_gate=ORC0
charter=charters/compiler-svelte-compiler/SCP6.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# SCP6 — Svelte assembly, artifacts, host integration, and atomic cutover

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Svelte assembly, artifacts, host integration, and atomic cutover. The current owner is **Svelte runtime emitter and assembly paths**. The final and sole owner is **Svelte-owned Default compiler cells over shared compiler substrate**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src/framework/svelte`, `crates/verter_svelte_conformance`, `packages/svelte-runtime-tests`.
- Named API/data boundaries: `SvelteSemanticSnapshot`, `SvelteCompilePlan`, `SvelteTarget`, `SvelteArtifactSet`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **SCP3:** exact current receipt ID and digest for “Svelte client Default compiler”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **SCP4:** exact current receipt ID and digest for “Svelte server Default compiler”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **SCP5:** exact current receipt ID and digest for “Svelte module compiler for `.svelte.js` and `.svelte.ts`”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **SST2:** exact current receipt ID and digest for “Svelte style-match facts and adaptive matcher cutover”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** publish complete Svelte artifacts and remove framework semantics from generic session/host code.
- **Problem:** client/server/module/style outputs can remain separately assembled, and experimental/old paths may coexist.
- **Solution and architecture decisions:**
- assemble complete client/server/module artifacts inside the Svelte compiler;

## Acceptance IDs and discriminating proof

- **SCP6-AC1 — sole-owner proof:** add `scp6_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **SCP6-AC2 — positive contract:** add `scp6_publishes_exact_sveltesemanticsnapshot`; assert exact identities, provenance, completeness, and deterministic ordering.
- **SCP6-AC3 — incremental equivalence:** add `scp6_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **SCP6-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
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

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three named independent reviewers with assigned lenses, model `gpt-5.6-sol`, effort `ultra`, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1709`

## Reconciled source-plan contract

**Intent:** publish complete Svelte artifacts and remove framework semantics from generic session/host code.

**Problem:** client/server/module/style outputs can remain separately assembled, and experimental/old paths may coexist.

**Solution and architecture decisions:**

- assemble complete client/server/module artifacts inside the Svelte compiler;
- publish JS/CSS/maps/metadata through `CompileArtifactSet`;
- route CSS injection/extraction, HMR and virtual-module policy through framework-host integration;
- share client/server prerequisites and style facts;
- atomically cut direct/prepared/managed/public routes to V2;
- delete experimental compiler representations, style matcher routes, session assembly and temporary CCA adapters assigned to Svelte.

**Suggested predecessors:** `SCP3`, `SCP4`, `SCP5`, `SST2`.

**Normative source decomposition:** artifact assembly, style publication, host integration, multi-target orchestration, route cutover, deletion/rollback.

**Acceptance:** generic session contains no Svelte module topology; all compiler products are complete and map-qualified; one style-match fact product serves all targets; no old compiler authority remains reachable.

**Forbidden:** compatibility dual-running, host repair of incomplete semantics, native preprocessor, or fixed SFC artifact schema.

**Deletion/abort:** sole Svelte compiler cutover/deletion owner; abort on unexplained target/artifact/map divergence.

---

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1709-E6AD95E47C48

- Kind: `context`
- Source: `compiler-proposal.md:1709-1709`
- Applicability: `SCP6`
- Exact text SHA-256: `e6ad95e47c48a13b435f0047bf81a5dd36ae451bbe715b40e3156df50c242ad0`

~~~~markdown
## `SCP6.md` — Svelte assembly, artifacts, host integration, and atomic cutover
~~~~

### SRC-COMP-L1711-8962DD639DEA

- Kind: `deletion`
- Source: `compiler-proposal.md:1711-1711`
- Applicability: `SCP6`
- Exact text SHA-256: `8962dd639deae9ddde8f168c984e24be4d749db729aa94ad9a2d90765b4602cf`

~~~~markdown
**Intent:** publish complete Svelte artifacts and remove framework semantics from generic session/host code.
~~~~

### SRC-COMP-L1713-36EC84A71466

- Kind: `context`
- Source: `compiler-proposal.md:1713-1713`
- Applicability: `SCP6`
- Exact text SHA-256: `36ec84a714669a150ffa91350e0d6c4f4ed00fa2c92b7f6c44017b0ad5a7f947`

~~~~markdown
**Problem:** client/server/module/style outputs can remain separately assembled, and experimental/old paths may coexist.
~~~~

### SRC-COMP-L1715-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:1715-1715`
- Applicability: `SCP6`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1717-38846151AF93

- Kind: `context`
- Source: `compiler-proposal.md:1717-1717`
- Applicability: `SCP6`
- Exact text SHA-256: `38846151af93a9029abd2c353d307871daa796312eb1f2caa7a30ae09e9b4f2c`

~~~~markdown
- assemble complete client/server/module artifacts inside the Svelte compiler;
~~~~

### SRC-COMP-L1718-05497E18BDB8

- Kind: `context`
- Source: `compiler-proposal.md:1718-1718`
- Applicability: `SCP6`
- Exact text SHA-256: `05497e18bdb882fcda836aa3b0078af7067b754c2167972d093408a4ae0eab0d`

~~~~markdown
- publish JS/CSS/maps/metadata through `CompileArtifactSet`;
~~~~

### SRC-COMP-L1719-4EB36E25A6A7

- Kind: `context`
- Source: `compiler-proposal.md:1719-1719`
- Applicability: `SCP6`
- Exact text SHA-256: `4eb36e25a6a7e339d0db6eb8da20eca38a1b7898508f70f8b5492dc1f6cd214d`

~~~~markdown
- route CSS injection/extraction, HMR and virtual-module policy through framework-host integration;
~~~~

### SRC-COMP-L1720-EE68DD63B100

- Kind: `context`
- Source: `compiler-proposal.md:1720-1720`
- Applicability: `SCP6`
- Exact text SHA-256: `ee68dd63b1009d5cb3aa39c13988f681552a1a64335329590ba923d303af6121`

~~~~markdown
- share client/server prerequisites and style facts;
~~~~

### SRC-COMP-L1721-07B322271776

- Kind: `context`
- Source: `compiler-proposal.md:1721-1721`
- Applicability: `SCP6`
- Exact text SHA-256: `07b32227177618b9f8299d49e258c4c4868db25990539d4dfcf7511fabae933e`

~~~~markdown
- atomically cut direct/prepared/managed/public routes to V2;
~~~~

### SRC-COMP-L1722-9FB4B9AF4D59

- Kind: `deletion`
- Source: `compiler-proposal.md:1722-1722`
- Applicability: `SCP6`
- Exact text SHA-256: `9fb4b9af4d5959fdd17396dc42dcfb2c9320f5511e10928fc64103da0efd0584`

~~~~markdown
- delete experimental compiler representations, style matcher routes, session assembly and temporary CCA adapters assigned to Svelte.
~~~~

### SRC-COMP-L1724-DC694BD37802

- Kind: `context`
- Source: `compiler-proposal.md:1724-1724`
- Applicability: `SCP6`
- Exact text SHA-256: `dc694bd37802ce9a7635b1ef92674522373fa6f5be398f934f881e224c4d6fe5`

~~~~markdown
**Suggested predecessors:** `SCP3`, `SCP4`, `SCP5`, `SST2`.
~~~~

### SRC-COMP-L1726-782D53BC1528

- Kind: `deletion`
- Source: `compiler-proposal.md:1726-1726`
- Applicability: `SCP6`
- Exact text SHA-256: `782d53bc152819142739bd6b60264999ff0bc104d07abd129daea175b7d269f7`

~~~~markdown
**Suggested subblocks:** artifact assembly, style publication, host integration, multi-target orchestration, route cutover, deletion/rollback.
~~~~

### SRC-COMP-L1728-6FB773C9229B

- Kind: `acceptance`
- Source: `compiler-proposal.md:1728-1728`
- Applicability: `SCP6`
- Exact text SHA-256: `6fb773c9229bc08bce7b1ba95d163c19a9626d5a5139a45b27d810bef7cf2792`

~~~~markdown
**Acceptance:** generic session contains no Svelte module topology; all compiler products are complete and map-qualified; one style-match fact product serves all targets; no old compiler authority remains reachable.
~~~~

### SRC-COMP-L1730-3613C6105AB8

- Kind: `forbidden`
- Source: `compiler-proposal.md:1730-1730`
- Applicability: `SCP6`
- Exact text SHA-256: `3613c6105ab88dc70c05d65515bca5d14b8e0b234f81ecb15cbdac132a65159e`

~~~~markdown
**Forbidden:** compatibility dual-running, host repair of incomplete semantics, native preprocessor, or fixed SFC artifact schema.
~~~~

### SRC-COMP-L1732-D014EF862CCC

- Kind: `deletion`
- Source: `compiler-proposal.md:1732-1732`
- Applicability: `SCP6`
- Exact text SHA-256: `d014ef862ccc264f28441d308efa324c023160c4ddea280da8978ea7c26ac1d0`

~~~~markdown
**Deletion/abort:** sole Svelte compiler cutover/deletion owner; abort on unexplained target/artifact/map divergence.
~~~~

### SRC-COMP-L1734-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1734-1734`
- Applicability: `SCP6`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
