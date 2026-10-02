## Reactive-wrapper demand (shared vocabulary)

`project_semantic_dispatch/reactive_wrapper.rs` is the ONE owner of the question
"does this authored reference head resolve to a reactive wrapper exported by the
`vue` package?". It owns `WrapperCandidate`, the exact route gate
`wrapper_candidate_for_route` (terminal import source is exactly `vue` **and**
`ResolverContext::workspace_is_package_backed`), and the closed vocabulary
`wrapper_role_for_vue_export` (`Ref` / `ShallowRef` / `ComputedRef` — with
`WritableComputedRef` normalizing onto it — / `ModelRef` / `Reactive` /
`ShallowReactive`). Template-class classification and the value-signature return
position both consume those items; a second copy is a one-engine violation.

`wrapper_role_for_value_signature_return(dispatch, canonical, owner, symbol,
signature_ordinal)` is the return-position demand entry. It composes only landed
machinery: `prepared_value_decl` (a fact COPY — no locator deref, no resolution)
→ `signatures[ordinal].return_reference_head` →
`resolve_authored_reference_route` → `wrapper_candidate_for_route` →
`raise_semantic_type_source_to_hot` over
`SemanticTypeSource::Authored(AuthoredBodyLocator::DeclBody(signature.return_ty))`
→ `demand_terminal_symbol_instantiation` (`ProjectionMode::Navigate`). It is
NOT `typeof_key_for`, which yields the function's own type rather than its
return. Demand begins only for the requested subject; there is no owner-wide
wrapper-candidate scan, and the entry publishes nothing of its own.

Published states are exact: a proven package-backed `vue` terminal publishes its
role plus a complete `ReactiveWrapperImportProvenance`; a resolved non-Vue
terminal (or an authored `NotReference` return) publishes
`ReactiveWrapperRole::None` — a COMPLETE non-wrapper proof; every partial
(cycle, missing dependency, unsupported head, unavailable prepared declaration,
tripped work/budget envelope, or a terminal the graph did not reach) publishes
`Unresolved { reason }` with the exact typed reason and no provenance. `None` is
never used for a partial, and `Unresolved { AnalysisUnavailable }` is never used
as "not a wrapper".

Two landed boundaries, both fail-closed and both asserted rather than implied:
an OBJECT-MEMBER method typed `(): Ref<number>` never classifies (the
`has_authored_return` gate publishes `Unavailable`), and a package wrapper
authored as a TRANSPARENT ALIAS (`type Reactive<T> = Unwrapped<T>`) routes past
its own name to the alias terminal, which is outside the closed vocabulary, so
it publishes `None`.

**The consumer entry is `wrapper_role_for_sole_value_signature_return(dispatch,
canonical, owner, symbol)`** — the same composition, plus one closed rule: a
declaration carrying MORE THAN ONE signature is a merged overload group, and
which overload a call site selects requires argument-based overload resolution,
so it fails closed with `Unresolved { Unsupported }` rather than guessing ordinal
0. No signature at all is `AnalysisUnavailable`. It DELEGATES to the
ordinal-bearing entry, so there is exactly one composition.

**Production consumer: component-meta's script-binding surface.**
`host_manage/component_meta_extract.rs::resolved_binding_reactivity(ctx,
snapshot)` demands the role from inside `extract_component_meta_from_inputs`,
where every property the mechanism requires already holds: the caller's `ctx` is
request-bound (`is_request_bound() == true`, so a session overlay is visible to
the demand and the projection fuse is armed), the enclosing cold compute runs
under `with_fact_tracer`, and the `ColdComputeCompletenessScope` is already open.
It is NOT reachable from any context-free snapshot accessor — `get_analysis`,
`get_analysis_batch`, `get_analysis_all`, `eval_env`, and
`enrich_destructured_bindings` are all untouched, so LSP/MCP snapshot consumers
still read the value-space `reactivity_kind` only.

Demand is per requested subject with no owner-wide or workspace-wide scan. A
binding is demanded only when it is still `ReactivityKind::MaybeRef` (the
analyzer's undecided-composable state), its initializer is a `FunctionCall` that
binds the call's WHOLE result (`BindingInitializer::FunctionCall
{ binds_whole_call_result: true, .. }` — the parse-domain discriminator
`classify_initializer` sets and `extract_destructured_bindings` clears), and the
owner's own import row for that local name carries both a resolved canonical id
and an imported name. The imported ROOT is resolved through
`ResolverContext::resolve_imported_type_root_with_facts` (barrels and re-exports
included) and its route facts are recorded onto the active tracer, so a barrel
retarget misses the enclosing warm component-meta entry. One shared
`ProjectSemanticDispatch` serves the whole loop; each top-level route walk is its
own connected-demand root.

**Authority order is MONOTONE refinement, never override.** The value-space walk
runs first and unmodified; anything it decided keeps its answer. An exact role
maps onto the collapsed decoration kind (`Ref`/`ShallowRef`/`ModelRef` → `Ref`;
`ComputedRef` → `Computed`; `Reactive`/`ShallowReactive` → `Reactive`).
`ReactiveWrapperRole::None` refines NOTHING: it is a proof about the wrapper
FAMILY of the whole return type, and Vue's `reactive<T>()` returns
`UnwrapNestedRefs<T>` rather than `Reactive<T>`, so a proven non-wrapper return
type never downgrades a value-space `reactive` fact. A typed degradation refines
nothing either and additionally folds
`ResultCompleteness::partial(PROPAGATED)` into the cold-compute completeness, so
the component-meta result is refused warm admission — per-row and fail-closed:
one degraded binding never drops its own row and never suppresses a sibling's
exact role.

The exactness carrier is the add-only
`BindingAnalysis.return_wrapper_role: Option<ReactiveWrapperRole>` (absent = not
demanded, distinct from the `None` proof). Route PROVENANCE never crosses FFI —
`ReactiveWrapperImportProvenance` embeds `TypeArgLocator`s and stays
session-internal; only the closed role vocabulary crosses, as
`FfiBindingMeta.return_wrapper_role` + `.return_wrapper_unresolved_reason` and
proto `BindingMeta` fields 7/8 (`COMPONENT_META_SCHEMA_VERSION` 7).

The capability's product justification is a BODILESS declaration: the value path
(`build_composable_info`) is gated on a function body, so `declare function
useCounter(): Ref<number>` in a `.d.ts` can never be classified by it and today
publishes the undecided `MaybeRef`. Acceptance:
`component_meta_binding_return_wrapper_role_is_exact_and_degrades_typed` plus its
siblings in `crates/verter_session/src/host_manage_tests.rs`.


