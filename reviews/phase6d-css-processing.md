# Phase 6d: CSS Processing + Style Review

## Overall: WELL-ARCHITECTED — Clean Two-Phase Design with Scoping Edge Cases

Pragmatic prepass/normalize/scope pipeline. Dual-path design (normalized vs fast). Strong test coverage. Key issues around `:deep()` with commas and native CSS nesting.

---

## Critical Issues

### C1. `v_bind.rs::extract_v_bind()` Doesn't Handle Strings Inside Expressions
**File**: style/v_bind.rs:85-97

AST-pipeline `v_bind.rs` scanner does NOT skip quoted strings when counting parentheses (only matches `(` and `)` literally). Prepass version correctly handles quotes and backticks.

`v-bind(fn('hello)'))` would be misparsed — closing paren inside string literal counted.

**Impact**: Correctness divergence between prepass and AST-pipeline paths. Rare in practice.

---

## High Issues

### H1. CSS Modules Hashing Differs from Vue's Reference
Counter-based scheme (`{className}_{componentId}_{counter}`) vs Vue's content-hash-based approach. Consequences:
- CSS rule reordering changes hashed names (breaks caching)
- Different output from `@vue/compiler-sfc` (breaks snapshot tests, potential SSR hydration issues)

### H2. Native CSS Nesting in `process_style_fast` Produces Double-Scoped Selectors
Without lightningcss flattening, nested selectors get individually scoped:
```css
.parent[data-v-xxx] { .child[data-v-xxx] { } }
```
Instead of Vue's expected single-scope on flattened result. Documented trade-off but silently breaks specificity.

### H3. `:deep()` Without Arguments Not Properly Handled
`:deep()` with empty parens produces `[__v_deep__]` + nothing, resulting in potentially invalid selectors. Bare `:deep` without parens passes through to lightningcss (could cause parse error).

---

## Medium Issues

### M1. Comma Inside `:deep()` Splits Incorrectly
`:deep(.a, .b)` → prepass produces `[__v_deep__] .a, .b` → comma split treats `.b` as standalone selector → `.b[data-v-xxx]` instead of `[data-v-xxx] .b`.

**Impact**: Affects `:deep()` with comma-separated selectors (common in Element Plus/Ant Design overrides).

### M2. Walker Doesn't Handle `url()` with Parentheses in Values
Not an issue for normalized path (lightningcss cleans up). For `process_style_fast`, pathological `url()` could confuse state tracking.

### M3. Mixed `:global()` Within Compound Selectors
Any part containing `:global(` → entire selector treated as global. `.scoped :global(.reset)` loses scoping on `.scoped`.

### M4. No Source Map Support
Both `process_style` and `process_style_fast` always return `source_map: None`. Prepass mutations shift positions but no mappings tracked. Significant DX gap.

### M5. CSS Modules Don't Hash `:composes` References
No handling of `composes: className from './other.module.css'`. Broken references for projects using CSS Modules composition.

### M6. `@counter-style`, `@property`, `@container` Not Explicitly Handled
Currently work by accident due to `@`-prefix skip. Fragile and undocumented.

---

## Low Issues

- L1: `generate_var_name` sanitization could cause CSS variable name collisions for different expressions (theoretical)
- L2: Empty `v-bind()` produces meaningless var with empty expression
- L3: `@import` correctly handled (no issue)
- L4: Minor `var_name.clone()` allocation in prepass

---

## Strengths

### Clean Two-Phase Architecture
Vue syntax → CSS-valid markers (prepass), then lightningcss normalization, then scoped/modules transforms. Avoids teaching lightningcss about Vue syntax.

### Dual-Path Design
`process_style` (normalized via lightningcss) and `process_style_fast` (raw CSS walking). Fast path valuable for dev mode.

### Comprehensive Test Coverage
Basic/compound selectors, combinators, pseudo-classes/elements, @rules, strings with braces, comments, escaped characters, cross-comparison between paths, UTF-8 safety.

### Correct @keyframes Handling
`keyframes_depth` tracking with `keyframes_entry_depths` stack. Handles `@keyframes`, `@-webkit-keyframes`, and selectors after.

### Byte-Level Scanning with UTF-8 Safety
Only ASCII delimiters matched. UTF-8 continuation bytes cannot false-match.

### Correct Pseudo-Class/Element Scoping Order
`[data-v-xxx]` inserted before pseudo-classes/elements. Handles escaped colons and attribute selectors.

### SmallVec Usage
`SmallVec<[_; 4]>` for selector segments avoids heap allocation for typical selectors.

---

## Priority Fixes
1. **C1**: Add string/quote handling to `v_bind.rs::extract_v_bind()` (match prepass)
2. **M1**: Handle commas inside `:deep()` correctly
3. **M3**: Support mixed `:global()` in compound selectors
4. **H1**: Consider content-hash-based CSS Modules for Vue compatibility
5. **M4**: Add source map support for CSS transforms
