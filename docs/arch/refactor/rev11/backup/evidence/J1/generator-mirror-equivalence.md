# CSS generator mirror equivalence

**PASS — all shared generators produced byte-identical output at every input tested.**

## Scope and source identity

- Copy A: `crates/verter_compiler/tests/allocator_canaries.rs`, generator
  section `style_planner_gen` (consumed by the legacy `css::process_style`
  canaries, the `style_planner` canaries, and the intra-parser attribution
  probe).
- Copy B: `crates/verter_bench/src/css_identities.rs`, generator section
  preceding `allocation_category_universe`.
- The two SHA-256 digests below identify the exact compared bytes of each
  **file** at recapture. Equivalence is a byte-identity contract on generator
  **output**, not a source-spelling identity: Copy A uses inline `{i}` format
  args and Copy B uses `format!` positional args.
- Copy A file SHA-256: `1193d898ce7a3811730071345e737f6ad25dbb5ce481d8e3a98000bb1912a96e`.
- Copy B file SHA-256: `8955f3754867da5d478c279d43b32301814fc82f68039a2133f5005b52fa5990`.
- Pinned output-digest table:
  `docs/arch/refactor/rev11/evidence/J1/generator-mirror-digests.json`
  (106 cases: 10 one-argument generators × 7 sizes + 36 `repeated_classes` pairs).

The two copies exist because `verter_compiler` tests cannot depend on
`verter_bench`. Each copy is hashed independently against the same pinned
table:

- Copy A: `allocator_canaries.rs::generator_mirror::allocator_canary_generators_match_pinned_mirror_digests`
- Copy B: `css_gate.rs::css_identities_generators_match_pinned_mirror_digests`

The throwaway comparison that produced the pin invoked each generator,
hashed the returned `String` through `as_bytes()`, and recorded the digest.
Source spelling and matching function names were not the decision.

## Shared generator set

| Generator | Copy A | Copy B | Inputs | Byte result |
|---|---|---|---|---|
| `generate_class_rules` | present | present | `n = 1, 5, 8, 20, 40, 50, 100` | identical at all 7 sizes |
| `generate_descendant_selectors` | present | present | `n = 1, 5, 8, 20, 40, 50, 100` | identical at all 7 sizes |
| `generate_pseudo_selectors` | present | present | `n = 1, 5, 8, 20, 40, 50, 100` | identical at all 7 sizes |
| `generate_selector_lists` | present | present | `n = 1, 5, 8, 20, 40, 50, 100` | identical at all 7 sizes |
| `generate_v_bind_rules` | present | present | `n = 1, 5, 8, 20, 40, 50, 100` | identical at all 7 sizes |
| `generate_v_bind_dotted` | present | present | `n = 1, 5, 8, 20, 40, 50, 100` | identical at all 7 sizes |
| `generate_deep_rules` | present | present | `n = 1, 5, 8, 20, 40, 50, 100` | identical at all 7 sizes |
| `generate_slotted_rules` | present | present | `n = 1, 5, 8, 20, 40, 50, 100` | identical at all 7 sizes |
| `generate_mixed_vue` | present | present | `n = 1, 5, 8, 20, 40, 50, 100` | identical at all 7 sizes |
| `generate_global_rules` | present | present | `n = 1, 5, 8, 20, 40, 50, 100` | identical at all 7 sizes |
| `generate_repeated_classes` | present | present | `{1, 5, 10, 20, 50, 100} × {1, 5, 10, 20, 50, 100}` | identical in all 36 cases |

There are no Copy-A-only or Copy-B-only generators in those sections.

## Discrimination control

The same byte comparator compared Copy A's `generate_class_rules(1)` against
Copy B's `generate_deep_rules(1)` and reported `DIFFERING` at byte offset 0:

- A excerpt: `.class-0 { color: red; padding: 0px; }`
- B excerpt: `:deep(.inner-0) { color: red; }`

Tests: `generator_mirror_control_class_rules_differs_from_deep_rules` in both
`allocator_canaries.rs` and `css_gate.rs`.

## Commands

```sh
cargo test -p verter_bench --lib css_identities_generators_match_pinned_mirror_digests
cargo test -p verter_compiler --test allocator_canaries allocator_canary_generators_match_pinned_mirror_digests
```

Both passed on this recapture.
