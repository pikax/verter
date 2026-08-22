# Legacy `css/` tree test inventory (delete candidates)

Per charter §6 ("Tests whose only purpose is to preserve normalized
Lightning CSS output" — row 3): an exhaustive inventory of every inline
`#[cfg(test)]` module in `crates/verter_compiler/src/css/` (the legacy
lightningcss-backed pipeline, §1.1 row 3, disposition **Delete**), plus
`style_pipeline_tests.rs` (included from `mod.rs` via `#[path]`, same
disposition).

**Not deleted by this change.** This document is an implementation
pre-step only, per charter §6: it names the delete candidates for the
LATER, gated train that removes `crates/verter_compiler/src/css/` and the
`lightningcss` dependency (blocked on `perf-baseline.md` landing first, per
§2 Bounds "Latency"). No test file listed here is modified or removed by
this change.

## Why every test below qualifies

`crates/verter_compiler/src/css/` is, in its entirety, the legacy
lightningcss-backed CSS pipeline (§1.1 row 3): every function under test in
every file below is either a direct lightningcss `StyleSheet::parse(...).to_css(...)`
caller or a component the legacy pipeline composes (`prepass`, `scoped`,
`modules`, `walk`). There is no CSS parsing surface in this tree that is
NOT part of the legacy pipeline row 3 disposes of — so every inline test
here pins some observable slice of that pipeline's (lightningcss-normalized,
or lightningcss-pipeline-composed) output and is a delete candidate once the
replacement (`StyleSyntaxIr`-backed `style_planner`, §1.1 row 2) reaches
parity, per A2/A4.

## Inventory

Total: **169** inline `#[test]` functions across 6 files.

### `crates/verter_compiler/src/css/mod.rs` — 21 tests

Module: `#[cfg(test)] mod tests` at `mod.rs:275-276`. Exercises the
top-level `process_style`/`extract_css_class_names` entry points end to end
against the full lightningcss-backed pipeline.

```
test_process_style_scoped_basic
test_process_style_scoped_with_v_bind
test_process_style_deep
test_process_style_slotted
test_process_style_global
test_process_style_modules
test_process_style_no_transform
test_process_style_scoped_and_modules
test_process_style_pseudo_class_ordering
test_process_style_pseudo_class_and_pseudo_element
test_process_style_pseudo_element_ordering
test_process_style_grid_layout_normalization
test_process_style_media_query_selectors_scoped
test_process_style_supports_query_selectors_scoped
test_process_style_media_complex_selectors_scoped
extract_basic_classes
extract_classes_with_media_query
extract_deduplicates
extract_kebab_case_classes
extract_empty_css
extract_no_classes
```

Also hosts, via `#[path]` at `mod.rs:25-26`:

### `crates/verter_compiler/src/css/style_pipeline_tests.rs` — 16 tests

Named explicitly in charter §6/§3 as a delete candidate alongside the rest
of the tree. Exercises the pipeline's owner-path normalization,
zero-copy/marker-detection facts, and parity of the legacy pipeline's own
scoped/`:deep`/`:slotted`/`:global`/v-bind/CSS-Modules output shapes.

```
zero_marker_style_borrows_input_zero_copy
prepass_borrows_marker_free_css_and_owns_on_transform
returned_facts_describe_each_marker_kind
prepass_reports_deep_and_slotted_facts
owner_path_normalizes_once_for_transforms_and_never_on_passthrough
exactly_one_public_style_processor
parity_scoped_basic
parity_scoped_compound_and_descendant
parity_deep
parity_slotted
parity_global
parity_nested_selectors
parity_v_bind_scoped
parity_v_bind_without_scope_is_unnormalized
css_module_mapping_and_emitted_css_unchanged
parity_scoped_and_module_combined
```

Two of these (`css/mod.rs`'s inline suite, not `style_pipeline_tests.rs`)
are additionally named in charter §3.1 as non-discriminating: a suite
asserting only the vue-benchmarks `:is()`-argument-scoping defect's
*absence-marker's presence*, never the *leak's absence*
(`scoped.rs:686`'s `.item:is(.a, .b)` case). That defect lives in
`scoped.rs` below, not `mod.rs`/`style_pipeline_tests.rs`; flagged here for
completeness — the replacement suite for A10c/A10f must not repeat the
non-discriminating pattern regardless of which file it lands in.

### `crates/verter_compiler/src/css/scoped.rs` — 51 tests

Module: `#[cfg(test)] mod tests` at `scoped.rs:260-261`. Exercises
`apply_scoped` (selector scoping) directly — combinators, pseudo-classes,
pseudo-elements, `:deep`/`:slotted`/`:global`, media/supports/keyframes
interaction, compound selectors, attribute selectors, comments/strings in
declaration values.

```
test_basic_class
test_element_selector
test_id_selector
test_multiple_selectors
test_descendant_selector
test_pseudo_class_ordering
test_pseudo_element_ordering
test_deep_marker
test_deep_with_parent
test_slotted_marker
test_global_no_scope
test_media_query_inner_selectors_scoped
test_media_query_multiple_inner_selectors
test_supports_query_inner_selectors_scoped
test_multiple_media_blocks
test_media_with_descendant_selector
test_compound_class_selector
test_multiple_compound_selectors
test_id_class_compound
test_element_class_compound
test_attribute_selector_standalone
test_attribute_selector_with_pseudo
test_child_combinator
test_child_combinator_preserves_structure
test_sibling_combinator_preserves_structure
test_general_sibling_combinator
test_pseudo_class_and_pseudo_element
test_nth_child_and_pseudo_element
test_three_level_descendant
test_universal_selector
test_element_pseudo_element
test_not_pseudo_class
test_is_pseudo_class
test_many_rules_in_sequence
test_keyframes_not_scoped
test_selector_after_keyframes
test_media_and_keyframes_mixed
test_string_with_braces_in_value
test_comment_between_selectors
test_grid_template_areas_strings
test_nested_at_rules
test_layer_at_rule
test_comma_separated_in_descendant
test_escaped_colon_in_class
test_empty_rule
test_after_charset_still_scoped
test_font_face_not_scoped
test_deep_with_comma_e2e
test_deep_with_comma_and_parent_e2e
test_deep_empty_parens_e2e
test_deep_bare_no_parens_e2e
```

`test_is_pseudo_class` is the non-discriminating case named in charter §3.1:
it asserts the `.item` scope prefix is present on `.item:is(.a, .b)` output
but never asserts the `:is()` ARGUMENT LIST stays unscoped — the real
vue-benchmarks defect (§3.1 item 3) leaks scoping into the argument list
today and this test does not catch it. Recorded here per §3.1; not fixed by
this change (row 3 is deleted wholesale, not patched).

### `crates/verter_compiler/src/css/modules.rs` — 16 tests

Module: `#[cfg(test)] mod tests` at `modules.rs:122-123`. Exercises
`apply_css_modules` (CSS-Modules class hashing) — basic/chained/repeated
classes, element/id exclusion, media/supports/nested-at-rule interaction,
keyframes exclusion.

```
test_basic_class_hashing
test_content_hash_is_deterministic
test_different_component_id_different_hash
test_multiple_classes
test_same_class_reused
test_chained_classes
test_element_not_hashed
test_id_not_hashed
test_selector_list
test_modules_inside_media
test_modules_multiple_inside_media
test_modules_mixed_media
test_modules_inside_supports
test_modules_nested_at_rules
test_modules_same_class_in_media_and_top
test_modules_keyframes_not_hashed
```

### `crates/verter_compiler/src/css/prepass.rs` — 28 tests

Module: `#[cfg(test)] mod tests` at `prepass.rs:362-363`. Exercises the
`prepass` marker pre-pass (`v-bind()`, `:deep`/`v-deep`, `:slotted`/`v-slotted`,
`:global` detection) — quoting, nesting, comment/string exclusion, legacy
`v-`-prefixed spellings, non-ASCII content.

```
test_v_bind_simple
test_v_bind_quoted
test_v_bind_nested_parens
test_deep_selector
test_deep_with_prefix
test_v_deep_legacy
test_slotted_selector
test_v_slotted_legacy
test_global_passthrough
test_v_bind_in_string_not_transformed
test_v_bind_in_comment_not_transformed
test_multiple_v_binds
test_mixed_transforms
test_v_bind_single_quote_char
test_v_bind_empty_parens
test_v_bind_unclosed
test_non_ascii_in_comment
test_non_ascii_in_content
test_v_bind_optional_chaining
test_v_bind_array_access
test_v_bind_arithmetic
test_v_bind_function_call
test_v_bind_dollar_sign
test_v_bind_hyphen_preserved
test_deep_without_parens
test_deep_with_comma_separated_selectors
test_deep_with_comma_and_nested_parens
test_deep_empty_parens_passthrough
```

### `crates/verter_compiler/src/css/walk.rs` — 37 tests

Module: `#[cfg(test)] mod tests` at `walk.rs:143-144`. Exercises the
hand-rolled declaration-list/selector walker `walk.rs` composes for the
legacy pipeline's selector rewriting — comment/string preservation, CSS
nesting, keyframes exclusion, at-rule selector collection, attribute/pseudo
selectors.

```
test_single_class_selector
test_multiple_rules
test_comma_separated_selectors
test_descendant_selector
test_at_rule_prefix_not_collected
test_comment_preserved_in_output
test_string_with_braces
test_double_quoted_string
test_escaped_quote_in_string
test_transform_adds_suffix
test_transform_multiple_rules
test_leading_whitespace_preserved
test_newline_between_rules
test_empty_input
test_only_comment
test_nested_at_rule_with_selector
keyframe_selectors_not_transformed
keyframe_percentage_selectors_not_transformed
normal_selectors_after_keyframes_still_transformed
css_nesting_nested_selectors_found
css_nesting_multiple_nested_selectors
css_nesting_pseudo_class
css_nesting_modifier
css_nesting_deep
css_nesting_inside_media
css_nesting_declarations_preserved
webkit_keyframes_selectors_not_transformed
test_attribute_selector
test_pseudo_class_in_selector
media_inner_selectors_collected
supports_inner_selectors_collected
layer_inner_selectors_collected
multiple_media_blocks_selectors
nested_at_rules_inner_selectors
font_face_no_selectors
media_inner_selectors_transformed
after_charset_removal_selector_works
```

## Summary by file

| File | Test count |
|---|---:|
| `mod.rs` | 21 |
| `style_pipeline_tests.rs` | 16 |
| `scoped.rs` | 51 |
| `modules.rs` | 16 |
| `prepass.rs` | 28 |
| `walk.rs` | 37 |
| **Total** | **169** |

Also delete-candidate, not an inline `#[cfg(test)]` module but named
alongside this tree in charter §3/§6: `crates/verter_bench/benches/css_bench.rs`
(the lightningcss-pipeline benchmark this same change's `perf-baseline.md`
captures a pre-cutover snapshot from) — to be rewritten or deleted once the
replacement pipeline lands, per §6.
