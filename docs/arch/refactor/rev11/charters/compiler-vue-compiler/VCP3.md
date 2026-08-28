<!-- unified-charter-v2
id=VCP3
name=Vue VDOM Default compiler
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
source_refs=source:compiler-proposal.md:L1287
external_requirements=
activation_gate=ORC0
charter=charters/compiler-vue-compiler/VCP3.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# VCP3 — Vue VDOM Default compiler

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Vue VDOM Default compiler. The current owner is **Vue runtime emitter and assembly paths**. The final and sole owner is **Vue-owned Default compiler cells over shared compiler substrate**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src/framework/vue`, `crates/verter_vue_conformance`, `packages/vue-conformance-oracle`.
- Named API/data boundaries: `VueSemanticSnapshot`, `VueCompilePlan`, `VueTarget`, `VueArtifactSet`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **VCP2:** exact current receipt ID and digest for “Compact Vue compiler structure and canonical template topology”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **VST0:** exact current receipt ID and digest for “Vue framework style semantics and scope plan”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** implement the primary Vue runtime target on the new semantic and structural authorities.
- **Problem:** target code can rediscover semantic facts, dynamically dispatch per node, allocate whole target trees, and mix maps/emission decisions.
- **Solution and architecture decisions:**
- monomorphic Vue+VDOM executor;

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **VCP3-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **VCP3-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **VCP3-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **VCP3-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
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
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_vue_conformance`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial`, `conformance`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 2/2 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `targeted` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 2 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1287`

## Reconciled source-plan contract

**Intent:** implement the primary Vue runtime target on the new semantic and structural authorities.

**Problem:** target code can rediscover semantic facts, dynamically dispatch per node, allocate whole target trees, and mix maps/emission decisions.

**Solution and architecture decisions:**

- monomorphic Vue+VDOM executor;
- sparse target plan for patch classes, dynamic props, hoists, cache slots, helpers and target diagnostics;
- use `Default` canonical component-local facts, including stronger cheap alias-proven reactivity where safe;
- no SSR/Vapor/effect/style-query work;
- segmented emission and map/no-map specialization;
- exact runtime/module/map contract from `VCP0`.

**Suggested predecessors:** `VCP2`, `VST0`.

**Normative source decomposition:** element/text/interpolation, directives/bindings/events, components/slots/control flow, patch/hoist/cache planning, emission/maps, conformance/performance closure.

**Acceptance:** all locked VDOM cells pass runtime and map validators; no compiler-local semantic rederivation; no per-node dynamic dispatch; VDOM/no-map work ledger contains zero SSR/Vapor/VST1 work.

**Forbidden:** cloning the structural tree into a full VDOM AST without evidence, output-only tests, or delaying known correctness defects to later targets.

**Deletion/abort:** delete the old VDOM path atomically only at `VCP6`/`VCP7`; retain adapters until then.

---

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1287-65234AAE74C1

- Kind: `context`
- Source: `compiler-proposal.md:1287-1287`
- Applicability: `VCP3`
- Exact text SHA-256: `65234aae74c1f02c18abe5fb7d837212615688828dd39149c8672b51d5b136de`

~~~~markdown
## `VCP3.md` — Vue VDOM Default compiler
~~~~

### SRC-COMP-L1289-7BFAEEA664C5

- Kind: `context`
- Source: `compiler-proposal.md:1289-1289`
- Applicability: `VCP3`
- Exact text SHA-256: `7bfaeea664c5910d5016cc9aeaaf52df70e65f9337f642e5262fc2bb2e6de525`

~~~~markdown
**Intent:** implement the primary Vue runtime target on the new semantic and structural authorities.
~~~~

### SRC-COMP-L1291-A5E1EA79D8EC

- Kind: `context`
- Source: `compiler-proposal.md:1291-1291`
- Applicability: `VCP3`
- Exact text SHA-256: `a5e1ea79d8ec220e5dcee47949e62baa0c23287ceb394ba55d125bbf38f2523e`

~~~~markdown
**Problem:** target code can rediscover semantic facts, dynamically dispatch per node, allocate whole target trees, and mix maps/emission decisions.
~~~~

### SRC-COMP-L1293-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:1293-1293`
- Applicability: `VCP3`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1295-8A8747FA32DE

- Kind: `context`
- Source: `compiler-proposal.md:1295-1295`
- Applicability: `VCP3`
- Exact text SHA-256: `8a8747fa32de4426c77910aebc2d52568452887be86856f277cfe44cf32798ad`

~~~~markdown
- monomorphic Vue+VDOM executor;
~~~~

### SRC-COMP-L1296-3781BC6F95B1

- Kind: `context`
- Source: `compiler-proposal.md:1296-1296`
- Applicability: `VCP3`
- Exact text SHA-256: `3781bc6f95b128a8d169be508ec934592b58a312f814d499ad4e4b4d4d2a9562`

~~~~markdown
- sparse target plan for patch classes, dynamic props, hoists, cache slots, helpers and target diagnostics;
~~~~

### SRC-COMP-L1297-1DDC4230E787

- Kind: `context`
- Source: `compiler-proposal.md:1297-1297`
- Applicability: `VCP3`
- Exact text SHA-256: `1ddc4230e7879095cf7a8c3be5eb20c2c7fdf3a85dc57433d6bbbbd3bab1eae6`

~~~~markdown
- use `Default` canonical component-local facts, including stronger cheap alias-proven reactivity where safe;
~~~~

### SRC-COMP-L1298-5B5454FC5B2C

- Kind: `context`
- Source: `compiler-proposal.md:1298-1298`
- Applicability: `VCP3`
- Exact text SHA-256: `5b5454fc5b2c7bfedfb93566ea1dfb1f9fbf9edb77df14a21f77d4ef73a0eb88`

~~~~markdown
- no SSR/Vapor/effect/style-query work;
~~~~

### SRC-COMP-L1299-F425A5E5BC0D

- Kind: `context`
- Source: `compiler-proposal.md:1299-1299`
- Applicability: `VCP3`
- Exact text SHA-256: `f425a5e5bc0d67a015d1176b79006dab775928ed823e694b170fcc7a6f5e1565`

~~~~markdown
- segmented emission and map/no-map specialization;
~~~~

### SRC-COMP-L1300-88BCDDD2E573

- Kind: `requirement`
- Source: `compiler-proposal.md:1300-1300`
- Applicability: `VCP3`
- Exact text SHA-256: `88bcddd2e573c5c6e15bd00356d30afe514b7f78b46b55326acb78357f9960a7`

~~~~markdown
- exact runtime/module/map contract from `VCP0`.
~~~~

### SRC-COMP-L1302-1E4BF1D3A01D

- Kind: `context`
- Source: `compiler-proposal.md:1302-1302`
- Applicability: `VCP3`
- Exact text SHA-256: `1e4bf1d3a01d79737e78ee77dc804362fd1e31beac1e93e294a66d41aa363639`

~~~~markdown
**Suggested predecessors:** `VCP2`, `VST0`.
~~~~

### SRC-COMP-L1304-0E0870DAEBE8

- Kind: `requirement`
- Source: `compiler-proposal.md:1304-1304`
- Applicability: `VCP3`
- Exact text SHA-256: `0e0870daebe8a4dafc9542915950de07280909350f67fc167bdc465f2db5cda6`

~~~~markdown
**Suggested subblocks:** element/text/interpolation, directives/bindings/events, components/slots/control flow, patch/hoist/cache planning, emission/maps, conformance/performance closure.
~~~~

### SRC-COMP-L1306-B946285742B5

- Kind: `acceptance`
- Source: `compiler-proposal.md:1306-1306`
- Applicability: `VCP3`
- Exact text SHA-256: `b946285742b5869e0c35dd996f882704f055cab743b9ef53ad49dce075ba0433`

~~~~markdown
**Acceptance:** all locked VDOM cells pass runtime and map validators; no compiler-local semantic rederivation; no per-node dynamic dispatch; VDOM/no-map work ledger contains zero SSR/Vapor/VST1 work.
~~~~

### SRC-COMP-L1308-09E07AE2894F

- Kind: `forbidden`
- Source: `compiler-proposal.md:1308-1308`
- Applicability: `VCP3`
- Exact text SHA-256: `09e07ae2894fd4ec4d052e119d715183b79fabc1c512d45019a6085bab985565`

~~~~markdown
**Forbidden:** cloning the structural tree into a full VDOM AST without evidence, output-only tests, or delaying known correctness defects to later targets.
~~~~

### SRC-COMP-L1310-966C4B1BA5D5

- Kind: `deletion`
- Source: `compiler-proposal.md:1310-1310`
- Applicability: `VCP3`
- Exact text SHA-256: `966c4b1ba5d5560bb301cb2cab2e4e4d29afc598354d569e3aa818f458a4758e`

~~~~markdown
**Deletion/abort:** delete the old VDOM path atomically only at `VCP6`/`VCP7`; retain adapters until then.
~~~~

### SRC-COMP-L1312-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1312-1312`
- Applicability: `VCP3`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
