# Verter Benchmark Results

**Generated:** 2025-11-21 23:37 UTC

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
   · verter  4,388.68  0.2115   0.8578  0.2279  0.2228   0.5974   0.6702   0.7636  ±0.96%     2195
   · Volar     163.22  4.2214  18.2569  6.1266  6.9333  18.2569  18.2569  18.2569  ±8.61%       82
```

**Result:** Verter is **26.89x faster** than Volar

### single avatar.vue

```
     name          hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  3,614.34  0.2488  2.2883  0.2767  0.2597  0.5117  1.9985  2.2691  ±2.94%     1808
   · Volar     683.36  1.0713  8.7421  1.4633  1.3475  5.2024  6.0825  8.7421  ±6.09%      342
```

**Result:** Verter is **5.29x faster** than Volar

### single button.vue

```
     name          hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  5,205.94  0.1735  1.9647  0.1921  0.1805  0.2516  1.6180  1.8661  ±2.67%     2603
   · Volar     657.89  1.2170  5.5651  1.5200  1.4037  4.1052  5.2562  5.5651  ±4.60%      329
```

**Result:** Verter is **7.91x faster** than Volar

### single card.vue

```
     name           hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  27,943.64  0.0315  1.5693  0.0358  0.0339  0.0494  0.0738  1.0223  ±2.03%    13972
   · Volar    3,137.46  0.2305  3.8786  0.3187  0.2744  2.4787  2.7858  3.8431  ±5.15%     1569
```

**Result:** Verter is **8.91x faster** than Volar

### single dynamicInput.vue

```
     name           hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  11,585.33  0.0789  1.6042  0.0863  0.0820  0.1098  0.1575  1.2918  ±2.11%     5793
   · Volar    1,431.93  0.5165  5.3523  0.6984  0.6587  2.9412  3.2422  5.3523  ±4.69%      716
```

**Result:** Verter is **8.09x faster** than Volar

### single icon.vue

```
     name          hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  5,451.27  0.1655  2.6221  0.1834  0.1705  0.2688  1.8760  2.3421  ±3.13%     2726
   · Volar   2,401.45  0.3194  4.7468  0.4164  0.3597  2.6118  2.8292  3.1224  ±5.04%     1202
```

**Result:** Verter is **2.27x faster** than Volar

### single index.story.vue

```
     name        hz     min      max    mean     p75      p99     p995     p999     rme  samples
   · verter  985.37  0.9194   3.3985  1.0148  0.9415   2.9243   3.0191   3.3985  ±3.25%      493
   · Volar   187.43  4.3730  11.8595  5.3354  6.0193  11.8595  11.8595  11.8595  ±4.60%       94
```

**Result:** Verter is **5.26x faster** than Volar

### single medium.vue

```
     name           hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  48,943.22  0.0182  0.9861  0.0204  0.0196  0.0254  0.0329  0.6547  ±1.64%    24472
   · Volar    5,205.03  0.1154  6.9231  0.1921  0.1507  2.5553  3.4016  4.3130  ±7.61%     2610
```

**Result:** Verter is **9.40x faster** than Volar

### single small.vue

```
     name           hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  50,646.48  0.0176  0.9921  0.0197  0.0189  0.0263  0.0336  0.6591  ±1.55%    25324
   · Volar    6,278.97  0.1192  3.8666  0.1593  0.1290  2.1904  2.3170  2.7501  ±5.56%     3140
```

**Result:** Verter is **8.07x faster** than Volar

### single table.vue

```
     name        hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  880.96  1.0397  3.1792  1.1351  1.0706  3.0163  3.1472  3.1792  ±2.88%      441
   · Volar   211.34  3.9020  7.4130  4.7318  4.7609  7.2170  7.4130  7.4130  ±3.87%      106
```

**Result:** Verter is **4.17x faster** than Volar

## completions.bench.ts

**Description:** Tests Vue.js template completion performance in a simple component.

### template completion

```
     name          hz     min      max    mean     p75      p99     p995     p999     rme  samples
   · Volar     163.43  5.2170  11.5712  6.1190  6.3573  11.5712  11.5712  11.5712  ±4.06%       82
   · Verter  1,226.76  0.6178   3.6863  0.8152  0.8354   1.6923   2.1063   3.6863  ±2.48%      614
```

**Result:** Verter is **7.51x faster** than Volar

## real-world-components.bench.ts

**Description:** Measures completion performance in real-world Vue components with multiple edits and completion requests, simulating actual development workflows.

### Real world editing workflow

```
     name                                     hz      min      max     mean      p75      p99     p995     p999      rme  samples
   · Volar - complete editing workflow   30.3553  28.8633  36.7468  32.9431  34.6228  36.7468  36.7468  36.7468   ±3.44%       16
   · Verter - complete editing workflow   296.99   2.4268  42.9822   3.3671   3.3393  11.0282  42.9822  42.9822  ±16.18%      149
```

**Result:** Verter is **9.78x faster** than Volar


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
| single ContactInformation.vue | 4,388.68 ops/sec | 163.22 ops/sec | ✅ 26.89x faster |
| single avatar.vue | 3,614.34 ops/sec | 683.36 ops/sec | ✅ 5.29x faster |
| single button.vue | 5,205.94 ops/sec | 657.89 ops/sec | ✅ 7.91x faster |
| single card.vue | 27,943.64 ops/sec | 3,137.46 ops/sec | ✅ 8.91x faster |
| single dynamicInput.vue | 11,585.33 ops/sec | 1,431.93 ops/sec | ✅ 8.09x faster |
| single icon.vue | 5,451.27 ops/sec | 2,401.45 ops/sec | ✅ 2.27x faster |
| single index.story.vue | 985.37 ops/sec | 187.43 ops/sec | ✅ 5.26x faster |
| single medium.vue | 48,943.22 ops/sec | 5,205.03 ops/sec | ✅ 9.40x faster |
| single small.vue | 50,646.48 ops/sec | 6,278.97 ops/sec | ✅ 8.07x faster |
| single table.vue | 880.96 ops/sec | 211.34 ops/sec | ✅ 4.17x faster |
| template completion | 1,226.76 ops/sec | 163.43 ops/sec | ✅ 7.51x faster |
| Real world editing workflow | 296.99 ops/sec | 30.36 ops/sec | ✅ 9.78x faster |
