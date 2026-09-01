<!-- unified-charter-v2
id=BND2
name=Resolve-load-transform and processed-content handoff
predecessors=BND1,CCA2D
phase=expansion
train=expansion.bundler-host
product=bundler_host
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.bundler-host:unplugin lifecycle, virtual-module, build-graph, preprocessing, and HMR authority
conflict_domains=bundler_host,compiler_execution,style_semantics
resource_class=ts-heavy
gate_profile=canonical
review_profile=public-3
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
charter=charters/expansion-bundler-host/BND2.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# BND2 — Resolve-load-transform and processed-content handoff

## Independently acceptable outcome and owners

Make resolve/load/transform/build-start/build-end behavior consume canonical compiler artifacts and exchange externally processed content through one qualified envelope. Current adapters duplicate ordering, cache, and style-preprocessor assumptions; final owner is the common unplugin pipeline plus thin tool adapters.

## Surfaces, APIs, and predecessor contracts

Expected surfaces: unplugin core compiler, macro hydration, precompile, preprocessor and hook adapters; typed native requests remain CCA/CMP-owned. APIs: `BundlerCompileIntent`, `HookInvocationId`, `ProcessedContentEnvelope`, `ProcessedStageId`, `BundlerTransformResult`, `QualifiedSourceMap`. `BND1` supplies session/module identity; `CCA2D` supplies qualified style continuation and prohibits unqualified processed-content reuse.

## Binding architecture and subblocks

1. Define one hook-independent resolve/load/transform plan and deterministic output ordering.
2. Translate each tool hook into the plan without changing compiler demand or framework semantics.
3. Hand preprocessor/PostCSS results back with source revision, block identity, dialect, stage/tool/options hashes, dependencies, diagnostics, and composed maps.
4. Share precompile/on-demand cache admission and cancellation; only complete exact-basis artifacts warm.

JS adapters may execute their host toolchain and preprocessors; the core receives typed prepared inputs and never evaluates arbitrary JS. Shared laws apply.

## Migration, deletions, forbidden designs, and acceptance

Migrate hooks incrementally behind measurement-only parity, switch the common route, then delete duplicate tool pipelines and unqualified CSS handoffs. Forbid compiling twice across load/transform, content-only processed cache keys, source-map dropping, silent preprocessor fallback, and embedding bundler policy in the compiler.

- **BND2-AC1:** build/dev, client/SSR, Vue/Svelte, external-src, style/custom virtual modules, diagnostics and maps are exact.
- **BND2-AC2:** planted stage/source/options/map mismatch is rejected as stale/NeedInputs.
- **BND2-AC3:** edit/revert/preprocessor dependency change equals fresh and never warm-admits partial output.
- **BND2-AC4:** equivalent-work proves one admitted compile per semantic demand and no duplicate parse/emit/copy.
- Performance evidence records hook invocations, prepared-input bytes, compiler requests, map compositions, allocations, latency, and retained processed artifacts for cold/warm/incremental/cancelled modes; disabled/inapplicable hooks do zero work.
- Abort if HMR decisions enter (BND3) or tool-specific public option semantics cannot share this plan.
- Verify unplugin/native/compiler/preprocessor/E2E suites, canonical gate, and `public-3`.

BND3/BND4 consume the pipeline. Ceiling: 800 LOC, 8 files, 2 packages; ledger presence records completion.
