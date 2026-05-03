# component_meta — Tier 0 Step 0.3 god-module split audit

**File:** `crates\verter_session\src\resolver_core\component_meta.rs`  
**LOC:** 3948  
**Function definitions:** 88  
**Intra-file call edges:** 83  
**Method:** automated extraction via `tmp/audit_extract.py` (regex-based function-and-call enumeration; Tarjan SCC). The plan's §2.1.0 "Default tool" is a `syn`-AST extension to the architecture-guards scanner; that extension is deferred — this document was produced by the lighter-weight extractor in the same time window. The Tier 2 worker assigned to this module should re-derive any sections that look noisy with the syn-AST tool when it lands.

## 1. Intra-file strongly-connected components

### Non-trivial SCCs (size ≥ 2)

**SCC 1 (size 2):** `resolve_imported_macro_surface`, `resolve_macro_elements`

Recursion in component-meta lowering — a type-args projection arm calls back into the resolver entry to lower nested generic substitutions. Tier 2 should keep the recursion entry+helper together in `lower_props_emits.rs`.

**SCC 2 (size 2):** `render_function_type_for_projected_surface`, `render_type_expr_for_projected_surface`

Recursion in component-meta lowering — a type-args projection arm calls back into the resolver entry to lower nested generic substitutions. Tier 2 should keep the recursion entry+helper together in `lower_props_emits.rs`.

### Self-recursive functions (size 1)

- `resolve_type_declaration`
- `type_expr_has_direct_macro_reference`

(Single-function SCCs report self-recursion or method-name collisions where a same-named library method is invoked on a borrowed receiver. The Tier 2 split must check each one against the syn-AST tool when it lands.)

## 2. Recursion-budget edges

No recursion-budget edges detected in this file.

Recursion in this file (if any) does not consult an explicit pinned budget constant or named depth counter. Cross-module budgets (e.g. `assertions::WALKER_DEPTH_CAP`) may still bound callers from outside.

## 3. Cache-identity edges

No `*Db` cache reads or writes detected in this file.

## 4. Public-surface edges

`pub fn` count: 9.

- `pub fn collect_requested_binding_names` — line 22 (span 22-27)
- `pub fn component_meta_resolved_macros` — line 108 (span 108-128)
- `pub fn component_meta_type_registry` — line 130 (span 130-143)
- `pub fn resolve_component_meta_parts` — line 470 (span 470-1066)
- `pub fn resolved_elements_to_type_expr_via_type_text` — line 3167 (span 3167-3174)
- `pub fn projected_macro_surfaces_to_type_expr` — line 3176 (span 3176-3250)
- `pub(crate) fn project_macro_surfaces_from_expanded_shape` — line 3252 (span 3252-3294)
- `pub(crate) fn imported_declaration_surface_is_authoritative` — line 3850 (span 3850-3878)
- `pub(crate) fn imported_registry_seed_can_skip_refresh` — line 3880 (span 3880-3890)

## 5. Cross-file shared-cache edges

No references to `ProjectTypeStore`, `SemanticGraphStore`, or any host-owned `*Db` from this file.

## 6. Tier 2 split sketch

**Tier 2 W5d candidate split** — 4 sub-modules. This is a SUGGESTION; the W5* worker assigned to this module is free to deviate.

### `resolver_entry.rs`

The cold-resolver public entry (`resolve_component_meta_*`), warm-cache check, fence revalidation, and the dep-signature publish path.

### `lower_props_emits.rs`

Type-args lowering for `defineProps` / `defineEmits` / `defineSlots`, plus the withDefaults projector. The two intra-file SCCs in the file live here.

### `structure_materialize.rs`

`materialize_component_meta_structure` and the publish path into `MaterializeStructureDb`. Cooperative-admission post_publish goes here.

### `trace_diagnostics.rs`

Macro-expansion diagnostic builders, audit-trace plumbing, and the structured-event emit helpers used by component-meta cold paths.
