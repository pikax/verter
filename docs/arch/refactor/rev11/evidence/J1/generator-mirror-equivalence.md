# CSS generator mirror — inventory

The allocation canaries and the CSS bench-identity universe must generate
byte-identical CSS for every shared generator. This file inventories the two
copies. It is not a landing measurement; a later recapture proves the
equivalence against the tree that captures the baseline.

## Copies

- Copy A: `crates/verter_compiler/tests/allocator_canaries.rs`, generator
  section `style_planner_gen` (consumed by the legacy `css::process_style`
  canaries, the `style_planner` canaries, and the intra-parser attribution
  probe).
- Copy B: `crates/verter_bench/src/css_identities.rs`, generator section
  preceding `allocation_category_universe`.

The two copies exist because `verter_compiler` tests cannot depend on
`verter_bench`. Equivalence is a byte-identity contract on generator output,
not a source-spelling identity: Copy A uses inline `{i}` format args and Copy
B uses `format!` positional args. Matching function names are not the
decision.

## Shared generator set

| Generator | Copy A | Copy B |
|---|---|---|
| `generate_class_rules` | present | present |
| `generate_descendant_selectors` | present | present |
| `generate_pseudo_selectors` | present | present |
| `generate_selector_lists` | present | present |
| `generate_v_bind_rules` | present | present |
| `generate_v_bind_dotted` | present | present |
| `generate_deep_rules` | present | present |
| `generate_slotted_rules` | present | present |
| `generate_mixed_vue` | present | present |
| `generate_global_rules` | present | present |
| `generate_repeated_classes` | present | present |

There are no Copy-A-only or Copy-B-only generators in those sections.

## Inputs a recapture must cover

Every one-argument generator at `n = 1, 5, 8, 20, 40, 50, 100`. This includes
the required `1, 5, 20, 100`, the allocation canary's `N = 50`, and the
additional `8` and `40` inputs used by canaries that consume the shared
Vue-selector generators.

`generate_repeated_classes(unique, repeats)` over the 36-case cross-product
`{1, 5, 10, 20, 50, 100} × {1, 5, 10, 20, 50, 100}`. This includes the
allocation canary's exact `(5, 10)` input and exercises both independent size
arguments.

## Discrimination control

A recapture must also compare Copy A's `generate_class_rules(1)` against Copy
B's `generate_deep_rules(1)` and report `DIFFERING` at byte offset 0:

- A excerpt: `.class-0 { color: red; padding: 0px; }`
- B excerpt: `:deep(.inner-0) { color: red; }`

This control establishes that the comparison reports unequal generator output
rather than returning identical unconditionally.
