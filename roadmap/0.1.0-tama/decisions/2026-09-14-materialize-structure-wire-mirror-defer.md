# DEFER ruling and debt row: structural-materialiser wire mirrors (rev11.type-evaluation)

- Status: proposed — awaiting maintainer ratification
- Date: 2026-09-14
- Adds: no DAG node. Records the debt rows the TE5 candidate leaves behind so the deferral is a disposition, not a TODO.
- Scope: two findings disposed during the TE5 candidate's review. Each is recorded here because it was found during roadmap review of a node candidate, following the same convention as `2026-09-01-d2c-flow-return-audit-partiality-defer.md`.

## Context

The TE5 candidate deleted the session-layer structural materialiser: the
`materialize_component_meta_structure` walker, its per-thread in-flight table,
the `MaterializeStructureDb` slot on `ProjectTypeStore` with its
compute/admission rail, the fence-observation bridges, the host test-injection
knobs, and the allow-hidden route-extraction / package-root predicates. None of
it had a production caller in any build configuration; every structural surface
a consumer publishes is produced by the one shared query route
(`SemanticQueryKey` → `ProjectSemanticDispatch::execute` → `SemanticGraphStore`)
and reduced by the projector pipeline.

Two residues were identified by the candidate's three fresh reviews and are
outside the charter's mutation boundary (public/native/wire contracts must not
change; the route switch must not alter display meaning).

## Debt row 1 — `MATERIALIZE-STRUCTURE-WIRE-MIRRORS`

- **Finding:** the audit and stats wire still carries the materialiser's
  mirrors with no producer: `RequestContext.materialize_structure_calls` /
  `materialize_structure_cache_hits`, `CacheLayerBreakdown.materialize_structure`,
  the `verter_audit` `MaterializeStructure{Enter,Exit,PolicySkip,CycleDetected,DepthFuseTripped}`
  structured events, `MaterializationScopeAudit`, `MaterializeSkipReason`,
  `ComponentMetaPayload.materialize_structure_*`, the `HostStats`
  `materialize_structure_fact_tracer_installs` / `materialize_structure_overflow_refusals`
  rows, the generated `packages/types/audit.generated.ts` mirrors, the
  `verter_napi` stats expectations, the `verter_audit` rustdoc on those types
  (which feeds the generated TS comments), and the `docs/audit-footprint/*` pages
  that describe them. All are zero-valued / never emitted. The recorded
  cache-baseline fixtures (`tests/cases/fixtures/cache_baseline/*.md`) carry a
  status note rather than a rewrite.
- **Why deferred:** retiring them changes the audit payload schema, the
  generated TS bindings, and the napi stats surface — a wire-contract change
  the TE5 charter forbids ("Do not widen public … wire contracts"; the audit
  envelope is additive-only). The Rust-internal residue was NOT deferred: it
  is deleted in the TE5 candidate.
- **Durable owner block:** a wire-contract retirement block in the audit /
  typeinfo wire train (to be named by the maintainer at ratification; the
  natural home is the block that next bumps the audit payload schema).
- **Resolution gate:** no later than plan close; the retirement lands with the
  next audit-schema bump so the wire changes once.
- **Acceptance ID/test:** the retirement removes every mirror listed above and
  regenerates `packages/types/audit.generated.ts`; the existing
  `cache_layer_regression_per_layer` and napi `meta.rs` expectations are
  updated in the same change. Until then, `docs/audit-footprint/api-reference.md`
  and `structured-events.md` label the rows as producer-less.
- **Ruling reference:** this decision (TE5 candidate review, architecture
  specialist P1-1 / conformance P2 / adversarial P3-1).

## Debt row 2 — `MACRO-SURFACE-REPLAY-IMPORT-TYPE-IDENTITY`

- **Finding:** a macro payload surface member typed by an inline
  `import("pkg").Name<…>` shell is published carrying the unresolved
  `ImportType` node, but the same surface replayed later (after the shell was
  resolved on demand) carries the resolved `InstantiationRef` node. A
  `CallableOccurrenceHandle` minted at publication therefore no longer matches
  the replayed member (`matches_subject` compares store-local node handles),
  and a slot-return raise for such a member fails as `UnraisableSource`.
  Today this is masked for a shell AT THE ROOT of the member value because
  the callable realizer keeps that shell as a carrier-semantics stop (typed
  `Incomplete(MissingDependency)`, no return position recorded) — exactly the
  pre-existing display behaviour. The stop is root-only: a shell reached
  through a local alias publishes the alias node as its occurrence subject,
  replays identically, and realizes through the shared demand (pinned by
  `svelte_alias_wrapped_import_type_snippet_prop_publishes_return`). The
  candidate preserves the root stop deliberately (`realize_callable_member`,
  documented arm) because the charter forbids display-meaning changes.
- **Why deferred:** the fix is a path-independence repair in the macro
  surface producer / occurrence identity (publish the resolved carrier, or key
  the occurrence on a content-free identity), a semantic change outside the
  TE5 route switch.
- **Durable owner block:** the framework-surface / occurrence-identity block
  that owns `CallableOccurrenceHandle` (to be named at ratification).
- **Resolution gate:** no later than plan close.
- **Acceptance ID/test:** `svelte_inline_import_type_snippet_prop_resolves_typed_role`
  extended to assert the `header` / `children` snippet slots publish a
  `Present` return position and that `get_component_meta_output` raises it;
  the realizer's import-type carrier stop is then deleted in the same change.
- **Ruling reference:** this decision (TE5 candidate fix pass, reproduced
  under the rewritten `realize_callable_member`).

## Closure-inventory note (no debt row)

Two consumer sites choose the projection context for a demand they forward
through the shared route rather than receiving it from a caller:
`meta_resolve/slot_binding_graph.rs` (`slot_param_root_is_symbolic_only`'s
concrete-check `ProjectPath` and Skeleton `Instantiate`) and
`typeinfo/vue_macro_codegen/runtime.rs` (the per-hop `eager_context` /
`carrier_context` worklist). Both dispatch `SemanticQueryKey`s on the one route
and build no evaluator of their own; they are named here so the entrance
inventory is explicit. Reading the charter's second-context clause strictly
would make them the next candidates for a request-owned context.

## Decision

1. The two debt rows above are the disposition of record for the residues the
   TE5 candidate leaves; neither is a TODO in source.
2. Ratification assigns each row its owner block; until then the rows are open
   deferrals counted at plan close.
