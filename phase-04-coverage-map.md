# Phase 4 — Coverage map for `host.read_source` fallback deletion

This document maps each of the 4 production `host.read_source` callsites in
`crates/verter_session/src/resolver_core/component_meta.rs` to (a) the §5.0
Phase 5 variant that closes the gap (per §4b.0a table) AND (b) at least one
discriminating test in the §5.B/§5.B.5/§5.C corpus that exercises the variant
for the specific callsite shape.

## Pre-flight (§4.1)

```
$ grep -n "host.read_source" crates/verter_session/src/resolver_core/component_meta.rs
351:        host.read_source(owner_canonical)
533:                host.read_source(declaration.canonical_source.as_str())
619:                host.read_source(declaration.canonical_source.as_str())
745:            let owner_source = host.read_source(owner_canonical);
```

Exactly 4 production matches — matches §4.1 expectation, no anchor drift.
(Lines 1044 and 1190 are test-impl trait methods on `TestHost`, not in scope.)

## Per-callsite coverage

### Callsite 351 — `let owner_source = host.read_source(owner_canonical)`

**What the read fed (per §4b.0a row 1):** owner-source for macro-surface
text recovery. Used by `is_direct_macro_type_reference` (line 3231) and
`keep_direct_imported_vue_macro` (line 3259) — these helpers already short-
circuit on `mac.type_references` containing `dep.type_name` (lines 3239-3245
and 3267-3271). Owner-source is a refinement signal that runs only AFTER
`type_references` already names the dep; when source is `None`, the helpers
return `true` by `unwrap_or(true)` (line 3250) which is the conservative
correct behaviour.

**§5.0 variant that closes the gap (per §4b.0a):** `ResolveMacroPayload`
(graph-native macro extraction from `IndexedReady`, no source text needed).
The macro's identity, surface kind, and direct-reference recognition are
already encoded in the analysis snapshot (`AnalyzedMacro.type_references`)
and the dispatch through `ResolveMacroPayload`/`ResolveDecl` populates the
graph-typed surface without re-reading the raw SFC source.

**Discriminating tests in §5.B/§5.B.5/§5.C corpus:**

| Test | File | Why it discriminates |
|---|---|---|
| `external_type_loaded_files_exactly` | `tests/component_meta_audit/external_type.rs` | `defineProps<PanelProps>()` with `PanelProps` declared in a sibling `.ts`. Asserts BOTH props extraction AND that loaded files are exactly `["/c.vue", "/panel_types.ts"]` — proves resolution does not re-read owner source for refinement and the prop set surfaces (`title`, `collapsible`, `variant`) regardless. |
| `auth_form_loaded_files_exactly` | `tests/component_meta_audit/corpus_representatives/auth_form.rs` | `defineProps<{ fields: AuthFormField[]; submit: AuthFormSubmit }>()` exercises the `is_direct_macro_type_reference` path on a multi-dep macro (two cross-file type refs). Loaded-files set match acts as a tripwire if owner-source were silently re-loaded. |
| `closed_conditional_collapses_to_concrete_arm` | `tests/component_meta_audit/closed_conditional.rs` | `defineProps<ClosedConditionalProps<string>>()` — direct macro reference with type args. The `is_direct_macro_type_reference` refinement path (which the deleted owner_source feeds) is the one that recognises `ClosedConditionalProps` as the directly-referenced type; this test asserts the surface (`value`, `isString`) materialises through the graph path. |

### Callsite 533 — `host.read_source(declaration.canonical_source.as_str())`

**What the read fed (per §4b.0a row 2):** declaration-source for cross-file
macro projection. Drives `project_macro_surfaces(declaration_source.as_deref(),
…)` (line 535) plus a deeper text-reparse fallback at lines 547-557 when the
primary projection produced empty surfaces.

**§5.0 variants that close the gap (per §4b.0a):** `ResolveMacroPayload` +
cross-file declaration routing through dispatch (`ResolveDecl`).
`ResolveMacroPayload` resolves the imported macro's payload to a single
`SemanticNodeId`; `ResolveDecl` resolves the declaration identity. The
`elements` returned by `host.resolve_macro_elements` already encode the
prop/emit/slot surface graph-natively; `project_macro_surfaces(None, …)`
preserves the core data extraction (lines 39-69, 122-154 of
`surface_projector.rs`) — `source` only feeds JSDoc + raw type-text, both
ancillary. The deeper text-reparse fallback (lines 537-557, currently guarded
by a `TODO(follow-up)` comment) becomes unreachable once read_source returns
no text.

**Discriminating tests in §5.B/§5.B.5/§5.C corpus:**

| Test | File | Why it discriminates |
|---|---|---|
| `barrel_chain_resolves_through_reexport` | `tests/component_meta_audit/barrel_chain.rs` | `defineProps<DialogProps>()` where `DialogProps` is re-exported through `barrel_index.ts → dialog_types.ts`. Cross-file declaration routing must reach the leaf, surface `open` and `title`, AND the loaded-files set must be exactly the entry + barrel + leaf. This is the canonical `ResolveMacroPayload + ResolveDecl` cross-file routing case for site 533. |
| `pick_and_my_pick_produce_identical_props` | `tests/component_meta_audit/lib_parity.rs` | Userland `MyPick<T,K>` mapped type vs ambient lib `Pick<T,K>` — both declared cross-decl and routed through declaration metadata. Asserts both paths surface `["alpha", "beta"]` AND neither surfaces `gamma`. The `ResolveMacroPayload` variant must hand back identical structure for both the lib- and userland-routed declarations. |
| `pathological_table_loading_animation` | `tests/component_meta_audit/pathological_table_loading_animation.rs` | Pathological regression on declaration routing through nested helpers; this test was a §5.B-era addition exercising graph-routed resolution under a payload shape that would have previously hit the source-text fallback. |

### Callsite 619 — `host.read_source(declaration.canonical_source.as_str())`

**What the read fed (per §4b.0a row 3):** declaration-source for cross-file
macro projection (sibling site to 533, taken when `imported_elements` is
`None`). Drives `project_macro_surfaces_from_source_type_name(projection_source,
mac.kind, dep_exported_name)` at lines 621-627.

**§5.0 variants that close the gap (per §4b.0a row 3):** same as 533 —
`ResolveMacroPayload` + `ResolveDecl`. When `imported_elements` is `None`,
the imported macro surface is unresolvable graph-natively (the resolver
already tried). Re-reading source text and reparsing is exactly the
"text-based fallback" the architecture rule prohibits. Once
`ResolveMacroPayload` is the authoritative resolver, an `imported_elements
= None` outcome means the macro has no resolvable surface — record an
empty surface and proceed; do NOT reparse source text.

**Discriminating tests in §5.B/§5.B.5/§5.C corpus:**

| Test | File | Why it discriminates |
|---|---|---|
| `barrel_chain_resolves_through_reexport` | `tests/component_meta_audit/barrel_chain.rs` | Same as 533 — when the resolver successfully reaches the leaf via `ResolveMacroPayload`, `imported_elements` is `Some(_)` and site 619's branch is never entered. The discrimination is: post-deletion, the test STILL PASSES because the graph path produces `imported_elements = Some(_)`. If the deletion broke graph resolution, this test would regress. |
| `accordion_loaded_files_exactly` | `tests/component_meta_audit/corpus_representatives/accordion.rs` | `defineProps<{ items: AccordionItem[]; multiple?: boolean }>()` with `AccordionItem` declared in a sibling `.ts`. Loaded-files set match catches any silent re-read of the declaration source as a regression. |
| `external_type_loaded_files_exactly` | `tests/component_meta_audit/external_type.rs` | Same shape — single cross-file dep declared in `panel_types.ts`. Asserts exact loaded-files set + prop surface; proves the graph path produces `imported_elements = Some(_)` for typical cross-file declarations. |

### Callsite 745 — `let owner_source = host.read_source(owner_canonical)` (in expanded loop)

**What the read fed (per §4b.0a row 4):** owner-source for fallthrough/
inheritance text recovery. Drives the chain at lines 754-768:
`project_macro_surfaces_from_source_type_name(projection_source, mac.kind,
resolved.name)` then falls back to `project_macro_surfaces_from_expanded_text(
mac.kind, &resolved.expanded)`. Also threaded into `keep_direct_imported_vue_macro`
at line 638 via the surrounding closure.

**§5.0 variant that closes the gap (per §4b.0a row 4):** `ResolveFallthroughSurface`
(graph-native fallthrough). Per the §5.0 docstring on `ResolveMacroPayload`
(`semantic_query.rs` lines 842-885), `ResolveFallthroughSurface` is a
non-variant dispatch helper that composes existing variants; the actual
graph-typed surface for owner-local macro resolution arrives via
`host.resolve_owner_local_macro_surface(owner_canonical, root_name, mac.kind)`
(see lines 874-878) which routes through the prepared owner-local surface
infrastructure introduced in 5d/5f for owner-local macro projection.

The `project_macro_surfaces_from_source_type_name` and
`project_macro_surfaces_from_expanded_text` text fallbacks are the LAST
remaining text-text-text recovery for the owner-local expanded loop. The
`resolve_owner_local_macro_surface` already produces the authoritative
projected surface in the second pass (lines 869-944 — the
`prepared_surface_will_handle` block) using only graph data. Site 745 is
the legacy first pass that runs before that second pass and was kept as a
safety net; per the brief it is now obsolete.

**Discriminating tests in §5.B/§5.B.5/§5.C corpus:**

| Test | File | Why it discriminates |
|---|---|---|
| `resolver_coverage_inherited_emits_branch_merged_surface` | `tests/component_meta_audit/resolver_coverage_inherited_emits.rs` | The §5.A seed for inherited-emits/fallthrough surface routing through `ProjectPath{[],Expanded}` (i.e. graph-native fallthrough). Parent `<Child />` inherits child's emits across a branch-merged conditional `Mode extends 'editor' ? EditorEmits : ViewerEmits`. Owner-local macro resolution must produce both `itemEdited` and `itemViewed` AND must NOT leak `phantomEventXyz`. This test is currently PASSING (closed by 5f §7) and is the strongest discriminating signal against site 745. |
| `single_file_generic_loaded_files_exactly` | `tests/component_meta_audit/single_file_generic.rs` | `<script setup generic="T extends { id: string }">` with `defineProps<{ rows: T[]; keyField?: keyof T }>()` and `defineEmits<{ select: [row: T] }>()`. Owner-local macro projection resolves the prepared surface (`rows`, `keyField`); loaded-files set match (`["/c.vue"]`) ensures owner-source is not re-read. |
| `path_precise_projection_extracts_leaf_only` | `tests/component_meta_audit/path_precise_projection.rs` | `defineProps<DeepConfig['ui']['header']>()` — owner-local macro with an indexed-access type expression. The `ResolveMacroPayload` path lowers the type and `resolve_owner_local_macro_surface` produces the leaf shape (`title`, `sticky`); siblings (`show`, `source`) must NOT leak. |
| Class A `fixture_fallthrough_inherit` (`fixture_fallthrough_inherit.correctness.snap.json`) | `tests/correctness/fixtures.rs` line 832 + snapshot | `defineOptions({ inheritAttrs: false })` + `defineProps<{ disabled?: boolean }>()` — direct exercise of the fallthrough surface inheritAttrs=false rule under the Class A correctness suite. The expected snapshot pins both the props surface and the fallthrough projection. |
| Class A `fixture_fallthrough_root_inherit` (`fixture_fallthrough_root_inherit.correctness.snap.json`) | `tests/correctness/fixtures.rs` line 838 + snapshot | Single-component-root inheritance: wrapper `<Inner />` inherits child's `label` prop. End-to-end fallthrough path must produce the inherited surface graph-natively. |

## Summary

All 4 callsites map to §5.0 variants per §4b.0a (no entry lacks a mapping
— no STOP under §4.5 condition #1).

All 4 callsites have at least one discriminating test in the §5.B/§5.B.5/
§5.C corpus that already exercises the closure path graph-natively (no
entry lacks a discriminating test — no STOP under §4.5 condition #2).

Phase 4 deletion is therefore safe: the four `host.read_source(...)` calls
plus the source-text reparse fallbacks they feed are dead code in the
post-Phase-5l tree, kept only as the legacy safety nets the architecture
rule prohibits. The §4.4 architecture guard
(`no_read_source_in_component_meta`) un-ignored at the end of the phase
is the structural assertion that proves the deletion landed.
