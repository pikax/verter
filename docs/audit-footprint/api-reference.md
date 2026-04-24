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
| `RustAuditRecord`              | Top-level envelope. Carries timings, solver, store, memory, footprint. |
| `RustTimingAudit`              | Phase timings in milliseconds (`f64`).                                 |
| `RustSolverAudit`              | Solver counters (`total_resolve_steps`, `solve_count`).                |
| `RustStoreAudit`               | Store/view counters + imported-dependency byte total.                  |
| `RustMemoryAudit`              | RSS + host-cache + workspace byte snapshots.                           |
| `RustSemanticFootprintAudit`   | Derived footprint — see below.                                         |

### Footprint

`RustSemanticFootprintAudit` fields, each a `Vec<R>` where `R`
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
impl RustAuditRecord {
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

`VerterHost::take_audit_record(request_id) -> Option<RustAuditRecord>`
drains the entry.
