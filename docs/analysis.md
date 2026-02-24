# verter_analysis

Static analysis crate for Vue Single File Components. Provides import/export extraction, Vue API classification, binding analysis, template analysis, style analysis, and a project-wide component/dependency index.

`verter_analysis` is independent from `verter_core`. The compilation crate (`verter_core`) produces `RawTemplateData` during compilation, which `verter_host` converts into `verter_analysis` types. This separation keeps analysis types reusable across consumers (build tools, LSP, linters) without pulling in the full compiler.

## AnalysisScope

Bitflags (`u32`) controlling which analysis passes run during file upsert. Different consumers request exactly the analysis depth they need.

### Individual Flags

**Script (bits 0-7)**

| Flag | Bit | Description |
|------|-----|-------------|
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
|------|-----|-------------|
| `TPL_COMPONENTS` | 8 | Component usages + prop expressions |
| `TPL_BINDINGS` | 9 | Which script bindings are used in template |
| `TPL_SLOTS` | 10 | Slot definitions + usages |
| `TPL_REFS` | 11 | Template ref attributes |
| `TPL_EVENTS` | 12 | Event handler bindings |
| `TPL_CONSTNESS` | 13 | Prop constness classification |

**Style (bits 16-19)**

| Flag | Bit | Description |
|------|-----|-------------|
| `STYLE_CSS` | 16 | Full CSS analysis (selectors, classes, IDs) |
| `STYLE_VBIND` | 17 | v-bind() in styles |
| `STYLE_SCOPED` | 18 | Scoped/module metadata |
| `STYLE_PSEUDOS` | 19 | :deep/:global/:slotted |

**Cross-file (bits 24-26)**

| Flag | Bit | Description |
|------|-----|-------------|
| `CROSS_RENDER_TREE` | 24 | Build render tree from template analysis |
| `CROSS_PROVIDE` | 25 | Provide/inject chain validation |
| `CROSS_PROP_CONST` | 26 | Prop constness optimization |

### Presets

| Preset | Flags | Use Case |
|--------|-------|----------|
| `BUILD` | IMPORTS, BINDINGS, MACROS, MACRO_TYPE_DEPS, EXPORT_SIGNATURES, STYLE_VBIND, STYLE_SCOPED | Minimal overhead for compilation + smart invalidation |
| `BUILD_OPTIMIZED` | BUILD + REACTIVITY, VUE_API_USAGE, TPL_COMPONENTS, TPL_BINDINGS, TPL_CONSTNESS, CROSS_RENDER_TREE, CROSS_PROVIDE, CROSS_PROP_CONST | Build with cross-file optimization |
| `LSP` | All flags | Full analysis for completions, hover, diagnostics |
| `LINTER` | IMPORTS, BINDINGS, REACTIVITY, MACROS, VUE_API_USAGE, TPL_COMPONENTS, TPL_BINDINGS, TPL_SLOTS, TPL_REFS, TPL_EVENTS | Script + template for lint rules |
| `ESSENTIAL` | IMPORTS, BINDINGS, MACROS, MACRO_TYPE_DEPS, EXPORT_SIGNATURES | Script-only (legacy compat) |
| `NONE` | Empty | Tokenization and hashing only |

### Guard Methods

- `needs_script_analysis()` -- true if any script flag is set
- `needs_style_analysis()` -- true if any style flag is set
- `needs_full_css_analysis()` -- true only if `STYLE_CSS` is set (expensive lightningcss parse)
- `needs_template_analysis()` -- true if any template flag is set
- `needs_cross_file_analysis()` -- true if any cross-file flag is set

## Script Analysis

### ScriptAnalysisSnapshot

The primary output of `build_script_analysis()`. Produced by a single OXC parse + AST walk.

| Field | Type | Description |
|-------|------|-------------|
| `imports` | `Vec<AnalyzedImport>` | All import declarations with source, bindings, spans |
| `bindings` | `Vec<AnalyzedBinding>` | Top-level variable/function/class declarations |
| `macros` | `Vec<AnalyzedMacro>` | Vue macro calls (defineProps, defineEmits, etc.) |
| `macro_type_deps` | `Vec<MacroTypeDep>` | Cross-file type references used by macros |
| `flags` | `AnalysisFlags` | Bitwise flags for O(1) queries (HAS_DEFINE_PROPS, ASYNC_SETUP, etc.) |
| `exported_functions` | `Vec<AnalyzedExportedFunction>` | Non-SFC exported functions (composable analysis) |
| `type_enhancements` | `Option<ScriptTypeEnhancements>` | Placeholder for external type provider (TSGO) |

### Key Types

**AnalyzedBinding**: A top-level declaration with `name`, `kind` (Const/Let/Var/Function/AsyncFunction/Class), `reactivity_kind`, optional `type_annotation`, `initializer`, and byte spans.

**ReactivityKind**: Classification of a binding's reactivity behavior.

| Variant | Meaning |
|---------|---------|
| `None` | Not reactive (plain const, function, class) |
| `Ref` | Ref-like: `ref()`, `shallowRef()`, `toRef()` -- needs `.value` |
| `Computed` | `computed()` -- needs `.value`, read-only |
| `Reactive` | `reactive()`, `shallowReactive()` -- direct property access |
| `MaybeRef` | Composable return -- may or may not be ref |
| `Mutable` | `let` binding -- reassignable |

**AnalyzedExportedFunction**: For non-SFC files (composables, utility modules). Includes `params`, `return_type_annotation`, `return_reactivity`, `is_async`, and optional `ComposableInfo` for `useXxx` functions.

**ComposableInfo**: Tracks lifecycle hooks called, provide/inject usage, internal reactive state, and return shape (`Single`, `Object`, `Tuple`, or `Unknown`).

## Template Analysis

### TemplateAnalysisSnapshot

Populated after compilation by converting `RawTemplateData` from `verter_core`.

| Field | Type | Description |
|-------|------|-------------|
| `components` | `Vec<TemplateComponentUsage>` | Components used in template with props and slots |
| `binding_occurrences` | `Vec<TemplateBindingOccurrence>` | Script bindings referenced in template with spans |
| `unresolved_bindings` | `Vec<UnresolvedBinding>` | Bindings referenced but not found in script |
| `defined_slots` | `Vec<DefinedSlot>` | `<slot>` elements defined in template |
| `template_refs` | `Vec<TemplateRef>` | `ref="foo"` attributes |
| `event_handlers` | `Vec<TemplateEventHandler>` | `@click`, `@input`, etc. |
| `elements` | `Vec<TemplateElement>` | Full element tree for linter traversal |
| `if_chains` | `Vec<IfChain>` | v-if/v-else-if chains for duplicate detection |
| `prop_definitions` | `Vec<AnalyzedPropDefinition>` | Props from defineProps |
| `emit_definitions` | `Vec<AnalyzedEmitDefinition>` | Emits from defineEmits |
| `comment_directives` | `Vec<CommentDirective>` | `@verter:disable`, `@verter:todo`, etc. |

### Key Types

**TemplateComponentUsage**: Component tag name (PascalCase), import source, `is_dynamic`, props with constness, spread detection, slots used, and byte spans.

**TemplateBindingOccurrence**: Binding `name` + `span_start`/`span_end` + `usage_kind`. Enables textDocument/references, rename, and documentHighlight in the LSP.

**BindingUsageKind**: Interpolation, DirectiveValue, EventHandler, ComponentTag, TemplateRef, IteratorSource.

**PropValueConstness**: Const (compile-time constant), Dynamic (potentially reactive), Unknown (cannot analyze).

## Style Analysis

### StyleBlockAnalysis

Complete analysis of a single `<style>` block.

- **Vue features** (all languages): `v_binds` (v-bind() expressions in CSS) and `special_pseudos` (`:deep`, `:global`, `:slotted`).
- **Full CSS analysis** (CSS-only, requires `STYLE_CSS` flag): selectors, classes, IDs, custom properties, at-rules, specificity. Uses lightningcss for parsing.
- **Preprocessor passthrough**: For SCSS/Less/Stylus, only Vue features are stored; full CSS analysis is skipped.

**StyleAnalysisFlags**: Bitflags for quick queries (HAS_SCOPED, HAS_MODULE, HAS_VBIND, HAS_DEEP, HAS_GLOBAL, HAS_SLOTTED, etc.).

## Cross-File Analysis

### ProjectIndex

Aggregates file-level usage into project-wide indexes:

- **provide_index**: provide key to files that call `provide(key)`
- **inject_index**: inject key to files that call `inject(key)`
- **component_graph**: file to components it uses (forward edges)
- **component_reverse_index**: component name to files that use it
- **class_index**: CSS class name to files that define it
- **v_bind_css_index**: v-bind CSS expression to files that use it
- **custom_property_index**: CSS custom property to files that define it

### FileUsageFlags

Combined bitflags for quick queries about file capabilities (HAS_PROVIDE, HAS_INJECT, HAS_LIFECYCLE_HOOKS, HAS_REACTIVE_STATE, HAS_TEMPLATE_REFS, HAS_DEFINE_PROPS, HAS_SCOPED_STYLE, etc.).

## Module Structure

| Module | Purpose |
|--------|---------|
| `scope` | `AnalysisScope` bitflags and presets |
| `types` | Core script analysis types (`ScriptAnalysisSnapshot`, `AnalyzedBinding`, etc.) |
| `analysis` | `build_script_analysis()` -- OXC parse + AST walk |
| `imports` | Import extraction from AST |
| `exports` | Export signature extraction for change detection |
| `macros` | Vue macro detection and type reference collection |
| `classify` | Vue API classification (`classify_vue_api`, `is_reactivity_api`, etc.) |
| `template` | Template analysis types (`TemplateAnalysisSnapshot`, etc.) |
| `style` | Style block analysis (lightningcss + Vue features) |
| `file_usage` | File-level usage summaries (`FileUsageInfoOwned`, `FileUsageFlags`) |
| `project_index` | Cross-file `ProjectIndex` (provide/inject, component graph, CSS) |

## Data Flow

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
