---
name: compiler-codegen
description: "Rust compiler pipeline, template codegen (VDOM/IDE), CodeTransform, cached directives, strict slots, IDE error recovery, style preprocessing, CompileTarget"
---

# Compiler & Codegen

## Rust Compiler Architecture

The Rust compiler uses an AST-based pipeline. The `compile()` orchestrator drives a linear 5-phase pipeline:

```
Vue SFC Source
    |
[Tokenizer]  byte-level SFC tokenization (tokenizer/byte.rs)
    |
[Parser]     builds arena-based template AST + extracts script/style blocks (parser/)
    |
[Style]      v-bind() scan + CSS processing (style/ + css/)
    |
[Script]     macro expansion, binding extraction, component wrapper (script/)
    |
[Template]   render function codegen -- VDOM or Vapor backends (template/)
    |
[Compile]    orchestrates the above, applies CodeTransform, emits output (compile.rs)
```

**Module overview:**

```
compile.rs                # Pipeline orchestrator, options, result types
tokenizer/
+-- byte.rs               # Zero-copy byte-level SFC tokenizer (production)
+-- helpers.rs            # Tokenizer utility functions
+-- types.rs              # Event, QuoteType
parser/
+-- mod.rs                # Syntax state machine (tokenizer events -> AST)
+-- types.rs              # RootNodeScript, RootNodeStyle, RootNodeTemplate
ast/
+-- mod.rs                # TemplateAst (flat arena with O(1) navigation)
+-- builder.rs            # TemplateAstBuilder (incremental AST construction)
+-- types.rs              # AstNode, ElementNode, NodeId, pre-computed flags
script/
+-- mod.rs                # generate_script() entry point
+-- process.rs            # Script setup processing, companion script merging
+-- macros.rs             # defineProps/Emits/Model/Slots/Expose/Options
+-- css_vars.rs           # _useCssVars() injection for v-bind() in styles
template/
+-- oxc/                  # OXC expression parsing for template bindings
|   +-- mod.rs            # parse_template_expressions()
|   +-- types.rs          # OxcParsedAst, OxcParsedElement, OxcParsedExpression
+-- code_gen/             # Render function codegen
    +-- mod.rs            # generate_template() entry point
    +-- walker.rs         # DFS tree walker (shared by all backends)
    +-- types.rs          # TemplateCodeGen trait, CodeGenOutput
    +-- binding.rs        # BindingResolver (_ctx./$setup. prefix resolution)
    +-- shared/           # Shared codegen helpers
    +-- vdom/             # VDOM render function output (_createElementVNode, etc.)
    +-- vapor/            # Vapor mode output (_template, _renderEffect, etc.)
ide/                      # IDE codegen: TSX or JSX+JSDoc (for LSP/TSGO type checking)
+-- mod.rs                # generate_ide_template() -- Vue template -> valid JSX; IdeScriptOptions, IdeTemplateOptions
+-- script.rs             # generate_ide_script() -- script block -> TS or JS+JSDoc wrapper
+-- script_recover.rs     # Token scanner for macro binding recovery from broken script tails
+-- condition.rs          # v-if/v-else-if/v-else condition chain codegen
+-- template/
    +-- mod.rs            # walk_element/walk_node, cached directive removal, ref conversion
    +-- directives.rs     # v-if -> ternary, v-for -> .map(), v-show -> style
    +-- props.rs          # :prop -> prop={}, @event -> onEvent={}, v-bind spread
style/
+-- mod.rs                # generate_style() entry point
+-- v_bind.rs             # v-bind() scanning in CSS
css/
+-- mod.rs                # process_style() -- CSS pipeline entry point
+-- prepass.rs            # Vue syntax -> valid CSS markers (v-bind, :deep, :slotted)
+-- scoped.rs             # Scoped CSS: insert [data-v-xxx] selectors
+-- modules.rs            # CSS Modules: hash class names
+-- walk.rs               # String-level CSS selector walking
+-- types.rs              # ProcessStyleOptions, ProcessStyleResult
code_transform/
+-- code_transform.rs     # Chunk-based deferred mutation engine (MagicString equivalent)
+-- chunk.rs              # Chunk types (Original, Overwritten, Inserted, InsertedMapped)
+-- source_map.rs         # Source map generation from chunk positions
utils/
+-- oxc/                  # OXC parser utilities
|   +-- bindings/         # Expression binding extraction
|   +-- vue/              # Vue-specific OXC helpers (macros, type resolution, v-for, v-slot)
+-- vue/                  # Vue runtime helpers (tag detection, patch flags)
```

## Arena-Based Template AST

The parser builds a flat `Vec<AstNode>` arena with O(1) navigation:

```rust
pub struct TemplateAst {
    nodes: Vec<AstNode>,        // flat arena
    root: RootNodeTemplate,
}

pub struct AstNode {
    kind: AstNodeKind,          // Element | Text | Comment | Interpolation
    parent: Option<NodeId>,     // O(1) parent lookup
    index_in_parent: usize,     // O(1) sibling lookup
}
```

`ElementNode` pre-computes metadata during parsing to avoid re-scanning in codegen:

- `tag_type`: Element / Component / SlotOutlet / Template
- `prop_flag`: Bitset of prop characteristics (has class, style, spread, etc.)
- `children_flag`: Bitset of children characteristics (has text, elements, v-if, etc.)
- `children_mode`: Enum for codegen branching (Empty, TextOnly, SingleElement, Mixed, etc.)
- Cached directives: `v_condition`, `v_for`, `v_slot`, `v_once`, `v_ref`

## CodeTransform (Deferred Mutations)

All codegen phases use `CodeTransform` -- a chunk-based deferred mutation engine:

```rust
let mut ct = CodeTransform::new(input, &allocator);
ct.overwrite(start, end, replacement);  // deferred
ct.prepend_left(pos, content);          // deferred
let output = ct.build_string();         // single-pass concatenation
```

Key features:

- `cursor_hint`: Accelerates forward-progressing access patterns to amortized O(1)
- `output_delta`: Incremental length tracking avoids full scan
- Pre-allocated chunk capacity: `source_len / 13` (empirically tuned)

## CodeTransform Is the Single Source of Truth (CRITICAL)

**All modifications to generated code MUST go through `CodeTransform` operations** (`overwrite`, `prepend_left`, `append_left`, `move_with_suffix`, etc.). Never apply string replacements, regex transforms, or manual splicing to the output of `build_string()` or to content that was produced by a `CodeTransform`.

Post-hoc string manipulation breaks sourcemap accuracy: the `CodeTransform` generates source maps by tracking chunks (Original, Inserted, Moved, Overwritten). If you modify the string after the transform, byte offsets in the source map no longer match the actual content. This causes position mismatches in the LSP (e.g., hover landing on the wrong token, go-to-definition jumping to wrong locations).

**Correct:** Use `ct.prepend_left(pos, ".ts")` to insert text at a known position -- the chunk list and source map stay consistent.

**Wrong:** Call `content.replace(".vue'", ".vue.ts'")` on the built string -- the source map still reflects the pre-replace byte offsets.

## Binding Metadata Flow

1. `script/process.rs` parses `<script setup>` -> walks AST -> classifies bindings as `BindingType` (SetupConst, SetupRef, Props, etc.)
2. Bindings passed to `template/code_gen/` via `generate_template()` parameter
3. `BindingResolver` determines correct accessor prefix (`_ctx.`, `$setup.`, `__props.`) and suffix (`.value` for refs)
4. Binding patches accumulated in `CodeGenOutput`, batch-applied to `CodeTransform`

## IDE Prefixed-Expression Emit Substrate (`ide/template/emit.rs`)

IDE template codegen emits a Vue binding value as JSX through the typed `EmitOp` vocabulary so the user expression keeps an exact source-map mapping while synthetic JSX scaffolding stays unmapped. `EmitText` (`Static`/`Borrowed`/`Owned`) is the text payload; `EmitOp` variants are `InsertUnmapped` (order-preserving unmapped insert, lowers via `prepend_ordered_unmapped`), `InsertMapped` (`InsertedMapped` chunk, mapped at `source_start`+`content_offset`), `PreserveOriginal` (pure no-op — the bytes stay an `Original` 1:1 chunk), `OverwriteSyntheticBoundary` (delete + unmapped insert; NEVER a mapped `out.overwrite`), and `MoveOriginal`. `emit_op` is the single lowering point. `emit_jsx_binding_value` emits a `JsxBindingValue` (`source_expr`/`prefix`/`suffix`/`occurrences`/`bindings`) `occurrences` times for RELOCATED emission (native `v-model` emits the expression 2-3x); in-place sites (v-html, v-text, `:[key]`, `.foo=`, `v-bind="obj"`, static `:prop`) preserve the bytes and emit `OverwriteSyntheticBoundary` + `collect_binding_patches` around them.

The bug this replaces: baking `prefix + identifier` into one `out.overwrite(prop.start, prop_end, &format!(...))` produced a `Chunk::Overwritten` mapping the whole run back to the prop start (identifier hover/go-to-definition landed on the prop name). The flat-string IDE producers `resolve_prefixed_expr`/`resolve_prefixed_dynamic_arg` were deleted; wrapped/transformed flat-string consumers (v-on spreads, dynamic event-name keys, v-show) call the shared `build_prefixed_expr` directly. Guard: `crates/verter_compiler/tests/ide_no_baked_prefix_overwrite.rs`.

## Template Codegen Backends

Three backends implement the `TemplateCodeGen` trait, called by `walker::walk_template()` in DFS order:

- **VDOM** (`vdom/`): In-place source overwrites producing `_createElementVNode()` calls
- **Vapor** (`vapor/`): Replaces entire template block with direct DOM manipulation code

## Two Template Codegen Paths (CRITICAL)

The Rust compiler has **two separate template codegen paths**. Modifying one does NOT affect the other:

| Path           | Module                    | Purpose                                     | Output                           |
| -------------- | ------------------------- | ------------------------------------------- | -------------------------------- |
| **VDOM/Vapor** | `template/code_gen/vdom/` | Runtime render functions for bundler output | `_createElementVNode(...)` calls |
| **IDE**        | `ide/template/`           | Valid JSX/TSX for LSP/TSGO type checking    | `<div prop={expr}>` JSX elements |

The **LSP uses the IDE path** via `host.ensure_compiled()` with `CompileTarget::IDE`. TSGO type-checks this output. Changes to VDOM codegen do NOT affect LSP hover/completions. The IDE codegen auto-detects the script language: TS SFCs produce `.tsx` (TypeScript + JSX), while JS SFCs (no `lang` or `lang="js"`) produce `.jsx` (JavaScript + JSDoc annotations).

## Strict Slot Children Type Checking (Experimental)

When `strict_slots: true` (VS Code: `verter.experimental.strictSlots`), the IDE template codegen emits `strictRenderSlot` calls after the JSX tree. These enforce that slot children match the parent component's `defineSlots()` type signature ([RFC #733](https://github.com/vuejs/rfcs/discussions/733)).

**Generated pattern** (inside the block scope, after JSX):

```tsx
___VERTER___strictRenderSlot({} as NonNullable<ReturnType<typeof ___VERTER___Comp{offset}>['$slots']['{slot}']>, [TabItem, {} as HTMLElementTagNameMap["input"], "" as string]);
```

**Child type references**: Component -> constructor name, HTML element -> `HTMLElementTagNameMap["tag"]`, text/interpolation -> `"" as string`. Each child is a sourcemapped `InsertedMapped` chunk pointing to its template position.

**Skipped cases**: self-closing components (no children), `is_jsx` mode, `<component :is>` (deferred), whitespace-only text, comments.

**Key files**: `ide/template/mod.rs` (`StrictSlotEntry`, `collect_strict_slot_children`, `emit_strict_slot_checks`), `ide/script.rs` (ambient `strictRenderSlot` type declarations).

## Cached Directive Fields on ElementNode

The parser extracts structural directives from `el.props` via `prop.take()` and caches them as dedicated fields on `ElementNode` (`ast/types.rs`):

| Field         | Directive                     | In `el.props`? | Notes                                            |
| ------------- | ----------------------------- | -------------- | ------------------------------------------------ |
| `v_condition` | `v-if`, `v-else-if`, `v-else` | **No** (taken) | Contains `ElementNodeCondition` with kind + prop |
| `v_for`       | `v-for`                       | **No** (taken) | Contains the full `NodeProp`                     |
| `v_slot`      | `v-slot`, `#name`             | **No** (taken) | Contains the full `NodeProp`                     |
| `v_once`      | `v-once`                      | **No** (taken) | Contains the full `NodeProp`                     |
| `v_ref`       | `ref`, `:ref`                 | **No** (taken) | Contains the full `NodeProp`                     |

**Consequence**: Code iterating `el.props` will **never see** these directives. Both codegen paths must handle them explicitly. The IDE module removes `v-if/v-for/v-slot/v-once` attributes (they become JSX wrappers/removals) and converts `ref` to JSX expression syntax (`ref={"name"}`).

## IDE Script Error Recovery

When OXC encounters parse errors during typing (e.g., `count.` mid-expression), the IDE script codegen (`ide/script.rs`) uses a **truncate-and-reparse** strategy instead of falling back to degraded file-scope output:

1. Find the earliest error offset from OXC diagnostics.
2. Truncate source at the last newline before that offset -- the "clean prefix".
3. Re-parse only the clean prefix (which succeeds since the broken code is removed).
4. Use the clean prefix AST for normal codegen (import hoisting, binding extraction, macro processing). The broken tail passes through unchanged in the CodeTransform.

A lightweight token scanner (`ide/script_recover.rs`) recovers macro binding names from the broken tail so template bindings still resolve. This means typing `count.` at the end of a script preserves hover, completions, and go-to-definition for all declarations above the cursor.

**Fallback**: When the clean prefix is empty (error on first line) or the clean prefix itself fails to parse, the system falls back to file-scope error recovery mode (`process_tsx_script_setup_error_mode`).

## Style Preprocessing in Bundler Mode

Style blocks with `lang="scss"`, `lang="sass"`, or `lang="less"` require preprocessing to CSS. The pipeline differs between Vite and non-Vite bundlers:

**Vite mode** (Vite-owned preprocessing, matching `@vitejs/plugin-vue`):

1. During main `.vue` `transform()`, the plugin parses the SFC with `compiler.parse()` and caches raw style block content in `styleBlockCache`. Style preprocessing is **skipped** in `applyPreprocessorRequests()`.
2. `load()` returns raw style source (e.g., SCSS with `$variables`) from `styleBlockCache`.
3. Style URLs preserve the original lang (`lang.scss`, not `lang.css`) since `meta.style_langs` is never overwritten.
4. Vite's CSS pipeline preprocesses SCSS/SASS/Less/Stylus automatically between `load()` and `transform()`.
5. `transform()` always runs `compiler.compileStyleAsync()` for Vue-specific post-processing: scoped CSS attribute selectors (`[data-v-...]`) and CSS `v-bind()` rewriting. This runs even for unscoped plain CSS blocks (CSS `v-bind()` still needs rewriting).

**Non-Vite mode** (preprocessor fallback):
Style preprocessing goes through `preprocessBlock()` -> `preprocessStyle()` which calls Vite's `preprocessCSS()` in-process (if Vite config is available). The compiled CSS is sent to the Rust host via `applyBlockOverrides()`, and `apply_style_overrides()` updates `meta.style_langs` to `"css"`. The `transform()` hook uses Rust `processStyle()` for CSS scoping only.

**Compiler resolution**: `vue/compiler-sfc` is resolved once per plugin instance from the project root in `configResolved()` via `createRequire(join(root, "package.json"))("vue/compiler-sfc")`. This is stored in the `compiler` variable and used for both SFC parsing (`compiler.parse()`) and style post-processing (`compiler.compileStyleAsync()`).

**Key files**: `packages/unplugin/src/index.ts` (`styleBlockCache`, `compileStyleAsync` in transform, style load from cache), `packages/unplugin/src/core/preprocessor.ts` (non-Vite style preprocessing via `preprocessStyle()`), `crates/verter_session/src/host_upsert.rs` (`apply_style_overrides` -- lang update, non-Vite only), `crates/verter_session/src/id.rs` (`render_ids` -- URL generation).

## CSS Processing Pipeline

```
Style block content
    | style/v_bind.rs     -- scan v-bind() expressions
    | css/prepass.rs       -- replace Vue syntax with CSS markers
    | lightningcss         -- parse + normalize CSS
    | css/modules.rs       -- hash class names (CSS Modules)
    | css/scoped.rs        -- insert [data-v-xxx] attribute selectors
```

## CompileTarget (Selective Pipeline)

`CompileTarget` (bitflags in `verter_compiler::compile::types`) controls which compilation steps run:

| Flag            | Controls                                             | Used By           |
| --------------- | ---------------------------------------------------- | ----------------- |
| `STYLE`         | Style codegen (CSS scoping, modules, v-bind)         | Bundler           |
| `SCRIPT`        | Script codegen (macro expansion, binding extraction) | Bundler, Analysis |
| `TEMPLATE`      | Template VDOM/Vapor render function codegen          | Bundler           |
| `TSX`           | TSX template codegen for type checking               | LSP/IDE           |
| `TSC`           | TSC declaration file generation                      | TSC               |
| `TEMPLATE_DATA` | Template data extraction (binding occurrences)       | LSP, Analysis     |

**Presets:**

| Preset     | Flags                         | Consumer                    |
| ---------- | ----------------------------- | --------------------------- |
| `BUNDLER`  | `STYLE \| SCRIPT \| TEMPLATE` | `@verter/unplugin`, default |
| `IDE`      | `TSX`                         | LSP, TSGO                   |
| `ANALYSIS` | `SCRIPT \| TEMPLATE_DATA`     | MCP analysis                |

**Key API**: `VerterHost::ensure_compiled(canonical_id, profile)` compiles with the given profile's target. Used by LSP and MCP to populate the cache. `get_virtual_file()` still exists for retrieving specific virtual file outputs.

## Key Files

| File | Purpose |
| --- | --- |
| `crates/verter_compiler/src/compile.rs` | Pipeline orchestrator (tokenize -> parse -> style -> script -> template) |
| `crates/verter_compiler/src/parser/mod.rs` | SFC parser: tokenizer events -> root nodes + template AST |
| `crates/verter_compiler/src/ast/types.rs` | AstNode, ElementNode, NodeId, PropFlags |
| `crates/verter_compiler/src/script/macros.rs` | defineProps/Emits/Model/Slots/Expose/Options |
| `crates/verter_compiler/src/script/process.rs` | Script setup processing, companion script merging |
| `crates/verter_compiler/src/template/code_gen/mod.rs` | Template codegen entry point |
| `crates/verter_compiler/src/template/code_gen/walker.rs` | DFS tree walker (shared by VDOM/Vapor backends) |
| `crates/verter_compiler/src/template/code_gen/binding.rs` | BindingResolver (\_ctx./$setup. prefix resolution) |
| `crates/verter_compiler/src/template/code_gen/vdom/` | VDOM render function codegen |
| `crates/verter_compiler/src/template/code_gen/vapor/` | Vapor mode codegen |
| `crates/verter_compiler/src/ide/mod.rs` | IDE codegen entry: TSX (TS SFCs) or JSX+JSDoc (JS SFCs) |
| `crates/verter_compiler/src/ide/script.rs` | IDE script codegen: TS annotations or JSDoc equivalents |
| `crates/verter_compiler/src/ide/script_recover.rs` | Token scanner for macro binding recovery from broken tails |
| `crates/verter_compiler/src/ide/condition.rs` | v-if/v-else-if/v-else condition chain codegen |
| `crates/verter_compiler/src/ide/template/mod.rs` | IDE template codegen: Vue -> JSX, StrictSlotEntry, emit_strict_slot_checks |
| `crates/verter_compiler/src/ide/template/directives.rs` | IDE: v-if -> ternary, v-for -> .map(), v-show -> style |
| `crates/verter_compiler/src/ide/template/props.rs` | IDE: :prop -> prop={}, @event -> onEvent={} |
| `crates/verter_compiler/src/ide/template/emit.rs` | IDE typed prefixed-expression emit substrate (`EmitOp`, `emit_jsx_binding_value`) |
| `crates/verter_compiler/src/style/mod.rs` | generate_style() entry point |
| `crates/verter_compiler/src/style/v_bind.rs` | v-bind() scanning in CSS |
| `crates/verter_compiler/src/css/mod.rs` | process_style() -- CSS pipeline entry point |
| `crates/verter_compiler/src/css/prepass.rs` | Vue syntax -> valid CSS markers |
| `crates/verter_compiler/src/css/scoped.rs` | Scoped CSS: insert [data-v-xxx] selectors |
| `crates/verter_compiler/src/css/modules.rs` | CSS Modules: hash class names |
| `crates/verter_compiler/src/code_transform/code_transform.rs` | Chunk-based deferred mutation engine |
| `crates/verter_compiler/src/code_transform/chunk.rs` | Chunk types (Original, Overwritten, Inserted, InsertedMapped) |
| `crates/verter_compiler/src/code_transform/source_map.rs` | Source map generation from chunk positions |
