<!-- unified-charter-v2
id=VCP1
name=Canonical Vue semantic authority convergence
phase=compiler
train=compiler.vue-compiler
product=vue_compiler
kind=convergence
semantic_role=convergence
class=compiler
predecessors=VCP0
conditional_predecessors=
owner=compiler.vue-compiler:Vue-owned Default compiler cells over shared compiler substrate
conflict_domains=semantic_authority,vue_product
resource_class=rust-mixed
review_profile=architecture-3
gate_profile=targeted-domain
size=S
dispatchable=true
optional=false
release_gating=none
source_refs=source:compiler-proposal.md:L1174
external_requirements=
activation_gate=ORC0
charter=charters/compiler-vue-compiler/VCP1.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# VCP1 — Canonical Vue semantic authority convergence

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Canonical Vue semantic authority convergence. The current owner is **Vue runtime emitter and assembly paths**. The final and sole owner is **Vue-owned Default compiler cells over shared compiler substrate**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src/framework/vue`, `crates/verter_vue_conformance`, `packages/vue-conformance-oracle`.
- Named API/data boundaries: `VueSemanticSnapshot`, `VueCompilePlan`, `VueTarget`, `VueArtifactSet`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **VCP0:** exact current receipt ID and digest for “Exact Vue Default compiler lock”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** make one Vue semantic authority provide every framework fact used by compiler and tooling.
- **Problem:** compiler-local import, binding, reactivity, directive, style, or dependency analysis can disagree with IDE/lint and duplicate expensive work.
- **Solution and architecture decisions:**
- implement/extend Vue fact families inside the Vue semantic authority using shared verter_analysis/type_info machinery;

## Acceptance IDs and discriminating proof

- **VCP1-AC1 — sole-owner proof:** add `vcp1_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **VCP1-AC2 — positive contract:** add `vcp1_publishes_exact_vuesemanticsnapshot`; assert exact identities, provenance, completeness, and deterministic ordering.
- **VCP1-AC3 — incremental equivalence:** add `vcp1_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **VCP1-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_vue_conformance/tests`, `packages/vue-conformance-oracle`.

## Deletions and forbidden designs

- Delete or structurally reject: **legacy Vue emitter route**.
- Delete or structurally reject: **per-target prerequisite duplication**.
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

1. `cargo nextest run -p verter_compiler -p verter_vue_conformance`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `architecture-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three named independent reviewers with assigned lenses, model `gpt-5.6-sol`, effort `ultra`, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1174`

## Reconciled source-plan contract

**Intent:** make one Vue semantic authority provide every framework fact used by compiler and tooling.

**Problem:** compiler-local import, binding, reactivity, directive, style, or dependency analysis can disagree with IDE/lint and duplicate expensive work.

**Solution and architecture decisions:**

- implement/extend Vue fact families inside the Vue semantic authority using shared `verter_analysis`/`type_info` machinery;
- scopes, bindings, props/macros, component/element classification, directives, slots, reads/writes/dependencies, mutability, stability, purity and reactivity have one owner;
- component-local framework-origin evidence supports contract-admitted literal framework imports, namespace/destructuring, immutable aliases and local alias chains visible in the SFC; it is distinct from resolved package provenance;
- no node_modules/package/declaration/implementation loading under `Default`;
- hot facts use compact dense summaries; provenance/explanations are sparse and demand-only;
- delete compiler-local reparse/scanner/analyzer paths as consumers migrate.

**Suggested predecessor:** `VCP0`.

**Normative source decomposition:** script/import facts, binding/scope facts, template/directive/slot facts, reactivity/dependency facts, compact storage/provenance, compiler-consumer cutover.

**Acceptance:** planted cheap alias cases produce the stronger correct fact in `Default`; same-spelled user functions and mutable aliases fail closed; compiler/IDE/lint observe one result; expression/import parse counts do not increase.

**Forbidden:** a separate “fast compiler analyzer”, project traversal, tsgo, type-shape-only origin proof, or compiler-owned Vue semantics.

**Deletion/abort:** delete duplicate analysis only after cross-consumer parity; return uncertain dynamic cases as `Unknown`.

---

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1174-5D78E68CE415

- Kind: `context`
- Source: `compiler-proposal.md:1174-1174`
- Applicability: `VCP1`
- Exact text SHA-256: `5d78e68ce415afe44debf4901c4e38339c57fa34e2c612497bb4c6ad94d0594b`

~~~~markdown
## `VCP1.md` — Canonical Vue semantic authority convergence
~~~~

### SRC-COMP-L1176-7E06E4C277C6

- Kind: `context`
- Source: `compiler-proposal.md:1176-1176`
- Applicability: `VCP1`
- Exact text SHA-256: `7e06e4c277c63af40bf62a897b1553181b4749d80974264f3b4a66441a086fb5`

~~~~markdown
**Intent:** make one Vue semantic authority provide every framework fact used by compiler and tooling.
~~~~

### SRC-COMP-L1178-2D381772175F

- Kind: `requirement`
- Source: `compiler-proposal.md:1178-1178`
- Applicability: `VCP1`
- Exact text SHA-256: `2d381772175fc1f9664c08a5891badcdfd9512f740f8b18f5a150b8e5790b0ab`

~~~~markdown
**Problem:** compiler-local import, binding, reactivity, directive, style, or dependency analysis can disagree with IDE/lint and duplicate expensive work.
~~~~

### SRC-COMP-L1180-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:1180-1180`
- Applicability: `VCP1`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1182-83B8ED4EBD11

- Kind: `context`
- Source: `compiler-proposal.md:1182-1182`
- Applicability: `VCP1`
- Exact text SHA-256: `83b8ed4ebd11d993d500d9d243e2dbe4f0981ea51b0a5c8963fd2e4e444849f9`

~~~~markdown
- implement/extend Vue fact families inside the Vue semantic authority using shared `verter_analysis`/`type_info` machinery;
~~~~

### SRC-COMP-L1183-3E55B7577DA7

- Kind: `requirement`
- Source: `compiler-proposal.md:1183-1183`
- Applicability: `VCP1`
- Exact text SHA-256: `3e55b7577da712df897a5a2a02f971d99f4f59ca0718100f3510d8018e9a6415`

~~~~markdown
- scopes, bindings, props/macros, component/element classification, directives, slots, reads/writes/dependencies, mutability, stability, purity and reactivity have one owner;
~~~~

### SRC-COMP-L1184-2DD3D92C39C4

- Kind: `context`
- Source: `compiler-proposal.md:1184-1184`
- Applicability: `VCP1`
- Exact text SHA-256: `2dd3d92c39c449090c9c0d8c6e615320b0218f12f365ec586fc66c6bef9590e7`

~~~~markdown
- component-local framework-origin evidence supports contract-admitted literal framework imports, namespace/destructuring, immutable aliases and local alias chains visible in the SFC; it is distinct from resolved package provenance;
~~~~

### SRC-COMP-L1185-77B81A60F63C

- Kind: `context`
- Source: `compiler-proposal.md:1185-1185`
- Applicability: `VCP1`
- Exact text SHA-256: `77b81a60f63cd9b4f8894dbacd60a0edc626ebd7fda8c4c7c1c72772c3a15c20`

~~~~markdown
- no node_modules/package/declaration/implementation loading under `Default`;
~~~~

### SRC-COMP-L1186-D3F761C9B08D

- Kind: `context`
- Source: `compiler-proposal.md:1186-1186`
- Applicability: `VCP1`
- Exact text SHA-256: `d3f761c9b08da85468787ce616e8e2cf6b501e848ca5ba8c4990e96573dac38a`

~~~~markdown
- hot facts use compact dense summaries; provenance/explanations are sparse and demand-only;
~~~~

### SRC-COMP-L1187-3912F7BD1F9E

- Kind: `deletion`
- Source: `compiler-proposal.md:1187-1187`
- Applicability: `VCP1`
- Exact text SHA-256: `3912f7bd1f9ed7509af5e8f4e3c543fae6ae93a3b2a272321dbb50676b0c5553`

~~~~markdown
- delete compiler-local reparse/scanner/analyzer paths as consumers migrate.
~~~~

### SRC-COMP-L1189-21D34E0AB4C5

- Kind: `context`
- Source: `compiler-proposal.md:1189-1189`
- Applicability: `VCP1`
- Exact text SHA-256: `21d34e0ab4c5ab89d22e6e9347c41e0de9e6bb4c8ab1c01a3f2fda0fab5a05e4`

~~~~markdown
**Suggested predecessor:** `VCP0`.
~~~~

### SRC-COMP-L1191-DC7A1EFC4EEE

- Kind: `requirement`
- Source: `compiler-proposal.md:1191-1191`
- Applicability: `VCP1`
- Exact text SHA-256: `dc7a1efc4eeed357bede4d0a661a96c525b83473af8911c57d2e9095f6fbe1ce`

~~~~markdown
**Suggested subblocks:** script/import facts, binding/scope facts, template/directive/slot facts, reactivity/dependency facts, compact storage/provenance, compiler-consumer cutover.
~~~~

### SRC-COMP-L1193-3A38E2F3FE97

- Kind: `acceptance`
- Source: `compiler-proposal.md:1193-1193`
- Applicability: `VCP1`
- Exact text SHA-256: `3a38e2f3fe97e3fe769f299c8df09bcec847795054120a00718714a8d7a1d24d`

~~~~markdown
**Acceptance:** planted cheap alias cases produce the stronger correct fact in `Default`; same-spelled user functions and mutable aliases fail closed; compiler/IDE/lint observe one result; expression/import parse counts do not increase.
~~~~

### SRC-COMP-L1195-7AE90F803D9D

- Kind: `forbidden`
- Source: `compiler-proposal.md:1195-1195`
- Applicability: `VCP1`
- Exact text SHA-256: `7ae90f803d9d61f1b38898fd97febdc402bfb9e1a362cf39a4a694c6473b3318`

~~~~markdown
**Forbidden:** a separate “fast compiler analyzer”, project traversal, tsgo, type-shape-only origin proof, or compiler-owned Vue semantics.
~~~~

### SRC-COMP-L1197-743C8303F57B

- Kind: `deletion`
- Source: `compiler-proposal.md:1197-1197`
- Applicability: `VCP1`
- Exact text SHA-256: `743c8303f57bd8effffb20259cec92cf76ce4b730e24b2308b86823e3281479d`

~~~~markdown
**Deletion/abort:** delete duplicate analysis only after cross-consumer parity; return uncertain dynamic cases as `Unknown`.
~~~~

### SRC-COMP-L1199-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1199-1199`
- Applicability: `VCP1`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
