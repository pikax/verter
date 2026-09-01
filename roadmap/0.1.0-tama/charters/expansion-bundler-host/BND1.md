<!-- unified-charter-v2
id=BND1
name=Unplugin session lifecycle and virtual-module identity
predecessors=BND0,CCA1O4
phase=expansion
train=expansion.bundler-host
product=bundler_host
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.bundler-host:unplugin lifecycle, virtual-module, build-graph, preprocessing, and HMR authority
conflict_domains=bundler_host,source_lineage
resource_class=ts-heavy
gate_profile=ts-domain
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
release_gating=none
external_requirements=
charter=charters/expansion-bundler-host/BND1.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# BND1 — Unplugin session lifecycle and virtual-module identity

## Independently acceptable outcome and owners

Own one plugin session per admitted project/tool graph with deterministic startup, update, cancellation, close, and virtual-module identity. Current identity/lifecycle is distributed across hooks and host calls; final owner is `BundlerHostSession` and its module catalog.

## Surfaces, APIs, and predecessor contracts

Expected surfaces: `packages/unplugin/src/index.ts`, `core/compiler.ts`, ID/filter/cache helpers, and focused tests. APIs: `BundlerHostSession`, kernel-owned `BundlerHostSessionId`/`BundlerModuleId`, `BundlerGraphKind`, `BundlerModuleRole`, and `BundlerSourceRevision`. `BND0` supplies the contract; `CCA1O4` supplies the typed framework-discriminated native request route and proves this node does not recreate legacy compile profiles.

## Binding architecture and subblocks

1. Bind adapter/tool versions, project snapshot, source-module environment, `FrameworkAdapterId`, typed `BundlerCompileIntent`, mode, graph kind, root, and plugin options once per session. A later project profile may supply an opaque compile-intent identity, but BND does not interpret it.
2. Define reversible, collision-free virtual IDs for main/script/template/style/custom/generated roles, preserving raw/canonical forms separately.
3. Serialize lifecycle transitions and cancel/retire prior work before publication.
4. Make close/restart release native host handles, caches, watchers, and virtual-module entries.

Identity includes framework capability, compile intent, source-module environment, and client/SSR/dev/prod axes; a raw query string or absolute filename alone is insufficient. It does not depend on the later `ProjectProfileId` authority. Shared laws apply.

## Migration, deletions, forbidden designs, and acceptance

Characterize existing IDs/lifecycle, install the session/catalog, migrate hooks, then delete local ID parsers and independent host/cache ownership. Forbid global singleton state, implicit cwd roots, framework detection after identity construction, lossy query sorting, and reuse across tool/project epochs.

- **BND1-AC1:** every role/framework/mode/graph round-trips through the ID catalog without collisions.
- **BND1-AC2:** planted missing framework/graph/project epoch aliases are rejected.
- **BND1-AC3:** edit/revert/restart/config-change sequences equal a fresh session and publish no stale module.
- **BND1-AC4:** warm resolve is allocation-bounded; close/restart reaches a retained-memory plateau with no orphan host/watch handle.
- Abort if transform semantics enter (BND2) or HMR policy enters (BND3).
- Verify unplugin unit/type/lifecycle suites and `architecture-3` under `ts-domain`.

BND2/BND3 consume the session. Ceiling: 800 LOC, 8 files, 2 packages; ledger presence records completion.
