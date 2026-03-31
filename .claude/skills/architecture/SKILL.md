---
name: architecture
description: "Verter codebase architecture: Rust compiler modules, TypeScript packages, plugin system, LSP features, CSS analysis, MCP server, analysis types, key files index"
---

# Verter Architecture Reference

## Shared Substrate Principle

Verter is designed as one shared optimized codebase. Consumers should reuse the same lower-level crates instead of growing separate semantic pipelines.

- Put reusable parsing, analysis, type-resolution, caching, and import-following behavior in the shared owner crate.
- `verter_host` is the shared host/session/cache boundary for host-backed consumers.
- `verter_analysis` and `verter_core` own reusable semantics.
- `verter_ffi` owns boundary DTOs shared by NAPI and WASM.
- Consumer packages and apps should stay adapter-oriented: thin wrappers, public API shaping, transport glue, and UX-specific behavior.

When a bug or slowdown shows up in one product surface, prefer fixing it in the shared substrate so other consumers benefit automatically.

## TypeScript Packages

| Package                         | Purpose                                                                                                                                                                            | Entry Point        |
| ------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------ |
| **`@verter/core`**              | SFC parser & TSX transformer. Converts `.vue` files to valid TSX using `MagicString` for sourcemap preservation                                                                    | `src/v5/index.ts`  |
| **`@verter/types`**             | TypeScript utility types (`PatchHidden`, `ExtractHidden`, `EmitsToProps`, etc.). Has `/string` export with `$V_` prefixed types for LSP injection                                  | `src/index.ts`     |
| **`@verter/language-shared`**   | Shared custom protocol types between VS Code client and Rust LSP binary                                                                                                            | `src/index.ts`     |
| **`@verter/typescript-plugin`** | TypeScript plugin that resolves `.vue` imports in TS/JS files. Intercepts module resolution to return transformed TSX                                                              | `src/index.ts`     |
| **`verter-vscode`**             | VS Code extension. Launches Rust `verter-lsp` binary over stdio, bundles TS plugin, handles extension activation                                                                   | `src/extension.ts` |
| **`@verter/unplugin`**          | Universal bundler plugin (Vite, Rollup, webpack, esbuild, rspack, Rolldown, Farm). Compiles `.vue` files via `@verter/native`. Supports `preCompile` for build-start cache warming | `src/index.ts`     |
| **`@verter/oxc-bindings`**      | Helper for downloading platform-specific OXC parser binaries                                                                                                                       | `src/index.ts`     |

## Unplugin Configuration (`packages/unplugin/`)

`@verter/unplugin` provides a `VerterPluginOptions` interface:

| Option              | Type                                       | Default      | Description                                                                                                                                                                                                                                                                                                           |
| ------------------- | ------------------------------------------ | ------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `componentId`       | `(filename, source, isProd) => string`     | hash-based   | Custom component ID generator                                                                                                                                                                                                                                                                                         |
| `include`           | `string \| RegExp \| (string \| RegExp)[]` | `[/\.vue$/]` | File patterns to include                                                                                                                                                                                                                                                                                              |
| `preCompile`        | `boolean`                                  | `false`      | Pre-compile all `.vue` files during `buildStart`. Scans the project root, upserts files into the host cache (including type dependencies for macros), and compiles them. When `transform()` later receives the same content, the host returns the cached result instantly. `node_modules` are excluded from scanning. |
| `crossFileOptimize` | `boolean`                                  | `false`      | Cross-file prop constness optimization. Requires `preCompile: true`. After pre-compilation, analyzes the render tree to determine which props are always passed constant values, skipping dynamic tracking in compiled output.                                                                                        |
| `template`          | `object`                                   | —            | Template compiler options (compat with `@vitejs/plugin-vue`)                                                                                                                                                                                                                                                          |

**`preCompile` architecture:**

- During `buildStart()`, scans the project root for `.vue` files (excluding `node_modules` and dot-directories)
- For each file: upserts it into the host, resolves external `src` attributes and macro type dependencies (e.g., `import type { Props } from './types'` used in `defineProps<Props>()`), then triggers compilation
- When another plugin modifies the file before `transform()`, the host detects the content change via internal hashing and recompiles
- Third-party `.vue` files in `node_modules` compile on-demand during `transform()` — no pre-compilation overhead

**Macro type resolution invariant:** cross-file macro type resolution must only follow imports reachable from the requested type's local declaration graph. Unrelated imports in the same file are out of scope for that traversal, and plain imports are not implicit re-exports.

## Core TS Transformation Pipeline (`packages/core/src/v5/`)

```
Vue SFC → parser/ → process/script/plugins/ → TSX output
              ↓              ↓
         ParsedBlock    MagicString (preserves sourcemaps)
```

1. **`parser/`** - Parses SFC into typed blocks
   - `parser.ts` - Main entry, uses `@vue/compiler-sfc`
   - `types.ts` - `ParsedBlockScript`, `ParsedBlockTemplate`, `ParsedBlockUnknown`
   - `script/` - Extracts script AST items (`ScriptItem`, `ScriptTypes`)
   - `template/` - Parses template expressions and bindings

2. **`process/`** - Plugin-based transformation system
   - `script/script.ts` - Orchestrates plugin execution
   - `types.ts` - `ProcessContext`, `ProcessPlugin`, `ProcessItemType`

### Plugin System (`packages/core/src/v5/process/script/plugins/`)

Plugins transform parsed SFC items into TSX. Each plugin can:

- Hook into `pre`/`post` phases
- Transform specific `ScriptTypes` via `transformXxx` methods
- Add items to `context.items` for downstream plugins

| Plugin              | Purpose                                                                                                            |
| ------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `macros/`           | Transforms Vue macros (`defineProps`, `defineEmits`, `defineModel`, `defineSlots`, `defineExpose`, `withDefaults`) |
| `template-binding/` | Generates template binding type for IDE support                                                                    |
| `binding/`          | Tracks variable declarations for binding context                                                                   |
| `imports/`          | Handles import statements                                                                                          |
| `script-block/`     | Wraps script setup content                                                                                         |
| `full-context/`     | Generates component context type                                                                                   |
| `attributes/`       | Processes component attributes                                                                                     |
| `resolvers/`        | Resolves component references                                                                                      |

**Plugin execution order**: Controlled by `enforce: "pre" | "post"`. Pre-plugins run first, then main transforms, then post-plugins.

## TypeScript Code Patterns

**Defining script plugins:**

```typescript
import { definePlugin, ScriptContext } from "../../types";
export const MyPlugin = definePlugin({
  name: "my-plugin",
  enforce: "pre", // or "post"
  pre(s, ctx) {
    /* runs before transforms */
  },
  transformFunctionCall(item, s, context) {
    /* transform specific type */
  },
  transformDeclaration(item, s, context) {
    /* another type */
  },
  post(s, context) {
    /* runs after all transforms */
  },
});
```

**Type helper prefix convention:**

- Internal helpers use `___VERTER___` prefix (see `packages/core/`)
- String-exported types use `$V_` prefix for collision avoidance

**Parser types** (`packages/core/src/v5/parser/`):

- `ParsedBlockScript`, `ParsedBlockTemplate` - Block-specific parsed data
- `ScriptItem`, `ScriptTypes` - Categorized script AST items

## Language Server Architecture (`crates/verter_lsp/src/`)

The LSP is a standalone Rust binary (`verter-lsp`) that communicates with VS Code over stdio.

```
main.rs (stdio transport + CLI args + provider selection)
    ↓
server.rs (LSP message loop, request dispatch)
    ↓
documents/       → Document tracking and synchronization
features/        → LSP feature handlers (see table below)
analysis/        → Static analysis integration
css/             → CSS-specific language features
tsgo/            → TSGO type provider integration (LSP protocol)
tsserver/        → tsserver type provider integration (JSON protocol)
capabilities.rs  → Server capability registration
config.rs        → Server configuration, ProjectConfig, ProjectRegistry, vite alias discovery
workspace_scanner.rs → Async priority-based workspace file scanner
sync_coordinator.rs  → Debounced type provider sync during typing
```

### Per-Project Configuration (`config.rs`)

`ProjectRegistry` groups per-project config for multi-root workspaces. Each `ProjectConfig` has: root path, `TsConfigPathResolver`, `ResolvedLintConfig`, `Linter` instance, and optional `vite_config_path`/`vite_config_deps`. Tsconfig-backed projects use only tsconfig paths; fallback projects get Vite aliases via OXC static analysis (`vite_config.rs`) or trusted Node.js execution.

### TypeProvider Trait (`tsgo/traits.rs`)

Both TSGO and tsserver implement the `TypeProvider` trait (14+ methods: hover, completions, diagnostics, definition, references, rename, signature help, code actions, semantic tokens, highlights, inlay hints, open/update/close file, shutdown). The trait is object-safe (`dyn TypeProvider`) so the server is backend-agnostic.

### TSGO Module (`tsgo/`)

| File              | Purpose                                                                                        |
| ----------------- | ---------------------------------------------------------------------------------------------- |
| `ipc.rs`          | LSP client: Content-Length framing, JSON-RPC request/response correlation                      |
| `traits.rs`       | `TypeProvider` trait definition + `TypeProviderError`                                          |
| `protocol.rs`     | Response types: `CompletionResult`, `HoverInfo`, `TypeDiagnostic`, etc.                        |
| `resilient.rs`    | `ResilientTypeProvider`: crash detection via `Notify`, auto-restart (max 3), file cache replay |
| `project_sync.rs` | `ProjectSync`: batches `open_tsx`/`sync_tsx`/`close_tsx` calls to the provider                 |
| `merge.rs`        | Merges TSGO diagnostics/completions with verter's own results                                  |
| `mock.rs`         | Mock provider for integration tests                                                            |

### tsserver Module (`tsserver/`)

| File           | Purpose                                                                                                                                           |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| `mod.rs`       | `find_tsserver()` (tsdk → workspace → global), `find_node()`, `detect_ts_major_version()`                                                         |
| `ipc.rs`       | `TsserverTypeProvider`: newline-delimited JSON transport, position conversion (byte offset ↔ 1-based line/offset), all 14+ `TypeProvider` methods |
| `resilient.rs` | `ResilientTsserverProvider`: same crash/restart pattern as TSGO resilient wrapper                                                                 |

### Provider Selection (`main.rs`)

CLI arg `--type-provider=auto|tsgo|tsserver|off` (from VS Code `verter.typeProvider` setting):

- **auto**: detect TS version in node_modules — TS 5.x/6.x → tsserver + recommend TSGO; else try TSGO
- **tsgo/tsserver**: explicit, no fallback
- **off**: verter-only mode

Only one provider runs at a time. Provider PID is sent to the extension via `$/verter/typeProviderStarted` notification for orphan cleanup.

**LSP features** (`features/`):

| Module               | LSP Method                          | Description                                                              |
| -------------------- | ----------------------------------- | ------------------------------------------------------------------------ |
| `completion`         | `textDocument/completion`           | Auto-completions (components, props, CSS classes)                        |
| `definition`         | `textDocument/definition`           | Go-to-definition: bindings, imports, CSS ↔ template, DOM query selectors |
| `hover`              | `textDocument/hover`                | Hover info: types, CSS rules on elements, elements on selectors          |
| `diagnostics`        | `textDocument/publishDiagnostics`   | Script/template diagnostics                                              |
| `css_diagnostics`    | (custom)                            | Unused scoped CSS hints, cross-file cascade detection                    |
| `inlay_hints`        | `textDocument/inlayHint`            | DOM query → matched element, `useTemplateRef` → matched ref              |
| `folding_range`      | `textDocument/foldingRange`         | SFC block + template element folding                                     |
| `linked_editing`     | `textDocument/linkedEditingRange`   | Rename matching open/close HTML tags                                     |
| `rename`             | `textDocument/rename`               | Symbol renaming                                                          |
| `references`         | `textDocument/references`           | Find all references                                                      |
| `document_symbol`    | `textDocument/documentSymbol`       | Document outline                                                         |
| `document_highlight` | `textDocument/documentHighlight`    | Highlight same symbols                                                   |
| `code_lens`          | `textDocument/codeLens`             | Code lens annotations                                                    |
| `color_info`         | `textDocument/documentColor`        | CSS color picker                                                         |
| `formatting`         | `textDocument/formatting`           | Document formatting                                                      |
| `organize_imports`   | `source.organizeImports`            | Import organization                                                      |
| `extract_component`  | (code action)                       | Extract selection to new component                                       |
| `call_hierarchy`     | `textDocument/prepareCallHierarchy` | Call hierarchy navigation                                                |
| `workspace_symbol`   | `workspace/symbol`                  | Project-wide symbol search                                               |
| `document_link`      | `textDocument/documentLink`         | Clickable links in source                                                |
| `document_drop_edit` | `textDocument/onDropEdit`           | Drag-and-drop editing                                                    |

### LSP Feature Flow

```
Request (stdio) → server.rs → Find document in host cache → Feature handler → Response (stdio)
```

## CSS Analysis & Selector Matching (`crates/verter_analysis/src/`)

CSS analysis uses a lightweight byte-level scanner (no external CSS parser dependency). The scanner extracts selectors, classes, IDs, custom properties, and at-rules from `<style>` blocks.

**Module structure:**

```
style.rs              # CSS scanner, structured selector parser, specificity computation
selector_match.rs     # Three-valued selector matching against template elements
template.rs           # Template element analysis, dynamic class extraction, :style CSS var extraction
```

**Key types:**

| Type                     | Location            | Purpose                                                                   |
| ------------------------ | ------------------- | ------------------------------------------------------------------------- |
| `StructuredSelector`     | `style.rs`          | Parsed CSS selector (compounds + combinators)                             |
| `CompoundSelector`       | `style.rs`          | Single compound: element, classes, id, attributes, pseudo-classes         |
| `SelectorCombinator`     | `style.rs`          | Descendant / Child / NextSibling / LaterSibling                           |
| `MatchResult`            | `selector_match.rs` | Three-valued: `Matches`, `MaybeMatches`, `NoMatch`                        |
| `DomQueryCallSite`       | `types.rs`          | DOM query call with parsed selector and spans                             |
| `StyleBlockAnalysis`     | `style.rs`          | Per-`<style>` block analysis with nested `CssAnalysis`                    |
| `AnalyzedCustomProperty` | `style.rs`          | CSS custom property with name/value spans, var references, selector index |
| `CssVarReference`        | `style.rs`          | `var()` call with name, span, optional fallback (recursive)               |
| `AnalyzedVarUsage`       | `style.rs`          | Regular CSS property using `var()` with property name and selector index  |
| `CssVarManipulation`     | `types.rs`          | Script-side CSS variable manipulation via DOM APIs                        |
| `DynamicStyleVar`        | `template.rs`       | CSS variable set via `:style` binding in template                         |
| `StaticStyleVar`         | `template.rs`       | CSS variable set via static `style` attribute in template                 |
| `CssVarFlow`             | `project_index.rs`  | Cross-component CSS variable flow (definitions + usages + manipulations)  |

**CSS Variable Analysis (three-block tracking):**

- **Style**: `scan_declarations()` extracts `AnalyzedCustomProperty` (definitions with values/spans) and `AnalyzedVarUsage` (var() references). `extract_var_references()` handles nested var() fallbacks.
- **Template**: `extract_dynamic_style_vars()` extracts CSS vars from `:style="{ '--color': val }"`. `extract_static_style_vars()` extracts from `style="--color: red"`.
- **Script**: `try_extract_css_var_manipulation()` detects `el.style.setProperty('--x', val)`, `getPropertyValue('--x')`, `removeProperty('--x')`.
- **Cross-component**: `ProjectIndex.css_var_flow(name)` and `VerterHost.css_var_flow(name)` return `CssVarFlow` with all files defining/referencing/manipulating a variable.

**Selector matching algorithm** (`match_selector()`):

1. Match rightmost compound against target element
2. Walk left through combinators: `Child` checks `parent_index`, `Descendant` walks ancestor chain
3. Dynamic `:class` or component types → `MaybeMatches` (can't determine statically)
4. `:not()` inverts, `:is()`/`:where()` takes best match across alternatives

**Position encoding for CSS spans**: `CssAnalysis` spans (classes, IDs, selectors) are **SFC-absolute byte offsets**. The CSS scanner produces content-relative offsets internally, then `CssAnalysis::make_spans_absolute(content_offset)` is called at the host level (after optional SCSS remap) to convert all spans to SFC-absolute. Consumers use spans directly without adding any offset. `StyleBlockAnalysis.content_offset` is retained for documentation and slice operations.

## Rust Compiler Architecture (`crates/verter_core/src/`)

The Rust compiler uses an AST-based pipeline. The `compile()` orchestrator drives a linear 5-phase pipeline:

```
Vue SFC Source
    ↓
[Tokenizer]  byte-level SFC tokenization (tokenizer/byte.rs)
    ↓
[Parser]     builds arena-based template AST + extracts script/style blocks (parser/)
    ↓
[Style]      v-bind() scan + CSS processing (style/ + css/)
    ↓
[Script]     macro expansion, binding extraction, component wrapper (script/)
    ↓
[Template]   render function codegen — VDOM or Vapor backends (template/)
    ↓
[Compile]    orchestrates the above, applies CodeTransform, emits output (compile.rs)
```

**Module overview:**

```
compile.rs                # Pipeline orchestrator, options, result types
tokenizer/
├── byte.rs               # Zero-copy byte-level SFC tokenizer (production)
├── helpers.rs            # Tokenizer utility functions
└── types.rs              # Event, QuoteType
parser/
├── mod.rs                # Syntax state machine (tokenizer events → AST)
└── types.rs              # RootNodeScript, RootNodeStyle, RootNodeTemplate
ast/
├── mod.rs                # TemplateAst (flat arena with O(1) navigation)
├── builder.rs            # TemplateAstBuilder (incremental AST construction)
└── types.rs              # AstNode, ElementNode, NodeId, pre-computed flags
script/
├── mod.rs                # generate_script() entry point
├── process.rs            # Script setup processing, companion script merging
├── macros.rs             # defineProps/Emits/Model/Slots/Expose/Options
└── css_vars.rs           # _useCssVars() injection for v-bind() in styles
template/
├── oxc/                  # OXC expression parsing for template bindings
│   ├── mod.rs            # parse_template_expressions()
│   └── types.rs          # OxcParsedAst, OxcParsedElement, OxcParsedExpression
└── code_gen/             # Render function codegen
    ├── mod.rs            # generate_template() entry point
    ├── walker.rs         # DFS tree walker (shared by all backends)
    ├── types.rs          # TemplateCodeGen trait, CodeGenOutput
    ├── binding.rs        # BindingResolver (_ctx./$setup. prefix resolution)
    ├── shared/           # Shared codegen helpers
    ├── vdom/             # VDOM render function output (_createElementVNode, etc.)
    └── vapor/            # Vapor mode output (_template, _renderEffect, etc.)
ide/                      # IDE codegen: TSX or JSX+JSDoc (for LSP/TSGO type checking)
├── mod.rs                # generate_ide_template() — Vue template → valid JSX; IdeScriptOptions, IdeTemplateOptions
├── script.rs             # generate_ide_script() — script block → TS or JS+JSDoc wrapper
├── condition.rs          # v-if/v-else-if/v-else condition chain codegen
└── template/
    ├── mod.rs            # walk_element/walk_node, cached directive removal, ref conversion
    ├── directives.rs     # v-if → ternary, v-for → .map(), v-show → style
    └── props.rs          # :prop → prop={}, @event → onEvent={}, v-bind spread
style/
├── mod.rs                # generate_style() entry point
└── v_bind.rs             # v-bind() scanning in CSS
css/
├── mod.rs                # process_style() — CSS pipeline entry point
├── prepass.rs            # Vue syntax → valid CSS markers (v-bind, :deep, :slotted)
├── scoped.rs             # Scoped CSS: insert [data-v-xxx] selectors
├── modules.rs            # CSS Modules: hash class names
├── walk.rs               # String-level CSS selector walking
└── types.rs              # ProcessStyleOptions, ProcessStyleResult
code_transform/
├── code_transform.rs     # Chunk-based deferred mutation engine (MagicString equivalent)
├── chunk.rs              # Chunk types (Original, Overwritten, Inserted, InsertedMapped)
└── source_map.rs         # Source map generation from chunk positions
utils/
├── oxc/                  # OXC parser utilities
│   ├── bindings/         # Expression binding extraction
│   └── vue/              # Vue-specific OXC helpers (macros, type resolution, v-for, v-slot)
└── vue/                  # Vue runtime helpers (tag detection, patch flags)
```

### Arena-Based Template AST

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

### CodeTransform (Deferred Mutations)

All codegen phases use `CodeTransform` — a chunk-based deferred mutation engine:

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

### Binding Metadata Flow

1. `script/process.rs` parses `<script setup>` → walks AST → classifies bindings as `BindingType` (SetupConst, SetupRef, Props, etc.)
2. Bindings passed to `template/code_gen/` via `generate_template()` parameter
3. `BindingResolver` determines correct accessor prefix (`_ctx.`, `$setup.`, `__props.`) and suffix (`.value` for refs)
4. Binding patches accumulated in `CodeGenOutput`, batch-applied to `CodeTransform`

### Template Codegen Backends

Three backends implement the `TemplateCodeGen` trait, called by `walker::walk_template()` in DFS order:

- **VDOM** (`vdom/`): In-place source overwrites producing `_createElementVNode()` calls
- **Vapor** (`vapor/`): Replaces entire template block with direct DOM manipulation code

### CSS Processing Pipeline

```
Style block content
    ↓ style/v_bind.rs     — scan v-bind() expressions
    ↓ css/prepass.rs       — replace Vue syntax with CSS markers
    ↓ lightningcss         — parse + normalize CSS
    ↓ css/modules.rs       — hash class names (CSS Modules)
    ↓ css/scoped.rs        — insert [data-v-xxx] attribute selectors
```

### Cross-File Type Resolution

External types for macros like `defineProps<ExternalType>()` are pre-resolved by the host:

1. Host detects type dependencies from imports
2. Host resolves types from its file store
3. Host passes resolved types via `VerterCompileOptions::external_types`
4. `script/process.rs` merges external types with companion `<script>` types

The Rust compiler never does file I/O — all external resolution is the host's responsibility.

**Resolver ownership rule:** host-backed component-meta and analysis must share one cross-file resolver. Do not build separate resolver logic for script-setup macros, Options API metadata, compat wrappers, or consumer-specific adapters.

**Resolver modes:**

- `Type` mode: resolve the requested symbol and its canonical source location only. This is for tracking identity and navigation, not shape expansion.
- `Expanded` mode: use the same traversal and the same caches, then materialize the expanded shape / expanded text for metadata consumers.

**Component-meta rule:** all metadata-producing macro and Options API surfaces must go through the shared resolver in `Expanded` mode. That includes props, emits, slots, data, computed, and expose-style members.

**Traversal rule:** only follow the import graph reachable from the requested type's declaration graph. Unrelated imports in the same file are out of scope.

**Caching rule:** when parsing a `.ts` / `.js` / declaration file for type resolution, cache discovered symbol name → canonical location mappings. Cache direct re-exports, barrelled exports, and any discovered `export *` hops as well, because repeated wildcard-barrel scanning is expensive.

**Component-meta cache rule:** when changing `verter_host`, `verter_resolver`, `verter_analysis`, or `packages/component-meta` type paths, use cached lookup/eval state as the only source of truth after the cache-owning pass. Do not rewalk AST/source as a fallback to recover or expand types.

**Component-meta publication rule:** keep registry publication shallow and demand-driven. Publish and expand only the symbols required by the active metadata query; do not eagerly materialize unrelated owner-local or package-local helpers.

**Component-meta target-selection rule:** keep runtime/declaration companion selection shallow. Canonicalization may probe cached raw source existence, but must not build export analysis, snapshots, or eval envs just to choose a target file.

**Component-meta imported-state rule:** after shallow imported dependency seeding, resolver stages must consume only the imported dependency cache for imported file snapshots/envs/analysis. Do not bounce from alias or registry hydration back into raw snapshot/source builders for imported files.

**Component-meta deepening rule:** resolve one requested symbol/query path at a time. Do not let a file-level resolver widen into unrelated sibling symbols/files while chasing a single metadata request.

**Component-meta projection rule:** when projecting metadata/fallthrough from an already-resolved query, reuse that resolved state plus the captured store/session view. Do not bounce back out to a fresh top-level fallthrough/meta query.

**Component-meta collection rule:** imported-eval collection must stay lazy/BFS over the active symbol route. Do not add eager collector modes or source-text reparsing fallbacks in shared resolver code.

## CompileTarget (Selective Pipeline)

`CompileTarget` (bitflags in `verter_core::compile::types`) controls which compilation steps run:

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

## Analysis MCP Server (`verter_mcp`)

The `verter-mcp` binary exposes Verter's full analysis, diagnostics, compilation, and scoring pipeline via MCP. It provides 33 tools for AI agents.

### Key Architecture

- `VerterMcpServer` wraps `VerterHost` (with `AnalysisScope::LSP` for maximum analysis data), `Linter`, and `ActionEngine`
- Tools auto-load files from disk via `ensure_loaded()` — agents don't need to pre-load
- Template analysis requires a compilation pass — `ensure_template_analysis()` triggers it transparently
- Cross-file tools iterate all loaded files (no `ProjectIndex` exposed from host)
- Scoring engine computes composite 0-100 quality scores from a11y, lint, template complexity, API surface, CSS health, and reactivity dimensions

## verter_analysis — Static Analysis Types

`verter_analysis` is independent from `verter_core`. The compilation crate produces `RawTemplateData` during compilation, which `verter_host` converts into `verter_analysis` types.

### AnalysisScope

Bitflags (`u32`) controlling which analysis passes run during file upsert.

**Script (bits 0-7)**

| Flag                | Bit | Description                                          |
| ------------------- | --- | ---------------------------------------------------- |
| `IMPORTS`           | 0   | Import declarations                                  |
| `BINDINGS`          | 1   | Variable/function/class declarations                 |
| `REACTIVITY`        | 2   | Ref/reactive/computed classification                 |
| `MACROS`            | 3   | defineProps/Emits/Model/Slots/Expose                 |
| `MACRO_TYPE_DEPS`   | 4   | Cross-file type references in macros                 |
| `VUE_API_USAGE`     | 5   | Track provide/inject/lifecycle/watcher calls         |
| `EXPORT_SIGNATURES` | 6   | Per-export hashes for smart invalidation             |
| `FUNC_RETURNS`      | 7   | Analyze function return reactivity (for composables) |

**Template (bits 8-15)**

| Flag             | Bit | Description                                |
| ---------------- | --- | ------------------------------------------ |
| `TPL_COMPONENTS` | 8   | Component usages + prop expressions        |
| `TPL_BINDINGS`   | 9   | Which script bindings are used in template |
| `TPL_SLOTS`      | 10  | Slot definitions + usages                  |
| `TPL_REFS`       | 11  | Template ref attributes                    |
| `TPL_EVENTS`     | 12  | Event handler bindings                     |
| `TPL_CONSTNESS`  | 13  | Prop constness classification              |

**Style (bits 16-19)**

| Flag            | Bit | Description                                 |
| --------------- | --- | ------------------------------------------- |
| `STYLE_CSS`     | 16  | Full CSS analysis (selectors, classes, IDs) |
| `STYLE_VBIND`   | 17  | v-bind() in styles                          |
| `STYLE_SCOPED`  | 18  | Scoped/module metadata                      |
| `STYLE_PSEUDOS` | 19  | :deep/:global/:slotted                      |

**Cross-file (bits 24-26)**

| Flag                | Bit | Description                              |
| ------------------- | --- | ---------------------------------------- |
| `CROSS_RENDER_TREE` | 24  | Build render tree from template analysis |
| `CROSS_PROVIDE`     | 25  | Provide/inject chain validation          |
| `CROSS_PROP_CONST`  | 26  | Prop constness optimization              |

**Presets:**

| Preset            | Flags                                                                                                                              | Use Case                                              |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------- |
| `BUILD`           | IMPORTS, BINDINGS, MACROS, MACRO_TYPE_DEPS, EXPORT_SIGNATURES, STYLE_VBIND, STYLE_SCOPED                                           | Minimal overhead for compilation + smart invalidation |
| `BUILD_OPTIMIZED` | BUILD + REACTIVITY, VUE_API_USAGE, TPL_COMPONENTS, TPL_BINDINGS, TPL_CONSTNESS, CROSS_RENDER_TREE, CROSS_PROVIDE, CROSS_PROP_CONST | Build with cross-file optimization                    |
| `LSP`             | All flags                                                                                                                          | Full analysis for completions, hover, diagnostics     |
| `LINTER`          | IMPORTS, BINDINGS, REACTIVITY, MACROS, VUE_API_USAGE, TPL_COMPONENTS, TPL_BINDINGS, TPL_SLOTS, TPL_REFS, TPL_EVENTS                | Script + template for lint rules                      |
| `ESSENTIAL`       | IMPORTS, BINDINGS, MACROS, MACRO_TYPE_DEPS, EXPORT_SIGNATURES                                                                      | Script-only (legacy compat)                           |

### ScriptAnalysisSnapshot

Primary output of `build_script_analysis()`. Produced by a single OXC parse + AST walk.

| Field                | Type                            | Description                                          |
| -------------------- | ------------------------------- | ---------------------------------------------------- |
| `imports`            | `Vec<AnalyzedImport>`           | All import declarations with source, bindings, spans |
| `bindings`           | `Vec<AnalyzedBinding>`          | Top-level variable/function/class declarations       |
| `macros`             | `Vec<AnalyzedMacro>`            | Vue macro calls (defineProps, defineEmits, etc.)     |
| `macro_type_deps`    | `Vec<MacroTypeDep>`             | Cross-file type references used by macros            |
| `flags`              | `AnalysisFlags`                 | Bitwise flags for O(1) queries                       |
| `exported_functions` | `Vec<AnalyzedExportedFunction>` | Non-SFC exported functions (composable analysis)     |

**ReactivityKind**: None | Ref | Computed | Reactive | MaybeRef | Mutable

### TemplateAnalysisSnapshot

Populated after compilation by converting `RawTemplateData` from `verter_core`.

| Field                 | Type                             | Description                                       |
| --------------------- | -------------------------------- | ------------------------------------------------- |
| `components`          | `Vec<TemplateComponentUsage>`    | Components used in template with props and slots  |
| `binding_occurrences` | `Vec<TemplateBindingOccurrence>` | Script bindings referenced in template with spans |
| `defined_slots`       | `Vec<DefinedSlot>`               | `<slot>` elements defined in template             |
| `template_refs`       | `Vec<TemplateRef>`               | `ref="foo"` attributes                            |
| `event_handlers`      | `Vec<TemplateEventHandler>`      | `@click`, `@input`, etc.                          |
| `elements`            | `Vec<TemplateElement>`           | Full element tree for linter traversal            |
| `if_chains`           | `Vec<IfChain>`                   | v-if/v-else-if chains for duplicate detection     |
| `prop_definitions`    | `Vec<AnalyzedPropDefinition>`    | Props from defineProps                            |
| `emit_definitions`    | `Vec<AnalyzedEmitDefinition>`    | Emits from defineEmits                            |
| `comment_directives`  | `Vec<CommentDirective>`          | `@verter:disable`, `@verter:todo`, etc.           |

### ProjectIndex

Aggregates file-level usage into project-wide indexes:

- **provide_index**: provide key → files that call `provide(key)`
- **inject_index**: inject key → files that call `inject(key)`
- **component_graph**: file → components it uses (forward edges)
- **component_reverse_index**: component name → files that use it
- **class_index**: CSS class name → files that define it
- **v_bind_css_index**: v-bind CSS expression → files that use it
- **custom_property_index**: CSS custom property → files that define it

### Data Flow

```
Vue SFC Source
    |
    v
verter_core::compile()
    |-- ScriptAnalysisSnapshot (from OXC parse during compilation)
    |-- RawTemplateData (spans, binding refs, component tags)
    |-- CssParsed* (v-bind spans, pseudo spans)
    |
    v
verter_host (conversion layer)
    |-- RawTemplateData --> TemplateAnalysisSnapshot
    |-- CssParsed*      --> StyleBlockAnalysis
    |-- Resolves import paths, populates resolved_canonical_id
    |-- Updates ProjectIndex with file usage
    |
    v
Consumers (LSP, build, linter) query snapshots + ProjectIndex
```

## Key Files

| File                                                                                        | Purpose                                                                |
| ------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `packages/core/src/v5/parser/parser.ts`                                                     | Main SFC parser entry                                                  |
| `packages/core/src/v5/process/script/script.ts`                                             | Script processing orchestration                                        |
| `packages/core/src/v5/process/script/types.ts`                                              | `definePlugin`, `ScriptContext`, `ScriptPlugin`                        |
| `packages/core/src/v5/process/script/plugins/macros/macros.ts`                              | Vue macro transformations                                              |
| `crates/verter_lsp/src/main.rs`                                                             | LSP binary entry point (stdio transport, CLI args, provider selection) |
| `crates/verter_lsp/src/server.rs`                                                           | LSP message loop, request dispatch, feature routing                    |
| `crates/verter_mcp/src/main.rs`                                                             | MCP binary entry point (stdio + HTTP transport)                        |
| `crates/verter_mcp/src/server.rs`                                                           | MCP tool router: 33 tools for analysis, diagnostics, scoring           |
| `crates/verter_mcp/src/tools/scoring.rs`                                                    | Quality/a11y scoring engine (0-100 composite scores)                   |
| `packages/types/src/helpers/helpers.ts`                                                     | Core type utilities                                                    |
| `crates/verter_core/src/compile.rs`                                                         | Pipeline orchestrator (tokenize → parse → style → script → template)   |
| `crates/verter_core/src/parser/mod.rs`                                                      | SFC parser: tokenizer events → root nodes + template AST               |
| `crates/verter_core/src/ast/types.rs`                                                       | AstNode, ElementNode, NodeId, PropFlags                                |
| `crates/verter_core/src/script/macros.rs`                                                   | defineProps/Emits/Model/Slots/Expose/Options                           |
| `crates/verter_core/src/script/process.rs`                                                  | Script setup processing, companion script merging                      |
| `crates/verter_core/src/template/code_gen/mod.rs`                                           | Template codegen entry point                                           |
| `crates/verter_core/src/template/code_gen/walker.rs`                                        | DFS tree walker (shared by VDOM/Vapor backends)                        |
| `crates/verter_core/src/template/code_gen/binding.rs`                                       | BindingResolver (\_ctx./$setup. prefix resolution)                     |
| `crates/verter_core/src/template/code_gen/vdom/`                                            | VDOM render function codegen                                           |
| `crates/verter_core/src/template/code_gen/vapor/`                                           | Vapor mode codegen                                                     |
| `crates/verter_core/src/ide/mod.rs`                                                         | IDE codegen entry: TSX (TS SFCs) or JSX+JSDoc (JS SFCs)                |
| `crates/verter_core/src/ide/script.rs`                                                      | IDE script codegen: TS annotations or JSDoc equivalents                |
| `crates/verter_core/src/ide/template/mod.rs`                                                | IDE template codegen: Vue → JSX (used by LSP/TSGO)                     |
| `crates/verter_core/src/ide/template/directives.rs`                                         | IDE: v-if → ternary, v-for → .map(), v-show → style                    |
| `crates/verter_core/src/ide/template/props.rs`                                              | IDE: :prop → prop={}, @event → onEvent={}                              |
| `crates/verter_core/src/ide/template/mod.rs` (`StrictSlotEntry`, `emit_strict_slot_checks`) | IDE: strict slot children type checking (`strictRenderSlot` calls)     |
| `crates/verter_core/src/css/`                                                               | CSS preprocessing and style transformation                             |
| `crates/verter_core/src/code_transform/code_transform.rs`                                   | Chunk-based deferred mutation engine                                   |
| `crates/verter_analysis/src/lib.rs`                                                         | Static analysis entry: imports, exports, bindings                      |
| `crates/verter_analysis/src/style.rs`                                                       | CSS scanner, selector parser, specificity computation                  |
| `crates/verter_analysis/src/selector_match.rs`                                              | Three-valued CSS selector matching against template elements           |
| `crates/verter_analysis/src/template.rs`                                                    | Template element analysis, component usage, dynamic class extraction   |
| `crates/verter_scheduler/src/scheduler.rs`                                                  | Scheduler: DashMap of FileNodes, driver loop, stage dispatch           |
| `crates/verter_scheduler/src/node.rs`                                                       | FileNode: ArcSwap snapshots, generation counter, pending requests      |
| `crates/verter_scheduler/src/executor.rs`                                                   | StageExecutor trait: host plugs in parse/analysis/compile              |
| `crates/verter_scheduler/src/edges.rs`                                                      | EdgeManager: ReverseIndex + BlockerRegistry (DashMap-sharded)          |
| `crates/verter_scheduler/src/queue.rs`                                                      | JobIndex: priority queue with aging, dedup, cancel                     |
| `crates/verter_host/src/lib.rs`                                                             | Host entry: compile, cache, upsert, dependency tracking                |
| `crates/verter_host/src/host_executor.rs`                                                   | HostStageExecutor: real parse pipeline for scheduler                   |
| `crates/verter_host/src/scheduler_shim.rs`                                                  | SchedulerBackedWorkspace: migration shim                               |
| `crates/verter_ffi/src/lib.rs`                                                              | FFI types shared between NAPI and WASM                                 |
| `packages/unplugin/src/index.ts`                                                            | Unplugin factory: `buildStart` (preCompile), `transform`, `load` hooks |
| `packages/unplugin/src/core/types.ts`                                                       | `VerterPluginOptions`, `HmrStrategy`                                   |
| `packages/unplugin/src/core/scanner.ts`                                                     | `scanVueFiles()` — async recursive directory walker for preCompile     |
| `packages/unplugin/src/core/compiler.ts`                                                    | Host singleton, `generateComponentId`, `processStyle`                  |
| `crates/verter_lsp/src/tsgo/traits.rs`                                                      | `TypeProvider` trait definition (14+ methods)                          |
| `crates/verter_lsp/src/tsgo/ipc.rs`                                                         | TSGO LSP client: Content-Length framing, JSON-RPC                      |
| `crates/verter_lsp/src/tsgo/resilient.rs`                                                   | `ResilientTypeProvider`: crash detection + auto-restart                |
| `crates/verter_lsp/src/tsgo/project_sync.rs`                                                | `ProjectSync`: batches open/sync/close to type provider                |
| `crates/verter_lsp/src/tsserver/mod.rs`                                                     | `find_tsserver()`, `find_node()`, `detect_ts_major_version()`          |
| `crates/verter_lsp/src/tsserver/ipc.rs`                                                     | `TsserverTypeProvider`: JSON transport, position conversion            |
| `crates/verter_lsp/src/tsserver/resilient.rs`                                               | `ResilientTsserverProvider`: crash detection + auto-restart            |
| `crates/verter_lsp/src/scheduler_integration.rs`                                            | LSP scheduler helpers: priority mapping, submit/read                   |
