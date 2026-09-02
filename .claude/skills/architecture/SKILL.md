---
name: architecture
description: "Verter codebase architecture: high-level module map, TypeScript packages, plugin system, CSS analysis, MCP server, static analysis types"
---

# Verter Architecture Reference

For domain-specific detail, see: `/type-resolution`, `/type-cache-architecture`, `/component-meta`, `/compiler-codegen`, `/host-session`.

## Shared Substrate Principle

Verter is one shared optimized codebase. Consumers reuse lower-level crates instead of separate semantic pipelines.

- Put reusable parsing, analysis, type-resolution, caching, and import-following behavior in the shared owner crate.
- `verter_language` is the zero-dependency leaf routing authority: `FileLanguage`, `FrameworkAdapterId`, `LanguageId`, `CapabilityId`, and the pure static `LanguageRegistry` (`classify_static(path)` — never reads project config). Host-gated classification (static registry × `ProjectCapabilitySnapshot`) is owned by `verter_session::framework::HostLanguageClassifier`; scheduler/workspace consumers reach it only through session-implemented trait objects (`SourceLoader::classify` / `WorkspaceAccess::classify_file`). The crate is a `verter_span`-only leaf (its design allowance — spans for the parse-artifact regions; strings stay crate-interned) and keeps a crate-local id-intern table: no lower crate exposes a reusable interning facility, and the id set is bounded by registered languages. It also owns the framework-neutral parse payload: `FrameworkParseArtifact` (typed `FrameworkParseCommon` — `ScriptRegion { span, source_type, kind }` / template / style regions, external links, `LanguageDiagnostic`s — plus a PRIVATE erased `Arc<dyn CarrierParse>`), with the raw downcast confined to each adapter's own bridge module (no capability token — a foreign artifact's erased payload is a DIFFERENT concrete `CarrierParse` type, so the `Any` downcast already fails structurally for it); the session's blessed accessors are `FrameworkAdapterCtx::carrier_for::<T>` (routed through each adapter's registered-projector opener, e.g. `open_vue_carrier`) and the Vue adapter's `vue_parse()`; the concrete `VueParseCarrier` + Vue producer live in `verter_compiler::framework_common::vue_bridge`.
- `verter_session` is the shared host/session/cache boundary for host-backed consumers.
- `verter_semantic` and `verter_compiler` own reusable semantics, lowering, and codegen.
- `verter_session::resolver_core` owns the host-backed resolver stack and type-resolution orchestration. Resolver-path methods receive `ctx: &dyn ResolverContext` (sealed super-trait at `resolver_core/resolver_context.rs`) — only `VerterHost` implements it, enforced by the `no_concrete_verter_host_in_seal_scope` arch-guard.
- `verter_protocol` owns transport-facing schema DTOs; `verter_ffi` stays a thin native/WASM adapter layer.
- Consumer packages and apps stay adapter-oriented: thin wrappers, public API shaping, transport glue, UX-specific behavior.

Bug or slowdown in one surface → fix in shared substrate so other consumers benefit.

## TypeScript Packages

| Package | Purpose | Entry Point |
| ------- | ------- | ----------- |
| **`@verter/types`** | TypeScript utility types (`PatchHidden`, `ExtractHidden`, `EmitsToProps`, etc.). Has `/string` export with `$V_` prefixed types for LSP injection | `src/index.ts` |
| **`@verter/language-shared`** | Shared custom protocol types between VS Code client and Rust LSP binary | `src/index.ts` |
| **`@verter/typescript-plugin`** | TypeScript plugin resolving `.vue` imports in TS/JS files. Intercepts module resolution to return transformed TSX | `src/index.ts` |
| **`verter-vscode`** | VS Code extension. Launches Rust `verter-lsp` binary over stdio, bundles TS plugin, handles extension activation | `src/extension.ts` |
| **`@verter/unplugin`** | Universal bundler plugin (Vite, Rollup, webpack, esbuild, rspack, Rolldown, Farm). Compiles `.vue` files via `@verter/native`. Supports `preCompile` for build-start cache warming | `src/index.ts` |

## Unplugin Configuration (`packages/unplugin/`)

`@verter/unplugin` provides a `VerterPluginOptions` interface:

| Option | Type | Default | Description |
| ------ | ---- | ------- | ----------- |
| `componentId` | `(filename, source, isProd) => string` | hash-based | Custom component ID generator |
| `include` | `string \| RegExp \| (string \| RegExp)[]` | `[/\.vue$/]` | File patterns to include |
| `preCompile` | `boolean` | `false` | Pre-compile all `.vue` files during `buildStart`. Scans project root, upserts files into host cache (including type dependencies for macros), and compiles them. When `transform()` later receives same content, host returns cached result instantly. `node_modules` excluded from scanning. |
| `crossFileOptimize` | `boolean` | `false` | Cross-file prop constness optimization. Requires `preCompile: true`. After pre-compilation, analyzes render tree to determine which props are always passed constant values, skipping dynamic tracking in compiled output. |
| `template` | `object` | — | Template compiler options (compat with `@vitejs/plugin-vue`) |

**`preCompile` architecture:** During `buildStart()`, scans project root for `.vue` files (excluding `node_modules` and dot-directories). For each file: upserts into host, resolves external `src` attributes and macro type dependencies (e.g., `import type { Props } from './types'` used in `defineProps<Props>()`), then triggers compilation. When another plugin modifies the file before `transform()`, host detects content change via internal hashing and recompiles. Third-party `.vue` files in `node_modules` compile on-demand during `transform()` — no pre-compilation overhead.

**Macro type resolution invariant:** cross-file macro type resolution must only follow imports reachable from the requested type's local declaration graph. Unrelated imports in the same file are out of scope; plain imports are not implicit re-exports.

## CSS Analysis & Selector Matching (`crates/verter_semantic/src/analysis/`)

`verter_css_syntax` is the shared lossless token/event authority for CSS, SCSS, indented Sass, Less, and Stylus. `StyleSyntaxIrSink` and `LosslessCstSink` are peers over the same parser event stream. Semantic style analysis projects only complete, static selector nodes into selectors, classes, IDs, custom properties, and at-rules; interpolation, recovery, and evaluation-dependent selectors fail closed. Each `AnalyzedCssClass` carries `selector_index` (exact class → comma-part selector join) and each `AnalyzedSelector` carries `rule_body_span` (brace- or indentation-delimited body span). Vue's planner separately consumes trusted IR for authored-dialect `v-bind()` and post-preprocess plain-CSS module hashing/scoping; Svelte consumes the IR as a trust gate for its distinct plain-CSS matcher/scoper. Svelte's carrier/CSS parser remains the compatibility owner until exact Svelte 5.56.10 error-code, offset, and read-past-close parity is proven. Style `v-bind()` usage is discovered through the same dialect-aware planner IR, then OXC-derived `expr_roots`/`roots_complete` remain the liveness facts consumed by `mark_bindings_used_in_style` and compile-input assembly.

`StyleSyntaxIr` retains positioned containment and balanced values without evaluating or compiling preprocessors. Imports, modules, plugins, guards, mixin/function arguments, and control expressions remain opaque-but-positioned. `StyleSyntaxIr::dependencies()` is the parse-minted inclusion inventory (`@import`/`@use`/`@forward`/`@plugin`, in source order, each with its keyword span and quote/`url()`-stripped specifier span), so no consumer walks the at-rules again to find out which sheets a block pulls in; `dependency_pulls_in_unparsed_bytes` is the same owner's answer to whether an inclusion actually brings foreign bytes with it (Sass's built-in `sass:` modules do not).

`verter_css_syntax::stage` owns the identity every style consumer shares — `StyleStage`, `StyleProducer`, `StyleDiagnostic`, `StyleDependency`, `QualifiedStyleResult`, and the `PreprocessedStyle` admission witness — plus the single `lang="…"` spelling → `CssDialect` mapping (`CssDialect::from_lang`, byte-exact) and the single "is this already CSS" predicate (`CssDialect::requires_external_preprocessing`). Style bytes leave the compiler as a `QualifiedStyleResult` and enter it as a `PreprocessedStyle` carrying its producer, never as a bare string. The carrier parser's `StyleLang::from_bytes` and the carrier-level `StyleDialect` are the deliberately larger universe (they also name `postcss`, plus an unrecognised state), owned by the carrier parse projection and read by the consumers that serve a dialect the rewrite pipeline refuses. See `/compiler-codegen` → "Stage-Qualified Style Identity".

Stylesheet parser mode is deterministic by dialect and structural tokens. CSS always uses brace grammar. Sass and Stylus use the layout-capable grammar, which also recognizes explicit braced blocks. SCSS and Less use brace grammar whenever the lexer emits any plain `LeftBrace`; only a brace-free source with an actual deeper-indented line pair uses layout grammar. Closing-brace indentation and other incidental formatting never select the parser. Selector trust folds every component descendant and functional-pseudo selector list; class/ID collection descends those lists and gates each component independently, so complete literal class components may still publish from an otherwise evaluation-dependent selector such as `&.active` or `:global(.a .#{$x})`. A textually certain `:deep`/`:global`/`:slotted` kind publishes independently of argument trust, while every class inside its argument remains subject to the same per-component gate. Ambiguous optional-syntax statements remain locally typed and diagnosed without recovering intact ancestor rules. A declaration may own a retained `StyleBlock` (for example, an indented Sass nested-property namespace); the IR sink never discards such a block.

**Module structure:**

```
style.rs              # Semantic style projection types and specificity computation
style_syntax.rs       # Five-dialect syntax-to-semantic projection
selector_match.rs     # Three-valued selector matching against template elements
template.rs           # Template element analysis, dynamic class extraction, :style CSS var extraction
```

**Key types:**

| Type | Location | Purpose |
| ---- | -------- | ------- |
| `StructuredSelector` | `style.rs` | Parsed CSS selector (compounds + combinators) |
| `CompoundSelector` | `style.rs` | Single compound: element, classes, id, attributes, pseudo-classes |
| `SelectorCombinator` | `style.rs` | Descendant / Child / NextSibling / LaterSibling |
| `MatchResult` | `selector_match.rs` | Three-valued: `Matches`, `MaybeMatches`, `NoMatch` |
| `DomQueryCallSite` | `types.rs` | DOM query call with parsed selector and spans |
| `StyleBlockAnalysis` | `style.rs` | Per-`<style>` block analysis with nested `CssAnalysis` |
| `AnalyzedCustomProperty` | `style.rs` | CSS custom property with name/value spans, var references, selector index |
| `CssVarReference` | `style.rs` | `var()` call with name, span, optional fallback (recursive) |
| `AnalyzedVarUsage` | `style.rs` | Regular CSS property using `var()` with property name and selector index |
| `CssVarManipulation` | `types.rs` | Script-side CSS variable manipulation via DOM APIs |
| `DynamicStyleVar` | `template.rs` | CSS variable set via `:style` binding in template |
| `StaticStyleVar` | `template.rs` | CSS variable set via static `style` attribute in template |
| `CssVarFlow` | `project_index.rs` | Cross-component CSS variable flow (definitions + usages + manipulations) |

**CSS Variable Analysis (three-block tracking):**

- **Style**: the balanced component-value IR projects `AnalyzedCustomProperty` (definitions with values/spans) and `AnalyzedVarUsage` (var() references). `extract_var_references()` handles nested var() fallbacks without evaluating values.
- **Template**: `extract_dynamic_style_vars()` extracts CSS vars from `:style="{ '--color': val }"`. `extract_static_style_vars()` extracts from `style="--color: red"`.
- **Script**: `try_extract_css_var_manipulation()` detects `el.style.setProperty('--x', val)`, `getPropertyValue('--x')`, `removeProperty('--x')`.
- **Cross-component**: `ProjectIndex.css_var_flow(name)` and `VerterHost.css_var_flow(name)` return `CssVarFlow` with all files defining/referencing/manipulating a variable.

**Selector matching algorithm** (`match_selector()`):

1. Match rightmost compound against target element
2. Walk left through combinators: `Child` checks `parent_index`, `Descendant` walks ancestor chain
3. Dynamic `:class` or component types → `MaybeMatches` (can't determine statically)
4. `:not()` inverts, `:is()`/`:where()` takes best match across alternatives

**Position encoding for CSS spans**: `CssAnalysis` spans (classes, IDs, selectors) are **SFC-absolute byte offsets**. `CssSource` is constructed with the style content origin, so syntax and projection spans are absolute from creation. Consumers use spans directly without adding any offset. `StyleBlockAnalysis.content_offset` is retained for documentation and slice operations.

## Analysis MCP Server (`verter_mcp`)

`verter-mcp` binary exposes Verter's full analysis, diagnostics, compilation, and scoring pipeline via MCP for AI agents. `VerterMcpServer` wraps `VerterHost` (with `AnalysisScope::LSP`), `Linter`, and `ActionEngine`. Tools auto-load via `ensure_loaded()`; template analysis triggers `ensure_template_analysis()` transparently. Cross-file tools iterate all loaded files (no `ProjectIndex` exposed from host). Scoring engine computes composite 0-100 quality scores from a11y, lint, template complexity, API surface, CSS health, and reactivity dimensions.

Both binaries (`verter-mcp`, and the LSP-decoupling twin `verter-mcp-server`) run the one shared entry body `verter_mcp::run::run`. The HTTP transport binds before its initial scan and announces the real bound port as a one-line stdout readiness record (`verter_mcp::readiness`, mirrored by `packages/vue-vscode/src/mcpServer.ts`); `--client-pid` arms the same `ClientProcessGuard` containment as the LSP. The VS Code extension spawns this standalone binary per LSP start attempt (`createMcpServerLifecycle`: awaited replacement, bounded crash respawn, provider-registration teardown on death) — see `docs/contributing/lsp-mcp-decoupling.md`.

## verter_semantic::analysis — Static Analysis Types

`verter_semantic::analysis` is the shared static-analysis surface consumed by `verter_session`, diagnostics, and tooling. Compilation crate owns lowering and codegen; `verter_session` projects compiler and workspace state into these semantic snapshots.

### AnalysisScope

Bitflags (`u32`) controlling which analysis passes run during file upsert.

**Script (bits 0-7)**

| Flag | Bit | Description |
| ---- | --- | ----------- |
| `IMPORTS` | 0 | Import declarations |
| `BINDINGS` | 1 | Variable/function/class declarations |
| `REACTIVITY` | 2 | Ref/reactive/computed classification |
| `MACROS` | 3 | defineProps/Emits/Model/Slots/Expose |
| `MACRO_TYPE_DEPS` | 4 | Cross-file type references in macros |
| `VUE_API_USAGE` | 5 | Track provide/inject/lifecycle/watcher calls |
| `EXPORT_SIGNATURES` | 6 | Per-export hashes for smart invalidation |
| `FUNC_RETURNS` | 7 | Analyze function return reactivity (for composables) |

**Template (bits 8-15)**

| Flag | Bit | Description |
| ---- | --- | ----------- |
| `TPL_COMPONENTS` | 8 | Component usages + prop expressions |
| `TPL_BINDINGS` | 9 | Which script bindings are used in template |
| `TPL_SLOTS` | 10 | Slot definitions + usages |
| `TPL_REFS` | 11 | Template ref attributes |
| `TPL_EVENTS` | 12 | Event handler bindings |
| `TPL_CONSTNESS` | 13 | Prop constness classification |

**Style (bits 16-19)**

| Flag | Bit | Description |
| ---- | --- | ----------- |
| `STYLE_CSS` | 16 | Full CSS analysis (selectors, classes, IDs) |
| `STYLE_VBIND` | 17 | v-bind() in styles |
| `STYLE_SCOPED` | 18 | Scoped/module metadata |
| `STYLE_PSEUDOS` | 19 | :deep/:global/:slotted |

**Cross-file (bits 24-26)**

| Flag | Bit | Description |
| ---- | --- | ----------- |
| `CROSS_RENDER_TREE` | 24 | Build render tree from template analysis |
| `CROSS_PROVIDE` | 25 | Provide/inject chain validation |
| `CROSS_PROP_CONST` | 26 | Prop constness optimization |

**Presets:**

| Preset | Flags | Use Case |
| ------ | ----- | -------- |
| `BUILD` | IMPORTS, BINDINGS, MACROS, MACRO_TYPE_DEPS, EXPORT_SIGNATURES, STYLE_VBIND, STYLE_SCOPED | Minimal overhead for compilation + smart invalidation |
| `BUILD_OPTIMIZED` | BUILD + REACTIVITY, VUE_API_USAGE, TPL_COMPONENTS, TPL_BINDINGS, TPL_CONSTNESS, CROSS_RENDER_TREE, CROSS_PROVIDE, CROSS_PROP_CONST | Build with cross-file optimization |
| `LSP` | All flags | Full analysis for completions, hover, diagnostics |
| `LINTER` | IMPORTS, BINDINGS, REACTIVITY, MACROS, VUE_API_USAGE, TPL_COMPONENTS, TPL_BINDINGS, TPL_SLOTS, TPL_REFS, TPL_EVENTS | Script + template for lint rules |
| `ESSENTIAL` | IMPORTS, BINDINGS, MACROS, MACRO_TYPE_DEPS, EXPORT_SIGNATURES | Script-only (legacy compat) |

### ScriptAnalysisSnapshot

Primary output of `build_script_analysis()`. Produced by a single OXC parse + AST walk.

| Field | Type | Description |
| ----- | ---- | ----------- |
| `imports` | `Vec<AnalyzedImport>` | All import declarations with source, bindings, spans |
| `bindings` | `Vec<AnalyzedBinding>` | Top-level variable/function/class declarations |
| `macros` | `Vec<AnalyzedMacro>` | Vue macro calls (defineProps, defineEmits, etc.) |
| `macro_type_deps` | `Vec<MacroTypeDep>` | Cross-file type references used by macros, tiered by structural position (`usage: MacroTypeDepUsage` — `Surface` = argument root / intersection-union arms / extends heritage / alias chains, missing ⇒ error; `Member` = top-level member annotation, missing ⇒ warning + `null` degrade). References nested deeper are never collected (runtime codegen does not need them) |
| `flags` | `AnalysisFlags` | Bitwise flags for O(1) queries |
| `exported_functions` | `Vec<AnalyzedExportedFunction>` | Non-SFC exported functions (composable analysis). Carries `name` / `is_default` / `params` / `is_async` / `composable` only. It carries NO return-type field: the declared return type's reactive-wrapper identity is a resolution decision answered at demand time from the lowered typed IR plus a package-backed route proof (`/type-resolution` → Reactive-wrapper demand), never from annotation text on this DTO |

**ReactivityKind**: None | Ref | Computed | Reactive | MaybeRef | Mutable

### TemplateAnalysisSnapshot

Populated after compilation by converting `RawTemplateData` from `verter_compiler`.

| Field | Type | Description |
| ----- | ---- | ----------- |
| `components` | `Vec<TemplateComponentUsage>` | Components used in template with props and slots |
| `binding_occurrences` | `Vec<TemplateBindingOccurrence>` | Script bindings referenced in template with spans |
| `defined_slots` | `Vec<DefinedSlot>` | `<slot>` elements defined in template |
| `template_refs` | `Vec<TemplateRef>` | `ref="foo"` attributes |
| `event_handlers` | `Vec<TemplateEventHandler>` | `@click`, `@input`, etc. |
| `elements` | `Vec<TemplateElement>` | Full element tree for linter traversal |
| `if_chains` | `Vec<IfChain>` | v-if/v-else-if chains for duplicate detection |
| `prop_definitions` | `Vec<AnalyzedPropDefinition>` | Props from defineProps |
| `emit_definitions` | `Vec<AnalyzedEmitDefinition>` | Emits from defineEmits |
| `comment_directives` | `Vec<CommentDirective>` | `@verter:disable`, `@verter:todo`, etc. |

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
