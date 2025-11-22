# Verter Benchmark Results

**Generated:** 2025-11-22 10:22 UTC

## System Information

| Property | Value |
|----------|-------|
| Platform | win32 x64 |
| CPU | AMD Ryzen 9 7950X 16-Core Processor             |
| Cores | 32 |
| Memory | 127.15 GB |
| Node.js | v20.19.0 |

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

## parser.bench.ts

**Description:** Vue file parsing and processing performance comparison with three distinct benchmarks:

- **parser**: Raw parsing to AST (Verter) vs full virtual code generation (Volar)
- **process**: Processing parsed AST into usable structures (Verter) vs extracting embedded codes (Volar)
- **parser + process**: End-to-end parsing and processing combined

Note: Verter uses a two-stage approach (parse AST → process), while Volar generates virtual TypeScript code directly during parsing.

Suites: 10

### single ContactInformation.vue [##------------------] 1/10 (10%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               5,817.71  0.1441  0.9852  0.1719  0.1692  0.6617  0.7176  0.8066  ±1.39%     2909
   · verter AcornLoose    6,697.05  0.1270  2.8716  0.1493  0.1461  0.4389  0.4920  0.5866  ±1.50%     3349
   · Volar              180,008.87  0.0015  3.9174  0.0056  0.0026  0.0067  0.0197  1.6593  ±8.85%    90254
```

**Result:** Volar is **30.94x faster** than Verter

### single avatar.vue [####----------------] 2/10 (20%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               4,345.86  0.1854  3.1293  0.2301  0.2110  1.8738  2.2998  2.7669  ±3.93%     2175
   · verter AcornLoose    7,723.05  0.1101  0.7109  0.1295  0.1299  0.2831  0.5282  0.6255  ±1.03%     3862
   · Volar              178,519.93  0.0015  4.7770  0.0056  0.0025  0.0284  0.0322  1.5528  ±8.54%    89260
```

**Result:** Volar is **41.08x faster** than Verter

### single button.vue [######--------------] 3/10 (30%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               6,718.74  0.1225  2.4805  0.1488  0.1373  0.2686  1.6446  2.1159  ±3.28%     3360
   · verter AcornLoose   10,535.93  0.0828  0.8563  0.0949  0.0908  0.2332  0.5025  0.6328  ±1.22%     5268
   · Volar              181,990.65  0.0015  3.2029  0.0055  0.0025  0.0072  0.0355  1.6597  ±8.69%    91114
```

**Result:** Volar is **27.09x faster** than Verter

### single card.vue [########------------] 4/10 (40%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999      rme  samples
   · Verter              34,428.24  0.0249  1.2160  0.0290  0.0271  0.0467  0.0733  0.9000   ±2.01%    17215
   · verter AcornLoose   37,057.27  0.0184  1.2806  0.0270  0.0287  0.0574  0.0687  0.7042   ±1.81%    18529
   · Volar              134,353.45  0.0015  6.9893  0.0074  0.0037  0.0084  0.0168  2.3413  ±10.42%    67318
```

**Result:** Volar is **3.90x faster** than Verter

### single dynamicInput.vue [##########----------] 5/10 (50%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999      rme  samples
   · Verter              11,714.42  0.0565  2.2896  0.0854  0.0885  0.1517  0.2299  1.8365   ±3.10%     5859
   · verter AcornLoose   17,669.52  0.0388  0.9666  0.0566  0.0608  0.1053  0.1296  0.7268   ±1.54%     8835
   · Volar              124,957.65  0.0016  5.5063  0.0080  0.0037  0.0102  0.0257  2.5889  ±10.94%    62479
```

**Result:** Volar is **10.67x faster** than Verter

### single icon.vue [############--------] 6/10 (60%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999      rme  samples
   · Verter               5,413.83  0.1146  4.5792  0.1847  0.1865  0.3941  2.5364  2.9887   ±4.40%     2707
   · verter AcornLoose   10,810.74  0.0611  0.9707  0.0925  0.0981  0.1726  0.3498  0.8124   ±1.43%     5406
   · Volar              133,767.65  0.0015  3.4438  0.0075  0.0037  0.0089  0.0297  2.3317  ±10.22%    66884
```

**Result:** Volar is **24.71x faster** than Verter

### single index.story.vue [##############------] 7/10 (70%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               1,046.00  0.6377  4.3862  0.9560  0.9447  3.6008  3.7656  4.3862  ±4.90%      524
   · verter AcornLoose    1,906.81  0.3539  4.7589  0.5244  0.5535  1.0965  1.3353  4.7589  ±2.34%      954
   · Volar              167,643.73  0.0015  4.7679  0.0060  0.0028  0.0070  0.0235  1.8674  ±9.34%    83822
```

**Result:** Volar is **160.27x faster** than Verter

### single medium.vue [################----] 8/10 (80%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999      rme  samples
   · Verter              55,594.00  0.0140  6.1346  0.0180  0.0158  0.0380  0.0518  0.8624   ±3.26%    27798
   · verter AcornLoose   58,254.60  0.0114  0.9929  0.0172  0.0172  0.0378  0.0501  0.5979   ±1.68%    29128
   · Volar              134,137.57  0.0015  5.5733  0.0075  0.0036  0.0096  0.0340  2.3160  ±10.17%    67294
```

**Result:** Volar is **2.41x faster** than Verter

### single small.vue [##################--] 9/10 (90%)

```
     name                       hz     min      max    mean     p75     p99    p995    p999      rme  samples
   · Verter              46,434.68  0.0137   1.2708  0.0215  0.0220  0.0528  0.0786  0.8943   ±2.19%    23218
   · verter AcornLoose   58,571.53  0.0108   1.0110  0.0171  0.0169  0.0383  0.0569  0.6850   ±1.96%    29286
   · Volar              114,302.79  0.0016  10.1633  0.0087  0.0040  0.0394  0.0490  2.5323  ±11.13%    57475
```

**Result:** Volar is **2.46x faster** than Verter

### single table.vue [####################] 10/10 (100%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999      rme  samples
   · Verter                 913.45  0.7104  4.2061  1.0947  1.1383  3.6955  3.8575  4.2061   ±4.08%      457
   · verter AcornLoose    1,285.62  0.5082  1.5508  0.7778  0.8358  1.3182  1.4027  1.5508   ±1.62%      643
   · Volar              127,743.61  0.0015  4.0231  0.0078  0.0038  0.0093  0.0310  2.4725  ±10.45%    63874
```

**Result:** Volar is **139.85x faster** than Verter


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
> - **Both systems use LSP+IPC architecture** for fair comparison in completion benchmarks
> - **Parser benchmarks measure different scopes**: Verter does AST parsing, Volar does full virtual code generation
> - **Verter is in heavy development** and currently lacks many features required for real-world usage (template completions, Vue directives, HTML tag completions, etc.)
> - These results primarily demonstrate TypeScript completion performance within Vue files
> - Production performance will vary based on project size, configuration, and usage patterns
>
> Use these benchmarks as **relative indicators** rather than absolute performance guarantees.

| Benchmark | Verter | Volar | Performance |
|-----------|--------|-------|-------------|
| single ContactInformation.vue | 5,817.71 ops/sec | 180,008.87 ops/sec | ⚠️ 30.94x slower |
| single avatar.vue | 4,345.86 ops/sec | 178,519.93 ops/sec | ⚠️ 41.08x slower |
| single button.vue | 6,718.74 ops/sec | 181,990.65 ops/sec | ⚠️ 27.09x slower |
| single card.vue | 34,428.24 ops/sec | 134,353.45 ops/sec | ⚠️ 3.90x slower |
| single dynamicInput.vue | 11,714.42 ops/sec | 124,957.65 ops/sec | ⚠️ 10.67x slower |
| single icon.vue | 5,413.83 ops/sec | 133,767.65 ops/sec | ⚠️ 24.71x slower |
| single index.story.vue | 1,046 ops/sec | 167,643.73 ops/sec | ⚠️ 160.27x slower |
| single medium.vue | 55,594 ops/sec | 134,137.57 ops/sec | ⚠️ 2.41x slower |
| single small.vue | 46,434.68 ops/sec | 114,302.79 ops/sec | ⚠️ 2.46x slower |
| single table.vue | 913.45 ops/sec | 127,743.61 ops/sec | ⚠️ 139.85x slower |
