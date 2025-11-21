# Verter Bench Tests & Benchmarks

This package contains tests and benchmarks for Vue language services, comparing @vue/language-service (Volar) with @verter/language-server (Verter).

## Structure

```
src/
  server/
    volar-server.ts           - Language server setup helper for Volar
    verter-server-direct.ts   - Direct API setup helper for Verter
  __tests__/
    completions.volar.spec.ts  - Completion tests for Volar (9 tests)
    completions.verter.spec.ts - Completion tests for Verter (4 TypeScript tests)
  __benchmarks__/
    completions.bench.ts       - Performance benchmarks comparing Volar vs Verter
test-workspace/
  fixture.vue - Test Vue files
  tsconfigProject/ - TypeScript project test files
    component-for-auto-import.vue
    fixture.vue
    tsconfig.json
```

## Running Tests

### Run all tests:
```bash
pnpm test
```

### Run specific test suites:
```bash
# Volar tests (all 9 passing)
pnpm test:volar

# Verter tests (4 TypeScript tests passing)
pnpm test:verter
```

### Watch mode:
```bash
pnpm test:watch
```

## Running Benchmarks

### Run benchmarks once:
```bash
pnpm bench
```

### Run benchmarks in watch mode:
```bash
pnpm bench:watch
```

### Run benchmarks with verbose comparison:
```bash
pnpm bench:compare
```

## Benchmark Categories

The benchmarks compare Volar and Verter performance across:

1. **Script setup completions** - Testing `$` Vue instance completions
2. **TypeScript completions** - Testing string method completions
3. **Auto import component** - Testing component import suggestions
4. **Complex TypeScript inference** - Testing ref<User> type inference
5. **Multiple file operations** - Testing 5 sequential file open/complete/close cycles

## Test Categories

### Volar Tests (completions.volar.spec.ts)

Complete Vue language service tests:
- ✅ Vue tags (template, script, style)
- ✅ HTML tags and built-in components
- ✅ HTML events
- ✅ Vue directives (v-show, v-if, v-for, etc.)
- ✅ $event argument
- ✅ Script setup completions
- ✅ Auto import components
- ✅ Slot props completion
- ✅ Event modifiers

### Verter Tests (completions.verter.spec.ts)

TypeScript-focused language service tests:
- ✅ Script setup completions (Vue instance properties like $attrs, $emit)
- ✅ TypeScript completions (string methods, type inference)
- ✅ Auto import component (TypeScript import completions)
- ✅ Event modifiers (TypeScript completions in templates)
- ⏸️ Template HTML/Vue tags (skipped - requires HTML language service)
- ⏸️ Vue directives (skipped - requires Vue template language service)

## Performance Expectations

Verter is designed to be lightweight and focused on TypeScript IntelliSense within Vue files. 

**Expected Results:**
- Verter should show comparable or faster performance for TypeScript completions in `<script>` blocks
- Volar provides full Vue template support but with potentially higher overhead
- Both should handle multiple file operations efficiently

## Adding New Tests

1. Create test Vue files in `test-workspace/`
2. Add new test specs in `src/__tests__/`
3. Mirror tests in benchmarks at `src/__benchmarks__/`

## Adding New Benchmarks

1. Add new benchmark suites to `src/__benchmarks__/completions.bench.ts`
2. Use `bench()` from vitest for performance tests
3. Run both Volar and Verter versions for comparison
4. Set appropriate time limits with `{ time: 5000 }` option
3. Use the helper functions from `volar-server.ts` to set up the language server
4. Run tests with `vitest run` flag to avoid hanging terminals

## Future Work

- Add hover tests (hover.volar.spec.ts)
- Add diagnostics tests (diagnostics.volar.spec.ts)
- Add references tests (references.volar.spec.ts)
- Add definition tests (definitions.volar.spec.ts)
- Create equivalent tests for @verter/core
- Convert tests to benchmarks
