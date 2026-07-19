---
name: compiler-codegen
description: "Rust compiler pipeline, template codegen (VDOM/IDE), CodeTransform, cached directives, strict slots, IDE error recovery, style preprocessing, CompileTarget"
---

# Compiler & Codegen

## Rust Compiler Architecture

AST-based pipeline. `compile()` orchestrator drives a linear 5-phase pipeline:

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
|   +-- vue/              # Vue-specific OXC helpers (macro syntax, v-for, v-slot)
+-- vue/                  # Vue runtime helpers (tag detection, patch flags)
```

## Arena-Based Template AST

Parser builds a flat `Vec<AstNode>` arena with O(1) navigation:

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

**All modifications to generated code MUST go through `CodeTransform` operations** (`overwrite`, `prepend_left`, `append_left`, `move_with_suffix`, etc.). Never apply string replacements, regex transforms, or manual splicing to the output of `build_string()` or to content produced by a `CodeTransform`.

Post-hoc string manipulation breaks sourcemap accuracy: `CodeTransform` generates source maps by tracking chunks (Original, Inserted, Moved, Overwritten). Modifying the string after the transform makes byte offsets in the source map no longer match the content, causing position mismatches in the LSP (e.g. hover landing on the wrong token, go-to-definition jumping to wrong locations).

**Correct:** Use `ct.prepend_left(pos, ".ts")` to insert text at a known position -- chunk list and source map stay consistent.

**Wrong:** Call `content.replace(".vue'", ".vue.ts'")` on the built string -- the source map still reflects the pre-replace byte offsets.

The rule has NO scoped exceptions: the Svelte scoped-CSS renderer (`crates/verter_compiler/src/svelte/runtime/css/render.rs`) edits the original component source through the shared `CodeTransform`'s checked (`try_*`) operations -- whose insertion-affinity chunk model carries the `magic-string` semantics the official `svelte@5.56.3` `render_stylesheet` depends on (content-only `try_update` preserving the replaced range's first-chunk boundary insertions, left/right insertion affinity with per-affinity stacking, `try_remove` clearing interior insertions; pinned by `code_transform/edit_semantics_tests.rs`) -- and generates the css source map (`css.map`) from the SAME transform that built `css.code`. The guard `svelte_css_renderer_uses_code_transform` (`crates/verter_compiler/tests/`) asserts the renderer stays on the shared transform and bans any private edit buffer from the css matcher/render tree.

### Svelte IDE structural projection

Svelte block projectors consume parser-owned structural spans. In particular, `{#snippet ...}` uses `SvelteBlock.head_span`, whose end is the grammar-balanced outer brace; downstream code must not rediscover the head with a first-`}` scan because destructured/defaulted parameters can contain nested braces. Element-owned snippets lower into a lexical IIFE so same-name snippets in sibling elements do not collide and forward, mutual, and recursive references remain valid. Unchanged snippet names, parameter lists, and bodies move as original `CodeTransform` chunks; only punctuation, annotations, or parameter text that genuinely requires a store/await-default rewrite is synthetic.

Private component-call checks may map only byte-identical authored tokens. Synthetic scaffolding, quoted/escaped property spellings, rewritten spreads, and transformed directive names stay unmapped. Legacy intrinsic `on:event|modifier={handler}` projects to the lowercase Svelte DOM attribute (`onevent={handler}`); modifiers are runtime listener behavior and never survive as TSX attribute syntax.

## Binding Metadata Flow

1. `script/process.rs` parses `<script setup>` -> walks AST -> classifies bindings as `BindingType` (SetupConst, SetupRef, Props, etc.)
2. Bindings passed to `template/code_gen/` via `generate_template()` parameter
3. `BindingResolver` determines correct accessor prefix (`_ctx.`, `$setup.`, `__props.`) and suffix (`.value` for refs)
4. Binding patches accumulated in `CodeGenOutput`, batch-applied to `CodeTransform`

## Vue Macro Semantic Boundary (CRITICAL)

The compiler owns Vue macro syntax and code emission, not typed macro
resolution. Parser macro facts are limited to authored spans, runtime
object/array constructors, defaults-object shape, model names/options, and
other syntax needed to preserve the source. Typed `defineProps`,
`defineEmits`, and `defineModel` surfaces arrive from TypeInfo through the
explicit `VueMacroSemanticInput` compile argument:

- `Unavailable`
- `Runtime(Arc<MacroRuntimeBundle>)`
- `Tsc(Arc<MacroTscBundle>)`
- `RuntimeAndTsc { runtime, tsc }`

Runtime and TSC are independent demands. Bundler script emission consumes only
`MacroRuntimeBundle`; declaration emission consumes only `MacroTscBundle`.
Entries join macro syntax by stable `syntax_index`. Runtime entries contain
the normalized props/emits/model shapes. TSC entries contain terminal splice
text and are emitted directly; the compiler does not parse or reinterpret the
splice.

Local declaration carriers preserve TypeInfo refusal detail through
`TscDependencyDeclaration.declaration_failure`: structural inference budgets
remain the closed depth/work variants, while deterministic unsupported and
unresolved declaration shapes remain distinct. The compiler forwards that
typed detail in `TscDeclarationShapeReason`; it never collapses the carrier to
a generic semantic-inference failure or a diagnostic string.

The compiler must never resolve a typed macro parameter, build a companion
type environment, accept a compiler-owned external-type map, or merge
host-resolved types into parser state. `PreparedScript` parses setup and
companion blocks once for syntax reuse only. Typed prop bindings are registered
from the runtime DTO; runtime-form object/array bindings remain parser-owned
syntax facts.

A target that encounters a typed macro without its required bundle, with a
degraded entry, or with a projection for the wrong macro role fails closed at
the authored macro/type anchor using `XMissingMacroSemanticBundle` or
`XUnavailableMacroSemanticResult`. Before runtime codegen, the compiler
structurally validates the whole bundle: syntax/effective macro identities,
roles, `withDefaults` association, public names, authored-member ordinals, and
synthesized model-row anchors must all match parser-owned syntax. Any invalid
row suppresses the entire runtime bundle; a `Complete` row with a degraded
member remains usable, emits `type: null`, and reports the typed reason/detail
at the exact authored key (or model-name/type) span.

Parser model-name facts carry both an OXC-decoded semantic value and the exact
authored literal span. Runtime/TSC joins compare the decoded value, retain the
span only for mappings and diagnostics, and serialize typed emit/model public
names with the canonical JavaScript string escaper.

`withDefaults` syntax remains parser-owned. A statically eligible object
(supported keys, no spread) is folded into each DTO-derived prop row, preserving
the first duplicate and method/default expression syntax. Dynamic, spread, or
unsupported-key defaults preserve the whole authored expression and emit
exactly one `_mergeDefaults`. Runtime prop rendering follows three independent
profiles: development emits `type`, `required: true|false`, `skipCheck`, then a
static default; production retains only Vue-required Boolean/Function types and
defaults; production custom-element mode retains every `type` field, including
`type: null`. `CodegenOptions.custom_element` selects the script policy and is
independent of template tag matching in `custom_elements`. Model props use
Vue's separate model policy (no synthesized `required`; custom-element mode
does not widen production model types).

## IDE Prefixed-Expression Emit Substrate (`ide/template/emit.rs`)

IDE template codegen emits a Vue binding value as JSX through the typed `EmitOp` vocabulary so the user expression keeps an exact source-map mapping while synthetic JSX scaffolding stays unmapped. `EmitText` (`Static`/`Borrowed`/`Owned`) is the text payload; `EmitOp` variants: `InsertUnmapped` (order-preserving unmapped insert, lowers via `prepend_ordered_unmapped`), `InsertMapped` (`InsertedMapped` chunk, mapped at `source_start`+`content_offset`), `PreserveOriginal` (pure no-op — bytes stay an `Original` 1:1 chunk), `OverwriteSyntheticBoundary` (delete + unmapped insert; NEVER a mapped `out.overwrite`), `MoveOriginal`. `emit_op` is the single lowering point. `emit_jsx_binding_value` emits a `JsxBindingValue` (`source_expr`/`prefix`/`suffix`/`occurrences`/`bindings`) `occurrences` times for RELOCATED emission (native `v-model` emits the expression 2-3x); in-place sites (v-html, v-text, `:[key]`, `.foo=`, `v-bind="obj"`, static `:prop`) preserve the bytes and emit `OverwriteSyntheticBoundary` + `collect_binding_patches` around them. A function-typed `:prop` under a v-if scope (e.g. `<div v-if="ok" :onX="() => handle()">`) gets a type-narrowing guard: `compute_function_guard_injection` (props.rs) locates the injection point in SOURCE coordinates from the OXC AST (arrow-EXPRESSION body start → ternary `!((cond))?undefined:`; arrow-BLOCK / `function` body `{`+1 → block `if(!((cond))) return;`), then the value is kept IN PLACE (boundary split + `collect_binding_patches`) and the guard is an UNMAPPED `prepend_alloc` spliced into the middle — emitted BEFORE `collect_binding_patches` so an arrow-expr body identifier at the injection offset stable-sorts as `<guard><accessor-prefix><identifier>`. The guard is never baked into a mapped overwrite. The v-on inline-handler guard (von.rs) is likewise a synthetic PREFIX inside `out.overwrite(prop.start, trimmed_vs, …)` with the handler body preserved in place — it never bakes the resolved value, so it is not migrated.

Bug this replaces: baking `prefix + identifier` into one `out.overwrite(prop.start, prop_end, &format!(...))` produced a `Chunk::Overwritten` mapping the whole run back to the prop start (identifier hover/go-to-definition landed on the prop name). The flat-string IDE producers `resolve_prefixed_expr`/`resolve_prefixed_dynamic_arg` were deleted; wrapped/transformed flat-string consumers (v-on spreads, dynamic event-name keys, v-show) call the shared `build_prefixed_expr` directly. Guard: `crates/verter_compiler/tests/cases/ide_no_baked_prefix_overwrite.rs` — scans `ide/template/**` for both the INLINE bake (`out.overwrite(.., &format!(..<resolver-var>..))`) and the `let`-INDIRECTION (`let v = …format!(..<resolver-var>..)… / build_prefixed_expr(..) / resolve_simple_expr(..); out.overwrite(.., &v)`), EXCLUDING self-anchored overwrites (`out.overwrite(base + node.start, base + node.end, &v)` replaces one node's own span → navigable; partial-interpolation recovery path is the canonical example). The allowlist is EMPTY.

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

The **LSP uses the IDE path** via `host.ensure_compiled()` with `CompileTarget::IDE`. TSGO type-checks this output. Changes to VDOM codegen do NOT affect LSP hover/completions. IDE codegen auto-detects the script language: TS SFCs produce `.tsx` (TypeScript + JSX); JS SFCs (no `lang` or `lang="js"`) produce `.jsx` (JavaScript + JSDoc annotations).

## Compiled-Output Conformance (CRITICAL)

Official-framework compiler conformance is behavioral plus structural/helper-topology parity, not raw-byte identity. For Vue VDOM/Vapor, Svelte `svelte/internal/*`, SSR/client, and future runtime backends, compare emitted output by observable behavior plus parsed/token-normalized structure: imports, helper families, helper call sequence where order is semantic, memoization/reactivity/effect topology, DOM/hydration template topology, class/style/attribute normalization, prop/property routing, event delegation, and diagnostic/reject ordering.

Cosmetic JS carrier formatting is not a finding: indentation, line breaks, non-semantic comments, intra-expression whitespace outside literals, and behavior-preserving redundant parentheses may differ from the official compiler. Directive, pragma, license/preserve, source-map/sourceURL, TS-directive, JSDoc, and other tool-consumed or framework-significant comments remain in contract. Generated local identifier spellings are waived only when the backend oracle implements scope-aware alpha-equivalence for private, non-observable bindings; otherwise identifiers are structural. Literal payload bytes, static HTML/CSS/SSR strings, public/exported or source-authored names, sourcemap mappings, diagnostic text/codes/order, and any framework-defined observable format remain in contract.

Do not build or route production compiled-output emission through JS printers, re-printers, redundant-paren canonicalizers, or any machinery whose role includes mimicking the official compiler's cosmetic JS carrier formatting. Direct-emission helpers may emit syntax-required tokens, including required parentheses for valid JavaScript expression/statement shape, but they must be scoped to semantic/syntactic correctness and covered by behavioral/structural tests rather than official cosmetic byte parity. Emit correct code directly and make conformance oracles structural for cosmetic categories: a cosmetic-only diff passes; a behavioral or structural divergence fails.

Byte-equality tests remain valid only where bytes are the actual contract, such as generated binding freshness, source-map exactness, or self-characterization during a refactor; they are not official-compiler conformance oracles.

Tracked guard gap: the positive structural-discriminator guard currently covers Svelte client only. Add backend-owned positive structural conformance oracles for Vue VDOM/Vapor and SSR/client outputs before those backends are considered fully guard-covered by this rule; the re-printer guard is cross-backend negative coverage.

### Deliberate documented deviations (Svelte client)

Default is parity with official's observable-correct behavior. A deviation is a DELIBERATE final-state choice to differ from official's correct behavior, recorded with a deviation record, durable code comment, and landed note; silent divergence is never a deviation. The native Svelte client backend currently has no deliberate deviations. This does not mean zero divergences: known structural/helper-topology divergences, behavior-equivalent topology differences, and unconverged SSR/unimplemented surfaces remain tracked in `crates/verter_compiler/src/svelte/runtime/diff_oracle_divergences.rs` or their owning tests and must be converged or kept fail-closed before promotion. `<svelte:head>` attributes fail closed matching official's `svelte_head_illegal_attribute`; that is reject-parity, not a deviation.

Guards: `svelte_structural_conformance_discriminates_cosmetic_from_behavioral_diffs`, `no_compiled_output_cosmetic_reprinter_path`.

### Svelte client text interpolation

Template text expressions are classified from the canonical retained OXC AST. Supported roots are identifier, member/optional-member, call/optional-call, binary, logical, conditional, template, `new`, and primitive literals. Rewriting, call memoization, binding impurity, D-14 constant evaluation, and nullish-coalescing analysis must consume that retained carrier rather than reparsing or scanning source text. Exact static runs use `textContent` (sole element child) or `nodeValue` (reached sibling text node) without an effect; mixed static/live chunks share one text update, and call-bearing values use the official deps-array `$.template_effect` topology. Each/await aliases retain their signal-root rewrite. Unsupported nested constructs preserve their precise typed refusal.

## Svelte Compile-Options Resolver

`resolve_svelte_compile_options(source, parsed, opts) -> Result<ResolvedSvelteCompileOptions, UnsupportedSvelteRuntimeSurface>` (`svelte/runtime/compile_options.rs`) is the SINGLE fold point for Svelte compile options. It runs ONCE per compile request from the single guarded call site at the top of `compile_client` (`svelte/runtime/client_compile.rs`) — every downstream consumer reads the resolved struct, never the raw `SvelteRuntimeOptions`.

**The fold** — compile-option side (`SvelteRuntimeOptions`) ∪ the inline `<svelte:options>` attributes, INLINE WINS per admitted key (matching `svelte@5.56.3` precedence). Inline values are read through the typed AST via the shared parser value authority (`options_namespace_value` / `options_boolean_value`), never a raw rescan. Only the keys the inline syntax admits (`namespace`, `preserveWhitespace`) fold; the resolver runs AFTER the official-reject gate, so it only ever sees official-accepted `<svelte:options>` shapes. The folded `namespace` is used ONLY to fail closed (see below) — the backend emits HTML-namespace roots ONLY, so no namespace value is threaded to codegen.

**Resolved struct** `ResolvedSvelteCompileOptions { fragments: SvelteFragments{Html,Tree}, preserve_whitespace: bool (default false), preserve_comments: bool (default false), disclose_version: bool (default true) }` — HTML-only, four fields. There is NO resolved `namespace` field (svg/mathml fail closed, so the emitted root is always html-namespaced), NO `component_name` field, and NO `css_hash_override` field: the component name is derived during LOWERING (`derive_component_name` in `naming.rs`, reading `opts.name` ?? filename, then `Scope.generate` sanitization + deconfliction against the canonical `ComponentScopeFacts` binder — `component_scope_facts.rs`, `source_declarations ∪ free_references` from one lexical pass over the module/instance scripts plus the template's authored declarations and stored expression references; the single scope authority, replacing the earlier selective `all_declared_names` + reparse approximations) and fed onto `ComponentIr`, and the `cssHash` override rides the carrier channel into the single style-plan scope point (see `/host-session` for the cache-identity seam).

**Namespace fail-close (html-only).** A `namespace: 'svg' | 'mathml'` selection (compile-option OR inline) fails closed at the resolver with a typed `UnsupportedSvelteRuntimeSurface::NamespaceUnsupported { namespace: SvelteNamespace, origin: CompileOptionOrigin{CompileProfile,Inline}, span: Option<Span> }` (stable code `svelte-runtime-unsupported-namespace`) → NO runtime module; an inline `namespace="html"` masks a compile-option `svg`/`mathml` (inline wins). svg/mathml ELEMENT emission (the `$.from_svg` / `$.from_mathml` root-helper family, the `TEMPLATE_USE_SVG` / `TEMPLATE_USE_MATHML` flag bits) is a separate deferred element-emission surface — see the svelte-native-compiler-plan D-62 row. There is NO ns×fragments matrix: every supported root, in every fragments mode, is html-namespaced.

**Per-option codegen consumers** (all read the resolved struct):

- `fragments` → the root template factory. `emit_root_hoist` (`client_module_frame.rs`) picks `$.from_html` (the backtick clone) or `$.from_tree` (the array-literal objectifier) under `fragments: 'tree'`; the root is always html-namespaced.
- `preserve_whitespace` → seeds the root `CleanContext { preserve_ws }` threaded through region synthesis.
- `preserve_comments` → a drop-set gate on retained comments, which serialize as `<!--data-->` (bare `<!>` for empty) in `template_serialize.rs` with the node-path shift applied.
- `disclose_version` → `ImportPlan.disclose_version` (`helpers.rs`), toggling the `import 'svelte/internal/disclose-version'` side-effect import.

**Fail-closed unsupported carrier.** Four officially-accepted options this backend does not support — `compatibility.componentApi` (any explicit value other than `5`), `hmr`, `accessors`, `immutable` — are demoted out of the essential surface. Any EXPLICIT presence (including a `false` / default-equivalent value, from EITHER the compile-option origin OR an inline `<svelte:options>` origin, even a value later masked by inline) fails closed with a typed `UnsupportedSvelteRuntimeSurface::CompileOptionUnsupported { option: UnsupportedSvelteCompileOption, origin: CompileOptionOrigin{CompileProfile,Inline}, span: Option<Span> }` (distinct stable codes `svelte-runtime-unsupported-{compatibility-component-api,hmr,accessors,immutable}`) → NO runtime module. This is a FEATURE refusal, NOT an official compile-error. The deprecated inline `tag` key stays the parser-first `svelte_options_deprecated_tag` HARD error, with a defensive unreachable resolver arm. `runes` is NOT folded here — it flows through the existing mode-inference plumbing (`forced_runes_option` + `opts.runes`); `css` / `customElement` / `dev` / `generate` / `experimental.async` stay delegated to their owners.

## Svelte Conformance Trace (`conformance-trace` feature)

`verter_compiler`'s `conformance-trace` Cargo feature (default OFF) enables the typed conformance-observability side channel `verter_compiler::svelte::runtime::conformance_trace` — CONFORMANCE-TOOLING-ONLY, consumed by the `verter_svelte_conformance` crate (which dev-deps `verter_compiler` with the feature on). It is not a production API surface: the default build compiles the module, its producer hooks, and every trace collection site out entirely, and production IR structs carry no trace state under either setting.

**API surface** (feature-gated): `compile_client_with_conformance_trace(...)` runs the production `compile_client` pipeline under a capture and returns the compile outcome together with the trace (a refused/rejected fixture still returns what was observed up to the failure); `capture(f)` installs a thread-local trace around a closure (captures nest, unwind-safe restore); `ConformanceTrace { static_attrs, style_matches }` carries static-attribute lexical provenance (quoting + HTML entity source representation, folded from the attribute-lowering producer boundary's single decode pass — never a second source scan) plus per-`<style>` matcher facts (per-selector tri-state certainty rows, used/scoped selector spans, scoped element identities); `MatchCertainty` is re-exported.

**`MatchCertainty` tri-state** (`svelte/runtime/css/match.rs`, always-on — NOT feature-gated): `No < Maybe < Yes`, `and` = min, `or` = max. Production projects through `might_match()`: `Yes | Maybe ⇒ true`, `No ⇒ false` — byte-identical to the pre-tri-state boolean matcher (`Maybe` was `true`; it is never treated as `No`). The per-selector certainty rows on the match sink exist only under `cfg(any(test, feature = "conformance-trace"))`.

**Zero cost when off**: by `#[cfg]` gating plus a monomorphized no-op entity-decode observer that compiles away in the default path. Guarded by `crates/verter_compiler/tests/svelte_conformance_trace_zero_cost_guard.rs` (prod-IR trace-mention ban, feature-gated module declaration, closed `AttrIr`/match-sink field inventories, decoder-mention ban, manifest keeps the default build feature-off with no dev-dependency re-enable channel) and an isolated feature-off CI gate (`cargo build`/`cargo test -p verter_compiler --lib` with no conformance crate in the dependency graph, so workspace feature unification cannot mask the default build).

## Strict Slot Children Type Checking (Experimental)

When `strict_slots: true` (VS Code: `verter.experimental.strictSlots`), the IDE template codegen emits `strictRenderSlot` calls after the JSX tree, enforcing that slot children match the parent component's `defineSlots()` type signature ([RFC #733](https://github.com/vuejs/rfcs/discussions/733)).

**Generated pattern** (inside the block scope, after JSX):

```tsx
___VERTER___strictRenderSlot({} as NonNullable<ReturnType<typeof ___VERTER___Comp{offset}>['$slots']['{slot}']>, [TabItem, {} as HTMLElementTagNameMap["input"], "" as string]);
```

**Child type references**: Component -> constructor name, HTML element -> `HTMLElementTagNameMap["tag"]`, text/interpolation -> `"" as string`. Each child is a sourcemapped `InsertedMapped` chunk pointing to its template position.

**Skipped cases**: self-closing components (no children), `is_jsx` mode, `<component :is>` (deferred), whitespace-only text, comments.

**Key files**: `ide/template/mod.rs` (`StrictSlotEntry`, `collect_strict_slot_children`, `emit_strict_slot_checks`), `ide/script.rs` (ambient `strictRenderSlot` type declarations).

## Cached Directive Fields on ElementNode

Parser extracts structural directives from `el.props` via `prop.take()` and caches them as dedicated fields on `ElementNode` (`ast/types.rs`):

| Field         | Directive                     | In `el.props`? | Notes                                            |
| ------------- | ----------------------------- | -------------- | ------------------------------------------------ |
| `v_condition` | `v-if`, `v-else-if`, `v-else` | **No** (taken) | Contains `ElementNodeCondition` with kind + prop |
| `v_for`       | `v-for`                       | **No** (taken) | Contains the full `NodeProp`                     |
| `v_slot`      | `v-slot`, `#name`             | **No** (taken) | Contains the full `NodeProp`                     |
| `v_once`      | `v-once`                      | **No** (taken) | Contains the full `NodeProp`                     |
| `v_ref`       | `ref`, `:ref`                 | **No** (taken) | Contains the full `NodeProp`                     |

**Consequence**: Code iterating `el.props` will **never see** these directives. Both codegen paths must handle them explicitly. The IDE module removes `v-if/v-for/v-slot/v-once` attributes (they become JSX wrappers/removals) and converts `ref` to JSX expression syntax (`ref={"name"}`).

## IDE Script Error Recovery

OXC parses the original `<script setup>` content exactly ONCE (`ide/script/setup.rs`). There is a single recovery surface — no truncate-and-reparse, no clean-prefix reparse authority, no file-scope error mode.

- **Clean parse** → the full codegen path runs unchanged (import/type-decl hoisting, binding extraction, macro lowering, the `___VERTER___TemplateBindingFN` wrapper).
- **Genuine syntax error** (both the TSX parse AND a TS-mode parse fail — a TSX-only failure is an angle-bracket assertion already handled by `rewrite_ts_type_assertions`) → a single token scan of the REAL source produces a `ScriptSetupRecoveryPlan` (`ide/script_recover.rs`, `ScriptTokenScanner::recover_plan`). The plan carries **top-level (bracket depth 0)** original-span `imports` / `macros` / `functions` / `variables` (reused for hoisting + binding registration) plus OUTPUT-ONLY recovery chunks (detected over the WHOLE source at any depth):
  - **member holes** — a dangling `a.` / `a?.` gets a universal member placeholder (`valueOf`) right after the operator so the dot cannot absorb the following token;
  - **expression holes** — a trailing operator / assignment RHS / conditional arm / arrow body gets an operand placeholder (`(undefined)`);
  - **scope closers** + a statement terminator at the recovery boundary (the `</script>` overwrite) — close the brackets the user left open so the generated scaffolding starts cleanly. A delimiter that requires a non-empty body but was left empty (a grouping/arrow-body paren `const x = (`, a computed-member bracket `foo[`) gets a placeholder operand BEFORE its closer (`undefined)`, `undefined]`); call args `foo()`, array literals `[]`, and blocks/objects `{}` are valid empty and get a bare closer.

**Top-level fact gate.** Recovered facts are gated to bracket depth 0, mirroring the clean top-level parser's `block_depth == 0` rule. A block-local declaration (`function f(){ const inner = 1; }`) is NEVER recovered as a setup binding/import; only the whole-source holes/closers fire inside nested scopes.

**Recovered macro = clean-lowering parity.** A recovered `defineProps`/`withDefaults` binding is registered `Props` AND emits the same `const __props = <binding>;` alias as clean macro lowering, so a template `props.x` (lowered to `__props.x`) resolves instead of dangling against a `__props` that was never declared.

The user's body STAYS inside the `___VERTER___TemplateBindingFN` wrapper in both cases; the broken-tail member access (`count.`) keeps hover/completion/go-to-definition working for declarations above the cursor.

**No synthesize-then-reparse.** Synthetic recovery chunks are output-only and unmapped; they are NEVER bindings, macros, imports, or any other source fact. Recovery metadata comes only from the original clean OXC AST or from original-span token recovery over the real source — a reparsed synthetic view is never an authority.

Guard: `crates/verter_compiler/tests/cases/ide_script_recovery_guard.rs` (scans `ide/script/setup.rs` for the deleted dual-recovery identifiers and the synthesize-then-reparse anti-pattern), plus `crates/verter_compiler/tests/cases/repro_member_access_ide_codegen.rs` (recovery shapes + clean-path preservation) and the negative-metadata tests in `script_recover.rs`.

## Style Preprocessing in Bundler Mode

Style blocks with `lang="scss"`, `lang="sass"`, or `lang="less"` require preprocessing to CSS. The pipeline differs between Vite and non-Vite bundlers:

**Vite mode** (Vite-owned preprocessing, matching `@vitejs/plugin-vue`):

1. During main `.vue` `transform()`, the plugin parses the SFC with `compiler.parse()` and caches raw style block content in `styleBlockCache`. Style preprocessing is **skipped** in `applyPreprocessorRequests()`.
2. `load()` returns raw style source (e.g. SCSS with `$variables`) from `styleBlockCache`.
3. Style URLs preserve the original lang (`lang.scss`, not `lang.css`) since `meta.style_langs` is never overwritten.
4. Vite's CSS pipeline preprocesses SCSS/SASS/Less/Stylus automatically between `load()` and `transform()`.
5. `transform()` always runs `compiler.compileStyleAsync()` for Vue-specific post-processing: scoped CSS attribute selectors (`[data-v-...]`) and CSS `v-bind()` rewriting. Runs even for unscoped plain CSS blocks (CSS `v-bind()` still needs rewriting).

**Non-Vite mode** (preprocessor fallback):
Style preprocessing goes through `preprocessBlock()` -> `preprocessStyle()` which calls Vite's `preprocessCSS()` in-process (if Vite config is available). The compiled CSS is sent to the Rust host via `applyBlockOverrides()`, and `apply_style_overrides()` updates `meta.style_langs` to `"css"`. The `transform()` hook uses Rust `processStyle()` for CSS scoping only.

**Compiler resolution**: `vue/compiler-sfc` is resolved once per plugin instance from the project root in `configResolved()` via `createRequire(join(root, "package.json"))("vue/compiler-sfc")`, stored in the `compiler` variable and used for both SFC parsing (`compiler.parse()`) and style post-processing (`compiler.compileStyleAsync()`).

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

**Empty SFC = valid empty component.** A completely block-less `.vue` file (0 bytes / whitespace / comments only) compiles to a minimal synthetic shell — `defineComponent({ __name: "<Filename>" })` + `export default` — through a dedicated synthetic-script branch (`empty_sfc_script_block` in `compile/helpers.rs`) adjacent to the scoped-style/vapor/SSR one, so the host assembles a `Main` virtual node instead of erroring `MissingVirtualNode`, and the imported public surface is empty (`$props: {}`, no slots). Zero-block files also count the whole input as one inter-block gap (`remove_inter_block_gaps`), so stray top-level comments never leak into generated module output. Template-only SFCs keep their existing no-synthetic-script shape.

## TypeExpr Lowering To The Semantic Graph (session boundary)

The OXC worker and the semantic-lowering surface produce owned `TypeExpr` IR (and worker-local OXC AST) ONLY — they never emit a session semantic-graph node (`SemanticNodeData` / `SemanticNodeId` / `HotTypeRef`); that crate barrier (`verter_semantic` never depends on `verter_session`) is locked from the worker side by the `oxc_worker_emits_no_session_graph_node` guard. Downstream, a session-owned, query-free **structural lowerer** (`crates/verter_session/src/structural_carrier_producer/lower.rs`, entry `lower_type_expr_structural`) consumes that owned `TypeExpr` and emits the dormant semantic-graph carriers (`BareRef` / `ImportType` / `RawFallback` / `ConstructorType` / `SyntheticBinding`, with tuple rest preserved on `TupleElement.rest`) plus the structural shells, NodeScopeId-rooted, performing NO name / import / type resolution: `Foo<Arg>` becomes a `BareRef` whose `type_args` are structurally lowered (never an `InstantiationRef`), and `keyof` / indexed-access / conditional / mapped / `typeof` stay deferred shells even where the eager path would reduce them. It is intern-only — it makes no host / dispatch query (`session_graph_lowerer_makes_no_query`) and never materializes a carrier back to `TypeExpr` during emission (`unresolved_carriers_not_materialized_during_emission`). It stays dormant / demand-time (never pulled into publish or indexing). Carrier RESOLUTION is a separate demand-time engine — see the type-resolution skill.

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
