# Component-Meta Shallow-By-Default Rule

The projector pipeline is the sole post-projection authority for finalising published field types in `verter_session`. Bare alias references — same-file or imported — stay shallow at the published surface; the projector does NOT eagerly inline alias bodies.

## Architectural rule

Types and properties are ALWAYS published shallow at the projector surface UNLESS the consumer explicitly walks the path.

## Concrete contract

- Plain alias references (`type Foo = ...`, same-file or imported) — published as `TypeExpr::Ref { name: "Foo" }`. Consumers re-resolve through the registry on demand. The projector does NOT eagerly inline the alias body.
- `Pick<Foo, "bar">` — materialises ONLY the `bar` member of Foo. Built-in utility types (`Pick`, `Omit`, `Required`, `Partial`) behave identically to a userland implementation referencing the same keys.
- `Omit<Foo, "bar">` — keeps `bar` shallow (excluded from surface) and materialises the others.
- `Foo['a']['b']` — path-precise: only `a` and `b` hops load; other Foo keys never enter the surface.
- True recursive types (`type Self = Pick<Self>`) — NOT supported. Stays as bare `Ref`.
- Recursive parameterised generic helpers (`type GetItemKeys<T> = ... GetItemKeys<...> ...`) — stay as the bare carrier `Ref { name, type_arguments }`. The shared transitive-cycle guard short-circuits reduction so the helper is not partially expanded.
- Imported alias names (workspace-owned OR package-backed) — stay shallow regardless of where they live.

## Implementation entry-points

- `crates/verter_session/src/meta_resolve/projectors/mod.rs::reduce_field_type_expr` — per-field reduction primitive. Reduces ONLY operator-shape inputs and generic instantiations (Ref with non-empty type_arguments). Bare Ref inputs are returned verbatim.
- `crates/verter_session/src/meta_resolve/projectors/mod.rs::reduce_published_field_types` — pipeline step that runs reduction over `props`, `emits`, `slot_bindings`, `bindings` and back-syncs `define_props.result.value.properties[name].ty`.
- `crates/verter_session/src/meta_resolve/projectors/mod.rs::merge_projected_fields_by_name` — merge logic. Projector output ALWAYS replaces parser-side pre-population for fields the projector produced (the projector is the sole post-projection authority); fields the projector did NOT produce are preserved.
- `crates/verter_session/src/host_manage/component_meta_methods.rs` (around line 620) — invocation site after `slot_binding_graph::resolve_slot_bindings_graph_native`.

## Predicate

`reduce_field_type_expr` triggers reduction when EITHER:
1. The expression contains an operator-shape node (`IndexedAccess`/`KeyOf`/`TypeOf`/`Conditional`/`Mapped`/`Infer`/`Rest`/`TemplateLiteral`).
2. The expression is a generic instantiation (`Ref` with non-empty `type_arguments`) AND its root does NOT reach a transitive cycle.

A bare `Ref` (empty `type_arguments`) NEVER reduces — the alias name publishes as a carrier per the shallow-by-default rule.

## Negative tests (locked invariants)

- `crates/verter_session/src/meta_tests.rs::published_bare_alias_ref_stays_shallow`
- `crates/verter_session/src/meta_tests.rs::published_same_file_alias_stays_shallow`
- `crates/verter_session/src/meta_tests.rs::pick_materialises_only_named_keys_others_stay_shallow`
- `crates/verter_session/src/meta_tests.rs::omit_excludes_named_keys_others_materialise`
- `crates/verter_session/src/meta_tests.rs::nested_indexed_access_publishes_only_terminal_path`
- `crates/verter_session/src/meta_tests.rs::projector_reduces_same_file_alias_via_pick_indexed_access`

## Retired-symbol guards

`crates/verter_session/tests/no_legacy_walker.rs` `RETIRED_SYMBOLS` list locks down:
- `materialize_component_meta_field_types`
- `rescue_field`
- `MEMBER_ROUTE_CALLS_COUNTER`
- `COMPONENT_CONFIG_FAST_PATH_HITS_COUNTER`
- `component_config_theme_variant_fast_path` and helpers
- All per-block instrumentation timer counters that observed the rescue cascade

Re-introducing any of these symbols outside `docs/arch/debt-closure/` triggers the static-grep gate.

## Architecture guard tests

- `cargo test -p verter_session --test no_legacy_walker no_legacy_walker_inner_helpers_outside_their_definitions` — passes.
- `cargo test -p verter_session no_phase_archaeology_in_production_code_broader_d111` — passes.
