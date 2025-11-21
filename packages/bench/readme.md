# Verter Bench Tests & Benchmarks

This package contains tests and benchmarks for Vue language services, comparing @vue/language-service (Volar) with @verter/language-server (Verter).

## Performance Results

**Latest benchmark results:** See [results.md](./results.md) for detailed performance comparisons.

**Quick Summary:** Verter demonstrates 5-9x faster completions in real-world Vue development scenarios.

## Structure

```
src/
  helpers/
    test-helpers.ts                    - Shared utilities for tests and benchmarks
  reporters/
    markdown-reporter.ts               - Custom reporter for benchmark results (WIP)
  server/
    volar-server.ts                    - Language server setup helper for Volar
    verter-server-direct.ts            - Direct API setup helper for Verter
  __tests__/
    completions.volar.spec.ts          - Completion tests for Volar (9 passing)
    completions.verter.spec.ts         - Completion tests for Verter (5 passing, 4 skipped)
    real-world-components.volar.spec.ts  - Real component tests for Volar (6 passing, 2 skipped)
    real-world-components.verter.spec.ts - Real component tests for Verter (6 passing, 2 skipped)
  __benchmarks__/
    completions.bench.ts               - Basic completion performance benchmarks
    real-world-components.bench.ts     - Real-world editing workflow benchmarks
scripts/
  generate-results.mjs                 - Script to generate markdown benchmark report
test-workspace/
  __bench__/                           - Real-world Vue component fixtures
    avatar.vue
    button.vue
    icon.vue
    table.vue
  fixture.vue                          - Basic test Vue files
  tsconfigProject/                     - TypeScript project test files
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

### Generate markdown report:
```bash
pnpm bench:report
```
This runs all benchmarks and generates a detailed report in `results.md` with system information and performance comparisons.

### Run benchmarks in watch mode:
```bash
pnpm bench:watch
```

### Run benchmarks with verbose comparison:
```bash
pnpm bench:compare
```

## Benchmark Categories

### Basic Completions (`completions.bench.ts`)
Simple template completion performance in a minimal component.

### Real-World Components (`real-world-components.bench.ts`)
Comprehensive benchmarks using actual Vue components:

1. **Real-world editing workflow** - Complete development cycle:
   - Open document
   - Multiple edits with content changes
   - Completion requests after each edit
   - Close document
   - Simulates actual developer workflow

This benchmark is the most representative of real-world performance, testing the complete interaction pattern developers experience during coding.

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
- ⏸️ Template tags (skipped - TODO: Implement template tag completions)
- ⏸️ HTML tags (skipped - TODO: Implement HTML tag completions)
- ⏸️ HTML events (skipped - TODO: Implement HTML event completions)
- ⏸️ Vue directives (skipped - TODO: Implement Vue directive completions)

### Real-World Component Tests

Both Volar and Verter have matching test suites for real Vue components:
- ✅ Script completions in button.vue (string methods)
- ✅ Script completions in avatar.vue (object properties)
- ✅ Script completions in table.vue (array methods)
- ⏸️ Template expression completions (skipped - needs investigation)
- ⏸️ Icon.vue props completions (skipped - requires @iconify dependencies)

## Test Helpers

The `src/helpers/test-helpers.ts` module provides shared utilities:

- `parseContentWithCursor(content)` - Extract cursor position from `|` marker
- `createTestUri(filename)` - Generate test workspace URIs
- `getCompletionLabels(completions)` - Extract completion item labels
- `assertCompletionExists(completions, expected)` - Verify completion presence
- `requestCompletionListToVueServer(server, uri, position)` - Request template completions
- `requestCompletionListToTsServer(server, file, offset)` - Request script completions

These utilities eliminate code duplication and standardize test patterns across all test files.

## Performance Expectations

Verter is designed to be lightweight and focused on TypeScript IntelliSense within Vue files. 

**Actual Results:**
- ✅ **5-9x faster** for template completions
- ✅ **5-6x faster** for real-world editing workflows (open, edit, complete, close cycles)
- ✅ Consistent performance advantage in developer-focused scenarios
- Volar provides full Vue template language service support
- Verter focuses on TypeScript completions within Vue files

## Adding New Tests

1. Create test Vue files in `test-workspace/`
2. Add new test specs in `src/__tests__/`
3. Mirror tests in benchmarks at `src/__benchmarks__/`

## Adding New Benchmarks

1. Add test Vue files to `test-workspace/__bench__/`
2. Create corresponding tests in `src/__tests__/` for both Volar and Verter
3. Add benchmark suites to `src/__benchmarks__/`
4. Use helper functions from `src/helpers/test-helpers.ts`
5. Use `bench()` from vitest for performance tests
6. Run both Volar and Verter versions for comparison
7. Generate report with `pnpm bench:report`

**Best Practices:**
- Mirror tests across Volar and Verter for fair comparison
- Use real-world Vue components for meaningful benchmarks
- Test complete workflows, not just isolated operations
- Document skipped tests with TODO comments explaining requirements

## Future Work

- Complete template language service features (HTML tags, Vue directives, events)
- Add hover tests (hover.volar.spec.ts, hover.verter.spec.ts)
- Add diagnostics tests (diagnostics.volar.spec.ts, diagnostics.verter.spec.ts)
- Add references tests (references.volar.spec.ts, references.verter.spec.ts)
- Add definition tests (definitions.volar.spec.ts, definitions.verter.spec.ts)
- Expand real-world component test coverage
- Add more complex workflow benchmarks (multiple files, imports, etc.)
