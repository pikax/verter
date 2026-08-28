# C1 eighth deviation — F9: `semantic_query_memo/` + `semantic_query.rs` omitted from the relocation inventory

Found while classifying `project_semantic_dispatch`'s `ResolverContext`
method usage per F8's corrected step 4. Dispositioned via a fresh Codex
xhigh consult. Full consult prompt/output:
`/tmp/c1-deviation3-consult-prompt.md` / `/tmp/c1-deviation3-consult-output.md`
(not committed — ephemeral scratch; this file is the durable record).

## Finding

Tracing `self.ctx.project_type_store().semantic_graph()` (one of
`project_semantic_dispatch`'s real call patterns through the
`project_type_store()` `ResolverContext` method) led to
`SemanticGraphStore` — the node arena + family-keyed memo table +
relation-proof table, defined at `crates/verter_session/src/
semantic_query_memo/mod.rs:234`, whose own doc comment says: "This store
alone does not execute queries — it is the cache substrate. Concrete
resolution happens inside a dispatcher that owns the solver/resolver
knowledge." This is, in plain terms, the "semantic node map"/`RelationMemo`
CLAUDE.md's own single-engine invariant is about.

`semantic_query_memo/` is a top-level module — **28 files, 24,794 lines** —
living entirely outside both `project_semantic_dispatch/` and
`resolver_core/`, the only two directories the scoping-spec's relocation
inventory (§1, §2) names. Neither the charter nor the scoping-spec mentions
it anywhere (grepped both, zero hits).

Also found in the same pass: `semantic_query.rs` (9,628 lines) +
`semantic_query/` (13 files, 6,698 lines) — the query key/node/value/
demand/carrier type surface `SemanticGraphStore` and
`ProjectSemanticDispatch` both operate over — is similarly absent from the
inventory.

**Combined SCC size, measured**: `project_semantic_dispatch` (146,416) +
`resolver_core` (52,434) + `semantic_query_memo` (24,794) +
`semantic_query.rs`/`semantic_query/` (16,326) = **239,970 lines**. This is
the true size of C1's relocation closure — roughly 1.6x what phases 4-5's
originally-cited counts (146K + 59-file resolver_core) suggested, and it is
now clear the four pieces are one tightly coupled SCC, not two separable
directories.

## Disposition: ADOPT-NOW (recorded as F9)

**Verdict: ADOPT-NOW, but not as a blind `git mv`.**
`semantic_query_memo/**` is in C1 scope and its canonical `SemanticGraphStore`
belongs in `verter_semantic` alongside `ProjectSemanticDispatch` — it is
part of the relocation SCC, not session lifecycle glue. This is NOT
F7-like ownership (F7's `HostStoreView` genuinely retains session roots/
workspace access/the artifact graph as its whole purpose); `SemanticGraphStore`
retains its OWN arena, family memo, relation tables, derivation graph,
reverse index, and in-flight state — "one instance per `ProjectTypeStore`"
describes lifetime/cardinality, not source-crate ownership.

It is not currently a clean leaf, though — before relocating:
- Warm validation currently takes `ResolverContext`.
- It reaches request TLS, `host_manage`, `capture_token`, `MetaProvenance`,
  and `cache_runtime`.
- It directly names scheduler request context.
- `execute_cooperative` parks joiners on a `Condvar`
  (`semantic_query_memo/mod.rs:~2429`) — a NEW blocking-wait site for the
  C1-AC-5 audit, on top of the two already named in scoping-spec.md §2
  (`SingleflightGroup::run`/`run_retaining` in `resolver_core/mod.rs`,
  `route_db_singleflight.rs`). Do not silently assume CPU-memo waits are
  exempt from the peek-or-`NeedInputs` conversion.

### Corrected/added inventory rows

| Surface | Disposition |
|---|---|
| `semantic_query_memo/**` — 28 files / 24,794 lines, including `SemanticGraphStore` and sibling tests | Dependency-neutralize (remove `ResolverContext`/TLS/`host_manage`/`capture_token`/`MetaProvenance`/`cache_runtime`/scheduler dependencies; disposition the `Condvar` wait), THEN relocate atomically with the `project_semantic_dispatch` SCC. Preserve exactly one store authority — no session-side `SemanticGraphStore`/semantic-node-map/relation-memo/forwarding facade may remain. |
| `semantic_query.rs` + `semantic_query/**` — 9,628 + 6,698 = 16,326 lines | Transitive closure audit/split under F2 (same "query-time algorithm relocates, session lifecycle glue stays" rule already governing `component_meta`/`component_meta_query_engine`). Query keys, node/value types, demand algebra, carriers, admission types, and other kernel contracts relocate or become dependency-neutral; only proven lifecycle/FFI/publication adapters may remain in `verter_session`. |

### Ownership shape (confirmed)

`ProjectTypeStore` (stays in `verter_session`) holds the one project-global
runtime instance as `Arc<verter_semantic::...::SemanticGraphStore>` — the
session creates/invalidates the project-lifetime instance,
`verter_semantic` defines and operates it. Runtime lifetime ownership does
not determine the defining crate. Do NOT expose `Arc<ProjectTypeStore>`
(the whole session store) through `ResolverObservation` — prefer an
explicit graph handle on `ProjectSemanticDispatch`/`ResolverAttemptView`,
or a small semantic-owned `SemanticKernelStores` bundle if several semantic
stores prove to always travel together. A graph-store handle is engine
state, not a missing external observation — it should NOT become an
`AttemptOutcome`-returning observation method.

### Systematic pre-pass required before resuming trait design

**Explicit instruction: stop adding real `ResolverObservation` methods
until the return-type/SCC pre-pass is materially further along.** F8
already required auditing the full relocation SCC and following every
returned capability transitively; F9 confirms that pre-pass is still
incomplete and has already found materially different dispositions per
return type (the consult's own spot-check, to be independently re-verified
during implementation, not trusted blind):

- `PreparedTypeDecl` / `PreparedValueDecl` — already semantic-owned (per
  the consult's read; matches my own earlier spot-check of
  `PreparedDeclBundle`'s plain-data shape).
- `IndexedReadyServe` — a session publication/fencing carrier wrapping
  `Arc<IndexedReady>` (`host_manage/prepared_decl.rs:49`); do not cross
  wholesale.
- `MaterializeScopeObservation` — currently embeds `Arc<IndexedReady>`
  (`resolver_context.rs:124`); needs a narrower no-tear DTO.
- `FileArtifactKey` — contains a session-private build-toolchain
  fingerprint (`file_artifact_store.rs:99`); not automatically
  dependency-neutral.
- `ShallowFileState` — embeds the session-owned `DeclBodyMemo`
  (`resolver_core/shallow_file_state.rs:78`); its observation boundary
  needs splitting, or a semantic handle rather than a wholesale return.
- `HostConfig` — must become narrow immutable semantic configuration, not
  cross as the entire host configuration struct.
- `SemanticGraphStore` — relocates (per above), but its scheduler/TLS/
  validation/wait dependencies must be peeled first.

For every `ResolverContext` method `project_semantic_dispatch` (or, per F9,
`semantic_query_memo`/`semantic_query.rs`) calls, record before writing any
trait method: required fact, semantic owner, returned DTO/handle, missing
`InputKey` (if any), side-effect/output disposition, blocking behavior,
and `cfg` gate. Only once every row closes should the `ResolverObservation`
surface be written and compiler-driven retyping begin.

## Not yet independently re-verified

The specific file:line citations above for `IndexedReadyServe`,
`MaterializeScopeObservation`, `FileArtifactKey`, `ShallowFileState`,
`HostConfig` come from the consult's own read, not my own line-by-line
confirmation — re-verify each before relying on it, per this whole
protocol's own standing instruction that facts drift and every citation
needs re-grepping at the point of use.
