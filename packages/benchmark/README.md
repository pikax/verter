# @verter/benchmark

Performance benchmarks comparing Vue and Verter SFC compilation.

## Overview

This package benchmarks Vue and Verter compilers using real-world Vue SFC fixtures:
- **Vue**: 2-phase compilation (parse → compileScript → compileTemplate)
- **Verter**: Single-pass compilation via Rust/NAPI

## Metrics

- **Time**: Compilation time in milliseconds (p50/p95/p99)
- **Throughput**: Megabytes per second (MB/s)
- **Memory**: Peak heap used during compilation (MB)
- **Operations**: Operations per second
- **Speedup**: Verter performance relative to Vue (e.g., 2.0x = twice as fast)

## Fixtures

Eight fixtures from tiny to complex:
1. `tiny-template.vue` - Minimal single element
2. `simple-interactive.vue` - Basic interactivity with ref
3. `list-rendering.vue` - v-for with dynamic lists
4. `conditional-heavy.vue` - Multiple v-if/v-else chains
5. `form-component.vue` - Form with v-model bindings
6. `composition-heavy.vue` - Complex script setup
7. `template-heavy.vue` - Large template structure
8. `kitchen-sink.vue` - All features combined

## Usage

```bash
# Run benchmarks with console output
pnpm bench

# Generate JSON report
pnpm bench:json
```

## CI Integration

Triggered via PR comment `/benchmark` or manually through GitHub Actions.

Results posted as PR comments with:
- ✅ **Pass**: Verter ≥ Vue performance (speedup ≥ 1.0x)
- ⚠️ **Warning**: Verter 50-99% of Vue performance (speedup 0.5-1.0x)
- ❌ **Fail**: Verter < 50% of Vue performance (speedup < 0.5x)

## Stress Test

Includes a stress test with ~20,000 files (created by repeating all fixtures):
- **Default**: 1 iteration, 1 warmup (fast)
- Useful for measuring aggregate throughput on large compilations
- Reports per-file compilation time and overall MB/s throughput
