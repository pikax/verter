<!-- unified-charter-v2
id=BND5
name=Bundler-host product convergence and legacy deletion
predecessors=BND4,PER0,VIM1
phase=expansion
train=expansion.bundler-host
product=bundler_host
kind=terminal
semantic_role=convergence
class=successor
owner=expansion.bundler-host:unplugin lifecycle, virtual-module, build-graph, preprocessing, and HMR authority
conflict_domains=bundler_host,program_authority,performance_evidence
resource_class=ts-heavy
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
charter=charters/expansion-bundler-host/BND5.md
size=S
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# BND5 — Bundler-host product convergence and legacy deletion

## Independently acceptable outcome and owners

Promote the bundler-host product after exact adapter, framework, lifecycle, HMR, performance, security, and deletion proof. BND5 adds no transform/HMR/adapter semantics. Final ownership is BND1–BND4 plus immutable capability and deletion receipts.

## Surfaces, APIs, and predecessor contracts

Expected surfaces are generated package exports/capabilities, exact route/deletion manifests, docs, and terminal fixtures. APIs: `BundlerHostCapabilitySnapshot`, `BundlerHostProductReceipt`, `BundlerLegacyDeletionManifest`. `BND4` supplies complete adapter receipts; `PER0` supplies cache/cancellation/work gates; `VIM1` supplies cross-vertical conformance.

## Binding architecture and subblocks

1. Validate all required cells on one candidate and generate truthful package/public capability tables.
2. Prove BND1–BND4 deleted duplicate lifecycle, ID, transform, preprocessing, invalidation, HMR, and manual support-claim routes; perform only bounded residual deletion/guard wiring named by the manifest.
3. Run security/path/archive/config trust, cancellation, transition-state, equivalent-work, latency, handle, and RSS terminals.
4. Publish the product receipt and explicit host-inapplicable cells with versioned negative proof. Any residual unsupported applicable cell blocks publication.

Terminal findings reopen BND1–BND4; no semantic patch is permitted here. Shared laws apply.

## Deletions, forbidden designs, acceptance, and verification

Delete only manifest-listed displaced authorities and migration flags. Retain JavaScript adapter code that is the final authority. Forbid calling the terminal “universal” with missing rows, retaining a fallback compiler path, or hiding unsupported cells.

- **BND5-AC1:** exact seven-bundler × Vue/Svelte capability matrix is current and complete, all applicable required cells pass, and every inapplicable cell carries pinned-host negative proof.
- **BND5-AC2:** planted legacy route or manual capability claim is structurally rejected.
- **BND5-AC3:** full state-transition/incremental/fresh/cancellation matrix passes.
- **BND5-AC4:** terminal equivalent-work, latency, allocation, handle, and RSS gates pass.
- Abort and reopen the precise predecessor for any missing behavior or cleanup exceeding 300 LOC/3 files/1 related package.
- Verify full unplugin E2E, canonical gate, strict DAG validation, and `architecture-3`.

NUX2 and SKT2 consume this product. Ceiling: 300 LOC, 3 files, 1 package; ledger presence records completion.
