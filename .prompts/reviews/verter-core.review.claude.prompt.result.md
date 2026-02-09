# Verter Core — Deep Code Review Report

## 1. Executive Summary

### Top Correctness Risks

1. **Scoped CSS scopes ALL selectors in descendant chains** — Vue's official compiler only adds `[data-v-xxx]` to the last selector in a descendant chain (`.parent .child → .parent .child[data-v-xxx]`), but verter scopes every simple selector (`.parent[data-v-xxx] .child[data-v-xxx]`). This produces visually different CSS in production. (`transformer.rs:278-308`)

2. **Scope ID hash inconsistency** — `has_scoped_style()` pre-scan computes `get_hash(component_name)` but `generate_component_id()` computes `get_hash(normalized_filepath)` (prod) or `get_hash(filepath + source)` (dev). The template `data-v-xxx` attributes and the CSS `[data-v-xxx]` selectors could end up with different hashes. (`codegen.rs:518-528` vs `codegen.rs:319-336`)

3. **defineEmits return variable not tracked as a binding** — `extract_binding_metadata()` ignores DefineEmits, so `const emit = defineEmits(...)` leaves `emit` as an unknown binding. In templates, `emit` gets prefixed with `_ctx.` instead of remaining as a direct `$setup` reference, causing incorrect runtime behavior. (`script.rs:231-265`)

### Top Source Map Risks

4. **Source map columns count char, not UTF-16 code units** — The source map spec (and all JS tooling) requires columns in UTF-16 code units. `calculate_line_column()` and `generate_map()` count Rust `char` (Unicode scalar values), which differs for characters outside BMP (emoji, CJK supplementary). A file with 🎉 at column 1 would report column=1 but JS sees column=2. (`source_map.rs:284-305`)

5. **O(n²) source map generation** — `calculate_line_column()` scans from the start of the original source for every chunk boundary. For a file with 500 chunks (common with many elements), this re-scans the entire source 500 times. (`source_map.rs:284-305`)

### Top Performance/Memory Risks

6. **Box::leak of SyntaxPluginOptions in both generate() and generate_for_vite()** — Every compilation call leaks ~48 bytes. In LSP/watch mode this is an unbounded memory leak. (`codegen.rs:495-496`, `codegen.rs:649-650`)

7. **ensure_split_at() in CodeTransform is O(n) per call** — Linear scan through all chunks for every overwrite/remove/prepend/append. With many operations, this becomes O(n²). (`code_transform.rs`)

### Additional Key Findings

8. **119 .unwrap() calls + 60+ panic!() calls in non-test code paths** — Many are in the OXC parser plugin's test functions, but several are in production code paths (tokenizer: 25, syntax: 2, element codegen: 8).

9. **Hand-rolled expression prefixer instead of OXC AST** — `transform_expr_with_ctx()` in `interpolation.rs:171-267` parses expressions character-by-character. Misses edge cases like escaped backslashes in strings (`"\\\""`), regex literals, and complex destructuring.

10. **Incomplete is_reserved_word() list** — Missing async, await, default, switch, case, try, catch, finally, throw, return, for, while, do, if, else, break, continue, class, const, let, var, function, yield, import, export, from, of, with, debugger, super, extends, implements, interface, package, private, protected, public, static. Also missing some Web APIs (Symbol, Map, Set, Promise, Error, RegExp, Proxy, Reflect, WeakMap, WeakSet, globalThis, setTimeout, setInterval, fetch, URL, Request, Response). (`interpolation.rs:280-322`)

11. **CSS parser plugin's v-bind expressions have zero-valued spans** — `var_name_start` and `var_name_end` are hardcoded to 0 in `transform_v_bind()`. (`transformer.rs:217-218`)

12. **Box::leak in test-only helper functions** — While less critical than production code, `Box::leak` in `vslot.rs:267`, `vfor.rs:406`, `slot.rs:114`, `vfor.rs:149` leaks memory in tests.

---

## 2. Pipeline Diagram

```text
┌─────────────────────────────────────────────────────────────────┐
│  ENTRY POINTS (builder/codegen.rs)                              │
│                                                                 │
│  generate(input, options, allocator) → CodegenResult             │
│  generate_for_vite(input, options, allocator) → ViteCodegenResult│
└─────────┬───────────────────────────────┬───────────────────────┘
          │                               │
          ▼                               ▼
┌─────────────────┐            ┌──────────────────────┐
│ Pre-scan Phase   │            │ Pre-scan Phase       │
│                  │            │                      │
│ has_scoped_style │            │ has_scoped_style     │
│ pre_scan_script_ │            │ pre_scan_script_     │
│ setup_bindings   │            │ setup_bindings       │
│ ScriptDetector   │            │ ScriptDetector       │
└────────┬────────┘            └──────────┬───────────┘
         │                                │
         ▼                                ▼
┌────────────────────────────────────────────────────────────────┐
│  TOKENIZER (tokenizer/byte.rs)                                 │
│  tokenize(bytes, callback) → Events (Tag, Prop, Text, etc.)    │
└────────────────────────┬───────────────────────────────────────┘
                         │
                         ▼
┌────────────────────────────────────────────────────────────────┐
│  SYNTAX PIPELINE (syntax/syntax.rs)                            │
│  Syntax { plugins: Vec<&mut dyn SyntaxPlugin> }                │
│                                                                │
│  Event flow: Tokenizer → handle() → emit() → Plugin chain     │
│                                                                │
│  Pipeline order (generate):                                    │
│  ┌──────────────────┐                                         │
│  │ 1. CssParserPlugin │  Detects <style> → CssStyleContent    │
│  │    css_parser.rs    │  Extracts scoped/module/lang attrs    │
│  └────────┬───────────┘                                       │
│           ▼                                                    │
│  ┌──────────────────┐                                         │
│  │ 2. OxcParserPlugin│  Parses JS/TS expressions with OXC     │
│  │    oxc_parser.rs   │  Produces OxcInterpolation, OxcVFor,   │
│  │                    │  OxcVSlot, OxcVConditional, OxcProp,   │
│  │                    │  OxcScriptContent                      │
│  └────────┬───────────┘                                       │
│           ▼                                                    │
│  ┌──────────────────┐                                         │
│  │ 3. Analysis       │  Scope/binding tracking                 │
│  │    analysis.rs    │  Produces Analysed* events              │
│  │                   │  (AnalysedVFor, AnalysedVSlot, etc.)    │
│  └────────┬──────────┘                                        │
│           ▼                                                    │
│  ┌──────────────────┐                                         │
│  │ 4. VueCodegenPlugin│  Template → render function codegen    │
│  │    plugin.rs       │  Script → component definition         │
│  │                    │  CSS → __css__ export                   │
│  └────────────────────┘                                       │
│                                                                │
│  Pipeline order (generate_for_vite):                           │
│  Same 1-3 above, but step 4 replaced with:                    │
│  ┌──────────────────┐  ┌──────────────────┐  ┌────────────┐  │
│  │ ScriptCodegen    │  │ TemplateCodegen   │  │ StyleCodegen│  │
│  │ script_plugin.rs │  │ template_plugin.rs│  │ style_     │  │
│  │                  │  │                   │  │ plugin.rs  │  │
│  └──────────────────┘  └──────────────────┘  └────────────┘  │
│  (run in parallel, each owns a CodeTransform instance)         │
└────────────────────────────────────────────────────────────────┘
                         │
                         ▼
┌────────────────────────────────────────────────────────────────┐
│  OUTPUT                                                        │
│  CodeTransform.to_string() → generated code                    │
│  CodeTransform.generate_map() → source map JSON                │
│                                                                │
│  NAPI Boundary (verter_napi/src/lib.rs):                       │
│  PositionResolver converts UTF-8 byte offsets → UTF-16         │
└────────────────────────────────────────────────────────────────┘
```

---

## 3. Codegen Deep Dive

### 3.1 Template Elements (`element.rs`)

Pattern: **"Store in Open, Emit in Close"** — directives/props are accumulated on `CurrentElement` during `OpenTagStart→props→OpenTagEnd`, then the element opening code is emitted at `OpenTagEnd`. Closing code is emitted at `CloseTag` or `AnalysedCloseScopes`.

#### Key functions

- `process_open_tag_end()` (L21-1135): Root element tracking, conditional chain closing, sibling comma logic, single child optimization, v-for/v-if/v-slot wrapping, element/component/slot opening, props emission, patch flags, self-closing handling
- `process_close_tag()` (L1138-1323): Component/slot/element closing, patch flag suffix, directive array emission
- `process_close_scopes()` (L1326-1798): Scope-based closings for v-for/v-slot, duplicates much of `process_close_tag` logic
- `finalize_template()` (L2134-2370): Multi-root fragment wrapping, import generation, render function wrapping
- `calculate_patch_flags()` (L4084-4178): Patch flag computation per element

**Design quality:** The 6400-line file is large but well-structured. The store-in-open/emit-in-close pattern is correct for streaming. However, `process_close_tag` and `process_close_scopes` have significant code duplication — the directive emission logic is repeated in both paths.

---

### 3.2 Directives (`directives.rs`)

Clean delegation pattern — each directive handler stores info on `CurrentElement` and removes the directive text from source. Actual code generation is deferred to `element.rs`.

**Finding:** v-model default name fallback uses `Span::new(event.start, event.name_end)` which would capture `v-model` literal text, not `"modelValue"`. This is likely overridden in `element.rs`'s prop emission, but could cause incorrect spans if used directly. (`directives.rs:311-315`)

---

### 3.3 Interpolations (`interpolation.rs`)

`{{ expr }}` → `_toDisplayString(_ctx.expr)` with three paths:

1. Simple concatenation with `+` (when previous sibling is text, no element children)
2. Direct `_toDisplayString(...)` (single child or first child)
3. `_createTextVNode(_toDisplayString(...), 1)` (array mode with element siblings)

#### Critical finding in expression prefixer (`transform_expr_with_ctx`, L171-267)

- String escape handling only checks `chars[i-2]` for backslash — fails on `"\\\""` (escaped backslash followed by escaped quote)
- No handling for regex literals (`/pattern/g`)
- Template literal nesting has no depth limit (recursive calls)
- `is_reserved_word` is missing ~50 JS keywords and global objects

---

### 3.4 Script (`script.rs`)

`process_script()` transforms `<script setup>` into a component definition.

- Dev mode uses `__sfc__` wrapper with `__returned__` object
- Prod mode generates inline `_defineComponent()` with setup function

`extract_binding_metadata()` categorizes bindings:

- Imports → `BindingType::Setup`
- Declarations → `BindingType::Setup`
- defineProps → `BindingType::Props`
- defineModel → `BindingType::Setup`

Missing: `defineEmits`, `defineExpose`, `defineSlots` bindings not tracked

---

### 3.5 Production vs Development Branches

- **Production:** `_ctx.` → `""` for Setup bindings (closure captures), `__props.` for Props
- **Development:** `_ctx.` prefix for all template identifiers
- **Render function:**
  - Prod uses `(_ctx, _cache) => { ... }`
  - Dev uses named function `render(_ctx, _cache, $props, $setup, $data, $options) { ... }`

---

## 4. Source Map Audit

### 4.1 Column Encoding Bug

`source_map.rs:150`: `generated_column += 1` per char. This counts Unicode scalar values, not UTF-16 code units.

**Impact:** Any source position after a supplementary Unicode character (emoji, CJK ext-B, etc.) will have an off-by-N column error where N = number of surrogate pairs before that position on the same line.

The NAPI boundary (`verter_napi/src/lib.rs`) correctly converts offset values to UTF-16 using `PositionResolver`, but the internal source map columns are generated before that boundary is reached.

---

### 4.2 Performance

`calculate_line_column()` iterates from byte 0 every time. Called once per Original chunk and once per Edited-with-original chunk. For a large template with 200 elements, expect ~400 chunks → 400 full-source scans.

**Fix:** pre-compute a line→offset index array (same pattern as `PositionResolver.find_lines_memchr_bump_vec()`).

---

### 4.3 Composition

No source map composition is performed. If the tokenizer adjusts positions or if CSS is processed by LightningCSS (which produces its own source map), those maps are not composed with CodeTransform's map.

Currently CSS uses its own CodeTransform instance in StyleCodegenPlugin, which handles CSS source maps independently — this is correct for Vite-mode since each block has its own map.

---

## 5. Binding/Scope Analysis Audit

### 5.1 BindingMetadata Lookup

`types.rs`: `BindingMetadata::get()` does a linear scan over entries: `Vec<(Span, BindingType)>`, comparing `source[span.start..span.end]` against the query bytes. For N bindings and M lookups, this is `O(N*M)`.

**Recommendation:** Use a `HashMap<&[u8], BindingType>` or sort + binary search.

---

### 5.2 Scope Tracking

The Analysis plugin (`analysis.rs`) correctly maintains a scope stack for v-for/v-slot/v-if. Bindings are inherited from parent scopes. The `vfor_locals_stack` in TemplateCodegenState is pushed/popped correctly.

---

### 5.3 Prefix Resolution

`resolve_binding_prefix()` returns:

- Props → `_ctx.` (dev) / `__props.` (prod)
- Setup → `_ctx.` (dev) / `""` (prod — closure captures)
- Unknown → `_ctx.`

**Risk:** If a binding is not in BindingMetadata (e.g., defineEmits return), it defaults to `_ctx.`. In prod mode this means `_ctx.emit(...)` instead of `$setup.emit(...)`, which is incorrect.

---

## 6. Vite Output / Import Extraction Audit

### 6.1 Block Splitting

`generate_for_vite()` uses three separate plugins:

- `ScriptCodegenPlugin`
- `TemplateCodegenPlugin`
- `StyleCodegenPlugin`

Each owns its own CodeTransform. This is clean — each block gets independent code + source map.

---

### 6.2 Import Extraction

`extract_imports()` (`codegen.rs:205-284`) is a hand-rolled parser that scans for `import { ... } from "..."` at line starts. It handles the common generated patterns correctly but:

- Only handles `import { ... } from` (not import default or `import * as`)
- Assumes imports are on a single line (no multi-line imports)
- The `unwrap_or('"')` for quote detection (`codegen.rs:242`) means non-ASCII first chars would default to `"` — unlikely but fragile

These limitations are acceptable since the function only parses its own generated output, not arbitrary user code.

---

### 6.3 UTF-16 Conversion at NAPI

The NAPI boundary (`compile_for_vite()`) creates PositionResolver per call and converts all byte offsets in BlockOutput/BlockImport to UTF-16. This is correct for VS Code/LSP consumption.

---

## 7. Style Handling Audit

### 7.1 CRITICAL: Scoped CSS Selector Transformation Bug

`transformer.rs:278-308`: `add_scope_to_selector()` splits by combinators and calls `scope_simple_selector()` on every segment:

```rust
while let Some(c) = chars.next() {
    match c {
        ' ' | '>' | '+' | '~' => {
            result.push_str(&self.scope_simple_selector(&current_simple));
            // ...
        }
    }
}
result.push_str(&self.scope_simple_selector(&current_simple)); // last one too
```

This means:

- `.parent .child { }` becomes `.parent[data-v-xxx] .child[data-v-xxx] { }`

Vue's official behavior: only the last selector gets the scope attribute:

- `.parent .child { }` → `.parent .child[data-v-xxx] { }`

This is a significant correctness bug that will cause styles to not apply when child elements don't have the scope attribute (e.g., elements inside child components or slots).

---

### 7.2 CSS Modules

`transformer.rs:553-731`: Hash format is `_{className}_{componentId}{counter}`. This differs from Vue CLI/Vite's default CSS module hash strategy, which uses content-hash-based names. However, since this is configurable in real projects, the difference is acceptable as long as the mapping is exposed correctly.

---

### 7.3 v-bind() in CSS

`transformer.rs:180-228`: Transforms:

- `v-bind(expr)` → `var(--{scopeId}-{sanitized_expr})`

The `CssVBindExpression.var_name_start/end` are hardcoded to 0 — these spans are needed downstream for proper source mapping but are never set correctly.

---

### 7.4 Missing CSS Parser AST Usage

The code has a TODO comment:

```rust
// TODO this should use the CSS AST and remove through there....
```

(`transformer.rs:481`)

The current hand-rolled CSS parser doesn't handle:

- Nested CSS (CSS Nesting spec)
- `@layer` rules
- `@container` queries
- Complex `:is()`, `:where()`, `:has()` selectors containing class selectors

---

## 8. Security & Robustness Audit

### 8.1 Panics in Production Code

The codebase has 60+ `panic!()` calls outside `#[cfg(test)]` modules. Most are in:

- `oxc_parser.rs` (30+ panics in AST matching code) — these handle "impossible" states from OXC output but would crash the LSP/Vite if triggered
- `syntax.rs` (5 panics in test assertion helpers and directive handling)
- `ast_vue.rs` (1 panic)

**Recommendation:** Replace all non-test panics with Result returns or `log::error!()` + graceful degradation.

---

### 8.2 Unwraps in Production Code

119 total `.unwrap()` calls across 18 files. Key hot spots:

- `tokenizer/byte.rs`: 25 unwraps
- `codegen.rs`: 11 unwraps (some in `extract_imports` parsing its own output)
- `element.rs`: 8 unwraps
- `analysis.rs`: 7 unwraps

---

### 8.3 Recursion Depth

`transform_expr_with_ctx()` in `interpolation.rs:219` recursively calls itself for template literal `${...}` expressions. A malicious template like:

```js
{{ `${`${`${...}`}`}` }}
```

could cause stack overflow. No depth limit is enforced.

---

### 8.4 Input Validation

- `has_scoped_style()` does byte-level scanning without checking for `<style` inside strings or comments
- `pre_scan_script_setup_bindings()` similarly scans for `<script` patterns without parsing context

These could produce false positives on unusual inputs (e.g., HTML comments containing `<style scoped>`), but the impact is limited to unnecessarily computing a scope ID.

---

## 9. Performance & Memory Audit

### 9.1 Memory Leak

`Box::leak` at `codegen.rs:496` and `codegen.rs:650` leaks `SyntaxPluginOptions` on every compilation call.

**Fix:** use a scoped reference or OnceCell.

---

### 9.2 CodeTransform Hot Paths

- `ensure_split_at()`: O(n) linear scan per operation — impacts all `overwrite()`, `remove()`, `prepend_left/right()`, `append_left/right()` calls
- `find_insert_position_*()`: also O(n)

Example cost:

- template with 100 elements × ~5 operations each = 500 operations
- each scanning up to 500 chunks
- total ~250,000 iterations

---

### 9.3 Source Map Generation

As noted in §4.2:

- `calculate_line_column()` is O(source_len) per call
- called O(chunks) times

Total: O(source_len × chunks)

---

### 9.4 Allocations

- `TemplateCodegenState` has ~20 HashMap fields — each HashMap allocates separately
- PropEntry vectors are created per element, then dropped
- CSS module transformation builds a `HashMap<String, String>` per class name
- `to_import_string()` on HelperFlags allocates a new Vec and String per call

---

### 9.5 String Allocations in Codegen

`process_open_tag_end()` builds multiple Strings per element for props, patch flags, and dynamic props. These are passed to `code_transform.overwrite()` which copies them.

**Recommendation:** Consider using `write!` directly into the CodeTransform.

---

## 10. Prioritized Issues & Recommendations

| #   | Priority    | Symptom                                                       | Root Cause                                                                       | File:Line                | Fix                                                                               | Test                                                                                       | Perf Impact          |
| --- | ----------- | ------------------------------------------------------------- | -------------------------------------------------------------------------------- | ------------------------ | --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | -------------------- |
| 1   | P0-Critical | Scoped styles apply to all ancestor elements, not just target | add_scope_to_selector scopes every segment in descendant chain                   | transformer.rs:278-308   | Only scope the last simple selector in each compound selector                     | `.parent .child → .parent .child[data-v-xxx]` not `.parent[data-v-xxx] .child[data-v-xxx]` | Low                  |
| 2   | P0-Critical | Template data-v-xxx ≠ CSS [data-v-xxx] in some configs        | has_scoped_style hashes component_name but generate_component_id hashes filepath | codegen.rs:518-528       | Use generate_component_id() result for scope ID too, or pass filepath to pre-scan | Add E2E test comparing template attr ID vs CSS selector ID                                 | None                 |
| 3   | P1-High     | defineEmits return var gets \_ctx. prefix in templates        | extract_binding_metadata ignores DefineEmits case                                | script.rs:231-265        | Add DefineEmits binding as BindingType::Setup                                     | Test `{{ emit('change') }}` in template                                                    | None                 |
| 4   | P1-High     | Source map columns wrong after emoji/supplementary chars      | calculate_line_column counts char not UTF-16 code units                          | source_map.rs:284-305    | Count `ch.len_utf16()` instead of 1 for each char                                 | Test SFC with emoji in template expression                                                 | None                 |
| 5   | P1-High     | LSP memory grows without bound                                | Box::leak(SyntaxPluginOptions) per compilation                                   | codegen.rs:495-496       | Scoped reference, OnceCell, or stack allocation                                   | Monitor RSS in watch mode                                                                  | +48 bytes/call saved |
| 6   | P1-High     | \_ctx.async or \_ctx.class in expressions                     | is_reserved_word() missing ~50 JS keywords                                       | interpolation.rs:280-322 | Add all ES2024 keywords and standard global objects                               | Test `{{ typeof x }}`, `{{ async () => {} }}`                                              | None                 |
| 7   | P2-Medium   | Crash on malformed template expressions                       | 60+ panic!() in non-test OXC parser code                                         | oxc_parser.rs            | Replace with Result/graceful fallback                                             | Fuzz with malformed expressions                                                            | None                 |
| 8   | P2-Medium   | Slow compilation on large templates                           | calculate_line_column() O(n²)                                                    | source_map.rs:284-305    | Pre-compute line offset index                                                     | Benchmark 1000-element template                                                            | Significant          |
| 9   | P2-Medium   | Slow compilation on large templates                           | ensure_split_at() O(n) per operation                                             | code_transform.rs        | Use BTreeMap or sorted index for chunks                                           | Benchmark 1000-element template                                                            | Significant          |
| 10  | P2-Medium   | CSS v-bind source positions lost                              | var_name_start/end hardcoded to 0                                                | transformer.rs:217-218   | Compute actual output positions                                                   | Test v-bind mapping spans                                                                  | None                 |
| 11  | P2-Medium   | Expression prefixing wrong on complex strings                 | Escape handling only checks 1 char back                                          | interpolation.rs:196     | Track escape state properly or use OXC AST                                        | Test `{{ "\\""` }}` in template                                                            | None                 |
| 12  | P3-Low      | Stack overflow on deeply nested template literals             | No recursion depth limit in transform_expr_with_ctx                              | interpolation.rs:219     | Add depth counter, bail at reasonable limit                                       | Craft 100-level nested template literal                                                    | None                 |
| 13  | P3-Low      | Nested CSS selectors not scoped correctly                     | Hand-rolled CSS parser doesn't support CSS Nesting                               | transformer.rs           | Use LightningCSS AST for selector transformation                                  | Test `.parent { .child { } }`                                                              | None                 |
| 14  | P3-Low      | Code duplication in close paths                               | process_close_tag and process_close_scopes duplicate directive emission          | element.rs:1138-1798     | Extract shared emit_close_directives() helper                                     | N/A                                                                                        | None                 |

---

## 11. Suggested Test Matrix

| #   | SFC Description                                                            | Tests                                                       | Coverage Gap       |
| --- | -------------------------------------------------------------------------- | ----------------------------------------------------------- | ------------------ |
| 1   | Single root `<div>{{ msg }}</div>` with `<script setup>`                   | Basic render function, `_ctx.msg` prefix                    | Baseline           |
| 2   | Multi-root template (div + span + comment)                                 | Fragment wrapping, STABLE_FRAGMENT flag, cache indices      | Multi-root         |
| 3   | `v-for="item in items"` with `:key="item.id"`                              | `_renderList`, KEYED_FRAGMENT, iterator locals not prefixed | v-for keying       |
| 4   | Nested v-for (outer + inner loop)                                          | vfor_locals_stack correct shadowing                         | Scope nesting      |
| 5   | v-if/v-else-if/v-else chain                                                | Ternary generation, comment vnode fallback                  | Conditionals       |
| 6   | Component with named slots `<Comp><template #header>...</template></Comp>` | `_withCtx`, slot function, `_: 1 /* STABLE */`              | Slots              |
| 7   | `<style scoped>` with `.parent .child` selector                            | Scope attr only on last selector                            | CSS scoping bug    |
| 8   | `<style scoped>` with `:deep()`, `:slotted()`, `:global()`                 | Correct transformations                                     | Special selectors  |
| 9   | Template with emoji `<div>{{ "🎉" + msg }}</div>`                          | UTF-16 column accuracy                                      | Source map bug     |
| 10  | `<script setup>` with `const emit = defineEmits(...)` + `{{ emit }}`       | `$setup.emit` in prod, not `_ctx.emit`                      | Binding bug        |
| 11  | v-model on input/select/textarea/component                                 | Correct vModel variants                                     | v-model variants   |
| 12  | Dynamic component `<component :is="currentTab">`                           | `_resolveDynamicComponent`                                  | Dynamic components |
| 13  | v-once on static element                                                   | Cache wrapper `_cache[N]`                                   | Caching            |
| 14  | CSS modules `<style module>` + `<style module="custom">`                   | hashing + `$style` binding                                  | CSS modules        |
| 15  | v-bind() in `<style scoped>` with `v-bind(theme.color)`                    | var replacement + extraction                                | CSS v-bind         |
| 16  | Event handler variants                                                     | caching/hydration correctness                               | Event handling     |
| 17  | Spread props `v-bind="obj"`                                                | FULL_PROPS flag, spread emission                            | Spread             |
| 18  | Custom directive `v-tooltip="msg"`                                         | `_withDirectives`, `_resolveDirective`                      | Custom directives  |
| 19  | TS setup lang + defineProps type param                                     | type-only imports skipped                                   | TypeScript         |
| 20  | Expression edge cases                                                      | typeof, optional chaining, literals                         | Expression cases   |
| 21  | Large template (100+ elements)                                             | no panics, reasonable perf                                  | Stress             |
| 22  | Vite mode split correctness                                                | block-level maps/imports                                    | Vite output        |

---

## 12. Suggested Benchmark Matrix

| #   | Benchmark                            | Purpose                              | Key Metric                    |
| --- | ------------------------------------ | ------------------------------------ | ----------------------------- |
| 1   | 10-element template, 100 iterations  | Baseline throughput                  | ops/sec                       |
| 2   | 100-element template with v-for/v-if | Codegen scaling                      | ms/compile                    |
| 3   | 500-element template (stress test)   | ensure_split_at + source map scaling | ms/compile, O(n) verification |
| 4   | Template with 50 interpolations      | Expression prefixer throughput       | ms/compile                    |
| 5   | Large `<style scoped>` (200 rules)   | CSS transformer throughput           | ms/compile                    |
| 6   | generate() vs generate_for_vite()    | Vite overhead                        | ms delta                      |
| 7   | Memory: 1000 sequential compilations | Box::leak growth measurement         | RSS delta                     |
| 8   | Source map: 200-line template        | calculate_line_column() hot path     | ms for map vs code            |
