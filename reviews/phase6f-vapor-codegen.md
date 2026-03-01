# Phase 6f: Template Codegen Vapor Review

## Overall: SOLID V1 Implementation — Named Slots & Built-ins Missing

Vapor v1 is substantially more complete than v2 (experimental). Correct Vue 3.6 Vapor runtime API targeting. Main gaps: named/scoped slots, built-in components, source maps.

---

## Critical Issues

**None identified.** Core architecture is sound and implemented features produce correct output.

---

## High Issues

### H1. Named/Scoped Slots Not Implemented in Vapor v1
Only default slot closures generated. `<template v-slot:header>` and slot props (`v-slot:default="{ item }"`) not handled. Significant gap for real-world component usage.

### H2. Missing Transition/KeepAlive/Teleport Built-in Component Handling
Treated as regular components via `_resolveComponent`. Won't work correctly at runtime.

### H3. Duplicated Code Between Vapor v1 and v2
`DELEGATABLE_EVENTS`, `is_member_expression`, `to_pascal_case`, `compute_dom_child_index`, `build_open_tag`, `close_html_tag` all duplicated.

---

## Medium Issues

### M1. Source Maps Lost for Vapor Output
Entire template replaced with single `overwrite()` → all generated code maps to template start position. VDOM mode preserves positions via in-place edits.

### M2. v-text Directive Silently Skipped
`continue` statement means `v-text="expr"` produces no output. Should emit `_renderEffect(() => _setText(nN, expr))`.

### M3. `<template v-if>`/`<template v-for>` Fragment Wrappers Not Handled
Only handles v-if/v-for on concrete elements (`depth == 0` checks). Fragment pattern needs separate handling.

### M4. `<component :is="expr">` Not Handled in v1
Dynamic components not supported. v2 handles via `_resolveDynamicComponent`.

### M5. No OXC Binding Resolution in v2 Event Handlers
Handler expressions use raw source text without resolving bindings.

### M6. v2 HTML Minimization Doesn't Match Vue 3.6
Always quotes attribute values and emits ` />` for self-closing. Vue 3.6 uses unquoted values and `>`.

---

## Low Issues

- L1: `is_member_expression` overly simplistic (ASCII only)
- L2: v2 `get_input_type` returns `Option<String>` instead of `Option<&str>`
- L3: v2 tests use Vapor mode for `make_options()` (not Vapor2-specific paths)

---

## Vapor v1 vs v2 Comparison

| Aspect | Vapor v1 | Vapor2 |
|--------|----------|--------|
| Variable naming | Sequential counters (n0, n1, t0) | NodeId-based (n3, n7) |
| State model | Element state stack with pool recycling | Scope-stack with buffer swapping |
| HTML minimization | Vue 3.6 compliant | Not implemented |
| Component API | `_createComponentWithFallback` | `_createComponent` (older) |
| Event handling | OXC binding resolution | Raw source text |
| v-model | Full classification with modifiers | Full classification with modifiers |
| force_js support | Yes | No |
| Const prop optimization | Yes | No |
| Source map support | Via `build_prefixed_expr` | No |

**Recommendation**: Vapor v1 is the production implementation. v2 should be considered an architectural experiment. Useful innovations (NodeId naming, scope-stack) should be backported to v1.

---

## Missing Features (Vapor v1)

| Feature | Impact |
|---------|--------|
| Transition/TransitionGroup | High |
| KeepAlive | High |
| Named/scoped slots | High |
| Dynamic component (`<component :is>`) | Medium |
| Teleport | Medium |
| Suspense | Medium |
| Custom directives | Medium |
| `<template v-if/v-for>` fragments | Medium |
| Dynamic `:key` spread (`v-bind="obj"`) test | Medium |
| v-text | Low |
| Source maps | Medium |

---

## Strengths

1. **Excellent memory management** — VaporElementState pool retains Vec capacities; bump allocation
2. **Clean module separation** — element/props/interpolation/text/comment each single responsibility
3. **Vue 3.6 HTML minimization** — trailing close tag stripping, unquoted attrs, `<br>` not `<br/>`
4. **Well-documented module headers** — vapor mod.rs provides excellent codegen strategy overview
5. **Cross-file const prop optimization** — detects const props, emits one-time statements
6. **Correct v-once handling** — effects without `_renderEffect` wrapper
7. **Event delegation** — correct delegatable events list, modifier classification, `_createInvoker`
8. **Comprehensive test coverage** — ~1025 lines for v1, ~1124 lines for v2

### Missing Test Edge Cases
- Adjacent text + interpolation coalescing ordering
- Entity-encoded text
- Comments affecting DOM child index
- Dynamic `:key` compound expression
- v-on="objectExpression" spread
- SVG attributes
- Multi-root with mixed static/dynamic effects
