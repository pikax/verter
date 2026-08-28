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
conditional_predecessors=
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
source_refs=source:compiler-proposal.md:L732
external_requirements=
activation_gate=ORC0
charter=charters/compiler-compiler-bridge/CCA2.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# CCA2 — Compiler artifact, assembly, style-stage, and host boundary

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Compiler artifact, assembly, style-stage, and host boundary. The current owner is **combined carrier compiler registry and host compile routes**. The final and sole owner is **verter_compiler capability traits plus immutable registration catalog**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src`, `crates/verter_session/src/host_compile.rs`, `packages/unplugin/src/core/compiler.ts`.
- Named API/data boundaries: `CarrierFrontend`, `FrameworkSemanticAuthority`, `ProjectionBackend`, `RuntimeCompiler`, `FrameworkHostIntegration`, `CompileArtifactSet`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **CCA1:** exact current receipt ID and digest for “Five-way compiler capability and registration cutover”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **J1:** exact current receipt ID and digest for “CSS owner reconciliation”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **B4:** exact current receipt ID and digest for “Logical source units mapping composition and atomic publication”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** establish the stable staged-compile outputs consumed by C2 and later compiler implementations without implementing Compiler V2.
- **Problem:** SFC-shaped generic outputs, session-owned framework assembly, opaque CSS preprocessing callbacks, and underspecified custom-block records would freeze the wrong long-term boundary.
- **Solution and architecture decisions:**
- define CompileArtifactSet with root artifact, artifacts, qualified maps, provenance, and typed relations;

## Acceptance IDs and discriminating proof

- **CCA2-AC1 — sole-owner proof:** add `cca2_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **CCA2-AC2 — positive contract:** add `cca2_publishes_exact_carrierfrontend`; assert exact identities, provenance, completeness, and deterministic ordering.
- **CCA2-AC3 — incremental equivalence:** add `cca2_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **CCA2-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
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
- Performance budget: equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance is 0.0% unless an owning-authority amendment supplies exact replacement thresholds; after warmup, retained bytes may not increase across 100 identical requests.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_session`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `architecture-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three fresh distinct harness reviewers with assigned lenses, deterministic low|medium|high effort, exact task/provider/model bindings, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L732`

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

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

No clause targets this file directly. Applicable contract clauses are selected by the validated `applicable_nodes` ledger and embedded verbatim in cold packets.
