# Testing Guide

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

Verter requires thorough testing for all changes. This guide covers testing patterns for both TypeScript and Rust code.

## TypeScript Tests

```bash
pnpm test                             # All JS/TS tests
pnpm vitest --run                     # All tests, non-watch mode
pnpm vitest --run path/to/test.ts     # Specific file
```

Tests are co-located as `*.spec.ts` files next to their source files. Type tests in `packages/types/` use `vitest --typecheck`.

### AI-Generated Tests

When adding tests with AI assistance, mark them appropriately:

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

### Sourcemap Testing

For testing sourcemap accuracy (see `macros.map.spec.ts` for examples):

```typescript
const { s, source, result } = processMacrosForSourcemap(code);
const map = s.generateMap({ source: "test.vue" });
```

## Rust Tests

```bash
cargo test --workspace --verbose                # All Rust tests
cargo test --package verter_compiler test_name      # Specific test by name
cargo test --package verter_compiler -- --nocapture # With stdout output
cargo test --package verter_compiler 2>&1 | tail -60  # Truncated output
```

### Test File Organization

When a Rust source file's inline `#[cfg(test)] mod tests` block exceeds ~400 lines, extract tests to a separate sibling file to keep source files focused on production code.

**For standalone files** (e.g., `analysis.rs`):

```rust
// In analysis.rs:
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

The extracted file contains `use super::*;`, helper functions, and `#[test]` functions directly — no wrapping `mod tests { }` block.

Small rule files (e.g., diagnostic rules at 50-150 lines) can keep tests inline.

## TDD Workflow

**Test-Driven Development is mandatory.** For every change:

1. **Write failing tests first** -- demonstrate the expected behavior and verify the tests fail
2. **Implement the minimum code** to make the failing tests pass
3. **Refactor** while keeping tests green

This applies to:

- **New features**: Add tests covering the new functionality
- **Bug fixes**: Add tests that would have caught the bug
- **Refactoring**: Ensure existing tests pass, add tests for edge cases discovered
- **Behavioral changes**: Add tests verifying the new behavior

## Assertion Requirements

### Always Include Negative Assertions

Every test must verify both what SHOULD be present AND what should NOT be present. A test that only checks for expected output can pass even when the output contains invalid or broken content alongside the expected content.

#### Rust Example

```rust
// GOOD: Both positive and negative assertions
let result = gen_tsx_template(
    r#"<template><div v-if="show">hello</div></template>"#
);
assert!(
    result.contains("_ctx.show ?"),
    "should have ternary condition"
);
assert!(
    !result.contains("v-if"),
    "v-if attribute must be removed from JSX"
);

// BAD: Only positive assertion -- passes even if v-if="show" leaks into output
let result = gen_tsx_template(
    r#"<template><div v-if="show">hello</div></template>"#
);
assert!(
    result.contains("_ctx.show ?"),
    "should have ternary condition"
);
// Missing negative assertion!
```

#### TypeScript Example

```typescript
// GOOD
expect(output).toContain('_createElementVNode("div")');
expect(output).not.toContain("v-for");
expect(output).not.toContain("v-if");

// BAD
expect(output).toContain('_createElementVNode("div")');
// No check that directives were removed
```

### Type Tests

Type tests must include both positive assertions and `@ts-expect-error` negative assertions. This prevents `any`, `unknown`, or `never` types from silently passing tests:

```typescript
it("type is correctly inferred", () => {
  type Result = SomeTypeHelper<Input>;

  // Positive assertion -- type matches expected
  assertType<Result>({} as ExpectedType);
  assertType<ExpectedType>({} as Result);

  // @ts-expect-error -- Result is not any/unknown/never
  assertType<{ unrelated: true }>({} as Result);
});
```

### Codegen Tests (Rust)

All codegen tests must validate that the output is syntactically valid JavaScript using the OXC parser. Use the `gen_and_validate()` helper:

```rust
use crate::test_utils::gen_and_validate;

#[test]
fn test_v_for_codegen() {
    let result = gen_and_validate(
        r#"<template><div v-for="item in items" :key="item.id">{{ item.name }}</div></template>"#
    );
    assert!(result.contains("_renderList"), "should use _renderList helper");
    assert!(!result.contains("v-for"), "v-for must not appear in output");
}
```

## Test Output Best Practices

When running tests where you need to inspect output, redirect to a temp file first to avoid re-running expensive test suites:

```bash
# Good: capture once, search multiple times
pnpm exec playwright test --project=preview 2>&1 | tee /tmp/e2e-output.log
grep -i "fail\|error" /tmp/e2e-output.log

# Bad: re-running the full suite each time
pnpm exec playwright test --project=preview 2>&1 | grep "fail"
pnpm exec playwright test --project=preview 2>&1 | grep "error"  # wasteful re-run
```

## Integration Tests

Integration tests verify Verter against real-world open-source Vue projects:

```bash
# Run integration test for a specific project (skip baseline, reuse checkout).
# Substitute the project name for $PROJECT — angle-bracket placeholders are shell
# redirects, so a copy-pasted `<project>` is a syntax error, not a prompt to fill in.
pnpm integration-test --skip-build --skip-baseline --no-clone "$PROJECT"
```

See the [CI/CD page](./ci-cd.md) for details on the integration test workflow.

## Server Cleanup

After starting any dev server, preview server, or other long-running process for testing, always terminate it when done — stale servers interfere with subsequent test runs. Capture the PID at spawn and terminate **that** PID. A port is a diagnostic, not proof of ownership: `lsof -t -i:<port>` returns whoever holds the port, which may be your own editor's server or another agent's. Never terminate by image name or pattern (`pkill -f node`, `taskkill /F /IM node.exe`, `Stop-Process -Name`).

```bash
pnpm --filter @verter/playground preview & SERVER_PID=$!   # capture at spawn

kill "$SERVER_PID"                                          # Unix — terminate only what you started
taskkill //F //T //PID "$(cat /proc/$SERVER_PID/winpid)"    # Windows — see both caveats below
```

Two Windows caveats, both established by running the commands, not by reading the flags:

- **`$!` is the MSYS pid, not the Windows pid `taskkill` expects.** Passing `$SERVER_PID` directly prints `ERROR: The process "…" not found`, exits 128, and terminates nothing — hence the `/proc/<pid>/winpid` lookup.
- **`//T` does not reap descendants.** It terminates the named process (exit 0, `SUCCESS: … has been terminated`) while its children survive, so `pnpm`'s child `vite`/`node` can outlive the kill. Confirm the server is really gone (`kill -0 "$SERVER_PID"`, or re-probe the port) instead of trusting the success line, and terminate any survivor by its own recorded PID.
