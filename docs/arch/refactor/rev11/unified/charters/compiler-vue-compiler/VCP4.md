<!-- unified-charter-v2
id=VCP4
name=Vue SSR Default compiler
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
source_refs=source:compiler-proposal.md:L1314
external_requirements=
activation_gate=ORC0
charter=charters/compiler-vue-compiler/VCP4.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# VCP4 — Vue SSR Default compiler

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Vue SSR Default compiler. The current owner is **Vue runtime emitter and assembly paths**. The final and sole owner is **Vue-owned Default compiler cells over shared compiler substrate**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src/framework/vue`, `crates/verter_vue_conformance`, `packages/vue-conformance-oracle`.
- Named API/data boundaries: `VueSemanticSnapshot`, `VueCompilePlan`, `VueTarget`, `VueArtifactSet`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **VCP2:** exact current receipt ID and digest for “Compact Vue compiler structure and canonical template topology”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **VST0:** exact current receipt ID and digest for “Vue framework style semantics and scope plan”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** implement server compilation as a distinct target that shares prerequisites but performs zero client-effect planning.
- **Problem:** server targets can accidentally inherit client/Vapor structures and unnecessary target materialization.
- **Solution and architecture decisions:**
- monomorphic Vue+SSR executor;

## Acceptance IDs and discriminating proof

- **VCP4-AC1 — sole-owner proof:** add `vcp4_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **VCP4-AC2 — positive contract:** add `vcp4_publishes_exact_vuesemanticsnapshot`; assert exact identities, provenance, completeness, and deterministic ordering.
- **VCP4-AC3 — incremental equivalence:** add `vcp4_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **VCP4-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
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

- `source:compiler-proposal.md:L1314`

## Reconciled source-plan contract

**Intent:** implement server compilation as a distinct target that shares prerequisites but performs zero client-effect planning.

**Problem:** server targets can accidentally inherit client/Vapor structures and unnecessary target materialization.

**Solution and architecture decisions:**

- monomorphic Vue+SSR executor;
- consume structural regions, escaping/staticness facts and style/scope relations;
- segment-oriented server emission; materialize an SSR plan only where it avoids rediscovery;
- zero VDOM patch planning, zero Vapor dependency/effect graph, zero VST1 query work;
- share parse/semantic/structure with VDOM/Vapor in multi-target requests.

**Suggested predecessors:** `VCP2`, `VST0`.

**Normative source decomposition:** text/escaping/static segments, elements/components/slots, control flow, SSR helpers/module surface, maps, multi-target/performance proof.

**Acceptance:** locked SSR behavior/maps pass; client-plan counters are zero; VDOM+SSR shares prerequisites and branches at the locked point; output remains deterministic across direct/prepared/managed paths.

**Forbidden:** reusing client target state merely for symmetry, client effect graph, or whole-tree server IR without measured need.

**Deletion/abort:** old SSR path deleted at framework cutover after parity.

---

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1314-F937B7DDA893

- Kind: `context`
- Source: `compiler-proposal.md:1314-1314`
- Applicability: `VCP4`
- Exact text SHA-256: `f937b7dda893f3ae26d6e966fa734bb2f32e345220b0b3a8bbeb7cecfee0e767`

~~~~markdown
## `VCP4.md` — Vue SSR Default compiler
~~~~

### SRC-COMP-L1316-E21DBCA2CACE

- Kind: `context`
- Source: `compiler-proposal.md:1316-1316`
- Applicability: `VCP4`
- Exact text SHA-256: `e21dbca2cace388a608b7d7d093f31384224decba9aef15d722c14c9c198e81e`

~~~~markdown
**Intent:** implement server compilation as a distinct target that shares prerequisites but performs zero client-effect planning.
~~~~

### SRC-COMP-L1318-D8663C045D34

- Kind: `context`
- Source: `compiler-proposal.md:1318-1318`
- Applicability: `VCP4`
- Exact text SHA-256: `d8663c045d34a90625557d6d17fcb3de3c6dc07fcc168a47b01ce48e4f1a4403`

~~~~markdown
**Problem:** server targets can accidentally inherit client/Vapor structures and unnecessary target materialization.
~~~~

### SRC-COMP-L1320-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:1320-1320`
- Applicability: `VCP4`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1322-65F60B27FE50

- Kind: `context`
- Source: `compiler-proposal.md:1322-1322`
- Applicability: `VCP4`
- Exact text SHA-256: `65f60b27fe50aae74e14fa0de85a071e743b890fdd488d7a85e4627666efd6d5`

~~~~markdown
- monomorphic Vue+SSR executor;
~~~~

### SRC-COMP-L1323-284B6038B5D2

- Kind: `context`
- Source: `compiler-proposal.md:1323-1323`
- Applicability: `VCP4`
- Exact text SHA-256: `284b6038b5d277475b5ef7591e98b198da4aa031ca1a806d536251ccefc42e6c`

~~~~markdown
- consume structural regions, escaping/staticness facts and style/scope relations;
~~~~

### SRC-COMP-L1324-CFE720820194

- Kind: `requirement`
- Source: `compiler-proposal.md:1324-1324`
- Applicability: `VCP4`
- Exact text SHA-256: `cfe720820194995011b8afc45ea04122c3fd2961e02ae930ac4f83cefdd58030`

~~~~markdown
- segment-oriented server emission; materialize an SSR plan only where it avoids rediscovery;
~~~~

### SRC-COMP-L1325-E7DB9F09040E

- Kind: `context`
- Source: `compiler-proposal.md:1325-1325`
- Applicability: `VCP4`
- Exact text SHA-256: `e7db9f09040efc0335dd7fd4b266a5a316f5c6233acbb33435d5cb4f3a4a75d0`

~~~~markdown
- zero VDOM patch planning, zero Vapor dependency/effect graph, zero VST1 query work;
~~~~

### SRC-COMP-L1326-F07C076B5CD0

- Kind: `context`
- Source: `compiler-proposal.md:1326-1326`
- Applicability: `VCP4`
- Exact text SHA-256: `f07c076b5cd0a43bf43705c93848d02a0dab26208fb9770eb09e330a9aae018c`

~~~~markdown
- share parse/semantic/structure with VDOM/Vapor in multi-target requests.
~~~~

### SRC-COMP-L1328-1E4BF1D3A01D

- Kind: `context`
- Source: `compiler-proposal.md:1328-1328`
- Applicability: `VCP4`
- Exact text SHA-256: `1e4bf1d3a01d79737e78ee77dc804362fd1e31beac1e93e294a66d41aa363639`

~~~~markdown
**Suggested predecessors:** `VCP2`, `VST0`.
~~~~

### SRC-COMP-L1330-B94AEBF49DAF

- Kind: `context`
- Source: `compiler-proposal.md:1330-1330`
- Applicability: `VCP4`
- Exact text SHA-256: `b94aebf49dafe16299fe2a47e75263326dc89404b5dab4cc8abc2c7d08c18fa0`

~~~~markdown
**Suggested subblocks:** text/escaping/static segments, elements/components/slots, control flow, SSR helpers/module surface, maps, multi-target/performance proof.
~~~~

### SRC-COMP-L1332-894F02A9EF9D

- Kind: `acceptance`
- Source: `compiler-proposal.md:1332-1332`
- Applicability: `VCP4`
- Exact text SHA-256: `894f02a9ef9d56e1d9a8a0fdff983e3f7f0de7a3fca2b7f2a162cb3cdbeeee88`

~~~~markdown
**Acceptance:** locked SSR behavior/maps pass; client-plan counters are zero; VDOM+SSR shares prerequisites and branches at the locked point; output remains deterministic across direct/prepared/managed paths.
~~~~

### SRC-COMP-L1334-D22C4920A312

- Kind: `forbidden`
- Source: `compiler-proposal.md:1334-1334`
- Applicability: `VCP4`
- Exact text SHA-256: `d22c4920a31229faf9ec753b9a81bd60825394810c223c0156d66839212215f8`

~~~~markdown
**Forbidden:** reusing client target state merely for symmetry, client effect graph, or whole-tree server IR without measured need.
~~~~

### SRC-COMP-L1336-B51294648A2F

- Kind: `deletion`
- Source: `compiler-proposal.md:1336-1336`
- Applicability: `VCP4`
- Exact text SHA-256: `b51294648a2f9049196903cdd7b205f4ed32f2e83d2b7d098e19e7c1e24596e6`

~~~~markdown
**Deletion/abort:** old SSR path deleted at framework cutover after parity.
~~~~

### SRC-COMP-L1338-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1338-1338`
- Applicability: `VCP4`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
