# Verter

Verter is a Vue compiler and Language Server Protocol (LSP) implementation. It converts Vue Single File Components (SFCs) to valid TSX (leveraging TypeScript for type checking) and compiles templates to optimized render functions. Unlike Volar, Verter generates actual valid TSX code rather than virtual files.

The project is a hybrid Rust + TypeScript monorepo: Rust crates handle template compilation (exposed via NAPI-RS native bindings and wasm-bindgen WASM) and the LSP server (`verter_lsp` binary, communicates over stdio), while TypeScript packages handle the SFC-to-TSX transformation and IDE integration.

## Architecture

### Package Dependency Graph

```
verter-vscode (VS Code extension)
├── verter-lsp (Rust LSP binary, stdio)
│   ├── verter_host (file host + compilation)
│   ├── verter_diagnostics (lint rules + DiagnosticSet)
│   ├── verter_actions (quick fixes + refactoring)
│   └── TsgoTypeProvider (optional, for TS type checking)
├── @verter/language-shared (custom protocol types)
├── @verter/typescript-plugin (.vue import resolution, NAPI-backed)
└── @verter/unplugin (bundler plugin)
    └── @verter/native

verter-mcp (MCP server binary, stdio + HTTP)
├── verter_host (file host + compilation)
├── verter_analysis (static analysis snapshots)
├── verter_diagnostics (lint rules + DiagnosticSet)
└── verter_actions (quick fixes + refactoring)

@verter/playground (Netlify-hosted)
└── @verter/wasm (Rust template compiler, wasm-bindgen)
```

### Repository Structure

```
crates/
  verter_core/       # Core template compiler (Rust)
  verter_analysis/   # Static analysis: imports, exports, bindings, type resolution
  verter_host/       # In-memory file host: caching, dependency tracking, multi-file compilation
  verter_diagnostics/ # Vue SFC diagnostic engine: rule trait, visitor, diagnostics (depends only on verter_analysis)
  verter_actions/    # Code actions engine: quick fixes, refactoring (depends on verter_diagnostics + verter_analysis)
  verter_lsp/        # Rust LSP server binary (stdio, launched by VS Code extension)
  verter_ffi/        # FFI types: shared serializable structs for NAPI/WASM boundaries
  verter_bench/      # Benchmarks and comparison examples (Rust)
  verter_mcp/        # MCP server binary: analysis, diagnostics, scoring for AI agents
  verter_napi/       # Native Node.js bindings (NAPI-RS cdylib)
  verter_wasm/       # WASM bindings (wasm-bindgen cdylib)
packages/
  core/              # @verter/core - SFC parser & TSX transformer
  types/             # @verter/types - TypeScript utility types
  native/            # @verter/native - Native binding loader + platform packages
  wasm/              # @verter/wasm - WASM binding wrapper
  unplugin/          # @verter/unplugin - Universal bundler plugin
  language-shared/   # @verter/language-shared - Shared LSP protocol types
  typescript-plugin/ # @verter/typescript-plugin - TS language service plugin
  oxc-bindings/      # @verter/oxc-bindings - OXC parser binary helper
  playground/        # @verter/playground - Online playground (private, Netlify-hosted)
  vue-vscode/        # verter-vscode - VS Code extension
  example/           # Example project
scripts/
  check-versions.mjs # Version check + publish order for CI
```

### TypeScript Packages

| Package | Purpose | Entry Point |
|---------|---------|-------------|
| **`@verter/core`** | SFC parser & TSX transformer. Converts `.vue` files to valid TSX using `MagicString` for sourcemap preservation | `src/v5/index.ts` |
| **`@verter/types`** | TypeScript utility types (`PatchHidden`, `ExtractHidden`, `EmitsToProps`, etc.). Has `/string` export with `$V_` prefixed types for LSP injection | `src/index.ts` |
| **`@verter/language-shared`** | Shared custom protocol types between VS Code client and Rust LSP binary | `src/index.ts` |
| **`@verter/typescript-plugin`** | TypeScript plugin that resolves `.vue` imports in TS/JS files. Intercepts module resolution to return transformed TSX | `src/index.ts` |
| **`verter-vscode`** | VS Code extension. Launches Rust `verter-lsp` binary over stdio, bundles TS plugin, handles extension activation | `src/extension.ts` |
| **`@verter/unplugin`** | Universal bundler plugin (Vite, Rollup, webpack, esbuild, rspack, Rolldown, Farm). Compiles `.vue` files via `@verter/native`. Supports `preCompile` for build-start cache warming | `src/index.ts` |
| **`@verter/oxc-bindings`** | Helper for downloading platform-specific OXC parser binaries | `src/index.ts` |

### Unplugin Configuration (`packages/unplugin/`)

`@verter/unplugin` provides a `VerterPluginOptions` interface:

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `componentId` | `(filename, source, isProd) => string` | hash-based | Custom component ID generator |
| `include` | `string \| RegExp \| (string \| RegExp)[]` | `[/\.vue$/]` | File patterns to include |
| `preCompile` | `boolean` | `false` | Pre-compile all `.vue` files during `buildStart`. Scans the project root, upserts files into the host cache (including type dependencies for macros), and compiles them. When `transform()` later receives the same content, the host returns the cached result instantly. `node_modules` are excluded from scanning. |
| `crossFileOptimize` | `boolean` | `false` | Cross-file prop constness optimization. Requires `preCompile: true`. After pre-compilation, analyzes the render tree to determine which props are always passed constant values, skipping dynamic tracking in compiled output. |
| `template` | `object` | — | Template compiler options (compat with `@vitejs/plugin-vue`) |

**`preCompile` architecture:**
- During `buildStart()`, scans the project root for `.vue` files (excluding `node_modules` and dot-directories)
- For each file: upserts it into the host, resolves external `src` attributes and macro type dependencies (e.g., `import type { Props } from './types'` used in `defineProps<Props>()`), then triggers compilation
- When another plugin modifies the file before `transform()`, the host detects the content change via internal hashing and recompiles
- Third-party `.vue` files in `node_modules` compile on-demand during `transform()` — no pre-compilation overhead

### Core Transformation Pipeline (`packages/core/src/v5/`)

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

| Plugin | Purpose |
|--------|---------|
| `macros/` | Transforms Vue macros (`defineProps`, `defineEmits`, `defineModel`, `defineSlots`, `defineExpose`, `withDefaults`) |
| `template-binding/` | Generates template binding type for IDE support |
| `binding/` | Tracks variable declarations for binding context |
| `imports/` | Handles import statements |
| `script-block/` | Wraps script setup content |
| `full-context/` | Generates component context type |
| `attributes/` | Processes component attributes |
| `resolvers/` | Resolves component references |

**Plugin execution order**: Controlled by `enforce: "pre" | "post"`. Pre-plugins run first, then main transforms, then post-plugins.

### Language Server Architecture (`crates/verter_lsp/src/`)

The LSP is a standalone Rust binary (`verter-lsp`) that communicates with VS Code over stdio.

```
main.rs (stdio transport + CLI args)
    ↓
server.rs (LSP message loop, request dispatch)
    ↓
documents/       → Document tracking and synchronization
features/        → LSP feature handlers (see table below)
analysis/        → Static analysis integration
css/             → CSS-specific language features
tsgo/            → Optional tsgo type provider integration
capabilities.rs  → Server capability registration
config.rs        → Server configuration
```

**LSP features** (`features/`):

| Module | LSP Method | Description |
|--------|-----------|-------------|
| `completion` | `textDocument/completion` | Auto-completions (components, props, CSS classes) |
| `definition` | `textDocument/definition` | Go-to-definition: bindings, imports, CSS ↔ template, DOM query selectors |
| `hover` | `textDocument/hover` | Hover info: types, CSS rules on elements, elements on selectors |
| `diagnostics` | `textDocument/publishDiagnostics` | Script/template diagnostics |
| `css_diagnostics` | (custom) | Unused scoped CSS hints, cross-file cascade detection |
| `inlay_hints` | `textDocument/inlayHint` | DOM query → matched element, `useTemplateRef` → matched ref |
| `folding_range` | `textDocument/foldingRange` | SFC block + template element folding |
| `linked_editing` | `textDocument/linkedEditingRange` | Rename matching open/close HTML tags |
| `rename` | `textDocument/rename` | Symbol renaming |
| `references` | `textDocument/references` | Find all references |
| `document_symbol` | `textDocument/documentSymbol` | Document outline |
| `document_highlight` | `textDocument/documentHighlight` | Highlight same symbols |
| `code_lens` | `textDocument/codeLens` | Code lens annotations |
| `color_info` | `textDocument/documentColor` | CSS color picker |
| `formatting` | `textDocument/formatting` | Document formatting |
| `organize_imports` | `source.organizeImports` | Import organization |
| `extract_component` | (code action) | Extract selection to new component |
| `call_hierarchy` | `textDocument/prepareCallHierarchy` | Call hierarchy navigation |
| `workspace_symbol` | `workspace/symbol` | Project-wide symbol search |
| `document_link` | `textDocument/documentLink` | Clickable links in source |
| `document_drop_edit` | `textDocument/onDropEdit` | Drag-and-drop editing |

### CSS Analysis & Selector Matching (`crates/verter_analysis/src/`)

CSS analysis uses a lightweight byte-level scanner (no external CSS parser dependency). The scanner extracts selectors, classes, IDs, custom properties, and at-rules from `<style>` blocks.

**Module structure:**

```
style.rs              # CSS scanner, structured selector parser, specificity computation
selector_match.rs     # Three-valued selector matching against template elements
template.rs           # Template element analysis, dynamic class extraction
```

**Key types:**

| Type | Location | Purpose |
|------|----------|---------|
| `StructuredSelector` | `style.rs` | Parsed CSS selector (compounds + combinators) |
| `CompoundSelector` | `style.rs` | Single compound: element, classes, id, attributes, pseudo-classes |
| `SelectorCombinator` | `style.rs` | Descendant / Child / NextSibling / LaterSibling |
| `MatchResult` | `selector_match.rs` | Three-valued: `Matches`, `MaybeMatches`, `NoMatch` |
| `DomQueryCallSite` | `types.rs` | DOM query call with parsed selector and spans |
| `StyleBlockAnalysis` | `style.rs` | Per-`<style>` block analysis with nested `CssAnalysis` |

**Selector matching algorithm** (`match_selector()`):
1. Match rightmost compound against target element
2. Walk left through combinators: `Child` checks `parent_index`, `Descendant` walks ancestor chain
3. Dynamic `:class` or component types → `MaybeMatches` (can't determine statically)
4. `:not()` inverts, `:is()`/`:where()` takes best match across alternatives

**Position encoding for CSS spans**: `CssAnalysis` spans (classes, IDs, selectors) are **SFC-absolute byte offsets**. The CSS scanner produces content-relative offsets internally, then `CssAnalysis::make_spans_absolute(content_offset)` is called at the host level (after optional SCSS remap) to convert all spans to SFC-absolute. Consumers use spans directly without adding any offset. `StyleBlockAnalysis.content_offset` is retained for documentation and slice operations.

### Position Encoding

#### Span Conventions (MUST follow)

##### Typed Span Types (`verter_span` crate)

All Rust span types are defined in `crates/verter_span/src/lib.rs`. Each type enforces a specific coordinate system at compile time:

| Type | Meaning | Serde? | Used Where |
|------|---------|--------|------------|
| `Span` | SFC-absolute byte offsets `[start, end)` | **Yes** (`spanStart`/`spanEnd`) | Analysis types, diagnostics, CSS analysis, CodeTransform, Raw* template data |
| `RelativeSpan` | Byte offsets relative to a base stored elsewhere | **No** | CSS scanner internals, OXC binding extraction (`Binding.span`) |
| `PartialGeneratedSpan` | Unresolved position in generated output (TSX) | **No** | TSGO response parsing before PositionMapper resolution |
| `GeneratedSpan` | Resolved mapping: generated position + SFC origin | **No** | TSGO diagnostics after resolution, codegen error mapping |

##### Typed Span Rules

1. **All data crossing a serialization boundary (serde, MCP, LSP custom protocol, FFI) MUST use `Span` (SFC-absolute).** `RelativeSpan`, `PartialGeneratedSpan`, and `GeneratedSpan` do not implement `Serialize`/`Deserialize`. Attempting to put them in a serializable struct is a compile error. Convert with `to_absolute(base)` before serialization.
2. **Inter-crate stored types prefer `Span`.** Types in analysis snapshots, host results, and diagnostic structs that cross crate boundaries use `Span`. `RelativeSpan` is for intra-crate processing only (e.g., CSS scanner working on a style block, OXC binding extraction within an expression).
3. **`RelativeSpan` is 8 bytes, same as `Span`.** The base offset lives in context (field on parent struct, function parameter). The value of `RelativeSpan` is compile-time type safety, not runtime data.
4. **`PartialGeneratedSpan` → `GeneratedSpan` via resolution.** Use `PartialGeneratedSpan` for raw TSGO byte offsets. After PositionMapper lookup resolves the SFC origin, use `partial.resolve(origin_span)` to get a `GeneratedSpan`. For display (LSP diagnostics), use `generated_span.origin`.

##### Key APIs

- `Span::new(start, end)` / `RelativeSpan::new(start, end)` / `PartialGeneratedSpan::new(start, end)`
- `RelativeSpan::to_absolute(base: u32) -> Span` — add base offset
- `Span::to_relative(base: u32) -> RelativeSpan` — subtract base offset
- `PartialGeneratedSpan::resolve(origin: Span) -> GeneratedSpan` — resolve with SFC origin
- `GeneratedSpan::new(generated: Span, origin: Span)` — create resolved mapping directly
- `slice(&self, source: &str) -> &str` — on `Span`, `RelativeSpan`, `PartialGeneratedSpan`
- `From<oxc_span::Span>` for both `Span` and `RelativeSpan`
- **No `From` conversions between span types** — type safety enforced at compile time

##### Position Encoding Layers

| Layer | Offset Format | Line/Col Base | Description |
|-------|---------------|---------------|-------------|
| **oxc_span** | UTF-8 byte offset, relative to parse start | N/A (byte offsets only) | OXC parser spans are byte offsets from the start of the parsed source text |
| **verter `Span`** | UTF-8 byte offset, absolute for the document | N/A (byte offsets only) | All stored Rust spans (`span` fields in analysis types, `CodeTransform` positions) are byte offsets from the start of the SFC source |
| **verter `RelativeSpan`** | UTF-8 byte offset, relative to a base | N/A (byte offsets only) | CSS scanner internals (relative to style content start), OXC bindings (relative to expression start) |
| **PositionResolver** | N/A | **1-based** line, **1-based** UTF-16 column | `cursor/position.rs` — returns 1-based. When passing to source maps or LSP, subtract 1 |
| **Source maps** | N/A | **0-based** line, **0-based** column | VLQ-encoded. `source_map.rs` converts from PositionResolver via `(line - 1, col - 1)` |
| **LSP Protocol** | Negotiated (UTF-8/UTF-16/UTF-32) | **0-based** line, **0-based** character | `Position { line: 0, character: 0 }` = first char of file. `LineIndex` handles conversion |
| **VS Code API** | UTF-16 code units | **0-based** line, **0-based** character | `new Position(0, 0)` = first char. Matches LSP UTF-16 |
| **verter_ffi** | UTF-16 code units | N/A (byte offsets only) | NAPI/WASM boundary always communicates in UTF-16 offsets. Reference: `crates/verter_ffi/src/convert.rs:byte_offset_to_utf16()` |
| **verter_lsp** | Negotiated encoding (UTF-8, UTF-16, or UTF-32) | **0-based** | The LSP negotiates encoding with the client during `initialize()`. All positions sent to and received from the client use the negotiated encoding |

#### Line/Column Base Rules (CRITICAL — off-by-one bugs)

- **PositionResolver is 1-based** (`cursor/position.rs`): `offset_to_line_and_col()` and `offset_to_line_col()` return (1-based line, 1-based column). Always subtract 1 before passing to source maps or LSP.
- **Source maps are 0-based**: VLQ segments use 0-indexed lines and columns.
- **LSP is 0-based**: `Position { line: 0, character: 0 }` is the first character.
- **VS Code is 0-based**: `new Position(0, 0)` is the first character.
- **OXC/verter spans are byte offsets** — no line/column, no base conversion needed.

#### LSP Position Encoding Negotiation

1. Server reads `capabilities.general.positionEncodings` from the client during `initialize()`
2. Server picks the best encoding: prefer UTF-8 (no conversion needed) > UTF-32 > UTF-16 > default UTF-16
3. Server announces the selected encoding in `ServerCapabilities.position_encoding`
4. All LSP positions (standard and custom protocol) use the negotiated encoding

**Standard LSP positions** (`line:character`): handled by `LineIndex` in `documents/line_index.rs`.
**Custom protocol data** (analysis spans): converted at the LSP boundary before serialization.

#### Encoding Conversion Reference

| Boundary | Pattern | Reference Implementation |
|----------|---------|--------------------------|
| Rust internal → NAPI/WASM | `byte_offset_to_utf16()` | `crates/verter_ffi/src/convert.rs:281` |
| Rust internal → LSP client | Negotiated encoding conversion | `crates/verter_lsp/src/documents/mod.rs` |
| LSP Position → byte offset | `LineIndex::position_to_offset()` | `crates/verter_lsp/src/documents/line_index.rs` |
| Byte offset → LSP Position | `LineIndex::offset_to_position()` | `crates/verter_lsp/src/documents/line_index.rs` |
| TSGO response → byte offset | `position_to_offset()` | `crates/verter_lsp/src/tsgo/ipc.rs` (ASCII TSX only) |

#### VS Code Extension

VS Code negotiates UTF-16. Analysis offsets arrive as UTF-16 code units from file start. JS string indexing is UTF-16 native, so `source.charCodeAt(offset)` and `source.length` work directly. Use the shared `utf16OffsetToPosition()` from `packages/vue-vscode/src/utils.ts`.

#### TSGO Integration

TSGO processes generated TSX which is always ASCII. For ASCII: byte offset == UTF-16 offset == UTF-32 offset. The `position_to_offset()`/`offset_to_position()` functions in `ipc.rs` treat `character` as byte offset within the line — correct for ASCII. Diagnostics from `publishDiagnostics` must resolve LSP positions to byte offsets using the TSX content cache.

### Path Normalization (CRITICAL — normalize at boundaries)

All file paths are stored internally in **canonical ID** format. Normalization happens at entry boundaries (receiving paths); denormalization happens at exit boundaries (sending paths to external systems).

#### Canonical ID Format

| Rule | Example |
|------|---------|
| Forward slashes only | `c:/Users/dev/App.vue` (never `c:\Users\dev\App.vue`) |
| Lowercase Windows drive | `c:/Users/...` (never `C:/Users/...`) |
| No query strings | `App.vue` (not `App.vue?vue&type=script`) |
| No virtual suffixes | `App.vue` (not `App.vue._VERTER_.bundle.ts`) |
| UTF-8, no percent-encoding | `/home/user/my project/App.vue` (not `my%20project`) |

#### Entry Boundaries (External → Canonical)

| Source | Function | Location |
|--------|----------|----------|
| LSP client URI | `uri_to_canonical_id_from_str()` | `verter_lsp/src/documents/mod.rs` |
| File system / bundler path | `canonicalize_id()` | `verter_host/src/id.rs` |
| Bundler plugin | `generateComponentId()` | `packages/unplugin/src/core/compiler.ts` |
| CLI args | `path_to_file_uri()` | `verter_lsp/src/main.rs` |

#### Exit Boundaries (Canonical → External)

| Target | Pattern | Location |
|--------|---------|----------|
| LSP client (file URI) | `file:///` + canonical ID | `verter_lsp/src/features/definition.rs` |
| TSGO type provider | `path_to_file_uri()` | `verter_lsp/src/main.rs` |
| File I/O | `std::path::Path::new(canonical_id)` | OS handles both `/` and `\` on Windows |

#### Implementation Rules

1. **Receive → normalize immediately**: Every path entering the system passes through `canonicalize_id()` or `uri_to_canonical_id_from_str()` before storage or comparison
2. **Store only canonical**: All maps, caches, and analysis types use canonical IDs as keys
3. **Send → denormalize at the boundary**: Convert back to `file://` URIs or OS paths only when sending to external systems
4. **Never compare raw paths**: Always compare canonical IDs, never raw OS paths or URIs

### Rust Compiler Architecture (`crates/verter_core/src/`)

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
    ├── vapor/            # Vapor mode output (_template, _renderEffect, etc.)
    └── vapor2/           # Experimental: alternative Vapor codegen approach
tsx/                      # TSX template codegen (for LSP/TSGO type checking)
├── mod.rs                # generate_tsx_template() — Vue template → valid JSX
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

### Two Template Codegen Paths (CRITICAL)

The Rust compiler has **two separate template codegen paths**. Modifying one does NOT affect the other:

| Path | Module | Purpose | Output |
|------|--------|---------|--------|
| **VDOM/Vapor** | `template/code_gen/vdom/` | Runtime render functions for bundler output | `_createElementVNode(...)` calls |
| **TSX** | `tsx/template/` | Valid JSX for LSP/TSGO type checking | `<div prop={expr}>` JSX elements |

The **LSP uses the TSX path** via `host.ensure_compiled()` with `CompileTarget::IDE`. TSGO type-checks this TSX output. Changes to VDOM codegen do NOT affect LSP hover/completions.

### CompileTarget (Selective Pipeline)

`CompileTarget` (bitflags in `verter_core::compile::types`) controls which compilation steps run. Each consumer sets the flags it needs, skipping unnecessary work:

| Flag | Controls | Used By |
|------|----------|---------|
| `STYLE` | Style codegen (CSS scoping, modules, v-bind) | Bundler |
| `SCRIPT` | Script codegen (macro expansion, binding extraction) | Bundler, Analysis |
| `TEMPLATE` | Template VDOM/Vapor render function codegen | Bundler |
| `TSX` | TSX template codegen for type checking | LSP/IDE |
| `TSC` | TSC declaration file generation | TSC |
| `TEMPLATE_DATA` | Template data extraction (binding occurrences) | LSP, Analysis |

**Presets** (convenience combinations):

| Preset | Flags | Consumer |
|--------|-------|----------|
| `BUNDLER` | `STYLE \| SCRIPT \| TEMPLATE` | `@verter/unplugin`, default |
| `IDE` | `TSX` | LSP, TSGO |
| `ANALYSIS` | `SCRIPT \| TEMPLATE_DATA` | MCP analysis |

**Key API**: `VerterHost::ensure_compiled(canonical_id, profile)` compiles with the given profile's target without requiring a `VirtualNodeKind`. Used by LSP and MCP to populate the cache. `get_virtual_file()` still exists for retrieving specific virtual file outputs.

**Performance savings**: LSP skips style + script + VDOM template codegen (~40-60% faster). MCP skips style + VDOM codegen (~30-50% faster).

### Cached Directive Fields on ElementNode

The parser extracts structural directives from `el.props` via `prop.take()` and caches them as dedicated fields on `ElementNode` (`ast/types.rs`):

| Field | Directive | In `el.props`? | Notes |
|-------|-----------|----------------|-------|
| `v_condition` | `v-if`, `v-else-if`, `v-else` | **No** (taken) | Contains `ElementNodeCondition` with kind + prop |
| `v_for` | `v-for` | **No** (taken) | Contains the full `NodeProp` |
| `v_slot` | `v-slot`, `#name` | **No** (taken) | Contains the full `NodeProp` |
| `v_once` | `v-once` | **No** (taken) | Contains the full `NodeProp` |
| `v_ref` | `ref`, `:ref` | **No** (taken) | Contains the full `NodeProp` |

**Consequence**: Code iterating `el.props` will **never see** these directives. Both codegen paths must handle them explicitly. The TSX module removes `v-if/v-for/v-slot/v-once` attributes (they become JSX wrappers/removals) and converts `ref` to JSX expression syntax (`ref={"name"}`).

## Build

```bash
pnpm install                  # Install all dependencies
pnpm build                    # Build everything: native → lsp → wasm → ts packages
pnpm run build:native         # Build native .node bindings only
pnpm run build:lsp            # Build Rust LSP binary (debug)
pnpm run build:lsp:release    # Build Rust LSP binary (release, optimized)
pnpm run build:mcp            # Build MCP server binary (debug)
pnpm run build:mcp:release    # Build MCP server binary (release, optimized)
pnpm run build:wasm           # Build WASM + copy to playground
pnpm run build:ts             # Build all TypeScript packages
pnpm run build:playground     # Build the playground for deployment
```

`pnpm build` runs sequentially: native bindings first (needed by unplugin), then LSP binary (shares compiled Rust deps with native, avoids recompilation), then WASM (needed by playground), then all TS packages. This ensures F5 debugging in VS Code and `pnpm --filter @verter/playground dev` both work.

### Build Dependency Chain

When changing Rust code, you must rebuild downstream artifacts in order:

```
verter_core + verter_analysis + verter_host + verter_ffi (Rust crates)
    ↓ cargo build
verter_napi (NAPI-RS cdylib)    verter_lsp (LSP binary)    verter_wasm (wasm-bindgen cdylib)
    ↓ pnpm run build:native         ↓ pnpm run build:lsp       ↓ pnpm run build:wasm
@verter/native (.node binary)   verter-lsp (target/debug/)  @verter/wasm (WASM pkg)
    ↓                                ↓                          ↓
@verter/unplugin (bundler)      verter-vscode (F5/VSIX)     @verter/playground (browser)
    ↓
playground build (Vite)
    ↓
playground E2E tests
```

**Common rebuild sequences:**

| What changed | Rebuild commands (in order) |
|---|---|
| Rust crate (`verter_core`) | `pnpm run build:native` → rebuild any downstream consumer |
| Rust LSP (`verter_lsp`) | `pnpm run build:lsp` (or `build:lsp:release` for optimized) → restart VS Code extension host |
| Unplugin (`packages/unplugin`) | `pnpm run build:ts` (or just rebuild unplugin) |
| Playground after Rust/unplugin change | `pnpm run build:native` → `cd packages/playground && rm -rf dist node_modules/.vite && npx vite build` |
| WASM (for playground browser editor) | `pnpm run build:wasm` |
| Everything | `pnpm build` (runs native → lsp → wasm → ts in correct order) |

**Key details:**
- `@verter/unplugin` depends on `@verter/native` — compiles `.vue` files at build time via the Rust native binary
- `@verter/playground` uses `@verter/unplugin` (devDep) for its own Vue SFC compilation, and `@verter/wasm` (dep) for the in-browser editor
- The native binary lives in `packages/native/dist/` after `build:native`
- The LSP binary lives in `target/debug/verter-lsp` (or `target/release/verter-lsp` with `build:lsp:release`)
- Clear Vite cache (`node_modules/.vite`) when rebuilding playground after native changes

## Development

```bash
pnpm watch                    # Watch-build TS packages for extension dev
pnpm dev-extension            # Build LSP binary, then watch language-shared + vscode extension + typescript-plugin
pnpm clean                    # Remove build artifacts
```

## Profiling with MCP (for agents)

Use the real-world profiling example with hotpath instrumentation. Two pipeline modes are available:

```bash
# AST-only pipeline (tokenize → parse → OXC expressions):
pnpm run profile:hotpath          # Timing hotspots
pnpm run profile:hotpath:alloc    # Timing + allocation hotspots
pnpm run profile:hotpath:mcp      # Starts MCP endpoint at http://localhost:6771/mcp

# Full compile pipeline (tokenize → parse → style → script → template codegen):
pnpm run profile:hotpath:full          # Timing hotspots
pnpm run profile:hotpath:full:alloc    # Timing + allocation hotspots
pnpm run profile:hotpath:full:mcp      # Starts MCP endpoint at http://localhost:6771/mcp
```

The full pipeline exercises all instrumented functions across the compilation flow:
compile, generate_script, process_script_setup, process_macro_item, generate_style,
process_style, apply_scoped_normalized, parse_template_expressions, generate_template,
walk_template, apply_to, batch_overwrite, batch_prepend_left_static, build_string,
generate_map, generate_map_json, alloc_node, attach_to_parent.

Agent MCP config template is checked in at:

```text
mcp/hotpath.mcp.json
```

Point your MCP-capable agent to that file (or copy its `mcpServers` entry into your local MCP config).
For client-specific setup examples, see [mcp/README.md](mcp/README.md).

## Analysis MCP Server (`verter_mcp`)

The `verter-mcp` binary exposes Verter's full analysis, diagnostics, compilation, and scoring pipeline via MCP. It provides 33 tools for AI agents to deeply understand Vue codebases without reading files directly.

```bash
# Build
pnpm run build:mcp            # Debug build
pnpm run build:mcp:release    # Release build

# Run (stdio — agent spawns as child process)
verter-mcp --project-root /path/to/vue-project

# Run (HTTP — remote/shared access)
verter-mcp --transport http --project-root /path/to/vue-project
# Serves at http://localhost:6772/mcp
```

MCP config files are checked in at:
- `mcp/verter.mcp.json` (stdio)
- `mcp/verter-http.mcp.json` (HTTP)

For the full tool catalog and agent workflow guide, see [mcp/README.md](mcp/README.md).

### Key Architecture

- `VerterMcpServer` wraps `VerterHost` (with `AnalysisScope::LSP` for maximum analysis data), `Linter`, and `ActionEngine`
- Tools auto-load files from disk via `ensure_loaded()` — agents don't need to pre-load
- Template analysis requires a compilation pass — `ensure_template_analysis()` triggers it transparently
- Cross-file tools iterate all loaded files (no `ProjectIndex` exposed from host)
- Scoring engine computes composite 0-100 quality scores from a11y, lint, template complexity, API surface, CSS health, and reactivity dimensions

## Testing

### Running Tests

```bash
# TypeScript / JavaScript
pnpm test                                    # All JS/TS tests
pnpm vitest --run                            # All tests (non-watch)
pnpm vitest --run path/to/test.spec.ts       # Specific file

# Rust
cargo test --workspace --verbose             # All Rust tests
cargo test --package verter_core test_name   # Specific Rust test
cargo test --package verter_core 2>&1 | tail -60  # Full suite with truncated output
```

### End-of-change Checks

Run these after making changes:

```bash
cargo clippy --fix --allow-dirty --allow-staged --workspace -- -D warnings
cargo fmt --all
```

### Testing Requirements

**IMPORTANT — TDD (Test-Driven Development) is mandatory**:
1. **Write failing tests first** — before implementing any feature or fix, write one or more tests that demonstrate the expected behavior and verify they fail
2. **Implement the minimum code** to make the failing tests pass
3. **Refactor** if needed while keeping tests green

Coverage expectations:
- New features: Add tests covering the new functionality
- Bug fixes: Add tests that would have caught the bug
- Refactoring: Ensure existing tests pass and add tests for edge cases discovered
- Behavioral changes: Add tests verifying the new behavior

Tests serve as documentation of expected behavior and prevent regressions.

**IMPORTANT — Always include negative assertions**:

Every test must verify both what SHOULD be present AND what should NOT be present. A test that only checks for expected output can pass even when the output contains invalid/broken content alongside the expected content.

```rust
// GOOD: Both positive and negative assertions
let result = gen_tsx_template(r#"<template><div v-if="show">hello</div></template>"#);
assert!(result.contains("_ctx.show ?"), "should have ternary condition");     // positive
assert!(!result.contains("v-if"), "v-if attribute must be removed from JSX"); // negative

// BAD: Only positive assertion — passes even if v-if="show" leaks into output
let result = gen_tsx_template(r#"<template><div v-if="show">hello</div></template>"#);
assert!(result.contains("_ctx.show ?"), "should have ternary condition"); // not enough!
```

For codegen tests: always verify that removed/transformed Vue syntax does NOT appear in output. For type tests: always include both positive assertions and `@ts-expect-error` negative assertions to guard against `any`/`never`.

### Server Cleanup

**IMPORTANT**: After starting any dev server, preview server, or other long-running process for testing purposes, **always kill it when done**. This prevents stale servers from interfering with subsequent test runs (e.g., Playwright's `reuseExistingServer: true` will use a stale server serving old builds).

```bash
# After finishing with a server, kill it
# If started in background, use the process ID or port:
kill $(lsof -t -i:4173)   # Unix
taskkill //F //PID <pid>   # Windows

# Or if using pnpm/npm scripts, Ctrl+C the process
```

### Test Output Best Practices

When running E2E tests or test suites where you need to inspect output, **redirect output to a temp file first**, then grep/read the file. This avoids re-running expensive builds and tests just to search for different patterns:

```bash
# Good: capture once, search multiple times
pnpm exec playwright test --project=preview 2>&1 | tee /tmp/e2e-output.log
# Then search as needed:
grep -i "fail\|error" /tmp/e2e-output.log

# Bad: re-running the full test suite each time you need different output
pnpm exec playwright test --project=preview 2>&1 | grep "fail"
pnpm exec playwright test --project=preview 2>&1 | grep "error"  # wasteful re-run
```

### TypeScript Test Patterns

**Test locations**: Unit tests are co-located as `*.spec.ts` next to source files. Type tests in `packages/types/` use `vitest --typecheck`.

**AI-generated tests**: Add appropriate comments indicating AI assistance:

```typescript
// For new test files, add a JSDoc at the top:
/**
 * @ai-generated - This test file was generated with AI assistance.
 * Brief description of what the tests cover.
 */

// For individual tests in existing files:
// @ai-generated - Tests X functionality with Y scenarios
it("does something", () => { /* ... */ });
```

**Sourcemap testing** (see `macros.map.spec.ts`):
```typescript
const { s, source, result } = processMacrosForSourcemap(code);
const map = s.generateMap({ source: "test.vue" });
```

**Type testing best practices** (`packages/types/`):
- Always include **both** a positive assertion and a `@ts-expect-error` negative assertion
- This prevents `any`/`unknown`/`never` types from silently passing tests

```typescript
it("type is correctly inferred", () => {
  type Result = SomeTypeHelper<Input>;

  // Positive assertion - type matches expected
  assertType<Result>({} as ExpectedType);
  assertType<ExpectedType>({} as Result);

  // @ts-expect-error - Result is not any/unknown/never
  assertType<{ unrelated: true }>({} as Result);
});
```

### Rust Test Patterns

See [CLAUDE_IMPLEMENTATION_GUIDE.md](CLAUDE_IMPLEMENTATION_GUIDE.md) for detailed Rust testing patterns including:

- **TDD workflow** — write failing tests first, then implement
- **`gen_and_validate()`** — all codegen tests MUST validate JS syntax via oxc parser
- **AST comparison** — E2E tests compare against Vue's official compiler output

## TypeScript Code Patterns

**Defining script plugins:**
```typescript
import { definePlugin, ScriptContext } from "../../types";
export const MyPlugin = definePlugin({
  name: "my-plugin",
  enforce: "pre", // or "post"
  pre(s, ctx) { /* runs before transforms */ },
  transformFunctionCall(item, s, context) { /* transform specific type */ },
  transformDeclaration(item, s, context) { /* another type */ },
  post(s, context) { /* runs after all transforms */ }
});
```

**Type helper prefix convention:**
- Internal helpers use `___VERTER___` prefix (see `packages/core/`)
- String-exported types use `$V_` prefix for collision avoidance

**Parser types** (`packages/core/src/v5/parser/`):
- `ParsedBlockScript`, `ParsedBlockTemplate` - Block-specific parsed data
- `ScriptItem`, `ScriptTypes` - Categorized script AST items

## Key Files

| File | Purpose |
|------|---------|
| `packages/core/src/v5/parser/parser.ts` | Main SFC parser entry |
| `packages/core/src/v5/process/script/script.ts` | Script processing orchestration |
| `packages/core/src/v5/process/script/types.ts` | `definePlugin`, `ScriptContext`, `ScriptPlugin` |
| `packages/core/src/v5/process/script/plugins/macros/macros.ts` | Vue macro transformations |
| `crates/verter_lsp/src/main.rs` | LSP binary entry point (stdio transport, CLI args) |
| `crates/verter_lsp/src/server.rs` | LSP message loop, request dispatch, feature routing |
| `crates/verter_mcp/src/main.rs` | MCP binary entry point (stdio + HTTP transport) |
| `crates/verter_mcp/src/server.rs` | MCP tool router: 33 tools for analysis, diagnostics, scoring |
| `crates/verter_mcp/src/tools/scoring.rs` | Quality/a11y scoring engine (0-100 composite scores) |
| `packages/types/src/helpers/helpers.ts` | Core type utilities |
| `crates/verter_core/src/compile.rs` | Pipeline orchestrator (tokenize → parse → style → script → template) |
| `crates/verter_core/src/parser/mod.rs` | SFC parser: tokenizer events → root nodes + template AST |
| `crates/verter_core/src/ast/types.rs` | AstNode, ElementNode, NodeId, PropFlags |
| `crates/verter_core/src/script/macros.rs` | defineProps/Emits/Model/Slots/Expose/Options |
| `crates/verter_core/src/script/process.rs` | Script setup processing, companion script merging |
| `crates/verter_core/src/template/code_gen/mod.rs` | Template codegen entry point |
| `crates/verter_core/src/template/code_gen/walker.rs` | DFS tree walker (shared by VDOM/Vapor backends) |
| `crates/verter_core/src/template/code_gen/binding.rs` | BindingResolver (_ctx./$setup. prefix resolution) |
| `crates/verter_core/src/template/code_gen/vdom/` | VDOM render function codegen |
| `crates/verter_core/src/template/code_gen/vapor/` | Vapor mode codegen |
| `crates/verter_core/src/tsx/template/mod.rs` | TSX template codegen: Vue → JSX (used by LSP/TSGO) |
| `crates/verter_core/src/tsx/template/directives.rs` | TSX: v-if → ternary, v-for → .map(), v-show → style |
| `crates/verter_core/src/tsx/template/props.rs` | TSX: :prop → prop={}, @event → onEvent={} |
| `crates/verter_core/src/css/` | CSS preprocessing and style transformation |
| `crates/verter_core/src/code_transform/code_transform.rs` | Chunk-based deferred mutation engine |
| `crates/verter_analysis/src/lib.rs` | Static analysis entry: imports, exports, bindings |
| `crates/verter_analysis/src/style.rs` | CSS scanner, selector parser, specificity computation |
| `crates/verter_analysis/src/selector_match.rs` | Three-valued CSS selector matching against template elements |
| `crates/verter_analysis/src/template.rs` | Template element analysis, component usage, dynamic class extraction |
| `crates/verter_host/src/lib.rs` | Host entry: compile, cache, upsert, dependency tracking |
| `crates/verter_ffi/src/lib.rs` | FFI types shared between NAPI and WASM |
| `packages/unplugin/src/index.ts` | Unplugin factory: `buildStart` (preCompile), `transform`, `load` hooks |
| `packages/unplugin/src/core/types.ts` | `VerterPluginOptions`, `HmrStrategy` |
| `packages/unplugin/src/core/scanner.ts` | `scanVueFiles()` — async recursive directory walker for preCompile |
| `packages/unplugin/src/core/compiler.ts` | Host singleton, `generateComponentId`, `processStyle` |

## Rust Performance

See [.claude/performance-guide.md](.claude/performance-guide.md) for Rust performance patterns including:

- **Batch over incremental** — collect mutations, apply in single O(n+m) passes
- **Allocation hierarchy** — `&'static str` > bump `&'alloc str` > `&str` > reusable buffer > `String`
- **Reusable buffer** — `std::mem::take` pattern to thread a single `String` through processing
- **Object pooling** — recycle structs with `.clear()` to retain Vec capacities
- **Reduce work** — skip expensive operations for trivial cases, cache repeated computations

## Dependencies Policy

- Keep dependencies at their latest versions
- Rust deps: update in `Cargo.toml`, run `cargo update`
- JS deps: `pnpm up -r -i -L` to interactively update all
- `workspace:^` deps are rewritten by `pnpm publish` automatically

## Commit Convention

This project uses **conventional commits** for automatic changelog generation via [git-cliff](https://git-cliff.org/).

```
<type>(<scope>): <description>

Types:
  feat     - New feature
  fix      - Bug fix
  perf     - Performance improvement
  refactor - Code refactoring (no behavior change)
  docs     - Documentation only
  test     - Adding/updating tests
  chore    - Build, CI, tooling changes
  release  - Version bump and release

Scopes:
  core     - verter_core Rust crate
  napi     - verter_napi / @verter/native
  wasm     - verter_wasm / @verter/wasm
  play     - playground
  unplugin - @verter/unplugin
  lsp      - language-server
  types    - @verter/types
  ts       - @verter/core (TypeScript)
  ci       - CI/CD workflows
  *        - multiple areas

Examples:
  feat(core): add v-memo directive support
  fix(wasm): correct memory leak in compile()
  chore(ci): add nightly WASM build workflow
  release(all): v0.0.1-alpha.1
```

## CI/CD

See [.claude/ci-cd.md](.claude/ci-cd.md) for detailed CI/CD documentation including:

- Workflow specifications (CI, nightly, release)
- Pre-release versioning flow (alpha → beta → rc → stable)
- Publishing process (npm + crates.io)
- Nightly WASM builds and playground deployment
- Required GitHub secrets configuration
