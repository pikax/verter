<!-- unified-charter-v2
id=CCA0
name=Compiler authority, policy, demand, and admission constitution
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=contract
semantic_role=delivery
class=compiler
predecessors=ORC0,B3,B6,C1
owner=compiler.compiler-bridge:verter_compiler capability traits plus immutable registration catalog
conflict_domains=compiler_execution
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
external_requirements=
charter=charters/compiler-compiler-bridge/CCA0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA0 — Compiler authority, policy, demand, and admission constitution

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Compiler authority, policy, demand, and admission constitution. The current owner is **combined carrier compiler registry and host compile routes**. The final and sole owner is **verter_compiler capability traits plus immutable registration catalog**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src`, `crates/verter_session/src/host_compile.rs`, `packages/unplugin/src/core/compiler.ts`.
- Named API/data boundaries: `CarrierFrontend`, `FrameworkSemanticAuthority`, `ProjectionBackend`, `RuntimeCompiler`, `FrameworkHostIntegration`, `CompileArtifactSet`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **ORC0:** implemented ledger row for “Trusted implementation-ledger cutover”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **B3:** implemented ledger row for “Canonical typed compiler request and prerequisite planner”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **B6:** implemented ledger row for “PreparedCarrier direct batch and direct-core closure”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **C1:** implemented ledger row for “ModuleResolverCore convergence and non-flow semantic basis”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- **Intent:** lock the compiler authority boundaries and policy semantics before C2 seals the staged compile facade.
- **Problem:** the current carrier/compiler seam can still conflate parsing, framework semantics, IDE projection, runtime compilation, module assembly, and host integration. The compiler policy lacks a stable meaning, and the demand/admission order can allow dup
- **Solution and architecture decisions:**
- ratify the five authorities:

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **CCA0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **CCA0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **CCA0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **CCA0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_compiler/tests`, `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete or structurally reject: **combined compiler registry**.
- Delete or structurally reject: **mixed framework/options bucket**.
- Delete or structurally reject: **tooling-only runtime stubs**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_session`
2. Run every final command in the bound `docs-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch adds one `[[implemented]]` row to `authority/state/implemented.toml` with the node ID, planned squash commit message, approximate date with timezone, and optional pull-request number. Row presence is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Reconciled source-plan contract

**Intent:** lock the compiler authority boundaries and policy semantics before C2 seals the staged compile facade.

**Problem:** the current carrier/compiler seam can still conflate parsing, framework semantics, IDE projection, runtime compilation, module assembly, and host integration. The compiler policy lacks a stable meaning, and the demand/admission order can allow duplicate analysis or late discovery of required facts.

**Solution and architecture decisions:**

- ratify the five authorities:
  - `CarrierFrontend`;
  - `FrameworkSemanticAuthority<FrameworkEpoch>`;
  - `ProjectionBackend`;
  - `RuntimeCompilerBackend<FrameworkEpoch>` with statically selected targets;
  - `FrameworkHostIntegrationBackend<FrameworkEpoch, HostEpoch>`;
- ratify `CompilePolicy::{Default, Optimized}` with only `Default` initially supported;
- ratify `DefaultCompilationContractId` and per-product equivalence grades;
- state that `Default` may use stronger cheap component-local facts and may correct prelocked upstream gaps;
- reserve `Optimized` as a future separate train;
- ratify bounded monotonic demand closure;
- ratify `ParseAdmission`, `SemanticAdmission`, and `CompileAdmission` ownership;
- ratify that each framework semantic epoch has one authority built on shared `verter_analysis`/`type_info` machinery;
- ratify that J owns CSS-family syntax/neutral facts and framework authorities own framework style meaning;
- ratify dense snapshot-local IDs and separate authored offsets/lineage;
- ratify no universal compiler IR, mandatory reactivity IR, compiler ABI, native preprocessor, or external OXC artifact.

**Suggested predecessors:** `B3`, `B6`, `C1`.

**Normative source decomposition:**

1. **CCA0-A — Current authority inventory.** Map every carrier/compiler/projection/semantic/module-assembly/style/host caller to one final owner; identify duplicate analyses and cross-framework option fields.
2. **CCA0-B — Policy and compatibility contract.** Define `CompilePolicy`, `DefaultCompilationContractId`, equivalence matrix, intentional-divergence records, and truthful unsupported `Optimized` capability.
3. **CCA0-C — Demand and admission contract.** Define the finite demand universe, reason edges, resumption basis, and the three admission tokens.
4. **CCA0-D — Semantic authority contract.** Define per-framework authority namespaces and the `type_info` versus framework-interpretation boundary.
5. **CCA0-E — Identity and representation laws.** Lock dense IDs, source anchors, optional lineage, lossless-sidecar exclusion, and optional physical materialization.
6. **CCA0-F — Architecture guards and exact-candidate review.** Add compile-time/dependency tests proving the generic compiler layer cannot import framework semantic types and the runtime compiler cannot own a second analyzer.

**Acceptance:**

- every current method/caller has exactly one final authority;
- `Default` has a versioned behavior contract and can admit a planted cheap local alias-proven reactivity case without project I/O;
- `Optimized` is present only as truthful future capability;
- no global framework semantic authority or type-info-as-framework-authority exists;
- J ownership is preserved;
- no compiler hot-path contract contains tooling recovery/trivia;
- all negative architecture fixtures fail structurally.

**Forbidden:** implementation of Vue/Svelte V2, CSS matcher changes, native preprocessors, project-wide optimization, dynamic plugin/ABI design, or preserving the combined authority behind aliases.

**Deletion/abort:** no broad deletion; reject/rescope if the authority split requires two active semantic answers or changes accepted compiler output in this lock block.

---

