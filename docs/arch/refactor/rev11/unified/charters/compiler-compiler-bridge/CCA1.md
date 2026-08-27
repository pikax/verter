<!-- unified-charter-v2
id=CCA1
name=Five-way compiler capability and registration cutover
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=cutover
semantic_role=delivery
class=compiler
predecessors=CCA0
conditional_predecessors=
owner=compiler.compiler-bridge:verter_compiler capability traits plus immutable registration catalog
conflict_domains=compiler_execution,capability_catalog
resource_class=rust-mixed
review_profile=public-3
gate_profile=targeted-domain
size=M
dispatchable=true
optional=false
release_gating=none
source_refs=source:compiler-proposal.md:L687
external_requirements=
activation_gate=ORC0
charter=charters/compiler-compiler-bridge/CCA1.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# CCA1 — Five-way compiler capability and registration cutover

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Five-way compiler capability and registration cutover. The current owner is **combined carrier compiler registry and host compile routes**. The final and sole owner is **verter_compiler capability traits plus immutable registration catalog**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src`, `crates/verter_session/src/host_compile.rs`, `packages/unplugin/src/core/compiler.ts`.
- Named API/data boundaries: `CarrierFrontend`, `FrameworkSemanticAuthority`, `ProjectionBackend`, `RuntimeCompiler`, `FrameworkHostIntegration`, `CompileArtifactSet`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **CCA0:** exact current receipt ID and digest for “Compiler authority, policy, demand, and admission constitution”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** atomically install the authority split with behavior-preserving adapters so C2 builds on the final seam rather than the combined carrier compiler abstraction.
- **Problem:** a tooling-only carrier must not pretend to compile, IDE projection must not be a runtime compiler product, generic sessions must not understand framework module topology, and framework/target dispatch must not occur dynamically per node.
- **Solution and architecture decisions:**
- add typed catalog/registry tables for:

## Acceptance IDs and discriminating proof

- **CCA1-AC1 — sole-owner proof:** add `cca1_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **CCA1-AC2 — positive contract:** add `cca1_publishes_exact_carrierfrontend`; assert exact identities, provenance, completeness, and deterministic ordering.
- **CCA1-AC3 — incremental equivalence:** add `cca1_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **CCA1-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_compiler/tests`, `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete or structurally reject: **combined compiler registry**.
- Delete or structurally reject: **mixed framework/options bucket**.
- Delete or structurally reject: **tooling-only runtime stubs**.
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

1. `cargo nextest run -p verter_compiler -p verter_session`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `public-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three named independent reviewers with assigned lenses, model `gpt-5.6-sol`, effort `ultra`, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L687`

## Reconciled source-plan contract

**Intent:** atomically install the authority split with behavior-preserving adapters so C2 builds on the final seam rather than the combined carrier compiler abstraction.

**Problem:** a tooling-only carrier must not pretend to compile, IDE projection must not be a runtime compiler product, generic sessions must not understand framework module topology, and framework/target dispatch must not occur dynamically per node.

**Solution and architecture decisions:**

- add typed catalog/registry tables for:
  - carrier frontends;
  - framework semantic authorities/profiles;
  - projection backends;
  - optional runtime compilers;
  - framework-host integrations;
- migrate Vue and Svelte through behavior-preserving adapters;
- keep target selection coarse and static inside each framework runtime compiler;
- keep multi-target prerequisite sharing inside one framework compiler cell;
- retain one immutable catalog construction authority;
- delete the combined carrier compiler trait/registry and cross-framework option bucket in the atomic cutover.

**Suggested predecessor:** `CCA0`.

**Normative source decomposition:**

1. **CCA1-A — Type and registry skeleton.** Land typed traits/tables and compile-time capability truth with no route cutover.
2. **CCA1-B — Frontend and semantic migration.** Move parse/source-unit/fact routes while preserving bytes, recovery, identities, and caches.
3. **CCA1-C — Projection migration.** Move IDE/checkable projection into `ProjectionBackend`; prove no runtime compiler dependency.
4. **CCA1-D — Runtime compiler migration.** Move Vue/Svelte compile routes and owner-local typed requests; preserve direct/prepared/managed behavior.
5. **CCA1-E — Host-integration migration.** Move existing framework-host behavior behind the explicit integration authority without changing semantics.
6. **CCA1-F — Atomic deletion and parity.** Delete combined traits/registries/options and generated guards only after all consumers move.

**Acceptance:**

- tooling-only test carriers compile without runtime-backend stubs;
- Vue/Svelte parse, projection, compile, maps, cache, diagnostics, and public outputs remain equivalent on pinned corpora;
- one framework can request multiple targets while sharing prerequisites;
- target dispatch occurs outside per-node loops;
- zero combined-registry/combined-options consumers remain.

**Forbidden:** dual-running registries, erased `Any` artifacts, one backend per target that duplicates framework prerequisites, public compatibility aliases that remain authorities, or framework branches in the generic session.

**Deletion/abort:** delete the old combined trait/registry and mixed option types atomically; abort on unexplained output/map/performance divergence.

---

## Collapsed non-authoritative subblock disposition

The recovery candidate mechanically split this source-owned atomic node into the following labels: `CCA1A`, `CCA1S`, `CCA1SC`, `CCA1SF`, `CCA1SH`, `CCA1SP`, `CCA1SS`, `CCA1T`, `CCA1V`, `CCA1VC`, `CCA1VF`, `CCA1VH`, `CCA1VP`, `CCA1VS`. They have no separate dispatch, lease, receipt, migration manifest, deletion ownership, or review standing. Their useful source-described concerns are internal RED/GREEN checklist items of **CCA1**; CCA1 alone owns the complete migration population, exactly one final deletion/cutover, and atomic acceptance. Any quoted “suggested subblock” wording in transferred source text is non-authoritative planning context.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

No clause targets this file directly. Applicable contract clauses are selected by the validated `applicable_nodes` ledger and embedded verbatim in cold packets.
