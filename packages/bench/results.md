# Verter Benchmark Results

**Generated:** 2025-11-22 10:38 UTC

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
   · Verter               6,180.04  0.1400  0.8231  0.1618  0.1607  0.5408  0.5754  0.7487  ±1.15%     3091
   · verter AcornLoose    6,982.94  0.1256  2.2890  0.1432  0.1421  0.3247  0.3733  0.4500  ±1.11%     3492
   · Volar              233,443.40  0.0014  2.5472  0.0043  0.0020  0.0054  0.0108  1.2481  ±7.25%   116789
```

**Result:** Volar is **37.77x faster** than Verter

### single avatar.vue [####----------------] 2/10 (20%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               4,751.07  0.1799  2.7710  0.2105  0.1983  1.3974  1.5674  1.8208  ±2.83%     2376
   · verter AcornLoose    7,647.10  0.1163  0.6146  0.1308  0.1321  0.2367  0.3904  0.5013  ±0.74%     3824
   · Volar              233,525.25  0.0014  2.2030  0.0043  0.0020  0.0057  0.0104  1.2617  ±7.12%   116763
```

**Result:** Volar is **49.15x faster** than Verter

### single button.vue [######--------------] 3/10 (30%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               7,175.98  0.1206  1.5301  0.1394  0.1343  0.2146  1.1166  1.3814  ±2.16%     3588
   · verter AcornLoose   10,733.57  0.0854  0.4586  0.0932  0.0930  0.1405  0.3414  0.3905  ±0.66%     5367
   · Volar              234,194.31  0.0014  2.0730  0.0043  0.0020  0.0053  0.0091  1.2442  ±7.15%   117351
```

**Result:** Volar is **32.64x faster** than Verter

### single card.vue [########------------] 4/10 (40%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter              36,580.17  0.0238  1.0334  0.0273  0.0259  0.0413  0.0577  0.6802  ±1.56%    18291
   · verter AcornLoose   47,248.63  0.0190  0.4632  0.0212  0.0204  0.0335  0.0460  0.2955  ±0.79%    23625
   · Volar              236,496.00  0.0014  2.7001  0.0042  0.0020  0.0055  0.0085  1.2003  ±7.13%   118248
```

**Result:** Volar is **6.47x faster** than Verter

### single dynamicInput.vue [##########----------] 5/10 (50%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter              15,650.00  0.0549  2.0321  0.0639  0.0602  0.1197  0.1601  0.9737  ±2.02%     7836
   · verter AcornLoose   22,788.57  0.0404  0.4116  0.0439  0.0426  0.0692  0.0927  0.3638  ±0.76%    11395
   · Volar              239,727.82  0.0014  2.0479  0.0042  0.0020  0.0051  0.0096  1.1835  ±6.91%   119867
```

**Result:** Volar is **15.32x faster** than Verter

### single icon.vue [############--------] 6/10 (60%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               7,711.50  0.1109  2.0909  0.1297  0.1209  0.1911  1.5212  1.8206  ±3.04%     3865
   · verter AcornLoose   14,160.25  0.0655  0.5735  0.0706  0.0688  0.1127  0.1598  0.4128  ±0.75%     7081
   · Volar              239,703.37  0.0014  1.9495  0.0042  0.0020  0.0052  0.0104  1.1672  ±6.93%   120012
```

**Result:** Volar is **31.08x faster** than Verter

### single index.story.vue [##############------] 7/10 (70%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               1,428.94  0.6048  3.1379  0.6998  0.6384  2.3319  2.5607  3.1379  ±3.42%      715
   · verter AcornLoose    2,476.18  0.3802  0.8407  0.4038  0.3985  0.6631  0.6799  0.8098  ±0.63%     1239
   · Volar              238,407.02  0.0014  2.0896  0.0042  0.0020  0.0050  0.0095  1.2128  ±7.06%   119306
```

**Result:** Volar is **166.84x faster** than Verter

### single medium.vue [################----] 8/10 (80%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter              63,312.39  0.0136  2.6795  0.0158  0.0149  0.0247  0.0313  0.4892  ±1.71%    31657
   · verter AcornLoose   79,580.65  0.0113  0.4791  0.0126  0.0122  0.0192  0.0228  0.2795  ±0.93%    39791
   · Volar              237,767.75  0.0014  1.9698  0.0042  0.0020  0.0051  0.0098  1.2237  ±7.04%   119105
```

**Result:** Volar is **3.76x faster** than Verter

### single small.vue [##################--] 9/10 (90%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter              63,003.23  0.0132  4.2019  0.0159  0.0145  0.0302  0.0425  0.5012  ±2.21%    31519
   · verter AcornLoose   82,228.83  0.0108  0.5206  0.0122  0.0117  0.0175  0.0210  0.2847  ±0.98%    41115
   · Volar              241,495.71  0.0014  2.0519  0.0041  0.0020  0.0053  0.0105  1.1653  ±6.86%   120748
```

**Result:** Volar is **3.83x faster** than Verter

### single table.vue [####################] 10/10 (100%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               1,301.32  0.6750  2.6617  0.7684  0.7114  2.4107  2.5583  2.6617  ±3.05%      651
   · verter AcornLoose    1,793.85  0.5136  1.1535  0.5575  0.5552  0.9235  0.9838  1.1535  ±0.85%      897
   · Volar              238,312.38  0.0014  1.9141  0.0042  0.0020  0.0053  0.0113  1.1634  ±6.93%   119473
```

**Result:** Volar is **183.13x faster** than Verter


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
> - **Verter is in heavy development** and currently lacks many features required for real-world usage (template completions, Vue directives, HTML tag completions, etc.)
> - These results primarily demonstrate TypeScript completion performance within Vue files
> - Production performance will vary based on project size, configuration, and usage patterns
>
> Use these benchmarks as **relative indicators** rather than absolute performance guarantees.

| Benchmark | Verter | Volar | Performance |
|-----------|--------|-------|-------------|
| single ContactInformation.vue | 6,180.04 ops/sec | 233,443.4 ops/sec | ⚠️ 37.77x slower |
| single avatar.vue | 4,751.07 ops/sec | 233,525.25 ops/sec | ⚠️ 49.15x slower |
| single button.vue | 7,175.98 ops/sec | 234,194.31 ops/sec | ⚠️ 32.64x slower |
| single card.vue | 36,580.17 ops/sec | 236,496 ops/sec | ⚠️ 6.47x slower |
| single dynamicInput.vue | 15,650 ops/sec | 239,727.82 ops/sec | ⚠️ 15.32x slower |
| single icon.vue | 7,711.5 ops/sec | 239,703.37 ops/sec | ⚠️ 31.08x slower |
| single index.story.vue | 1,428.94 ops/sec | 238,407.02 ops/sec | ⚠️ 166.84x slower |
| single medium.vue | 63,312.39 ops/sec | 237,767.75 ops/sec | ⚠️ 3.76x slower |
| single small.vue | 63,003.23 ops/sec | 241,495.71 ops/sec | ⚠️ 3.83x slower |
| single table.vue | 1,301.32 ops/sec | 238,312.38 ops/sec | ⚠️ 183.13x slower |
