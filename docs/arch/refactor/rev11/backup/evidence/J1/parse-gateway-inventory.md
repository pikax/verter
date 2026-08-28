# `verter_css_syntax` public parse-gateway inventory

Derived from this tree's crate export list (`crates/verter_css_syntax/src/lib.rs`),
not a hand-written name list. The sealed surface is asserted by
`crates/verter_css_syntax/tests/cases/parse_gateway_closure.rs::public_parse_surface_is_exactly_the_gateway`.

## Derivation

```sh
# Crate-root re-exports and module edges
sed -n '1,90p' crates/verter_css_syntax/src/lib.rs

# Explicitly public callables in crate-owned sources
rg -n '^[[:space:]]*pub([[:space:]]|\([^)]*\))[[:space:]]*(unsafe[[:space:]]+|const[[:space:]]+|async[[:space:]]+|extern[^[:space:]]*[[:space:]]+)*fn[[:space:]]+' \
  crates/verter_css_syntax/src/*.rs \
  crates/verter_css_syntax/src/dialect/*.rs
```

A parse route is anything that **starts a parse**, regardless of its name.
Observability accessors (`parse_style_ir_thread_invocations`,
`css_source_token_reconstructions`, `parse_inline_style_declarations_thread_invocations`,
`parse_selector_structure_thread_invocations`) only read counters and are not
routes. `Lexer` lexes tokens and does not drive the parser.

## Sealed public surface

| route | disposition |
|---|---|
| `parse_with_sink` | GATEWAY |
| `parse_lossless` | CRATE-PRIVATE |
| `Parser` / `Parser::new` / `Parser::parse` | CRATE-PRIVATE |
| `style_body_reject_code` | CRATE-PRIVATE |
| `parse_style_body` | RECORDED-JUSTIFICATION — single Svelte carrier caller owns span/error mapping |
| `parse_inline_style_declarations` | RECORDED-JUSTIFICATION — inline-local declaration coordinates |
| `parse_selector_structure` | RECORDED-JUSTIFICATION — typed `SelectorStructure` projection; production nested-pseudo fallback is deleted |
| `parse_style_ir` | RECORDED-JUSTIFICATION — whole-stylesheet projection owner (`StyleSyntaxIrSink` is crate-private) |
| `parse_component_value_tree` | RECORDED-JUSTIFICATION — single component-value consumer |

One gateway, three crate-private routes, five recorded public facades.
Modules stay public; closure is item-level.

## Expiry records for the retained routes

| route | named expiry fact |
|---|---|
| `parse_style_body` | **Single Svelte carrier caller.** Outside `verter_css_syntax`, its production caller set is exactly `crates/verter_compiler/src/svelte/runtime/css/mod.rs`. Expires if a second production caller appears, the named caller disappears, or span/error ownership moves elsewhere. |
| `parse_inline_style_declarations` | **Inline-local declaration coordinates.** The compiler inline-style emitter and semantic template analyzer consume declaration name/value spans local to an unwrapped attribute value. Expires when one shared non-parsing projection from gateway output provides that contract. |
| `parse_selector_structure` | **Typed selector-structure projection.** Syntax tests and any remaining production caller consume `SelectorStructure` while the selector sink is crate-private. Expires when that sink is a public non-parsing projection. |
| `parse_style_ir` | **Whole-stylesheet projection owner.** Production callers consume `StyleSyntaxIr` while `StyleSyntaxIrSink` cannot be publicly finished from gateway events. |
| `parse_component_value_tree` | **Single component-value consumer.** Production caller set is `verter_semantic/src/analysis/style_syntax.rs` standalone CSS-variable-value analysis. |

## Open follow-on: CSS Projection Separation

Split projection from parsing: make route-specific sinks finish non-parsing
projections from the gateway event stream, then close each facade whose expiry
fact has become false.
