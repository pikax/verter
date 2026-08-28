# Staged Compile Transaction and Anti-Replay Contract

**Status:** Normative direct/prepared/project-aware compiler protocol.

# 1. Stages

```text
prepare(source, parse options)
  -> PreparedCarrier
plan(prepared, CompileRequest)
  -> CompilePlan
project(plan.projection_batch, CompileTypeInfo)
  -> CompleteFacts | NeedInputs(LoadSet) | TerminalFailure
emit(prepared, plan, complete facts)
  -> CompileResult
```

Planning discovers the complete product prerequisite closure and closed semantic projection batch before projection. Emission does not discover new project-semantic demands. A new demand requires a new plan.

# 2. Binding tokens

`CompilePlanToken` binds the complete request for anti-replay. The plan additionally owns narrower deterministic tokens:

```text
ProjectionPlanToken   root + semantic profile + closed demand batch + kernel/projection domains
ProductSubplanToken   root + framework/product + output profile + required product mapping contract
TerminalSubplanToken  exact typed result/product + requested presentation/provenance/encoding profile
```

The whole `CompilePlanToken` binds at least:

- prepared root/source/unit identities;
- parse key/domain and relevant generation;
- normalized canonical typed product-request collection;
- shared semantic profile when observed;
- each product's output and requested terminal presentation/mapping/provenance/serialization profiles;
- framework/compiler compatibility identity;
- projection schema/domain;
- plan algorithm compatibility domain;
- deterministic demand ordering and digest.

The plan contains independently keyed subplans/artifact identities. A presentation-, map-encoding-, provenance-, or serialization-only change may create a new terminal subplan without invalidating an unchanged semantic projection or code-generation subplan. `CompilePlanToken` binds the complete request for anti-replay; it does not force every subartifact to use that whole token as its cache key.

`CompileFactsBatch` binds:

- exact `ProjectionPlanToken` and demand digest referenced by the containing plan;
- semantic profile and kernel compatibility domain;
- input basis and observed dependency fingerprints;
- one typed result for every demand;
- exactness/completeness/degradation;
- integrity and size limits when decoded or transferred.

Emission first validates the whole `CompilePlanToken`, then validates that the facts satisfy the exact `ProjectionPlanToken` referenced by that plan. It rejects missing, extra, duplicated, reordered without canonical identity, stale, wrong-profile, wrong-projection-plan, wrong-root, wrong-domain, or insufficient facts before output construction.

# 3. Zero-demand behavior

When the plan has no semantic projection demands:

- no `CompileTypeInfo` session is constructed;
- no module resolver or TypeInfo index/graph/flow work runs;
- projection stage is represented as an empty complete batch bound to the plan;
- Svelte current runtime compilation must prove zero Vue/native projection demand unless its capability contract changes.

# 4. `NeedInputs`

`NeedInputs(LoadSet)` is a resumable orchestration outcome. It does not mutate the current plan or prepared value with unversioned ambient data. The outer orchestrator loads and publishes/rebuilds a coherent input view according to `input-loading.md`, then reruns projection with the same plan only when the plan’s source/request/profile basis remains valid. Otherwise it replans.

# 5. Direct and managed modes

- local direct mode may project from the retained root only;
- direct project mode consumes an immutable caller-provided observation view and returns `NeedInputs` to the caller;
- managed mode consumes one `EngineSnapshot`/`InputBasisId` per attempt and uses the outer commit/retry loop;
- all modes execute one semantic kernel and the same closed framework projector;
- lifecycle, retention, and orchestration differ; semantics do not.

# 6. Error taxonomy

Distinct failures include parse/preparation failure, unsupported product combination, unavailable semantic capability, `NeedInputs`, no progress, projection gap, unresolved dependency, stale plan/facts, profile/domain mismatch, size/integrity failure, cancellation, budget exhaustion, and internal failure.

# 7. Required tests

- plan once and emit multiple requested products without rediscovery;
- simultaneous product combinations equal separately requested products where contracts declare composability;
- different per-product output profiles coexist without a global-profile collision;
- presentation/serialization-only changes reuse unchanged semantic and code subplans;
- irrelevant product/profile fields are rejected or normalized away before identity construction;
- zero-demand zero-initialization;
- local and imported projection batches;
- batched shared-root deduplication;
- stale/wrong-whole-plan/wrong-projection-plan/wrong-profile/extra/missing facts rejected;
- facts from a terminal-only sibling plan are accepted only when their `ProjectionPlanToken` is exactly equal;
- `NeedInputs` waves and no-progress behavior;
- basis change forces retry/replan as appropriate;
- direct/prepared/managed output equality for equal product/profile/input contracts;
- cancelled/superseded attempts do not terminally materialize output.
