# Verter Benchmark Results

**Generated:** 2025-11-22 00:47 UTC

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
     name          hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  7,682.81  0.1221  0.5346  0.1302  0.1274  0.3602  0.3842  0.4413  ±0.74%     3842
   · Volar     337.82  2.2970  7.4527  2.9601  3.3183  7.0878  7.4527  7.4527  ±4.18%      169
```

**Result:** Verter is **22.74x faster** than Volar

### single avatar.vue

```
     name          hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  6,227.59  0.1454  1.9538  0.1606  0.1499  0.9769  1.1714  1.3413  ±2.36%     3114
   · Volar   1,313.45  0.5817  3.5081  0.7614  0.6901  2.8126  3.0727  3.5081  ±4.05%      657
```

**Result:** Verter is **4.74x faster** than Volar

### single button.vue

```
     name          hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  9,185.99  0.1005  1.0791  0.1089  0.1034  0.1203  0.8516  0.9448  ±1.80%     4593
   · Volar   1,229.10  0.6498  4.6321  0.8136  0.7314  2.2733  2.9665  4.6321  ±3.76%      615
```

**Result:** Verter is **7.47x faster** than Volar

### single card.vue

```
     name           hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  49,841.21  0.0182  0.5601  0.0201  0.0194  0.0226  0.0245  0.4580  ±1.19%    24921
   · Volar    5,732.40  0.1285  2.2889  0.1744  0.1464  1.4569  1.6103  2.2636  ±4.18%     2867
```

**Result:** Verter is **8.69x faster** than Volar

### single dynamicInput.vue

```
     name           hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  20,009.60  0.0453  1.1441  0.0500  0.0471  0.0604  0.0886  0.8394  ±1.82%    10005
   · Volar    2,668.77  0.2898  3.3123  0.3747  0.3400  1.6468  1.8055  2.8990  ±3.71%     1335
```

**Result:** Verter is **7.50x faster** than Volar

### single icon.vue

```
     name          hz     min      max    mean     p75     p99    p995    p999      rme  samples
   · verter  7,509.66  0.0951  26.4660  0.1332  0.1023  0.5374  1.3112  2.1674  ±11.55%     3755
   · Volar   4,111.61  0.1765   2.6841  0.2432  0.2050  1.7680  1.9786  2.5531   ±4.57%     2056
```

**Result:** Verter is **1.83x faster** than Volar

### single index.story.vue

```
     name          hz     min      max    mean     p75     p99     p995     p999     rme  samples
   · verter  1,624.87  0.5305   3.9155  0.6154  0.5549  2.0466   2.1605   3.9155  ±3.40%      814
   · Volar     308.43  2.4891  15.7331  3.2422  3.5808  8.2738  15.7331  15.7331  ±6.44%      155
```

**Result:** Verter is **5.27x faster** than Volar

### single medium.vue

```
     name           hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  80,959.61  0.0105  2.1823  0.0124  0.0115  0.0181  0.0304  0.4900  ±1.76%    40480
   · Volar   10,549.29  0.0655  4.8845  0.0948  0.0722  1.4153  1.5795  2.3233  ±5.42%     5281
```

**Result:** Verter is **7.67x faster** than Volar

### single small.vue

```
     name           hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  87,246.36  0.0102  0.8268  0.0115  0.0110  0.0136  0.0161  0.3959  ±1.33%    43624
   · Volar   10,711.94  0.0678  2.6891  0.0934  0.0732  1.3062  1.4170  1.8321  ±4.70%     5356
```

**Result:** Verter is **8.14x faster** than Volar

### single table.vue

```
     name          hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · verter  1,520.30  0.5953  2.4164  0.6578  0.6176  1.7895  2.0334  2.4164  ±2.36%      761
   · Volar     382.63  2.1697  6.2513  2.6135  2.6519  5.4412  6.2513  6.2513  ±3.21%      192
```

**Result:** Verter is **3.97x faster** than Volar

## real-world-components.bench.ts

**Description:** Measures completion performance in real-world Vue components with multiple edits and completion requests, simulating actual development workflows. Both use LSP+IPC architecture.

### button.vue - Props in computed (fresh document)

```
     name                                                 hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Volar - button.vue props completions (fresh)   1,375.73  0.6080  1.4400  0.7269  0.7516  1.3095  1.3485  1.4400  ±1.01%      688
   · Verter - button.vue props completions (fresh)  2,163.63  0.3695  3.5740  0.4622  0.4656  1.4231  1.5688  3.0808  ±2.62%     1082
```

**Result:** Verter is **1.57x faster** than Volar

### button.vue - Props in computed (cached document)

```
     name                                                  hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Volar - button.vue props completions (cached)   2,804.77  0.3270  1.2313  0.3565  0.3630  0.4633  0.8763  1.1420  ±0.90%     1403
   · Verter - button.vue props completions (cached)  5,285.43  0.1732  0.7098  0.1892  0.1877  0.4997  0.5694  0.6697  ±0.86%     2643
```

**Result:** Verter is **1.88x faster** than Volar

### button.vue - Action object properties (fresh document)

```
     name                                                 hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Volar - button.vue action properties (fresh)   3,312.80  0.2583  1.0311  0.3019  0.3091  0.6206  0.8115  0.8758  ±0.89%     1657
   · Verter - button.vue action properties (fresh)  2,987.00  0.2867  4.7064  0.3348  0.3203  1.0784  1.1475  1.8563  ±2.46%     1494
```

**Result:** Volar is **1.11x faster** than Verter

### button.vue - Action object properties (cached document)

```
     name                                                  hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Volar - button.vue action properties (cached)   7,595.02  0.1245  0.7348  0.1317  0.1317  0.1487  0.1720  0.6624  ±0.67%     3798
   · Verter - button.vue action properties (cached)  6,432.33  0.1412  0.8019  0.1555  0.1533  0.3883  0.4452  0.5128  ±0.73%     3217
```

**Result:** Volar is **1.18x faster** than Verter

### avatar.vue - Props in computed (fresh document)

```
     name                                                 hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Volar - avatar.vue props completions (fresh)   1,752.23  0.5098  1.3830  0.5707  0.5797  1.0443  1.1414  1.3830  ±0.87%      877
   · Verter - avatar.vue props completions (fresh)  2,361.04  0.3667  1.7950  0.4235  0.4141  1.2937  1.3561  1.5707  ±1.89%     1181
```

**Result:** Verter is **1.35x faster** than Volar

### avatar.vue - Props in computed (cached document)

```
     name                                                  hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Volar - avatar.vue props completions (cached)   2,571.16  0.3587  1.2369  0.3889  0.3936  0.5126  0.7542  1.1476  ±0.79%     1286
   · Verter - avatar.vue props completions (cached)  5,410.99  0.1664  0.9172  0.1848  0.1806  0.4853  0.7099  0.8512  ±1.12%     2706
```

**Result:** Verter is **2.10x faster** than Volar

### avatar.vue - Ref properties (fresh document)

```
     name                                              hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Volar - avatar.vue ref properties (fresh)   1,074.43  0.8158  3.6019  0.9307  0.9202  1.9089  2.1215  3.6019  ±1.83%      538
   · Verter - avatar.vue ref properties (fresh)  1,627.10  0.5255  3.5851  0.6146  0.5761  1.6623  1.7332  3.5851  ±2.62%      814
```

**Result:** Verter is **1.51x faster** than Volar

### avatar.vue - Ref properties (cached document)

```
     name                                               hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Volar - avatar.vue ref properties (cached)   1,300.29  0.6630  3.7037  0.7691  0.7688  1.4848  1.8271  3.7037  ±1.83%      651
   · Verter - avatar.vue ref properties (cached)  3,101.95  0.2863  1.3383  0.3224  0.3221  0.7930  0.9038  1.1641  ±1.20%     1551
```

**Result:** Verter is **2.39x faster** than Volar

### avatar.vue - HTMLElement properties (fresh document)

```
     name                                                    hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Volar - avatar.vue HTMLElement properties (fresh)   857.36  1.0465  4.4409  1.1664  1.1213  1.9600  2.5125  4.4409  ±2.14%      429
   · Verter - avatar.vue HTMLElement properties (fresh)  803.43  1.1610  1.9665  1.2447  1.2067  1.9332  1.9555  1.9665  ±1.36%      402
```

**Result:** Volar is **1.07x faster** than Verter

### avatar.vue - HTMLElement properties (cached document)

```
     name                                                     hz     min      max    mean     p75     p99    p995     p999     rme  samples
   · Volar - avatar.vue HTMLElement properties (cached)   999.10  0.8848   4.2485  1.0009  0.9302  2.1261  2.8507   4.2485  ±2.65%      500
   · Verter - avatar.vue HTMLElement properties (cached)  869.39  0.9631  17.2863  1.1502  1.0681  3.3142  4.8195  17.2863  ±7.38%      435
```

**Result:** Volar is **1.15x faster** than Verter

### icon.vue - Ref element (fresh document)

```
     name                                         hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Volar - icon.vue ref element (fresh)   2,437.31  0.3766  0.8985  0.4103  0.4154  0.7071  0.7966  0.8981  ±0.65%     1219
   · Verter - icon.vue ref element (fresh)  2,299.90  0.3876  3.3158  0.4348  0.4212  1.1416  1.3679  1.7777  ±2.02%     1150
```

**Result:** Volar is **1.06x faster** than Verter

### icon.vue - Ref element (cached document)

```
     name                                          hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Volar - icon.vue ref element (cached)   3,813.07  0.2364  0.9970  0.2623  0.2636  0.4137  0.7713  0.8660  ±0.86%     1907
   · Verter - icon.vue ref element (cached)  4,061.62  0.2267  0.6921  0.2462  0.2443  0.5277  0.6034  0.6801  ±0.74%     2031
```

**Result:** Verter is **1.07x faster** than Volar

### Real world editing workflow

```
     name                                     hz      min      max     mean      p75      p99     p995     p999     rme  samples
   · Volar - complete editing workflow   52.3657  17.0570  21.7363  19.0965  19.8300  21.7363  21.7363  21.7363  ±2.87%       27
   · Verter - complete editing workflow   994.50   0.9080   2.3814   1.0055   0.9620   2.0617   2.2601   2.3814  ±1.93%      498
```

**Result:** Verter is **18.99x faster** than Volar

## completions.bench.ts

**Description:** Tests Vue.js template completion performance in a simple component. Both use LSP+IPC architecture.

### Script setup completions

```
     name                                   hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Volar - Script setup completions   238.73  3.6851  5.8257  4.1888  4.4711  5.3220  5.8257  5.8257  ±1.85%      120
   · Verter - Script setup completions  230.85  3.8500  5.5878  4.3318  4.4794  5.2763  5.5878  5.5878  ±1.39%      116
```

**Result:** Volar is **1.03x faster** than Verter

### TypeScript completions in script

```
     name                                   hz     min      max    mean     p75     p99    p995    p999     rme  samples
   · Volar - TypeScript completions   2,061.56  0.4089   4.5452  0.4851  0.4650  1.3253  1.5073  1.8424  ±0.65%    10308
   · Verter - TypeScript completions    254.37  3.6658  10.6211  3.9314  3.9550  6.7035  8.6431  9.8764  ±0.73%     1272
```

**Result:** Volar is **8.10x faster** than Verter

### Auto import component

```
     name                                hz     min      max    mean     p75     p99    p995     p999     rme  samples
   · Volar - Auto import component   247.15  3.6076  13.9182  4.0462  4.3796  5.4781  6.3197   8.0730  ±0.83%     1236
   · Verter - Auto import component  245.96  3.6720  21.5962  4.0658  4.0918  6.7721  8.4597  10.8513  ±1.06%     1230
```

**Result:** Volar is **1.00x faster** than Verter

### template completion

```
     name          hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Volar     323.33  2.8853  4.3184  3.0928  3.1385  4.0113  4.3184  4.3184  ±1.22%      162
   · Verter  3,363.47  0.2459  2.5221  0.2973  0.2999  0.6494  0.7365  1.3589  ±1.40%     1682
```

**Result:** Verter is **10.40x faster** than Volar

### Complex TypeScript inference

```
     name                                         hz     min      max    mean     p75     p99    p995    p999     rme  samples
   · Volar - Complex TypeScript inference   1,932.91  0.4653  14.6925  0.5174  0.5072  1.2223  1.4052  1.8498  ±0.74%     9665
   · Verter - Complex TypeScript inference    243.05  3.7558  10.4870  4.1144  4.2043  6.5725  7.3274  9.6342  ±0.72%     1216
```

**Result:** Volar is **7.95x faster** than Verter


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
| single ContactInformation.vue | 7,682.81 ops/sec | 337.82 ops/sec | ✅ 22.74x faster |
| single avatar.vue | 6,227.59 ops/sec | 1,313.45 ops/sec | ✅ 4.74x faster |
| single button.vue | 9,185.99 ops/sec | 1,229.1 ops/sec | ✅ 7.47x faster |
| single card.vue | 49,841.21 ops/sec | 5,732.4 ops/sec | ✅ 8.69x faster |
| single dynamicInput.vue | 20,009.6 ops/sec | 2,668.77 ops/sec | ✅ 7.50x faster |
| single icon.vue | 7,509.66 ops/sec | 4,111.61 ops/sec | ✅ 1.83x faster |
| single index.story.vue | 1,624.87 ops/sec | 308.43 ops/sec | ✅ 5.27x faster |
| single medium.vue | 80,959.61 ops/sec | 10,549.29 ops/sec | ✅ 7.67x faster |
| single small.vue | 87,246.36 ops/sec | 10,711.94 ops/sec | ✅ 8.14x faster |
| single table.vue | 1,520.3 ops/sec | 382.63 ops/sec | ✅ 3.97x faster |
| button.vue - Props in computed (fresh document) | 2,163.63 ops/sec | 1,375.73 ops/sec | ✅ 1.57x faster |
| button.vue - Props in computed (cached document) | 5,285.43 ops/sec | 2,804.77 ops/sec | ✅ 1.88x faster |
| button.vue - Action object properties (fresh document) | 2,987 ops/sec | 3,312.8 ops/sec | ⚠️ 1.11x slower |
| button.vue - Action object properties (cached document) | 6,432.33 ops/sec | 7,595.02 ops/sec | ⚠️ 1.18x slower |
| avatar.vue - Props in computed (fresh document) | 2,361.04 ops/sec | 1,752.23 ops/sec | ✅ 1.35x faster |
| avatar.vue - Props in computed (cached document) | 5,410.99 ops/sec | 2,571.16 ops/sec | ✅ 2.10x faster |
| avatar.vue - Ref properties (fresh document) | 1,627.1 ops/sec | 1,074.43 ops/sec | ✅ 1.51x faster |
| avatar.vue - Ref properties (cached document) | 3,101.95 ops/sec | 1,300.29 ops/sec | ✅ 2.39x faster |
| avatar.vue - HTMLElement properties (fresh document) | 803.43 ops/sec | 857.36 ops/sec | ⚠️ 1.07x slower |
| avatar.vue - HTMLElement properties (cached document) | 869.39 ops/sec | 999.1 ops/sec | ⚠️ 1.15x slower |
| icon.vue - Ref element (fresh document) | 2,299.9 ops/sec | 2,437.31 ops/sec | ⚠️ 1.06x slower |
| icon.vue - Ref element (cached document) | 4,061.62 ops/sec | 3,813.07 ops/sec | ✅ 1.07x faster |
| Real world editing workflow | 994.5 ops/sec | 52.37 ops/sec | ✅ 18.99x faster |
| Script setup completions | 230.85 ops/sec | 238.73 ops/sec | ⚠️ 1.03x slower |
| TypeScript completions in script | 254.37 ops/sec | 2,061.56 ops/sec | ⚠️ 8.10x slower |
| Auto import component | 245.96 ops/sec | 247.15 ops/sec | ⚠️ 1.00x slower |
| template completion | 3,363.47 ops/sec | 323.33 ops/sec | ✅ 10.40x faster |
| Complex TypeScript inference | 243.05 ops/sec | 1,932.91 ops/sec | ⚠️ 7.95x slower |
