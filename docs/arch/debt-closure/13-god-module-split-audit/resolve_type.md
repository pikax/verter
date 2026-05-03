# resolve_type — Tier 0 Step 0.3 god-module split audit

**File:** `crates\verter_parser\src\utils\oxc\vue\script\resolve_type.rs`  
**LOC:** 5595  
**Function definitions:** 146  
**Intra-file call edges:** 320  
**Method:** automated extraction via `tmp/audit_extract.py` (regex-based function-and-call enumeration; Tarjan SCC). The plan's §2.1.0 "Default tool" is a `syn`-AST extension to the architecture-guards scanner; that extension is deferred — this document was produced by the lighter-weight extractor in the same time window. The Tier 2 worker assigned to this module should re-derive any sections that look noisy with the syn-AST tool when it lands.

## 1. Intra-file strongly-connected components

### Non-trivial SCCs (size ≥ 2)

**SCC 1 (size 2):** `infer_runtime_type`, `infer_type_reference`

Recursion via `interface extends` / `class extends` heritage walking. The SCC composes the type-alias / interface / class resolvers because each can transitively call into the others when chasing a heritage chain.

**SCC 2 (size 3):** `collect_formal_parameter_reference_names`, `collect_interface_reference_names`, `collect_type_reference_names`

Recursion via `interface extends` / `class extends` heritage walking. The SCC composes the type-alias / interface / class resolvers because each can transitively call into the others when chasing a heritage chain.

**SCC 3 (size 2):** `append_qualified_type_name_cache_key`, `append_type_name_cache_key`

Recursion via `interface extends` / `class extends` heritage walking. The SCC composes the type-alias / interface / class resolvers because each can transitively call into the others when chasing a heritage chain.

**SCC 4 (size 2):** `append_semantic_type_cache_key`, `semantic_type_cache_key_with_active`

Recursion via `interface extends` / `class extends` heritage walking. The SCC composes the type-alias / interface / class resolvers because each can transitively call into the others when chasing a heritage chain.

**SCC 5 (size 10):** `apply_named_type_heritage_edge_with_ctx_ref`, `flatten_named_type_plan_with_ctx_ref`, `resolve_mapped_type_keys_with_ctx`, `resolve_mapped_type_with_ctx`, `resolve_named_local_type_with_ctx_ref`, `resolve_named_local_type_with_ctx_ref_inner`, `resolve_type_elements_inner_with_ctx_ref`, `resolve_type_elements_inner_with_ctx_ref_guarded`, `resolve_type_elements_with_ctx_ref`, `try_resolve_heritage_utility_type`

Recursion via `interface extends` / `class extends` heritage walking. The SCC composes the type-alias / interface / class resolvers because each can transitively call into the others when chasing a heritage chain.

**SCC 6 (size 3):** `collect_structural_formal_parameter_reference_names`, `collect_structural_interface_reference_names`, `collect_structural_type_reference_names`

Recursion via `interface extends` / `class extends` heritage walking. The SCC composes the type-alias / interface / class resolvers because each can transitively call into the others when chasing a heritage chain.

**SCC 7 (size 3):** `resolve_interface_with_extends_ctx`, `resolve_type_elements_inner_with_ctx`, `resolve_type_elements_inner_with_ctx_guarded`

Recursion via `interface extends` / `class extends` heritage walking. The SCC composes the type-alias / interface / class resolvers because each can transitively call into the others when chasing a heritage chain.

### Self-recursive functions (size 1)

- `from`
- `new`
- `find_call_type_param_in_expr`
- `extract_string_literal_keys_inner`
- `get_type_reference_name`
- `resolve_root_runtime_type_with_ctx_ref`
- `resolve_type_elements_inner`
- `resolve_root_runtime_type_with_ctx`

(Single-function SCCs report self-recursion or method-name collisions where a same-named library method is invoked on a borrowed receiver. The Tier 2 split must check each one against the syn-AST tool when it lands.)

## 2. Recursion-budget edges

No recursion-budget edges detected in this file.

Recursion in this file (if any) does not consult an explicit pinned budget constant or named depth counter. Cross-module budgets (e.g. `assertions::WALKER_DEPTH_CAP`) may still bound callers from outside.

## 3. Cache-identity edges

No `*Db` cache reads or writes detected in this file.

## 4. Public-surface edges

`pub fn` count: 51.

- `pub fn find_macro_type_param` — line 67 (span 67-77)
- `pub fn as_str` — line 168 (span 168-181)
- `pub fn format_runtime_types` — line 187 (span 187-205)
- `pub fn is_public` — line 224 (span 224-226)
- `pub fn name` — line 392 (span 392-406)
- `pub fn message` — line 423 (span 423-429)
- `pub fn take_last_resolution_budget_exceeded` — line 560 (span 560-562)
- `pub fn new` — line 739 (span 739-759)
- `pub fn set_named_type_cache` — line 765 (span 765-770)
- `pub fn refresh_companion_cache_key` — line 772 (span 772-780)
- `pub fn refresh_type_param_bindings_cache_key` — line 782 (span 782-792)
- `pub fn clear_type_param_bindings` — line 794 (span 794-797)
- `pub fn set_trace_label` — line 799 (span 799-801)
- `pub fn extend_companion_types` — line 803 (span 803-813)
- `pub fn is_type_blocked` — line 822 (span 822-859)
- `pub fn find_type_alias` — line 862 (span 862-872)
- `pub fn find_interface` — line 877 (span 877-894)
- `pub fn find_class` — line 897 (span 897-899)
- `pub fn find_type_param` — line 962 (span 962-975)
- `pub fn build_type_context` — line 1309 (span 1309-1413)
- `pub fn extract_companion_types` — line 1462 (span 1462-1570)
- `pub fn resolve_type_elements` — line 1580 (span 1580-1585)
- `pub fn resolve_type_elements_with_ctx` — line 1594 (span 1594-1604)
- `pub fn resolve_type_elements_with_ctx_ref` — line 1615 (span 1615-1625)
- `pub fn infer_runtime_type` — line 3655 (span 3655-3776)
- `pub fn resolve_external_type` — line 3992 (span 3992-3998)
- `pub fn required_import_names` — line 4057 (span 4057-4084)
- `pub fn direct_reexport_target` — line 4086 (span 4086-4090)
- `pub fn local_import_symbol_target` — line 4092 (span 4092-4096)
- `pub fn local_export_symbol_target` — line 4098 (span 4098-4102)
- `pub fn exported_local_type_names` — line 4104 (span 4104-4106)
- `pub fn exported_local_symbol_names` — line 4108 (span 4108-4110)
- `pub fn direct_reexport_entries` — line 4112 (span 4112-4122)
- `pub fn wildcard_reexport_sources` — line 4124 (span 4124-4126)
- `pub fn local_symbol_span` — line 4128 (span 4128-4132)
- `pub fn local_type_symbol` — line 4134 (span 4134-4136)
- `pub fn local_symbol_target_name` — line 4138 (span 4138-4153)
- `pub fn has_local_symbol_target` — line 4155 (span 4155-4158)
- `pub fn local_symbol_dependency_names` — line 4160 (span 4160-4181)
- `pub fn stats` — line 4183 (span 4183-4193)
- `pub fn analyze_external_type_source` — line 4197 (span 4197-4209)
- `pub fn extract_imported_type_bindings` — line 4211 (span 4211-4216)
- `pub fn required_import_alias_names_for_binding` — line 4218 (span 4218-4236)
- `pub fn imported_member_name_for_required_alias` — line 4238 (span 4238-4251)
- `pub fn extract_export_surface` — line 4272 (span 4272-4361)
- `pub fn collect_required_import_names_for_external_type` — line 4363 (span 4363-4369)
- `pub fn analyze_external_type_program` — line 4372 (span 4372-4638)
- `pub fn resolve_external_type_with_companion` — line 5298 (span 5298-5321)
- `pub fn resolve_external_type_in_program_with_analyzed_symbol_companion` — line 5324 (span 5324-5340)
- `pub fn resolve_external_type_in_context_with_analyzed_symbol_companion` — line 5343 (span 5343-5364)
- `pub fn hash_resolved_type` — line 5547 (span 5547-5591)

## 5. Cross-file shared-cache edges

No references to `ProjectTypeStore`, `SemanticGraphStore`, or any host-owned `*Db` from this file.

## 6. Tier 2 split sketch

**Tier 2 W5b candidate split** — 4 sub-modules. This is a SUGGESTION; the W5* worker assigned to this module is free to deviate.

### `context.rs`

`TypeResolutionContext` borrow shape — lifetimes, source slice, type-alias / interface / class / type-param tables. Dies along with `OwnedTypeResolutionContext` in Tier 1A; the rump that survives to Tier 2 should be limited to lookup helpers.

### `resolve_decl.rs`

`resolve_type_alias` / `resolve_interface` / `resolve_class` / `resolve_enum` plus their heritage-clause walkers. The `interface extends` recursion lives here.

### `resolve_expr.rs`

Expression-typed resolvers (`resolve_indexed_access`, `resolve_keyof`, `resolve_mapped_type`, `resolve_typeof`, `resolve_template_literal_type`, `resolve_conditional`).

### `resolve_macro.rs`

`defineProps` / `defineEmits` / `defineSlots` / `defineModel` macro shims, withDefaults handling, and the cross-file resolver entry point that orchestrates the above kernels.
