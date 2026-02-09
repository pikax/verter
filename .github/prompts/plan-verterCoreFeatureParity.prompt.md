# Plan: Verter Core Vue Compiler Feature Parity

This plan provides a comprehensive comparison of verter_core against Vue's official compiler and breaks down the work into small, incremental TDD tasks. Each task follows the pattern in `CLAUDE_IMPLEMENTATION_GUIDE.md`: write failing tests first, implement minimum code, verify no regressions.

## Current State Summary

**Working Features:** v-if/v-else/v-else-if, v-for, v-bind, v-on, v-model, v-slot, v-show, v-html, v-text, v-pre, interpolation, static hoisting, handler caching, defineProps/Emits/Model/Slots/Expose/Options, source maps, TypeScript support.

**Critical Bugs:**

1. Multi-root templates not wrapped in Fragment (broken output)
2. Hardcoded component name `'App'` instead of filename
3. Missing `key` props for v-if/v-else branches
4. Missing patch flags with comments
5. Async context missing `__temp, __restore` declarations
6. Text children wrapped in array `["text"]` instead of string `"text"`
7. Unnecessary `_openBlock()` for child elements (should only be on root/conditional/loop)

**Missing Features:** Event modifiers, key modifiers, v-model modifiers, v-once, v-memo, custom directives, built-in components (Teleport, Transition, etc.), dynamic components, v-bind object spread.

---

## Step 1: Fix Fragment Wrapping for Multi-Root Templates

**Problem:** Templates with multiple root elements produce invalid JavaScript - missing Fragment wrapper and return statement.

### Sub-task 1.1: Add root element counting to state

**File:** `crates/verter_core/src/codegen/vue/template/types.rs`

Add fields to `TemplateCodegenState`:

- `root_element_count: usize` - counts root-level elements
- `is_collecting_roots: bool` - flag for first pass to count roots
- `root_elements: Vec<String>` - stores generated code for each root

### Sub-task 1.2: Detect multiple roots and wrap with Fragment

**File:** `crates/verter_core/src/codegen/vue/template/element.rs`

**Input:**

```vue
<template>
  <div>First</div>
  <div>Second</div>
  <div>Third</div>
</template>
```

**Current (broken) output:**

```javascript
export function render(_ctx, _cache) {
  return (_openBlock(), _createElementBlock("div", null, ["First"]))(
    _openBlock(),
    _createElementBlock("div", null, ["Second"]),
  )(_openBlock(), _createElementBlock("div", null, ["Third"]));
}
```

**Expected output:**

```javascript
import { Fragment as _Fragment, ... } from "vue"

export function render(_ctx, _cache) {
  return (_openBlock(), _createElementBlock(_Fragment, null, [
    _createElementVNode("div", null, "First"),
    _createElementVNode("div", null, "Second"),
    _createElementVNode("div", null, "Third")
  ], 64 /* STABLE_FRAGMENT */))
}
```

### Sub-task 1.3: Add Fragment import when needed

**File:** `crates/verter_core/src/codegen/vue/template/element.rs`

Add `Fragment as _Fragment` to imports when `root_element_count > 1`.

---

## Step 2: Fix Component Name Derivation from Filename

**Problem:** Component name is hardcoded as `'App'` instead of derived from filename.

### Sub-task 2.1: Extract component name from filename

**File:** `crates/verter_core/src/builder/codegen.rs`

**Input:** `CodegenOptions { filename: "my-component.vue" }`

**Current output:**

```javascript
const __sfc__={
__name: 'App',
```

**Expected output:**

```javascript
export default {
  __name: 'my-component',
```

### Sub-task 2.2: Handle edge cases for filename

- Remove `.vue` extension
- Handle paths like `src/components/MyComponent.vue` → `MyComponent`
- Handle kebab-case, PascalCase, camelCase filenames

---

## Step 3: Add Key Props for v-if/v-else Branches

**Problem:** v-if/v-else branches missing `key` props for Vue's diffing algorithm.

### Sub-task 3.1: Add branch index tracking to state

**File:** `crates/verter_core/src/codegen/vue/template/types.rs`

Add `conditional_branch_index: usize` to track current branch in a conditional chain.

### Sub-task 3.2: Generate hoisted key objects

**File:** `crates/verter_core/src/codegen/vue/template/directives.rs`

**Input:**

```vue
<span v-if="show">Visible</span>
<span v-else>Hidden</span>
```

**Current output:**

```javascript
_ctx.show
  ? (_openBlock(), _createElementBlock("span", null, ["Visible"]))
  : (_openBlock(), _createElementBlock("span", null, ["Hidden"]));
```

**Expected output:**

```javascript
const _hoisted_1 = { key: 0 };
const _hoisted_2 = { key: 1 };

_ctx.show
  ? (_openBlock(), _createElementBlock("span", _hoisted_1, "Visible"))
  : (_openBlock(), _createElementBlock("span", _hoisted_2, "Hidden"));
```

### Sub-task 3.3: Handle v-else-if chains

**Input:**

```vue
<span v-if="a">A</span>
<span v-else-if="b">B</span>
<span v-else-if="c">C</span>
<span v-else>D</span>
```

**Expected output:**

```javascript
const _hoisted_1 = { key: 0 };
const _hoisted_2 = { key: 1 };
const _hoisted_3 = { key: 2 };
const _hoisted_4 = { key: 3 };

_ctx.a
  ? (_openBlock(), _createElementBlock("span", _hoisted_1, "A"))
  : _ctx.b
    ? (_openBlock(), _createElementBlock("span", _hoisted_2, "B"))
    : _ctx.c
      ? (_openBlock(), _createElementBlock("span", _hoisted_3, "C"))
      : (_openBlock(), _createElementBlock("span", _hoisted_4, "D"));
```

---

## Step 4: Add Patch Flags with Comments

**Problem:** Patch flags are missing, which affects Vue's runtime optimization.

### Sub-task 4.1: Create patch flag formatting helper

**File:** `crates/verter_core/src/codegen/vue/template/types.rs`

```rust
pub fn format_patch_flag(flags: u32) -> String {
    // Returns "1 /* TEXT */" or "3 /* TEXT, CLASS */" etc.
}
```

**Flag values:**

- `1` = TEXT
- `2` = CLASS
- `4` = STYLE
- `8` = PROPS
- `16` = FULL_PROPS
- `32` = NEED_HYDRATION
- `64` = STABLE_FRAGMENT
- `128` = KEYED_FRAGMENT
- `256` = UNKEYED_FRAGMENT
- `512` = NEED_PATCH
- `1024` = DYNAMIC_SLOTS
- `-1` = CACHED
- `-2` = BAIL

### Sub-task 4.2: Add TEXT patch flag for interpolation

**Input:**

```vue
<h1>{{ msg }}</h1>
```

**Current output:**

```javascript
_createElementBlock("h1", null, [_toDisplayString(_ctx.msg)]);
```

**Expected output:**

```javascript
_createElementVNode("h1", null, _toDisplayString(_ctx.msg), 1 /* TEXT */);
```

### Sub-task 4.3: Add CLASS patch flag for dynamic class

**Input:**

```vue
<div :class="dynamicClass">Content</div>
```

**Expected output:**

```javascript
_createElementVNode("div", { class: _normalizeClass(_ctx.dynamicClass) }, "Content", 2 /* CLASS */);
```

### Sub-task 4.4: Add PROPS patch flag for dynamic props

**Input:**

```vue
<div :id="dynamicId">Content</div>
```

**Expected output:**

```javascript
_createElementVNode("div", { id: _ctx.dynamicId }, "Content", 8 /* PROPS */, ["id"]);
```

---

## Step 5: Fix Text Children Format

**Problem:** Text children wrapped in array instead of string.

### Sub-task 5.1: Use string for single text child

**Input:**

```vue
<span>Hello</span>
```

**Current output:**

```javascript
_createElementBlock("span", null, ["Hello"]);
```

**Expected output:**

```javascript
_createElementVNode("span", null, "Hello");
```

### Sub-task 5.2: Use \_toDisplayString directly without array

**Input:**

```vue
<span>{{ msg }}</span>
```

**Current output:**

```javascript
_createElementBlock("span", null, [_toDisplayString(_ctx.msg)]);
```

**Expected output:**

```javascript
_createElementVNode("span", null, _toDisplayString(_ctx.msg), 1 /* TEXT */);
```

---

## Step 6: Fix Unnecessary openBlock Calls

**Problem:** `_openBlock()` called for every element, should only be for block roots.

### Sub-task 6.1: Track block context in state

**File:** `crates/verter_core/src/codegen/vue/template/types.rs`

Add `is_block_root: bool` to track if current element is a block root.

### Sub-task 6.2: Only emit openBlock for block roots

Block roots are:

- Template root element(s)
- v-if/v-else-if/v-else branches
- v-for items
- Component roots

**Input:**

```vue
<div class="hello">
  <h1>{{ msg }}</h1>
</div>
```

**Current output:**

```javascript
(_openBlock(),
  _createElementBlock("div", _hoisted_1, [
    (_openBlock(), _createElementBlock("h1", null, [_toDisplayString(_ctx.msg)])),
  ]));
```

**Expected output:**

```javascript
(_openBlock(),
  _createElementBlock("div", _hoisted_1, [
    _createElementVNode("h1", null, _toDisplayString(_ctx.msg), 1 /* TEXT */),
  ]));
```

---

## Step 7: Implement Event Modifiers

**Problem:** Event modifiers like `.stop`, `.prevent` are parsed but not transformed.

### Sub-task 7.1: Implement .stop modifier

**Input:**

```vue
<button @click.stop="handleClick">Click</button>
```

**Expected output:**

```javascript
_createElementVNode(
  "button",
  {
    onClick: _withModifiers(_ctx.handleClick, ["stop"]),
  },
  "Click",
);
```

### Sub-task 7.2: Implement .prevent modifier

**Input:**

```vue
<form @submit.prevent="handleSubmit">...</form>
```

**Expected output:**

```javascript
_createElementVNode("form", {
  onSubmit: _withModifiers(_ctx.handleSubmit, ["prevent"])
}, ...)
```

### Sub-task 7.3: Implement .capture modifier (special naming)

**Input:**

```vue
<div @click.capture="handleClick">...</div>
```

**Expected output:**

```javascript
_createElementVNode("div", {
  onClickCapture: _ctx.handleClick
}, ...)
```

### Sub-task 7.4: Implement .once modifier (special naming)

**Input:**

```vue
<button @click.once="handleClick">Click</button>
```

**Expected output:**

```javascript
_createElementVNode(
  "button",
  {
    onClickOnce: _ctx.handleClick,
  },
  "Click",
);
```

### Sub-task 7.5: Implement .passive modifier (special naming)

**Input:**

```vue
<div @scroll.passive="handleScroll">...</div>
```

**Expected output:**

```javascript
_createElementVNode("div", {
  onScrollPassive: _ctx.handleScroll
}, ...)
```

### Sub-task 7.6: Implement .self modifier

**Input:**

```vue
<div @click.self="handleClick">...</div>
```

**Expected output:**

```javascript
_createElementVNode("div", {
  onClick: _withModifiers(_ctx.handleClick, ["self"])
}, ...)
```

### Sub-task 7.7: Implement combined modifiers

**Input:**

```vue
<button @click.stop.prevent="handleClick">Click</button>
```

**Expected output:**

```javascript
_createElementVNode(
  "button",
  {
    onClick: _withModifiers(_ctx.handleClick, ["stop", "prevent"]),
  },
  "Click",
);
```

---

## Step 8: Implement Key Modifiers

### Sub-task 8.1: Implement basic key modifiers

**Input:**

```vue
<input @keyup.enter="submit" />
```

**Expected output:**

```javascript
_createElementVNode("input", {
  onKeyup: _withKeys(_ctx.submit, ["enter"]),
});
```

### Sub-task 8.2: Implement system key modifiers (.ctrl, .alt, .shift, .meta)

**Input:**

```vue
<input @keyup.ctrl.enter="submitWithCtrl" />
```

**Expected output:**

```javascript
_createElementVNode("input", {
  onKeyup: _withKeys(_withModifiers(_ctx.submitWithCtrl, ["ctrl"]), ["enter"]),
});
```

### Sub-task 8.3: Implement .exact modifier

**Input:**

```vue
<button @click.ctrl.exact="onCtrlClick">Ctrl+Click only</button>
```

**Expected output:**

```javascript
_createElementVNode(
  "button",
  {
    onClick: _withModifiers(_ctx.onCtrlClick, ["ctrl", "exact"]),
  },
  "Ctrl+Click only",
);
```

---

## Step 9: Implement v-model Modifiers

### Sub-task 9.1: Implement .lazy modifier

**Input:**

```vue
<input v-model.lazy="value" />
```

**Expected output:**

```javascript
_withDirectives(
  _createElementVNode(
    "input",
    {
      "onUpdate:modelValue": ($event) => (_ctx.value = $event),
    },
    null,
    512 /* NEED_PATCH */,
  ),
  [[_vModelText, _ctx.value, void 0, { lazy: true }]],
);
```

### Sub-task 9.2: Implement .number modifier

**Input:**

```vue
<input v-model.number="value" />
```

**Expected output:**

```javascript
_withDirectives(
  _createElementVNode(
    "input",
    {
      "onUpdate:modelValue": ($event) => (_ctx.value = $event),
    },
    null,
    512 /* NEED_PATCH */,
  ),
  [[_vModelText, _ctx.value, void 0, { number: true }]],
);
```

### Sub-task 9.3: Implement .trim modifier

**Input:**

```vue
<input v-model.trim="value" />
```

**Expected output:**

```javascript
_withDirectives(
  _createElementVNode(
    "input",
    {
      "onUpdate:modelValue": ($event) => (_ctx.value = $event),
    },
    null,
    512 /* NEED_PATCH */,
  ),
  [[_vModelText, _ctx.value, void 0, { trim: true }]],
);
```

---

## Step 10: Implement v-once Directive

### Sub-task 10.1: Basic v-once implementation

**Input:**

```vue
<span v-once>{{ staticContent }}</span>
```

**Expected output:**

```javascript
_cache[0] ||
  (_cache[0] = _createElementVNode(
    "span",
    null,
    _toDisplayString(_ctx.staticContent),
    1 /* TEXT */,
  ));
```

---

## Step 11: Implement Custom Directives

### Sub-task 11.1: Basic custom directive

**Input:**

```vue
<input v-focus />
```

**Expected output:**

```javascript
_withDirectives(_createElementVNode("input"), [[_directive_focus]]);
```

With import: `const _directive_focus = _resolveDirective("focus")`

### Sub-task 11.2: Custom directive with value

**Input:**

```vue
<div v-tooltip="'Hello'">Hover me</div>
```

**Expected output:**

```javascript
_withDirectives(_createElementVNode("div", null, "Hover me"), [[_directive_tooltip, "Hello"]]);
```

### Sub-task 11.3: Custom directive with argument and modifiers

**Input:**

```vue
<div v-custom:arg.mod1.mod2="value">Content</div>
```

**Expected output:**

```javascript
_withDirectives(_createElementVNode("div", null, "Content"), [
  [_directive_custom, _ctx.value, "arg", { mod1: true, mod2: true }],
]);
```

---

## Step 12: Implement Built-in Components

### Sub-task 12.1: Implement Teleport

**Input:**

```vue
<Teleport to="body">
  <div class="modal">Modal content</div>
</Teleport>
```

**Expected output:**

```javascript
(_openBlock(),
  _createBlock(_Teleport, { to: "body" }, [
    _createElementVNode("div", { class: "modal" }, "Modal content"),
  ]));
```

### Sub-task 12.2: Implement Transition

**Input:**

```vue
<Transition name="fade">
  <div v-if="show">Animated</div>
</Transition>
```

**Expected output:**

```javascript
_createVNode(
  _Transition,
  { name: "fade" },
  {
    default: _withCtx(() => [
      _ctx.show
        ? (_openBlock(), _createElementBlock("div", { key: 0 }, "Animated"))
        : _createCommentVNode("v-if", true),
    ]),
    _: 1 /* STABLE */,
  },
);
```

### Sub-task 12.3: Implement KeepAlive

**Input:**

```vue
<KeepAlive>
  <component :is="currentView" />
</KeepAlive>
```

**Expected output:**

```javascript
(_openBlock(),
  _createBlock(_KeepAlive, null, [
    (_openBlock(), _createBlock(_resolveDynamicComponent(_ctx.currentView))),
  ]));
```

---

## Step 13: Implement Dynamic Components

### Sub-task 13.1: Basic dynamic component

**Input:**

```vue
<component :is="currentComponent" />
```

**Expected output:**

```javascript
(_openBlock(), _createBlock(_resolveDynamicComponent(_ctx.currentComponent)));
```

### Sub-task 13.2: Dynamic component with props

**Input:**

```vue
<component :is="currentComponent" :prop="value" @event="handler" />
```

**Expected output:**

```javascript
(_openBlock(),
  _createBlock(_resolveDynamicComponent(_ctx.currentComponent), {
    prop: _ctx.value,
    onEvent: _ctx.handler,
  }));
```

---

## Step 14: Implement v-bind Object Spread

### Sub-task 14.1: Basic v-bind spread

**Input:**

```vue
<div v-bind="attrs">Content</div>
```

**Expected output:**

```javascript
_createElementVNode(
  "div",
  _normalizeProps(_guardReactiveProps(_ctx.attrs)),
  "Content",
  16 /* FULL_PROPS */,
);
```

### Sub-task 14.2: v-bind spread with other props

**Input:**

```vue
<div :class="classes" v-bind="attrs">Content</div>
```

**Expected output:**

```javascript
_createElementVNode(
  "div",
  _mergeProps(
    {
      class: _normalizeClass(_ctx.classes),
    },
    _ctx.attrs,
  ),
  "Content",
  16 /* FULL_PROPS */,
);
```

---

## Example Files to Create

The following `.vue` files should be created in `crates/verter_core/examples/codegen/source/` for testing:

1. `multi-root.vue` - Multiple root elements (Fragment test)
2. `event-modifiers.vue` - All event modifiers
3. `key-modifiers.vue` - Keyboard event modifiers
4. `v-model-modifiers.vue` - v-model .lazy/.number/.trim
5. `v-once.vue` - Static content caching
6. `custom-directives.vue` - Custom directive usage
7. `builtin-components.vue` - Teleport, Transition, KeepAlive
8. `dynamic-component.vue` - `<component :is="...">`
9. `v-bind-spread.vue` - Object spread syntax
10. `patch-flags.vue` - Various dynamic bindings for patch flag testing

---

## Priority Order

**Phase 1 - Critical Bugs (Must Fix First):**

1. Step 1: Fragment wrapping (broken output)
2. Step 5: Text children format
3. Step 6: Unnecessary openBlock calls
4. Step 2: Component name from filename

**Phase 2 - Optimization Parity:** 5. Step 4: Patch flags with comments 6. Step 3: Key props for conditionals

**Phase 3 - Event System:** 7. Step 7: Event modifiers 8. Step 8: Key modifiers

**Phase 4 - Directives:** 9. Step 9: v-model modifiers 10. Step 10: v-once 11. Step 11: Custom directives

**Phase 5 - Advanced Features:** 12. Step 12: Built-in components 13. Step 13: Dynamic components 14. Step 14: v-bind spread
