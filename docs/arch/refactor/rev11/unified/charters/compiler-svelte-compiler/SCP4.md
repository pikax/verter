<!-- unified-charter-v2
id=SCP4
name=Svelte server Default compiler
phase=compiler
train=compiler.svelte-compiler
product=svelte_compiler
kind=implementation
semantic_role=delivery
class=compiler
predecessors=SCP2,SST2
conditional_predecessors=
owner=compiler.svelte-compiler:Svelte-owned Default compiler cells over shared compiler substrate
conflict_domains=compiler_execution,svelte_product
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
source_refs=source:compiler-proposal.md:L1651
external_requirements=
activation_gate=ORC0
charter=charters/compiler-svelte-compiler/SCP4.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# SCP4 — Svelte server Default compiler

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Svelte server Default compiler. The current owner is **Svelte runtime emitter and assembly paths**. The final and sole owner is **Svelte-owned Default compiler cells over shared compiler substrate**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src/framework/svelte`, `crates/verter_svelte_conformance`, `packages/svelte-runtime-tests`.
- Named API/data boundaries: `SvelteSemanticSnapshot`, `SvelteCompilePlan`, `SvelteTarget`, `SvelteArtifactSet`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **SCP2:** exact current receipt ID and digest for “Compact Svelte compiler structure and canonical template topology”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **SST2:** exact current receipt ID and digest for “Svelte style-match facts and adaptive matcher cutover”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** implement server compilation with shared semantics/structure/style and zero client-effect work.
- **Problem:** server compilation can inherit client data structures or repeat shared analysis.
- **Solution and architecture decisions:**
- monomorphic Svelte+server executor;

## Acceptance IDs and discriminating proof

- **SCP4-AC1 — sole-owner proof:** add `scp4_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **SCP4-AC2 — positive contract:** add `scp4_publishes_exact_sveltesemanticsnapshot`; assert exact identities, provenance, completeness, and deterministic ordering.
- **SCP4-AC3 — incremental equivalence:** add `scp4_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **SCP4-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
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

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three fresh distinct harness reviewers with assigned lenses, deterministic low|medium|high effort, exact task/provider/model bindings, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1651`

## Reconciled source-plan contract

**Intent:** implement server compilation with shared semantics/structure/style and zero client-effect work.

**Problem:** server compilation can inherit client data structures or repeat shared analysis.

**Solution and architecture decisions:**

- monomorphic Svelte+server executor;
- consume shared structure and style facts;
- segment-oriented server emission and minimal server plan;
- zero client effects, DOM plan, transitions/actions/hydration work;
- share prerequisites with client when both requested.

**Suggested predecessors:** `SCP2`, `SST2`.

**Normative source decomposition:** server text/escaping, elements/components/slots/blocks, style/head/module relations, maps, client+server sharing, performance proof.

**Acceptance:** server behavior/maps/CSS pass; client target counters are zero; combined client/server requests do not repeat parse/semantic/style/topology work.

**Forbidden:** client graph reuse by convenience, duplicate style matching, or full server target tree without evidence.

**Deletion/abort:** old server path deleted at `SCP6` after parity.

---

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1651-11BAA9D3F797

- Kind: `context`
- Source: `compiler-proposal.md:1651-1651`
- Applicability: `SCP4`
- Exact text SHA-256: `11baa9d3f797d1eb215958a56f4e70942a5a55ee2323ee70fb51d98f397f0bd7`

~~~~markdown
## `SCP4.md` — Svelte server Default compiler
~~~~

### SRC-COMP-L1653-150E9718456F

- Kind: `context`
- Source: `compiler-proposal.md:1653-1653`
- Applicability: `SCP4`
- Exact text SHA-256: `150e9718456f5068c717d10de9366a779f0279ac95965d509246070ef0cff14f`

~~~~markdown
**Intent:** implement server compilation with shared semantics/structure/style and zero client-effect work.
~~~~

### SRC-COMP-L1655-7F80585FF698

- Kind: `context`
- Source: `compiler-proposal.md:1655-1655`
- Applicability: `SCP4`
- Exact text SHA-256: `7f80585ff698cd7cddaa4fd2396cf7584a40c2c3118e8c6401283fdcd1b97486`

~~~~markdown
**Problem:** server compilation can inherit client data structures or repeat shared analysis.
~~~~

### SRC-COMP-L1657-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:1657-1657`
- Applicability: `SCP4`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1659-AE18A21CDB86

- Kind: `context`
- Source: `compiler-proposal.md:1659-1659`
- Applicability: `SCP4`
- Exact text SHA-256: `ae18a21cdb861957a86e64a6f837604ac32af501173955652e5060903b10c347`

~~~~markdown
- monomorphic Svelte+server executor;
~~~~

### SRC-COMP-L1660-F90898FFD4D7

- Kind: `context`
- Source: `compiler-proposal.md:1660-1660`
- Applicability: `SCP4`
- Exact text SHA-256: `f90898ffd4d716f59555dce17944a5f2874ca8d1db5249e79a78bbab617085d7`

~~~~markdown
- consume shared structure and style facts;
~~~~

### SRC-COMP-L1661-AFC4C6FAA382

- Kind: `context`
- Source: `compiler-proposal.md:1661-1661`
- Applicability: `SCP4`
- Exact text SHA-256: `afc4c6faa382625e93c87dcadf34ae271994d17d3bad20a1d4058bfe6704a597`

~~~~markdown
- segment-oriented server emission and minimal server plan;
~~~~

### SRC-COMP-L1662-2A68551ED4AB

- Kind: `context`
- Source: `compiler-proposal.md:1662-1662`
- Applicability: `SCP4`
- Exact text SHA-256: `2a68551ed4ab58d8b0ec3a8768ad914a9ffad993cfc8f9989efc3b11e836a9d3`

~~~~markdown
- zero client effects, DOM plan, transitions/actions/hydration work;
~~~~

### SRC-COMP-L1663-5EBB3B0FECA4

- Kind: `context`
- Source: `compiler-proposal.md:1663-1663`
- Applicability: `SCP4`
- Exact text SHA-256: `5ebb3b0feca4e17f9be52b376ccef5cd014ba7f3b60737d87c36b332fe95c15d`

~~~~markdown
- share prerequisites with client when both requested.
~~~~

### SRC-COMP-L1665-E5ED001D2593

- Kind: `context`
- Source: `compiler-proposal.md:1665-1665`
- Applicability: `SCP4`
- Exact text SHA-256: `e5ed001d25934b7b6527d41b3a63d1dad6b3836f3f80f5fcb65503d87eca6f57`

~~~~markdown
**Suggested predecessors:** `SCP2`, `SST2`.
~~~~

### SRC-COMP-L1667-A243F43CC49E

- Kind: `context`
- Source: `compiler-proposal.md:1667-1667`
- Applicability: `SCP4`
- Exact text SHA-256: `a243f43cc49ea85182abe660205ac6c85e98ef7d3f549a350393378d323f885d`

~~~~markdown
**Suggested subblocks:** server text/escaping, elements/components/slots/blocks, style/head/module relations, maps, client+server sharing, performance proof.
~~~~

### SRC-COMP-L1669-1FBB5C5117ED

- Kind: `acceptance`
- Source: `compiler-proposal.md:1669-1669`
- Applicability: `SCP4`
- Exact text SHA-256: `1fbb5c5117ed4738b15fb874566f832d019ca2b1013bc1a21773ad16def8e120`

~~~~markdown
**Acceptance:** server behavior/maps/CSS pass; client target counters are zero; combined client/server requests do not repeat parse/semantic/style/topology work.
~~~~

### SRC-COMP-L1671-3CFA79B2A96A

- Kind: `forbidden`
- Source: `compiler-proposal.md:1671-1671`
- Applicability: `SCP4`
- Exact text SHA-256: `3cfa79b2a96a25b702d0696a1fdc9afe877cd519737e05019931d86df154050c`

~~~~markdown
**Forbidden:** client graph reuse by convenience, duplicate style matching, or full server target tree without evidence.
~~~~

### SRC-COMP-L1673-A9A147E78EEB

- Kind: `deletion`
- Source: `compiler-proposal.md:1673-1673`
- Applicability: `SCP4`
- Exact text SHA-256: `a9a147e78eeb3e0d3cbd4c207dc628bbeb101f48be32592d98cad73d9aa4a904`

~~~~markdown
**Deletion/abort:** old server path deleted at `SCP6` after parity.
~~~~

### SRC-COMP-L1675-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1675-1675`
- Applicability: `SCP4`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
