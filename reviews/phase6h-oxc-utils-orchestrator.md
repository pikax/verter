# Phase 6h: OXC Utilities + Compile Orchestrator Review

## Overall: VERY GOOD — Strong Architecture, Minor Completeness Gaps

Single-pass parsing, performance-conscious design, Vue parity in globals/keywords. No critical or high issues.

---

## Critical Issues

**None identified.**

---

## High Issues

**None identified.**

---

## Medium Issues

### M1. `iter_expressions()` Does Not Yield v-for or v-slot Expressions
**File**: template/oxc/types.rs:252-274

Doc comment says "useful for applying bulk transforms (e.g., TypeScript stripping) across all template expressions" but implementation misses v-for right-side expressions and v-slot parameter expressions. Compile orchestrator compensates with separate v-for loop (mod.rs:269-279), but TypeScript annotations in v-for/v-slot could survive in `force_js: true` mode.

### M2. `collect_expression_references` Misses `UpdateExpression`
**File**: helpers.rs:77-195

`count++` or `--index` operands not collected. The full `BindingVisitor` in expression.rs correctly handles `UpdateExpression`. Used by v-for (right-side) and v-slot (default values).

### M3. `AssignmentExpression` in `collect_expression_references` Only Collects from `right`
**File**: helpers.rs:166-168

Left side (`assign.left`) can contain identifier references (e.g., `obj.prop = val` — `obj` is a reference). BindingVisitor correctly handles both sides.

---

## Low Issues

- L1: `is_global()` matches Vue's `@vue/shared` exactly but misses newer globals (`structuredClone`, `AbortController`, etc.)
- L2: Catch-all `_ => {}` silently ignores new OXC expression types
- L3: v-for separator search doesn't handle template literals containing ` of `/` in `
- L4: `VerterCompileOptions::external_types` passed by value (performance concern for incremental LSP)
- L5: `to_source_location` TODO about offset not being UTF-16 aware
- L6: `extract_component_name` doesn't handle Windows path separators (mitigated by host normalization)
- L7: Cursor only counts `\n` as newline (matches HTML spec, correct for Vue)
- L8: Script detector could match `<script>` inside template literal content

---

## Strengths

### OXC Expression Parsing
- **Single forward pass** — O(1) lookup by node ID
- **Scope cascade via parent locals** — v-for/v-slot locals propagate without cloning
- **Fast path for plain elements** — `el.is_plain()` skips OXC entirely
- **Optimistic AllInterpolationsStatic** — parent sets flag, children clear it
- **Empty span guard** — avoids wasteful allocation for empty expressions
- **ExpressionFlag bitflags** — O(1) bitwise composition with PropFlags

### Binding Extraction
- **30+ Expression variants** handled in BindingVisitor including all TS-specific types
- **Shorthand property handling** — `is_shorthand` flag for `{ foo }` → `{ foo: _ctx.foo }`
- **Three-state dynamism** — Static/MaybeDynamic/Dynamic with correct merge semantics
- **Bitmap-based keyword/global check** — O(1) early rejection by identifier length

### v-for/v-slot Parsing
- **Right-to-left separator search** — handles nested `of`/`in` in expressions
- **Multi-variable destructuring** — tuples, objects, arrays, rest, nested patterns
- **Reference deduplication and sorting** — sorted by start position for forward scanning
- **Comprehensive test suites** — edge cases, TS annotations, multiple params

### Compile Orchestrator
- **Clear 5-phase pipeline** — Tokenize → Style → Script → Template → Assemble
- **Early OXC parse for import elision** — template expression analysis informs tree-shaking
- **V-for reference compensation** — explicit iteration of v-for data to fill gap
- **TSX source map combination** — merges script + template maps with line shifting
- **Inter-block gap removal** — cleans whitespace between SFC blocks
- **Optional template data extraction** — gated by flag, zero overhead when disabled

### Position Handling
- **Consistent 1-based convention** — explicit doc comments
- **Binary search for random access** — O(log n) offset-to-line lookup
- **UTF-16 column counting** — with ASCII fast path, correct surrogate pair handling
- **PositionSweep for monotonic offsets** — linear sweep for sorted sequences
- **Multiple `find_line_offsets` impls** — naive, memchr, chunks, u64, ptr, bump

### Language Detection
- **Comprehensive variant handling** — ts, typescript, tsx, jsx, js, javascript
- **Comment-aware detection** — skips `<script>` inside HTML comments
- **Mini tokenizer for attribute parsing** — handles quoted `>` in generics
- **Bounded-window optimization** — avoids full-file scanning

---

## Recommendations
1. Extend `iter_expressions()` to yield v-for/v-slot expressions, or clearly document intentional omission
2. Add `UpdateExpression` handling to `collect_expression_references`
3. Add `AssignmentExpression` left-side collection in helpers functions
