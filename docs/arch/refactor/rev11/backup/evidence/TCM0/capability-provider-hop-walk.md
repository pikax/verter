# Derived: does a `TypeProvider` hop exist on each capability's request path?

**This file is generated. Do not edit it.** Regenerate with `node docs/arch/refactor/rev11/evidence/TCM0/probes/capability-provider-hop-walk.mjs`; falsify with `--check`, which re-derives from the live tree and exits 1 on any drift.

The `TypeProvider` method names are read out of the trait body in `crates/verter_type_runtime/src/traits.rs` (44 methods) — never typed in. 20837 non-test `fn` definitions were indexed across 3130 `.rs` files. From each capability's entry point the walk follows every callee name that resolves in that index, breadth-first, and reports the SHORTEST path to a hop.

A **hop** is either:

- `trait-method-call` — `<receiver>.<m>(` or `<path>::<m>(` where `m` is a derived trait method;
- `provider-handle` — a read of the `type_provider` field/accessor or a mention of the `TypeProvider` type, i.e. obtaining a provider at all, before any call.

## What this walk cannot see

- **It over-approximates (L1).** Edges resolve by NAME, so a common name links unrelated definitions. **A reported hop is a candidate, not a finding** — the full source line is printed so it can be read before it is believed.
- **It under-approximates (L2).** A call reached only through `dyn Trait` dispatch, a stored closure, or a macro-pasted name is not an edge. A `NO-HOP` verdict is strong (the walk is otherwise generous) but is not a proof, and each one is paired in the ledger with a read of the entry point's own body.
- **It is capped (L3).** 20000 functions per capability; a capped walk reports `CAP`, never `NO-HOP`.
- **`crates/` only, non-test definitions only (L4).**

## Verdicts

| capability | verdict | fns explored | unresolved receivers | of those, hop-bearing | first hop |
|---|---|---|---|---|---|
| rename preparation | **HOP** | 5 | 14 | 0 | `type_provider` (provider-handle) at `crates/verter_lsp/src/server/rename_prepare.rs:161` |
| formatting (+ on-type) | **NO-HOP** | 61 | 41 | 1 | — |
| call hierarchy | **NO-HOP** | 21 | 28 | 1 | — |
| code lens | **NO-HOP** | 24 | 30 | 1 | — |
| folding | **NO-HOP** | 15 | 24 | 0 | — |
| selection ranges | **NO-HOP** | 15 | 24 | 1 | — |
| document symbols | **NO-HOP** | 32 | 33 | 1 | — |
| component surface resolution | **HOP** | 116 | 69 | 3 | `close_file` (trait-method-call) at `crates/verter_session/src/host_lifecycle.rs:986` |
| template expression typing | **NO-HOP** | 24 | 21 | 1 | — |
| props | **NO-HOP** | 15 | 26 | 1 | — |
| events | **NO-HOP** | 7 | 10 | 1 | — |
| slots and snippets | **NO-HOP** | 2 | 7 | 0 | — |
| directives | **NO-HOP** | 3 | 6 | 0 | — |
| framework macros | **HOP** | 1 | 0 | 0 | `type_provider` (provider-handle) at `crates/verter_lsp/src/features/macro_actions.rs:132` |
| auto-imports (Verter-owned half) | **NO-HOP** | 77 | 56 | 2 | — |
| background semantic analysis (Verter-owned lane) | **HOP** | 645 | 209 | 3 | `close_file` (trait-method-call) at `crates/verter_session/src/host_lifecycle.rs:986` |
| provider-adjacent caches | **NO-HOP** | 3 | 0 | 0 | — |

**4 of 17 capabilities reach a provider hop; 13 do not.** A uniform verdict over all of them is therefore not available from the tree.

## Per capability

### rename preparation — HOP

Entry points walked:

- `crates/verter_lsp/src/server/rename_prepare.rs:96` — `fn handle_prepare_rename`

Shortest path from an entry point to the hop:

1. `crates/verter_lsp/src/server/rename_prepare.rs:96` — `fn handle_prepare_rename`
2. `crates/verter_lsp/src/server/rename_prepare.rs:155` — `fn provider_proves_rename_target`

Hop: **`type_provider`** (provider-handle) at `crates/verter_lsp/src/server/rename_prepare.rs:161`

```rust
let Some(type_provider) = &server.type_provider else {
```

Unresolved receivers (method calls whose receiver type is not recoverable from text, so the walk did NOT follow them): **14** distinct names.

**None of them names a function that would itself have been a hop.** Every indexed function whose own body contains a provider hop was collected up front; the unresolved set for this capability does not intersect it. That is what makes the verdict above load-bearing rather than an artefact of where the walk stopped.

### formatting (+ on-type) — NO-HOP

Entry points walked:

- `crates/verter_lsp/src/features/formatting.rs:23` — `fn format_document`
- `crates/verter_lsp/src/server/aux_features.rs:1108` — `fn handle_formatting`
- `crates/verter_lsp/src/server/aux_features.rs:1158` — `fn handle_on_type_formatting`

No provider hop is reachable. 61 functions were explored to exhaustion — the walk ran out of reachable callees, it was not cut short.

Unresolved receivers (method calls whose receiver type is not recoverable from text, so the walk did NOT follow them): **41** distinct names.

**1 of them name a function that WOULD itself have been a hop** — the walk stopped exactly where a hop could hide, so this verdict is not complete on its own and needs a read:

- `new` — 1 of 468 definition(s) with that name contain a hop; 11 unfollowed call site(s). A name this heavily overloaded flags on collision, not on reachability

### call hierarchy — NO-HOP

Entry points walked:

- `crates/verter_lsp/src/features/call_hierarchy.rs:15` — `fn prepare_call_hierarchy`
- `crates/verter_lsp/src/features/call_hierarchy.rs:107` — `fn incoming_calls`
- `crates/verter_lsp/src/features/call_hierarchy.rs:157` — `fn outgoing_calls`
- `crates/verter_lsp/src/server/aux_features.rs:1213` — `fn handle_prepare_call_hierarchy`
- `crates/verter_lsp/src/server/aux_features.rs:1238` — `fn handle_incoming_calls`
- `crates/verter_lsp/src/server/aux_features.rs:1263` — `fn handle_outgoing_calls`

No provider hop is reachable. 21 functions were explored to exhaustion — the walk ran out of reachable callees, it was not cut short.

Unresolved receivers (method calls whose receiver type is not recoverable from text, so the walk did NOT follow them): **28** distinct names.

**1 of them name a function that WOULD itself have been a hop** — the walk stopped exactly where a hop could hide, so this verdict is not complete on its own and needs a read:

- `new` — 1 of 468 definition(s) with that name contain a hop; 2 unfollowed call site(s). A name this heavily overloaded flags on collision, not on reachability

### code lens — NO-HOP

Entry points walked:

- `crates/verter_lsp/src/features/code_lens.rs:13` — `fn code_lenses`
- `crates/verter_lsp/src/server/aux_features.rs:828` — `fn handle_code_lens`

No provider hop is reachable. 24 functions were explored to exhaustion — the walk ran out of reachable callees, it was not cut short.

Unresolved receivers (method calls whose receiver type is not recoverable from text, so the walk did NOT follow them): **30** distinct names.

**1 of them name a function that WOULD itself have been a hop** — the walk stopped exactly where a hop could hide, so this verdict is not complete on its own and needs a read:

- `new` — 1 of 468 definition(s) with that name contain a hop; 9 unfollowed call site(s). A name this heavily overloaded flags on collision, not on reachability

### folding — NO-HOP

Entry points walked:

- `crates/verter_lsp/src/features/folding_range.rs:14` — `fn build_folding_ranges`
- `crates/verter_lsp/src/server/aux_features.rs:90` — `fn handle_folding_range`

No provider hop is reachable. 15 functions were explored to exhaustion — the walk ran out of reachable callees, it was not cut short.

Unresolved receivers (method calls whose receiver type is not recoverable from text, so the walk did NOT follow them): **24** distinct names.

**None of them names a function that would itself have been a hop.** Every indexed function whose own body contains a provider hop was collected up front; the unresolved set for this capability does not intersect it. That is what makes the verdict above load-bearing rather than an artefact of where the walk stopped.

### selection ranges — NO-HOP

Entry points walked:

- `crates/verter_lsp/src/server/aux_features.rs:112` — `fn handle_selection_range`

No provider hop is reachable. 15 functions were explored to exhaustion — the walk ran out of reachable callees, it was not cut short.

Unresolved receivers (method calls whose receiver type is not recoverable from text, so the walk did NOT follow them): **24** distinct names.

**1 of them name a function that WOULD itself have been a hop** — the walk stopped exactly where a hop could hide, so this verdict is not complete on its own and needs a read:

- `new` — 1 of 468 definition(s) with that name contain a hop; 2 unfollowed call site(s). A name this heavily overloaded flags on collision, not on reachability

### document symbols — NO-HOP

Entry points walked:

- `crates/verter_lsp/src/features/document_symbol.rs:16` — `fn build_document_symbols`
- `crates/verter_lsp/src/server/aux_features.rs:40` — `fn handle_document_symbol`

No provider hop is reachable. 32 functions were explored to exhaustion — the walk ran out of reachable callees, it was not cut short.

Unresolved receivers (method calls whose receiver type is not recoverable from text, so the walk did NOT follow them): **33** distinct names.

**1 of them name a function that WOULD itself have been a hop** — the walk stopped exactly where a hop could hide, so this verdict is not complete on its own and needs a read:

- `new` — 1 of 468 definition(s) with that name contain a hop; 6 unfollowed call site(s). A name this heavily overloaded flags on collision, not on reachability

### component surface resolution — HOP

Entry points walked:

- `crates/verter_lsp/src/server/custom_methods/component_meta.rs:110` — `fn get_component_meta_surface`
- `crates/verter_session/src/typeinfo/framework_surface/executor.rs:91` — `fn resolve_framework_surface_with_audit`

Shortest path from an entry point to the hop:

1. `crates/verter_session/src/typeinfo/framework_surface/executor.rs:91` — `fn resolve_framework_surface_with_audit`
2. `crates/verter_session/src/typeinfo/framework_surface/executor.rs:202` — `fn execute_framework_surface`
3. `crates/verter_session/src/host_manage/prepared_decl.rs:1829` — `fn ensure_indexed_ready_serve`
4. `crates/verter_session/src/host_manage/prepared_decl.rs:1899` — `fn ensure_indexed_ready_serve_uninstrumented`
5. `crates/verter_session/src/host_lifecycle.rs:954` — `fn ensure_loaded`

Hop: **`close_file`** (trait-method-call) at `crates/verter_session/src/host_lifecycle.rs:986`

```rust
self.scheduler.close_file(canonical_id);
```

Unresolved receivers (method calls whose receiver type is not recoverable from text, so the walk did NOT follow them): **69** distinct names.

**3 of them name a function that WOULD itself have been a hop** — the walk stopped exactly where a hop could hide, so this verdict is not complete on its own and needs a read:

- `new` — 1 of 468 definition(s) with that name contain a hop; 152 unfollowed call site(s). A name this heavily overloaded flags on collision, not on reachability
- `ensure_loaded` — 1 of 8 definition(s) with that name contain a hop; 1 unfollowed call site(s)
- `spawn` — 1 of 6 definition(s) with that name contain a hop; 1 unfollowed call site(s)

### template expression typing — NO-HOP

Entry points walked:

- `crates/verter_lsp/src/documents/mod.rs:1138` — `fn get_ide`

No provider hop is reachable. 24 functions were explored to exhaustion — the walk ran out of reachable callees, it was not cut short.

Unresolved receivers (method calls whose receiver type is not recoverable from text, so the walk did NOT follow them): **21** distinct names.

**1 of them name a function that WOULD itself have been a hop** — the walk stopped exactly where a hop could hide, so this verdict is not complete on its own and needs a read:

- `new` — 1 of 468 definition(s) with that name contain a hop; 8 unfollowed call site(s). A name this heavily overloaded flags on collision, not on reachability

### props — NO-HOP

Entry points walked:

- `crates/verter_lsp/src/features/component_diagnostics.rs:124` — `fn find_unknown_props`
- `crates/verter_lsp/src/features/component_actions.rs:224` — `fn suggest_matching_props`

No provider hop is reachable. 15 functions were explored to exhaustion — the walk ran out of reachable callees, it was not cut short.

Unresolved receivers (method calls whose receiver type is not recoverable from text, so the walk did NOT follow them): **26** distinct names.

**1 of them name a function that WOULD itself have been a hop** — the walk stopped exactly where a hop could hide, so this verdict is not complete on its own and needs a read:

- `new` — 1 of 468 definition(s) with that name contain a hop; 7 unfollowed call site(s). A name this heavily overloaded flags on collision, not on reachability

### events — NO-HOP

Entry points walked:

- `crates/verter_lsp/src/features/event_type_hints.rs:83` — `fn event_type_hint_actions`

No provider hop is reachable. 7 functions were explored to exhaustion — the walk ran out of reachable callees, it was not cut short.

Unresolved receivers (method calls whose receiver type is not recoverable from text, so the walk did NOT follow them): **10** distinct names.

**1 of them name a function that WOULD itself have been a hop** — the walk stopped exactly where a hop could hide, so this verdict is not complete on its own and needs a read:

- `new` — 1 of 468 definition(s) with that name contain a hop; 2 unfollowed call site(s). A name this heavily overloaded flags on collision, not on reachability

### slots and snippets — NO-HOP

Entry points walked:

- `crates/verter_lsp/src/server/component_resolve.rs:984` — `fn resolve_slot_name_definition`
- `crates/verter_lsp/src/server/component_resolve.rs:1029` — `fn resolve_slot_binding_definition`

No provider hop is reachable. 2 functions were explored to exhaustion — the walk ran out of reachable callees, it was not cut short.

Unresolved receivers (method calls whose receiver type is not recoverable from text, so the walk did NOT follow them): **7** distinct names.

**None of them names a function that would itself have been a hop.** Every indexed function whose own body contains a provider hop was collected up front; the unresolved set for this capability does not intersect it. That is what makes the verdict above load-bearing rather than an artefact of where the walk stopped.

### directives — NO-HOP

Entry points walked:

- `crates/verter_lsp/src/features/hover_directive_names.rs:106` — `fn builtin_directive_name_hover`

No provider hop is reachable. 3 functions were explored to exhaustion — the walk ran out of reachable callees, it was not cut short.

Unresolved receivers (method calls whose receiver type is not recoverable from text, so the walk did NOT follow them): **6** distinct names.

**None of them names a function that would itself have been a hop.** Every indexed function whose own body contains a provider hop was collected up front; the unresolved set for this capability does not intersect it. That is what makes the verdict above load-bearing rather than an artefact of where the walk stopped.

### framework macros — HOP

Entry points walked:

- `crates/verter_lsp/src/features/macro_actions.rs:97` — `fn macro_code_actions`

Shortest path from an entry point to the hop:

1. `crates/verter_lsp/src/features/macro_actions.rs:97` — `fn macro_code_actions`

Hop: **`type_provider`** (provider-handle) at `crates/verter_lsp/src/features/macro_actions.rs:132`

```rust
type_provider: None,
```

Unresolved receivers (method calls whose receiver type is not recoverable from text, so the walk did NOT follow them): **0** distinct names.

**None of them names a function that would itself have been a hop.** Every indexed function whose own body contains a provider hop was collected up front; the unresolved set for this capability does not intersect it. That is what makes the verdict above load-bearing rather than an artefact of where the walk stopped.

### auto-imports (Verter-owned half) — NO-HOP

Entry points walked:

- `crates/verter_lsp/src/server/nav_features_completion_resolve.rs:53` — `fn resolve_provider_auto_import_edits`
- `crates/verter_lsp/src/features/organize_imports.rs:17` — `fn organize_imports_actions`

No provider hop is reachable. 77 functions were explored to exhaustion — the walk ran out of reachable callees, it was not cut short.

Unresolved receivers (method calls whose receiver type is not recoverable from text, so the walk did NOT follow them): **56** distinct names.

**2 of them name a function that WOULD itself have been a hop** — the walk stopped exactly where a hop could hide, so this verdict is not complete on its own and needs a read:

- `new` — 1 of 468 definition(s) with that name contain a hop; 16 unfollowed call site(s). A name this heavily overloaded flags on collision, not on reachability
- `ensure_loaded` — 1 of 8 definition(s) with that name contain a hop; 1 unfollowed call site(s)

### background semantic analysis (Verter-owned lane) — HOP

Entry points walked:

- `crates/verter_lsp/src/documents/analysis.rs:143` — `fn schedule_semantic_analysis`

Shortest path from an entry point to the hop:

1. `crates/verter_lsp/src/documents/analysis.rs:143` — `fn schedule_semantic_analysis`
2. `crates/verter_lsp/src/documents/analysis.rs:147` — `fn spawn_semantic_analysis`
3. `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs:2753` — `fn get_public_api_projection`
4. `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs:2637` — `fn compose_component_contract`
5. `crates/verter_session/src/host_manage/component_meta_entry.rs:339` — `fn get_component_meta_output_via_view_with_fixed_store_view`
6. `crates/verter_session/src/host_manage/component_meta_entry.rs:839` — `fn try_component_meta_cache_entry_with_view`
7. `crates/verter_session/src/host_manage/prepared_decl.rs:1590` — `fn shallow_file_state`
8. `crates/verter_session/src/host_manage/prepared_decl.rs:1641` — `fn shallow_file_state_with_context`
9. `crates/verter_session/src/host_manage/analysis_io.rs:1197` — `fn artifact_current_indexed`
10. `crates/verter_session/src/host_manage/prepared_decl.rs:1829` — `fn ensure_indexed_ready_serve`
11. `crates/verter_session/src/host_manage/prepared_decl.rs:1899` — `fn ensure_indexed_ready_serve_uninstrumented`
12. `crates/verter_session/src/host_lifecycle.rs:954` — `fn ensure_loaded`

Hop: **`close_file`** (trait-method-call) at `crates/verter_session/src/host_lifecycle.rs:986`

```rust
self.scheduler.close_file(canonical_id);
```

Unresolved receivers (method calls whose receiver type is not recoverable from text, so the walk did NOT follow them): **209** distinct names.

**3 of them name a function that WOULD itself have been a hop** — the walk stopped exactly where a hop could hide, so this verdict is not complete on its own and needs a read:

- `new` — 1 of 468 definition(s) with that name contain a hop; 606 unfollowed call site(s). A name this heavily overloaded flags on collision, not on reachability
- `spawn` — 1 of 6 definition(s) with that name contain a hop; 4 unfollowed call site(s)
- `close` — 1 of 12 definition(s) with that name contain a hop; 1 unfollowed call site(s)

### provider-adjacent caches — NO-HOP

Entry points walked:

- `crates/verter_lsp/src/carrier_cache.rs:86` — `fn carrier_regeneration_is_fresh`
- `crates/verter_lsp/src/carrier_cache.rs:140` — `fn needs_engine_recheck`
- `crates/verter_lsp/src/carrier_cache.rs:153` — `fn mapped_results_valid`

No provider hop is reachable. 3 functions were explored to exhaustion — the walk ran out of reachable callees, it was not cut short.

Unresolved receivers (method calls whose receiver type is not recoverable from text, so the walk did NOT follow them): **0** distinct names.

**None of them names a function that would itself have been a hop.** Every indexed function whose own body contains a provider hop was collected up front; the unresolved set for this capability does not intersect it. That is what makes the verdict above load-bearing rather than an artefact of where the walk stopped.

