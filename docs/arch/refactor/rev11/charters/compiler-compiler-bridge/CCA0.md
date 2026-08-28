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
conditional_predecessors=
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
source_refs=source:compiler-proposal.md:L635
external_requirements=
activation_gate=ORC0
charter=charters/compiler-compiler-bridge/CCA0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# CCA0 — Compiler authority, policy, demand, and admission constitution

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Compiler authority, policy, demand, and admission constitution. The current owner is **combined carrier compiler registry and host compile routes**. The final and sole owner is **verter_compiler capability traits plus immutable registration catalog**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src`, `crates/verter_session/src/host_compile.rs`, `packages/unplugin/src/core/compiler.ts`.
- Named API/data boundaries: `CarrierFrontend`, `FrameworkSemanticAuthority`, `ProjectionBackend`, `RuntimeCompiler`, `FrameworkHostIntegration`, `CompileArtifactSet`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **ORC0:** exact current receipt ID and digest for “Orchestration v2 cutover and immutable-receipt migration”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **B3:** exact current receipt ID and digest for “Canonical typed compiler request and prerequisite planner”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **B6:** exact current receipt ID and digest for “PreparedCarrier direct batch and direct-core closure”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **C1:** exact current receipt ID and digest for “ModuleResolverCore convergence and non-flow semantic basis”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** lock the compiler authority boundaries and policy semantics before C2 seals the staged compile facade.
- **Problem:** the current carrier/compiler seam can still conflate parsing, framework semantics, IDE projection, runtime compilation, module assembly, and host integration. The compiler policy lacks a stable meaning, and the demand/admission order can allow dup
- **Solution and architecture decisions:**
- ratify the five authorities:

## Acceptance IDs and discriminating proof

- **CCA0-AC1 — sole-owner proof:** add `cca0_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **CCA0-AC2 — positive contract:** add `cca0_publishes_exact_carrierfrontend`; assert exact identities, provenance, completeness, and deterministic ordering.
- **CCA0-AC3 — incremental equivalence:** add `cca0_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **CCA0-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
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
- Performance budget: equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance is 0.0% unless an owning-authority amendment supplies exact replacement thresholds; after warmup, retained bytes may not increase across 100 identical requests.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_session`
2. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `architecture-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three fresh distinct harness reviewers with assigned lenses, deterministic low|medium|high effort, exact task/provider/model bindings, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L635`

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

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

No clause targets this file directly. Applicable contract clauses are selected by the validated `applicable_nodes` ledger and embedded verbatim in cold packets.
