<!-- unified-charter-v2
id=SCP5
name=Svelte module compiler for `.svelte.js` and `.svelte.ts`
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
source_refs=source:compiler-proposal.md:L1677
external_requirements=
activation_gate=ORC0
charter=charters/compiler-svelte-compiler/SCP5.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# SCP5 — Svelte module compiler for `.svelte.js` and `.svelte.ts`

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Svelte module compiler for `.svelte.js` and `.svelte.ts`. The current owner is **Svelte runtime emitter and assembly paths**. The final and sole owner is **Svelte-owned Default compiler cells over shared compiler substrate**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src/framework/svelte`, `crates/verter_svelte_conformance`, `packages/svelte-runtime-tests`.
- Named API/data boundaries: `SvelteSemanticSnapshot`, `SvelteCompilePlan`, `SvelteTarget`, `SvelteArtifactSet`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **SCP1:** exact current receipt ID and digest for “Canonical Svelte semantic authority convergence”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** compile module-rune semantics through the JS/TS frontend without forcing module files through the component carrier.
- **Problem:** module compilation is easy to omit or implement with raw-text scanning and does not naturally belong to an SFC frontend.
- **Solution and architecture decisions:**
- OXC JS/TS frontend

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **SCP5-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **SCP5-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **SCP5-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **SCP5-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
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
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_svelte_conformance`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial`, `conformance`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 2/2 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `targeted` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 2 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1677`

## Reconciled source-plan contract

**Intent:** compile module-rune semantics through the JS/TS frontend without forcing module files through the component carrier.

**Problem:** module compilation is easy to omit or implement with raw-text scanning and does not naturally belong to an SFC frontend.

**Solution and architecture decisions:**

```text
OXC JS/TS frontend
    +
Svelte semantic profile/authority
    ↓
Svelte module semantic facts
    ↓
Svelte module target planning/emission
```

OXC remains internal. Module semantics reuse canonical runes/bindings/dependencies but own their target-specific rewriting and artifacts.

**Suggested predecessor:** `SCP1`.

**Normative source decomposition:** module activation/options, rune/module facts, target plan, emission/maps, diagnostics, differential/performance tests.

**Acceptance:** no component frontend or source-string scanner is used; locked module behavior/maps pass; ordinary JS/TS remains unaffected when the Svelte module profile is inactive.

**Forbidden:** SFC wrappers, filename-only semantic activation without the locked contract, external AST output, or duplicated rune analysis.

**Deletion/abort:** delete old module transform paths after parity; keep unsupported cells explicit.

---

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1677-7009EA5C83DF

- Kind: `context`
- Source: `compiler-proposal.md:1677-1677`
- Applicability: `SCP5`
- Exact text SHA-256: `7009ea5c83dfd1c5e9ebf5fc66fe2290e47c524da0f59db722ff459cb4cbd5ad`

~~~~markdown
## `SCP5.md` — Svelte module compiler for `.svelte.js` and `.svelte.ts`
~~~~

### SRC-COMP-L1679-E6C8ACB752DA

- Kind: `context`
- Source: `compiler-proposal.md:1679-1679`
- Applicability: `SCP5`
- Exact text SHA-256: `e6c8acb752da13a7605db99568026eea51f8876b17c8d5aa81dc57127a25380a`

~~~~markdown
**Intent:** compile module-rune semantics through the JS/TS frontend without forcing module files through the component carrier.
~~~~

### SRC-COMP-L1681-4EE977BCFA1A

- Kind: `context`
- Source: `compiler-proposal.md:1681-1681`
- Applicability: `SCP5`
- Exact text SHA-256: `4ee977bcfa1a3378fecc0f03d4b08798b4c7c528d81479547a004c78c7ccb583`

~~~~markdown
**Problem:** module compilation is easy to omit or implement with raw-text scanning and does not naturally belong to an SFC frontend.
~~~~

### SRC-COMP-L1683-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:1683-1683`
- Applicability: `SCP5`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1685-795379E04995

- Kind: `context`
- Source: `compiler-proposal.md:1685-1693`
- Applicability: `SCP5`
- Exact text SHA-256: `795379e04995922912c6aa4e06a0f1758746c6a702b8638c36ec7d457e82eb60`

~~~~markdown
```text
OXC JS/TS frontend
    +
Svelte semantic profile/authority
    ↓
Svelte module semantic facts
    ↓
Svelte module target planning/emission
```
~~~~

### SRC-COMP-L1695-12CE1DB415BC

- Kind: `requirement`
- Source: `compiler-proposal.md:1695-1695`
- Applicability: `SCP5`
- Exact text SHA-256: `12ce1db415bc84a4de150306b83e09f43267c1702e316d8a672b24a09368378c`

~~~~markdown
OXC remains internal. Module semantics reuse canonical runes/bindings/dependencies but own their target-specific rewriting and artifacts.
~~~~

### SRC-COMP-L1697-5D1CE4FA2351

- Kind: `context`
- Source: `compiler-proposal.md:1697-1697`
- Applicability: `SCP5`
- Exact text SHA-256: `5d1ce4fa23518e2a2e9f83c3fe4cc011976d9189340d27dedecf5b6e19b2722b`

~~~~markdown
**Suggested predecessor:** `SCP1`.
~~~~

### SRC-COMP-L1699-04FD191B0F63

- Kind: `context`
- Source: `compiler-proposal.md:1699-1699`
- Applicability: `SCP5`
- Exact text SHA-256: `04fd191b0f63bfa2d36bcaaec9655376f2447fb4af12c6140ec389e8f3cd9a77`

~~~~markdown
**Suggested subblocks:** module activation/options, rune/module facts, target plan, emission/maps, diagnostics, differential/performance tests.
~~~~

### SRC-COMP-L1701-60CA5CF2C141

- Kind: `acceptance`
- Source: `compiler-proposal.md:1701-1701`
- Applicability: `SCP5`
- Exact text SHA-256: `60ca5cf2c1412855c5a9e342d162e3b6f279b000afc0f46c86e503e91a1d7e04`

~~~~markdown
**Acceptance:** no component frontend or source-string scanner is used; locked module behavior/maps pass; ordinary JS/TS remains unaffected when the Svelte module profile is inactive.
~~~~

### SRC-COMP-L1703-5182E671EE15

- Kind: `forbidden`
- Source: `compiler-proposal.md:1703-1703`
- Applicability: `SCP5`
- Exact text SHA-256: `5182e671ee15e4e96eacfa5861daf63dccdaf862a0b2e451bce23bc76c025367`

~~~~markdown
**Forbidden:** SFC wrappers, filename-only semantic activation without the locked contract, external AST output, or duplicated rune analysis.
~~~~

### SRC-COMP-L1705-5ECE75334D75

- Kind: `deletion`
- Source: `compiler-proposal.md:1705-1705`
- Applicability: `SCP5`
- Exact text SHA-256: `5ece75334d7524a997173b30cacff75f3da8bc86ef0ea26ef21060c86ee9887a`

~~~~markdown
**Deletion/abort:** delete old module transform paths after parity; keep unsupported cells explicit.
~~~~

### SRC-COMP-L1707-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1707-1707`
- Applicability: `SCP5`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
