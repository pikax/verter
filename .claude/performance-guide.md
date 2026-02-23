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

## 10.1 Agent Profiling via MCP (hotpath)

When performance work needs AI-agent investigation, run hotpath with MCP enabled so tools can query live profiling data.

### Start profiler with MCP endpoint

From repo root:

```bash
pnpm run profile:hotpath:mcp
```

This runs `crates/verter_bench/examples/profile_ast.rs` against real-world Vue repos (or fixture fallback) and exposes:

- `http://localhost:6771/mcp`

### Agent MCP config

Use the checked-in MCP config template:

```text
mcp/hotpath.mcp.json
```

It contains:

```json
{
    "mcpServers": {
        "verter-hotpath": {
            "url": "http://localhost:6771/mcp"
        }
    }
}
```

Point your MCP-capable agent to this file (or copy the `mcpServers.verter-hotpath` entry into your local MCP config).

### Non-MCP fallback modes

```bash
pnpm run profile:hotpath        # timing hotspots
pnpm run profile:hotpath:alloc  # timing + allocation hotspots
```

## 11. CodeTransform Optimization History

This section documents performance experiments on the `code_transform` module, including what worked and what didn't. Reference this before attempting further optimizations.

### Successful Optimizations (Committed)

**A. Fast-path `overwrite()` for single Original chunk** (`daae488`)
- **What**: When the overwritten range `[start, end)` falls within a single Original chunk, bypass the general `SmallVec<[Chunk; 4]>` + `Vec::splice` path. Instead, use direct `Vec::insert` (1-2 calls) for the 4 sub-cases (middle split → 3 chunks, left-aligned → 2, right-aligned → 2, exact match → 1 in-place).
- **Why it worked**: `overwrite()` is the most frequently called mutation. The common case is a single-chunk replacement, and `Vec::splice` has higher overhead than targeted `insert` calls.
- **Impact**: Measurable improvement on `basic_operations/overwrite` and `sequential_overwrite`.

**B. Eliminate `build_string` first pass via `output_delta` tracking** (`4b6a323`)
- **What**: Added an `output_delta: i64` field to `CodeTransform` that tracks the running difference between inserted and removed content. Each mutation updates it. `build_string()` uses `(original.len() as i64 + output_delta) as usize` for `String::with_capacity`, eliminating the first pass that computed exact length.
- **Why it worked**: `build_string()` previously iterated all chunks twice — once for length, once for building. Removing the first pass halved cache traffic for the string-building hot path.
- **Impact**: ~19% improvement on `chunk_iteration/build_string/2000`.

**C. Merge `move_wrapped` split + identification into single pass** (`5dfc598`)
- **What**: Replaced the 3 separate linear scans in `move_wrapped` (`ensure_split_at(start)`, `ensure_split_at(end)`, full iteration for `indices_to_move`) with a single forward `while` loop that splits at boundaries and collects indices in one pass.
- **Why it worked**: Reduced 3× O(n) scans to 1× O(n), with inline split logic avoiding redundant chunk-list traversals.
- **Impact**: Measurable improvement on `code_transform/moves`.

### Failed Optimizations (Reverted)

**D. Source map linear sweep (`PositionSweep`) for monotonic positions** — REVERTED
- **What**: Added `PositionSweep` struct to `position.rs` that replaces O(log N) binary search in `PositionResolver::offset_to_line_col()` with O(1) amortized linear sweep for monotonically increasing offsets. Changed `emit_mapped_content` to accept pre-resolved `(source_line, source_column)` instead of `&PositionResolver + offset`, allowing Original/Overwritten chunks to use the sweep while Moved chunks fall back to binary search.
- **Why it failed**: Changing `emit_mapped_content`'s function signature (removing `&PositionResolver` parameter, adding two `u32`s) altered LLVM's inlining/optimization decisions, causing **+8.7% regression on unmodified files** and **+7.2% on 10 edits**. Only 100+ edits showed marginal improvement (-2.8%). The binary search on typical file sizes (~400-700 lines ≈ 10 comparisons) is already fast enough that the sweep's constant savings (~5ns/chunk) don't overcome the indirect regression from function signature changes.
- **Lesson**: Binary search with ~10 comparisons is already in the CPU branch predictor's sweet spot. Replacing it with a linear sweep only helps at very high chunk counts (thousands), but the structural code changes needed to thread the sweep state through the API cause compiler optimization regressions that dominate in the common case. **Do not attempt to optimize `offset_to_line_col` for source map generation** — the binary search is not a bottleneck.
- **Note**: `PositionSweep` was kept in `position.rs` as it's well-tested and may be useful for other use cases with truly large line counts (10K+ lines).

### Optimizations Applied (2026-02-22 Performance Review)

**E. Scratch Vec pre-allocation in CodeTransform** (code_transform.rs)
- **What**: Changed `scratch: Vec::new()` to `scratch: Vec::with_capacity(estimated_chunks)` in `CodeTransform::new()`, using the same `(len / 13).max(64)` heuristic as the chunks Vec.
- **Why it works**: `batch_prepend_left_static` uses `scratch` for O(n+m) merge. Without pre-allocation, the first batch op triggers multiple reallocs as the Vec grows to accommodate all chunks.
- **Impact**: Eliminates reallocation overhead on first `batch_prepend_left_static` call.

**F. push_u32 direct digit computation** (shared/helpers.rs)
- **What**: Replaced `write!(buf, "{}", n)` with manual digit pushing for common small values (0-99) and stack-based conversion for larger values.
- **Why it works**: `write!` goes through `fmt::Display` + `Formatter` infrastructure even for trivial integers. Direct byte arithmetic avoids this overhead for the common case (most node IDs, indices, and patch flags are < 100).
- **Impact**: Removes formatting overhead per number push. Called frequently in all codegen backends.

**G. format_patch_flag static strings** (shared/helpers.rs)
- **What**: In production mode, returns `&'static str` for common patch flag values (0-9, powers of 2 up to 512) instead of calling `flag.to_string()` + `alloc_fn()`.
- **Why it works**: `&'static str` coerces to `&'alloc str`, bypassing both heap allocation (`to_string()`) and bump allocation (`alloc_fn`). The vast majority of production patch flags are single common values.
- **Impact**: Eliminates both heap and bump allocation for ~95% of patch flag formatting calls.

**H. provided_locals Vec::to_vec elimination** (template/oxc/mod.rs, types.rs)
- **What**: Changed `OxcParsedElement.provided_locals` from `Vec<&'alloc str>` to `Option<Vec<&'alloc str>>`. `None` means "same as parent — no v-for/v-slot locals added". Only clones the parent's locals when v-for or v-slot is present.
- **Why it works**: Most elements have neither v-for nor v-slot, so the previous `parent_ignored.to_vec()` clone was wasted for the majority of elements. The walker now walks through `None` entries to find the nearest ancestor with actual locals.
- **Impact**: Eliminates Vec clone for all elements without scoping directives (majority of template elements).

**Combined benchmark result (E-H)**: compile aggregate -9.4% (p<0.05 vs immediately-prior baseline). Template-only benchmarks within noise. Note: system thermal drift introduces 5-20% variance; absolute improvement may be smaller.

**K. format_scope_close → &'static str** (vdom/directives.rs)
- **What**: Changed `format_scope_close` return type from `String` to `&'static str`. All scope close strings (v-if ternary fallback, v-else-if continuation, v-else close, v-for `renderList` close) are compile-time constants. Callers use `prepend_static` (zero-allocation) instead of `prepend_alloc`.
- **Why it works**: Scope close strings are emitted per conditional/loop element. Previously each one allocated a `String` via `format!()` and then bump-allocated via `alloc_str()`. Since all variants are static constants, both allocations are eliminated.
- **Impact**: Eliminates 1 heap + 1 bump allocation per v-if/v-else-if/v-else/v-for element.

**L. children.rs text run static string constants** (vdom/children.rs)
- **What**: Replaced heap-allocated `String` literals in `add_children_separators_array` with `&'static str` constants using `prepend_static`/`overwrite` instead of `prepend_alloc`/`overwrite`. Text run wrappers (`_createTextVNode(`, `, 1)`, `+ `, etc.) and separator strings (`, `) are all compile-time constants.
- **Why it works**: Text run wrapping and child separation logic runs per-child, making it one of the highest-frequency codegen operations. Switching from dynamic strings to statics eliminates bump allocation overhead for every separator and text wrapper.
- **Impact**: Eliminates bump allocations for all child separator and text wrapper strings.

**M. condition_scope_close (replacing build_condition_prefix)** (vdom/directives.rs)
- **What**: Replaced `build_condition_prefix` (which returned `(String, ScopeClose)`) with `condition_scope_close` (returns just `ScopeClose`). The `String` prefix was always discarded by the only production caller — the condition prefix is computed separately using OXC binding data and stored in `resolved_condition_prefixes`.
- **Why it works**: Eliminated a `format!("({expr}) ? ")` allocation per v-if/v-else-if element that was immediately discarded.
- **Impact**: Eliminates 1 wasted `format!()` allocation per v-if/v-else-if element.

**N. build_child_records condition prefix optimization** (vdom/mod.rs)
- **What**: In `build_child_records`, replaced `.cloned().unwrap_or_else(|| format!(...))` with borrowing from the `resolved_condition_prefixes` HashMap + `String::with_capacity` + `push_str`. Eliminates 1 clone + 1 `format!()` per v-if/v-else-if child record.
- **Why it works**: The old code cloned the resolved expression from the HashMap, then formatted it into a new String. The new code borrows from the HashMap and builds the prefix string with a pre-sized allocation.
- **Impact**: Eliminates 1 String clone + 1 `format!()` per v-if/v-else-if child.

**Combined benchmark result (K-N)**: no_sourcemap aggregate -5.0%, with_sourcemap vuetify -13%, ant-design-vue -15%, nuxt-ui -17%, element-plus -8%, shadcn-vue -7%. Note: with_sourcemap baseline may include prior batch improvements.

### Investigated but Not Pursued

**I. BindingContext clone elimination** (utils/oxc/bindings/expression.rs) — INCONCLUSIVE
- **What**: Changed `extract_bindings_from_expression` to take `BindingContext` by value instead of `&BindingContext` to eliminate `.clone()` inside the function.
- **Result**: Initial benchmark showed +10-20% regression, but subsequent baseline re-measurements showed 20-33% system noise (thermal throttling). The regression was likely noise, not real. Reverted to avoid risk.
- **Lesson**: The BindingContext clone cost (one FxHashSet clone per expression) is likely small because most template expressions reference only a few ignored identifiers. The optimization anti-pattern of changing hot function signatures (see anti-patterns) may also apply here.

**J. Template codegen format! reductions** — SKIPPED
- **What**: Audit found only 11 real `format!` calls in production template codegen (6 in vdom/element.rs for v-model, 2 in vdom/slots.rs, 1 in binding.rs, 2 in vapor — already comments about avoiding format!).
- **Why skipped**: The codebase already uses `push_str`/`push_u32` patterns extensively. The remaining `format!` calls are in moderate-frequency code paths (per v-model directive, per slot). Replacing them would save < 0.1% total time.
- **Lesson**: The template codegen is already well-optimized for string allocation. Future optimization efforts should focus on higher-level algorithmic improvements rather than micro-optimizing string construction.

**O. Vec<ChildRecord> reuse via std::mem::take** — REVERTED (no measurable improvement)
- **What**: Added `children_buf: Vec<ChildRecord>` field to `VdomCodeGen`, changed `build_child_records` to accept `&mut Vec<ChildRecord>` as output parameter. Used `std::mem::take` + return pattern (like `self.buf`) to reuse the Vec across elements, avoiding per-element Vec allocation.
- **Result**: Benchmarks showed no consistent improvement — results within noise (aggregate no_sourcemap +1.9%, with_sourcemap +2.7%). Individual projects showed ±2-4% fluctuation.
- **Lesson**: `Vec::with_capacity(n)` for small `n` (1-10 children per element) is already efficient. The allocator handles small allocations (~40-200 bytes) quickly. The overhead of `std::mem::take` + move semantics + clearing `Option<String>` fields in ChildRecord doesn't save enough to overcome the noise floor. **Do not attempt to pool/reuse small Vecs** — only pool structs with large inner collections (see Object Pooling §4).

### Where NOT to Look for Further Gains

| Area | Why it won't help |
|------|-------------------|
| `offset_to_line_col` binary search | See Opt D above — already fast for typical file sizes |
| `emit_mapped_content` signature | Changing parameters causes LLVM optimization regressions |
| `memchr_iter` in source map | Already optimal — memchr uses SIMD on supported platforms |
| `SourceMapBuilder::add_token` | External dependency (oxc_sourcemap), can't optimize internally |
| `advance_generated_position` | Already a tight loop with memchr, no fat to cut |
| `Vec<ChildRecord>` reuse | See Opt O — small Vec allocations (1-10 items) are already fast |
| `format_scope_close` allocation | Already `&'static str` (Opt K) — no further improvement possible |
| `build_condition_prefix` allocation | Already eliminated unused prefix (Opt M) |
| `resolve_simple_expr` per-expression String | ~10-20 calls per component, ~20 bytes each — below noise floor |
| `to_imports()` Vec allocation | Called once per compilation — negligible |
| Component resolution String allocation | Per-component, unavoidable (tag names are dynamic) |

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
| Changing hot function signatures to pass pre-resolved values | Replacing `&SomeResolver` parameter with pre-computed values changes LLVM inlining/optimization decisions — can cause 5-10% regressions even when the new version does strictly less work | Keep hot function signatures stable; if you need a pre-computation, do it inside the function or use a trait to abstract the lookup strategy |
| Linear sweep replacing binary search on <1K elements | Binary search on ~700 elements ≈ 10 comparisons is already in the CPU branch predictor's sweet spot; linear sweep saves ~5ns/call but structural overhead dominates | Only consider linear sweep when element count exceeds ~10K and the sweep state can be threaded without function signature changes |
| Reusing small Vecs (1-10 items) via `std::mem::take` | Allocator handles small allocations (~40-200 bytes) efficiently; `take` + move + clear overhead ≈ allocation cost | Only pool/reuse Vecs with large inner collections or when item count exceeds ~50 |
