---
name: testing
description: "Testing patterns, TDD workflow, TypeScript and Rust test conventions, sourcemap testing, E2E best practices, server cleanup for Verter"
---

# Testing Patterns & Conventions

## Server Cleanup

**IMPORTANT**: After starting any dev server, preview server, or other long-running process for testing purposes, **always kill it when done**. This prevents stale servers from interfering with subsequent test runs (e.g., Playwright's `reuseExistingServer: true` will use a stale server serving old builds).

```bash
# After finishing with a server, kill it
# If started in background, use the process ID or port:
kill $(lsof -t -i:4173)   # Unix
taskkill //F //PID <pid>   # Windows

# Or if using pnpm/npm scripts, Ctrl+C the process
```

## Test Output Best Practices

When running E2E tests or test suites where you need to inspect output, **redirect output to a temp file first**, then grep/read the file. This avoids re-running expensive builds and tests just to search for different patterns:

```bash
# Good: capture once, search multiple times
pnpm exec playwright test --project=preview 2>&1 | tee /tmp/e2e-output.log
# Then search as needed:
grep -i "fail\|error" /tmp/e2e-output.log

# Bad: re-running the full test suite each time you need different output
pnpm exec playwright test --project=preview 2>&1 | grep "fail"
pnpm exec playwright test --project=preview 2>&1 | grep "error"  # wasteful re-run
```

## TypeScript Test Patterns

**Test locations**: Unit tests are co-located as `*.spec.ts` next to source files. Type tests in `packages/types/` use `vitest --typecheck`.

**AI-generated tests**: Add appropriate comments indicating AI assistance:

```typescript
// For new test files, add a JSDoc at the top:
/**
 * @ai-generated - This test file was generated with AI assistance.
 * Brief description of what the tests cover.
 */

// For individual tests in existing files:
// @ai-generated - Tests X functionality with Y scenarios
it("does something", () => {
  /* ... */
});
```

**Sourcemap testing** (see `macros.map.spec.ts`):

```typescript
const { s, source, result } = processMacrosForSourcemap(code);
const map = s.generateMap({ source: "test.vue" });
```

**Type testing best practices** (`packages/types/`):

- Always include **both** a positive assertion and a `@ts-expect-error` negative assertion
- This prevents `any`/`unknown`/`never` types from silently passing tests

```typescript
it("type is correctly inferred", () => {
  type Result = SomeTypeHelper<Input>;

  // Positive assertion - type matches expected
  assertType<Result>({} as ExpectedType);
  assertType<ExpectedType>({} as Result);

  // @ts-expect-error - Result is not any/unknown/never
  assertType<{ unrelated: true }>({} as Result);
});
```

## Rust Test Patterns

### Test File Organization

When a Rust source file's inline `#[cfg(test)] mod tests` block exceeds ~400 lines, extract tests to a separate sibling file. Two patterns:

**For standalone files** (e.g., `analysis.rs`):

```rust
// In analysis.rs — replace the inline #[cfg(test)] mod tests { ... } block:
#[cfg(test)]
#[path = "analysis_tests.rs"]
mod analysis_tests;
```

**For `mod.rs` files** (e.g., `ide/template/mod.rs`):

```rust
// In mod.rs — loads tests.rs from the same directory:
#[cfg(test)]
mod tests;
```

The extracted file contains the module contents directly — `use super::*;`, helpers, and `#[test]` functions. No wrapping `mod tests { }` block.

### TDD Workflow

1. Write failing tests first
2. Implement the minimum code to pass
3. Run `cargo clippy --fix --allow-dirty --allow-staged --workspace -- -D warnings && cargo fmt --all`
4. Run `cargo test --package verter_core --lib`

### Arena-Based Template AST

The parser builds a flat `Vec<AstNode>` arena with O(1) navigation:

```rust
pub struct TemplateAst {
    nodes: Vec<AstNode>,        // flat arena
    root: RootNodeTemplate,
}

pub struct AstNode {
    kind: AstNodeKind,          // Element | Text | Comment | Interpolation
    parent: Option<NodeId>,     // O(1) parent lookup
    index_in_parent: usize,     // O(1) sibling lookup
}
```

`ElementNode` pre-computes metadata during parsing to avoid re-scanning in codegen:

- `tag_type`: Element / Component / SlotOutlet / Template
- `prop_flag`: Bitset of prop characteristics (has class, style, spread, etc.)
- `children_flag`: Bitset of children characteristics (has text, elements, v-if, etc.)
- `children_mode`: Enum for codegen branching (Empty, TextOnly, SingleElement, Mixed, etc.)
- Cached directives: `v_condition`, `v_for`, `v_slot`, `v_once`, `v_ref`

### Test Validation Pattern

All codegen tests must validate generated JS syntax:

```rust
let result = compile_sfc(source);
let tpl = result.template.unwrap();
// Parse generated code with OXC to verify valid JS
let parsed = oxc_parser::Parser::new(&alloc, &tpl.code, source_type).parse();
assert!(parsed.errors.is_empty(), "JS parse error: {:?}\n{}", parsed.errors, tpl.code);
```

### Adding a New TS Plugin

1. Create plugin file in `packages/core/src/v5/process/script/plugins/`
2. Implement `ScriptPlugin` interface using `definePlugin`
3. Register in `packages/core/src/v5/process/script/plugins/index.ts`
4. Add tests

### Testing Strategy

- **Unit tests**: Test individual plugins with minimal SFC snippets
- **Integration tests**: Test full transformation pipeline
- **Type tests**: Verify TypeScript inference (using `vitest --typecheck`)
- **Sourcemap tests**: Verify position mappings
