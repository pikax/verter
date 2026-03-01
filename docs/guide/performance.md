# Performance

::: warning Experimental
Verter is experimental software at v0.0.1-alpha.3. APIs may change without notice.
:::

Verter's template compiler is written in Rust and exposed to Node.js via NAPI-RS. On average, it compiles Vue SFC templates approximately 9x faster than Vue's JavaScript compiler.

## Benchmark Results

The following benchmarks compare Verter's Rust compiler against Vue's `@vue/compiler-sfc` across 8 real-world-style fixtures of varying size and complexity, plus a 20,000-file stress test.

| Fixture | Size | Vue (ops/s) | Verter (ops/s) | Speedup | Throughput |
|---|---|---:|---:|---:|---:|
| tiny-template | 42 B | 21,240 | 112,368 | **5.3x** | 4.50 MB/s |
| simple-interactive | 242 B | 3,140 | 52,440 | **16.7x** | 12.10 MB/s |
| list-rendering | 1.3 KB | 1,141 | 15,066 | **13.2x** | 18.85 MB/s |
| conditional-heavy | 2.0 KB | 1,288 | 14,928 | **11.6x** | 29.27 MB/s |
| form-component | 4.3 KB | 750 | 7,359 | **9.8x** | 30.58 MB/s |
| composition-heavy | 9.0 KB | 368 | 3,644 | **9.9x** | 31.86 MB/s |
| template-heavy | 8.5 KB | 1,077 | 2,914 | **2.7x** | 24.20 MB/s |
| kitchen-sink | 26.7 KB | 141 | 1,106 | **7.8x** | 28.86 MB/s |
| 20k files (stress) | 127 MB | 30.8s | 5.0s | **6.1x** | 25.16 MB/s |

**Average speedup: 9.2x.** Throughput scales from ~4.5 MB/s on tiny files to ~32 MB/s on larger components, as the fixed per-file overhead becomes proportionally smaller.

### Key Observations

- **Small files** see the highest relative speedup (up to 16.7x) because Rust's lower per-call overhead dominates.
- **Large files** show consistent throughput around 25--32 MB/s, indicating efficient linear scaling.
- **template-heavy** has the lowest speedup (2.7x) because it exercises codegen more heavily relative to parsing. This is still nearly 3x faster.
- **The 20k file stress test** processes 127 MB of Vue SFCs in 5 seconds, demonstrating real-world build performance for large projects.

## Methodology

Benchmarks use 8 Vue SFC fixtures that cover a range of component patterns:

- **tiny-template** -- Minimal template with a single element
- **simple-interactive** -- Basic reactive bindings and event handlers
- **list-rendering** -- `v-for` with keyed lists
- **conditional-heavy** -- Nested `v-if`/`v-else-if`/`v-else` chains
- **form-component** -- Form inputs with `v-model` and validation
- **composition-heavy** -- Complex `<script setup>` with composables
- **template-heavy** -- Large template with many nested elements
- **kitchen-sink** -- Everything combined in one large SFC

The 20,000-file stress test generates synthetic SFCs of varying sizes and compiles them all sequentially to measure sustained throughput.

Benchmarks are run in CI and can be triggered on any pull request by posting a `/benchmark` comment.

## Running Locally

The benchmark suite is in the `crates/verter_bench/` directory. To run it:

```bash
cargo bench --package verter_bench
```

For the comparison benchmarks against Vue's compiler:

```bash
pnpm --filter @verter/bench bench
```

## Pre-Compilation for Faster Builds

Beyond raw compiler speed, Verter's `@verter/unplugin` supports a `preCompile` option that compiles all `.vue` files during `buildStart` before bundling begins. This front-loads compilation work and caches results, so subsequent `transform()` calls return instantly:

```ts
// vite.config.ts
import { defineConfig } from 'vite'
import VerterVite from '@verter/unplugin/vite'

export default defineConfig({
  plugins: [
    VerterVite({
      preCompile: true,
    }),
  ],
})
```

See [Cross-File Optimization](./cross-file-optimization) for how `preCompile` enables whole-program analysis.

## Next Steps

- [Features](./features) -- Type safety features overview
- [Cross-File Optimization](./cross-file-optimization) -- Whole-program prop constness analysis
- [Architecture](./architecture) -- How the compiler pipeline works
