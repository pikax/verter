## Derivation / Origin Layer Contract

The type graph structurally interns nodes — two distinct derivations producing the same structural result share one arena entry. Origin therefore cannot live inline on each node: the same node may be the result of many derivations. Origin is modelled as a **separate graph layer** of edges from result nodes to their source nodes, co-owned by the `SemanticGraphStore`.

- Origin edges are stored outside the interned node table, in a sibling edge set keyed by `(result_node, edge_kind, source_node)` with optional per-edge metadata.
- A single result node may have multiple origin edges of the same kind from different derivations; the layer MUST support this. Walking from a result returns the full edge set, not one canonical chain.
- Walking is a first-class API on `SemanticGraphStore`, not a private solver internal. External consumers (component-meta compat, LSP hover, error-message rendering) walk origin to present provenance.
- Origin edges are immutable once published; they participate in the same `ReadSetSignature.facts` fact-signature validation as the interned nodes they point at. Cancelled / budget-exceeded / superseded derivations do not publish origin edges.

**Required edge kinds** (names are normative; semantics must not drift):

- **`Instantiate`** — `result = decl<args>`. From the instantiated result back to the declaration identity and concrete argument nodes.
- **`SubstituteTypeParam`** — `result_position = T -> V`. From a concrete type in a substituted position back to the declaration's type parameter and the binding that produced the substitution.
- **`ConditionalSelect`** — `result = select(conditional, True | False | Deferred)`. From the selected-branch result back to the conditional's check, extends, branches, and the deciding relation judgement. Records the branch taken (or `Deferred` if the check stayed open).
- **`InferBind`** — `result = T bound via infer`. From the inferred type back to the `infer` binding site and the concrete type captured by the relation check.
- **`ProjectMember` / `ProjectIndex` / `ProjectPath`** — `result = base.segment(s)`. From the projected result back to the base node and the path segment(s) requested.
- **`Normalize`** — `result = normalize(source_members)`. From a normalized union / intersection / simplified result back to each contributing member node.
- **`AliasResolve`** — `result = unwrap(alias)`. From the unwrapped-target result node back to the alias declaration identity. Emitted once per alias hop (direct alias, re-export alias, barrel alias). Chains are walkable end-to-end so clients can render the full alias provenance.

Edges compose. `ProjectPath(OtherType<string>, ['a','foo'])` produces a node whose origin traces `ProjectMember → ProjectMember → Instantiate → SubstituteTypeParam → ConditionalSelect`. Clients needing derivation-aware display walk the edges and present whichever chain is relevant.

**Three distinct `OriginEdgeKind` taxonomies (do not conflate or reconcile).** The nine derivation kinds above live as `verter_type_engine::semantic_query::OriginEdgeKind`. The audit substrate mirrors them and adds one audit-only kind: `verter_audit::OriginEdgeKind` = the same nine + `SharedLoadReuse` (emitted when a request joins a winner's in-flight artifact via scheduler dedup). The typeinfo **wire** graph uses a SEPARATE 10-arm graph-relationship taxonomy (proto `GraphOriginEdgeKind`: `DECLARES`/`INSTANTIATES`/`REFERENCES`/`MEMBER_OF`/`RESOLVES_TO`/`SHARED_LOAD_REUSE`/`FALLTHROUGH`/`RELATION_PROOF_STEP`/`BACK_EDGE_CYCLE`/`AUGMENTATION_STITCH`) — NOT the derivation taxonomy renamed, and only session↔audit is name-isomorphic (modulo `SharedLoadReuse`). The three lists are pinned by `origin_edge_taxonomy_locked` (`crates/verter_session/tests/cases/g_block/typeinfo_graph_contract_guards.rs`).

**Typeinfo wire-contract guard surface.** The closed typeinfo wire surface (proto node/symbol/origin/request/error taxonomies, the split env-hash query identity, the closure-bound and schema-version request contracts, and the `AuditedResult` audit carrier) is pinned by a family of static guards under `crates/verter_session/tests/cases/g_block/typeinfo_{wire_surface,graph_contract,request_contract,audit_contract}_guards.rs`.

**First-class telemetry.** `SemanticGraphStore` exposes `SemanticGraphStats` as a public API. Per-`SemanticQueryKey`-variant counters (cache hits, misses, same-path-sentinel returns, in-flight peak, cross-thread-join wait time) and per-dispatch-builder counters (instantiations, conditional branch selections, budget/fallback invocations, path length p50/p95, projection depth p50/p95, origin edges emitted, origin edges per node p50/p95) are mandatory — not an optional observability pass. The trace-check harness, benchmark pipeline, and feedback-file report at track exit consume `SemanticGraphStats::snapshot()` directly.

