<!-- unified-charter-v2
id=VCP0
name=Exact Vue Default compiler lock
phase=compiler
train=compiler.vue-compiler
product=vue_compiler
kind=lock
semantic_role=delivery
class=compiler
predecessors=CMP5
conditional_predecessors=
owner=compiler.vue-compiler:Vue-owned Default compiler cells over shared compiler substrate
conflict_domains=compiler_execution,vue_product
resource_class=docs-light
review_profile=architecture-3
gate_profile=docs-domain
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
source_refs=source:compiler-proposal.md:L1145
external_requirements=
activation_gate=ORC0
charter=charters/compiler-vue-compiler/VCP0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# VCP0 — Exact Vue Default compiler lock

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Exact Vue Default compiler lock. The current owner is **Vue runtime emitter and assembly paths**. The final and sole owner is **Vue-owned Default compiler cells over shared compiler substrate**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src/framework/vue`, `crates/verter_vue_conformance`, `packages/vue-conformance-oracle`.
- Named API/data boundaries: `VueSemanticSnapshot`, `VueCompilePlan`, `VueTarget`, `VueArtifactSet`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **CMP5:** exact current receipt ID and digest for “Provisional shared compiler-core contract lock”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** freeze the exact Vue semantic epoch, default behavior contracts, targets, corpora, known divergences, and performance gates before implementation.
- **Problem:** output similarity is insufficient, upstream behavior is not an infallible oracle, and post-implementation criteria invite compatibility drift.
- **Solution and architecture decisions:**
- pin exact Vue release/commits and Verter semantic/compiler epochs;

## Acceptance IDs and discriminating proof

- **VCP0-AC1 — sole-owner proof:** add `vcp0_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **VCP0-AC2 — positive contract:** add `vcp0_publishes_exact_vuesemanticsnapshot`; assert exact identities, provenance, completeness, and deterministic ordering.
- **VCP0-AC3 — incremental equivalence:** add `vcp0_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **VCP0-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_vue_conformance/tests`, `packages/vue-conformance-oracle`.

## Deletions and forbidden designs

- Delete or structurally reject: **legacy Vue emitter route**.
- Delete or structurally reject: **per-target prerequisite duplication**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance is 0.0% unless an owning-authority amendment supplies exact replacement thresholds; after warmup, retained bytes may not increase across 100 identical requests.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_vue_conformance`
2. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `architecture-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three fresh distinct harness reviewers with assigned lenses, deterministic low|medium|high effort, exact task/provider/model bindings, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1145`

## Reconciled source-plan contract

**Intent:** freeze the exact Vue semantic epoch, default behavior contracts, targets, corpora, known divergences, and performance gates before implementation.

**Problem:** output similarity is insufficient, upstream behavior is not an infallible oracle, and post-implementation criteria invite compatibility drift.

**Solution and architecture decisions:**

- pin exact Vue release/commits and Verter semantic/compiler epochs;
- define `DefaultCompilationContractId` cells for VDOM, SSR and Vapor;
- lock runtime/hydration/public-export/diagnostic/map/CSS/module behavior grades;
- lock permitted Verter corrections to upstream gaps before implementation;
- lock component-local facts allowed by `Default` and prove no workspace file loading;
- pin official/reference compilers, runtime validators, source-map validators and real-project corpora;
- lock custom blocks as opaque descriptors and `Optimized` as unsupported;
- lock equivalent-work and RSS gates from `CPER0`.

**Suggested predecessor:** `CMP5`.

**Normative source decomposition:** release/oracle dossier, product matrix, divergence ledger, corpus/runtime validator, performance lock, independent challenge reviews.

**Acceptance:** no criterion is selected after implementation; every target/option/CSS/map/diagnostic cell has an owner and observable pass rule; cheap local semantic improvements are either admitted or explicitly forbidden by contract.

**Forbidden:** byte parity as the only oracle, project-wide optimization, custom-block ABI, or scope growth after seeing failures.

**Deletion/abort:** no code; rescope unsupported cells rather than silently weaken them.

---

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1145-4C752CB4A437

- Kind: `requirement`
- Source: `compiler-proposal.md:1145-1145`
- Applicability: `VCP0`
- Exact text SHA-256: `4c752cb4a437b8888942993e6cb327ee057c68f65c9c57b9675a3f099fe4b8b5`

~~~~markdown
## `VCP0.md` — Exact Vue Default compiler lock
~~~~

### SRC-COMP-L1147-8B11CFB8C983

- Kind: `requirement`
- Source: `compiler-proposal.md:1147-1147`
- Applicability: `VCP0`
- Exact text SHA-256: `8b11cfb8c9834444c2eceaf1c8155fbff49c02dcb1e1fcc14334f7cd9414012e`

~~~~markdown
**Intent:** freeze the exact Vue semantic epoch, default behavior contracts, targets, corpora, known divergences, and performance gates before implementation.
~~~~

### SRC-COMP-L1149-43DA3A85C18F

- Kind: `context`
- Source: `compiler-proposal.md:1149-1149`
- Applicability: `VCP0`
- Exact text SHA-256: `43da3a85c18fd5e251bcaa02d93d216f5dbc6785b151a6f47177e07410bcc912`

~~~~markdown
**Problem:** output similarity is insufficient, upstream behavior is not an infallible oracle, and post-implementation criteria invite compatibility drift.
~~~~

### SRC-COMP-L1151-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:1151-1151`
- Applicability: `VCP0`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1153-4701A21EAB2E

- Kind: `requirement`
- Source: `compiler-proposal.md:1153-1153`
- Applicability: `VCP0`
- Exact text SHA-256: `4701a21eab2e664b86d57ed4863600a48dbb7293986cf4c38f7bb5c21830f907`

~~~~markdown
- pin exact Vue release/commits and Verter semantic/compiler epochs;
~~~~

### SRC-COMP-L1154-CA44A459B82D

- Kind: `context`
- Source: `compiler-proposal.md:1154-1154`
- Applicability: `VCP0`
- Exact text SHA-256: `ca44a459b82d5f2a1bf054bf835f09eb33eab15672a603c49a30b60a9c73a9fd`

~~~~markdown
- define `DefaultCompilationContractId` cells for VDOM, SSR and Vapor;
~~~~

### SRC-COMP-L1155-8C4088B4445C

- Kind: `context`
- Source: `compiler-proposal.md:1155-1155`
- Applicability: `VCP0`
- Exact text SHA-256: `8c4088b4445cf38ea70fe4795990302bb4f7193827ac7ab21afbe2f5acb5c7e9`

~~~~markdown
- lock runtime/hydration/public-export/diagnostic/map/CSS/module behavior grades;
~~~~

### SRC-COMP-L1156-06D540616969

- Kind: `context`
- Source: `compiler-proposal.md:1156-1156`
- Applicability: `VCP0`
- Exact text SHA-256: `06d54061696952d5d5ebc34f828ad409284e1eefe370bb793f1081eda58a1f09`

~~~~markdown
- lock permitted Verter corrections to upstream gaps before implementation;
~~~~

### SRC-COMP-L1157-FED24729D9F3

- Kind: `context`
- Source: `compiler-proposal.md:1157-1157`
- Applicability: `VCP0`
- Exact text SHA-256: `fed24729d9f3d355d82a01c82f8912a0e2320ed6770eb60abb85fa96074492cd`

~~~~markdown
- lock component-local facts allowed by `Default` and prove no workspace file loading;
~~~~

### SRC-COMP-L1158-1C0BF5892FB2

- Kind: `context`
- Source: `compiler-proposal.md:1158-1158`
- Applicability: `VCP0`
- Exact text SHA-256: `1c0bf5892fb2c0838813da0026970a82cad6269f4c5d2dbe7e639d230e1cf4a5`

~~~~markdown
- pin official/reference compilers, runtime validators, source-map validators and real-project corpora;
~~~~

### SRC-COMP-L1159-6D4537D29A13

- Kind: `context`
- Source: `compiler-proposal.md:1159-1159`
- Applicability: `VCP0`
- Exact text SHA-256: `6d4537d29a13199f9533d5c07c048a103fd8b538f87de0b201b2cbc348a84922`

~~~~markdown
- lock custom blocks as opaque descriptors and `Optimized` as unsupported;
~~~~

### SRC-COMP-L1160-4893E0E7C655

- Kind: `context`
- Source: `compiler-proposal.md:1160-1160`
- Applicability: `VCP0`
- Exact text SHA-256: `4893e0e7c655e8cfc1ab74a8fd80aa44356f66afe7ffd98182257934b8f028be`

~~~~markdown
- lock equivalent-work and RSS gates from `CPER0`.
~~~~

### SRC-COMP-L1162-5F57552C6CA5

- Kind: `context`
- Source: `compiler-proposal.md:1162-1162`
- Applicability: `VCP0`
- Exact text SHA-256: `5f57552c6ca5d7f886bf237de4d0b55692ed5ce22d73bc3acb98e3b5bc163688`

~~~~markdown
**Suggested predecessor:** `CMP5`.
~~~~

### SRC-COMP-L1164-D2CEFE0F8D72

- Kind: `context`
- Source: `compiler-proposal.md:1164-1164`
- Applicability: `VCP0`
- Exact text SHA-256: `d2cefe0f8d72e86f60f642b091bf109eb55745516dd46eac46a82f6fe26ea85d`

~~~~markdown
**Suggested subblocks:** release/oracle dossier, product matrix, divergence ledger, corpus/runtime validator, performance lock, independent challenge reviews.
~~~~

### SRC-COMP-L1166-46FE42ED3B3E

- Kind: `forbidden`
- Source: `compiler-proposal.md:1166-1166`
- Applicability: `VCP0`
- Exact text SHA-256: `46fe42ed3b3eaa763c6f0de2a90f77794cc9d6e77ef70b64e9ab23f9f0208b8b`

~~~~markdown
**Acceptance:** no criterion is selected after implementation; every target/option/CSS/map/diagnostic cell has an owner and observable pass rule; cheap local semantic improvements are either admitted or explicitly forbidden by contract.
~~~~

### SRC-COMP-L1168-CDB8A740CAC1

- Kind: `forbidden`
- Source: `compiler-proposal.md:1168-1168`
- Applicability: `VCP0`
- Exact text SHA-256: `cdb8a740cac1c807760dbf5769f0c52250f5a55d247710e379bf0347471c96be`

~~~~markdown
**Forbidden:** byte parity as the only oracle, project-wide optimization, custom-block ABI, or scope growth after seeing failures.
~~~~

### SRC-COMP-L1170-463535728746

- Kind: `deletion`
- Source: `compiler-proposal.md:1170-1170`
- Applicability: `VCP0`
- Exact text SHA-256: `463535728746c0612005eade34581cb4e514b14690bdf72bdd691c51beacb4b6`

~~~~markdown
**Deletion/abort:** no code; rescope unsupported cells rather than silently weaken them.
~~~~

### SRC-COMP-L1172-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1172-1172`
- Applicability: `VCP0`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
