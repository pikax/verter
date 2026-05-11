---
name: architecture
description: "Verter codebase architecture: high-level module map, TypeScript packages, plugin system, CSS analysis, MCP server, static analysis types"
---

# Verter Architecture Reference

For domain-specific detail, see: `/type-resolution`, `/type-cache-architecture`, `/component-meta`, `/compiler-codegen`, `/host-session`.

## Shared Substrate Principle

Verter is designed as one shared optimized codebase. Consumers should reuse the same lower-level crates instead of growing separate semantic pipelines.

- Put reusable parsing, analysis, type-resolution, caching, and import-following behavior in the shared owner crate.
- `verter_session` is the shared host/session/cache boundary for host-backed consumers.
- `verter_semantic` and `verter_compiler` own reusable semantics, lowering, and codegen.
- `verter_session::resolver_core` owns the host-backed resolver stack and type-resolution orchestration. Resolver-path methods receive `ctx: &dyn ResolverContext` (sealed super-trait at `resolver_core/resolver_context.rs`) — only `VerterHost` implements it, enforced by the `no_concrete_verter_host_in_seal_scope` arch-guard.
- `verter_protocol` owns transport-facing schema DTOs, while `verter_ffi` stays a thin native/WASM adapter layer.
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

### Adding a New TS Plugin

1. Create plugin file in `packages/core/src/v5/process/script/plugins/`
2. Implement `ScriptPlugin` interface using `definePlugin`
3. Register in `packages/core/src/v5/process/script/plugins/index.ts`
4. Add tests

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

## CSS Analysis & Selector Matching (`crates/verter_semantic/src/analysis/`)

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

## Analysis MCP Server (`verter_mcp`)

The `verter-mcp` binary exposes Verter's full analysis, diagnostics, compilation, and scoring pipeline via MCP. It provides tools for AI agents.

### Key Architecture

- `VerterMcpServer` wraps `VerterHost` (with `AnalysisScope::LSP` for maximum analysis data), `Linter`, and `ActionEngine`
- Tools auto-load files from disk via `ensure_loaded()` — agents don't need to pre-load
- Template analysis requires a compilation pass — `ensure_template_analysis()` triggers it transparently
- Cross-file tools iterate all loaded files (no `ProjectIndex` exposed from host)
- Scoring engine computes composite 0-100 quality scores from a11y, lint, template complexity, API surface, CSS health, and reactivity dimensions

## verter_semantic::analysis — Static Analysis Types

`verter_semantic::analysis` is the shared static-analysis surface consumed by `verter_session`, diagnostics, and tooling. The compilation crate still owns lowering and codegen, while `verter_session` projects compiler and workspace state into these semantic snapshots.

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

Populated after compilation by converting `RawTemplateData` from `verter_compiler`.

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
verter_compiler::compile()
    |-- ScriptAnalysisSnapshot (from OXC parse during compilation)
    |-- RawTemplateData (spans, binding refs, component tags)
    |-- CssParsed* (v-bind spans, pseudo spans)
    |
    v
verter_session (conversion layer)
    |-- RawTemplateData --> TemplateAnalysisSnapshot
    |-- CssParsed*      --> StyleBlockAnalysis
    |-- Resolves import paths, populates resolved_canonical_id
    |-- Updates ProjectIndex with file usage
    |
    v
Consumers (LSP, build, linter) query snapshots + ProjectIndex
```
