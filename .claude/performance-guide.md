# Rust Performance Guide

Principles for writing performant Rust in `verter_core`, grounded in the crate's APIs and patterns.

## 1. Batch Over Incremental

The highest-impact pattern. `CodeTransform` operations like `overwrite()` and `prepend_left()` each walk the chunk list in O(n). Calling them in a loop is O(n*N).

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

| Need | Use |
|------|-----|
| Known constant value | `&'static str` |
| Generated text that outlives current function | `code_transform.alloc_str(buf)` → `&'alloc str` |
| Substring of source input | `&ctx.input[start..end]` |
| Temporary text build-up | Shared `&mut String` buffer (see below) |
| Truly owned, long-lived, mutable text | `String` |

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

This avoids both heap allocation and bump-allocator overhead for common cases. Identify high-frequency return values first, then add static fast paths for them.

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

This is faster than char-by-char `push` because most strings have few special characters, so the bulk memcpy dominates.

## 10. Benchmarking Methodology

When comparing against another compiler (e.g., Vue's `@vue/compiler-sfc`):

- **Ensure feature parity** — if one compiler generates source maps and the other doesn't, you're not comparing the same work. Add options to toggle expensive features (e.g., `skip_source_map`) to isolate core compilation speed.
- **Source maps are expensive** — VLQ encoding + JSON serialization + base64 can consume 30-40% of compile time for template-heavy files (~600 chunks). Always account for this when profiling.
- **NAPI overhead is fixed** — allocator creation + JS↔Rust marshalling adds ~7μs per call. This is a floor that no Rust-side optimization can remove. For sub-100μs compilations, NAPI overhead is a significant percentage.
- **Profile in release mode** — debug builds are 10-50x slower and have completely different bottleneck profiles.

## Anti-Patterns

| Pattern | Problem | Fix |
|---------|---------|-----|
| `overwrite()`/`prepend_left()` in a loop | O(n) per call | Collect into Vec + batch API |
| `buf.clone()` for storage | Heap alloc per clone | `code_transform.alloc_str(buf)` |
| `.to_string()` on `ctx.input` slices | Unnecessary heap copy | `&ctx.input[start..end]` |
| Fresh Vec-heavy structs per iteration | Alloc/dealloc churn | Pool + `reset()` with `.clear()` |
| `Instant::now()` unconditionally | Panics in WASM | `#[cfg(not(target_arch = "wasm32"))]` guard |
| SmallVec with large types (>64B) in Box'd structs | Inflates allocation size, hurts cache locality — can cause 40-50% regression | Keep `Vec` — one small heap allocation is cheaper than hundreds of bytes of inline storage |
| `Vec<String>` for bump-allocatable content | Per-element heap alloc for strings that only need to live as long as the allocator | `Vec<&'alloc str>` + save/truncate on shared buf |
| Separate `String` per element for temporary codegen | Allocation churn across hundreds of elements | Save/truncate pattern on shared buffer (§3) |
| Explicit `is_sorted` check before sort | Rust's TimSort already detects sorted runs in O(n) — the check adds overhead | Just call `.sort_by_key()` directly |
| Replacing sort with merge for deferred-op buffers | Deferred emission buffers (`pending_append_lefts`, `pending_prepend_lefts`) are NOT in document order — binding patches and v-if fallbacks break position ordering | Only `pending_overwrites` maintains document order. Always sort deferred buffers. |
| `_createStaticVNode` HTML string building | Verter's overwrite architecture (`&'alloc str` references into bump memory) is already more efficient than building escaped HTML strings — iterating source bytes + JS escaping + heap allocation costs MORE than individual VNode overwrites | Keep individual VNode generation. Overwrite-based codegen is already the optimal representation for static content. |
| `.drain(..)` to avoid clone for Copy-like types | For structs with mostly Copy fields (u32, bool, enum) and `Option::None` modifiers, clone is nearly free — drain saves one Vec data buffer but not the field copy cost | Only use drain when types have expensive-to-clone fields (String, Vec, etc.) |
