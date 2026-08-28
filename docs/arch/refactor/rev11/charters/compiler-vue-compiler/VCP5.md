<!-- unified-charter-v2
id=VCP5
name=Vue Vapor Default compiler
phase=compiler
train=compiler.vue-compiler
product=vue_compiler
kind=implementation
semantic_role=delivery
class=compiler
predecessors=VCP2,VST0
conditional_predecessors=
owner=compiler.vue-compiler:Vue-owned Default compiler cells over shared compiler substrate
conflict_domains=compiler_execution,vue_product
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
release_gating=none
source_refs=source:compiler-proposal.md:L1340
external_requirements=
activation_gate=ORC0
charter=charters/compiler-vue-compiler/VCP5.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# VCP5 — Vue Vapor Default compiler

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Vue Vapor Default compiler. The current owner is **Vue runtime emitter and assembly paths**. The final and sole owner is **Vue-owned Default compiler cells over shared compiler substrate**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src/framework/vue`, `crates/verter_vue_conformance`, `packages/vue-conformance-oracle`.
- Named API/data boundaries: `VueSemanticSnapshot`, `VueCompilePlan`, `VueTarget`, `VueArtifactSet`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **VCP2:** exact current receipt ID and digest for “Compact Vue compiler structure and canonical template topology”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **VST0:** exact current receipt ID and digest for “Vue framework style semantics and scope plan”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** implement fine-grained Vue compilation using demanded dependency/effect relations rather than a mandatory reactivity AST.
- **Problem:** Vapor needs richer relationships than VDOM, but a whole second reactive tree would duplicate structure and impose work on other targets.
- **Solution and architecture decisions:**
- consume canonical reactivity/read/write/dependency facts;

## Acceptance IDs and discriminating proof

- **VCP5-AC1 — sole-owner proof:** add `vcp5_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **VCP5-AC2 — positive contract:** add `vcp5_publishes_exact_vuesemanticsnapshot`; assert exact identities, provenance, completeness, and deterministic ordering.
- **VCP5-AC3 — incremental equivalence:** add `vcp5_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **VCP5-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_vue_conformance/tests`, `packages/vue-conformance-oracle`.

## Deletions and forbidden designs

- Delete or structurally reject: **legacy Vue emitter route**.
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

1. `cargo nextest run -p verter_compiler -p verter_vue_conformance`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `semantic-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three fresh distinct harness reviewers with assigned lenses, deterministic low|medium|high effort, exact task/provider/model bindings, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1340`

## Reconciled source-plan contract

**Intent:** implement fine-grained Vue compilation using demanded dependency/effect relations rather than a mandatory reactivity AST.

**Problem:** Vapor needs richer relationships than VDOM, but a whole second reactive tree would duplicate structure and impose work on other targets.

**Solution and architecture decisions:**

- consume canonical reactivity/read/write/dependency facts;
- build only demanded dependency sets, effect groups, ordering edges and direct-DOM operations;
- index effect/operation ranges by stable Vue compiler identities;
- keep structure in `VCP2`, target state in sparse/graph arenas;
- use only `Default` component-local evidence; project-wide evidence waits for `OPT0`;
- emit through `CMP4` segmented artifacts/maps.

**Suggested predecessors:** `VCP2`, `VST0`.

**Normative source decomposition:** dependency graph, effect grouping, DOM operation planning, control-flow/region integration, emission/maps, conformance/performance.

**Acceptance:** no reactive AST copy exists; SSR/VDOM requests produce zero Vapor graph work; locked runtime semantics and maps pass; graph sizes/edges are ledger-visible and bounded.

**Forbidden:** project analysis, generic proof engine, target operations stored in shared semantic facts, or production speculative candidate comparison.

**Deletion/abort:** old Vapor path deleted at cutover only after full parity.

---

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1340-4A35BA674D42

- Kind: `context`
- Source: `compiler-proposal.md:1340-1340`
- Applicability: `VCP5`
- Exact text SHA-256: `4a35ba674d42b2b9e8dbef9ba613b5ff456c05366599016c6e2b95aa45964f7f`

~~~~markdown
## `VCP5.md` — Vue Vapor Default compiler
~~~~

### SRC-COMP-L1342-2DE2A6B7A251

- Kind: `context`
- Source: `compiler-proposal.md:1342-1342`
- Applicability: `VCP5`
- Exact text SHA-256: `2de2a6b7a2514e59c625c27e02a7b065d1b0cd3d974682da64957b2b91d560b6`

~~~~markdown
**Intent:** implement fine-grained Vue compilation using demanded dependency/effect relations rather than a mandatory reactivity AST.
~~~~

### SRC-COMP-L1344-E12D8166727F

- Kind: `context`
- Source: `compiler-proposal.md:1344-1344`
- Applicability: `VCP5`
- Exact text SHA-256: `e12d8166727fbc1d9fa5aa5165c3dd2800b99d5b0d08171bdeb531ad3fd2cfc3`

~~~~markdown
**Problem:** Vapor needs richer relationships than VDOM, but a whole second reactive tree would duplicate structure and impose work on other targets.
~~~~

### SRC-COMP-L1346-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:1346-1346`
- Applicability: `VCP5`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1348-5EF1E2488FBF

- Kind: `context`
- Source: `compiler-proposal.md:1348-1348`
- Applicability: `VCP5`
- Exact text SHA-256: `5ef1e2488fbfada699194715874237f8e34b7f09c05a072b0fcdbc6a95b75a12`

~~~~markdown
- consume canonical reactivity/read/write/dependency facts;
~~~~

### SRC-COMP-L1349-0DEBCB4EABC4

- Kind: `requirement`
- Source: `compiler-proposal.md:1349-1349`
- Applicability: `VCP5`
- Exact text SHA-256: `0debcb4eabc45551e84bfce075bd948eef19905081e8e80106042f61a43afbac`

~~~~markdown
- build only demanded dependency sets, effect groups, ordering edges and direct-DOM operations;
~~~~

### SRC-COMP-L1350-362AC3CA0964

- Kind: `context`
- Source: `compiler-proposal.md:1350-1350`
- Applicability: `VCP5`
- Exact text SHA-256: `362ac3ca09649b146159047d70a73a63b445e0342027b38786555d8f2387ca5a`

~~~~markdown
- index effect/operation ranges by stable Vue compiler identities;
~~~~

### SRC-COMP-L1351-F4252B717E31

- Kind: `context`
- Source: `compiler-proposal.md:1351-1351`
- Applicability: `VCP5`
- Exact text SHA-256: `f4252b717e3125ed647a27de7d575717aa1b0bc81385b356a834f06ae2c0ea49`

~~~~markdown
- keep structure in `VCP2`, target state in sparse/graph arenas;
~~~~

### SRC-COMP-L1352-6BEE8A9D8D35

- Kind: `requirement`
- Source: `compiler-proposal.md:1352-1352`
- Applicability: `VCP5`
- Exact text SHA-256: `6bee8a9d8d35630d6eb6f432370e43df2104b157f26972ca5559edd3622dce84`

~~~~markdown
- use only `Default` component-local evidence; project-wide evidence waits for `OPT0`;
~~~~

### SRC-COMP-L1353-05C1F2FCF800

- Kind: `context`
- Source: `compiler-proposal.md:1353-1353`
- Applicability: `VCP5`
- Exact text SHA-256: `05c1f2fcf8005734889e04baabf366f2ef129fe7df18beb91beefabaf24325fd`

~~~~markdown
- emit through `CMP4` segmented artifacts/maps.
~~~~

### SRC-COMP-L1355-1E4BF1D3A01D

- Kind: `context`
- Source: `compiler-proposal.md:1355-1355`
- Applicability: `VCP5`
- Exact text SHA-256: `1e4bf1d3a01d79737e78ee77dc804362fd1e31beac1e93e294a66d41aa363639`

~~~~markdown
**Suggested predecessors:** `VCP2`, `VST0`.
~~~~

### SRC-COMP-L1357-C2BCBEB3154B

- Kind: `context`
- Source: `compiler-proposal.md:1357-1357`
- Applicability: `VCP5`
- Exact text SHA-256: `c2bcbeb3154ba8b22a8dc2dd4a870135e8050c725e0d567bb46c73882ae7a7f1`

~~~~markdown
**Suggested subblocks:** dependency graph, effect grouping, DOM operation planning, control-flow/region integration, emission/maps, conformance/performance.
~~~~

### SRC-COMP-L1359-3C49BA2B7F41

- Kind: `acceptance`
- Source: `compiler-proposal.md:1359-1359`
- Applicability: `VCP5`
- Exact text SHA-256: `3c49ba2b7f411846a63eae87aa70f7ec38723ad4617e43e1e8bb02073d89bbf9`

~~~~markdown
**Acceptance:** no reactive AST copy exists; SSR/VDOM requests produce zero Vapor graph work; locked runtime semantics and maps pass; graph sizes/edges are ledger-visible and bounded.
~~~~

### SRC-COMP-L1361-55DFC7B3E911

- Kind: `forbidden`
- Source: `compiler-proposal.md:1361-1361`
- Applicability: `VCP5`
- Exact text SHA-256: `55dfc7b3e91135c6fe1f84b8603a8496895f8268e23f48e3cbf42db699ab8117`

~~~~markdown
**Forbidden:** project analysis, generic proof engine, target operations stored in shared semantic facts, or production speculative candidate comparison.
~~~~

### SRC-COMP-L1363-B5A6662D06C2

- Kind: `deletion`
- Source: `compiler-proposal.md:1363-1363`
- Applicability: `VCP5`
- Exact text SHA-256: `b5a6662d06c2448d869d78b3f8f3e57348e1a6bbacebc7cb4db49fa963294ada`

~~~~markdown
**Deletion/abort:** old Vapor path deleted at cutover only after full parity.
~~~~

### SRC-COMP-L1365-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1365-1365`
- Applicability: `VCP5`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
