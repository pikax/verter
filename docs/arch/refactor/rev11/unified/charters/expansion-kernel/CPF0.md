<!-- unified-charter-v2
id=CPF0
name=Carrier frontend/compiler-backend separation proof
phase=expansion
train=expansion.kernel
product=kernel
kind=proof
semantic_role=delivery
class=successor
predecessors=UAK1,VID0
conditional_predecessors=
owner=expansion.kernel:typed immutable universal catalog and demand-selected kernel services
conflict_domains=compiler_execution
resource_class=docs-light
review_profile=semantic-3
gate_profile=docs-domain
size=M
dispatchable=true
optional=false
release_gating=none
source_refs=source:successor-expansion.md:L797
external_requirements=
activation_gate=ORC0
charter=charters/expansion-kernel/CPF0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# CPF0 — Carrier frontend/compiler-backend separation proof

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Carrier frontend/compiler-backend separation proof. The current owner is **framework-shaped host/session registries and untagged public boundaries**. The final and sole owner is **typed immutable universal catalog and demand-selected kernel services**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `crates/verter_protocol/src`, `crates/verter_identity/src`.
- Named API/data boundaries: `CarrierProfileId`, `FrameworkProfileId`, `ProjectProfileId`, `CatalogSnapshot`, `DemandPlan`, `TypeInfoRequest`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **UAK1:** exact current receipt ID and digest for “Universal-tooling constitution and program split”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **VID0:** exact current receipt ID and digest for “Orthogonal identities and exact-release law”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** prove the compiler-shaped carrier abstraction can be split without weakening current compilation or tooling.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **central framework switch**, **untagged coordinate/public identity**, **duplicate component information authority** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

- **CPF0-AC1 — sole-owner proof:** add `cpf0_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **CPF0-AC2 — positive contract:** add `cpf0_publishes_exact_carrierprofileid`; assert exact identities, provenance, completeness, and deterministic ordering.
- **CPF0-AC3 — incremental equivalence:** add `cpf0_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **CPF0-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_language/tests`, `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **central framework switch**.
- Delete or structurally reject: **untagged coordinate/public identity**.
- Delete or structurally reject: **duplicate component information authority**.
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

1. `cargo nextest run -p verter_language -p verter_protocol -p verter_session`
2. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `semantic-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three named independent reviewers with assigned lenses, model `gpt-5.6-sol`, effort `ultra`, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L797`

## Reconciled source-plan contract

**Intent:** prove the compiler-shaped carrier abstraction can be split without weakening current compilation or tooling.
**Predecessors:** `UAK1`, `VID0`.
**Subblocks:** (1) inventory every `CarrierCompiler` method/caller; (2) classify frontend versus optional compiler products; (3) design `CarrierFrontend` and `CarrierCompilerBackend` contracts plus capability rows; (4) map Vue/Svelte migration and all deletion sites; (5) compile representative tooling-only HTML/Astro stubs as type-level proofs without `Unsupported` compiler implementations; (6) benchmark dispatch/allocation impact.
**Acceptance:** a reviewed migration ledger accounts for every method/type/caller; compiler output bytes/maps remain owned by the optional backend; the frontend can exist without importing runtime codegen.
**Forbidden:** implementing production behavior, preserving one combined trait behind aliases, or making “no compiler” an error path of normal tooling.
**Deletion/abort:** no deletion in the proof block; abort if separation requires duplicating parse artifacts or changing accepted Vue/Svelte output semantics.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1869-0B2135787554

- Kind: `context`
- Source: `compiler-proposal.md:1869-1869`
- Applicability: `CPF0`
- Exact text SHA-256: `0b21357875545eab8e91fcb1db8f9fe46e3bcf58dfd984d493c678cb335c4fc7`

~~~~markdown
## 11.1 `CPF0`
~~~~

### SRC-COMP-L1871-FDD0B74F467E

- Kind: `context`
- Source: `compiler-proposal.md:1871-1871`
- Applicability: `CPF0`
- Exact text SHA-256: `fdd0b74f467ea7b3098c71ad72176cf3f2c03006de7728e132a1e150a49ab025`

~~~~markdown
Change the two-way proof:
~~~~

### SRC-COMP-L1873-EC161EEBB74D

- Kind: `context`
- Source: `compiler-proposal.md:1873-1876`
- Applicability: `CPF0`
- Exact text SHA-256: `ec161eebb74ddcf99b1bedba72eee32215ecde04d4b36ce30e79905c30cf85d2`

~~~~markdown
```text
CarrierFrontend
CarrierCompilerBackend
```
~~~~

### SRC-COMP-L1878-0C94F065BCE0

- Kind: `context`
- Source: `compiler-proposal.md:1878-1878`
- Applicability: `CPF0`
- Exact text SHA-256: `0c94f065bce0818cd63fc9d1502beaa0c94661d03d2629c1121173dfb01794b8`

~~~~markdown
into verification of the accepted five authorities:
~~~~

### SRC-COMP-L1880-BFA9029A5AC3

- Kind: `context`
- Source: `compiler-proposal.md:1880-1886`
- Applicability: `CPF0`
- Exact text SHA-256: `bfa9029a5ac3e3199c8d9fa3e0de7f58857f3db3933b52d9b24d69d46013a731`

~~~~markdown
```text
CarrierFrontend
FrameworkSemanticAuthority<FrameworkEpoch>
ProjectionBackend
RuntimeCompilerBackend<FrameworkEpoch>
FrameworkHostIntegrationBackend<FrameworkEpoch, HostEpoch>
```
~~~~

### SRC-COMP-L1888-99E5F070AF04

- Kind: `forbidden`
- Source: `compiler-proposal.md:1888-1888`
- Applicability: `CPF0`
- Exact text SHA-256: `99e5f070af04b1e248a20189f1f54eb7a0652f93db55ed553dbc6cdca8a2db0c`

~~~~markdown
`CPF0` should consume the accepted CCA receipts through `BR0`; it must not reopen policy, artifact or host boundaries.
~~~~

### SRC-COMP-L1890-FADD8AF65244

- Kind: `requirement`
- Source: `compiler-proposal.md:1890-1890`
- Applicability: `CPF0`
- Exact text SHA-256: `fadd8af652442f4bceb52088486467bc74499622fb4fedb28c31b36d25331c16`

~~~~markdown
Required negative proofs:
~~~~

### SRC-COMP-L1892-350D3F27CEC2

- Kind: `requirement`
- Source: `compiler-proposal.md:1892-1892`
- Applicability: `CPF0`
- Exact text SHA-256: `350d3f27cec2c077f6e6ea32e63eec26267500276dfa2149fcb78e8bc0da0ba7`

~~~~markdown
- tooling-only carrier requires no runtime compiler;
~~~~

### SRC-COMP-L1893-1722485AB191

- Kind: `requirement`
- Source: `compiler-proposal.md:1893-1893`
- Applicability: `CPF0`
- Exact text SHA-256: `1722485ab191f37ead2015dc59b9d02e9d14da0de88786703bd8ae2906f4060e`

~~~~markdown
- runtime compiler requires no projection backend;
~~~~

### SRC-COMP-L1894-D091517E8CBB

- Kind: `requirement`
- Source: `compiler-proposal.md:1894-1894`
- Applicability: `CPF0`
- Exact text SHA-256: `d091517e8cbb56ddf8ad5fe3f4662bf672ca46759b725708d48324e904d4b291`

~~~~markdown
- projection backend requires no runtime module topology;
~~~~

### SRC-COMP-L1895-D72C659339B7

- Kind: `context`
- Source: `compiler-proposal.md:1895-1895`
- Applicability: `CPF0`
- Exact text SHA-256: `d72c659339b73c63dd7353726516dec5574ada8c02c9714f55214e2973ed0580`

~~~~markdown
- framework semantic authority imports no target codegen;
~~~~

### SRC-COMP-L1896-2DB3C67A8C97

- Kind: `context`
- Source: `compiler-proposal.md:1896-1896`
- Applicability: `CPF0`
- Exact text SHA-256: `2db3c67a8c97e592a3dd491f9736f0ae5842e6964e8356bb6e1a59ad2812dea6`

~~~~markdown
- `type_info` cannot issue framework conclusions;
~~~~

### SRC-COMP-L1897-2D267CE948A3

- Kind: `context`
- Source: `compiler-proposal.md:1897-1897`
- Applicability: `CPF0`
- Exact text SHA-256: `2d267ce948a3a2eff00523041ac6fbae3d50c16f4e81626faea8d6e32ac98dca`

~~~~markdown
- host integration cannot repair incomplete framework semantics;
~~~~

### SRC-COMP-L1898-600D2F35E798

- Kind: `context`
- Source: `compiler-proposal.md:1898-1898`
- Applicability: `CPF0`
- Exact text SHA-256: `600d2f35e79860ca92d6efb157dec2f97bfd6ea288a05d0dcfea82a390f41ef6`

~~~~markdown
- J remains CSS syntax/neutral semantic owner;
~~~~

### SRC-COMP-L1899-CA93EF258AE5

- Kind: `context`
- Source: `compiler-proposal.md:1899-1899`
- Applicability: `CPF0`
- Exact text SHA-256: `ca93ef258ae5e5562299adebbae351250a28df13f1ba3a2629cd92ae773f2c49`

~~~~markdown
- lossless tooling sidecars cannot enter compiler IR.
~~~~

### SRC-EXP-L797-A6B04D41D43C

- Kind: `context`
- Source: `successor-expansion.md:797-797`
- Applicability: `CPF0`
- Exact text SHA-256: `a6b04d41d43c34734e73e0014a978f0689ae46bc03bd65706ba6d78707bffc8d`

~~~~markdown
### `CPF0.md` — Carrier frontend/compiler-backend separation proof
~~~~

### SRC-EXP-L799-69D7826900DD

- Kind: `forbidden`
- Source: `successor-expansion.md:799-804`
- Applicability: `CPF0`
- Exact text SHA-256: `69d7826900dd492889922725f402be10415d4f3e1a7f6a04b19bd237d64b7c06`

~~~~markdown
**Intent:** prove the compiler-shaped carrier abstraction can be split without weakening current compilation or tooling.
**Predecessors:** `UAK1`, `VID0`.
**Subblocks:** (1) inventory every `CarrierCompiler` method/caller; (2) classify frontend versus optional compiler products; (3) design `CarrierFrontend` and `CarrierCompilerBackend` contracts plus capability rows; (4) map Vue/Svelte migration and all deletion sites; (5) compile representative tooling-only HTML/Astro stubs as type-level proofs without `Unsupported` compiler implementations; (6) benchmark dispatch/allocation impact.
**Acceptance:** a reviewed migration ledger accounts for every method/type/caller; compiler output bytes/maps remain owned by the optional backend; the frontend can exist without importing runtime codegen.
**Forbidden:** implementing production behavior, preserving one combined trait behind aliases, or making “no compiler” an error path of normal tooling.
**Deletion/abort:** no deletion in the proof block; abort if separation requires duplicating parse artifacts or changing accepted Vue/Svelte output semantics.
~~~~
