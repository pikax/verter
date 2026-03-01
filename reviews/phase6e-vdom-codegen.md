# Phase 6e: Template Codegen VDOM Review

## Overall: PRODUCTION-QUALITY — Correct Vue 3 VDOM Specification Implementation

No critical issues. Well-architected with clean trait abstraction, correct patch flag computation, and comprehensive directive handling. Main concern: `build_props_object_into` complexity.

---

## Critical Issues

**None identified.**

---

## High Issues

### H1. `build_props_object_into` Is 500+ Lines with Deep Nesting
Handles static attrs, dynamic binds, events, v-model (component + native), spreads, merge props, normalize class/style, and event modifiers in a single function. High cyclomatic complexity and regression risk.

### H2. v-model:name.modifier Combination Coverage
Component v-model with modifiers on named models (`v-model:title.trim.lazy`) has complex string concatenation logic. Needs dedicated integration tests for all combinations.

### H3. v-for + v-if on `<template>` Test Coverage Gap
Precedence interaction between v-if and v-for on same element needs dedicated integration test to guard against regressions.

---

## Medium Issues

- M1: Vapor-specific types leak into shared `types.rs` (1004 lines)
- M2: Static hoisting flags computed but not fully utilized (in-place architecture limitation)
- M3: Dynamic key detection treats `LiteralConst` bindings as dynamic (matches Vue spec)
- M4: `_mergeProps` allocation for small cases (optimization opportunity)
- M5: Event handler classification heuristic (`contains(';')`) could misclassify template literals
- M6: Slot name deduplication is O(n^2) (fine for <10 slots)
- M7: Conditional slot detection spans multiple functions (correct but complex)
- M8: `build_prefixed_iterable` byte-level insertion brittle if OXC spans change
- M9: `format_scope_close` allocates fresh Strings (could reuse buffer)
- M10: Suspense/Teleport block semantics not explicitly verified
- M11: Text static analysis granularity limited (no module-level hoisting)
- M12: Binding resolution byte slicing lacks `debug_assert!(source.is_char_boundary())`

---

## Low Issues

- L1: CodeGenMode enum couples backends
- L2: Slot fallback whitespace overhead
- L3: Entity decoding subset (not all 2231 HTML5 entities)
- L4: Comment `*/` escaping edge case in minified output
- L5: Props binding dual-path (`__props` vs `$props`) cognitive overhead

---

## Strengths

### Architecture
- Clean `TemplateCodeGen` trait with 7 methods
- Explicit DFS stack walker (no recursion limits)
- `CodeGenOutput` with deferred batch application preserves source maps
- Pre-computed condition prefixes via HashMap eliminate redundant work

### Correctness
- All structural directives (v-if chains, v-for, v-slot)
- All runtime directives (v-show, v-model component + native, v-on with modifiers, v-bind spread)
- Block tree optimization with correct patch flag computation
- Whitespace condensation matching Vue condense mode spec
- Event modifier 3-way classification (option/runtime/key)
- Component resolution with PascalCase normalization and self-reference detection
- Dynamic slot names with `_createSlots` fallback

### Performance
- Batch application O(n+m)
- Reusable buffer pattern (`std::mem::take`)
- `VdomHelperFlags` bitflags with `trailing_zeros` for O(popcount) import generation
- Fast-path checks skip expensive processing

### Text/Interpolation
- Whitespace condensation (condense mode)
- JS string escaping (including U+2028/U+2029)
- HTML entity decoding (named, numeric, hex)
- Adjacent text merging with correct separators
- `_createTextVNode` wrapping with TEXT patch flag
