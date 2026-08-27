<!-- unified-charter-v2
id=VST1
name=Vue selector-to-template query engine
phase=compiler
train=compiler.vue-style-query
product=vue_style_query
kind=implementation
semantic_role=delivery
class=compiler
predecessors=VCP2,VST0
conditional_predecessors=
owner=compiler.vue-style-query:indexed selector-to-template query service
conflict_domains=style_semantics,vue_product
resource_class=rust-mixed
review_profile=semantic-3
gate_profile=targeted-domain
size=M
dispatchable=true
optional=false
release_gating=non_release
source_refs=source:compiler-proposal.md:L1257
external_requirements=
activation_gate=ORC0
charter=charters/compiler-vue-style-query/VST1.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# VST1 — Vue selector-to-template query engine

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Vue selector-to-template query engine. The current owner is **Vue selector/template scans**. The final and sole owner is **indexed selector-to-template query service**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src/framework/vue`, `crates/verter_semantic/src`.
- Named API/data boundaries: `SelectorQuery`, `TemplateCandidateIndex`, `MatchFact`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **VCP2:** exact current receipt ID and digest for “Compact Vue compiler structure and canonical template topology”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **VST0:** exact current receipt ID and digest for “Vue framework style semantics and scope plan”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** provide a Vue-owned selector applicability service for tooling and future optimization without taxing default runtime compilation.
- **Problem:** CSS diagnostics/navigation/component analysis need selector-to-template relationships, but Vue default runtime compilation does not require selector pruning and should not pay for it.
- **Solution and architecture decisions:**
- consume J selector structure and VCP2 Vue template topology;

## Acceptance IDs and discriminating proof

- **VST1-AC1 — sole-owner proof:** add `vst1_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **VST1-AC2 — positive contract:** add `vst1_publishes_exact_selectorquery`; assert exact identities, provenance, completeness, and deterministic ordering.
- **VST1-AC3 — incremental equivalence:** add `vst1_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **VST1-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_compiler/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **whole-template rescans**.
- Delete or structurally reject: **string-only selector inference**.
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

1. `cargo nextest run -p verter_compiler`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `semantic-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three named independent reviewers with assigned lenses, model `gpt-5.6-sol`, effort `ultra`, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1257`

## Reconciled source-plan contract

**Intent:** provide a Vue-owned selector applicability service for tooling and future optimization without taxing default runtime compilation.

**Problem:** CSS diagnostics/navigation/component analysis need selector-to-template relationships, but Vue default runtime compilation does not require selector pruning and should not pay for it.

**Solution and architecture decisions:**

- consume J selector structure and `VCP2` Vue template topology;
- derive a compact selector query plan only when demanded and cost-effective;
- use adaptive direct versus indexed matching;
- postings use only sound positive anchors; negated predicates never seed candidates;
- dynamic tags/classes/IDs/attributes and spreads enter explicit maybe buckets;
- exact Vue matcher returns `Yes | Maybe | No` and remains authoritative;
- produce `VueStyleMatchFacts` for diagnostics, navigation, component information and future `Optimized` consideration;
- `Default` runtime targets demand none of this work unless a separately locked correctness cell requires it;
- no pruning behavior is admitted by this block.

**Suggested predecessors:** `VCP2`, `VST0`.

**Normative source decomposition:** semantic contract, direct matcher, topology feature index, selector query plan, adaptive cost model, fact/witness publication and performance gates.

**Acceptance:** direct and indexed paths are semantically identical; candidate reduction has no false negatives; dynamic cases remain `Maybe`; default compiler ledgers show zero VST1 work; tooling consumers can request sparse witnesses without production overhead.

**Forbidden:** making VST1 a VCP7 predecessor, universal selector semantics, always building an index, or using `Maybe` to remove CSS.

**Deletion/abort:** no runtime compiler deletion; move shared mechanics only after measured neutral equivalence.

---

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1257-56B783CE3304

- Kind: `context`
- Source: `compiler-proposal.md:1257-1257`
- Applicability: `VST1`
- Exact text SHA-256: `56b783ce33044714c35e5015d54cee652504001d47a3c54dc968841d70db426b`

~~~~markdown
## `VST1.md` — Vue selector-to-template query engine
~~~~

### SRC-COMP-L1259-E2BAF1185E45

- Kind: `context`
- Source: `compiler-proposal.md:1259-1259`
- Applicability: `VST1`
- Exact text SHA-256: `e2baf1185e45062f53149ec3dee05c56e5a8cf08f4859cd1537cc0aa85838b67`

~~~~markdown
**Intent:** provide a Vue-owned selector applicability service for tooling and future optimization without taxing default runtime compilation.
~~~~

### SRC-COMP-L1261-DC6EA449CCEA

- Kind: `context`
- Source: `compiler-proposal.md:1261-1261`
- Applicability: `VST1`
- Exact text SHA-256: `dc6ea449ccea3dbd871485832faec638fd7d135b2237c673d20e162fc010f85c`

~~~~markdown
**Problem:** CSS diagnostics/navigation/component analysis need selector-to-template relationships, but Vue default runtime compilation does not require selector pruning and should not pay for it.
~~~~

### SRC-COMP-L1263-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:1263-1263`
- Applicability: `VST1`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1265-9ABF6D07A24B

- Kind: `context`
- Source: `compiler-proposal.md:1265-1265`
- Applicability: `VST1`
- Exact text SHA-256: `9abf6d07a24b81f956dd24ae0e93a1892a5d4434686addb2bb9ad781eae1d74e`

~~~~markdown
- consume J selector structure and `VCP2` Vue template topology;
~~~~

### SRC-COMP-L1266-0E1AF0410204

- Kind: `requirement`
- Source: `compiler-proposal.md:1266-1266`
- Applicability: `VST1`
- Exact text SHA-256: `0e1af04102044764b3f75b1051e003c1373723dff83987cb39f46b20a205d8b8`

~~~~markdown
- derive a compact selector query plan only when demanded and cost-effective;
~~~~

### SRC-COMP-L1267-CC88520214F5

- Kind: `context`
- Source: `compiler-proposal.md:1267-1267`
- Applicability: `VST1`
- Exact text SHA-256: `cc88520214f5c5527d70f864c390060cc8df7fab97ea9c3b6907cea91e955751`

~~~~markdown
- use adaptive direct versus indexed matching;
~~~~

### SRC-COMP-L1268-124DCB4EFEF0

- Kind: `forbidden`
- Source: `compiler-proposal.md:1268-1268`
- Applicability: `VST1`
- Exact text SHA-256: `124dcb4efef0231af66477f11ab93de9c0c91801e1c4f3c11e2602a4cbdff0a1`

~~~~markdown
- postings use only sound positive anchors; negated predicates never seed candidates;
~~~~

### SRC-COMP-L1269-89F5A1554016

- Kind: `context`
- Source: `compiler-proposal.md:1269-1269`
- Applicability: `VST1`
- Exact text SHA-256: `89f5a1554016b6a30042364a7722361a74b2fe069593a1c36d6d84a7d86e0686`

~~~~markdown
- dynamic tags/classes/IDs/attributes and spreads enter explicit maybe buckets;
~~~~

### SRC-COMP-L1270-9AC095E12DB2

- Kind: `requirement`
- Source: `compiler-proposal.md:1270-1270`
- Applicability: `VST1`
- Exact text SHA-256: `9ac095e12db278889658492d1abe0a6c93e492b29ebe7ee2f48931352f1c7548`

~~~~markdown
- exact Vue matcher returns `Yes | Maybe | No` and remains authoritative;
~~~~

### SRC-COMP-L1271-133978099F24

- Kind: `context`
- Source: `compiler-proposal.md:1271-1271`
- Applicability: `VST1`
- Exact text SHA-256: `133978099f2467000d49bcf2679ccfbf9b7afecd1b6e2ad4d4d513c211b68507`

~~~~markdown
- produce `VueStyleMatchFacts` for diagnostics, navigation, component information and future `Optimized` consideration;
~~~~

### SRC-COMP-L1272-5181DE322672

- Kind: `requirement`
- Source: `compiler-proposal.md:1272-1272`
- Applicability: `VST1`
- Exact text SHA-256: `5181de322672736ccf8913d6f0d8fba2027d99bdd0beeb7c3b7519c720b5adae`

~~~~markdown
- `Default` runtime targets demand none of this work unless a separately locked correctness cell requires it;
~~~~

### SRC-COMP-L1273-5AE5FD71083C

- Kind: `context`
- Source: `compiler-proposal.md:1273-1273`
- Applicability: `VST1`
- Exact text SHA-256: `5ae5fd71083c194a485e6a710e31e83ce155d73227a0fcd91665374d4d8e572b`

~~~~markdown
- no pruning behavior is admitted by this block.
~~~~

### SRC-COMP-L1275-1E4BF1D3A01D

- Kind: `context`
- Source: `compiler-proposal.md:1275-1275`
- Applicability: `VST1`
- Exact text SHA-256: `1e4bf1d3a01d79737e78ee77dc804362fd1e31beac1e93e294a66d41aa363639`

~~~~markdown
**Suggested predecessors:** `VCP2`, `VST0`.
~~~~

### SRC-COMP-L1277-4F6DD79FCF20

- Kind: `context`
- Source: `compiler-proposal.md:1277-1277`
- Applicability: `VST1`
- Exact text SHA-256: `4f6dd79fcf2094b43f4a2c89c25ef904aa21c9e1225117b280d440188709e80f`

~~~~markdown
**Suggested subblocks:** semantic contract, direct matcher, topology feature index, selector query plan, adaptive cost model, fact/witness publication and performance gates.
~~~~

### SRC-COMP-L1279-06DE28D11A43

- Kind: `acceptance`
- Source: `compiler-proposal.md:1279-1279`
- Applicability: `VST1`
- Exact text SHA-256: `06de28d11a43ea435714d5040d734cc83b4cb67243a9253c3decadda9c8e95a2`

~~~~markdown
**Acceptance:** direct and indexed paths are semantically identical; candidate reduction has no false negatives; dynamic cases remain `Maybe`; default compiler ledgers show zero VST1 work; tooling consumers can request sparse witnesses without production overhead.
~~~~

### SRC-COMP-L1281-F7FE3CF656C7

- Kind: `forbidden`
- Source: `compiler-proposal.md:1281-1281`
- Applicability: `VST1`
- Exact text SHA-256: `f7fe3cf656c7e4ebac875ea79a18c24f6bd26f01e412791f8f3f879ca561734d`

~~~~markdown
**Forbidden:** making VST1 a VCP7 predecessor, universal selector semantics, always building an index, or using `Maybe` to remove CSS.
~~~~

### SRC-COMP-L1283-78EB7F1FFCFF

- Kind: `deletion`
- Source: `compiler-proposal.md:1283-1283`
- Applicability: `VST1`
- Exact text SHA-256: `78eb7f1ffcff2bfce4e2b359eb042bf2f0043aaea474101169ae7694139d3606`

~~~~markdown
**Deletion/abort:** no runtime compiler deletion; move shared mechanics only after measured neutral equivalence.
~~~~

### SRC-COMP-L1285-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1285-1285`
- Applicability: `VST1`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
