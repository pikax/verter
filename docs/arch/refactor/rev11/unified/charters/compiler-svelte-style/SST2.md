<!-- unified-charter-v2
id=SST2
name=Svelte style-match facts and adaptive matcher cutover
phase=compiler
train=compiler.svelte-style
product=svelte_style
kind=cutover
semantic_role=delivery
class=compiler
predecessors=SST1
conditional_predecessors=
owner=compiler.svelte-style:Svelte-owned adaptive matcher over canonical CSS/template facts
conflict_domains=style_semantics,svelte_product
resource_class=rust-mixed
review_profile=public-3
gate_profile=targeted-domain
size=M
dispatchable=true
optional=false
release_gating=none
source_refs=source:compiler-proposal.md:L1587
external_requirements=
activation_gate=ORC0
charter=charters/compiler-svelte-style/SST2.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# SST2 — Svelte style-match facts and adaptive matcher cutover

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Svelte style-match facts and adaptive matcher cutover. The current owner is **Svelte style matching and source-stage glue**. The final and sole owner is **Svelte-owned adaptive matcher over canonical CSS/template facts**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src/framework/svelte`, `crates/verter_css_syntax/src`.
- Named API/data boundaries: `SvelteStylePlan`, `CandidateIndex`, `StyleMatchFact`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **SST1:** exact current receipt ID and digest for “Svelte selector query plan and candidate-index architecture”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** publish selector applicability/scoping/pruning facts once for compiler, lint, IDE and metadata and delete compiler-local matcher ownership.
- **Problem:** multiple consumers can repeat matching or retain heavy witness/path data; uncertain selectors can be pruned unsafely.
- **Solution and architecture decisions:**
- produce compact SvelteStyleMatchFacts:

## Acceptance IDs and discriminating proof

- **SST2-AC1 — sole-owner proof:** add `sst2_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **SST2-AC2 — positive contract:** add `sst2_publishes_exact_sveltestyleplan`; assert exact identities, provenance, completeness, and deterministic ordering.
- **SST2-AC3 — incremental equivalence:** add `sst2_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **SST2-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_compiler/tests`, `crates/verter_css_syntax/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **Svelte-local CSS parser**.
- Delete or structurally reject: **unbounded selector scan**.
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

1. `cargo nextest run -p verter_compiler -p verter_css_syntax`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `public-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three named independent reviewers with assigned lenses, model `gpt-5.6-sol`, effort `ultra`, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1587`

## Reconciled source-plan contract

**Intent:** publish selector applicability/scoping/pruning facts once for compiler, lint, IDE and metadata and delete compiler-local matcher ownership.

**Problem:** multiple consumers can repeat matching or retain heavy witness/path data; uncertain selectors can be pruned unsafely.

**Solution and architecture decisions:**

- produce compact `SvelteStyleMatchFacts`:

  ```text
  selector_use: Yes | Maybe | No
  scoped_template_nodes: dense bitset
  scoped_selector_compounds: dense bitset
  uncertainty reasons: sparse
  witnesses: optional sparse arena
  ```

- choose direct/indexed strategy once per component with the locked cost model;
- exact verifier walks complete selector semantics right-to-left;
- only `No` permits pruning;
- `PruneOnly`, `ScopePlan`, `Diagnostics`, and `ConformanceTrace` demand products materialize different data;
- client and server requested together reuse one style-match product;
- detailed witnesses are absent from production compile unless demanded.

**Suggested predecessor:** `SST1`.

**Normative source decomposition:** fact schema, exact verifier integration, scope/prune products, diagnostic witnesses, consumer cutover, old matcher/index deletion and performance terminal.

**Acceptance:** no pruning false negatives across the locked corpus; `Maybe` always fails open; client/server/lint/IDE share one fact basis; `PruneOnly` materializes zero witnesses; old runtime-IR matcher authority is deleted.

**Forbidden:** `Maybe` pruning, target-specific repeated matching, witness strings in dense facts, or hidden full element scans in indexed mode.

**Deletion/abort:** this is the sole Svelte matcher cutover/deletion owner; revert to direct exact matching rather than weaken correctness.

---

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1587-2560056182E3

- Kind: `context`
- Source: `compiler-proposal.md:1587-1587`
- Applicability: `SST2`
- Exact text SHA-256: `2560056182e3e46c38c567056c402e702bc2463a4124c1bec430368fdc7cf841`

~~~~markdown
## `SST2.md` — Svelte style-match facts and adaptive matcher cutover
~~~~

### SRC-COMP-L1589-88F961DB7929

- Kind: `deletion`
- Source: `compiler-proposal.md:1589-1589`
- Applicability: `SST2`
- Exact text SHA-256: `88f961db79296002cb43b02d9246dd7bc81f71b288dc19bca54ba9b8c485a932`

~~~~markdown
**Intent:** publish selector applicability/scoping/pruning facts once for compiler, lint, IDE and metadata and delete compiler-local matcher ownership.
~~~~

### SRC-COMP-L1591-2F711EC80F64

- Kind: `context`
- Source: `compiler-proposal.md:1591-1591`
- Applicability: `SST2`
- Exact text SHA-256: `2f711ec80f641afd3d3ab76a5766b8a94201fb5e2ceb935bc81ce9f48077d43a`

~~~~markdown
**Problem:** multiple consumers can repeat matching or retain heavy witness/path data; uncertain selectors can be pruned unsafely.
~~~~

### SRC-COMP-L1593-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:1593-1593`
- Applicability: `SST2`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1595-37D6CA828AD7

- Kind: `context`
- Source: `compiler-proposal.md:1595-1595`
- Applicability: `SST2`
- Exact text SHA-256: `37d6ca828ad770ab7f910e55450db6b942b27b306967dc20c901c02076e490f0`

~~~~markdown
- produce compact `SvelteStyleMatchFacts`:
~~~~

### SRC-COMP-L1597-1F5400E380F8

- Kind: `context`
- Source: `compiler-proposal.md:1597-1603`
- Applicability: `SST2`
- Exact text SHA-256: `1f5400e380f8d67153f12845f91e14c55c236381dee391387d24a108782100ca`

~~~~markdown
```text
  selector_use: Yes | Maybe | No
  scoped_template_nodes: dense bitset
  scoped_selector_compounds: dense bitset
  uncertainty reasons: sparse
  witnesses: optional sparse arena
  ```
~~~~

### SRC-COMP-L1605-C9E29AED7F06

- Kind: `context`
- Source: `compiler-proposal.md:1605-1605`
- Applicability: `SST2`
- Exact text SHA-256: `c9e29aed7f061122a8a6ebe530604791658191a34c91e8cf71b7640d52a05c0e`

~~~~markdown
- choose direct/indexed strategy once per component with the locked cost model;
~~~~

### SRC-COMP-L1606-CA74E867AE8A

- Kind: `requirement`
- Source: `compiler-proposal.md:1606-1606`
- Applicability: `SST2`
- Exact text SHA-256: `ca74e867ae8afbeff94b6465e968bcbd376a286ff13672a855399e3fa1f23391`

~~~~markdown
- exact verifier walks complete selector semantics right-to-left;
~~~~

### SRC-COMP-L1607-C3D7E53FB3D7

- Kind: `requirement`
- Source: `compiler-proposal.md:1607-1607`
- Applicability: `SST2`
- Exact text SHA-256: `c3d7e53fb3d761cce415648471952e4458a79fed4d4152e7207050c6cb31a6dd`

~~~~markdown
- only `No` permits pruning;
~~~~

### SRC-COMP-L1608-D64842911428

- Kind: `context`
- Source: `compiler-proposal.md:1608-1608`
- Applicability: `SST2`
- Exact text SHA-256: `d64842911428fcc7e0eee59852e116970e4b650f81787e2f2888b6edbf8131e3`

~~~~markdown
- `PruneOnly`, `ScopePlan`, `Diagnostics`, and `ConformanceTrace` demand products materialize different data;
~~~~

### SRC-COMP-L1609-8D796A60C401

- Kind: `context`
- Source: `compiler-proposal.md:1609-1609`
- Applicability: `SST2`
- Exact text SHA-256: `8d796a60c4019b12b09808fc83a1d7344e91c4320e8ba979bee887ef2d787b91`

~~~~markdown
- client and server requested together reuse one style-match product;
~~~~

### SRC-COMP-L1610-19C76A6AD01E

- Kind: `context`
- Source: `compiler-proposal.md:1610-1610`
- Applicability: `SST2`
- Exact text SHA-256: `19c76a6ad01e641c805db8d5e80b413f0c937ec67b03cf2f7e8cb7136a3cdb3d`

~~~~markdown
- detailed witnesses are absent from production compile unless demanded.
~~~~

### SRC-COMP-L1612-5F987FE04C82

- Kind: `context`
- Source: `compiler-proposal.md:1612-1612`
- Applicability: `SST2`
- Exact text SHA-256: `5f987fe04c82fa2737016d0f72f8caaf1e38cf610e6f7593d616f8a3c614f945`

~~~~markdown
**Suggested predecessor:** `SST1`.
~~~~

### SRC-COMP-L1614-0DD3E74C838F

- Kind: `deletion`
- Source: `compiler-proposal.md:1614-1614`
- Applicability: `SST2`
- Exact text SHA-256: `0dd3e74c838fd6d5afad21a740d09ed2fa819461e031d38717ccc084687a28fc`

~~~~markdown
**Suggested subblocks:** fact schema, exact verifier integration, scope/prune products, diagnostic witnesses, consumer cutover, old matcher/index deletion and performance terminal.
~~~~

### SRC-COMP-L1616-5FAD3B4C80C0

- Kind: `deletion`
- Source: `compiler-proposal.md:1616-1616`
- Applicability: `SST2`
- Exact text SHA-256: `5fad3b4c80c01b6efdc730698daf0efc13bbfc639951b5df7be7cdc22f06825f`

~~~~markdown
**Acceptance:** no pruning false negatives across the locked corpus; `Maybe` always fails open; client/server/lint/IDE share one fact basis; `PruneOnly` materializes zero witnesses; old runtime-IR matcher authority is deleted.
~~~~

### SRC-COMP-L1618-DF36815A2261

- Kind: `forbidden`
- Source: `compiler-proposal.md:1618-1618`
- Applicability: `SST2`
- Exact text SHA-256: `df36815a22612f61d6460dd148dfd4a5d16b33f2489c06cdc20550243f776878`

~~~~markdown
**Forbidden:** `Maybe` pruning, target-specific repeated matching, witness strings in dense facts, or hidden full element scans in indexed mode.
~~~~

### SRC-COMP-L1620-A67AEC9CB335

- Kind: `deletion`
- Source: `compiler-proposal.md:1620-1620`
- Applicability: `SST2`
- Exact text SHA-256: `a67aec9cb335e8a19a846cce5fad4c3564a4ce29600e8d4b5b7a57ba83cf66bb`

~~~~markdown
**Deletion/abort:** this is the sole Svelte matcher cutover/deletion owner; revert to direct exact matching rather than weaken correctness.
~~~~

### SRC-COMP-L1622-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1622-1622`
- Applicability: `SST2`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
