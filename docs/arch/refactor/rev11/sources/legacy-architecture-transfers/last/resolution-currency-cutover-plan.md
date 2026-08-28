# Part B — Architecture ruling v3

## Ruling

Adopt the resolution decision DAG, domain-wise signature compaction, request-world prepared-bundle memo, root-owned context-selection memo, typed error rails, bare-host deletion, and materializer dispatch guard.

The seven audit findings are correct. I found no tree evidence requiring an argument against them. In particular:

- `im` root cloning is structurally shared and effectively `O(1)`, not a deep root copy.
- `ProjectGeneration` is shape-only.
- `RecoveryScope` cannot double as the compacted resolution aggregate.
- The corpus driver cannot measure warm-pass behavior.
- `source_raise` does not exist.
- Bare-host removal is a test migration, not a production-caller migration.

## B1. Resolution decisions form a versioned dependency DAG

Add two derived resolution fact families:

```text
Decision(population, ResolutionQueryKey)
OwnerResolutionSet(population, owner)
```

A decision records only:

- Direct primitive resolution facts observed by that query.
- Direct child `Decision` facts reused by that query.

It never absorbs or flattens a child’s signature. `OwnerResolutionSet` similarly records child decisions, not their leaves.

### Publication rules

- A published decision receives a fresh, non-initial version.
- Removal invalidates the existing fact.
- Reintroduction always mints a fresh version, preventing ABA validation.
- Recomputing a decision atomically replaces its complete direct edge set.
- Observation and edge classification are one exhaustive operation. A new `ResolutionFactKey` variant cannot compile until it has a direct-leaf, derived-node, or terminal disposition.
- `ResolutionTransaction::absorb` is replaced by typed child-decision observation.

### Mutation propagation

For each mutation batch:

1. Advance the directly changed primitive facts.
2. Seed a queue with those changed keys.
3. Traverse reverse edges.
4. Use a mutation-batch-local visited set.
5. Advance each reachable `Decision` or `OwnerResolutionSet` once.
6. Publish the updated immutable roots.
7. Leave cache entries in place for ordinary read-side validation.

Cycle termination follows from the visited set over the finite node set. Event-minted versions make “advance once per mutation batch” correct; they are not the reason traversal terminates.

Lazy decision derivation is rejected deliberately. It would require `O(subgraph)` work on each validation, and memoizing that derivation would recreate the same invalidation problem as the eager reverse graph.

## B2. Signature compaction uses newly minted terminal aggregates

The current compaction substrate does not exist. Six leaf variants need a newly implemented route to a sound terminal aggregate; the existing `ProjectGeneration` fact is the only already-terminal family.

### Terminal-domain mapping

| Precise fact family | Compaction domain | Terminal fact |
|---|---|---|
| `FileWholeHash` | Content | `ContentGeneration` |
| `DerivedFactHash` | Content | `ContentGeneration` |
| `Parse` | Content | `ContentGeneration` |
| `FileSourceEnv` | Content | `ContentGeneration` |
| `ResolveImports::Semantic` | Semantic imports | `SemanticImportsGeneration` |
| `ResolveImports::Resolution` | Resolution currency | `ResolutionGeneration` |
| `RouteSurface` | Route surface | `RouteSurfaceGeneration` |
| `ProjectGeneration` | Workspace shape | Existing `ProjectGeneration` |

Every new aggregate carries the applicable base/session population identity as well as its expected generation. A session signature cannot validate against the base aggregate merely because the numeric generation happens to match.

### Aggregate producers

- `ContentGeneration` is new `FactVersionRef` plumbing over `WorkspaceEngine.content_generation`. It covers content, parse, derived-fact, and source-environment observations.
- `SemanticImportsGeneration` and `RouteSurfaceGeneration` are new domain counters owned by `ProjectTypeStore`.
- `ResolutionGeneration` is a new per-root family owned by `ResolutionFactRoot`, separately for base and session roots.
- `ProjectGeneration` remains shape-only. Its three route-resolution callers must not be described as content coverage.

The minimum producer-edit set is therefore:

- `crates/verter_workspace/src/engine.rs`
- `crates/verter_session/src/project_type_store.rs`
- `crates/verter_session/src/host_lifecycle.rs`
- `crates/verter_session/src/host_manage/analysis_io.rs`

Every production operation capable of changing a leaf in one of these domains must advance the corresponding aggregate. The project-generation-only paths in `host_lifecycle.rs` and `analysis_io.rs` are explicitly audited because they mutate routing while `content_generation` remains unchanged.

### Resolution aggregate is not `RecoveryScope`

Precise per-path mutations must continue not to advance ancestor `RecoveryScope` keys. The invariant at `engine.rs:898-904` and the `INITIAL` assertion in `resolution_currency_contract_tests.rs` remain intact.

`ResolutionGeneration` is a separate coarse family advanced whenever any resolution-currency fact in that base or session root changes. Its coarseness affects only already-compacted signatures. Those signatures are unrootable today, so coarse invalidation is strictly better than the current non-admission behavior.

### Domain-wise lifting

Compaction is per domain, not whole-signature:

- A domain remains precise through 1,023 distinct facts.
- Its 1,024th distinct fact replaces only that domain’s precise bucket with its terminal aggregate.
- Other domains remain precise until their own threshold.
- Further observations in a compacted domain do not regrow the bucket.
- If multiple populations occur in one request, retain one terminal aggregate per represented population.

This avoids the unnecessary “1,023 facts precise, 1,024 facts wholly coarse” cliff while keeping storage bounded by the closed domain and population inventory.

A content-compacted entry is invalidated by any content mutation. Consequently, this work does not provide IDE warm-across-edit reuse for compacted components. That is not a regression—they are currently unrootable—and the mutation-free corpus driver will not expose this limitation.

### Finalization and admission

Finalization becomes structurally:

```text
Complete(ReadSetSignature)
NonCacheable { observations, refusal }
```

There is no size-based third outcome.

- `ReadSetSignature` loses `overflowed`.
- `ReadSetSignature::overflow()` is deleted.
- `FactReadSetFinalise::Overflow` is deleted.
- `SignatureOverflow` ceases to be a non-admission reason.
- Signature cardinality never produces `ReturnOnly`.
- Complete compacted signatures follow the normal cacheable path.
- Genuine cancellation, fencing, unstable completion, lease failure, unresolved provenance, or partial work remain typed non-cacheable outcomes.

`SignatureAdmission::from_finalise` is the central migration point, not merely one literal `Overflow` match. Roughly 25 consumers route through that fold. Deleting the enum variant and carrier field will make the remaining direct and indirect consumers compile errors; the compiler is the completeness oracle. This is a plan-completeness issue, not an unresolved correctness design.

`template_class_facts.rs` needs a real carrier restructure before `overflow()` disappears:

```text
Complete(ReadSetSignature)
ReturnOnly { observations, refusal }
Fenced
```

The current `NonCacheable(_) | Overflow` fold must first be split into explicit arms. `NonCacheable` carries a typed return-only state; it must not fabricate an overflow signature sentinel.

## B3. Reverse edges live inside persistent resolution roots

The reverse graph belongs in `ResolutionFactRoot`, which is embedded in both `ResolutionWorldRoot` and `ResolutionSessionRoot`.

Each fact root owns persistent `im` maps for:

```text
derived node -> direct dependencies
dependency -> direct derived dependents
```

Consequences:

- Root cloning remains an `O(1)` structural share.
- Edge and version publication is structurally atomic within the immutable root.
- Base and session edges have the same lifetime and population ownership as their versions.
- A later session cannot inherit edges from an out-of-root global index.
- Edge replacement updates both maps in the same mutation operation.

A base mutation may affect decisions stored in session roots that directly depend on base facts. While the base resolution epoch is odd, the engine traverses each live affected session root under the existing base-before-session lock order, publishes its derived-version changes, then makes the base epoch stable. Captures therefore cannot observe a new base root paired with an unpropagated session decision root.

### Rule amendment

Amend both owning rules in `.claude/skills/type-cache-architecture/SKILL.md`.

Hard Rule 8 becomes:

> Reverse dependency graphs are not cache-eviction authority. A root-owned resolution graph may advance derived resolution-currency facts when edges are exhaustively recorded by the same observation operation, propagation runs under the resolution mutation publication protocol, and every dependent cache still records the derived fact and validates it on read. It may not evict or bypass validation.

R3 receives the same narrow exception:

> Advancing a derived resolution fact is currency propagation, not eager cache invalidation. No reverse-dependent cache entry is drained; it becomes cold only when its recorded derived version fails ordinary read-side validation.

The existing `reverse_graph_not_wired_to_invalidation` scanner and its bookkeeping assertion are deleted. They enforce the pre-amendment invariant and would either reject the valid implementation or survive as a stale guard.

Replacement enforcement is:

- The exhaustive observation-to-edge match, which is a structural compiler rail.
- `reverse_decision_propagation_does_not_evict_cache_entries`, which proves executable behavior.
- The existing upsert/no-reverse-eviction behavioral protections.

No replacement name-keyed scanner is added.

## B4. Context changes and deep appearance are complete mutation sources

### `ContextSelection`

`replace_published` compares the complete selected-context value for every registered `ContextSelection(path)` leaf. Changed leaves seed normal derived propagation.

This enumeration costs:

```text
O(registered ContextSelection leaves × full membership evaluation)
```

It is bounded and publish-time-only. The Block 4 context memo cannot optimize it because `replace_published` intentionally installs a fresh memo.

### Deep appearance

A precise path appearance must re-observe:

- Its exact `PathProbe`.
- Parent `DirectoryMembers`.
- Every previously recorded absent-ancestor `Realpath` fact capable of changing because of the appearance.

The ancestor walk uses point lookups in the existing persistent maps and remains `O(path depth)`; no new index is needed.

`DirectoryTreeDirty` continues to advance `RecoveryScope`. Precise mutations do not.

## B5. Prepared bundles use a typed request-reuse rail

Prepared-bundle results are classified as:

```text
Shared      — complete and safe for request and shared-cache reuse
RequestOnly — complete and deterministic under this immutable request view,
              but carries a non-cacheable refusal
NoReuse     — cancelled, partial, fenced, superseded, unstable, lease-missed,
              or otherwise not safe even for request memoization
```

Every cold return, sequential memo hit, and singleflight follower of a `RequestOnly` value must replay its stored propagation into the enclosing tracer before returning.

The implementation pattern already exists in `resolver_core/mod.rs:1013`, inside the stable-flight path reached by leader and follower. Reuse that mechanism.

The acceptance distinction is:

- The returned `NonCacheableRefusal` exposes the exact `NonCacheableReadReason` and `NonCacheablePropagation`.
- Tracer finalization exposes only that the enclosing tracer is non-cacheable; it does not record the reason.

Generalize the overlay memo into one request-world prepared-bundle memo:

- Container identity includes the exact store-view validation token, resolution-world identity, population, request/session identity, and overlay identity.
- Base and overlay namespaces remain distinct.
- Complete `Shared` and `RequestOnly` values may be memoized.
- Only `Shared` may publish into shared caches.
- `NoReuse` never enters the memo.
- Sequential and concurrent callers use one singleflight state machine.

## B6. Context selection has one root-owned memo

Add a concurrent memo owned by `ResolutionWorldRoot`, keyed by normalized path and carrying the typed selected-context result.

- A miss delegates to `ProjectResolver`.
- Unrelated immutable-root clones may share the memo.
- `replace_published` always installs a fresh memo.
- Errors and “no project” are memoized as typed results.
- The seven direct `nearest_config_for_path`/`effective_configs_for_path` callers remain outside this memo unless the landed counter proves material repetition.

## B7. Host, error, Avatar, and materializer rails

### Bare-host resolver

Delete `impl ResolverContext for VerterHost` after prepared-bundle consumers have migrated.

This is pure test migration:

- `host_construction.rs:625` is the signature of a documented test-bridge accessor, not a production caller.
- The two non-`cfg`-gated bare-host constructions have no production callers.
- The repository has approximately 49 `ComponentMetaQueryEngine::new(<bare host>)` test constructions, not 32.
- Six production `.semantic_dispatch()` calls are a different `ComponentMetaQueryEngine` method that already routes through a real context and must not be swept into this change.

Delete all nine panic arms:

1. `prepared_decl_bundle`
2. `prepared_type_decl`
3. `prepared_value_decl`
4. `store_view`
5. `resolve_imported_type_root`
6. `resolve_imported_type_root_with_facts`
7. `resolve_named_type_export_target_shallow`
8. `resolve_owner_direct_import`
9. `resolve_type_declaration_for_dep`

The current scanner cannot distinguish `resolve_imported_type_root` from the `_with_facts` prefix. It is deleted rather than extended.

### QueryError dispositions

| Variant | Disposition |
|---|---|
| `Miss` | Optional absence only at an explicitly optional boundary; otherwise typed missing dependency. |
| `UnsupportedIntrinsic` | Genuine typed unsupported error. |
| `BudgetExceeded` | Typed partial/`ReturnOnly`; never shared. |
| `Cancelled` | Typed partial/`ReturnOnly`; never shared. |
| `UnstableState` | Typed partial/`ReturnOnly`; never shared. |
| `AliasCycle` | Walker control sentinel; preserve its participant chain. |
| `RecursiveRef` | Raise to a recursive reference, not an error. |
| `Other` | Genuine typed failure. |
| `DeclPlaceholder` | Expandable `Instantiate` carrier, never “not found.” |
| `ValueDomainMismatch` | Genuine typed failure. |
| `RaiseAliasCycle` | Raise-boundary control sentinel. |
| `TypeParamCycle` | Type-parameter-cycle control sentinel. |
| `RaiseMiss` | Preserve unless the caller explicitly owns an optional boundary. |
| `UnrepresentableSurface` | Typed unsupported-surface result with existing sentinel semantics. |
| `UnrepresentableSurfaceMember` | Typed unsupported-member result with existing sentinel semantics. |

There is no `source_raise` API. The actual rails are:

- `PolicyCtx::raise_source`
- `ProjectSemanticDispatch::raise_semantic_type_source_to_hot`
- `ProjectSemanticDispatch::raise_semantic_type_source_to_hot_strict`

The strict function already returns `Result<_, StrictSourceRaiseFailure>`. Extend and reuse that typed rail; do not replace it with a newly invented parallel API.

### Avatar

Treat Avatar as an independent defect. Capture the first Verter-owned panic frame through the production release harness, land a minimal hermetic reproduction, and fix the owning layer. Bare-host deletion cannot claim causality without that evidence.

### Materializer

Add a per-request counter keyed by settled member identity.

For each completed component:

- Every settled member dispatch count equals one.
- Recursive back-edges terminate through existing typed sentinels.
- Total dispatches equal the number of unique settled members.

This counter plus behavioral tests is the structural prevention rail. Do not add a name-keyed legacy-walker scanner.

## B8. Material bounds and measurement ownership

| Operation | Bound |
|---|---|
| Immutable root clone | `O(1)` structural share |
| Persistent root mutation | `O(changed keys × log n)` |
| Cold decision build | `O(direct primitive observations + direct child decisions)` |
| Warm decision reuse | One decision observation plus ordinary signature validation |
| Resolution graph storage | `O(D + E)` per base/session root |
| Edge replacement | `O((old edges + new edges) × log n)` |
| Derived propagation | `O((changed leaves + reachable nodes + reachable edges) × log n)` |
| Base-to-session propagation | Same bound summed over affected live session roots |
| Deep appearance | `O(path depth × point lookup)` |
| Context publish enumeration | `O(registered leaves × membership evaluation)` |
| Context memo hit | Expected `O(1)` |
| Prepared bundle | One cold flight per exact request-world key |
| Materializer | One dispatch per settled member |
| Signature storage | Bounded by per-domain threshold and closed population inventory |

The previous `O(size of ResolutionWorldRoot)` clone claim is deleted. `im = "15.1"` maps are persistent HAMTs; `(*current).clone()` at `verter_workspace/src/engine.rs:727` structurally shares their nodes.

### Harness ownership

The corpus driver and Rust warm gate measure different things:

- The corpus driver owns production release equivalence, panic/error freedom, deterministic normalized output, and the aggregate one-shot query-time diagnostic.
- The Rust in-process gate owns warm-pass decision executions, `resolve_cold_builds`, `bundle_cold_flight_runs`, and materializer redispatch.

The corpus driver cannot measure warm reuse: it launches one fresh process per component, constructs a fresh workspace/host/session, performs one query, and exits.

### PERF-2 metric

For each complete 180-component sweep:

```text
Q_run = sum(query_ms_from_stdout for every successful component)
R_query = median(final Q_run) / median(baseline Q_run)
```

`wall_ms` remains diagnostic because it includes the per-child Node, `tsx`, NAPI, and process floor.

`R_query` is recorded and reviewed, not a pass/fail gate. If `R_query >= 1.0`, Block 6 requires an explicit architecture disposition:

- `ADOPT-NOW` with a bounded, evidenced correction, or
- `REJECT` with profiling evidence that the residual is outside this architecture and all deterministic invariants pass.

There is no arbitrary `0.10` threshold and no unconditional adopt-now clause.

---

# Part C — Worktree execution plan v3

## Context

Baseline:

```text
Repository: <repo root>
Commit:     2de3b2d076d72ea84932e23f8d801906429c6646
Status:     clean
```

Finding dispositions:

| Finding | Disposition | Closure |
|---|---|---|
| Transitive resolution-signature growth | ADOPT-NOW | Blocks 1–3 |
| Missing compaction aggregate producers | ADOPT-NOW | Block 1 |
| RequestOnly repeated-build/taint behavior | ADOPT-NOW | Block 3 |
| Context membership repetition | ADOPT-NOW | Block 4 |
| Bare-host compatibility implementation | ADOPT-NOW | Block 5A |
| QueryError erasure | ADOPT-NOW | Block 5B |
| Avatar panic | ADOPT-NOW | Block 5C |
| Materializer redispatch | ADOPT-NOW | Blocks 0 and 6 |
| Normalized-output nondeterminism, if observed | CHARACTERIZE | Block 0; pass gate Block 6 |
| One-shot query-time residual | REVIEW | Block 6; no fixed ratio gate |

No open `DEFER` disposition is permitted when Block 6 closes.

## Intent Contract

| ID | Acceptance |
|---|---|
| RC-1 | Every precise or imprecise resolution mutation invalidates affected decisions, including appearance beneath a previously absent probe or realpath ancestor. |
| RC-2 | A decision records direct primitive facts and direct child decisions only. |
| RC-3 | Propagation advances each reachable derived node once per mutation batch, terminates on cycles, and prevents removal/reintroduction ABA. |
| RC-4 | Base and session populations cannot validate each other. |
| DAG-1 | One exhaustive observation operation owns witness creation and edge classification. |
| RULE-1 | R3 and Hard Rule 8 contain the narrow derived-currency amendment; propagation never evicts caches or replaces read validation. |
| SIG-1 | Signature cardinality never causes non-admission or `ReturnOnly`. |
| SIG-2 | Each compacted domain is represented by a sound advancing terminal aggregate. |
| SIG-3 | Compaction is domain-wise; unrelated domains remain precise and warm. |
| RM-1 | A complete deterministic prepared bundle computes once per immutable request world. |
| RM-2 | Every `RequestOnly` cold return, memo hit, and follower replays non-cacheable propagation into the enclosing tracer. |
| RM-3 | `RequestOnly` never publishes shared; partial, cancelled, fenced, superseded, unstable, and lease-missed work never enters the request memo. |
| PD-1 | An in-process warm pass performs zero additional cold bundle flights. |
| CTX-1 | Context membership is evaluated once per normalized path per root; `replace_published` resets the memo. |
| META-1 | Every settled materializer member dispatches exactly once. |
| HOST-1 | `VerterHost` no longer implements `ResolverContext`; tests use owned host/session contexts. |
| ERR-1 | All 15 `QueryError` variants follow the Part B table. |
| CORPUS-1 | All 180 components complete through the production release harness, including Avatar. |
| DET-1 | Two identical final configurations produce identical normalized component artifacts. |
| PERF-1 | Rust warm gate reports zero repeated decision executions, zero additional cold bundle builds, and no materializer count above one. |
| PERF-2 | Record `R_query`; `R_query >= 1.0` receives an explicit architecture ruling rather than failing an arbitrary ratio gate. |
| MUT-1 | Documentation and profiling report persistent-map mutation, reachable-DAG propagation, context-publish enumeration, and deep-appearance costs accurately. |

## Changes

### Block 0A — Freeze measurement and characterize determinism

Run the same production driver for baseline and final:

```bash
node scripts/benchmark/trace-component-corpus.mjs \
  --ui-root=.integration-tests/repos/nuxt-ui \
  --output-dir=<unique-output-directory> \
  --timeout-ms=30000
```

Do not pass `--no-trace`. It is not a supported flag, unknown arguments are silently ignored, and audit/footprint capture is unconditional in `_audit-component.ts`.

Record:

- Commit and release binary hash.
- Corpus revision.
- Machine, OS, Rust, Node, and package-manager versions.
- Environment configuration.
- The fixed driver concurrency of one.
- One warm-up sweep and at least five measured sweeps.
- `Q_run` for each measured sweep and the median.

Treat `wall_ms` only as diagnostic.

Run the same configuration twice for determinism characterization. Compare the already-normalized per-component artifacts without removing extra fields. The normalizer already sorts names and excludes timings, request IDs, and counters; any remaining divergence is semantic.

If divergence occurs:

1. Identify the first component whose digest differs.
2. Diff its focused JSONL/audit records.
3. Inspect `truncation_counters` and `cache_outcomes`.
4. Test the bounded candidate set: per-process `RandomState` iteration ordering, CPU-pool winner ordering, and the default projection budget boundary.
5. Disposition the finding `ADOPT-NOW`, `DEFER-TO-BLOCK-6`, or `REJECT` with evidence.

Block 0 characterizes determinism; it does not require DET-1 to pass.

### Block 0B — Add red-at-tip behavioral fixtures

All behavioral fixtures must compile at the baseline and fail through pre-existing observables.

Add the positive-resolution signature-growth fixture:

- More than 1,024 authored `./_chunks/*.mjs` imports.
- Runtime `.mjs` targets absent.
- Matching `.d.mts` siblings present.
- Every import resolves positively.
- Current transitive absorption exceeds the signature limit.

Also characterize:

- Two components sharing a resolved helper.
- Manifest retarget.
- Deep appearance without `DirectoryTreeDirty`.
- An absent-realpath ancestor becoming present.
- Unrelated mutation remaining warm.
- Base/session isolation.
- Removal and reintroduction.
- Repeated materializer dispatch, using an existing observable if possible or the Block 6 counter once landed.

Add the taint/reuse pairing before introducing the new API:

- Prepare a deliberately non-cacheable overlay bundle.
- At tip, assert `bundle_cold_flight_runs > 1`, proving the current insertion gate deliberately forgoes reuse.
- Assert the enclosing tracer finalizes non-cacheable on every touch.
- Post-change, the same fixture must retain non-cacheable tracer finalization while `bundle_cold_flight_runs == 1`.

This fixture is the red-at-tip anchor for all four Block 3B API tests.

API-introducing tests do not independently count as red:

- Decision API tests pair with the growth and mutation fixtures.
- Typed raise tests pair with an output-classification fixture using the existing `raise_source`/semantic-source rails.
- Context memoization becomes behaviorally red after Block 4A lands its counter.
- Block 3B typed-refusal tests pair with the non-cacheable repeated-flight fixture above.

Acceptance:

- Baseline measurements are frozen.
- Determinism is characterized and dispositioned.
- Every implementation family has a failing pre-existing observable.

### Block 1A — Land domain-wise compaction and aggregate producers

Primary implementation files include:

- `crates/verter_workspace/src/fact_read_set.rs`
- `crates/verter_workspace/src/fact_cache.rs`
- `crates/verter_workspace/src/resolution_currency.rs`
- `crates/verter_workspace/src/engine.rs`
- `crates/verter_session/src/project_type_store.rs`
- `crates/verter_session/src/host_lifecycle.rs`
- `crates/verter_session/src/host_manage/analysis_io.rs`
- `crates/verter_session/src/fact_signature_helpers.rs`
- `crates/verter_session/src/cache_runtime/admission.rs`
- `crates/verter_session/src/resolver_core/mod.rs`

TDD sequence:

1. Prove the 1,024th fact compacts only its domain.
2. Prove other domains remain precise.
3. Prove additional observations do not regrow a compacted bucket.
4. Prove each of the seven current leaf variants maps exhaustively.
5. Prove every aggregate-producing mutation invalidates its aggregate.
6. Prove unrelated domains remain valid.
7. Prove base and session aggregates do not cross-validate.
8. Prove precise resolution mutation advances `ResolutionGeneration` while the ancestor `RecoveryScope` remains `INITIAL`.
9. Prove route-only mutations advance their new aggregate while `content_generation` remains unchanged.
10. Implement the minimum substrate.

Keep `ProjectGeneration` shape-only. Do not broaden its semantics to hide missing content or resolution aggregates.

Acceptance: `SIG-1`, `SIG-2`, `SIG-3`, `RC-4`.

### Block 1B — Migrate the complete overflow graph and preserve anti-poisoning coverage

Before removing the overflow test knob, restage `non_cacheable_materialization_is_not_memoized` on a surviving genuine refusal such as `FencedServe`, using `force_indexed_ready_serve_fence_for_tests`, or on a controlled `LeaseMiss`.

The restaged test must still prove:

- The caller receives the result.
- Two touches produce distinct values under the current no-memo rule.
- The memo remains empty.

Only after this passes may `force_fact_tracer_overflow_observations` be deleted.

Then:

1. Change `SignatureAdmission::from_finalise` so complete compacted signatures follow `Cacheable`.
2. Delete the `Overflow` enum arm and `ReadSetSignature.overflowed`.
3. Use compiler failures to migrate the entire `SignatureAdmission` consumer graph.
4. Remove the double-sided warm-read overflow defenses in `compile_output_node`.
5. Update validation helpers, self-root validation, admission helpers, resolution currency, virtual-file publication, and every other exhaustive match exposed by compilation.
6. Restructure template-class dependency admission into typed `Complete`, `ReturnOnly`, and `Fenced` carriers.
7. Replace `FactSignatureOverflow` audit output with `FactSignatureCompacted`, including generated TypeScript bindings.
8. Remove length-based admission checks in `ValidatedFactCache`.
9. Update critical-rule registration with structural compaction and complete/non-cacheable finalization guards.

Do not retain the previous literal 14-site list:

- `SignatureAdmission::from_finalise` is the central indirect consumer.
- `svelte_exec.rs` has no literal `Overflow` arm; its relevant behavior is a fall-through.
- `script_facts.rs` is handled indirectly through `from_finalise`.
- The cited `component_meta_caches` function is test-support gated.
- Deleting the legacy variant and field makes the compiler the authoritative migration census.

Update:

- `.claude/skills/type-cache-architecture/SKILL.md` hard rules 5 and 6, bounded-signature section, typed-admission section, and overflow hard rules.
- `.claude/skills/component-meta/SKILL.md` where overflow or the nonexistent `--no-trace` contract is described.
- `CLAUDE.md` summaries only where their pointers or summaries changed.
- `docs/arch/path-precise-resolution-currency.md`.
- `docs/arch/next/cache-admission-closure-design.md`.
- Owning fact-cache documentation.

Acceptance:

- No signature-size refusal remains.
- The anti-poisoning test never loses coverage during the train.
- All Block 1 tests and critical-rule meta-guards pass.

### Block 2A — Add decision schema and atomic direct-edge publication

Primary files:

- `crates/verter_workspace/src/resolution_currency.rs`
- `crates/verter_workspace/src/engine.rs`
- Resolution-currency contract/spec tests.

Add:

- `ResolutionFactKey::Decision`.
- `ResolutionFactKey::OwnerResolutionSet`.
- Persistent forward and reverse edge maps in `ResolutionFactRoot`.
- Fresh-version publication.
- Removal/reintroduction handling.
- Atomic direct-edge replacement.
- Exhaustive observation-to-edge classification.
- Typed child-decision observation replacing signature flattening.

Tests:

- `resolution_decision_positive_reused_candidate_depends_on_child_decision`
- `resolution_decision_records_only_direct_dependencies`
- `resolution_decision_reintroduction_mints_fresh_version`
- `resolution_decision_removal_invalidates_old_witness`
- `resolution_decision_overlay_never_validates_as_base`
- Updated exhaustive resolution-family inventory assertion at the actual contract assertion near `resolution_currency_contract_tests.rs:739`.

Acceptance: `RC-2`, `RC-4`, `DAG-1`.

### Block 2B — Root-owned DAG propagation and rule amendment

Implement propagation in the base/session immutable roots.

For base mutations:

1. Hold the base publication epoch odd.
2. Advance direct base facts.
3. Propagate within the base graph.
4. Propagate base-key changes through affected live session-root graphs under base-before-session lock ordering.
5. Publish changed session roots.
6. Publish the base root.
7. Restore stable epochs.

For session-only mutations, propagate only through that session root.

Tests:

- `resolution_decision_child_advance_advances_parent_once`
- `resolution_decision_cycle_advances_each_node_once`
- `resolution_decision_unrelated_mutation_stays_valid`
- `resolution_base_mutation_advances_dependent_session_decision`
- `resolution_session_mutation_does_not_advance_base_decision`
- `reverse_decision_propagation_does_not_evict_cache_entries`
- Full mutation-to-decision-family matrix.

Edit `.claude/skills/type-cache-architecture/SKILL.md` in this block:

- Amend Hard Rule 8.
- Amend R3.
- Update any R22 prose that names the old scanner as enforcement.
- State that dependent caches still record and validate the derived fact.

Delete:

- `reverse_graph_not_wired_to_invalidation`.
- Its `workspace_bookkeeping_invariants` registration/assertion.
- Any stale critical-rule registry entry naming it.

Replace it with the exhaustive structural match and executable non-eviction test. Do not add another symbol-name scanner.

Update mutation-cost documentation to use the persistent-map bounds from Part B.

Acceptance: `RC-1`, `RC-3`, `RULE-1`, `MUT-1`.

### Block 2C — Context-change and deep-appearance propagation

Keep this separate from the general DAG mechanism.

Implement:

- Registered `ContextSelection` change enumeration in `replace_published`.
- Changed-context leaves as propagation seeds.
- Exact `PathProbe` and parent `DirectoryMembers` re-observation.
- Absent-ancestor `Realpath` re-observation.
- No precise-mutation `RecoveryScope` advancement.

Tests:

- `resolution_decision_context_replace_advances_version`
- `resolution_decision_context_replace_unchanged_selection_stays_valid`
- `resolution_decision_negative_deep_appearance_advances_without_tree_dirty`
- `resolution_decision_absent_realpath_ancestor_appearance_advances`
- `resolution_decision_directory_tree_dirty_advances_via_recovery_scope`
- `precise_path_mutation_preserves_recovery_scope_initial`
- `resolution_decision_unrelated_path_mutation_stays_valid`

Record the bounded but non-memoized `replace_published` enumeration cost.

Acceptance: `RC-1`, `RC-3`, `MUT-1`.

### Block 2D — Publish owner projections

Publish `OwnerResolutionSet` only through `OwnerImportSurfaceDb`.

Tests:

- `owner_resolution_set_advances_with_any_child_decision`
- `owner_resolution_set_unchanged_for_unrelated_decision`
- `owner_resolution_set_published_only_by_owner_import_surface_db`
- `owner_resolution_set_records_child_decisions_not_flattened_leaves`

Acceptance: one owner-surface authority and bounded owner witnesses.

### Block 3A — Migrate import-route witness consumers

Use a symbol census and compiler errors rather than stale line lists.

The required producer/consumer families include:

- `import_route_witness_for_specifiers`
- `import_route_witness_for_lanes`
- `authored_import_specifiers`
- `decline_import_route_witness`
- Their internal self-calls
- `observe_owner_import_route_witness`
- `record_resolution_witness`
- Host lifecycle resolution publication
- Compile fact emission
- Project semantic dispatch build/carrier consumers
- Prepared-declaration and component-meta consumers

Warm resolution reuse must observe `Decision(query)`. Full-owner consumers observe `OwnerResolutionSet`.

After every consumer migrates, delete:

- `ResolutionWitnessScope`
- `WITNESS_DEPTH`
- `WITNESS_FRAMES`
- `record_resolution_witness`
- Associated ambient TLS plumbing

Tests prove:

- A warm decision records its typed decision fact in every enclosing tracer.
- Full-owner witnesses contain `OwnerResolutionSet`.
- Consumers do not require an ambient witness scope.

Acceptance: `RC-2`, `PD-1`.

### Block 3B — Typed RequestOnly rail and taint replay

Implement `Shared`, `RequestOnly`, and `NoReuse`.

Tests:

- `request_only_memo_hit_replays_taint_to_enclosing_signature`
- `request_only_singleflight_follower_replays_identical_taint`
- `request_only_first_and_nth_return_same_refusal_reason_and_propagation`
- `request_only_bundle_never_publishes_shared`

All four pair with the Block 0B red-at-tip repeated-flight fixture.

Assertions:

- Cold and memo-hit outer tracers both finalize as non-cacheable.
- The returned refusal—not tracer finalization—has the same reason and propagation.
- Singleflight followers execute the same replay path as the leader.
- `RequestOnly` cannot reach shared publication.

Reuse the stable-flight replay pattern at `resolver_core/mod.rs:1013`.

Acceptance: `RM-2`, `RM-3`.

### Block 3C — Generalize the prepared-bundle request memo

Replace the overlay-only memo after Block 3B passes.

Tests:

- `base_bundle_computes_once_sequentially_per_request_world`
- `overlay_bundle_computes_once_sequentially_per_request_world`
- `base_and_overlay_bundle_memos_are_isolated`
- `request_only_bundle_computes_once_and_replays_each_touch`
- `cancelled_or_partial_bundle_is_not_request_memoized`
- `superseded_or_fenced_bundle_is_not_request_memoized`
- `lease_missed_bundle_is_not_request_memoized`
- `request_world_token_change_forces_new_bundle`
- Concurrent callers join one cold flight.

Use `bundle_cold_flight_runs` as the deterministic oracle.

Acceptance: `RM-1`, `RM-3`, `PD-1`.

### Block 4A — Land context-membership measurement

Add the counter at the `RESOLVE_CONTEXT_SELECT` membership-evaluation boundary.

First demonstrate `N > 1` under the existing implementation with:

- `context_selection_evaluates_membership_once_per_path_per_root`

The new API alone is not the red phase; the pre-memo counter result is.

### Block 4B — Add the root-owned context memo

Implement typed result memoization and reset it in `replace_published`.

Tests:

- `context_selection_evaluates_membership_once_per_path_per_root`
- `context_selection_memo_shared_by_unrelated_root_clone`
- `replace_published_resets_context_selection_memo`
- `context_selection_error_and_no_project_are_memoized_typed`
- `context_selection_change_advances_dependent_decision`

Document that the memo cannot reduce `replace_published` enumeration and does not automatically cover the seven direct resolver callers.

Acceptance: `CTX-1`.

### Block 5A — Delete the bare-host resolver as a test migration

This block follows Block 3.

Migrate test constructors to `HostResolverContext` or `SessionResolverContext`. Use compiler errors to census:

- `ProjectSemanticDispatch::new(<bare host>)`
- All approximately 49 bare-host `ComponentMetaQueryEngine::new` constructions
- The two non-`cfg` bare-host bridge constructions with no production callers
- Test bridge accessor use

Do not rewrite the six production `.semantic_dispatch()` method calls that already use the engine’s real context.

Delete:

- `impl ResolverContext for VerterHost`
- All nine panic arms listed in Part B
- The related bare-host branch in prepared-declaration code
- `no_bare_host_resolver_shims.rs`

Primary enforcement is absence of the implementation and compiler/type failure at any remaining construction. Do not replace the scanner with another name-keyed scanner.

Acceptance: `HOST-1`.

### Block 5B — Complete typed QueryError propagation

Use the existing APIs:

- `PolicyCtx::raise_source`
- `raise_semantic_type_source_to_hot`
- `raise_semantic_type_source_to_hot_strict`

Preserve the existing strict `Result<_, StrictSourceRaiseFailure>` rail and extend typed failures through currently ambiguous optional boundaries.

Add one exhaustive `QueryError` disposition match implementing the 15-row table.

Tests cover every variant and specifically prove:

- `RecursiveRef` raises as recursion.
- `DeclPlaceholder` reaches `Instantiate`.
- `AliasCycle`, `RaiseAliasCycle`, and `TypeParamCycle` remain control carriers.
- `BudgetExceeded`, `Cancelled`, and `UnstableState` remain partial/`ReturnOnly`.
- `Other`, `UnsupportedIntrinsic`, and `ValueDomainMismatch` remain genuine failures.
- `Miss` and `RaiseMiss` become absence only at explicitly optional boundaries.
- Unsupported surface sentinels retain existing output semantics.

Acceptance: `ERR-1`.

### Block 5C — Diagnose and fix Avatar independently

Run Avatar through the production release harness with watchdog/backtrace support.

Record:

- Exact entry point.
- First Verter-owned panic frame.
- Query and population.
- Whether ownership is resolver, request context, semantic raise, or materializer.

Land a minimal hermetic failing fixture before the fix. Correct only the evidenced owning layer.

Acceptance:

- The hermetic Avatar regression passes.
- Avatar passes in the full corpus.
- No causal claim relies on Block 5A without evidence.

### Block 6 — Structural gates, final determinism, corpus, and closure

Strengthen the in-process Rust gate so the same host/session performs a cold and warm pass.

Require on the warm pass:

- Zero additional decision executions.
- Zero additional `resolve_cold_builds`.
- Zero additional `bundle_cold_flight_runs`.
- Every materializer settled-member count remains exactly one.
- RequestOnly reuse still marks the outer tracer non-cacheable.
- The mutation-to-family matrix and cycle test pass.

Run the production corpus driver separately:

- All 180 components succeed.
- No panic, timeout, query error, or missing audit artifact.
- Avatar succeeds.
- Normalized output remains semantically equivalent to the baseline.
- Two identical final configurations produce identical normalized digests.

DET-1 becomes a hard pass gate here. Any Block 0 determinism deferral must be resolved before equivalence is accepted.

Compute and record `R_query`. If it is at or above `1.0`, produce the explicit `ADOPT-NOW` or evidenced `REJECT` ruling described in Part B. The ratio itself does not override deterministic correctness gates.

Wire the external corpus feature so the documented CI job actually executes its registered test. Keep default tests hermetic.

Acceptance: every Intent Contract item and zero open deferrals.

## Legacy Deletions

1. `FactReadSetFinalise::Overflow`.
2. `ReadSetSignature.overflowed`.
3. `ReadSetSignature::overflow()`.
4. `SignatureOverflow` admission/audit semantics.
5. All global length-based non-admission checks.
6. Overflow warm-read defenses and sentinel construction.
7. Overflow-oriented critical-rule guards and positive controls.
8. `force_fact_tracer_overflow_observations`, only after anti-poisoning coverage is restaged.
9. Template-class use of an overflow signature as a non-cacheable sentinel.
10. `ResolutionTransaction::absorb` signature flattening.
11. Out-of-root/global reverse-edge storage, if any staging introduces it.
12. `reverse_graph_not_wired_to_invalidation` and its bookkeeping registration.
13. Ambient resolution-witness TLS and scopes.
14. The overlay-only prepared-bundle memo after the generalized memo passes.
15. `impl ResolverContext for VerterHost` and all nine panic shims.
16. `no_bare_host_resolver_shims.rs`.
17. Ambiguous raise/source `Option` paths where absence is not semantically optional.
18. The independently diagnosed Avatar defect.
19. Any materializer redispatch path exposed by the landed counter.
20. Documentation claiming `--no-trace` is a supported corpus-driver switch.
21. Documentation claiming immutable resolution-root cloning is `O(root size)`.
22. Stale text describing `ProjectGeneration` as content coverage or `RecoveryScope` as a general resolution aggregate.

## Verification

### TDD discipline

For every block:

1. Land the failing behavioral test.
2. Demonstrate the failure at the baseline or immediately preceding block.
3. Implement the minimum architecture.
4. Rerun the focused test.
5. Refactor only after green.
6. Run the affected package suite before moving on.

API non-compilation is never accepted as the sole red phase.

### Mutation recipes

Each correctness test documents:

- Initial files, project graph, base/session population, and captured root.
- Exact cold query and observed decision/aggregate facts.
- Exact mutation API used.
- Expected direct fact changes.
- Expected derived decision/owner changes.
- Expected aggregate changes.
- Facts required to remain unchanged.
- Warm-read or recomputation oracle.

The required mutation matrix includes:

| Mutation | Must advance | Must remain unchanged where applicable |
|---|---|---|
| Content edit | `ContentGeneration`; affected decision chain | Unrelated route/shape facts |
| Semantic-import publication change | `SemanticImportsGeneration`; affected decisions | Unrelated content facts |
| Route-surface change | `RouteSurfaceGeneration`; affected decisions | Content generation for route-only mutation |
| Precise path appearance | Exact probe, parent members, relevant realpath ancestors, `ResolutionGeneration`, affected decisions | Ancestor `RecoveryScope` |
| `DirectoryTreeDirty` | `RecoveryScope`, `ResolutionGeneration`, affected decisions | Unrelated roots |
| Project/context replacement | Changed `ContextSelection`, applicable semantic/route aggregate, affected decisions | Unchanged selections |
| Session overlay mutation | Session aggregate and decisions | Base decisions |
| Base fact used by session decision | Base fact plus dependent session decision | Unrelated session roots |
| Removal/reintroduction | Fresh derived version | Prior version must never validate |

### Focused gates

Run the owning package tests after each block. The final Rust gate is the canonical pair:

```bash
cargo nextest run --workspace
cargo test -p verter_session --tests
```

Do not substitute bare `cargo test --workspace --tests`; it omits the shared-process `verter_session` integration surface under feature unification.

Also run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo check --workspace --release
cargo clippy --target wasm32-unknown-unknown -p verter_wasm -- -D warnings
```

Run applicable TypeScript/package tests and regenerate checked-in bindings after audit schema changes.

### Final architecture verification

Require:

- No signature overflow carrier, enum arm, reason, audit event, or test force.
- Exhaustive aggregate-domain mapping.
- Every new aggregate has a live producer and validator.
- `RecoveryScope` precise-mutation contract remains intact.
- Root-owned forward/reverse graphs in base and session fact roots.
- No reverse-dependent cache eviction.
- No ambient witness TLS.
- No bare-host `ResolverContext`.
- All 15 `QueryError` variants covered exhaustively.
- Materializer one-dispatch invariant.
- Rust warm-pass counters exactly zero.
- Full 180-component production corpus success.
- DET-1 identical final normalized digests.
- Recorded `R_query` and any required architecture ruling.
- Accurate owning documentation.
- Zero unresolved or deferred findings.

No source files were modified while producing this ruling.
