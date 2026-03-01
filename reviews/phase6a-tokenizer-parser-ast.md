# Phase 6a: Tokenizer + Parser + AST Review

## Overall: HIGH-QUALITY — Solid Foundation with No Critical Issues

SIMD-accelerated tokenizer, clean state machine parser, O(1) arena navigation. Comprehensive edge case handling and test coverage.

---

## Critical Issues

**None identified.**

---

## High Issues

**None identified.**

---

## Medium Issues

### M1. Silent Fallthrough for Custom Block Tag Names >34 Characters
**File**: tokenizer/byte.rs:810-822

`rcdata_custom_seq` buffer is 36 bytes. Tag names >34 chars silently fall through without entering RCDATA mode → content parsed as HTML. Extremely unlikely but silent failure.

### M2. `resolve_root_kind` Is Case-Sensitive While Tokenizer Is Case-Insensitive
**File**: parser/mod.rs:269-276

`resolve_root_kind` does byte-exact match (`b"template"`, `b"script"`, `b"style"`). `<Script>` or `<TEMPLATE>` treated as Unknown. Vue SFCs always use lowercase, but this is an inconsistency with the close-tag matching which is case-insensitive.

### M3. Dead Structural Directive Skip Code in Vapor Codegen
**Files**: vapor/mod.rs:814-826, vapor/props.rs:132-139

Iterates `el.props` checking for v-if/v-for/v-slot/v-once by name with `continue`. These directives are **taken** during parsing and never present in `el.props`. Dead code could mislead future developers.

---

## Low Issues

- L1: Double arena lookup in `next_sibling`/`prev_sibling` (node read twice)
- L2: `v-once` in `needs_expression_parsing` triggers unnecessary OXC parsing
- L3: `close_element` panics on empty stack (invariant-guaranteed but no graceful fallback)
- L4: `set_tag_open_end` silently returns on empty stack in release mode
- L5: No duplicate `<template>` root detection (second overwrites first)
- L6: Dead TSX structural directive check in props.rs
- L7: Attributes on nested elements inside non-template roots silently dropped

---

## Strengths

### Tokenizer
- **SIMD-accelerated scanning** via `memchr`/`memmem` for 3x throughput
- **Clean state machine** — only two top-level dispatch states, all others via direct function calls
- **Thorough v-pre handling** — dual-phase prepass with depth tracking
- **SFC mode RCDATA** for custom blocks (prevents `Array<string>` from opening tags)
- **Lightweight entity detection** — only boundaries identified, decoding deferred

### Parser
- **Robust close-tag validation** — case-insensitive, void tag handling, orphan/mismatch diagnostics
- **EOF recovery** — force-closes all unclosed elements, emits diagnostics, produces usable AST
- **v-condition adjacency validation** — correctly skips comments and whitespace-only text
- **Prop capacity reuse** — `drain(..).collect()` preserves Vec capacity

### AST Arena
- **O(1) navigation** verified — parent, children, siblings all constant-time
- **Boxing optimization** — `ElementNode` ~392 bytes boxed, reduces Vec element size from ~416 to ~40 bytes (24% improvement measured)
- **SmallVec<[NodeId; 4]>** — 78% of elements have ≤4 children (measured across 10k+ .vue files)

### Builder
- **Stack-based incremental construction** — leaves immediately attached, elements stacked
- **`set_cached_directive` pattern** — single generic helper, first-occurrence-wins, duplicate detection
- **`compute_children_meta`** — comprehensive ChildrenFlag → ChildrenMode computation

### PropFlags
- All flag combinations valid — no impossible states
- Clean mask definitions for `CLASS_MASK`, `STYLE_MASK`, `SPREAD_MASK`, `DIRECTIVE_MASK`
- `NEEDS_OXC_MASK` correctly inverts `STATIC_ONLY_MASK` within 15-bit range

### Test Coverage (9,083 lines total)
- Tokenizer: RCDATA, CDATA, entities, v-pre, EOF states, processing instructions, comment edge cases
- Parser: SFC root nodes, void elements, close-tag recovery, v-condition adjacency, directive caching
- AST types: ChildrenFlag/Mode combinations, PropFlag masks, `needs_expression_parsing`
- Builder: open/close sequences, nested elements, leaf attachment, children metadata

### Missing Tests
- SFC custom block RCDATA in `tokenize_sfc` path
- `resolve_root_kind` case sensitivity
- Tag names exceeding 34 characters
- `handle_end` with multiple unclosed SFC roots
