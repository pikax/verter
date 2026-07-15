# Performance

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

Verter's compiler is written in Rust and exposed to Node.js through NAPI-RS.
The project treats performance as a measured release property, not a static
marketing claim.

## Interpreting Results

Benchmark results are valid only for the recorded source revisions, dependency
versions, build profiles, platform, corpus, cache state, and work performed.
The repository therefore does not publish a timeless "average speedup" table.
Use the machine-readable artifact from a specific run when evaluating a change.

For a comparative result to support a release statement, it must:

- identify immutable Verter and comparison-compiler revisions;
- perform equivalent work, including source-map generation when maps are part
  of either measured path;
- validate behavior or output contracts before timing;
- attest cold, warm, and stateless cache behavior rather than assuming it;
- isolate workers and alternate ordering to limit cross-backend bias;
- use process RSS, not only JavaScript heap, for native-versus-JavaScript memory
  comparisons; and
- publish raw samples and corpus metadata, not only a ratio.

For Svelte, the CI rail pairs separate conformance and official-oracle behavior
tests with a timed fence that validates clean mapped output, immutable identity,
fresh stateless compilation, and isolated-process peak RSS. It is a regression
rail for that explicit corpus, not evidence that Verter is universally faster
or uses less memory than the official Svelte compiler.

## Benchmark Families

- **Compiler fixtures** measure Vue compilation across a fixed set of component
  shapes. They are useful for local regressions but do not represent every Vue
  application.
- **Svelte equal-work fence** compares the experimental Verter client compiler
  with the pinned official compiler using source maps, behavioral validation,
  fresh stateless compilation, isolated workers, and peak RSS.
- **Repository performance gate** measures first-pass and warm-pass workloads
  with explicit corpus and statistical contracts.
- **Component metadata and editor benchmarks** measure their own public paths;
  their numbers must not be generalized to compiler throughput.

## Running Locally

Run Rust microbenchmarks with:

```bash
cargo bench --package verter_bench
```

Run the JavaScript benchmark package with:

```bash
pnpm --filter @verter/benchmark bench
pnpm --filter @verter/benchmark bench:perf
```

The Svelte comparison requires a clean worktree at a full Git revision because
the result records and verifies that identity:

```bash
pnpm --filter @verter/benchmark bench:svelte:compiler
```

See the [benchmark package](https://github.com/pikax/verter/tree/main/packages/benchmark)
for fixture definitions, additional workloads, and result schemas.

## Pre-Compilation for Faster Builds

Beyond raw compiler speed, Verter's `@verter/unplugin` supports a `preCompile` option that compiles all `.vue` files during `buildStart` before bundling begins. This front-loads compilation work and caches results, so subsequent `transform()` calls return instantly:

```ts
// vite.config.ts
import { defineConfig } from "vite";
import VerterVite from "@verter/unplugin/vite";

export default defineConfig({
  plugins: [
    VerterVite({
      preCompile: true,
    }),
  ],
});
```

See [Cross-File Optimization](./cross-file-optimization) for how `preCompile` enables whole-program analysis.

## Next Steps

- [Features](./features) -- Type safety features overview
- [Cross-File Optimization](./cross-file-optimization) -- Whole-program prop constness analysis
- [Architecture](./architecture) -- How the compiler pipeline works
