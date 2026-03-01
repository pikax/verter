# Phase 6g: TSX Codegen (LSP Path) Review

## Overall: STRONG Architecture — Critical Gaps in v-model and Slots

Excellent IIFE-based v-if type narrowing and condition scope tracking. Three critical unimplemented features: v-model, `<slot>` outlets, dynamic event names.

---

## Critical Issues

### C1. v-model NOT Converted to JSX — Left as Raw Vue Syntax
**File**: tsx/template/props.rs:53-69

`v-model` directive explicitly skipped with "for now" comment. `v-model="count"` passes through as invalid JSX. TypeScript cannot type-check bidirectional bindings.

**Impact**: Every component using v-model produces TypeScript errors (one of Vue's most common features).

### C2. `<slot>` Outlet NOT Converted to JSX
**File**: tsx/template/mod.rs:295-298

`<slot>` left as-is. No type-checking for slot props, scoped slot parameters, or fallback content.

**Impact**: Zero type safety for slot-based component authoring.

### C3. Dynamic Event Names Not Handled
**File**: tsx/template/props.rs:216-305

`process_v_on` doesn't check `prop.is_dynamic`. `@[eventName]="handler"` produces invalid JSX like `on[eventName]={handler}`.

---

## High Issues

### H1. v-html/v-text Binding Prefixes Not Applied
`process_v_html` and `process_v_text` directly splice raw expressions without calling `resolve_prefixed_expr`. Refs and props won't get `.value`/`__props.` prefixes.

### H2. v-show Binding Patches Orphaned
`emit_v_show` overwrites entire prop range, then `collect_binding_patches` targets original positions that no longer exist. Binding prefixes silently dropped.

### H3. `event_to_jsx_name` Doesn't CamelCase Kebab Events
`@custom-event` → `onCustom-event` (not valid JSX identifier). Should be `onCustomEvent`. Tests document this as "v5 parity" but it breaks type checking.

### H4. Companion Script (`<script>` alongside `<script setup>`) Ignored
`_normal_script` parameter prefixed with `_` and discarded. Options API definitions (`inheritAttrs`, `name`, etc.) invisible to type checker.

### H5. Duplicate Functions Between script.rs and props.rs
`get_directive_name` and `event_to_jsx_name` duplicated identically. Fix in one but not other creates drift.

---

## Medium Issues

### M1. `replace_word_boundary` Assumes ASCII
Byte-by-byte iteration with `bytes[pos] as char`. Multi-byte UTF-8 characters (valid in TS identifiers) would produce garbled output.

### M2. v-for `str::find(" in ")` Naive Parsing
Could misparse `item in items.filter(x => x.kind in types)`. Also `v-for="n in 10"` produces `.map()` on number — invalid JS.

### M3. `is_member_expr` Detection Fragile
`resolved_expr.contains('.') && !resolved_expr.contains('(')` misclassifies `a.b + c.d` and `1.5`.

### M4. v-show Expression Not Escaped for `}}`
If expression contains `}}`, breaks JSX object literal syntax.

### M5. `onUpdate:modelValue` Format Not Standard JSX
Colon in prop name not valid in JSX without quoting. Should be `"onUpdate:modelValue"={...}`.

### M6. `collect_sibling_negations` Uses `resolve_simple_expr`
Should use full expression resolution for compound conditions.

---

## Low Issues

- L1: Test helper `gen_tsx_template` uses empty bindings (only tests `_ctx.` prefix)
- L2: Condition scope O(n^2) allocation (parent chain cloned per element)
- L3: `is_simple_type_reference` arbitrary distinction (dots OK, generics not)
- L4: `PREFIX` constant (`___VERTER___`) not shared across modules

---

## Strengths

### S1. IIFE Pattern for v-if Type Narrowing
`{()=>{if(cond){...}}}` instead of ternaries. TypeScript control flow analysis narrows types inside `if` blocks. `typeof test === 'string'` correctly narrows to `string`.

### S2. Sophisticated Condition Scope Tracking
`ConditionScope` tracks parent + sibling conditions for accurate type narrowing guards. Handles nested v-if chains, v-else-if negations, and parent scope inheritance.

### S3. Source Map Preservation via Mapped Prepends
`emit_mapped_condition_expr` decomposes conditions into per-identifier segments with individual source mappings. Hover and go-to-definition work on individual identifiers within compound v-if expressions.

### S4. Dynamic Component Handling
Three strategies: static `is="div"` → rewrite tag, dynamic `:is="'div'"` with literal → rewrite tag, dynamic `:is="expr"` → hoist to temp variable.

### S5. Generic Component Support
`TsxGenericInfo` handles `<script setup generic="T extends object">` with cross-references, sanitized names, and proper constraints/defaults.

### S6. Comprehensive Type Construct Emission
Complete type model: TemplateBinding, FullContext, Comp functions, Instance types, Component constructor. Uses `PublicInstanceFromMacro`, `ExtractComponentProps`, `OmitConstructorSignature`.

### S7. v-if + v-for Interaction
Correctly detects coexistence and switches from IIFE to ternary to avoid parsing issue inside `.map()` bodies.

### S8. Good Test Coverage with Negative Assertions
Tests verify both IIFE emission AND absence of `v-if` in output.

---

## Priority Fixes
1. **C1**: Implement v-model → modelValue + onUpdate:modelValue conversion
2. **C2**: Handle `<slot>` outlet → typed slot function calls
3. **C3**: Add dynamic event name handling in `process_v_on`
4. **H1-H2**: Fix binding resolution for v-html/v-text/v-show
5. **H3**: CamelCase kebab event names in `event_to_jsx_name`
6. **H4**: Process companion script blocks
7. **H5**: Deduplicate shared functions
