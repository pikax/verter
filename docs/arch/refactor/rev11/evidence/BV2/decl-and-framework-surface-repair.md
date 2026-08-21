# BV2 items 9–10 — investigation trail and disposition

Scope: owned-scope items 9 (declaration-output call/function fidelity) and 10
(framework-surface memberless-runtime-macro gap) only. Items 1–8 (VDOM/SSR
codegen) belong to the sibling implementer in `verter-bv2`.

## Item 9 — declaration-output call/function fidelity

**Read first:** `crates/verter_compiler/src/tsc/script.rs` around
`expose_declaration_fallback`/`render_callable_shape`/`ExposeEntry` (comment at
what is now ~line 6024, matching the charter's `:6003`/`:6015` citations
almost exactly). The existing doc comment is a DELIBERATE, already-reasoned
policy: `unknown` for a non-callable member with nothing recoverable from
syntax, rather than inventing a type. It explicitly reasons about "the result
of inference this producer does not perform."

**Layering check (did the resolver lookup exist and this producer just wasn't
calling it?):** `verter_compiler` depends on `verter_semantic` only — it does
NOT depend on `verter_session` (checked `crates/verter_compiler/Cargo.toml`).
The shared query-time type resolver (`SemanticQueryKey` →
`ProjectSemanticDispatch::execute` → `SemanticGraphStore`) lives in
`verter_session` (`resolver_core`), a HIGHER crate. `tsc/script.rs` is a
codegen text producer inside `verter_compiler`; it cannot call upward into
`verter_session`'s resolver without an architecture-forbidden back-edge. So
"ask the shared resolver for the real inferred type" is not reachable from
this exact producer — confirmed by grep (`verter_session` does not appear in
`verter_compiler`'s dependency list) and by CLAUDE.md's crate-ownership rules
(`verter_semantic` + `verter_compiler` own lowering/codegen;
`verter_session::resolver_core` owns type-resolution orchestration).

**What WAS available and unused: authored TypeScript type annotations.**
Checked `verter_parser`'s `CallableParam`/`CallableShape`/`ScriptDeclaration`
(`crates/verter_parser/src/utils/oxc/vue/script/types.rs`) and their OXC
construction sites (`setup.rs`). Confirmed via the vendored `oxc_ast` 0.126.0
source (`FormalParameter.type_annotation`, `Function.return_type` /
`ArrowFunctionExpression.return_type`, `VariableDeclarator.type_annotation`)
that the parser discards every authored `: T` annotation on a callable
parameter, a function/arrow return type, and a `const`/`let`/`var`
declarator's own annotation — none of these spans were captured anywhere.
This is NOT a resolver gap; it's a producer-side lowering gap for an AUTHORED
annotation the codebase already has an established pattern for capturing
(`MacroTypeParams.type_span` + `content_str[span]` slicing, already used by
`process_expose`/`process_slots` in the same file; `MacroProperty.prop_type_annotation`
in `verter_parser`).

**Disposition: ADOPT-NOW, scoped to authored annotations.** Extended:
- `CallableParam::type_span: Option<Span>` — the param's own `: T` span.
- `CallableShape::return_type_span: Option<Span>` — the `): T` span.
- `ScriptDeclaration::type_annotation_span: Option<Span>` — the declarator's
  own `: T` span (`const count: Ref<number> = ref(0)`).

All three are populated at the SAME OXC construction sites that already
existed (`setup.rs::callable_shape`, `callable_shape_of_initializer`,
`process_variable_declaration`), by reading fields OXC already exposes on the
node — no new parsing, no re-entry into `oxc_parser`, no text scanning.

`render_callable_shape` (verter_compiler) now slices `content_str[span]`
verbatim per param / return position when a span is present, falling back to
`any` exactly as before when absent. `expose_declaration_fallback` gained a
third source: when the exposed member's referenced declaration has no
callable shape (it isn't a function) but DOES carry its own
`type_annotation_span`, that authored type renders verbatim instead of
falling through to `unknown`. An UNTYPED call-initialized value (`const count
= ref(0)`, no annotation) still falls back to `unknown`, unchanged — this is
the genuinely-unrecoverable residue the existing doc comment already
correctly identified; fixing it would require the exact resolver call-up this
producer cannot make.

This is copying AUTHORED SYNTAX verbatim, the same pattern this file already
uses pervasively (`ts_type` fields, `MacroTypeParams.type_span` slicing) —
not a second type resolver, not inference, not a Typed-IR-Only Resolver Rule
violation (that rule scopes the component-meta/typeinfo resolver pipeline in
`verter_session`; this is the codegen text producer one layer below, which
has never itself been a query-time resolver).

**Tests** (`crates/verter_compiler/src/tsc/tests.rs`, all TDD — confirmed
failing on unmodified HEAD before the fix, green after):
- `declaration_mode_expose_typed_function_recovers_authored_param_and_return_types`
- `declaration_mode_expose_untyped_function_still_falls_back_to_any_shape` (control)
- `declaration_mode_expose_typed_call_initialized_const_recovers_authored_type`
- `declaration_mode_expose_untyped_call_initialized_const_still_falls_back_to_unknown` (control)

Full `tsc::` (203 tests), `script::` (529 tests), `ide::script` (441 tests),
and `verter_parser`'s full suite (991 tests + doctests) all green after the
change — no regressions.

## Item 10 — framework-surface memberless-runtime-macro gap

**Where the guard actually is.** Charter cites
`typeinfo/framework_surface/vue_exec/mod.rs:533`. Confirmed in this tree at
`resolve_vue_macro_surface_with_ctx`: `if !mac.is_type_based { return None; }`.
Checked the Vue adapter's `plan_surfaces`
(`typeinfo/adapters/vue/adapter.rs`) FIRST, per the charter's literal
instruction to extend it — it already plans a `PlannedDemand::MacroPayload`
unconditionally for every macro kind (it has no snapshot access at plan
time, so it structurally CANNOT branch on `is_type_based` there). The actual
rejection is downstream, in the RESOLUTION path this file owns. **Charter
ambiguity, resolved:** the charter's "extend the adapter's `plan_surfaces`
step" is read as "the adapter's plan/normalize responsibility for this
demand," not literally the `plan_surfaces` function body — the fix lands in
the resolution half of that same adapter leg (`vue_exec`, still the Vue
adapter's own code, still not touching C3's demand vocabulary or the general
dispatcher). Recorded here per the brief's explicit instruction to record
this interpretation.

**Investigated the "already resolved elsewhere" hint.** Checked
`meta_resolve/projectors/exposed.rs` (`project_exposed`) — component-meta's
OWN `defineExpose` projector. It has the IDENTICAL `if !mac.is_type_based {
return Vec::new(); }` guard. So component-meta's SURFACE-based projector does
NOT resolve the runtime-object case either; per its own doc comment, the
runtime-object case is resolved by a SEPARATE, non-dispatch mechanism:
`verter_semantic::analysis::component_meta::extract_exposed_from_macro` +
`resolve_exposed_type`, which reads a pre-computed
`ExpandedComponentTypes.bindings` — the output of the FULL component-meta
materialization pipeline (`get_component_meta`), not the shared five-mode
`SemanticQueryKey` dispatch this charter names. Reusing that full pipeline
from the lean framework-surface executor would be a much heavier, differently
-shaped dependency than "the same shared five-mode dispatch every other macro
form uses," and risks exactly the kind of cross-pipeline coupling CLAUDE.md
warns against for this hardest-case surface.

**What IS a clean, structural, shared-dispatch-consistent fix, evaluated and
NOT taken (recorded per the disposition rule):** `TypeExpr::TypeOf(ValueRef)`
+ synthesizing a `{ name: typeof binding }` object type argument, routed
through the SAME `Instantiate`/surface machinery the type-based path uses,
would recover REAL per-member types for the common shorthand-identifier case.
This requires: (a) `AnalyzedExposeField`/`AnalyzedPropField` to carry which
local binding a runtime-object member's value expression names (today only
the parser's `MacroProperty.value_span`/`is_method`/`callable` carry that
shape, and it is not threaded onto the analyzer DTO), and (b) either
extending the single-producer-guarded structural lowerer
(`structural_carrier_producer::macro_arg_producer::lower_type_expr_structural`,
explicitly compiler-and-guard-confined to ONE producer — a second call site
is an architecture violation) or building new locator/lowering
infrastructure to feed a synthesized (non-authored-source) `TypeExpr` through
an equivalent path. This is real, but multi-crate, higher-risk surface work
I could not safely implement AND verify (the single-producer guards, the
`session_graph_lowerer_makes_no_query` invariant, and the broader
`verter_session` test suite are not something I can prove green without
running the full gate, which this brief explicitly excludes) within this
block's scope. **Disposition: DEFER** — real per-member runtime-object typing
is a follow-on, not silently dropped: it needs (1) the analyzer to carry a
member's referenced-binding name/shape and (2) a `TypeOf`-based per-member
demand routed through the shared dispatch, either via a new `PlannedDemand`
arm or an extension of the existing macro-payload synthesis. Named here as
the durable owner note; no debt-row identifier exists in this program's
ledger for it, since this evidence file IS the record per BV2's "preserve
evidentiary records" instruction.

**What WAS implemented (ADOPT-NOW): real member presence, honest placeholder
values.** `mac.expose_fields` / `mac.prop_fields` on
`verter_semantic::analysis::types::AnalyzedMacro` are ALREADY populated by
the analyzer for BOTH type-based and runtime-object macro forms (confirmed by
reading the field doc comments: "Individual prop fields from `defineProps`
(type-based and runtime)"). `resolve_vue_macro_surface_with_ctx` now, for a
non-type-based `DefineExpose`/`DefineProps` macro (structural dispatch on
`request.macro_kind`, a typed enum — never a name-string check), synthesizes
a one-level `SemanticNodeData::Object` surface directly from those
already-typed facts: one `SurfaceMember` per field, `key` = the field's real
name, `spans` = `MemberSpans::name_only(field.span)` (the EXACT helper this
codebase already documents for "an aggregate surface synthesized from
per-field analysis" — a direct, pre-existing precedent for this shape), and
`value` = an interned `PrimitiveKind::Unknown` placeholder (the same honest
"we do not re-derive this value's type here" policy `tsc/script.rs`'s
sibling fallback already applies to an unannotated call-initialized value).
The synthesized object is interned via the ordinary, non-single-producer-
guarded `SemanticGraphStore::intern_node` (an established pattern for ad-hoc
structural node construction outside the authored-source lowering path — see
`component_meta_materialize.rs`, `meta_resolve/graph_predicates.rs`,
`project_semantic_dispatch/absorb.rs` for existing direct `intern_node`
call sites building `Object`/`Primitive` nodes the same way), then projected
through the EXISTING, UNCHANGED `project_shallow_surface_from_base` — the
SAME one-level-surface projection `resolve_vue_public_type` already uses for
the synthesized `$props`/`$emit`/`$slots` instance object. No second
resolver, no second projector; only the member-list SOURCE is new (analyzer
facts instead of a lowered type argument), and it is fed into the identical
downstream machinery.

Only `DefineExpose`/`DefineProps` are handled (the two forms the ruling
names); every other non-type-based macro kind (`DefineEmits`, `DefineSlots`,
the `WithDefaults` outer macro) is unchanged (`None`, exactly as before) — no
change to C3's demand vocabulary or the general semantic dispatcher.

**Acceptance bar actually satisfied:** the charter's stated, measurable
defect is "publishes with zero members" / "never a memberless surface" — the
blast-radius acceptance table's item-10 row says exactly this (member
presence), unlike item 9's row which explicitly names "the real inferred or
signature-derived type." This fix satisfies the stated bar precisely; richer
per-member typing is the deferred follow-on above.

**Tests** (`crates/verter_session/src/typeinfo/typeinfo_tests/vue_adapter.rs`,
TDD, confirmed failing pre-fix / green post-fix):
- `runtime_object_define_expose_publishes_real_members_not_a_memberless_surface`
- `runtime_object_define_props_publishes_real_members_not_a_memberless_surface`
- `runtime_object_define_expose_empty_call_still_returns_none` (control — a
  bare `defineExpose()` with no members genuinely has nothing to surface,
  distinct from the memberless-surface defect this fix closes)

## Fix round 2 (architecture ruling applied)

A codex xhigh architecture consult ruled on both open dispositions above. Full transcript recorded
outside the repo (`/tmp/bv2-scope-consult.log` on the implementing machine); the two rulings and what
changed follow.

### Item 9 — DEFER rejected; untyped call-initialized/function residue now resolves for real

The round-1 DEFER was rejected: `unknown`/`(...): any` for an unannotated `const count = ref(0)` /
untyped `function` still blocks BV2 acceptance, and the round-1 crate-layering diagnosis
(`verter_compiler` cannot call up into `verter_session`) — while correct — missed that the codebase
already has the intended downward semantic handoff for exactly this: the dependency-neutral
`verter_macro_dto` DTO both crates already share for Props/Emits/Model. The ruling's concrete repair
landed as specified:

**`verter_semantic`** — `AnalyzedExposeField` gained `referenced_binding: Option<String>`
(`analysis/types.rs`), populated STRUCTURALLY in `extract_expose_fields`
(`analysis/macros.rs`) by reading the OXC property's own `p.method`/`p.value` fields directly — never
sliced from source text, never guessed from a name pattern. A shorthand member (`{ foo }`) and an
explicit non-method identifier member (`{ myVal: val }`) both carry an OXC `Expression::Identifier`
value node, so one match arm covers both forms; a method or any other expression shape yields `None`.

**`verter_macro_dto`** — `MacroTscProjection` gained an `Expose(TscExposeProjection)` variant:
`TscExposeProjection { members: Vec<TscExposeMemberRow>, scope: TscScopeRequirements }`,
`TscExposeMemberRow { name, member_type: TscExposeMemberType, anchor }`, and the closed
`TscExposeMemberType { Resolved(TscSpliceText), Unavailable(TscDeclarationFailureReason) }` — reusing
the EXISTING `TscDeclarationFailureReason` taxonomy (`SemanticInferenceUnavailable` /
`Unsupported` / `Unresolved`) rather than inventing a parallel one, and reusing the SAME
`TscScopeRequirements` shape Props/Emits/Model already carry.

**`verter_session`** (`typeinfo/vue_macro_codegen.rs` + the sibling `tsc_projection.rs`) — `DefineExpose`
stays deliberately OUT of `is_codegen_macro` (it has no runtime `props`/`emits` shape, so it must not be
forced through the payload/runtime-projection flow those three roles share). Instead, a DEDICATED branch
in the producer loop handles a runtime-object `defineExpose` (non-type-based, with at least one member)
as its own TSC-only lane: `project_expose_runtime_object` resolves each member with a captured
`referenced_binding` through the EXISTING `TypeOf` query built by
`ProjectSemanticDispatch::typeof_key_for` — the same dispatcher capability
`resolve_svelte_value_export_member` already uses for Svelte instance exports, not a new resolver and not
a change to C3's demand vocabulary. A member with no capturable binding (method, non-identifier value) or
a genuine `TypeOf` miss reports a typed `Unavailable(TscDeclarationFailureReason)` row — never a silent
`unknown` disguised as success. `tsc_scope_requirements` (previously hard-wired to read `mac.type_references`)
was refactored, with NO logic change, into `tsc_scope_requirements_for` taking an explicit name-list
parameter; the Expose lane feeds it names collected from each resolved member's node via the existing
`resolver_core::component_meta_registry::collect_node_ref_names` (the same utility Svelte's resolver
uses), so a resolved type that references an importable/local name still gets it retained in scope
exactly like Props/Emits/Model do. The cancelled/terminal-partial fallback path was updated in lockstep
so a runtime-object `defineExpose` always advertises a matching (Partial, on that path) TSC entry.

**`verter_compiler`** (`tsc/script.rs`) — `TscMode::Public`'s "setup body IS emitted" path
(`expose_typeof_resolvable == true`) was ALREADY correct before this round: for a TypeScript-dialect SFC
with any expose entries, `needs_setup_body` is always true, so the generated surface already emits
`count: typeof count` and TypeScript's own checker infers the real type from the copied setup body — no
change needed there. The genuine residue was entirely `TscMode::Declaration` (the setup body is OMITTED,
so `typeof <ident>` is unavailable and the renderer falls back to the syntax-derived
`ExposeEntry::declaration_fallback`). `TscMacroState` gained `expose_syntax_index: Option<u32>`, set by
`build_macro_state` only for the runtime-object form; a new `apply_expose_bundle_entry`, called from
`apply_tsc_bundle`, is a BEST-EFFORT enrichment (not the mandatory `TscSemanticSlot` machinery Props/
Emits/Model use) that overwrites a matching `expose_entries[i].declaration_fallback` with the DTO's
`Resolved` text when present, and leaves the EXISTING syntax-derived fallback untouched for
`Unavailable`/absent rows. Best-effort by design: a runtime-object `defineExpose` never had a macro type
argument, so it never participated in the strict slot/`MissingEntry`/`DuplicateEntry` contract, and it
must not start requiring one now — a caller asserting `MacroTscInput::NotRequired` for a file whose only
macro is a runtime-object `defineExpose` keeps compiling exactly as before this producer existed.
`render_instance_shape_body` needed NO change: it already consumed `declaration_fallback` for the
non-typeof-resolvable branch; only the SOURCE of that value was upgraded.

**Tests** (new/renamed, all TDD — confirmed the new resolver-backed assertions failing red against the
pre-round-2 tree before implementing):
- `crates/verter_compiler/src/tsc/tests.rs` (compiler-side consumption, via hand-built bundles):
  `declaration_mode_expose_runtime_object_const_resolves_authoritative_type`,
  `declaration_mode_expose_runtime_object_computed_resolves_authoritative_type`,
  `declaration_mode_expose_runtime_object_function_resolves_authoritative_signature`,
  `declaration_mode_expose_runtime_object_unavailable_member_falls_back_to_syntax_derived_type`,
  `declaration_mode_expose_runtime_object_retains_referenced_local_type_declaration`,
  `declaration_mode_expose_runtime_object_without_bundle_still_compiles` (backward-compat control);
  `declaration_mode_expose_untyped_call_initialized_const_still_falls_back_to_unknown` RENAMED to
  `declaration_mode_expose_untyped_call_initialized_const_falls_back_to_unknown_without_a_bundle`,
  narrowed to document it exercises the NO-SESSION-BACKING (`NotRequired`) path specifically, distinct
  from the new resolver-backed tests.
- `crates/verter_session/src/typeinfo/typeinfo_tests/vue_macro_codegen_expose.rs` (new sibling test
  module, session-side real end-to-end resolution against a live `VerterHost`; fixtures use LOCALLY
  DEFINED functions rather than the real `vue` package to avoid depending on package resolvability in
  the standalone-host test harness — the mechanism exercised is identical either way):
  `runtime_object_expose_call_initialized_const_resolves_real_type`,
  `runtime_object_expose_unannotated_function_resolves_real_signature`,
  `runtime_object_expose_method_shorthand_reports_typed_unavailable`,
  `runtime_object_expose_non_identifier_value_reports_typed_unavailable`,
  `runtime_object_expose_resolved_type_expands_a_referenced_local_interface`,
  `runtime_object_expose_shorthand_and_explicit_identifier_both_resolve`.

Full `verter_compiler` `tsc::tests` (209 passed), `verter_session` `typeinfo::` (712 passed) and
`framework_surface` (27 passed), `verter_semantic` (1633 + 1 corpus test), and `verter_macro_dto`
(10, including the marker-witness/dependency-closure guards over the new DTO types) all green.

### Item 10 — member presence accepted; the two verification-gap proofs added

The round-1 fix (real member names via analyzer facts, honest `unknown` placeholder values, routed
through the existing `project_shallow_surface_from_base`) was ACCEPTED as-is — no further per-member
typing required. The ruling flagged two verification gaps; both are closed:

**Proof 1 — the AUDITED public entry.** `runtime_object_define_expose_publishes_real_members_through_the_audited_entry`
(`crates/verter_session/src/typeinfo/typeinfo_tests/vue_adapter.rs`) drives a real
`GRAPH_OPERATION_FRAMEWORK_SURFACES` `TypeInfoGraphRequest` envelope through
`VerterHost::resolve_framework_surface_with_audit` (not the internal `resolve_vue_macro_surface` round-1
tested) and decodes the wire `TypeInfoGraphResponse`, confirming the SAME `bump`/`count` members publish
through this route too.

**Proof 2 — the LSP framework-surface request route: FINDING, not silently substituted.** Inspection of
`crates/verter_lsp/src/` found NO reference to `resolve_framework_surface_with_audit`,
`FrameworkSurfacePayload`, `TypeInfoGraphRequest`, or `GRAPH_OPERATION_FRAMEWORK_SURFACES` anywhere —
`verter_lsp` has no framework-surface / typeinfo-graph request handler at all today. `CLAUDE.md`'s own
"Framework Adapter Substrate" section names `resolve_framework_surface_with_audit` as the SOLE audited
wire entry for this operation, and its actual currently-wired production callers are `verter_napi` and
`verter_wasm` (confirmed by direct usage grep), not the LSP. Building a new LSP handler for this operation
would be new wire surface, outside this fix's authorized scope (no change to C3's demand vocabulary or
the general dispatcher, no new public option/product). Disposition: **ADOPT-NOW, substituted route** — a
route-level proof through `verter_napi`, the operation's actual production consumer, exercised through
the FULL encode → NAPI call → decode round-trip:
`resolve_framework_surface_with_audit_publishes_real_runtime_object_expose_members`
(`crates/verter_napi/src/lib.rs`, in the existing `mod tests`). This satisfies the ruling's own hedge
("if so... don't invent a parallel resolution path") applied to the actual state of the tree: the ONE
wire route that exists is proven end-to-end; no second route was invented to satisfy the letter of "LSP"
where none exists. If/when an LSP framework-surface route is added, it is expected to reach the exact
same audited entry and this finding can be closed with a mirroring LSP-side test.

Full `verter_session` `typeinfo::typeinfo_tests::vue_adapter` (4 relevant tests) and `verter_napi`
(the new test, targeted run) green.

### What was NOT touched (round 2)

Same constraints as round 1, reaffirmed: no text/regex heuristics, no name-suffix role inference, no new
resolver engine (the Expose lane routes through the pre-existing `TypeOf`/`typeof_key_for` dispatcher
capability), no change to C3's demand vocabulary or the general semantic dispatcher, no silent `unknown`
presented as success where a typed `Unavailable` is the honest answer. The type-argument form
(`defineExpose<T>()`) is completely unchanged — it still splices `expose_type_text` verbatim from
authored syntax and never touches the new DTO path.

### Verification note

`node scripts/gate.mjs` was NOT run (excluded by this round's brief). `cargo test --package verter_lsp`
was run and shows 184 pre-existing, unrelated failures — every one panics at
`crates/verter_lsp/src/test_harness_fixture_dependencies.rs:622` on
`canonicalize real-provider fixture dependency .../packages/types/node_modules/vue: No such file or
directory` — a missing `pnpm install` artifact in this bare worktree (this exact class is recorded
separately as an environmental gate-baseline gap, not a code regression: none of the 184 failing test
names reference `expose`/`vue_macro`/`framework_surface`/`tsc_`/`declaration_mode`, confirmed by grep over
the full failure list). All other requested verification commands (`verter_compiler` `tsc::`,
`verter_session` `framework_surface`, `verter_session` `typeinfo`, `verter_semantic`, `verter_macro_dto`)
ran clean with zero failures. `cargo clippy` over every touched crate (`verter_session`,
`verter_compiler`, `verter_semantic`, `verter_macro_dto`, `verter_napi`) and `cargo fmt --all -- --check`
are both clean.

## Second fix pass — leaked-sentinel defect, item 9 real-route coverage, item 10 route gaps

An independent three-mandate review (conformance / architecture / adversarial) of the round-2 tip
found one blocking correctness defect and several verification-coverage gaps. This section records
their disposition.

### Blocking defect: `project_expose_runtime_object` could splice an internal sentinel into real output

**Root cause.** `render_tsc_node` (the shared raise/render terminal sink every macro-codegen
producer calls) can return `Ok(text)` even when a NESTED resolver degradation (`QueryError::Miss`,
`QueryError::UnmodeledPosition`, and other members of the same "unmaterialized-sentinel" family —
see `crate::project_semantic_dispatch::raise_sentinel::query_error_is_unmaterialized_sentinel`)
occurs deep inside a structurally-materialized instantiated type. The degradation does not always
bubble up to an overall `Err` — it can be baked into the returned text as the literal internal
compat-projection sentinel spelling instead (e.g. `semanticMiss`, `unmodeledPosition`). This is a
pre-existing, general characteristic of the shared raise pipeline (reproduced with NO Vue types at
all via a purely local generic-closure pattern) — genuinely out of this producer's scope to fix at
the root. What IS in scope: `project_expose_runtime_object` is the producer that turns that `Ok(text)`
into published `TscExposeMemberType::Resolved` declaration output, and it was trusting `Ok(text)`
unconditionally. Two independently-reachable real trigger shapes were confirmed:

- `defineExpose({ someComputed })` where `someComputed = computed(() => ...)` — the charter's own
  headline `ref`/`computed` shape — leaked `semanticMiss` via a nested `Miss` inside the
  closure-inferred `ComputedRef<T>` instantiation.
- An unannotated `function` whose return position is a binary arithmetic expression over a typed
  parameter (`function bump(step: number) { return step + 1 }`) leaked `unmodeledPosition` via the
  flow-return substrate's inability to model that position. This same shape was, unnoticed, already
  present in the round-2 test suite: `runtime_object_expose_unannotated_function_resolves_real_signature`
  asserted only `text.contains("step: number") && text.contains("number")`, which the leaked
  `"(step: number) => unmodeledPosition"` string satisfies trivially (the parameter slice
  `"step: number"` already contains the substring `"number"`) — a non-discriminating assertion that
  masked the same defect class inside an already-landed, already-green test.

**Fix.** `project_expose_runtime_object` (`crates/verter_session/src/typeinfo/vue_macro_codegen.rs`)
now screens every `Ok(text)` from `render_tsc_node` through a new shared predicate,
`compat_spelling::text_embeds_unmaterialized_sentinel` (`crates/verter_session/src/semantic_query/compat_spelling.rs`
— the existing single owner of the legacy sentinel-spelling family). The predicate checks for a
STANDALONE-TOKEN embedded occurrence (never a substring of a longer identifier) of any spelling in
the exact `QueryError` "unmaterialized-sentinel" class — deliberately EXCLUDING the
"materialized-placeholder" family (`RaiseMiss`, `TypeParamCycle`, `RecursiveRef`,
`ValueDomainMismatch`, `DeclPlaceholder`, `Other`), which is legitimate by-design tree content, not a
leak. A match routes the member to `TscExposeMemberType::Unavailable(Unresolved(MissingDeclaration))`
— the same honest degradation the pre-existing `Err`/`QueryResult::Error` arms already produce —
instead of publishing the leaked text. This is a narrow, local guard at the ONE producer that
publishes this DTO row; it does not attempt to fix the deeper raise-pipeline gap (bubbling every
nested miss through `render_tsc_node` as a proper `Err` for all callers), which is recorded here as a
named, legitimate DEFER — a pre-existing defect this round's new code path made newly reachable and
newly publishable, not something this round introduced, and not safely fixable inside this producer's
own scope.

**Tests** (TDD, mutation-confirmed: reverting the guard makes exactly these three go red with the
leaked text visible in the panic message, restoring makes them green again):
- `runtime_object_expose_generic_closure_inferred_member_never_leaks_compat_sentinel`
  (`vue_macro_codegen_expose.rs`) — the pure-local-generic `wrap<T>` repro (no Vue types), isolating
  the root cause.
- `runtime_object_expose_function_arithmetic_return_position_reports_typed_unavailable_not_leaked_sentinel`
  (`vue_macro_codegen_expose.rs`) — the `unmodeledPosition` variant.
- `expose_untyped_computed_call_never_leaks_compat_sentinel_through_tsc_public_and_declaration_routes`
  (`host_resolve_tests.rs`) — the real `computed()`-via-`vue`-fixture reproduction, checked across all
  three production routes.

The pre-existing `runtime_object_expose_unannotated_function_resolves_real_signature` was corrected in
place: its body changed from `return step + 1` (which now correctly degrades to `Unavailable`) to
`return step` (an identity return the flow-return substrate CAN model), and its assertion tightened
from the non-discriminating substring check to an exact-string `assert_eq!` so a future regression
back to a leaked sentinel fails loudly instead of passing by coincidence.

### Item 9 — real host-driven end-to-end coverage added

New tests in `crates/verter_session/src/host_resolve_tests.rs` drive a real `VerterHost` +
`MemoryWorkspace`-backed fixture (a local `/workspace/node_modules/vue/index.d.ts` carrying `Ref`/
`ComputedRef`/`ref`/`computed`, wired via `set_import_dependencies` — the same fixture pattern
already used in `host_manage_tests.rs`) through the FULL production route
(`produce_vue_macro_codegen_with_ctx` for the session-side TSC row, `get_public_api_with_mode` for
compiled `PublicApi`/`Declaration` output) — never a hand-injected `MacroTscBundle`. Every test
cross-checks that the session-side TSC row's resolved text is EXACTLY what lands in the compiled
declaration output (`decl.contains(format!("{{name}}: {{tsc_text}}"))`), proving the wiring, not just
"some real type published somewhere":
- `expose_untyped_ref_call_resolves_real_type_through_tsc_public_and_declaration_routes`
- `expose_typed_ref_call_resolves_real_type_through_tsc_public_and_declaration_routes`
- `expose_untyped_computed_call_never_leaks_compat_sentinel_through_tsc_public_and_declaration_routes`
- `expose_untyped_function_resolves_real_signature_through_tsc_public_and_declaration_routes`
- `expose_typed_function_resolves_real_signature_through_tsc_public_and_declaration_routes`

`PublicApiMode::Public` output for expose members always resolves via `typeof <ident>` against the
copied setup body (real downstream `tsc` does the resolution, not this producer) whenever the setup
body is emitted, which it always is for a component with any expose entries — confirmed by reading
`render_instance_shape_body`'s `expose_typeof_resolvable` gate. So the producer's resolved DTO type is
consumed ONLY by `PublicApiMode::Declaration` (setup body omitted); the `Public`-mode assertions above
are a regression/no-leak check on that route, not a second consumption path for the DTO.

### Item 10 — remaining route gaps closed

**Audited-direct `defineProps`.** `runtime_object_define_props_publishes_real_members_through_the_audited_entry`
(`crates/verter_session/src/typeinfo/typeinfo_tests/vue_adapter.rs`) mirrors the existing `defineExpose`
audited-direct test — the entry-point helper was generalized (`vue_expose_members_via_audited_entry` →
`vue_surface_members_via_audited_entry(host, canonical_id, kind)`) rather than duplicated.

**LSP route — resolved by consult.** A follow-up architecture consult on this specific ambiguity ruled
the charter's "via LSP framework-surface request" acceptance-table cell is NON-APPLICABLE AS WRITTEN
(no LSP handler for `resolve_framework_surface_with_audit` exists anywhere in the codebase, and
building one is new wire surface outside this fix's scope) and directed accepting the existing NAPI
production-wire test as the substitute proof for BOTH macro forms. Per that ruling:
`resolve_framework_surface_with_audit_publishes_real_runtime_object_props_members`
(`crates/verter_napi/src/lib.rs`) was added as the sibling to the existing `defineExpose` NAPI
round-trip test; both now share one `runtime_object_members_via_napi_wire_round_trip` helper
(encode → `NapiVerterHost::resolve_framework_surface_with_audit` → decode, parameterized on source and
`FrameworkSurfaceKind`) instead of duplicating the envelope-construction boilerplate.

**Blast-radius table correction.** Item 10's framework-surface invocation cross should read "via
existing production transport binding (NAPI, substituted for the non-existent LSP route per ruling)"
rather than literally claiming LSP coverage — recorded here as the authoritative correction; the
in-code comments at the `vue_adapter.rs`/`verter_napi/src/lib.rs` audited-entry test sites were also
rewritten to drop the plan/round vocabulary the architecture review flagged (they still name the
finding — no LSP handler exists — without naming which fix pass or block found it).

### Verification note (second pass)

`node scripts/gate.mjs` was NOT run (excluded by this pass's brief, same as round 2). Targeted suites
run clean: `verter_compiler` `tsc::`, `verter_session` `typeinfo::`, `verter_session` `runtime_object`,
`verter_semantic` (full), `verter_macro_dto` (full), `verter_napi` (full). `cargo clippy` over every
touched crate (`verter_session`, `verter_compiler`, `verter_semantic`, `verter_macro_dto`,
`verter_napi`) and `cargo fmt --all -- --check` are both clean.
