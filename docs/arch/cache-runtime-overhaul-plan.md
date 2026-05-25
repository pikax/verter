# Cache runtime overhaul plan

This plan describes the target architecture for bringing Verter's cache
runtime to the strongest correctness and performance shape the project can
support. It assumes breaking changes, broad migrations, and API changes are
allowed when they produce a better final architecture.

## Context

Verter's current cache architecture has the right core philosophy:
lazy validation, fact-granular read signatures, content-addressed parse
artifacts, query-identity semantic caches, and `ComputeAdmission::ReturnOnly`
for valid-but-not-cacheable results.

The current implementation is still not the best possible shape because
several concerns remain coupled:

- Pure SFC compilation can flow through host/session/cache machinery that is
  only needed for workspace-aware semantic answers.
- `compileMany` is a parallel loop over per-file host operations rather than a
  true transactional batch.
- Some artifact caches are ad hoc maps instead of nodes in a shared artifact
  runtime.
- Cache admission discipline is not uniformly enforced at every producer. In
  particular, overflow or incomplete fact signatures must never degrade into an
  empty cacheable signature.
- Scheduler pools, cache admission, and batch execution are not one coherent
  runtime. `compileMany` currently constructs a local Rayon pool per call while
  scheduler parse work uses scheduler-owned pools.
- Persistent caching exists as an architectural opportunity, but the project
  should persist only pure content-addressed artifacts until semantic-query
  admission is fully audited.

The target architecture is a deterministic incremental computation engine:
every reusable output is an immutable node in an artifact or query graph, every
workspace-aware result records the facts it read, and every warm hit is
validated against the current `StoreView` before return.

Native function-body flow-return should build on this substrate, not carry its
own bespoke cache runtime. The flow-return plan's `FlowLoweredBody`,
`FlowBody` fact, and `FlowReturn` query are a good first semantic consumer of
the new architecture.

## Hard cache rules

These rules are non-negotiable for the target architecture. Implementation
blocks may add stricter rules, but they must not weaken these.

1. Cache correctness is read-side authoritative. A warm hit is correct only
   after validation against the caller's current `StoreView`.

2. A cache key must include every deterministic input that changes the value.
   If that is not possible, the value is not cacheable.

3. Query-identity keys must not include content hashes, version hashes, or
   `fact_dep_signature`. Version identity belongs on the cached value.

4. The five env hash dimensions stay split. `parse_env_hash`,
   `resolve_env_hash`, `type_env_hash`, `lib_env_hash`, and
   `project_identity` must not be bundled into a single `project_config_hash`.

5. Empty and overflowed signatures are different states. Empty means
   dependency-free. Overflowed means valid result, non-cacheable result.

6. Tracer overflow, budget exhaustion, cancellation, generation supersession,
   incomplete self-rooting, and unresolved provenance all route through
   `ReturnOnly`.

7. `ReturnOnly` never publishes a cache entry, never registers reverse-index
   metadata, and never becomes a persistent artifact.

8. Reverse dependency graphs are not invalidation authority. They may support
   observability, prefetch, diagnostics, and targeted stale sweeps only.

9. Same-canonical edits must be caught by strict self-root validation. They
   must not rely on eager own-canonical drains for correctness.

10. Cross-file edits invalidate consumers lazily through recorded facts, not
    through reverse-dependent eviction cascades.

11. Base cache and overlay cache writes are separate. Overlay/session results
    must not populate base-only artifacts or persistent cache entries.

12. Pure artifacts may be persisted only when their keys contain all semantic,
    compiler, env, profile, plugin, and source-map-policy dimensions.

13. Fact-validated semantic query results are memory-only until every query
    family has audited strict self-root validation, complete env keys, and
    typed non-cacheable admission.

14. Singleflight is required for every cold cacheable node. Concurrent callers
    for the same key must produce at most one cold computation per miss window.

15. Joiners on an in-flight computation must validate the winner's published
    entry against their own view before returning it.

16. Cache admission must be typed. Boolean flags, empty arrays, sentinel hashes,
    or side-channel `RefCell` state must not decide whether a result is
    cacheable.

17. Cacheable entries must be immutable after publish. Mutation creates a new
    versioned value or a new artifact key.

18. A cache hit must not allocate audit payloads when no request accumulator is
    active.

19. Public APIs must expose cache semantics when behavior differs. A single
    ambiguous compile path must not hide `stateless`, `content`, and `session`
    behavior.

20. Benchmarks must report cache mode, source-map policy, batch shape, thread
    count, hit count, and fallback count. A benchmark without those dimensions
    is not an architecture signal.

## Changes

### 1. Define the final cache model

Update:

- `.claude/skills/type-cache-architecture/SKILL.md`
- `docs/arch/fact-based-cache.md`
- `docs/arch/cache-runtime-overhaul-plan.md`

Add the final model as a first-class rule set:

- `WorldSnapshot` is the concurrency identity for a request. It captures file
  hashes, parse env hash, resolve env hash, type env hash, lib env hash,
  project identity, overlay identity, compiler version, plugin versions,
  source-map policy, and public API mode.
- Pure artifact nodes are keyed by exact deterministic inputs.
- Query-identity nodes are keyed by content-free semantic identity. Version
  roots and fact signatures live on the value, never in the key.
- Session overlays are views over the base project cache. They must not mutate
  base artifacts.
- Reverse dependency graphs are observability and prefetch data only. They are
  not invalidation authority.
- Cache admission is explicit: every producer returns `Cacheable`,
  `ReturnOnly`, or `Failed`.

Add architecture guards or discriminating tests for every new critical rule:

- query keys do not contain `fact_dep_signature`
- query keys do not bundle the five env hashes into `project_config_hash`
- overflowed signatures cannot validate as cacheable
- same-canonical edits fail strict self-root validation
- cross-file edits invalidate consumers lazily through fact validation
- overlay results cannot populate base-only caches

### 2. Introduce a generic cache runtime

Create:

- `crates/verter_session/src/cache_runtime/mod.rs`
- `crates/verter_session/src/cache_runtime/admission.rs`
- `crates/verter_session/src/cache_runtime/artifact.rs`
- `crates/verter_session/src/cache_runtime/query.rs`
- `crates/verter_session/src/cache_runtime/singleflight.rs`
- `crates/verter_session/src/cache_runtime/store.rs`
- `crates/verter_session/src/cache_runtime/memory_policy.rs`
- `crates/verter_session/src/cache_runtime/metrics.rs`
- `crates/verter_session/src/cache_runtime/tests.rs`

Move the reusable parts of `cooperative_admission` into this runtime without
weakening its guarantees:

- one cold computer per key and miss window
- cooperative wait, not spin
- panic safety
- post-compute revalidation
- removal cleanup
- value projection isolation
- joiners validate against their own view
- `ReturnOnly` is returned only to the winner; joiners fork and recompute

Add typed wrappers:

```rust
pub enum CacheAdmission<Value, Entry> {
    Cacheable(Entry),
    ReturnOnly(Value),
    Failed,
}

pub trait ArtifactNode {
    type Key;
    type Value;
    type AdmissionMeta;
}

pub trait QueryNode {
    type Key;
    type Entry;
    type Value;
}
```

The traits should not become a dynamic-dispatch framework. Prefer monomorphized
typed wrappers and small shared primitives. The goal is uniform behavior, not a
stringly runtime.

### 3. Make admission semantics impossible to misuse

Update:

- `crates/verter_session/src/cooperative_admission.rs`
- `crates/verter_session/src/fact_signature_helpers.rs`
- `crates/verter_session/src/compile_fact_emission.rs`
- `crates/verter_session/src/component_meta_materialize.rs`
- `crates/verter_session/src/component_meta_caches.rs`
- `crates/verter_session/src/semantic_query_memo/mod.rs`
- every caller that constructs a `ReadSetSignature`

Required final state:

- `ReadSetSignature::overflow()` is a non-cacheable carrier.
- `ReadSetSignature::empty()` remains valid only for truly dependency-free
  cacheable values.
- No helper may convert tracer overflow into `Arc<[]>` without an overflow bit.
- Producers that cannot construct a sound signature return `ReturnOnly`.
- A `ReturnOnly` value never publishes an entry and never registers reverse
  index metadata.

Replace `finalise_signature_or_empty` with a typed API:

```rust
pub enum SignatureAdmission {
    Cacheable(ReadSetSignature),
    NonCacheable,
}
```

Every producer must pattern-match it. There should be no "empty means maybe
safe" path.

### 4. Convert artifact storage into nodes

Update:

- `crates/verter_session/src/file_artifact_store.rs`
- `crates/verter_session/src/file_artifact_store_tests.rs`
- `crates/verter_session/src/member_semantic_fact_store.rs`
- `crates/verter_session/src/member_display_fact_store.rs`
- `crates/verter_session/src/host_manage/prepared_decl.rs`
- `crates/verter_session/src/host_manage/analysis_io.rs`
- `crates/verter_session/src/parse_stable_hash.rs`

Create typed artifact nodes for:

- `IndexedReady`
- `ResolvedImportFacts`
- typed IR resolve artifacts
- member semantic facts
- member display facts
- module augmentation index entries
- compile outputs
- future `FlowBodyHash`
- future `FlowLoweredBody`

Artifact keys must spell out exact dimensions. Examples:

```rust
IndexedReadyKey {
    canonical,
    content_hash,
    parse_env_hash,
    parser_version,
}

CompileOutputKey {
    canonical,
    source_hash,
    parse_env_hash,
    resolve_env_hash,
    type_env_hash,
    lib_env_hash,
    compile_profile_hash,
    compiler_version,
    source_map_policy,
}

FlowLoweredBodyKey {
    canonical,
    parse_stable_hash,
    body_semantic_hash,
    parser_version,
    function_symbol,
}
```

Artifact nodes must support:

- in-memory singleflight
- deterministic key hashing
- no implicit host mutation
- explicit admission or non-admission
- byte-size accounting
- optional persistence only when the node is pure

### 5. Split pure compilation from session compilation

Update:

- `crates/verter_session/src/host_compile.rs`
- `crates/verter_session/src/host_compile_tests.rs`
- `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs`
- `crates/verter_session/src/compile_fact_emission.rs`
- `crates/verter_napi/src/lib.rs`
- `packages/native/index.ts`
- `packages/native/index.js`
- `packages/native/index.spec.ts`
- `packages/benchmark/src/apple-to-apple.ts`

Add explicit public cache modes:

```ts
type CompileCacheMode = "stateless" | "content" | "session";
```

Semantics:

- `stateless`: direct compile, no cache admission, no host session mutation.
- `content`: use pure content-addressed artifact nodes. No workspace semantic
  query admission.
- `session`: full host/session/fact-validated semantics.

The fast path may be used only when the input does not require workspace
semantics:

- no external `src` blocks
- no macro type dependencies
- no workspace alias resolution needed for codegen
- no module augmentation observation
- no block or style overrides
- no IDE-only template/type analysis
- no dev last-good behavior

If any condition is not satisfied, fall back to `session`.

### 6. Make `compileMany` a transactional batch

Update:

- `crates/verter_session/src/host_compile.rs`
- `crates/verter_session/src/host_upsert.rs`
- `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs`
- `crates/verter_session/src/host_compile_tests.rs`
- `crates/verter_session/src/host_compile_audit.rs`
- `crates/verter_scheduler/src/scheduler.rs`
- `crates/verter_scheduler/src/queue.rs`
- `crates/verter_scheduler/src/job.rs`
- `crates/verter_scheduler/src/audit_publish.rs`

Replace the current per-canonical submit/wait shape with a batch transaction:

```rust
begin_batch(snapshot)
dedupe_inputs()
classify_fast_path_or_session_path()
compute_source_diffs()
submit_all_required_parse_jobs()
wait_all_parse_jobs()
publish_artifacts_once()
compile_all_missing_outputs()
admit_cache_entries_once()
finish_batch()
```

The batch must:

- preserve input order in output
- compile each unique canonical once
- report duplicate canonical/source conflicts deterministically
- perform one publish phase for cache-visible source changes
- batch VFS edge recording and overlay notifications
- avoid per-call Rayon pool construction
- use host-owned CPU pools with cancellation and backpressure
- expose one audit envelope for the batch and per-file child spans

### 7. Integrate scheduler and cache runtime

Update:

- `crates/verter_scheduler/src/scheduler.rs`
- `crates/verter_scheduler/src/pool.rs`
- `crates/verter_scheduler/src/driver.rs`
- `crates/verter_scheduler/src/executor.rs`
- `crates/verter_scheduler/src/stage.rs`
- `crates/verter_session/src/host_construction.rs`
- `crates/verter_session/src/project_type_store.rs`

Required final state:

- CPU work runs on CPU workers.
- I/O work runs on I/O workers.
- Source-provided parse jobs do not execute CPU parsing on the I/O pool.
- Cache-node computes are scheduled through one runtime with priorities,
  cancellation, and backpressure.
- In-flight dedupe occurs before work is scheduled where possible.
- The runtime can execute a dependency DAG, not only independent jobs.

This is necessary for best performance because a cache hit is the fastest
possible task, but a cold miss should still avoid duplicate work and avoid
queueing CPU work behind unrelated I/O.

### 8. Rehome remaining host caches

Update:

- `crates/verter_session/src/lib.rs`
- `crates/verter_session/src/project_type_store.rs`
- `crates/verter_session/src/host_resolve.rs`
- `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs`
- `crates/verter_session/src/semantic_query_memo/mod.rs`
- `crates/verter_session/src/component_meta_caches.rs`
- `crates/verter_session/src/component_meta_materialize.rs`
- `docs/arch/debt-closure/12-host-cache-rehoming.md`

Move all cache-shaped host fields into the project cache root:

- compile cache
- resolved type cache
- eval env cache
- semantic DB handle

`VerterHost` should own configuration, workspace, scheduler, and project cache
roots. It should not own standalone result maps with bespoke invalidation.

### 9. Add a persistent pure artifact cache

Create:

- `crates/verter_session/src/cache_runtime/persistent/mod.rs`
- `crates/verter_session/src/cache_runtime/persistent/cas.rs`
- `crates/verter_session/src/cache_runtime/persistent/manifest.rs`
- `crates/verter_session/src/cache_runtime/persistent/tests.rs`

Persist only pure artifacts at first:

- parse artifacts that are independent of overlays
- resolved import facts when keyed by source and resolve env
- compile outputs for `content` mode
- member semantic/display facts when their keys are complete
- future `FlowLoweredBody` when `body_semantic_hash` is not over budget

Do not persist fact-validated semantic query results until the semantic cache
audit proves every query has strict self-root validation, complete env keys,
and sound non-cacheable admission.

Persistent cache requirements:

- content-addressed path layout
- atomic write then rename
- schema version and compiler version salt
- five env hash dimensions preserved
- source-map policy in compile-output keys
- corruption detection
- bounded size policy
- no writes from overlay-only views

### 10. Add memory policy and cache observability

Update:

- `crates/verter_session/src/project_type_store.rs`
- `crates/verter_session/src/request_context.rs`
- `crates/verter_session/src/host_manage.rs`
- `crates/verter_session/src/component_meta_audit/mod.rs`
- `crates/verter_audit/src/record.rs`
- `crates/verter_audit/src/structured_event.rs`
- `packages/benchmark/src/audit-validator.ts`

Every cache node must expose:

- hit count
- miss count
- stale rejection count
- non-admission count
- in-flight dedupe count
- compute duration
- validation duration
- stored bytes where measurable
- evicted bytes
- live entry count

Use a weighted policy, not an unbounded map:

- active snapshot pins prevent eviction of entries currently in use
- pure artifacts use byte-weighted admission and eviction
- semantic query entries use entry count plus observed recompute cost
- route and component-meta caches expose stale sweeps separately

Audit emission must be allocation-free when there is no active accumulator.

### 11. Update native flow-return to target the runtime

Update the flow-return plan before implementation:

- `/tmp/verter-native-flow-return-coverage.md` or its durable successor under
  `docs/arch/`

Replace bespoke `FileArtifactStore::flow_lowered_body_for` language with a
typed artifact-node integration:

- `ArtifactNode::FlowBodyHash`
- `ArtifactNode::FlowLoweredBody`
- `SemanticQueryKey::FlowReturn`
- `FactKey::FlowBody`

Keep the existing good contracts:

- pre-lookup `body_semantic_hash`
- `FlowLoweredBody` not inside `IndexedReady`
- `body_semantic_hash` excluded from the `FlowReturn` query key
- `FlowBody` fact recorded on the cached value
- over-budget body hash is non-admitted at result, artifact, and fact layers
- whitespace edits stay warm
- semantic body edits cold-rebuild only the affected function

### 12. Add benchmark and regression gates

Create or update:

- `packages/benchmark/src/cache-runtime-bench.ts`
- `packages/benchmark/src/apple-to-apple.ts`
- `packages/benchmark/src/meta-ui-bench.ts`
- `crates/verter_bench/examples/profile_host.rs`
- `crates/verter_bench/examples/profile_cache_runtime.rs`
- `crates/verter_session/src/cache_runtime/tests.rs`
- `crates/verter_session/tests/cache_runtime_architecture_guards.rs`

Benchmark scenarios:

- pure SFC stateless compile
- pure SFC content-cache cold and warm
- full session compile cold and warm
- `compileMany` unique 80 files
- `compileMany` duplicate canonicals
- `compileMany` with external `src`
- component-meta cold and warm on real corpus
- LSP open/edit/hover loop
- thundering herd on one semantic query
- overlay vs base cache isolation
- persistent cache after process restart

Performance acceptance rules:

- no regression to hot cached `compileMany` latency
- cold dependency-free `content` mode stays in the same performance class as
  direct compiler execution
- `session` mode remains correct and should only pay semantic costs for files
  that require semantic observation
- concurrent cold callers compute once per key
- whitespace-only edits do not rebuild semantic body artifacts
- semantic edits invalidate by fact validation, not reverse-dependent eviction

## Legacy Deletions

Remove these paths or patterns as part of the migration:

- `finalise_signature_or_empty` and any equivalent helper that converts
  overflow to an empty cacheable signature.
- Direct cache admission from a `ReadSetSignature` without checking
  `overflowed`.
- Any production `get_unvalidated` warm-read path for semantic query results.
- Per-call Rayon pool construction in `compile_many`.
- Per-file submit-and-wait scheduler loops inside batch compilation.
- Unconditional compile-tier owner `ensure_indexed_ready` prefetch for
  dependency-free compile output.
- Unconditional external type collection setup when `macro_type_deps` is empty.
- Direct host-owned result maps for compile, resolved type, eval env, and
  semantic DB cache state.
- Bespoke cache invalidation lists spread across `configure_projects`,
  `set_workspace`, `clear_compile_cache`, `notify_close`, and upsert paths.
- Any reverse-dependent eviction loop used as correctness authority.
- Audit event payload allocation before checking whether a request accumulator
  exists.
- Any public API that hides stateless/content/session semantics behind one
  ambiguous cache behavior.
- Any persistent semantic-query cache admission before the semantic query audit
  proves complete fact signatures and strict self-root validation.

## Verification

Run targeted tests as each block lands:

```bash
cargo test --package verter_session cooperative_admission --tests --verbose
cargo test --package verter_session cache_runtime --tests --verbose
cargo test --package verter_session file_artifact_store --tests --verbose
cargo test --package verter_session host_compile --tests --verbose
cargo test --package verter_session query_db_self_root --tests --verbose
cargo test --package verter_session component_meta_cache --tests --verbose
```

Run full Rust verification before declaring the migration complete:

```bash
cargo test --workspace --tests --verbose
```

Run native and package builds after Rust changes that affect NAPI or generated
types:

```bash
pnpm run build:native
pnpm run build:ts
pnpm --filter @verter/native test
pnpm --filter @verter/benchmark test
```

Run performance verification:

```bash
pnpm --filter @verter/benchmark exec tsx packages/benchmark/src/apple-to-apple.ts
pnpm --filter @verter/benchmark exec tsx packages/benchmark/src/cache-runtime-bench.ts
cargo run --package verter_bench --example profile_host --release --features hotpath
cargo run --package verter_bench --example profile_cache_runtime --release --features hotpath
```

Run real component-meta corpus checks:

```bash
node scripts/benchmark/trace-component-corpus.mjs \
  --output-dir=tmp/cm-cache-runtime \
  --no-trace

npx tsx packages/benchmark/src/trace-check.ts \
  tmp/cm-cache-runtime \
  --strict \
  --check-expected
```

Expected outcomes:

- all full workspace Rust tests pass
- no architecture guard reports an unguarded critical cache rule
- cache-mode output equivalence holds for stateless/content/session where their
  semantics overlap
- `ReturnOnly` values are returned but not admitted
- persistent pure artifacts survive process restart and reject mismatched env
  hashes
- session overlays cannot populate base-only cache entries
- batch compile preserves output order and per-file diagnostics
- benchmark output shows cold dependency-free content compilation avoids the
  full session overhead while session mode preserves semantic correctness
