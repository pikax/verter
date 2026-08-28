<!-- unified-charter-v2
id=CCA2
name=Compiler artifact, assembly, style-stage, and host boundary
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=convergence
semantic_role=convergence
class=compiler
predecessors=CCA1,J1,B4
owner=compiler.compiler-bridge:verter_compiler capability traits plus immutable registration catalog
conflict_domains=style_semantics,compiler_execution,host_service_graph
resource_class=rust-mixed
review_profile=architecture-3
gate_profile=targeted-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=S
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-compiler-bridge/CCA2.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA2 — Compiler artifact, assembly, style-stage, and host boundary

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Compiler artifact, assembly, style-stage, and host boundary. The current owner is **combined carrier compiler registry and host compile routes**. The final and sole owner is **verter_compiler capability traits plus immutable registration catalog**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src`, `crates/verter_session/src/host_compile.rs`, `packages/unplugin/src/core/compiler.ts`.
- Named API/data boundaries: `CarrierFrontend`, `FrameworkSemanticAuthority`, `ProjectionBackend`, `RuntimeCompiler`, `FrameworkHostIntegration`, `CompileArtifactSet`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **CCA1:** implemented ledger row for “Five-way compiler capability and registration cutover”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **J1:** implemented ledger row for “CSS owner reconciliation”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **B4:** implemented ledger row for “Logical source units mapping composition and atomic publication”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- **Intent:** establish the stable staged-compile outputs consumed by C2 and later compiler implementations without implementing Compiler V2.
- **Problem:** SFC-shaped generic outputs, session-owned framework assembly, opaque CSS preprocessing callbacks, and underspecified custom-block records would freeze the wrong long-term boundary.
- **Solution and architecture decisions:**
- define CompileArtifactSet with root artifact, artifacts, qualified maps, provenance, and typed relations;

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **CCA2-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **CCA2-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **CCA2-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **CCA2-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_compiler/tests`, `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete or structurally reject: **combined compiler registry**.
- Delete or structurally reject: **mixed framework/options bucket**.
- Delete or structurally reject: **tooling-only runtime stubs**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 300 production LOC, 3 production files, 1 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_session`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch adds one `[[implemented]]` row to `authority/state/implemented.toml` with the node ID, planned squash commit message, approximate date with timezone, and optional pull-request number. Row presence is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Reconciled source-plan contract

**Intent:** establish the stable staged-compile outputs consumed by C2 and later compiler implementations without implementing Compiler V2.

**Problem:** SFC-shaped generic outputs, session-owned framework assembly, opaque CSS preprocessing callbacks, and underspecified custom-block records would freeze the wrong long-term boundary.

**Solution and architecture decisions:**

- define `CompileArtifactSet` with root artifact, artifacts, qualified maps, provenance, and typed relations;
- keep framework-local strongly typed results internally and convert only at the shared product boundary;
- make framework compilers own semantic module assembly;
- make `FrameworkHostIntegrationBackend` own bundler/HMR/virtual-module/manifest policy;
- define a stage-qualified external style continuation compatible with the J-owned boundary; do not create a second preprocessor authority;
- preserve custom blocks through a source-backed `CustomBlockDescriptor` separating role/tag name from `lang`, source reference, attributes, order, region, and content availability;
- unknown custom blocks remain opaque and perform zero semantic/runtime work by default;
- keep OXC internal and stable artifacts text/bytes based;
- install temporary behavior-preserving adapters for current runtime outputs with explicit deletion ownership.

**Suggested predecessor:** `CCA1`.

**Normative source decomposition:**

1. **CCA2-A — Artifact schema and map qualification.** Define artifact IDs, roles, languages, relations, map families, provenance, and terminal serialization.
2. **CCA2-B — Framework assembly boundary.** Move or wrap Vue/Svelte semantic module assembly behind the runtime compiler authority; keep behavior unchanged.
3. **CCA2-C — Host integration boundary.** Define exact framework×host identity, lifecycle, cancellation, and publication responsibilities.
4. **CCA2-D — External style continuation.** Reuse/extend the J-owned preprocessor result shape; add exact stage identity and prevent double transformation.
5. **CCA2-E — Custom-block descriptor.** Preserve source-backed role/language/attrs/src/order/content state; no parser or transform ABI.
6. **CCA2-F — C2 integration and legacy adapter ledger.** Make C2 consume the final contracts and name each temporary output adapter’s deletion owner.

**Acceptance:**

- generic staged compilation no longer requires fixed script/template/style/custom-block fields as its durable contract;
- the generic session contains no framework module topology;
- every style result names the exact stage and input basis;
- unknown custom blocks round-trip as opaque source-backed attachments;
- text-only requests build no native AST artifact;
- existing compiler bytes/maps remain equivalent through adapters.

**Forbidden:** CSS parser work, preprocessor implementation, selector matcher rewrite, Vue/Svelte Compiler V2, custom-block ABI, external OXC AST, or unqualified `processed_css: String` inputs.

**Deletion/abort:** delete only contract shapes and session assembly routes whose consumers are fully migrated; retain short-lived adapters with named VCP/SCP deletion owners; abort if the bridge requires semantic output changes.

---

# 7. Shared compiler architecture and performance charters

## Collapsed non-authoritative subblock disposition

The recovery candidate mechanically split this source-owned atomic node into the following labels: `CCA2A`, `CCA2B`, `CCA2C`, `CCA2D`, `CCA2E`. They have no separate dispatch, lease, receipt, migration manifest, deletion ownership, or review standing. Their useful source-described concerns are internal RED/GREEN checklist items of **CCA2**; CCA2 alone owns the complete migration population, exactly one final deletion/cutover, and atomic acceptance. Any quoted “suggested subblock” wording in transferred source text is non-authoritative planning context.

