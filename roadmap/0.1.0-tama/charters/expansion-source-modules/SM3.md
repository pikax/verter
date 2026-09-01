<!-- unified-charter-v2
id=SM3
name=Source-module consumer cutover and conformance
predecessors=SM2,PM4,PER0,VIM1
phase=expansion
train=expansion.source-modules
product=source_modules
kind=terminal
semantic_role=convergence
class=successor
owner=expansion.source-modules:static source-module facts, provenance, read sets, and membership authority
conflict_domains=source_module_facts,program_authority,performance_evidence
resource_class=rust-mixed
gate_profile=canonical
review_profile=architecture-3
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
dispatchable=true
optional=false
release_gating=product
external_requirements=
charter=charters/expansion-source-modules/SM3.md
size=S
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# SM3 — Source-module consumer cutover and conformance

## Independently acceptable outcome and owners

Certify the static source-module product after SM1/SM2 migrate required semantic/product consumers and delete duplicate alias/asset/query/glob/env authorities. SM3 adds no fact semantics and performs only bounded residual manifest/guard cleanup within its S-node budget. Final ownership is SM1/SM2 plus a generated capability, consumer, and deletion receipt.

## Surfaces, APIs, and predecessor contracts

Expected terminal surfaces are the exact manifests, generated guards, and evidence covering project/workspace, TypeScript plugin/provider, language service, checker, compiler host, CLI, and unplugin. APIs: `SourceModuleCapabilitySnapshot`, `SourceModuleConsumerManifest`, `SourceModuleDeletionReceipt`. `SM2` closes fact production and its consumer migration population; `PM4` supplies canonical project snapshots; `PER0` supplies cache/cancellation/work law; `VIM1` supplies conformance infrastructure.

## Binding architecture and subblocks

1. Freeze required consumer/profile cells and exact captured-host version matrix.
2. Prove consumers use typed facts and never execute bundler work during semantic demand.
3. Prove SM1/SM2 deleted duplicate resolvers/enumerators/env readers and install only bounded residual no-bypass guard/manifest wiring.
4. Run Vue/Svelte fixtures plus framework-neutral captured-fact shapes representative of Nuxt/SvelteKit, incremental/fresh, cancellation, security, work, and memory gates. These are schema counterfixtures only; SM3 does not require or simulate the later project-profile implementations.

Terminal discoveries reopen SM1/SM2. Common laws apply.

## Deletions, forbidden designs, acceptance, and verification

Delete only manifest-listed source-module heuristics, stores, flags, and duplicate tests. Retained bundler execution must be behind BND APIs. Forbid terminal semantic fixes, hidden plugin execution, sampled capability claims, or residual required consumer rows.

- **SM3-AC1:** every required consumer/profile cell has exact current evidence.
- **SM3-AC2:** planted direct alias/glob/env path is structurally rejected.
- **SM3-AC3:** full incremental/fresh and transition matrix passes with no stale fact publication.
- **SM3-AC4:** warm/disabled/cancel/churn/allocation/RSS gates pass.
- Abort and reopen predecessors for missing semantics, incomplete inventory, or cleanup exceeding 300 LOC/3 files/1 related package.
- Verify the full source-module matrix, canonical gate, strict DAG validation, and `architecture-3`.

BND0 and PPR0 consume this convergence. Ceiling: 300 LOC, 3 files, 1 package; ledger presence records completion.
