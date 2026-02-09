# Verter Compiler Gap Analysis and Implementation Plan

## Executive Summary

Analysis of 16,250 Vue files comparing Vue official compiler output vs verter output revealed **critical structural issues** that make verter's output invalid JavaScript in most cases. While verter reports 0 errored files vs Vue's 1,100, the generated code contains fundamental syntax errors that would fail parsing.

**Statistics:**

- Vue: 27s compile time, 130MB output, 1,100 errors (actual Vue parsing issues)
- Verter: 57s compile time, 74MB output, 0 errors (but invalid JS output)

---

## Critical Issues (P0) - Invalid JavaScript Syntax

These issues cause the output to fail JavaScript parsing entirely.

### 1. Component Definition Structure is Invalid

**File:** `codegen/vue/script.rs` or main component wrapper

**Current (Verter):**

```javascript
const __sfc__=_defineComponent({
__name: '1_index',props:Props),  // Invalid: no brace, type name, extra paren
emits:["blur","input"]),          // Invalid: extra paren
setup(__props,{expose:__expose}){__expose();
```

**Expected (Vue):**

```javascript
import { defineComponent as _defineComponent } from 'vue'
export default /*@__PURE__*/_defineComponent({
  __name: '1_index',
  props: {
    isTimeFilter: { type: Boolean, required: false, default: false }
  },
  emits: ["blur", "input"],
  setup(__props, { expose: __expose }) {
    __expose();
```

**Issues:**

- [ ] Missing `_defineComponent` import
- [ ] Missing `export default`
- [ ] `props:Props)` uses TypeScript type instead of runtime props object
- [ ] Extra closing parens after props and emits
- [ ] Missing proper brace structure

---

### 2. Code Placement Structure is Wrong

**File:** `codegen/vue/plugin.rs`, `codegen/vue/template/mod.rs`

**Current (Verter):**

```javascript
setup(__props,{expose:__expose}){__expose();
import { openBlock as _openBlock, ... } from "vue"  // INSIDE setup!
const _hoisted_1 = { ... }                           // INSIDE setup!
export function render(_ctx, _cache) { ... }         // INSIDE setup!
// ... script code ...
const props = _props;
return __returned__
}});
```

**Expected (Vue):**

```javascript
export default _defineComponent({
  setup(__props, { expose: __expose }) {
    __expose();
    // script code ONLY
    const props = __props;
    return __returned__
  }
})
// AFTER component:
import { ... } from "vue"
const _hoisted_1 = { ... }
export function render(_ctx, _cache) { ... }
```

**Issues:**

- [ ] Vue runtime imports placed inside setup() - invalid syntax
- [ ] Hoisted constants placed inside setup()
- [ ] Render function defined inside setup() with `export` - invalid syntax
- [ ] Script body appears after render function inside setup()

---

### 3. Invalid Negation Syntax

**File:** `codegen/vue/template/interpolation.rs` or expression handling

**Current (Verter):**

```javascript
_ctx.!isLoadingBridges && !bridges.length
```

**Expected (Vue):**

```javascript
!_ctx.isLoadingBridges && !_ctx.bridges.length;
```

**Issues:**

- [ ] `_ctx.!` is invalid JS - negation must come before the expression
- [ ] Missing `_ctx.` prefix on second variable

---

### 4. Function Definitions Inlined in Return Object

**File:** `codegen/vue/script.rs`

**Current (Verter):**

```javascript
const __returned__={props, emit, function lockWeight(keepLocked?: boolean) {
  // entire function body duplicated here
}, function onInput(event) { ... }}
```

**Expected (Vue):**

```javascript
const __returned__ = { props, emit, lockWeight, onInput };
```

**Issues:**

- [ ] Functions are defined inline in object literal - syntax error
- [ ] Should be references to already-defined functions

---

### 5. Malformed Ternary/Conditional Expressions

**File:** `codegen/vue/template/directives.rs`

**Current (Verter):**

```javascript
_toDisplayString(_ctx.formatDate(bridge.lastSeen, true))
  _ctx.!bridge.lastSeen           // Missing operator, invalid negation
? _createVNode(...)
```

**Expected (Vue):**

```javascript
[
  _createTextVNode(_toDisplayString(_ctx.formatDate(...)) + " ", 1),
  (!bridge.lastSeen)
    ? (_openBlock(), _createBlock(...))
    : _createCommentVNode("v-if", true)
]
```

**Issues:**

- [ ] Missing array wrapper
- [ ] Missing operator between expressions
- [ ] Missing `_createTextVNode` wrapper
- [ ] Invalid placement of ternary operator

---

### 6. Invalid Event Property Names

**File:** `codegen/vue/template/element.rs` (event handler generation)

**Current (Verter):**

```javascript
{ onUpdate:modelValue: _cache[2] || ... }
```

**Expected (Vue):**

```javascript
{ "onUpdate:modelValue": _cache[4] || ... }
```

**Issues:**

- [ ] Property names with colons must be quoted strings

---

## High Priority Issues (P1) - Runtime Errors

These compile but fail at runtime.

### 7. v-model on Components Uses Wrong Pattern

**File:** `codegen/vue/template/directives.rs`

**Current (Verter):**

```javascript
_withDirectives(_createVNode(_resolveComponent("BalTextInput"),
  { modelValue: _ctx._weight, ... }), [[_vModelText, _ctx._weight]])
```

**Expected (Vue):**

```javascript
_createBlock(_component_BalTextInput, _mergeProps({
  modelValue: _ctx._weight,
  "onUpdate:modelValue": _cache[4] || (_cache[4] = $event => ((_ctx._weight) = $event)),
  ...
}, _ctx.$attrs, { ... }), { ... })
```

**Issues:**

- [ ] Components use `modelValue` prop + `onUpdate:modelValue` event, NOT `_withDirectives`
- [ ] `_withDirectives`/`_vModelText` is only for native HTML inputs
- [ ] Missing `_createBlock` for component root

---

### 8. Duplicate Event Handler Logic

**File:** `codegen/vue/template/element.rs`

**Current (Verter):**

```javascript
onClick: _cache[7] ||
  (_cache[7] = (...args) => _ctx.lockWeight(false) && _ctx.lockWeight(false)(...args));
```

**Expected (Vue):**

```javascript
onClick: _cache[2] || (_cache[2] = ($event) => _ctx.lockWeight(false));
```

**Issues:**

- [ ] Handler logic duplicated and chained with `&&`
- [ ] Second part tries to call the result as a function

---

### 9. Missing `_normalizeClass` for Dynamic Classes

**File:** `codegen/vue/template/element.rs`

**Current (Verter):**

```javascript
{ class: ['static-class', {'dynamic': condition }] }
```

**Expected (Vue):**

```javascript
{ class: _normalizeClass(['static-class', { 'dynamic': condition }]) }
```

**Issues:**

- [ ] Arrays/objects for class must be wrapped in `_normalizeClass()`

---

### 10. Missing Context Prefix on Variables

**File:** `codegen/vue/template/interpolation.rs`

**Current (Verter):**

```javascript
!bridges.length; // bridges is undefined
formatDate(bridge.lastSeen); // formatDate is undefined
```

**Expected (Vue):**

```javascript
!_ctx.bridges.length;
_ctx.formatDate(bridge.lastSeen);
```

**Issues:**

- [ ] Inconsistent `_ctx.` prefixing on template expressions
- [ ] Need to identify all bindings that need `_ctx.`

---

### 11. Scoped Slot Variable Context Issues

**File:** `codegen/vue/template/directives.rs` (v-slot handling)

**Current (Verter):**

```javascript
_withCtx(({ rowData: bridge }) => [
  _createElementVNode("div", { title: _ctx.bridge.name }, ...)
])
```

**Expected (Vue):**

```javascript
_withCtx(({ rowData: bridge }: { rowData: Bridge }) => [
  _createElementVNode("div", { title: bridge.name }, ...)
])
```

**Issues:**

- [ ] Slot-scoped variables like `bridge` shouldn't have `_ctx.` prefix
- [ ] Need to track slot scope bindings

---

### 12. Missing `_createTextVNode` for Text Content

**File:** `codegen/vue/template/interpolation.rs`

**Current (Verter):**

```javascript
[_toDisplayString(data.foo)];
```

**Expected (Vue):**

```javascript
[_createTextVNode(_toDisplayString(data.foo), 1)];
```

**Issues:**

- [ ] Text content needs `_createTextVNode` wrapper in some contexts

---

### 13. Component Slots vs Children Confusion

**File:** `codegen/vue/template/element.rs`

**Current (Verter):**

```javascript
_createVNode(_resolveComponent("BalStack"), { ... }, [
  _createVNode(_resolveComponent("BalIcon"), ...)
])
```

**Expected (Vue):**

```javascript
_createVNode(_component_BalStack, { ... }, {
  default: _withCtx(() => [
    _createVNode(_component_BalIcon, ...)
  ]),
  _: 1
})
```

**Issues:**

- [ ] Component children should be slots object, not array
- [ ] Need `_withCtx` wrapper for default slot
- [ ] Need `_: 1` (STABLE) or `_: 2` (DYNAMIC) flag

---

## Medium Priority Issues (P2) - Functionality Gaps

### 14. Extra Empty Fallback on Slots

**File:** `codegen/vue/template/element.rs`

**Current (Verter):**

```javascript
_renderSlot(_ctx.$slots, "no-bind", {}, () => []);
```

**Expected (Vue):**

```javascript
_renderSlot(_ctx.$slots, "no-bind");
```

**Issues:**

- [ ] Slots without fallback content shouldn't have `() => []` parameter

---

### 15. Missing Static Caching

**File:** `codegen/vue/template/element.rs`

**Expected (Vue):**

```javascript
loadingRow: _withCtx(() => [...(_cache[1] || (_cache[1] = [
  _createElementVNode("div", { class: "..." }, [...], -1 /* CACHED */)
]))]),
```

**Issues:**

- [ ] Static content within slots should be cached
- [ ] Need `_cache[n] || (_cache[n] = [...])` pattern

---

### 16. Hoisted Constants Incomplete

**File:** `codegen/vue/template/element.rs`

**Current (Verter):**

```javascript
const _hoisted_4 = { key: 0 }; // Missing class!
```

**Expected (Vue):**

```javascript
const _hoisted_4 = { key: 0, class: "w-full h-full flex items-center justify-center" };
```

**Issues:**

- [ ] Hoisted objects missing properties
- [ ] Then re-creating objects inline, defeating hoisting purpose

---

### 17. Style Block Included in JS Output

**File:** `codegen/vue/plugin.rs`

**Current (Verter):**

```javascript
}});

<style scoped>
.ease-color { ... }
</style>
```

**Issues:**

- [ ] `<style>` blocks should be stripped from JS output

---

### 18. Component Resolution Caching

**File:** `codegen/vue/template/element.rs`

**Expected (Vue):**

```javascript
export function render(_ctx, _cache) {
  const _component_Icon = _resolveComponent("Icon")  // Cached at top
  return (_openBlock(), _createElementBlock(..., [
    _createVNode(_component_Icon, ...)
  ]))
}
```

**Current (Verter):**

```javascript
return (..., [
  _createVNode(_resolveComponent("Icon"), ...)  // Resolved inline each time
])
```

**Issues:**

- [ ] `_resolveComponent` should be called once at render function start
- [ ] Store in local variable, use variable in VNode creation

---

### 19. Missing `_openBlock()` Before `_createBlock()`

**File:** `codegen/vue/template/element.rs`

**Expected (Vue):**

```javascript
(_openBlock(), _createBlock(_component_BaseTableNew, ...))
```

**Current (Verter):**

```javascript
_createVNode(_resolveComponent("BaseTableNew"), ...)  // No openBlock
```

**Issues:**

- [ ] Root-level component/element needs `_openBlock()` call
- [ ] Use `_createBlock` instead of `_createVNode` for block roots

---

## Implementation Steps

### Phase 1: Fix Critical Structure (P0)

1. **Fix component wrapper generation**
   - Files: `codegen/vue/script.rs`, `codegen/vue/plugin.rs`
   - Add `_defineComponent` import
   - Add `export default`
   - Generate proper props object from TypeScript types
   - Fix emits array syntax

2. **Fix code placement**
   - Move Vue runtime imports to module level (after component def)
   - Move hoisted constants to module level
   - Move render function to module level
   - Keep only script code inside setup()

3. **Fix expression negation**
   - File: `codegen/vue/template/interpolation.rs`
   - Detect unary operators and emit them before `_ctx.`

4. **Fix **returned** object**
   - File: `codegen/vue/script.rs`
   - Reference functions by name, don't inline definitions

5. **Fix ternary expressions**
   - File: `codegen/vue/template/directives.rs`
   - Ensure proper array wrapping
   - Emit complete ternary expressions

6. **Fix event property names**
   - File: `codegen/vue/template/element.rs`
   - Quote property names containing special characters

### Phase 2: Fix Runtime Issues (P1)

7. **Fix v-model for components**
   - Detect component vs native element
   - Use `modelValue`/`onUpdate:modelValue` for components
   - Use `_withDirectives`/`_vModelText` only for native inputs

8. **Fix event handler generation**
   - Remove duplicate logic
   - Generate correct caching pattern

9. **Add `_normalizeClass` calls**
   - Detect array/object class bindings
   - Wrap in `_normalizeClass()`

10. **Fix context prefixing**
    - Track all scope-local bindings
    - Apply `_ctx.` only to template references

11. **Fix scoped slot variables**
    - Track slot scope bindings
    - Don't add `_ctx.` to slot-provided variables

12. **Add `_createTextVNode` wrapper**
    - Detect text content that needs wrapping
    - Apply wrapper in appropriate contexts

13. **Fix component children as slots**
    - Detect component children
    - Wrap in `{ default: _withCtx(() => [...]), _: 1 }`

### Phase 3: Fix Functionality Gaps (P2)

14. Remove extra slot fallbacks
15. Implement static content caching
16. Complete hoisted constant extraction
17. Strip `<style>` blocks from output
18. Cache `_resolveComponent` calls
19. Add `_openBlock()` before `_createBlock()`

### Phase 4: Validation

- Run full test suite after each change
- Compare output with Vue compiler for sample files
- Validate all output is parseable JavaScript using oxc
- Ensure no regressions in existing functionality

---

## Verification Plan

1. **For each change:**

   ```bash
   cargo test --package verter_core 2>&1 | tail -60
   ```

2. **After Phase 1 (structure fixes):**
   - All generated files should parse as valid JavaScript
   - Run: `node -c generated/*.verter.js` (syntax check)

3. **After Phase 2 (runtime fixes):**
   - Compare AST structure with Vue output for 10+ files
   - Run sample components in browser

4. **After Phase 3:**
   - Full comparison with Vue output
   - Performance benchmarks

---

## Critical Files to Modify

| Priority | File                                    | Issues                                   |
| -------- | --------------------------------------- | ---------------------------------------- |
| P0       | `codegen/vue/script.rs`                 | #1, #4                                   |
| P0       | `codegen/vue/plugin.rs`                 | #2, #17                                  |
| P0       | `codegen/vue/template/mod.rs`           | #2                                       |
| P0       | `codegen/vue/template/interpolation.rs` | #3, #10, #12                             |
| P0       | `codegen/vue/template/directives.rs`    | #5, #7, #11                              |
| P0       | `codegen/vue/template/element.rs`       | #6, #8, #9, #13, #14, #15, #16, #18, #19 |
| P0       | `codegen/vue/template/types.rs`         | State tracking for fixes                 |
