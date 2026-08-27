<!-- unified-charter-v2
id=SCP2
name=Compact Svelte compiler structure and canonical template topology
phase=compiler
train=compiler.svelte-compiler
product=svelte_compiler
kind=implementation
semantic_role=delivery
class=compiler
predecessors=SCP1
conditional_predecessors=
owner=compiler.svelte-compiler:Svelte-owned Default compiler cells over shared compiler substrate
conflict_domains=compiler_execution,svelte_product
resource_class=rust-mixed
review_profile=semantic-3
gate_profile=targeted-domain
size=M
dispatchable=true
optional=false
release_gating=none
source_refs=source:compiler-proposal.md:L1480
external_requirements=
activation_gate=ORC0
charter=charters/compiler-svelte-compiler/SCP2.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# SCP2 — Compact Svelte compiler structure and canonical template topology

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Compact Svelte compiler structure and canonical template topology. The current owner is **Svelte runtime emitter and assembly paths**. The final and sole owner is **Svelte-owned Default compiler cells over shared compiler substrate**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src/framework/svelte`, `crates/verter_svelte_conformance`, `packages/svelte-runtime-tests`.
- Named API/data boundaries: `SvelteSemanticSnapshot`, `SvelteCompilePlan`, `SvelteTarget`, `SvelteArtifactSet`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **SCP1:** exact current receipt ID and digest for “Canonical Svelte semantic authority convergence”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** build one source-authoritative Svelte structural/topology product before target lowering erases information.
- **Problem:** style matching and target transforms can reconstruct paths from runtime IR, while object-heavy nodes retain repeated strings/vectors and target concerns.
- **Solution and architecture decisions:**
- dense Svelte-owned node/region/expression/scope IDs;

## Acceptance IDs and discriminating proof

- **SCP2-AC1 — sole-owner proof:** add `scp2_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **SCP2-AC2 — positive contract:** add `scp2_publishes_exact_sveltesemanticsnapshot`; assert exact identities, provenance, completeness, and deterministic ordering.
- **SCP2-AC3 — incremental equivalence:** add `scp2_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **SCP2-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
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

Apply `semantic-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three named independent reviewers with assigned lenses, model `gpt-5.6-sol`, effort `ultra`, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1480`

## Reconciled source-plan contract

**Intent:** build one source-authoritative Svelte structural/topology product before target lowering erases information.

**Problem:** style matching and target transforms can reconstruct paths from runtime IR, while object-heavy nodes retain repeated strings/vectors and target concerns.

**Solution and architecture decisions:**

- dense Svelte-owned node/region/expression/scope IDs;
- region-owned `if`, `each`, `await`, `key`, snippet and slot/component structures;
- canonical topology side tables:

  ```text
  parent
  first_child
  next_sibling
  previous_sibling
  preorder_start/end
  region/existence class
  static/dynamic tag/id/class/attribute facts
  snippet definition/render-site edges
  ```

- flat child/attribute/operation/range arenas;
- source fragments/anchors retained separately from target state;
- client/server/style consume the same topology;
- no style semantics depend on runtime lowering retaining accidental geometry.

**Suggested predecessor:** `SCP1`.

**Normative source decomposition:** dense ID/data layout, region lowering, topology, dynamic feature facts, snippet edges, old runtime-IR consumer migration.

**Acceptance:** node access is O(1) by dense ID; style/client/server use one topology; source offset is not node identity; object-size/allocation budgets pass; no target helper/code layout lives in structure.

**Forbidden:** source-offset IDs, duplicated client/server trees, compiler-local topology reconstruction, or Vue-shaped structural operations.

**Deletion/abort:** migrate consumers incrementally but keep one authority; abort shared mechanics that require framework semantic branches.

---

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1480-C6525ABC0FCE

- Kind: `context`
- Source: `compiler-proposal.md:1480-1480`
- Applicability: `SCP2`
- Exact text SHA-256: `c6525abc0fce13593e79d674a19b310c90b53b1f7d1ba541fcda93fcd59977ce`

~~~~markdown
## `SCP2.md` — Compact Svelte compiler structure and canonical template topology
~~~~

### SRC-COMP-L1482-0F72EB7F9387

- Kind: `context`
- Source: `compiler-proposal.md:1482-1482`
- Applicability: `SCP2`
- Exact text SHA-256: `0f72eb7f93873f72bb98c7844e59e002e966c2e5992afb79dc49e548f505d289`

~~~~markdown
**Intent:** build one source-authoritative Svelte structural/topology product before target lowering erases information.
~~~~

### SRC-COMP-L1484-3C9334A82ED7

- Kind: `context`
- Source: `compiler-proposal.md:1484-1484`
- Applicability: `SCP2`
- Exact text SHA-256: `3c9334a82ed7b7b3bf3e9c947fd5de586b58bba11d81be04ad49ab722eb8847a`

~~~~markdown
**Problem:** style matching and target transforms can reconstruct paths from runtime IR, while object-heavy nodes retain repeated strings/vectors and target concerns.
~~~~

### SRC-COMP-L1486-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:1486-1486`
- Applicability: `SCP2`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1488-9FF238255BD3

- Kind: `context`
- Source: `compiler-proposal.md:1488-1488`
- Applicability: `SCP2`
- Exact text SHA-256: `9ff238255bd3137a8f98f2cad64df94eb9b87d24eed5d59066bccf4fcb803172`

~~~~markdown
- dense Svelte-owned node/region/expression/scope IDs;
~~~~

### SRC-COMP-L1489-F8DC534F26F1

- Kind: `context`
- Source: `compiler-proposal.md:1489-1489`
- Applicability: `SCP2`
- Exact text SHA-256: `f8dc534f26f1327a3b967595640771c7e684e8b40e80e39279a67fae406e7609`

~~~~markdown
- region-owned `if`, `each`, `await`, `key`, snippet and slot/component structures;
~~~~

### SRC-COMP-L1490-434DD32BC9BB

- Kind: `context`
- Source: `compiler-proposal.md:1490-1490`
- Applicability: `SCP2`
- Exact text SHA-256: `434dd32bc9bbd13559f2e82caa6d23a2c5800ad33d2b3ea22f9facb28f2750d5`

~~~~markdown
- canonical topology side tables:
~~~~

### SRC-COMP-L1492-F4F9A3A2E686

- Kind: `context`
- Source: `compiler-proposal.md:1492-1501`
- Applicability: `SCP2`
- Exact text SHA-256: `f4f9a3a2e686750f7815cabcf9e728cf2377508de74b409fbd706a95fb7041bd`

~~~~markdown
```text
  parent
  first_child
  next_sibling
  previous_sibling
  preorder_start/end
  region/existence class
  static/dynamic tag/id/class/attribute facts
  snippet definition/render-site edges
  ```
~~~~

### SRC-COMP-L1503-0A5DB4BF6881

- Kind: `context`
- Source: `compiler-proposal.md:1503-1503`
- Applicability: `SCP2`
- Exact text SHA-256: `0a5db4bf688152a6ca892b13de458d43600985a3f258e81f559aaa6962bdfffe`

~~~~markdown
- flat child/attribute/operation/range arenas;
~~~~

### SRC-COMP-L1504-CDE23053695B

- Kind: `context`
- Source: `compiler-proposal.md:1504-1504`
- Applicability: `SCP2`
- Exact text SHA-256: `cde23053695baed63ce3cdab7d1ba4ba1085b16736b1cc9f8bd63d5fd1226fd6`

~~~~markdown
- source fragments/anchors retained separately from target state;
~~~~

### SRC-COMP-L1505-BDE22D0848E6

- Kind: `context`
- Source: `compiler-proposal.md:1505-1505`
- Applicability: `SCP2`
- Exact text SHA-256: `bde22d0848e611a5dba5aa7fa6dc1286f3a19cc3a79e5846a93aed08ec800201`

~~~~markdown
- client/server/style consume the same topology;
~~~~

### SRC-COMP-L1506-C1272562A40C

- Kind: `context`
- Source: `compiler-proposal.md:1506-1506`
- Applicability: `SCP2`
- Exact text SHA-256: `c1272562a40cb60d8e282fe678935eb2b49a5d5d71ceaf88d8df3cb326f00a5a`

~~~~markdown
- no style semantics depend on runtime lowering retaining accidental geometry.
~~~~

### SRC-COMP-L1508-5D1CE4FA2351

- Kind: `context`
- Source: `compiler-proposal.md:1508-1508`
- Applicability: `SCP2`
- Exact text SHA-256: `5d1ce4fa23518e2a2e9f83c3fe4cc011976d9189340d27dedecf5b6e19b2722b`

~~~~markdown
**Suggested predecessor:** `SCP1`.
~~~~

### SRC-COMP-L1510-BA4A0B975A3B

- Kind: `context`
- Source: `compiler-proposal.md:1510-1510`
- Applicability: `SCP2`
- Exact text SHA-256: `ba4a0b975a3b532f926057b24f8564535e12f1fbf74dc4f775cd66d850a8712e`

~~~~markdown
**Suggested subblocks:** dense ID/data layout, region lowering, topology, dynamic feature facts, snippet edges, old runtime-IR consumer migration.
~~~~

### SRC-COMP-L1512-9E4F498B1EC3

- Kind: `acceptance`
- Source: `compiler-proposal.md:1512-1512`
- Applicability: `SCP2`
- Exact text SHA-256: `9e4f498b1ec3b7e58b5c5b15dba3a68ac97abf0920704c4bc9026ec0f27f4280`

~~~~markdown
**Acceptance:** node access is O(1) by dense ID; style/client/server use one topology; source offset is not node identity; object-size/allocation budgets pass; no target helper/code layout lives in structure.
~~~~

### SRC-COMP-L1514-096831512D57

- Kind: `forbidden`
- Source: `compiler-proposal.md:1514-1514`
- Applicability: `SCP2`
- Exact text SHA-256: `096831512d575acbdd3af837c3b664b1d78a1ca0a24fb68c475a50a6632acf5c`

~~~~markdown
**Forbidden:** source-offset IDs, duplicated client/server trees, compiler-local topology reconstruction, or Vue-shaped structural operations.
~~~~

### SRC-COMP-L1516-998863A07119

- Kind: `deletion`
- Source: `compiler-proposal.md:1516-1516`
- Applicability: `SCP2`
- Exact text SHA-256: `998863a0711932cef98be5e2b7e5d0c5d1230c74a8ba031dcb6c2ba41a2c7c1a`

~~~~markdown
**Deletion/abort:** migrate consumers incrementally but keep one authority; abort shared mechanics that require framework semantic branches.
~~~~

### SRC-COMP-L1518-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1518-1518`
- Applicability: `SCP2`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
