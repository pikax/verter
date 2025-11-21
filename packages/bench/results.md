# Verter Benchmark Results

**Generated:** 2025-11-21 23:13 UTC

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
   · verter  4,366.44  0.2098   0.8355  0.2290  0.2219   0.6546   0.7340   0.8114  ±1.15%     2184
   · Volar     162.70  4.2195  11.9311  6.1464  7.1016  11.9311  11.9311  11.9311  ±6.12%       82
```

**Result:** Verter is **26.84x faster** than Volar

### single avatar.vue

```
     name          hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  3,545.27  0.2529  2.7957  0.2821  0.2632  1.7076  2.0520  2.6750  ±3.08%     1773
   · Volar     687.30  1.0671  6.1133  1.4550  1.3710  4.8923  5.2335  6.1133  ±5.44%      344
```

**Result:** Verter is **5.16x faster** than Volar

### single button.vue

```
     name          hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  5,215.11  0.1741  1.8248  0.1918  0.1802  0.2945  1.6347  1.7833  ±2.57%     2608
   · Volar     659.53  1.2280  4.7052  1.5162  1.4259  4.2503  4.6556  4.7052  ±4.33%      331
```

**Result:** Verter is **7.91x faster** than Volar

### single card.vue

```
     name           hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  27,208.56  0.0321  4.5414  0.0368  0.0343  0.0707  0.0988  0.9460  ±2.59%    13626
   · Volar    3,111.39  0.2308  4.9028  0.3214  0.2783  2.5385  2.7863  3.9169  ±5.30%     1556
```

**Result:** Verter is **8.74x faster** than Volar

### single dynamicInput.vue

```
     name           hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  11,602.33  0.0786  1.3972  0.0862  0.0816  0.1269  0.1609  1.2721  ±2.15%     5802
   · Volar    1,473.31  0.5245  4.9932  0.6787  0.6340  2.7738  2.9480  4.9932  ±4.32%      737
```

**Result:** Verter is **7.88x faster** than Volar

### single icon.vue

```
     name          hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  5,358.52  0.1665  3.5422  0.1866  0.1716  0.3214  1.9555  2.4068  ±3.40%     2680
   · Volar   2,407.61  0.3155  4.1118  0.4154  0.3660  2.7585  3.0678  3.7428  ±5.13%     1204
```

**Result:** Verter is **2.23x faster** than Volar

### single index.story.vue

```
     name        hz     min      max    mean     p75      p99     p995     p999     rme  samples
   · verter  959.51  0.9208   4.2706  1.0422  0.9487   3.2724   3.3229   4.2706  ±3.69%      480
   · Volar   172.86  4.3556  14.3912  5.7850  6.8736  14.3912  14.3912  14.3912  ±5.75%       87
```

**Result:** Verter is **5.55x faster** than Volar

### single medium.vue

```
     name           hz     min      max    mean     p75     p99    p995    p999     rme  samples
   · verter  47,719.13  0.0184   1.0240  0.0210  0.0200  0.0320  0.0567  0.6534  ±1.53%    23860
   · Volar    5,542.84  0.1151  10.7230  0.1804  0.1267  2.4245  2.8935  6.8568  ±9.12%     2772
```

**Result:** Verter is **8.61x faster** than Volar

### single small.vue

```
     name           hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  49,547.43  0.0179  0.8803  0.0202  0.0194  0.0257  0.0351  0.6687  ±1.55%    24774
   · Volar    6,340.55  0.1194  3.7795  0.1577  0.1284  2.1819  2.3307  2.6967  ±5.50%     3171
```

**Result:** Verter is **7.81x faster** than Volar

### single table.vue

```
     name        hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  858.43  1.0390  4.4220  1.1649  1.0795  3.1700  3.5752  4.4220  ±3.37%      430
   · Volar   211.38  3.9226  7.3338  4.7308  4.7485  7.1698  7.3338  7.3338  ±3.66%      106
```

**Result:** Verter is **4.06x faster** than Volar

## completions.bench.ts

**Description:** Tests Vue.js template completion performance in a simple component.

### template completion

```
     name          hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Volar     169.35  5.2207  8.2182  5.9048  6.0656  8.2182  8.2182  8.2182  ±2.47%       85
   · Verter  1,349.07  0.5717  6.2244  0.7413  0.7543  1.7859  2.0620  6.2244  ±3.08%      675
```

**Result:** Verter is **7.97x faster** than Volar

## real-world-components.bench.ts

**Description:** Measures completion performance in real-world Vue components with multiple edits and completion requests, simulating actual development workflows.

### Real world editing workflow

```
     name                                     hz      min      max     mean      p75      p99     p995     p999     rme  samples
   · Volar - complete editing workflow   29.9568  31.0575  35.5886  33.3814  34.7853  35.5886  35.5886  35.5886  ±2.40%       15
   · Verter - complete editing workflow   173.04   4.9445   7.8132   5.7790   6.2670   7.8132   7.8132   7.8132  ±2.23%       87
```

**Result:** Verter is **5.78x faster** than Volar


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
| single ContactInformation.vue | 4,366.44 ops/sec | 162.7 ops/sec | ✅ 26.84x faster |
| single avatar.vue | 3,545.27 ops/sec | 687.3 ops/sec | ✅ 5.16x faster |
| single button.vue | 5,215.11 ops/sec | 659.53 ops/sec | ✅ 7.91x faster |
| single card.vue | 27,208.56 ops/sec | 3,111.39 ops/sec | ✅ 8.74x faster |
| single dynamicInput.vue | 11,602.33 ops/sec | 1,473.31 ops/sec | ✅ 7.88x faster |
| single icon.vue | 5,358.52 ops/sec | 2,407.61 ops/sec | ✅ 2.23x faster |
| single index.story.vue | 959.51 ops/sec | 172.86 ops/sec | ✅ 5.55x faster |
| single medium.vue | 47,719.13 ops/sec | 5,542.84 ops/sec | ✅ 8.61x faster |
| single small.vue | 49,547.43 ops/sec | 6,340.55 ops/sec | ✅ 7.81x faster |
| single table.vue | 858.43 ops/sec | 211.38 ops/sec | ✅ 4.06x faster |
| template completion | 1,349.07 ops/sec | 169.35 ops/sec | ✅ 7.97x faster |
| Real world editing workflow | 173.04 ops/sec | 29.96 ops/sec | ✅ 5.78x faster |
