1) **Executive Summary**
- Biggest correctness risk #1: local scope variables are not consistently respected in template expressions outside interpolation. `v-slot`/`v-for` locals are not fed into prop/event/v-if expression rewriting, so locals can be incorrectly prefixed as `_ctx.*` (`crates/verter_core/src/codegen/vue/template/directives.rs:184`, `crates/verter_core/src/codegen/vue/template/element.rs:286`, `crates/verter_core/src/codegen/vue/template/interpolation.rs:152`).
- Biggest correctness risk #2: `v-on` dynamic args are parsed (`is_dynamic_arg`) but codegen ignores that for events, always emitting static `onXxx` keys (`crates/verter_core/src/codegen/vue/template/directives.rs:286`, `crates/verter_core/src/codegen/vue/template/element.rs:2829`).
- Biggest correctness risk #3: non-`<script setup>` blocks are effectively unsupported and currently left untransformed in monolithic flow, likely producing invalid JS/SFC remnants (`crates/verter_core/src/codegen/vue/script.rs:289`).
- Biggest sourcemap risk #1: generated/source columns are advanced by Unicode scalar (`.chars()`) count, not UTF-16 code units; astral chars will drift columns (`crates/verter_core/src/code_transform/source_map.rs:98`, `crates/verter_core/src/code_transform/source_map.rs:151`, `crates/verter_core/src/code_transform/source_map.rs:284`).
- Biggest sourcemap risk #2: `calculate_line_column` mixes byte traversal with scalar column increments; this is not UTF-16-consistent (`crates/verter_core/src/code_transform/source_map.rs:284`).
- Biggest sourcemap risk #3: `v-for` span fallback can use un-offset parsed spans, corrupting both emitted slices and mappings in separator-edge cases (`crates/verter_core/src/codegen/vue/template/directives.rs:88`, `crates/verter_core/src/codegen/vue/template/directives.rs:121`).
- Biggest performance risk #1: every expression rewrite can allocate a fresh OXC allocator/parser (`crates/verter_core/src/codegen/vue/template/element.rs:3480`, `crates/verter_core/src/codegen/vue/template/element.rs:3494`).
- Biggest performance risk #2: per-compile memory leak from `Box::leak(SyntaxPluginOptions)` in both entrypoints (`crates/verter_core/src/builder/codegen.rs:496`, `crates/verter_core/src/builder/codegen.rs:650`).
- Biggest performance risk #3: tokenizer hot-path allocates and drops a `String::from_utf8(...to_vec())` in `v-pre` branch (`crates/verter_core/src/tokenizer/byte.rs:701`).
- `v-model` generation is incomplete vs Vue semantics: argument/modifier forms for components are not emitted as `foo`/`onUpdate:foo`/`fooModifiers` (`crates/verter_core/src/codegen/vue/template/directives.rs:315`, `crates/verter_core/src/codegen/vue/template/element.rs:3000`).
- `v-bind` spread handling is incorrect in multiple cases: only first spread handled in spread-only branch and spread dropped in props-with-key path (`crates/verter_core/src/codegen/vue/template/element.rs:2670`, `crates/verter_core/src/codegen/vue/template/element.rs:2674`, `crates/verter_core/src/codegen/vue/template/element.rs:3460`).
- Scoped-style/CSS pipeline is partially implemented: parser stage still TODO for LightningCSS + CSS `v-bind()` extraction (`crates/verter_core/src/syntax/plugins/css_parser/css_parser.rs:113`).
- Component-id logic is inconsistent with documented strategy: `generate_component_id` exists but style scoping/modules often still use `get_hash(component_name)` (`crates/verter_core/src/builder/codegen.rs:319`, `crates/verter_core/src/builder/codegen.rs:524`, `crates/verter_core/src/codegen/vue/style_plugin.rs:163`).
- SSR option is defined but unused in core codegen flow (`crates/verter_core/src/builder/codegen.rs:69` and no downstream usage in `verter_core/src`).

2) **Implemented Pipeline (Diagram + Call Graph)**

`generate(input, options, allocator)`  
-> create `SyntaxPluginContext` (`builder/codegen.rs:484`)  
-> detect script lang via `ScriptDetector::detect` (`builder/codegen.rs:503`)  
-> pre-scan scoped style + pre-scan script-setup bindings (`builder/codegen.rs:518`, `builder/codegen.rs:533`)  
-> syntax pipeline run: `CssParserPlugin -> OxcParserPlugin -> Analysis -> VueCodegenPlugin` (`builder/codegen.rs:552`)  
-> `tokenize(bytes, |e| syntax.handle(e))` (`builder/codegen.rs:558`)  
-> finalize code + map (`builder/codegen.rs:565`, `builder/codegen.rs:593`).

`generate_for_vite(...)`  
-> same detect/pre-scan pattern (`builder/codegen.rs:635`)  
-> split pipeline: `CssParserPlugin -> OxcParserPlugin -> Analysis -> ScriptCodegenPlugin -> TemplateCodegenPlugin -> StyleCodegenPlugin` (`builder/codegen.rs:704`)  
-> build `script/template/styles` block outputs + import extraction (`builder/codegen.rs:738`, `builder/codegen.rs:767`).

Stage contracts (implemented)
- Stage 1 Tokenization: input bytes -> tokenizer events; offsets are byte-based (`tokenizer/byte.rs`, `syntax/syntax.rs:105`).
- Stage 2 Syntax event normalization: tokenizer events -> `SyntaxEvent`; plugin chain supports keep/replace/drop (`syntax/syntax.rs:40`, `syntax/plugin.rs:70`).
- Stage 3 CSS parse stage: tracks root style attrs and emits `CssStyleContent` at style close (`css_parser.rs:186`); currently metadata-first, no LightningCSS parse (`css_parser.rs:113`).
- Stage 4 OXC parse stage: converts interpolations/props/directives/script into OXC-backed events (`oxc_parser.rs:167`, `oxc_parser.rs:247`, `oxc_parser.rs:102`).
- Stage 5 Analysis stage: enriches with scope/binding events and replaces `CloseTag` with `AnalysedCloseScopes` (`analysis.rs:772`).
- Stage 6 Template/script/style codegen: consumes analysed events and rewrites source through `CodeTransform` (`codegen/vue/plugin.rs:262`, `template_plugin.rs:1`, `style_plugin.rs:1`).
- Stage 7 Source map: generated from `CodeTransform` chunks (`code_transform/source_map.rs:65`).

3) **Codegen Deep Dive (Template Focus)**

Template element handling (`store in open, emit in close`)
- Open side: props/directives are accumulated in `state.current_element`; actual opening call emitted at `process_open_tag_end` (`template/element.rs:21`).
- Close side: `process_close_tag` / `process_close_scopes` finish child arrays, patch flags, v-for/v-slot close actions (`template/element.rs:1138`, `template/element.rs:1326`).
- Root wrapping/finalization: template content is removed/moved and wrapped into render function (`template/element.rs:2083`, `template/element.rs:2134`).

Directive handling
- `v-if/v-for/v-slot` are stored early via analysed events (`template/directives.rs:19`, `template/directives.rs:60`, `template/directives.rs:138`).
- `v-for` issue: iterable expression rewrite is called with `local_vars = []`, so outer locals in nested loops can be wrongly prefixed (`template/element.rs:243`).
- `v-for` locals extraction is string-split based and fails destructuring fidelity (`template/element.rs:4065`).
- `v-for` fallback span extraction can return parser-local spans without adding `value_span.start` (`template/directives.rs:121`).
- `v-on` dynamic arg parsed but ignored in emission path (`template/directives.rs:294`, `template/element.rs:2829`).
- `v-model` argument/modifier semantics are reduced to `modelValue` / `onUpdate:modelValue` (`template/element.rs:3000`).
- `v-bind` spread correctness gaps: spread-only branch uses only first spread (`template/element.rs:2674`), key-path skips spread (`template/element.rs:3460`).
- Slot key emission: `v-slot` name is inserted raw, so non-identifier static names can yield invalid object syntax (`template/element.rs:677`, `template/element.rs:683`).
- Slot outlet `<slot :name="...">` dynamic name path is not handled; only static `name` prop is read (`template/element.rs:633`).

Interpolation / expression compilation
- Interpolation uses text-scanner transform with local bindings from analysis-provided bindings (`template/interpolation.rs:13`, `template/interpolation.rs:152`, `template/interpolation.rs:171`).
- Prop/event/directive expressions primarily use `write_expr_with_ctx` with AST extraction (`template/element.rs:3480`).
- Mismatch risk: interpolation and element props use different rewrite engines.
- Non-self-closing directive array emission path regresses to simple `resolve_binding_prefix + raw expression` for custom directives/v-show, unlike self-closing path using full `write_expr_with_ctx` (`template/element.rs:1642`, `template/element.rs:1741` vs `template/element.rs:978`, `template/element.rs:1069`).

Script macros
- Macros wired: `defineProps`, `withDefaults`, `defineEmits`, `defineModel`, `defineSlots`, `defineExpose`, `defineOptions` (`codegen/vue/macros/mod.rs:32`).
- Binding metadata extraction uses script parse items (`codegen/vue/script.rs:214`).
- Non-setup script path early-returns without converting/removing script tags (`codegen/vue/script.rs:289`).

SSR / prod vs dev
- Dev/prod branches exist in script and template finalization (`codegen/vue/script.rs:515`, `template/element.rs:2270`).
- SSR option is not plumbed into template/script emission behavior (`builder/codegen.rs:69` only).

Concrete mismatch examples (expected shape)
- `<template><Comp v-on:[evt]="h" /></template>` should emit computed handler key; current path emits static `on...` logic.
- `<template><Comp v-model:title="x" /></template>` should emit `title` + `onUpdate:title`; current emits `modelValue` + `onUpdate:modelValue`.
- `<template><template #my-slot-name>...</template></template>` should quote slot key or use computed form; current emits raw `my-slot-name:`.

4) **Source Map Audit**

Mapping origin/composition
- Mappings are generated from `CodeTransform` chunk stream in `generate_map` (`code_transform/source_map.rs:65`).
- `move_wrapped` keeps moved chunk original spans and adds unmapped prefix/suffix (`code_transform/code_transform.rs:635`, `code_transform/chunk.rs:47`).
- Monolithic path emits inline map after `vue_codegen.generate_source_map(...)` (`builder/codegen.rs:593`).
- Vite split blocks generate per-block maps from each plugin transform (`builder/codegen.rs:738`, `builder/codegen.rs:767`).

UTF/offset risks
- Column math uses scalar chars in many places (`source_map.rs:98`, `source_map.rs:150`, `source_map.rs:243`, `source_map.rs:275`).
- Line/column conversion also mixes byte progress with scalar columns (`source_map.rs:284`).
- NAPI separately converts import/body offsets to UTF-16 (`verter_napi/src/lib.rs:195`), which can disagree with map columns for non-BMP text.

Off-by-one/newline risks
- Syntax layer has explicit offset asymmetry between `OpenTagEnd + 1` vs self-closing unchanged; correctness depends on tokenizer contract (`syntax/syntax.rs:149`, `syntax/syntax.rs:162`).
- Attribute value end uses quoted/unquoted branching; boundary bugs here propagate into expression slices/maps (`syntax/syntax.rs:297`).

5) **Binding / Scope Analysis Audit**

Binding metadata production/consumption
- Produced from parsed script items/macros (`codegen/vue/script.rs:214`).
- Consumed by prefix resolver (`template/types.rs:55`) via name-based lookup (`template/types.rs:41`).

Scope tracking
- Analysis builds parent bindings + provided bindings from Loop/Slot scopes (`analysis.rs:238`).
- Analysis emits enriched events for props/interpolations/v-if/v-for/v-slot (`analysis.rs:517`, `analysis.rs:607`, `analysis.rs:646`, `analysis.rs:781`, `analysis.rs:918`).
- Interpolations consume provided bindings (`template/interpolation.rs:152`), but prop/directive/event codegen path does not consume analysis scope bindings (it only consumes spans + v-for local string stack), creating shadowing/locality bugs.

Shadowing/hoisting notes
- `BindingMetadata::get` is first-match by identifier bytes (`template/types.rs:42`), not lexical scope-aware.
- `props_destructure` feature flag exists but behavior is marker-only in output (`builder/codegen.rs:572`).

6) **Vite Output / Import Extraction Audit**

Import extraction and split integrity
- Import extraction is line-oriented and recognizes only `import ... from ...` with simple braces parsing (`builder/codegen.rs:205`, `builder/codegen.rs:239`, `builder/codegen.rs:249`).
- Side-effect imports (`import "x"`), multiline imports, and complex import formatting can be missed or partially parsed.
- `body_start` is first non-import byte; correctness depends on heuristic parser (`builder/codegen.rs:273`).

Split block lifecycle
- Script/template/style split plugins are correctly ordered after analysis (`builder/codegen.rs:704`).
- Template plugin removes non-template regions and finalizes standalone render block (`template_plugin.rs:83`, `template_plugin.rs:172`).
- Style blocks get independent transforms/maps; good structure, but CSS parsing semantics are still partial (`style_plugin.rs:119`, `css_parser.rs:113`).

7) **Issues & Recommendations (Prioritized)**

| Priority | Symptom | Root cause | File:Line/Function | Fix | Test to add | Perf impact |
|---|---|---|---|---|---|---|
| P0 | Wrong `_ctx.` prefix for slot/loop locals in props/events/v-if | Scope locals not carried into non-interpolation expression rewriting | `template/directives.rs:184`, `template/element.rs:286`, `template/interpolation.rs:152` | Thread analysed scope bindings into `PropEntry` and pass local sets into `write_expr_with_ctx` everywhere | `v-slot` param used in `:class`, `@click`, `v-if` | Medium |
| P0 | Nested `v-for` iterable references outer local as `_ctx.*` | Iterable rewrite explicitly uses empty locals | `template/element.rs:243` | Pass accumulated loop/slot locals when rewriting iterable expression | Nested `v-for="sub in item.children"` | Low |
| P0 | `@[evt]` compiled as static event key | `PropKind::On` ignores `is_dynamic_arg` | `template/directives.rs:294`, `template/element.rs:2829` | Emit computed keys for dynamic event args (Vue-compatible `toHandlerKey`) | `v-on:[name]`, `@[name].once` | Low |
| P0 | `v-model:foo` and component modifiers miscompiled | Model emission hardcodes default model channel | `template/directives.rs:315`, `template/element.rs:3000` | Carry model arg/modifier data through and emit `foo`/`onUpdate:foo`/`fooModifiers` | `v-model:title`, `v-model.trim` on component | Low |
| P0 | Spread props dropped in key path / partial in spread-only path | Spread handling logic incomplete | `template/element.rs:2670`, `template/element.rs:2674`, `template/element.rs:3460` | Normalize to unified spread merge path (`mergeProps`) including keyed branches | `v-if` branch with `v-bind="a"` and `v-bind="b"` | Low |
| P0 | Non-setup `<script>` leaves invalid SFC remnants | Early return without script transform/removal | `codegen/vue/script.rs:289` | Implement options-script transform or explicit hard error diagnostic | `<script>` + `<template>` compile output validity | Low |
| P0 | Complex directive/v-show expressions wrong on non-self-closing elements | Non-self-closing directive array path uses simple prefixing, not expression rewrite | `template/element.rs:1642`, `template/element.rs:1741` | Use `write_expr_with_ctx` consistently in both self-closing and non-self-closing paths | `v-show="a+b"`, custom directive value expression | Low |
| P1 | Sourcemap columns drift on emoji/non-BMP chars | Column increment uses scalar chars, not UTF-16 units | `source_map.rs:98`, `source_map.rs:150`, `source_map.rs:284` | Use UTF-16 column accounting in map builder path | Source-map validation with astral chars | Medium |
| P1 | `v-for` fallback can slice wrong source and map wrong regions | Fallback spans not offset by directive value start | `template/directives.rs:121` | Offset fallback spans by `value_span.start` | `v-for` with non-standard spacing/newline around `in/of` | Low |
| P1 | Import metadata incomplete for Vite blocks | Heuristic line parser misses valid import forms | `builder/codegen.rs:205` | Parse imports with OXC parser over generated JS AST | Side-effect and multiline import fixtures | Medium |
| P1 | Component/style id strategy inconsistent with docs/options | `generate_component_id` exists but style paths often hash `component_name` | `builder/codegen.rs:319`, `builder/codegen.rs:524`, `style_plugin.rs:163` | Centralize ID generation and pass through all style/template/script paths | Deterministic id parity tests (monolithic vs Vite/dev vs prod) | Low |
| P2 | High CPU on expression-heavy templates | Fresh parser+allocator for each expression | `template/element.rs:3494` | Reuse parser context/arena or precompute analysed expression rewrites | Expression-heavy template microbench | High |
| P2 | Per-compile memory growth | `Box::leak` for syntax options | `builder/codegen.rs:496`, `builder/codegen.rs:650` | Keep options on stack and pass borrowed lifetime without leak | Repeated compile loop memory test | Medium |
| P2 | Avoidable tokenizer allocation | Dead `String::from_utf8(...to_vec())` in hot path | `tokenizer/byte.rs:701` | Remove dead allocation | Tokenizer throughput regression bench | Medium |
| P3 | CSS parser pipeline claims ahead of implementation | LightningCSS parse/v-bind extraction TODO | `css_parser.rs:113`, `css_parser.rs:134` | Implement parser stage or narrow feature claims + diagnostics | CSS `v-bind()` + preprocessor feature tests | Medium |
| P3 | Whitespace mode API not honored in template emission | `WhitespaceMode` exists but text path is hardcoded condense logic | `template/types.rs:14`, `template/element.rs:1814` | Gate whitespace behavior on `state.whitespace` | preserve vs condense snapshot tests | Low |
| P3 | SSR/feature flags mostly no-op in core behavior | options defined but not functionally consumed | `builder/codegen.rs:69`, `builder/codegen.rs:572` | Wire options into script/template branches or remove from API | SSR parity tests vs expected helper usage | Low |

8) **Suggested Test Matrix**

1. `v-slot` param used in prop value: `<template #d="{ item }"><div :class="item.c"/></template>`.
2. `v-slot` param in event handler: `@click="item.onClick"`.
3. Nested `v-for` with outer local in inner iterable: `v-for="sub in item.children"`.
4. `v-for` with destructuring iterator: `v-for="({id}, i) in list"`.
5. `v-for` with newline/tabs around separator to trigger fallback span path.
6. Dynamic event arg: `@[evt]="h"`.
7. Dynamic event arg with modifiers: `@[evt].once.prevent="h"`.
8. Component `v-model:title="x"` emits `title` + `onUpdate:title`.
9. Component `v-model.trim="x"` emits `modelModifiers`.
10. Native input `v-model` with `type="checkbox"` selects checkbox directive helper.
11. Two spread binds only: `v-bind="a" v-bind="b"`.
12. Spread + keyed conditional branch: `v-if` + `v-bind="obj"` + static props.
13. Non-self-closing custom directive with complex expression value.
14. Same custom directive in self-closing and non-self-closing forms; assert parity.
15. Slot name requiring quoting: `#my-slot-name`.
16. Slot outlet dynamic name: `<slot :name="n" />`.
17. Non-setup `<script>` + template compile validity.
18. Unicode sourcemap fixture with emoji in template/script/style.
19. Import extraction fixture with side-effect + multiline imports.
20. Scoped style id parity fixture between monolithic `generate` and `generate_for_vite`.

9) **Suggested Benchmark Matrix**

1. Expression rewrite microbench: many dynamic props/events/interpolations; isolate `write_expr_with_ctx`.
2. Nested-scope template bench: deep `v-for`/`v-if`/`v-slot` trees to stress local-binding resolution.
3. Sourcemap Unicode bench: large non-BMP content to measure UTF-16 column path overhead.
4. End-to-end split bench (`generate_for_vite`) with large real SFC corpus; compare current import extraction vs AST import parse.
5. Tokenizer hot-path bench including `v-pre` tags; confirm dead-allocation removal win.
6. Repeated compile memory bench to catch leaks/regressions (`Box::leak` removal validation).
7. CSS transform bench for scoped/modules with large selectors; include preprocessor-like input.
8. Existing Criterion integration points: extend `crates/verter_core/benches/real_world_bench.rs`, `crates/verter_core/benches/real_world_analysis_bench.rs`, and add a new `template_codegen_bench.rs`.

I could not run Rust tests/benches locally because `cargo` is not available in this environment (`/bin/bash: cargo: command not found`).