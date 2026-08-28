<!-- unified-charter-v2
id=VCP6
name=Vue module assembly, artifacts, host integration, and atomic cutover
phase=compiler
train=compiler.vue-compiler
product=vue_compiler
kind=cutover
semantic_role=delivery
class=compiler
predecessors=VCP3,VCP4,VCP5,VST0
conditional_predecessors=
owner=compiler.vue-compiler:Vue-owned Default compiler cells over shared compiler substrate
conflict_domains=compiler_execution,host_service_graph,vue_product
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
source_refs=source:compiler-proposal.md:L1367
external_requirements=
activation_gate=ORC0
charter=charters/compiler-vue-compiler/VCP6.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# VCP6 — Vue module assembly, artifacts, host integration, and atomic cutover

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Vue module assembly, artifacts, host integration, and atomic cutover. The current owner is **Vue runtime emitter and assembly paths**. The final and sole owner is **Vue-owned Default compiler cells over shared compiler substrate**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src/framework/vue`, `crates/verter_vue_conformance`, `packages/vue-conformance-oracle`.
- Named API/data boundaries: `VueSemanticSnapshot`, `VueCompilePlan`, `VueTarget`, `VueArtifactSet`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **VCP3:** exact current receipt ID and digest for “Vue VDOM Default compiler”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **VCP4:** exact current receipt ID and digest for “Vue SSR Default compiler”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **VCP5:** exact current receipt ID and digest for “Vue Vapor Default compiler”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **VST0:** exact current receipt ID and digest for “Vue framework style semantics and scope plan”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** make the Vue compiler produce complete framework artifacts and remove Vue semantics from generic session/host code.
- **Problem:** target outputs can remain fragments requiring session-level assembly, style/custom-block handling can be ambiguous, and old/new target routes can coexist.
- **Solution and architecture decisions:**
- assemble the complete Vue framework module inside the Vue runtime compiler;

## Acceptance IDs and discriminating proof

- **VCP6-AC1 — sole-owner proof:** add `vcp6_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **VCP6-AC2 — positive contract:** add `vcp6_publishes_exact_vuesemanticsnapshot`; assert exact identities, provenance, completeness, and deterministic ordering.
- **VCP6-AC3 — incremental equivalence:** add `vcp6_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **VCP6-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
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

Apply `public-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three fresh distinct harness reviewers with assigned lenses, deterministic low|medium|high effort, exact task/provider/model bindings, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1367`

## Reconciled source-plan contract

**Intent:** make the Vue compiler produce complete framework artifacts and remove Vue semantics from generic session/host code.

**Problem:** target outputs can remain fragments requiring session-level assembly, style/custom-block handling can be ambiguous, and old/new target routes can coexist.

**Solution and architecture decisions:**

- assemble the complete Vue framework module inside the Vue runtime compiler;
- publish JS/CSS/maps/metadata/opaque custom-block attachments through `CompileArtifactSet`;
- route framework-host behavior through the exact `FrameworkHostIntegrationBackend`;
- compose VDOM/SSR/Vapor multi-target requests from shared prerequisites;
- preserve custom blocks as descriptors/attachments only;
- atomically route public/direct/prepared/managed compiler entry points to V2;
- delete old Vue target walkers, session assembly, mixed outputs and temporary CCA adapters assigned to Vue.

**Suggested predecessors:** `VCP3`, `VCP4`, `VCP5`, `VST0`.

**Normative source decomposition:** framework assembly, style/CSS artifacts, host adapters, custom-block opaque publication, route cutover, deletion and rollback.

**Acceptance:** generic session has no Vue module topology; all targets/maps/artifacts are complete; old and new paths never remain simultaneously authoritative; custom blocks are preserved without execution; host integrations cannot repair semantic output.

**Forbidden:** dynamic custom-block ABI, generic session assembly, hidden CSS pipeline, or per-host compiler semantics.

**Deletion/abort:** this is the sole Vue cutover/deletion owner; abort on any unexplained target/artifact/map divergence.

---

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1367-8B2CDC66F2A2

- Kind: `context`
- Source: `compiler-proposal.md:1367-1367`
- Applicability: `VCP6`
- Exact text SHA-256: `8b2cdc66f2a2d56514ebcd7a0c2021d068d96d6c8d8ed9cfc10adc7f5bdf1407`

~~~~markdown
## `VCP6.md` — Vue module assembly, artifacts, host integration, and atomic cutover
~~~~

### SRC-COMP-L1369-1C06E1D68D26

- Kind: `deletion`
- Source: `compiler-proposal.md:1369-1369`
- Applicability: `VCP6`
- Exact text SHA-256: `1c06e1d68d263bf060e5fbe934005c02a0b105dcc7be50ae40fe52b4f8040713`

~~~~markdown
**Intent:** make the Vue compiler produce complete framework artifacts and remove Vue semantics from generic session/host code.
~~~~

### SRC-COMP-L1371-FCDA4AE917A1

- Kind: `context`
- Source: `compiler-proposal.md:1371-1371`
- Applicability: `VCP6`
- Exact text SHA-256: `fcda4ae917a16d0ea6947038487a469dcb66015447c960d80156aba6cfffc4fb`

~~~~markdown
**Problem:** target outputs can remain fragments requiring session-level assembly, style/custom-block handling can be ambiguous, and old/new target routes can coexist.
~~~~

### SRC-COMP-L1373-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:1373-1373`
- Applicability: `VCP6`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1375-CA09C8639A34

- Kind: `context`
- Source: `compiler-proposal.md:1375-1375`
- Applicability: `VCP6`
- Exact text SHA-256: `ca09c8639a342f0eb44e9d0bb62803f0f8d7f01183b73fd069685fc1ad847ffc`

~~~~markdown
- assemble the complete Vue framework module inside the Vue runtime compiler;
~~~~

### SRC-COMP-L1376-C66D38BA22D2

- Kind: `context`
- Source: `compiler-proposal.md:1376-1376`
- Applicability: `VCP6`
- Exact text SHA-256: `c66d38ba22d219261fbf2ab6ccade6c14b8731a49dd89666dc27f8a8deb45c4c`

~~~~markdown
- publish JS/CSS/maps/metadata/opaque custom-block attachments through `CompileArtifactSet`;
~~~~

### SRC-COMP-L1377-F378FA90EA59

- Kind: `requirement`
- Source: `compiler-proposal.md:1377-1377`
- Applicability: `VCP6`
- Exact text SHA-256: `f378fa90ea598a9664dd1134b5539d7b5f91b3d100cbfffdb17e1b5a1facb397`

~~~~markdown
- route framework-host behavior through the exact `FrameworkHostIntegrationBackend`;
~~~~

### SRC-COMP-L1378-E835AD672882

- Kind: `context`
- Source: `compiler-proposal.md:1378-1378`
- Applicability: `VCP6`
- Exact text SHA-256: `e835ad672882b8a901550f749fc3d00ed29d17884f240be735eb6c20e50e3398`

~~~~markdown
- compose VDOM/SSR/Vapor multi-target requests from shared prerequisites;
~~~~

### SRC-COMP-L1379-E383F36FF8E7

- Kind: `requirement`
- Source: `compiler-proposal.md:1379-1379`
- Applicability: `VCP6`
- Exact text SHA-256: `e383f36ff8e76c6b7e04d8c51aa18ce34ab8b250c5601d04c1b715bed9225c06`

~~~~markdown
- preserve custom blocks as descriptors/attachments only;
~~~~

### SRC-COMP-L1380-B1B1900DD213

- Kind: `context`
- Source: `compiler-proposal.md:1380-1380`
- Applicability: `VCP6`
- Exact text SHA-256: `b1b1900dd213ef40d303c02287ab38cb6840d054ab2f4a7c28c0d957f3301bcd`

~~~~markdown
- atomically route public/direct/prepared/managed compiler entry points to V2;
~~~~

### SRC-COMP-L1381-6FF766EEECA1

- Kind: `deletion`
- Source: `compiler-proposal.md:1381-1381`
- Applicability: `VCP6`
- Exact text SHA-256: `6ff766eeeca1aa945b61280783cea3809c410335cf7e95b99e4fbff30da1a1e0`

~~~~markdown
- delete old Vue target walkers, session assembly, mixed outputs and temporary CCA adapters assigned to Vue.
~~~~

### SRC-COMP-L1383-1C4EFA89FD8E

- Kind: `context`
- Source: `compiler-proposal.md:1383-1383`
- Applicability: `VCP6`
- Exact text SHA-256: `1c4efa89fd8e1c01d08fbc2670460814368d375ad3fdf9221dde40c8981af934`

~~~~markdown
**Suggested predecessors:** `VCP3`, `VCP4`, `VCP5`, `VST0`.
~~~~

### SRC-COMP-L1385-49F49B930AD5

- Kind: `deletion`
- Source: `compiler-proposal.md:1385-1385`
- Applicability: `VCP6`
- Exact text SHA-256: `49f49b930ad54828d13fc8def21ca8b432fe0826fc5502b00dbfddc2383840fa`

~~~~markdown
**Suggested subblocks:** framework assembly, style/CSS artifacts, host adapters, custom-block opaque publication, route cutover, deletion and rollback.
~~~~

### SRC-COMP-L1387-DE2D082940FC

- Kind: `forbidden`
- Source: `compiler-proposal.md:1387-1387`
- Applicability: `VCP6`
- Exact text SHA-256: `de2d082940fc825d72f4e32158b1f7f43b30d757176f5f5b4035848feee06b54`

~~~~markdown
**Acceptance:** generic session has no Vue module topology; all targets/maps/artifacts are complete; old and new paths never remain simultaneously authoritative; custom blocks are preserved without execution; host integrations cannot repair semantic output.
~~~~

### SRC-COMP-L1389-87D187266D7F

- Kind: `forbidden`
- Source: `compiler-proposal.md:1389-1389`
- Applicability: `VCP6`
- Exact text SHA-256: `87d187266d7fc2ef80263028ed4265e380f17c4c9912638c53c3cec564c91719`

~~~~markdown
**Forbidden:** dynamic custom-block ABI, generic session assembly, hidden CSS pipeline, or per-host compiler semantics.
~~~~

### SRC-COMP-L1391-30F6A9EF1E00

- Kind: `deletion`
- Source: `compiler-proposal.md:1391-1391`
- Applicability: `VCP6`
- Exact text SHA-256: `30f6a9ef1e0035ff2ff9204839ce9a210be502b94afdc7a138b9f731d4786d92`

~~~~markdown
**Deletion/abort:** this is the sole Vue cutover/deletion owner; abort on any unexplained target/artifact/map divergence.
~~~~

### SRC-COMP-L1393-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1393-1393`
- Applicability: `VCP6`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
