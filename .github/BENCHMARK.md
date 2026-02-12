# Benchmark System

This document describes the Verter benchmark system for comparing compilation performance between Vue's official compiler (@vue/compiler-sfc) and Verter.

## Overview

The benchmark system measures and compares the performance of Vue and Verter compilers across a range of realistic Vue SFC fixtures, from minimal templates to complex applications.

### Key Features

- **Fair Comparison**: Vue uses 2-phase compilation (parse → compileScript → compileTemplate), Verter uses single-pass compilation
- **Statistical Rigor**: Uses [tinybench](https://github.com/tinylibs/tinybench) for accurate benchmarking with warmup iterations
- **Multiple Metrics**: Measures time (ms), throughput (MB/s), operations per second, and percentiles (p50/p95/p99)
- **Comprehensive Fixtures**: 8 fixtures ranging from 50 bytes to 50+ KB covering all Vue features
- **CI Integration**: Automated PR checks with pass/warning/fail status based on performance

## Fixtures

The benchmark suite includes 8 carefully designed fixtures:

| Fixture | Description | Features | Size |
|---------|-------------|----------|------|
| `tiny-template` | Minimal single element | Basic template | ~50 B |
| `simple-interactive` | Counter with button click | refs, event handlers | ~200 B |
| `list-rendering` | Todo list with v-for | v-for, v-model, dynamic lists | ~1 KB |
| `conditional-heavy` | Multiple nested v-if/v-else | Conditional rendering chains | ~2 KB |
| `form-component` | Registration form | Forms, v-model, validation | ~3 KB |
| `composition-heavy` | User profile manager | Composition API, watchers, lifecycle hooks | ~8 KB |
| `template-heavy` | Dashboard layout | Large template structure, many elements | ~10 KB |
| `kitchen-sink` | Task management app | All features combined | ~25 KB |

Each fixture tests specific aspects of SFC compilation:
- **Script Compilation**: `<script setup>` processing, TypeScript, composition API
- **Template Compilation**: Directives (v-for, v-if, v-model, v-bind, v-on), interpolation, events
- **Style Processing**: Scoped styles detection
- **Binding Analysis**: Reactive binding detection and metadata generation

## Metrics

### Time Metrics
- **Mean**: Average compilation time across all iterations
- **Median (p50)**: 50th percentile - half of compilations are faster
- **p95/p99**: 95th/99th percentile - measure tail latency
- **Min/Max**: Best and worst case performance

### Performance Metrics
- **Operations per Second (ops/sec)**: How many compilations per second
- **Throughput (MB/s)**: Megabytes of source code processed per second
- **Speedup Factor**: Ratio of Vue time to Verter time (>1.0 means Verter is faster)

### Status Determination

Results are categorized into three statuses:

| Status | Criteria | Meaning |
|--------|----------|---------|
| ✅ **Pass** | Speedup ≥ 1.0x | Verter is at least as fast as Vue |
| ⚠️ **Warning** | 0.5x ≤ Speedup < 1.0x | Verter is 50-99% of Vue's speed |
| ❌ **Fail** | Speedup < 0.5x | Verter is less than 50% of Vue's speed |

The overall status is determined by the worst individual fixture status (any failure = overall fail).

## Running Benchmarks

### Local Execution

```bash
# Run benchmarks with console output
pnpm --filter @verter/benchmark bench

# Generate JSON report for CI
pnpm --filter @verter/benchmark bench:json
```

Results are saved to:
- `benchmark-results/results.md` - Human-readable markdown report
- `benchmark-results/results.json` - Machine-readable JSON for CI/CD

### CI/CD Execution

#### Manual Trigger

1. Go to **Actions** → **Benchmark Workflow**
2. Click **Run workflow**
3. Optionally specify a branch/commit to benchmark
4. View results in workflow summary

#### PR Comment Trigger

Comment `/benchmark` on any pull request to trigger benchmarks on that PR's branch.

**Requirements:**
- Must be a collaborator with write access
- PR must be open
- Comment must start with `/benchmark` (can have text after)

**Example:**
```
/benchmark
```

or

```
/benchmark please run performance tests
```

#### Post-Comment Workflow

1. **Permission Check**: Verifies commenter has write access
2. **Branch Checkout**: Checks out PR branch
3. **Build**: Compiles Rust native package and benchmark package
4. **Benchmark**: Runs all 8 fixtures with statistical analysis
5. **Report**: Posts detailed results as PR comment
6. **Check Run**: Creates GitHub check with pass/warning/fail status

### Reading Results

#### PR Comment Format

```markdown
## ✅ Benchmark Results

**Overall Status:** 🟢 **PASS**

### Summary

- Total Fixtures: 8
- ✅ Passed: 7
- ⚠️ Warnings: 1
- ❌ Failed: 0
- Average Speedup: **1.35x**

### Detailed Results

| Fixture | Size | Vue (ms) | Verter (ms) | Speedup | Throughput | Status |
|---------|------|----------|-------------|---------|------------|--------|
| tiny-template | 0.05 KB | 0.12 | 0.08 | 1.50x | 0.64 MB/s | ✅ |
| simple-interactive | 0.20 KB | 0.25 | 0.18 | 1.39x | 1.11 MB/s | ✅ |
...
```

#### Understanding Speedup

- **1.5x**: Verter is 50% faster than Vue
- **1.0x**: Verter and Vue have equal performance
- **0.8x**: Verter is 20% slower than Vue (Warning)
- **0.4x**: Verter is 60% slower than Vue (Fail)

#### Understanding Throughput

Throughput measures data processing speed:
- Calculated as: `file_size_bytes / compilation_time_seconds / 1_048_576`
- Higher is better
- Useful for understanding performance on large files

## Implementation Details

### Architecture

```
packages/benchmark/
├── package.json          # Dependencies: tinybench, @vue/compiler-sfc, @verter/native
├── src/
│   ├── index.ts         # Main benchmark runner
│   ├── fixtures/        # 8 Vue SFC test files
│   │   ├── tiny-template.vue
│   │   ├── simple-interactive.vue
│   │   ├── list-rendering.vue
│   │   ├── conditional-heavy.vue
│   │   ├── form-component.vue
│   │   ├── composition-heavy.vue
│   │   ├── template-heavy.vue
│   │   └── kitchen-sink.vue
│   ├── compilers/       # Compiler wrappers
│   │   ├── vue.ts       # 2-phase Vue compilation
│   │   └── verter.ts    # Single-pass Verter compilation
│   └── utils/           # Utilities
│       ├── stats.ts     # Statistical calculations
│       └── report.ts    # Report generation
└── README.md
```

### Vue Compiler Wrapper

```typescript
// 2-phase compilation as per Vue's design
export function compileVue(source: string, filename: string) {
  // Phase 1: Parse SFC
  const { descriptor } = parse(source, { filename })
  
  // Phase 2: Compile script with binding analysis
  const scriptResult = compileScript(descriptor, { id: filename })
  const bindingMetadata = scriptResult.bindings
  
  // Phase 3: Compile template with bindings
  const templateResult = compileTemplate({
    source: descriptor.template.content,
    compilerOptions: { bindingMetadata }
  })
  
  return { code: scriptResult.content + templateResult.code }
}
```

### Verter Compiler Wrapper

```typescript
// Single-pass compilation with built-in timing
export function compileVerter(source: string, filename: string) {
  const result = compile(source, { filename })
  
  return {
    code: result.code,
    durationMs: result.duration_ms // Rust-measured compilation time
  }
}
```

### Tinybench Configuration

```typescript
const bench = new Bench({
  time: 1000,           // Run for at least 1 second
  warmupIterations: 10, // 10 warmup runs before measurement
  iterations: 50        // Minimum 50 measured iterations
})

// Tinybench handles timing automatically - no manual performance.now()
bench.add('Vue', () => compileVue(source, filename))
bench.add('Verter', () => compileVerter(source, filename))

await bench.run()
```

## Troubleshooting

### Benchmarks Running Slow

- **Cause**: Cold start, CPU throttling, or background processes
- **Solution**: Benchmarks include warmup iterations; ensure CI runners have consistent resources

### Inconsistent Results

- **Cause**: Resource contention on CI runners
- **Solution**: Results are statistical averages over many iterations to reduce variance

### Warning Status on Passing Builds

- **Cause**: Verter 50-99% of Vue speed on some fixtures
- **Interpretation**: Acceptable for early development; focus on correctness first, then optimize
- **Action**: Investigate which fixtures are slower and profile those cases

### All Benchmarks Failing

- **Cause**: Compilation errors in Verter or regression
- **Solution**: Check error messages in detailed results, run locally for debugging

### JSON Output Issues

- **Cause**: Compilation output mixed with JSON
- **Solution**: Use `bench:json` script which redirects properly

## Best Practices

### Adding New Fixtures

1. Create `.vue` file in `src/fixtures/`
2. Add filename to `FIXTURES` array in `src/index.ts`
3. Run locally to verify: `pnpm bench`
4. Commit fixture and updated benchmarks

### Interpreting Results

- **Focus on average speedup** across all fixtures, not individual outliers
- **Warning status is acceptable** during development focus on correctness
- **Investigate failures** immediately - indicates regression or bug
- **Compare trends** over time rather than absolute numbers

### Performance Optimization

1. **Profile before optimizing**: Run benchmarks to establish baseline
2. **Focus on slow fixtures**: Optimize the worst-performing cases first
3. **Verify improvements**: Re-run benchmarks after changes
4. **Watch for regressions**: Monitor PR benchmark results

## FAQ

**Q: Why use tinybench instead of Criterion (Rust benchmarks)?**

A: We need an apples-to-apples comparison with Vue's JavaScript compiler. Tinybench allows fair comparison of Vue (@vue/compiler-sfc) and Verter (Rust via NAPI) in the same environment.

**Q: Why 8 fixtures? Why not more?**

A: 8 fixtures provide good coverage of Vue features while keeping benchmark runtime manageable (~30-60 seconds). More fixtures would slow CI without adding significant value.

**Q: What's the difference between integration tests and benchmarks?**

A: Integration tests verify Verter works with real projects (Vuetify, PrimeVue, etc.). Benchmarks measure pure compilation performance with controlled fixtures.

**Q: Can I run just one fixture?**

A: Not directly, but you can modify the `FIXTURES` array in `src/index.ts` temporarily for local testing.

**Q: Why does Verter include `duration_ms` from Rust?**

A: This measures pure Rust compilation time without Node.js/NAPI overhead. It's informational but not used for benchmark comparison (tinybench times are used).

**Q: What if Verter is slower than Vue?**

A: Early in development, correctness matters more than speed. Warning status (50-99% of Vue) is acceptable. Focus on passing integration tests first, then optimize hot paths revealed by benchmarks.

## Related Documentation

- [Integration Tests](.github/INTEGRATION_TEST.md) - Testing against real-world projects
- [Architecture](docs/architecture.md) - Overall Verter architecture
- [Contributing](CONTRIBUTING.md) - Development guidelines
