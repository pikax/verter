# Audit Footprint — API reference

Canonical types live in
[`verter_session::component_meta_audit`](../../crates/verter_session/src/component_meta_audit/mod.rs).
TS bindings are generated from Rust via `ts-rs` into
[`packages/types/audit.generated.ts`](../../packages/types/audit.generated.ts).
The sync test `audit_ts_bindings_are_in_sync` fails with a unified
diff if the two drift.

## Record types

| Rust type                      | Summary                                                                |
| ------------------------------ | ---------------------------------------------------------------------- |
| `RequestAuditRecord`              | Top-level envelope. Carries timings, solver, store, memory, footprint. |
| `RequestTimingAudit`              | Phase timings in milliseconds (`f64`).                                 |
| `ComponentMetaPayload`              | Solver counters (`total_resolve_steps`, `solve_count`).                |
| `RequestStoreAudit`               | Store/view counters + imported-dependency byte total + materialiser counters (plan §3.2). |
| `RequestMemoryAudit`              | RSS + host-cache + workspace byte snapshots.                           |
| `RequestFootprintAudit`   | Derived footprint — see below.                                         |

### `RequestStoreAudit` materialiser counters (plan §3.2)

Six `u64` counters (decimal-string serialized, `string` in TS)
were added to `RequestStoreAudit` for the session-layer materialiser
cutover:

| Field                          | Meaning                                                                                |
| ------------------------------ | -------------------------------------------------------------------------------------- |
| `materialize_structure_calls`  | Total `materialize_component_meta_structure` invocations during the request.           |
| `materialize_structure_cache_hits` | Subset satisfied by the materialiser's `MaterializeStructureDb` peek (warm cache).  |
| `node_arena_lock_acquisitions` | Lock acquisitions on the per-scope `NodeArena` dedup index.                            |
| `family_map_lock_acquisitions` | Lock acquisitions on the family-map dep-signature reverse index.                       |
| `dep_signature_merges`         | Times a `dep_signature` was merged into the materialiser's `local_fence`.              |
| `dep_signature_intern_hits`    | Subset of `dep_signature_merges` that hit an existing intern bucket (no allocation).   |

Cache hit rate: `materialize_structure_cache_hits /
materialize_structure_calls` — should be `> 0` on warm/cold-seq
passes (warm peek satisfies repeat lookups). Intern hit rate:
`dep_signature_intern_hits / dep_signature_merges` — exercises
the content-hash bucketed `Weak`-ref interner; `> 0` on cold-seq
where a second pass re-merges the same dep facts into a fresh
fence.

### Cache-outcome enum

`CacheOutcomeKind` (used by both `RequestStoreAudit::cache_outcomes`
and `StructuredAuditEvent::MaterializeStructureExit`):

| Variant                | Meaning                                                                  |
| ---------------------- | ------------------------------------------------------------------------ |
| `Hit`                  | Warm cache hit.                                                          |
| `Miss`                 | Cache miss (no entry present).                                           |
| `JoinedWait`           | Joined a peer's in-flight slot and waited.                               |
| `Sentinel`             | Observed a sentinel (placeholder) entry.                                 |
| `ColdBuild`            | Performed a cold build from source.                                      |
| `InflightAbortedRetry` | Retry loop after an in-flight slot was aborted.                          |
| `ColdAbortSwept`       | Cold entry reaped during generation reconciliation.                      |
| `Tainted`              | Path-dependent outcome (depth-fuse trip, scope-unloaded mid-compute, or `Recursive` sub-call). Non-cacheable; propagates as `MaterializeOutcome::Tainted`. Plan §3.3. |

### Materialise-skip-reason enum

`MaterializeSkipReason` (carried by
`StructuredAuditEvent::MaterializeStructurePolicySkip`):

| Variant                                | Meaning                                                                 |
| -------------------------------------- | ----------------------------------------------------------------------- |
| `FunctionPropertyAtNested`             | Object-property lookup hit a function-typed property at `Nested` axis.  |
| `GenericRefWithArgsTopLevel`           | Top-level generic ref carried explicit type arguments (reserved for the `InstantiationRef` arm). |
| `PackageRefTopLevel`                   | Top-level ref resolved under `node_modules/` — package types stay opaque. |
| `RegistryRouteNotInlineMaterialisable` | Registry-route check rejected the input as not inline-materialisable (e.g. `Pick`/`Omit` over a non-bare root). |
| `NonStructuralTopLevel`                | Top-level shape is non-structural (primitive, literal, type-param, etc.). |

### Footprint

`RequestFootprintAudit` fields, each a `Vec<R>` where `R`
derives `serde + ts-rs::TS`:

- `indexed_ready_builds: Vec<IndexedReadyBuildRecord>`
- `vfs_reads: Vec<VfsReadRecord>`
- `shared_load_reuses: Vec<SharedLoadReuseRecord>`
- `instantiations: Vec<InstantiationRecord>`
- `projections: Vec<ProjectionRecord>`
- `conditional_decisions: Vec<ConditionalRecord>`
- `substitutions: Vec<SubstitutionRecord>`
- `alias_resolutions: Vec<AliasResolveRecord>`
- `materializations: Vec<MaterializationRecord>`
- `cache_outcomes: CacheOutcomeTally` (exact per-context counters;
  NO `is_approximate` field — plan §1.4).
- `graph_completeness: GraphCompletenessReport`
- `derivation_subgraph: DerivationSubgraph`

Methods:

- `loaded_files() -> Vec<Arc<str>>` — union of `vfs_reads` +
  `shared_load_reuses`, sorted + deduplicated.
- `declared_dependency_files() -> Vec<Arc<str>>` — broader union
  including `indexed_ready_builds`.
- `mask_incidental_spans() -> Self` — clones with flaky fields
  stripped (VFS reads). Snapshot tests use this.

## Derivation subgraph

`DerivationSubgraph { nodes: Vec<NodeRecord>, edges: Vec<DerivationEdgeRecord> }`.

- `NodeId(u32)` and `EdgeId(u32)` are in-audit opaque indices.
- `NodeRecord { kind: SemanticNodeKind, named_identity: Option<NamedIdentity>, structural_hash: Hash16, display_label: Arc<str> }`.
- `DerivationEdgeRecord { result: NodeId, kind: OriginEdgeKind, sources: Vec<NodeId>, meta: OriginEdgeMetaDto }`.
- `SemanticNodeKind` is `#[non_exhaustive]` — forward-compatible
  with future variants via the `Other { name }` catch-all.

Sort keys (plan §1.4 "Deterministic NodeId assignment"):

- Nodes: `(kind as u32, structural_hash, named_identity.map(|n| (canonical_id, symbol_name, args_fingerprint)))`.
- Edges: `(result, kind as u32, sources)`.

Identical requests produce identical serialized footprints
regardless of thread interleaving.

## Walker

```rust
impl RequestAuditRecord {
    pub fn why_loaded(&self, canonical_id: &str) -> ProvenanceChain;
    pub fn why_instantiated(
        &self,
        decl_canonical_id: &str,
        decl_symbol_name: &str,
        args_fingerprint: Hash16,
    ) -> ProvenanceChain;

    pub fn assert_loaded_files_exactly<I, S>(&self, expected: I) -> Result<(), AssertionDiff>
    where I: IntoIterator<Item = S>, S: AsRef<str>;

    pub fn assert_declared_dependency_files_exactly<I, S>(&self, expected: I) -> Result<(), AssertionDiff>
    where I: IntoIterator<Item = S>, S: AsRef<str>;
}
```

`ProvenanceChain` carries `steps: Vec<ProvenanceStep>`,
`terminated: ChainTermination`, and `shared_load_terminals: Vec<SharedLoadReuseRecord>`.

## Harness — `AuditedRequest`

```rust
use verter_session::audited_request::AuditedRequest;

// hermetic
let (analysis, resolution, record) = AuditedRequest::builder()
    .files([("/a.vue", SRC)])
    .resolve("/a.vue")?;

// attach to existing host
let (analysis, resolution, record) = AuditedRequest::builder()
    .attach_to(host)
    .resolve("/a.vue")?;

// closure variant — useful for multi-step fixtures
let (analysis, resolution, record) = AuditedRequest::builder()
    .attach_to(host)
    .run_custom(|host| host.get_component_meta_with_resolution(id))?;
```

Errors: `NestedAuditNotSupported`, `MultipleRequestsInSingleRun`,
`AuditRecordMissing`, `PrerequisitesNotMet`, `ResolutionFailed`.

## NAPI / WASM bindings

```ts
// NAPI — packages/native/audit.ts
class ComponentMetaSession {
  getComponentMetaWithAudit(id: string): Buffer | null;   // JSON AuditBundle
  whyLoadedFromAuditJson(auditJson: string, canonicalId: string): string;
  whyInstantiatedFromAuditJson(
    auditJson: string,
    declCanonicalId: string,
    declSymbolName: string,
    argsFingerprintHex: string,
  ): string;
}
```

All three are synchronous — no `async fn`, no
`wasm_bindgen_futures::future_to_promise`. TS wrappers may wrap
the sync call in `Promise.resolve(...)` at the consumer layer.

## Retrieval store

`VerterHost::audit_records` is an `IndexMap`-backed bounded store
(capacity: `AUDIT_RECORDS_STORE_CAPACITY = 256`). Insert order
controls eviction; oldest entries evict on overflow. Strict
insert-then-take — no access refresh.

`VerterHost::take_audit_record(request_id) -> Option<RequestAuditRecord>`
drains the entry.
