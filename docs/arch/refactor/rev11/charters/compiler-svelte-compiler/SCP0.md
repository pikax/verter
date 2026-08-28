<!-- unified-charter-v2
id=SCP0
name=Exact Svelte Default compiler lock
phase=compiler
train=compiler.svelte-compiler
product=svelte_compiler
kind=lock
semantic_role=delivery
class=compiler
predecessors=CMP5
conditional_predecessors=
owner=compiler.svelte-compiler:Svelte-owned Default compiler cells over shared compiler substrate
conflict_domains=compiler_execution,svelte_product
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
source_refs=source:compiler-proposal.md:L1426
external_requirements=
activation_gate=ORC0
charter=charters/compiler-svelte-compiler/SCP0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# SCP0 — Exact Svelte Default compiler lock

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Exact Svelte Default compiler lock. The current owner is **Svelte runtime emitter and assembly paths**. The final and sole owner is **Svelte-owned Default compiler cells over shared compiler substrate**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src/framework/svelte`, `crates/verter_svelte_conformance`, `packages/svelte-runtime-tests`.
- Named API/data boundaries: `SvelteSemanticSnapshot`, `SvelteCompilePlan`, `SvelteTarget`, `SvelteArtifactSet`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **CMP5:** exact current receipt ID and digest for “Provisional shared compiler-core contract lock”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** freeze one exact Svelte semantic epoch, target contracts, style semantics, module compilation, corpora and performance gates.
- **Problem:** the current experimental compiler cannot define its own acceptance after implementation, and default behavior must distinguish source-language semantics from output cosmetics.
- **Solution and architecture decisions:**
- pin exact release/semantic epoch and DefaultCompilationContractId for client, server and module targets;

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **SCP0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **SCP0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **SCP0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **SCP0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_svelte_conformance/tests`, `packages/svelte-runtime-tests/test`.

## Deletions and forbidden designs

- Delete or structurally reject: **legacy Svelte emitter route**.
- Delete or structurally reject: **per-target prerequisite duplication**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_svelte_conformance`
2. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 3 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, `architecture-specialist`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1426`

## Reconciled source-plan contract

**Intent:** freeze one exact Svelte semantic epoch, target contracts, style semantics, module compilation, corpora and performance gates.

**Problem:** the current experimental compiler cannot define its own acceptance after implementation, and default behavior must distinguish source-language semantics from output cosmetics.

**Solution and architecture decisions:**

- pin exact release/semantic epoch and `DefaultCompilationContractId` for client, server and module targets;
- lock runes/legacy, hydration, diagnostics, CSS pruning/scoping, maps, module surface, and unsupported cells;
- lock `Default` component-local facts and no workspace loading;
- lock official/reference differential, runtime/hydration validators and independent comparator use;
- lock equivalent-work/RSS gates;
- lock deletion scope of the experimental compiler.

**Suggested predecessor:** `CMP5`.

**Normative source decomposition:** release/oracle dossier, behavior matrix, CSS/hydration/module corpus, current-baseline capture, performance lock, independent review.

**Acceptance:** every target/style/diagnostic/map cell has a preimplementation pass rule; unsupported behavior is fail-closed and named.

**Forbidden:** preserving an experimental representation solely because it exists, parser-speed-only goals, or criteria chosen from produced output.

**Deletion/abort:** no code; rescope rather than silently approximate unsupported semantics.

---

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1426-CC0C62C38EA6

- Kind: `requirement`
- Source: `compiler-proposal.md:1426-1426`
- Applicability: `SCP0`
- Exact text SHA-256: `cc0c62c38ea6eb81f17fa5ca075f4ef9b45aa41bad81b66461aac5c608445c88`

~~~~markdown
## `SCP0.md` — Exact Svelte Default compiler lock
~~~~

### SRC-COMP-L1428-E3CC9136BA9B

- Kind: `requirement`
- Source: `compiler-proposal.md:1428-1428`
- Applicability: `SCP0`
- Exact text SHA-256: `e3cc9136ba9b37c65852ac72abce2daf41d312be560042bf10feed45b0f80284`

~~~~markdown
**Intent:** freeze one exact Svelte semantic epoch, target contracts, style semantics, module compilation, corpora and performance gates.
~~~~

### SRC-COMP-L1430-35A936533B02

- Kind: `acceptance`
- Source: `compiler-proposal.md:1430-1430`
- Applicability: `SCP0`
- Exact text SHA-256: `35a936533b029e5d28605dacaffc5961f8c432a05238919a06f283fb271a97b7`

~~~~markdown
**Problem:** the current experimental compiler cannot define its own acceptance after implementation, and default behavior must distinguish source-language semantics from output cosmetics.
~~~~

### SRC-COMP-L1432-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:1432-1432`
- Applicability: `SCP0`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1434-EE05C498DD11

- Kind: `requirement`
- Source: `compiler-proposal.md:1434-1434`
- Applicability: `SCP0`
- Exact text SHA-256: `ee05c498dd118cd0b6d001560447be634bc173912ff0d0af50992488e3206567`

~~~~markdown
- pin exact release/semantic epoch and `DefaultCompilationContractId` for client, server and module targets;
~~~~

### SRC-COMP-L1435-76A2DFC6065B

- Kind: `context`
- Source: `compiler-proposal.md:1435-1435`
- Applicability: `SCP0`
- Exact text SHA-256: `76a2dfc6065bd1cb1f1875a3b0e3184ebf686959774372e55807ffc44ffa7667`

~~~~markdown
- lock runes/legacy, hydration, diagnostics, CSS pruning/scoping, maps, module surface, and unsupported cells;
~~~~

### SRC-COMP-L1436-8534E145BF11

- Kind: `context`
- Source: `compiler-proposal.md:1436-1436`
- Applicability: `SCP0`
- Exact text SHA-256: `8534e145bf11e50f16c8190fe7c6c52b98acb69989c68395c7c800d68078a057`

~~~~markdown
- lock `Default` component-local facts and no workspace loading;
~~~~

### SRC-COMP-L1437-D66B66F946E3

- Kind: `context`
- Source: `compiler-proposal.md:1437-1437`
- Applicability: `SCP0`
- Exact text SHA-256: `d66b66f946e3214fc25d658e735f6609d83a5786c1f09402ca40704693a8bb1d`

~~~~markdown
- lock official/reference differential, runtime/hydration validators and independent comparator use;
~~~~

### SRC-COMP-L1438-EE23035BDC7C

- Kind: `context`
- Source: `compiler-proposal.md:1438-1438`
- Applicability: `SCP0`
- Exact text SHA-256: `ee23035bdc7c93a30af09224f617a80c3d15ebbf8a9cb598ab1b5669df4839aa`

~~~~markdown
- lock equivalent-work/RSS gates;
~~~~

### SRC-COMP-L1439-6E1EF5EDB6EC

- Kind: `deletion`
- Source: `compiler-proposal.md:1439-1439`
- Applicability: `SCP0`
- Exact text SHA-256: `6e1ef5edb6ec0156bbb94b3d25eb290aaef97d4e43234c1bd1f1b37bc04c46b5`

~~~~markdown
- lock deletion scope of the experimental compiler.
~~~~

### SRC-COMP-L1441-5F57552C6CA5

- Kind: `context`
- Source: `compiler-proposal.md:1441-1441`
- Applicability: `SCP0`
- Exact text SHA-256: `5f57552c6ca5d7f886bf237de4d0b55692ed5ce22d73bc3acb98e3b5bc163688`

~~~~markdown
**Suggested predecessor:** `CMP5`.
~~~~

### SRC-COMP-L1443-4E34E0B3173E

- Kind: `context`
- Source: `compiler-proposal.md:1443-1443`
- Applicability: `SCP0`
- Exact text SHA-256: `4e34e0b3173ee8b7450b2fe32e122544c7b41247c0d62c813ead74ec757040eb`

~~~~markdown
**Suggested subblocks:** release/oracle dossier, behavior matrix, CSS/hydration/module corpus, current-baseline capture, performance lock, independent review.
~~~~

### SRC-COMP-L1445-17C7FD96FB55

- Kind: `acceptance`
- Source: `compiler-proposal.md:1445-1445`
- Applicability: `SCP0`
- Exact text SHA-256: `17c7fd96fb5554d23303cbab0d5a6ff6fc5341a052a158ec0c26ea9350bc3031`

~~~~markdown
**Acceptance:** every target/style/diagnostic/map cell has a preimplementation pass rule; unsupported behavior is fail-closed and named.
~~~~

### SRC-COMP-L1447-65BD2D32E2FC

- Kind: `forbidden`
- Source: `compiler-proposal.md:1447-1447`
- Applicability: `SCP0`
- Exact text SHA-256: `65bd2d32e2fc0390cc3d99047b5d13e6fb48bec707eae26cfb3e0484d358ea2b`

~~~~markdown
**Forbidden:** preserving an experimental representation solely because it exists, parser-speed-only goals, or criteria chosen from produced output.
~~~~

### SRC-COMP-L1449-D808961E0562

- Kind: `deletion`
- Source: `compiler-proposal.md:1449-1449`
- Applicability: `SCP0`
- Exact text SHA-256: `d808961e0562c51fa80e6cc0334e6775037a2de9951d13da83bc068fde68c9ec`

~~~~markdown
**Deletion/abort:** no code; rescope rather than silently approximate unsupported semantics.
~~~~

### SRC-COMP-L1451-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1451-1451`
- Applicability: `SCP0`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
