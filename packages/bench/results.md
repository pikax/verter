# Verter Benchmark Results

**Generated:** 2025-11-22 12:57 UTC

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

### single ContactInformation.vue [#-------------------] 1/39 (3%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               5,063.21  0.1429  1.2507  0.1975  0.2072  0.6888  0.7697  0.9332  ±1.70%     2532
   · verter AcornLoose    5,981.90  0.1266  0.6661  0.1672  0.1698  0.4113  0.4670  0.6275  ±1.26%     2991
   · Volar              174,756.34  0.0015  3.1699  0.0057  0.0024  0.0279  0.0329  1.7023  ±8.54%    87539
```

**Result:** Volar is **34.51x faster** than Verter

### single avatar.vue [#-------------------] 2/39 (5%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               4,145.65  0.1829  5.7642  0.2412  0.2209  2.0057  2.2812  2.8313  ±4.43%     2073
   · verter AcornLoose    6,699.91  0.1127  0.9624  0.1493  0.1508  0.3381  0.5415  0.7271  ±1.35%     3352
   · Volar              185,766.49  0.0014  3.1899  0.0054  0.0024  0.0072  0.0114  1.6487  ±8.60%    93154
```

**Result:** Volar is **44.81x faster** than Verter

### single button.vue [##------------------] 3/39 (8%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               6,342.71  0.1203  2.2958  0.1577  0.1458  0.4174  1.4815  2.0040  ±3.01%     3172
   · verter AcornLoose    9,875.92  0.0837  0.5986  0.1013  0.0974  0.2010  0.4286  0.5273  ±1.04%     4938
   · Volar              182,647.38  0.0015  3.0541  0.0055  0.0025  0.0075  0.0141  1.6254  ±8.57%    91324
```

**Result:** Volar is **28.80x faster** than Verter

### single card.vue [##------------------] 4/39 (10%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter              33,272.66  0.0241  1.4280  0.0301  0.0266  0.0674  0.0764  0.9953  ±2.20%    16637
   · verter AcornLoose   43,160.71  0.0184  8.5929  0.0232  0.0203  0.0561  0.0763  0.3465  ±3.50%    21581
   · Volar              190,040.58  0.0014  3.0419  0.0053  0.0023  0.0069  0.0156  1.6178  ±8.49%    95322
```

**Result:** Volar is **5.71x faster** than Verter

### single dynamicInput.vue [###-----------------] 5/39 (13%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter              14,287.65  0.0540  2.0072  0.0700  0.0635  0.1343  0.1728  1.4250  ±2.72%     7144
   · verter AcornLoose   20,105.31  0.0394  0.9772  0.0497  0.0436  0.1271  0.4907  0.6621  ±1.79%    10053
   · Volar              188,961.77  0.0015  2.5535  0.0053  0.0023  0.0071  0.0114  1.6574  ±8.58%    94481
```

**Result:** Volar is **13.23x faster** than Verter

### single icon.vue [###-----------------] 6/39 (15%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               6,688.45  0.1120  3.1283  0.1495  0.1356  0.3199  1.9401  2.3436  ±3.76%     3348
   · verter AcornLoose   12,964.96  0.0636  0.9221  0.0771  0.0717  0.1567  0.4401  0.5735  ±1.23%     6483
   · Volar              190,465.81  0.0015  2.3783  0.0053  0.0023  0.0068  0.0112  1.6416  ±8.49%    95233
```

**Result:** Volar is **28.48x faster** than Verter

### single index.story.vue [####----------------] 7/39 (18%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               1,282.42  0.6194  3.3665  0.7798  0.7017  2.7180  2.7649  3.3665  ±4.12%      642
   · verter AcornLoose    2,378.30  0.3643  1.1200  0.4205  0.4164  0.8301  0.9736  1.0799  ±1.28%     1190
   · Volar              187,057.40  0.0014  2.7883  0.0053  0.0024  0.0070  0.0106  1.6608  ±8.55%    93529
```

**Result:** Volar is **145.86x faster** than Verter

### single medium.vue [####----------------] 8/39 (21%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter              56,983.54  0.0139  3.0332  0.0175  0.0155  0.0437  0.0523  0.6375  ±2.08%    28492
   · verter AcornLoose   75,215.83  0.0112  0.6549  0.0133  0.0124  0.0274  0.0356  0.3742  ±1.21%    37608
   · Volar              195,245.49  0.0014  2.4529  0.0051  0.0023  0.0067  0.0115  1.5941  ±8.37%    97623
```

**Result:** Volar is **3.43x faster** than Verter

### single small.vue [#####---------------] 9/39 (23%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter              59,856.84  0.0134  1.5906  0.0167  0.0150  0.0382  0.0459  0.7897  ±2.12%    29929
   · verter AcornLoose   77,060.38  0.0108  0.5754  0.0130  0.0119  0.0320  0.0398  0.3619  ±1.13%    38531
   · Volar              191,158.23  0.0014  3.1477  0.0052  0.0023  0.0070  0.0123  1.5762  ±8.50%    95640
```

**Result:** Volar is **3.19x faster** than Verter

### single table.vue [#####---------------] 10/39 (26%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               1,158.39  0.6810  3.8357  0.8633  0.7833  2.9843  3.0565  3.8357  ±4.02%      580
   · verter AcornLoose    1,595.37  0.5076  1.4900  0.6268  0.6129  1.1005  1.1443  1.4900  ±1.75%      798
   · Volar              186,712.13  0.0014  2.3039  0.0054  0.0024  0.0080  0.0388  1.6112  ±8.34%    93611
```

**Result:** Volar is **161.18x faster** than Verter

## completions.bench.ts

**Description:** Tests Vue.js template completion performance in a simple component. Both use LSP+IPC architecture.

Suites: 6

### Script setup completions [######--------------] 11/39 (28%)

```
     name                                     hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - Script setup completions  1,198.59  0.6243  2.7222  0.8343  0.8955  1.5309  1.7398  2.7222  ±2.01%      600
   · Volar - Script setup completions     204.19  4.0432  8.1030  4.8975  5.1882  7.8758  8.1030  8.1030  ±3.17%      103
```

**Result:** Verter is **5.87x faster** than Volar

### TypeScript completions in script [######--------------] 12/39 (31%)

```
     name                                   hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - TypeScript completions  1,958.56  0.4145  1.5563  0.5106  0.5362  1.0065  1.0655  1.5563  ±1.44%      980
   · Volar - TypeScript completions     659.85  1.2016  3.0143  1.5155  1.6505  2.5674  2.6392  3.0143  ±1.85%      331
```

**Result:** Verter is **2.97x faster** than Volar

### Auto import component [#######-------------] 13/39 (33%)

```
     name                                hz     min      max    mean     p75      p99     p995     p999     rme  samples
   · Verter - Auto import component  185.55  4.8228   9.2712  5.3892  5.5153   9.2712   9.2712   9.2712  ±2.62%       93
   · Volar - Auto import component   144.42  5.8077  10.2490  6.9241  7.3275  10.2490  10.2490  10.2490  ±3.80%       73
```

**Result:** Verter is **1.28x faster** than Volar

### template completion [#######-------------] 14/39 (36%)

```
     name          hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter  1,541.99  0.5450  2.1775  0.6485  0.6685  1.1607  1.3473  2.1775  ±1.46%      771
   · Volar     194.76  4.2226  8.6945  5.1346  5.4882  8.6945  8.6945  8.6945  ±3.27%       98
```

**Result:** Verter is **7.92x faster** than Volar

### Complex TypeScript inference [########------------] 15/39 (38%)

```
     name                                         hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - Complex TypeScript inference  6,491.65  0.1112  1.3186  0.1540  0.1516  0.3196  0.3442  0.9889  ±1.12%     3246
   · Volar - Complex TypeScript inference     747.88  1.1136  2.7372  1.3371  1.4246  2.4637  2.6922  2.7372  ±1.80%      374
```

**Result:** Verter is **8.68x faster** than Volar

### Multiple file operations [########------------] 16/39 (41%)

```
     name                                     hz      min      max     mean      p75      p99     p995     p999     rme  samples
   · Verter - Open and complete 5 files   402.68   2.1206   3.1642   2.4834   2.7234   3.1563   3.1632   3.1642  ±1.60%      202
   · Volar - Open and complete 5 files   20.3463  42.1840  57.6910  49.1491  54.7212  57.6910  57.6910  57.6910  ±7.60%       11
```

**Result:** Verter is **19.79x faster** than Volar

## parser_process.bench.ts

**Description:** Benchmark comparison between Volar and Verter.

Suites: 8

### single ContactInformation.vue [#########-----------] 17/39 (44%)

```
     name                     hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter             1,532.89  0.5685  1.3910  0.6524  0.6537  1.1323  1.2258  1.3910  ±1.18%      767
   · verter AcornLoose  1,659.70  0.5332  1.2393  0.6025  0.5981  1.0331  1.1872  1.2393  ±1.08%      830
   · Volar                210.94  3.3264  9.5898  4.7407  5.3993  9.0729  9.5898  9.5898  ±5.38%      106
```

**Result:** Verter is **7.27x faster** than Volar

### single avatar.vue [#########-----------] 18/39 (46%)

```
     name                     hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter             1,782.42  0.4292  2.4925  0.5610  0.5954  1.7311  1.7850  2.4925  ±2.37%      892
   · verter AcornLoose  1,895.07  0.3398  1.3220  0.5277  0.6122  1.0700  1.1921  1.3220  ±1.70%      948
   · Volar                621.22  0.9372  6.5941  1.6097  1.6159  4.8936  5.4906  6.5941  ±5.51%      311
```

**Result:** Verter is **2.87x faster** than Volar

### single button.vue [##########----------] 19/39 (49%)

```
     name                     hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter             1,985.29  0.3429  2.2198  0.5037  0.5766  1.5793  1.7152  2.2198  ±2.31%      993
   · verter AcornLoose  2,337.13  0.2927  1.2921  0.4279  0.4874  1.0060  1.1581  1.2260  ±1.66%     1169
   · Volar                607.96  1.0508  5.0228  1.6449  1.7057  4.0702  4.9164  5.0228  ±4.54%      304
```

**Result:** Verter is **3.27x faster** than Volar

### single card.vue [##########----------] 20/39 (51%)

```
     name                     hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter             8,341.38  0.0857  1.3851  0.1199  0.1352  0.2224  0.7360  0.9992  ±1.74%     4171
   · verter AcornLoose  9,468.08  0.0729  1.0003  0.1056  0.1247  0.2125  0.6368  0.8125  ±1.57%     4735
   · Volar              2,287.77  0.2693  4.8498  0.4371  0.4404  2.6389  3.0333  3.9456  ±4.81%     1144
```

**Result:** Verter is **3.65x faster** than Volar

### single icon.vue [###########---------] 21/39 (54%)

```
     name                     hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter             3,068.99  0.2223  2.0022  0.3258  0.3732  1.6161  1.7371  1.9429  ±2.67%     1535
   · verter AcornLoose  4,985.76  0.1531  0.9176  0.2006  0.2142  0.5557  0.7228  0.8708  ±1.43%     2493
   · Volar              1,981.09  0.3430  4.3143  0.5048  0.5255  3.2314  3.3396  4.3143  ±5.11%      991
```

**Result:** Verter is **1.55x faster** than Volar

### single index.story.vue [###########---------] 22/39 (56%)

```
     name                   hz     min      max    mean     p75     p99     p995     p999     rme  samples
   · Verter             534.65  1.3924   3.9719  1.8704  1.9709  3.8502   3.9646   3.9719  ±3.10%      268
   · verter AcornLoose  725.92  1.1297   6.3863  1.3776  1.4238  2.4682   3.2305   6.3863  ±2.96%      363
   · Volar              241.30  2.9552  12.1988  4.1442  4.8342  6.5613  12.1988  12.1988  ±5.23%      121
```

**Result:** Verter is **2.22x faster** than Volar

### single medium.vue [############--------] 23/39 (59%)

```
     name                      hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter             12,454.71  0.0526  0.9459  0.0803  0.0925  0.1622  0.5849  0.7934  ±1.61%     6228
   · verter AcornLoose  17,937.94  0.0441  0.7305  0.0557  0.0519  0.1275  0.4325  0.6439  ±1.43%     8969
   · Volar               4,516.63  0.1334  7.0182  0.2214  0.2129  2.5215  2.7921  4.8105  ±6.42%     2280
```

**Result:** Verter is **2.76x faster** than Volar

### single small.vue [############--------] 24/39 (62%)

```
     name                      hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter             16,052.66  0.0520  1.2270  0.0623  0.0600  0.1046  0.5023  0.7196  ±1.63%     8027
   · verter AcornLoose  19,981.20  0.0427  0.8376  0.0500  0.0473  0.0895  0.3837  0.6682  ±1.48%     9991
   · Volar               5,622.41  0.1335  4.5283  0.1779  0.1492  1.9664  2.2945  2.9689  ±5.21%     2812
```

**Result:** Verter is **2.86x faster** than Volar

## process.bench.ts

**Description:** Benchmark comparison between Volar and Verter.

Suites: 8

### single ContactInformation.vue [#############-------] 25/39 (64%)

```
     name              hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter      9,223.45  0.0808  1.7874  0.1084  0.1015  0.3307  1.1532  1.3712  ±2.79%     4612
   · Volar   2,286,744.63  0.0003  1.6929  0.0004  0.0005  0.0006  0.0007  0.0012  ±0.78%  1143373
```

**Result:** Volar is **247.93x faster** than Verter

### single avatar.vue [#############-------] 26/39 (67%)

```
     name              hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter     16,994.77  0.0465  2.2263  0.0588  0.0523  0.0978  1.1236  1.4294  ±3.44%     8498
   · Volar   1,949,072.83  0.0004  0.5018  0.0005  0.0005  0.0007  0.0008  0.0011  ±0.51%   974537
```

**Result:** Volar is **114.69x faster** than Verter

### single button.vue [##############------] 27/39 (69%)

```
     name              hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter     17,122.23  0.0461  1.9800  0.0584  0.0501  0.0916  1.3271  1.7282  ±4.06%     8562
   · Volar   2,268,272.00  0.0003  0.6242  0.0004  0.0005  0.0006  0.0007  0.0010  ±0.73%  1134136
```

**Result:** Volar is **132.48x faster** than Verter

### single card.vue [##############------] 28/39 (72%)

```
     name              hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter     24,435.51  0.0299  2.1853  0.0409  0.0329  0.0740  0.2376  1.8252  ±4.90%    12255
   · Volar   2,268,671.73  0.0003  0.5402  0.0004  0.0005  0.0006  0.0007  0.0010  ±0.84%  1134337
```

**Result:** Volar is **92.84x faster** than Verter

### single icon.vue [###############-----] 29/39 (74%)

```
     name              hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter     19,977.44  0.0365  2.8921  0.0501  0.0404  0.0868  0.2342  2.1095  ±5.35%     9989
   · Volar   1,930,640.46  0.0004  0.6567  0.0005  0.0005  0.0006  0.0008  0.0012  ±0.87%   965321
```

**Result:** Volar is **96.64x faster** than Verter

### single index.story.vue [###############-----] 30/39 (77%)

```
     name              hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter      7,561.02  0.1105  1.6469  0.1323  0.1188  1.1644  1.2657  1.4430  ±3.01%     3781
   · Volar   2,239,779.10  0.0003  1.0139  0.0004  0.0005  0.0006  0.0008  0.0013  ±0.99%  1119890
```

**Result:** Volar is **296.23x faster** than Verter

### single medium.vue [################----] 31/39 (79%)

```
     name              hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter     28,588.23  0.0254  2.1449  0.0350  0.0281  0.0533  0.1019  1.9353  ±5.16%    14340
   · Volar   2,252,759.10  0.0003  0.7285  0.0004  0.0005  0.0006  0.0008  0.0011  ±1.00%  1126380
```

**Result:** Volar is **78.80x faster** than Verter

### single small.vue [################----] 32/39 (82%)

```
     name              hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter     27,947.55  0.0245  3.1106  0.0358  0.0273  0.0464  0.0728  2.3532  ±6.36%    13974
   · Volar   2,228,028.00  0.0003  1.0775  0.0004  0.0005  0.0007  0.0008  0.0011  ±1.39%  1114014
```

**Result:** Volar is **79.72x faster** than Verter

## real-world-components.bench.ts

**Description:** Measures completion performance in real-world Vue components with multiple edits and completion requests, simulating actual development workflows. Both use LSP+IPC architecture.

Suites: 7

### button.vue - Props in computed (fresh document) [#################---] 33/39 (85%)

```
     name                                                 hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - button.vue props completions (fresh)  1,240.73  0.6463  3.0460  0.8060  0.8299  1.5744  1.6686  3.0460  ±1.89%      621
   · Volar - button.vue props completions (fresh)     643.97  1.2912  5.4055  1.5529  1.5871  2.4971  3.7982  5.4055  ±2.34%      322
```

**Result:** Verter is **1.93x faster** than Volar

### button.vue - Action object properties (fresh) [#################---] 34/39 (87%)

```
     name                                                 hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - button.vue action properties (fresh)  1,490.82  0.5749  1.6824  0.6708  0.6669  1.4724  1.5060  1.6824  ±1.56%      746
   · Volar - button.vue action properties (fresh)     953.64  0.8896  1.8653  1.0486  1.0836  1.7314  1.7759  1.8653  ±1.00%      477
```

**Result:** Verter is **1.56x faster** than Volar

### avatar.vue - Props in computed (fresh) [##################--] 35/39 (90%)

```
     name                                                 hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - avatar.vue props completions (fresh)  1,252.73  0.6490  3.6038  0.7983  0.7817  1.7210  1.9722  3.6038  ±2.20%      627
   · Volar - avatar.vue props completions (fresh)     786.60  1.0827  2.0164  1.2713  1.3078  1.9149  1.9785  2.0164  ±1.06%      394
```

**Result:** Verter is **1.59x faster** than Volar

### avatar.vue - Ref properties (fresh) [##################--] 36/39 (92%)

```
     name                                            hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - avatar.vue ref properties (fresh)  942.17  0.8936  6.4376  1.0614  1.0209  2.0519  2.1243  6.4376  ±2.84%      472
   · Volar - avatar.vue ref properties (fresh)   628.71  1.4055  2.2296  1.5906  1.6231  2.1059  2.2111  2.2296  ±0.90%      315
```

**Result:** Verter is **1.50x faster** than Volar

### avatar.vue - HTMLElement properties (fresh) [###################-] 37/39 (95%)

```
     name                                                    hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - avatar.vue HTMLElement properties (fresh)  530.40  1.6975  5.3581  1.8854  1.8704  3.6038  3.9315  5.3581  ±2.20%      266
   · Volar - avatar.vue HTMLElement properties (fresh)   456.47  1.8851  3.4238  2.1907  2.2064  3.3618  3.4205  3.4238  ±1.86%      229
```

**Result:** Verter is **1.16x faster** than Volar

### icon.vue - Ref element (fresh) [###################-] 38/39 (97%)

```
     name                                         hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - icon.vue ref element (fresh)  1,211.02  0.7138  1.6885  0.8258  0.8301  1.5503  1.5963  1.6885  ±1.41%      606
   · Volar - icon.vue ref element (fresh)     919.65  0.9373  2.0301  1.0874  1.1343  1.6915  1.8195  2.0301  ±1.10%      460
```

**Result:** Verter is **1.32x faster** than Volar

### Real world editing workflow [####################] 39/39 (100%)

```
     name                                     hz      min      max     mean      p75      p99     p995     p999     rme  samples
   · Verter - complete editing workflow   497.83   1.7532   3.7401   2.0087   1.9752   3.0059   3.1096   3.7401  ±1.76%      249
   · Volar - complete editing workflow   39.8354  22.7838  28.2660  25.1033  25.8880  28.2660  28.2660  28.2660  ±3.21%       20
```

**Result:** Verter is **12.50x faster** than Volar


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

| File | Benchmark | Verter | Volar | Performance |
|------|-----------|--------|-------|-------------|
| parser.bench.ts | single ContactInformation.vue | 5,063.21 ops/sec | 174,756.34 ops/sec | ⚠️ 34.51x slower |
| parser.bench.ts | single avatar.vue | 4,145.65 ops/sec | 185,766.49 ops/sec | ⚠️ 44.81x slower |
| parser.bench.ts | single button.vue | 6,342.71 ops/sec | 182,647.38 ops/sec | ⚠️ 28.80x slower |
| parser.bench.ts | single card.vue | 33,272.66 ops/sec | 190,040.58 ops/sec | ⚠️ 5.71x slower |
| parser.bench.ts | single dynamicInput.vue | 14,287.65 ops/sec | 188,961.77 ops/sec | ⚠️ 13.23x slower |
| parser.bench.ts | single icon.vue | 6,688.45 ops/sec | 190,465.81 ops/sec | ⚠️ 28.48x slower |
| parser.bench.ts | single index.story.vue | 1,282.42 ops/sec | 187,057.4 ops/sec | ⚠️ 145.86x slower |
| parser.bench.ts | single medium.vue | 56,983.54 ops/sec | 195,245.49 ops/sec | ⚠️ 3.43x slower |
| parser.bench.ts | single small.vue | 59,856.84 ops/sec | 191,158.23 ops/sec | ⚠️ 3.19x slower |
| parser.bench.ts | single table.vue | 1,158.39 ops/sec | 186,712.13 ops/sec | ⚠️ 161.18x slower |
| completions.bench.ts | Script setup completions | 1,198.59 ops/sec | 204.19 ops/sec | ✅ 5.87x faster |
| completions.bench.ts | TypeScript completions in script | 1,958.56 ops/sec | 659.85 ops/sec | ✅ 2.97x faster |
| completions.bench.ts | Auto import component | 185.55 ops/sec | 144.42 ops/sec | ✅ 1.28x faster |
| completions.bench.ts | template completion | 1,541.99 ops/sec | 194.76 ops/sec | ✅ 7.92x faster |
| completions.bench.ts | Complex TypeScript inference | 6,491.65 ops/sec | 747.88 ops/sec | ✅ 8.68x faster |
| completions.bench.ts | Multiple file operations | 402.68 ops/sec | 20.35 ops/sec | ✅ 19.79x faster |
| parser_process.bench.ts | single ContactInformation.vue | 1,532.89 ops/sec | 210.94 ops/sec | ✅ 7.27x faster |
| parser_process.bench.ts | single avatar.vue | 1,782.42 ops/sec | 621.22 ops/sec | ✅ 2.87x faster |
| parser_process.bench.ts | single button.vue | 1,985.29 ops/sec | 607.96 ops/sec | ✅ 3.27x faster |
| parser_process.bench.ts | single card.vue | 8,341.38 ops/sec | 2,287.77 ops/sec | ✅ 3.65x faster |
| parser_process.bench.ts | single icon.vue | 3,068.99 ops/sec | 1,981.09 ops/sec | ✅ 1.55x faster |
| parser_process.bench.ts | single index.story.vue | 534.65 ops/sec | 241.3 ops/sec | ✅ 2.22x faster |
| parser_process.bench.ts | single medium.vue | 12,454.71 ops/sec | 4,516.63 ops/sec | ✅ 2.76x faster |
| parser_process.bench.ts | single small.vue | 16,052.66 ops/sec | 5,622.41 ops/sec | ✅ 2.86x faster |
| process.bench.ts | single ContactInformation.vue | 9,223.45 ops/sec | 2,286,744.63 ops/sec | ⚠️ 247.93x slower |
| process.bench.ts | single avatar.vue | 16,994.77 ops/sec | 1,949,072.83 ops/sec | ⚠️ 114.69x slower |
| process.bench.ts | single button.vue | 17,122.23 ops/sec | 2,268,272 ops/sec | ⚠️ 132.48x slower |
| process.bench.ts | single card.vue | 24,435.51 ops/sec | 2,268,671.73 ops/sec | ⚠️ 92.84x slower |
| process.bench.ts | single icon.vue | 19,977.44 ops/sec | 1,930,640.46 ops/sec | ⚠️ 96.64x slower |
| process.bench.ts | single index.story.vue | 7,561.02 ops/sec | 2,239,779.1 ops/sec | ⚠️ 296.23x slower |
| process.bench.ts | single medium.vue | 28,588.23 ops/sec | 2,252,759.1 ops/sec | ⚠️ 78.80x slower |
| process.bench.ts | single small.vue | 27,947.55 ops/sec | 2,228,028 ops/sec | ⚠️ 79.72x slower |
| real-world-components.bench.ts | button.vue - Props in computed (fresh document) | 1,240.73 ops/sec | 643.97 ops/sec | ✅ 1.93x faster |
| real-world-components.bench.ts | button.vue - Action object properties (fresh) | 1,490.82 ops/sec | 953.64 ops/sec | ✅ 1.56x faster |
| real-world-components.bench.ts | avatar.vue - Props in computed (fresh) | 1,252.73 ops/sec | 786.6 ops/sec | ✅ 1.59x faster |
| real-world-components.bench.ts | avatar.vue - Ref properties (fresh) | 942.17 ops/sec | 628.71 ops/sec | ✅ 1.50x faster |
| real-world-components.bench.ts | avatar.vue - HTMLElement properties (fresh) | 530.4 ops/sec | 456.47 ops/sec | ✅ 1.16x faster |
| real-world-components.bench.ts | icon.vue - Ref element (fresh) | 1,211.02 ops/sec | 919.65 ops/sec | ✅ 1.32x faster |
| real-world-components.bench.ts | Real world editing workflow | 497.83 ops/sec | 39.84 ops/sec | ✅ 12.50x faster |
