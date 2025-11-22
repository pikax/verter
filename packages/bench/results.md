# Verter Benchmark Results

**Generated:** 2025-11-22 00:23 UTC

## System Information

| Property | Value |
|----------|-------|
| Platform | darwin arm64 |
| CPU | Apple M3 |
| Cores | 8 |
| Memory | 24.00 GB |
| Node.js | v20.19.4 |

## Library Versions

| Library | Version |
|---------|---------|
| @verter/core | 1.0.0 |
| @verter/language-server | 1.0.0 |
| @volar/language-server | 2.4.23 |
| vue | 3.5.17 |
| typescript | 5.9.3 |
| vitest | 4.0.12 |
| @vue/language-server | 3.1.4 |
| @vue/language-core | 3.1.4 |
| @vue/language-service | 3.1.4 |

---

## 

**Description:** Benchmark comparison between Volar and Verter.

### single ContactInformation.vue

```
     name          hz     min      max    mean     p75      p99     p995     p999     rme  samples
   · verter  4,368.34  0.2120   0.8120  0.2289  0.2248   0.6050   0.6489   0.7638  ±0.92%     2185
   · Volar     167.57  4.2210  14.0475  5.9678  6.7682  14.0475  14.0475  14.0475  ±6.07%       84
```

**Result:** Verter is **26.07x faster** than Volar

### single avatar.vue

```
     name          hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  3,400.67  0.2547  2.7328  0.2941  0.2670  1.3252  2.2520  2.7137  ±3.29%     1701
   · Volar     654.83  1.0785  8.3722  1.5271  1.4027  5.3263  6.0065  8.3722  ±6.18%      328
```

**Result:** Verter is **5.19x faster** than Volar

### single button.vue

```
     name          hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  5,220.45  0.1754  1.6758  0.1916  0.1816  0.2574  1.4958  1.6552  ±2.37%     2611
   · Volar     659.96  1.1947  6.6625  1.5152  1.3899  4.1294  6.1842  6.6625  ±4.89%      330
```

**Result:** Verter is **7.91x faster** than Volar

### single card.vue

```
     name           hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  28,722.59  0.0316  1.0117  0.0348  0.0335  0.0404  0.0529  0.8440  ±1.67%    14362
   · Volar    3,009.82  0.2340  8.3016  0.3322  0.2762  2.5505  2.8917  3.9955  ±6.14%     1505
```

**Result:** Verter is **9.54x faster** than Volar

### single dynamicInput.vue

```
     name           hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  11,640.51  0.0787  1.5040  0.0859  0.0816  0.1147  0.1655  1.2670  ±2.08%     5821
   · Volar    1,430.33  0.5228  5.7549  0.6991  0.6535  2.9051  3.1287  5.7549  ±4.76%      717
```

**Result:** Verter is **8.14x faster** than Volar

### single icon.vue

```
     name          hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  5,415.12  0.1666  2.9096  0.1847  0.1729  0.2487  1.9047  2.0828  ±3.04%     2708
   · Volar   2,392.49  0.3193  4.8033  0.4180  0.3553  2.7188  2.9451  4.0472  ±5.31%     1197
```

**Result:** Verter is **2.26x faster** than Volar

### single index.story.vue

```
     name        hz     min      max    mean     p75      p99     p995     p999     rme  samples
   · verter  957.00  0.9340   3.7956  1.0449  0.9610   3.2254   3.5044   3.7956  ±3.55%      479
   · Volar   190.75  4.3167  11.3222  5.2424  5.9465  11.3222  11.3222  11.3222  ±4.61%       96
```

**Result:** Verter is **5.02x faster** than Volar

### single medium.vue

```
     name           hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  49,388.59  0.0180  0.8568  0.0202  0.0195  0.0241  0.0284  0.6308  ±1.52%    24695
   · Volar    6,392.60  0.1135  4.9633  0.1564  0.1238  2.3052  2.6201  2.9227  ±6.23%     3197
```

**Result:** Verter is **7.73x faster** than Volar

### single small.vue

```
     name           hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  49,790.25  0.0176  3.6120  0.0201  0.0191  0.0252  0.0389  0.6787  ±2.09%    24896
   · Volar    6,121.88  0.1187  5.7655  0.1633  0.1289  2.2057  2.3827  2.7626  ±6.07%     3061
```

**Result:** Verter is **8.13x faster** than Volar

### single table.vue

```
     name        hz     min      max    mean     p75     p99     p995     p999     rme  samples
   · verter  882.54  1.0485   3.1776  1.1331  1.0723  2.7730   2.9206   3.1776  ±2.67%      442
   · Volar   211.22  3.8897  14.1547  4.7344  4.6862  6.8462  14.1547  14.1547  ±5.09%      106
```

**Result:** Verter is **4.18x faster** than Volar

## completions.bench.ts

**Description:** Tests Vue.js template completion performance in a simple component.

### template completion

```
     name          hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Volar     181.11  4.9625  7.5797  5.5216  5.7250  7.5797  7.5797  7.5797  ±2.09%       91
   · Verter  1,329.93  0.5245  2.9020  0.7519  0.7798  1.8697  1.9796  2.9020  ±2.65%      665
```

**Result:** Verter is **7.34x faster** than Volar

## real-world-components.bench.ts

**Description:** Measures completion performance in real-world Vue components with multiple edits and completion requests, simulating actual development workflows.

### Real world editing workflow

```
     name                                     hz      min      max     mean      p75      p99     p995     p999     rme  samples
   · Volar - complete editing workflow   28.0610  31.6541  41.2558  35.6366  37.5880  41.2558  41.2558  41.2558  ±3.90%       15
   · Verter - complete editing workflow   373.33   1.9436   5.2350   2.6786   2.8962   4.9733   5.2350   5.2350  ±3.28%      187
```

**Result:** Verter is **13.30x faster** than Volar


---

## How to Run Benchmarks

To reproduce these results:

```bash
pnpm i
# Run all benchmarks and generate this report
pnpm bench:report

# Or run benchmarks without generating report
pnpm bench

# Run benchmarks in watch mode
pnpm bench:watch

# Run benchmarks with verbose output
pnpm bench:compare
```

---

## Performance Summary

> **⚠️ Important Disclaimer**
>
> These benchmarks are **simulated tests** and may not fully represent real-world performance characteristics:
>
> - The Volar implementation used here is a test harness and may introduce overhead not present in production VS Code environments
> - **Verter is in heavy development** and currently lacks many features required for real-world usage (template completions, Vue directives, HTML tag completions, etc.)
> - These results primarily demonstrate TypeScript completion performance within Vue files
> - Production performance will vary based on project size, configuration, and usage patterns
>
> Use these benchmarks as **relative indicators** rather than absolute performance guarantees.

| Benchmark | Verter | Volar | Performance |
|-----------|--------|-------|-------------|
| single ContactInformation.vue | 4,368.34 ops/sec | 167.57 ops/sec | ✅ 26.07x faster |
| single avatar.vue | 3,400.67 ops/sec | 654.83 ops/sec | ✅ 5.19x faster |
| single button.vue | 5,220.45 ops/sec | 659.96 ops/sec | ✅ 7.91x faster |
| single card.vue | 28,722.59 ops/sec | 3,009.82 ops/sec | ✅ 9.54x faster |
| single dynamicInput.vue | 11,640.51 ops/sec | 1,430.33 ops/sec | ✅ 8.14x faster |
| single icon.vue | 5,415.12 ops/sec | 2,392.49 ops/sec | ✅ 2.26x faster |
| single index.story.vue | 957 ops/sec | 190.75 ops/sec | ✅ 5.02x faster |
| single medium.vue | 49,388.59 ops/sec | 6,392.6 ops/sec | ✅ 7.73x faster |
| single small.vue | 49,790.25 ops/sec | 6,121.88 ops/sec | ✅ 8.13x faster |
| single table.vue | 882.54 ops/sec | 211.22 ops/sec | ✅ 4.18x faster |
| template completion | 1,329.93 ops/sec | 181.11 ops/sec | ✅ 7.34x faster |
| Real world editing workflow | 373.33 ops/sec | 28.06 ops/sec | ✅ 13.30x faster |
