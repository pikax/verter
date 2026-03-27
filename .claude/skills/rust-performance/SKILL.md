---
name: rust-performance
description: "Rust performance optimization patterns: batch operations, allocation hierarchy, object pooling, CodeTransform API for verter_core"
---

# Rust Performance Guide

Principles for writing performant Rust in `verter_core`, grounded in the crate's APIs and patterns.

## 1. Batch Over Incremental

The highest-impact pattern. `CodeTransform` operations like `overwrite()` and `prepend_left()` each walk the chunk list in O(n). Calling them in a loop is O(n\*N).

Instead, collect operations into `Vec`s and apply with the batch APIs:

- `batch_overwrite(ops: &[(u32, u32, &str)])` — sorted overwrites in one chunk-list pass
- `batch_prepend_left_static(ops: &[(u32, &str)])` — sorted inserts in one pass

```rust
// BAD: O(n) per call × N calls
for (start, end, content) in replacements {
    code_transform.overwrite(start, end, content);
}

// GOOD: O(n+m) single pass
replacements.sort_by_key(|(start, _, _)| *start);
code_transform.batch_overwrite(&replacements);
```

General principle: if you're calling a mutating method in a loop, ask whether the operations can be collected and applied in one traversal.

## 2. Allocation Hierarchy

Prefer allocations in this order (fastest to slowest):

1. **`&'static str`** — zero-cost, compile-time constants
2. **`&'alloc str`** via `code_transform.alloc_str(buf)` — OXC bump allocator, freed in bulk
3. **`&str`** from `ctx.input[start..end]` — zero-cost slice of source
4. **Reusable `&mut String` buffer** — amortized cost via capacity reuse
5. **`String`** — heap-allocated, avoid in hot paths

| Need                                          | Use                                             |
| --------------------------------------------- | ----------------------------------------------- |
| Known constant value                          | `&'static str`                                  |
| Generated text that outlives current function | `code_transform.alloc_str(buf)` → `&'alloc str` |
| Substring of source input                     | `&ctx.input[start..end]`                        |
| Temporary text build-up                       | Shared `&mut String` buffer (see below)         |
| Truly owned, long-lived, mutable text         | `String`                                        |

## 3. Reusable Buffer Pattern

Codegen generators (e.g., `VdomTemplateGenerator`) keep a `buf: String` field. Use `std::mem::take` to temporarily take ownership — avoids borrow conflicts with other `self` fields:

```rust
// Take buffer — avoids per-element heap allocation:
let mut buf = std::mem::take(&mut self.buf);
process_element(&mut buf, ...);
self.buf = buf; // return — retains capacity for next element

// In called functions, accept buf: &mut String.
// Use `buf` directly, not `&mut buf` (it's already &mut String).
```

After building text in `buf`, persist it via bump allocator:

```rust
buf.clear();
buf.push_str("_createVNode(");
buf.push_str(tag);
buf.push(')');
let s: &'alloc str = code_transform.alloc_str(&buf);
pending_overwrites.push((start, end, s));
```

### Save/Truncate for Nested Buffer Use

When you need to build a temporary string inside a function that already uses `buf`, use save/truncate instead of allocating a second buffer:

```rust
let saved = buf.len();
buf.push_str("{ ");
for (i, prop) in props.iter().enumerate() {
    if i > 0 { buf.push_str(", "); }
    buf.push_str(prop);
}
buf.push_str(" }");
let result = code_transform.alloc_str(&buf[saved..]);
buf.truncate(saved); // restore buf to previous state
```

This avoids per-element heap allocation when building intermediate strings like hoisted props.

## 4. Object Pooling

`StateStack` (per-element state during tree walk) contains multiple `Vec` fields. Instead of allocating/dropping per element, pool them:

```rust
// Take from pool — Vecs retain capacity from previous use:
fn take_state(&mut self, id: u32) -> StateStack {
    if let Some(mut s) = self.state_pool.pop() {
        s.reset(id);  // .clear() on all Vecs — retains capacity
        s
    } else {
        StateStack { id, ..Default::default() }
    }
}

// Return to pool after element close:
fn return_state(&mut self, state: StateStack) {
    self.state_pool.push(state);
}
```

Apply this pattern to any struct with inner collections that is repeatedly created/dropped in a loop. `Vec::clear()` retains allocated capacity.

## 5. Borrow Source Instead of Cloning

`ctx.input` holds the full source text. Borrow slices directly instead of cloning:

```rust
// BAD: heap allocation just to read
let name: String = ctx.input[start..end].to_string();
buf.push_str(&name);

// GOOD: zero-cost borrow
let name: &str = &ctx.input[start as usize..end as usize];
buf.push_str(name);
```

For struct fields, prefer `&'alloc str` (bump-allocated) when the struct's lifetime allows it. If adding a lifetime would cascade through too many types, `String` is acceptable.

## 6. Static Fast Paths

For functions that frequently return one of a small set of constants, return `&'static str` directly. Note that `&'static str` coerces to `&'alloc str`, so static constants can be used anywhere bump-allocated strings are expected.

```rust
// Common close strings — no bump allocation needed
let close_str: &'alloc str = if patch_flag.0 == 0 && !is_block_root {
    if needs_array { "])" } else { ")" }  // &'static str coerces to &'alloc str
} else {
    // Rare case: build dynamically
    buf.clear();
    write_patch_flag_suffix(buf, patch_flag, &dynamic_props);
    code_transform.alloc_str(buf)
};
```

## 7. Pre-size Collections

Use `with_capacity` when the expected size is known or estimable:

```rust
pending_overwrites: Vec::with_capacity(512),
pending_prepend_lefts: Vec::with_capacity(256),
buf: String::with_capacity(128),
```

Over-estimating slightly is cheaper than re-allocating.

## 8. Reduce Work, Not Just Speed

Once allocation and batching are optimized, further gains come from doing less work:

- **Skip expensive operations for trivial cases** — e.g., don't sort prop indices when all props have the same priority, don't run OXC parser for a bare identifier binding
- **Early-return fast paths** — e.g., skip Vec allocation + sort + clone when all element props are static (no directives)
- **Merge redundant operations** — e.g., combine two adjacent overwrites (tag name + props) into a single overwrite when both are known at the same time
- **Cache/deduplicate repeated computations** — e.g., `resolved_components_set` for component dedup
- **Short-circuit early** when results are known

## 9. Bulk-Copy String Processing

When processing strings character-by-character (e.g., escaping), prefer a bulk-copy pattern that tracks unmodified regions and copies them in one `push_str` call:

```rust
fn escape_js_string_into(buf: &mut String, s: &str) {
    let mut last_copy_end = 0;
    for (i, ch) in s.char_indices() {
        let replacement = match ch {
            '"' => "\\\"",
            '\\' => "\\\\",
            '\n' => "\\n",
            _ => continue,
        };
        buf.push_str(&s[last_copy_end..i]); // bulk copy unmodified region
        buf.push_str(replacement);
        last_copy_end = i + ch.len_utf8();
    }
    buf.push_str(&s[last_copy_end..]); // copy remaining
}
```

## 10. Benchmarking Methodology

When comparing against another compiler (e.g., Vue's `@vue/compiler-sfc`):

- **Ensure feature parity** — if one compiler generates source maps and the other doesn't, you're not comparing the same work
- **Source maps are expensive** — VLQ encoding + JSON serialization + base64 can consume 30-40% of compile time
- **NAPI overhead is fixed** — allocator creation + JS↔Rust marshalling adds ~7μs per call
- **Profile in release mode** — debug builds are 10-50x slower

### Agent Profiling via MCP (hotpath)

```bash
pnpm run profile:hotpath:mcp    # Start with MCP endpoint at http://localhost:6771/mcp
pnpm run profile:hotpath        # Timing hotspots (no MCP)
pnpm run profile:hotpath:alloc  # Timing + allocation hotspots
```

Agent MCP config template: `mcp/hotpath.mcp.json`

## 11. CodeTransform Optimization History

### Successful Optimizations (Committed)

**A. Fast-path `overwrite()` for single Original chunk** — When the overwritten range falls within a single Original chunk, bypass the general `SmallVec<[Chunk; 4]>` + `Vec::splice` path. Use direct `Vec::insert` (1-2 calls) for the 4 sub-cases.

**B. Eliminate `build_string` first pass via `output_delta` tracking** — Added `output_delta: i64` field tracking running difference between inserted and removed content. `build_string()` uses it for `String::with_capacity`, eliminating the first chunk-iteration pass. ~19% improvement on `build_string/2000`.

**C. Merge `move_wrapped` split + identification into single pass** — Replaced 3 separate linear scans with a single forward `while` loop.

**E-H. Scratch Vec pre-allocation, push_u32 direct digit computation, format_patch_flag static strings, provided_locals Option optimization** — Combined: compile aggregate -9.4%.

**K-N. format_scope_close → &'static str, children.rs text run static constants, condition_scope_close, build_child_records prefix optimization** — Combined: no_sourcemap -5.0%, with_sourcemap -8% to -17%.

### Failed/Reverted Optimizations

**D. PositionSweep for monotonic positions** — Changing `emit_mapped_content`'s function signature caused +8.7% regression on unmodified files. Binary search with ~10 comparisons is already in the CPU branch predictor's sweet spot.

**I. BindingContext clone elimination** — Inconclusive due to 20-33% system noise. BindingContext clone cost is small (one FxHashSet per expression).

**O. Vec<ChildRecord> reuse via std::mem::take** — No measurable improvement. Small Vec allocations (1-10 items) are already efficient.

### Where NOT to Look for Further Gains

| Area                                        | Why it won't help                                              |
| ------------------------------------------- | -------------------------------------------------------------- |
| `offset_to_line_col` binary search          | Already fast for typical file sizes                            |
| `emit_mapped_content` signature             | Changing parameters causes LLVM optimization regressions       |
| `memchr_iter` in source map                 | Already optimal — memchr uses SIMD                             |
| `Vec<ChildRecord>` reuse                    | Small Vec allocations (1-10 items) are already fast            |
| `resolve_simple_expr` per-expression String | ~10-20 calls per component, ~20 bytes each — below noise floor |
| Component resolution String allocation      | Per-component, unavoidable (tag names are dynamic)             |

## Anti-Patterns

| Pattern                                              | Problem                                                  | Fix                                         |
| ---------------------------------------------------- | -------------------------------------------------------- | ------------------------------------------- |
| `overwrite()`/`prepend_left()` in a loop             | O(n) per call                                            | Collect into Vec + batch API                |
| `buf.clone()` for storage                            | Heap alloc per clone                                     | `code_transform.alloc_str(buf)`             |
| `.to_string()` on `ctx.input` slices                 | Unnecessary heap copy                                    | `&ctx.input[start..end]`                    |
| Fresh Vec-heavy structs per iteration                | Alloc/dealloc churn                                      | Pool + `reset()` with `.clear()`            |
| `Instant::now()` unconditionally                     | Panics in WASM                                           | `#[cfg(not(target_arch = "wasm32"))]` guard |
| SmallVec with large types (>64B) in Box'd structs    | Inflates allocation size, 40-50% regression              | Keep `Vec`                                  |
| `Vec<String>` for bump-allocatable content           | Per-element heap alloc                                   | `Vec<&'alloc str>` + save/truncate          |
| Explicit `is_sorted` check before sort               | Rust's TimSort already detects sorted runs               | Just call `.sort_by_key()`                  |
| Changing hot function signatures                     | Causes LLVM optimization regressions                     | Keep hot function signatures stable         |
| Linear sweep replacing binary search on <1K elements | Binary search already in CPU branch predictor sweet spot | Only consider at >10K elements              |
| Reusing small Vecs (1-10 items)                      | Allocator handles small allocations efficiently          | Only pool/reuse Vecs with >50 items         |
