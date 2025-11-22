# Verter Benchmark Results

**Generated:** 2025-11-22 12:49 UTC

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

## completions.bench.ts

**Description:** Tests Vue.js template completion performance in a simple component. Both use LSP+IPC architecture.

Suites: 6

### Script setup completions [#-------------------] 1/39 (3%)

```
     name                                     hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - Script setup completions  1,241.63  0.6192  2.4483  0.8054  0.8745  1.4435  1.5863  2.4483  ±1.79%      621
   · Volar - Script setup completions     186.41  4.1456  9.9254  5.3646  6.1679  9.9254  9.9254  9.9254  ±4.17%       94
```

**Result:** Verter is **6.66x faster** than Volar

### TypeScript completions in script [#-------------------] 2/39 (5%)

```
     name                                   hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - TypeScript completions  1,960.12  0.4186  1.2582  0.5102  0.5294  0.9442  1.1529  1.2582  ±1.37%      981
   · Volar - TypeScript completions     653.89  1.2252  2.6836  1.5293  1.6646  2.4193  2.5201  2.6836  ±1.77%      327
```

**Result:** Verter is **3.00x faster** than Volar

### Auto import component [##------------------] 3/39 (8%)

```
     name                                hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - Auto import component  183.08  4.7809  8.4588  5.4619  5.5914  8.4588  8.4588  8.4588  ±2.64%       92
   · Volar - Auto import component   147.25  5.7862  9.5555  6.7911  7.2928  9.5555  9.5555  9.5555  ±2.96%       74
```

**Result:** Verter is **1.24x faster** than Volar

### template completion [##------------------] 4/39 (10%)

```
     name          hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter  1,557.52  0.5504  1.5359  0.6420  0.6581  1.1788  1.2825  1.5359  ±1.37%      779
   · Volar     195.65  4.1991  7.3186  5.1111  5.5146  7.3186  7.3186  7.3186  ±3.18%       98
```

**Result:** Verter is **7.96x faster** than Volar

### Complex TypeScript inference [###-----------------] 5/39 (13%)

```
     name                                         hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - Complex TypeScript inference  6,395.98  0.1096  1.1256  0.1563  0.1507  0.3100  0.3326  0.9636  ±1.09%     3198
   · Volar - Complex TypeScript inference     726.48  1.1145  2.8376  1.3765  1.4873  2.5454  2.7881  2.8376  ±1.87%      364
```

**Result:** Verter is **8.80x faster** than Volar

### Multiple file operations [###-----------------] 6/39 (15%)

```
     name                                     hz      min      max     mean      p75      p99     p995     p999     rme  samples
   · Verter - Open and complete 5 files   397.19   2.1384   3.6806   2.5177   2.7553   3.6290   3.6806   3.6806  ±1.88%      199
   · Volar - Open and complete 5 files   21.3769  42.4417  50.0815  46.7796  49.0735  50.0815  50.0815  50.0815  ±3.24%       11
```

**Result:** Verter is **18.58x faster** than Volar

## real-world-components.bench.ts

**Description:** Measures completion performance in real-world Vue components with multiple edits and completion requests, simulating actual development workflows. Both use LSP+IPC architecture.

Suites: 7

### button.vue - Props in computed (fresh document) [####----------------] 7/39 (18%)

```
     name                                                 hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - button.vue props completions (fresh)  1,005.05  0.7166  3.5561  0.9950  1.1041  2.0765  2.3388  3.5561  ±2.45%      503
   · Volar - button.vue props completions (fresh)     563.25  1.4033  4.9715  1.7754  1.8858  2.9676  3.2672  4.9715  ±2.30%      282
```

**Result:** Verter is **1.78x faster** than Volar

### button.vue - Action object properties (fresh) [####----------------] 8/39 (21%)

```
     name                                                 hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - button.vue action properties (fresh)  1,361.35  0.6117  1.8350  0.7346  0.7301  1.6598  1.7872  1.8350  ±1.87%      681
   · Volar - button.vue action properties (fresh)     847.52  0.9767  2.0692  1.1799  1.2446  1.8072  1.8275  2.0692  ±1.39%      424
```

**Result:** Verter is **1.61x faster** than Volar

### avatar.vue - Props in computed (fresh) [#####---------------] 9/39 (23%)

```
     name                                                 hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - avatar.vue props completions (fresh)  1,129.29  0.7027  2.5764  0.8855  0.9415  2.0608  2.2733  2.5764  ±2.34%      565
   · Volar - avatar.vue props completions (fresh)     699.35  1.1984  2.3022  1.4299  1.5347  1.9939  2.0373  2.3022  ±1.41%      350
```

**Result:** Verter is **1.61x faster** than Volar

### avatar.vue - Ref properties (fresh) [#####---------------] 10/39 (26%)

```
     name                                            hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - avatar.vue ref properties (fresh)  811.31  0.9421  8.3224  1.2326  1.3594  2.3720  2.8888  8.3224  ±3.91%      406
   · Volar - avatar.vue ref properties (fresh)   539.91  1.5538  2.7081  1.8522  1.9866  2.6360  2.6724  2.7081  ±1.78%      270
```

**Result:** Verter is **1.50x faster** than Volar

### avatar.vue - HTMLElement properties (fresh) [######--------------] 11/39 (28%)

```
     name                                                    hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - avatar.vue HTMLElement properties (fresh)  507.94  1.7356  2.9880  1.9687  2.0509  2.8644  2.9076  2.9880  ±1.55%      254
   · Volar - avatar.vue HTMLElement properties (fresh)   422.95  1.9703  3.8855  2.3643  2.5092  3.6856  3.7793  3.8855  ±2.20%      212
```

**Result:** Verter is **1.20x faster** than Volar

### icon.vue - Ref element (fresh) [######--------------] 12/39 (31%)

```
     name                                         hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - icon.vue ref element (fresh)  1,079.38  0.7563  2.4204  0.9265  0.9507  1.7896  2.0081  2.4204  ±1.93%      540
   · Volar - icon.vue ref element (fresh)     807.72  1.0376  2.6485  1.2380  1.3244  1.8113  1.8834  2.6485  ±1.52%      404
```

**Result:** Verter is **1.34x faster** than Volar

### Real world editing workflow [#######-------------] 13/39 (33%)

```
     name                                     hz      min      max     mean      p75      p99     p995     p999     rme  samples
   · Verter - complete editing workflow   459.84   1.8601   3.2695   2.1747   2.3263   3.1871   3.2069   3.2695  ±2.03%      230
   · Volar - complete editing workflow   33.4821  25.6145  38.2794  29.8667  31.7631  38.2794  38.2794  38.2794  ±6.12%       17
```

**Result:** Verter is **13.73x faster** than Volar

## process.bench.ts

**Description:** Benchmark comparison between Volar and Verter.

Suites: 8

### single ContactInformation.vue [#######-------------] 14/39 (36%)

```
     name              hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter      6,474.94  0.0802  2.5017  0.1544  0.1743  0.4619  1.2805  2.2821  ±3.01%     3238
   · Volar   2,174,690.70  0.0003  1.6310  0.0005  0.0005  0.0009  0.0009  0.0018  ±0.84%  1087346
```

**Result:** Volar is **335.86x faster** than Verter

### single avatar.vue [########------------] 15/39 (38%)

```
     name              hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter     15,139.25  0.0462  2.5568  0.0661  0.0550  0.1526  1.3184  1.5508  ±3.69%     7570
   · Volar   1,753,263.55  0.0004  0.3907  0.0006  0.0006  0.0010  0.0011  0.0019  ±0.45%   876633
```

**Result:** Volar is **115.81x faster** than Verter

### single button.vue [########------------] 16/39 (41%)

```
     name              hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter     15,802.48  0.0456  1.9702  0.0633  0.0529  0.1170  1.4932  1.7336  ±4.12%     7902
   · Volar   2,133,402.00  0.0003  0.6892  0.0005  0.0005  0.0009  0.0009  0.0013  ±0.67%  1066701
```

**Result:** Volar is **135.00x faster** than Verter

### single card.vue [#########-----------] 17/39 (44%)

```
     name              hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter     23,177.59  0.0298  2.6662  0.0431  0.0340  0.0774  0.1158  1.8400  ±4.97%    11589
   · Volar   2,197,774.24  0.0003  0.4462  0.0005  0.0005  0.0008  0.0009  0.0015  ±0.60%  1098888
```

**Result:** Volar is **94.82x faster** than Verter

### single icon.vue [#########-----------] 18/39 (46%)

```
     name              hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter     19,815.79  0.0362  2.4747  0.0505  0.0405  0.0912  0.2214  1.9342  ±4.88%     9908
   · Volar   1,882,832.90  0.0004  0.5559  0.0005  0.0005  0.0009  0.0010  0.0013  ±0.75%   941443
```

**Result:** Volar is **95.02x faster** than Verter

### single index.story.vue [##########----------] 19/39 (49%)

```
     name              hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter      6,007.93  0.1103  8.5761  0.1664  0.1877  0.9499  1.3523  1.9225  ±4.52%     3004
   · Volar   2,138,888.72  0.0003  0.4907  0.0005  0.0005  0.0008  0.0009  0.0017  ±0.72%  1069445
```

**Result:** Volar is **356.01x faster** than Verter

### single medium.vue [##########----------] 20/39 (51%)

```
     name              hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter     25,827.54  0.0252  2.2397  0.0387  0.0296  0.0836  0.1136  1.9321  ±5.01%    12914
   · Volar   2,198,137.56  0.0003  0.5261  0.0005  0.0005  0.0008  0.0009  0.0011  ±0.71%  1099069
```

**Result:** Volar is **85.11x faster** than Verter

### single small.vue [###########---------] 21/39 (54%)

```
     name              hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter     27,691.78  0.0245  2.4402  0.0361  0.0272  0.0638  0.0905  2.1427  ±5.90%    13846
   · Volar   2,204,093.56  0.0003  0.6568  0.0005  0.0005  0.0008  0.0009  0.0015  ±0.88%  1102047
```

**Result:** Volar is **79.59x faster** than Verter

## parser_process.bench.ts

**Description:** Benchmark comparison between Volar and Verter.

Suites: 8

### single ContactInformation.vue [###########---------] 22/39 (56%)

```
     name                     hz     min      max    mean     p75      p99     p995     p999     rme  samples
   · Verter             1,265.29  0.5722   1.6003  0.7903  0.9891   1.3721   1.4540   1.6003  ±2.02%      633
   · verter AcornLoose  1,454.51  0.5441   1.8098  0.6875  0.7361   1.1225   1.2715   1.8098  ±1.72%      728
   · Volar                170.81  3.3029  12.9681  5.8544  6.7438  12.9681  12.9681  12.9681  ±6.90%       86
```

**Result:** Verter is **7.41x faster** than Volar

### single avatar.vue [############--------] 23/39 (59%)

```
     name                     hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter             1,729.40  0.4111  3.4541  0.5782  0.7065  1.7660  1.9383  3.4541  ±2.77%      865
   · verter AcornLoose  2,229.42  0.3274  1.1034  0.4485  0.5368  0.8373  0.9002  1.0344  ±1.63%     1115
   · Volar                705.01  0.8973  7.2754  1.4184  1.4932  4.4620  5.6119  7.2754  ±5.52%      353
```

**Result:** Verter is **2.45x faster** than Volar

### single button.vue [############--------] 24/39 (62%)

```
     name                     hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter             2,231.53  0.3404  1.8340  0.4481  0.4499  1.3928  1.5383  1.7711  ±2.29%     1116
   · verter AcornLoose  2,844.47  0.2850  1.7861  0.3516  0.3492  0.8563  0.9452  1.1660  ±1.60%     1423
   · Volar                669.74  1.0270  5.5204  1.4931  1.6462  4.3163  4.9539  5.5204  ±4.83%      335
```

**Result:** Verter is **3.33x faster** than Volar

### single card.vue [#############-------] 25/39 (64%)

```
     name                      hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter              8,791.05  0.0868  0.9502  0.1138  0.1131  0.2127  0.7074  0.8719  ±1.57%     4396
   · verter AcornLoose  11,333.56  0.0711  0.7409  0.0882  0.0842  0.1715  0.5206  0.6768  ±1.34%     5667
   · Volar               2,627.64  0.2653  4.2109  0.3806  0.3813  2.3598  2.5485  3.3222  ±4.66%     1314
```

**Result:** Verter is **3.35x faster** than Volar

### single icon.vue [#############-------] 26/39 (67%)

```
     name                     hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter             3,540.68  0.2151  2.2902  0.2824  0.2853  1.4215  1.6799  2.1900  ±2.67%     1771
   · verter AcornLoose  5,648.28  0.1506  0.8189  0.1770  0.1717  0.5918  0.6955  0.7785  ±1.26%     2825
   · Volar              2,067.71  0.3453  4.4204  0.4836  0.4581  2.8794  3.0453  3.6165  ±4.99%     1036
```

**Result:** Verter is **1.71x faster** than Volar

### single index.story.vue [##############------] 27/39 (69%)

```
     name                   hz     min      max    mean     p75     p99     p995     p999     rme  samples
   · Verter             582.10  1.3735   4.1903  1.7179  1.7681  3.7799   3.8433   4.1903  ±3.25%      292
   · verter AcornLoose  787.20  1.0976   3.3885  1.2703  1.2400  2.1387   2.7177   3.3885  ±1.98%      394
   · Volar              240.79  2.9477  12.4773  4.1530  4.8988  7.8411  12.4773  12.4773  ±5.73%      121
```

**Result:** Verter is **2.42x faster** than Volar

### single medium.vue [##############------] 28/39 (72%)

```
     name                      hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter             13,267.70  0.0532  1.0356  0.0754  0.0769  0.1443  0.6179  0.8613  ±1.77%     6634
   · verter AcornLoose  18,110.90  0.0441  4.4998  0.0552  0.0495  0.1568  0.4311  0.6490  ±2.33%     9056
   · Volar               4,461.28  0.1369  4.4857  0.2242  0.2124  2.7368  3.0014  3.9137  ±6.10%     2231
```

**Result:** Verter is **2.97x faster** than Volar

### single small.vue [###############-----] 29/39 (74%)

```
     name                      hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter             13,669.43  0.0519  1.1470  0.0732  0.0785  0.1542  0.6436  0.8619  ±1.83%     6835
   · verter AcornLoose  18,226.82  0.0424  0.7686  0.0549  0.0523  0.1113  0.5368  0.6770  ±1.62%     9114
   · Volar               5,083.19  0.1355  3.7295  0.1967  0.1739  2.4729  2.5909  2.9402  ±5.48%     2542
```

**Result:** Verter is **2.69x faster** than Volar

## parser.bench.ts

**Description:** Vue file parsing and processing performance comparison with three distinct benchmarks:

- **parser**: Raw parsing to AST (Verter) vs full virtual code generation (Volar)
- **process**: Processing parsed AST into usable structures (Verter) vs extracting embedded codes (Volar)
- **parser + process**: End-to-end parsing and processing combined

Note: Verter uses a two-stage approach (parse AST → process), while Volar generates virtual TypeScript code directly during parsing.

Suites: 10

### single ContactInformation.vue [###############-----] 30/39 (77%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               5,330.53  0.1429  0.9312  0.1876  0.1857  0.6475  0.7129  0.8281  ±1.53%     2666
   · verter AcornLoose    5,121.70  0.1273  0.8247  0.1952  0.2455  0.4906  0.5941  0.7082  ±1.45%     2561
   · Volar              187,671.32  0.0014  4.2532  0.0053  0.0025  0.0276  0.0317  1.3617  ±8.02%    93836
```

**Result:** Volar is **35.21x faster** than Verter

### single avatar.vue [################----] 31/39 (79%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               4,557.46  0.1820  3.1431  0.2194  0.2024  1.7269  1.9009  2.4676  ±3.50%     2280
   · verter AcornLoose    7,250.65  0.1195  0.7745  0.1379  0.1387  0.4036  0.4508  0.6682  ±1.00%     3626
   · Volar              195,073.15  0.0015  2.6293  0.0051  0.0025  0.0059  0.0118  1.4738  ±8.13%    97614
```

**Result:** Volar is **42.80x faster** than Verter

### single button.vue [################----] 32/39 (82%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               5,250.96  0.1224  2.9503  0.1904  0.2138  0.3487  1.9965  2.4762  ±3.59%     2626
   · verter AcornLoose    7,804.63  0.0889  4.0098  0.1281  0.1506  0.3750  0.5291  0.8961  ±2.18%     3903
   · Volar              156,060.22  0.0015  3.4477  0.0064  0.0032  0.0083  0.0330  1.8772  ±9.26%    78121
```

**Result:** Volar is **29.72x faster** than Verter

### single card.vue [#################---] 33/39 (85%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter              29,899.42  0.0242  1.3703  0.0334  0.0310  0.0836  0.1077  0.9108  ±2.06%    14950
   · verter AcornLoose   37,037.08  0.0188  0.8622  0.0270  0.0287  0.0685  0.0796  0.5826  ±1.46%    18519
   · Volar              197,659.84  0.0015  2.3580  0.0051  0.0025  0.0059  0.0143  1.4336  ±8.08%    98830
```

**Result:** Volar is **6.61x faster** than Verter

### single dynamicInput.vue [#################---] 34/39 (87%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter              14,661.90  0.0561  3.3015  0.0682  0.0623  0.1477  0.1965  1.6036  ±3.09%     7331
   · verter AcornLoose   22,407.78  0.0410  0.6266  0.0446  0.0435  0.0668  0.0858  0.3703  ±0.81%    11204
   · Volar              227,999.09  0.0014  1.9599  0.0044  0.0022  0.0053  0.0111  1.2268  ±7.01%   114000
```

**Result:** Volar is **15.55x faster** than Verter

### single icon.vue [##################--] 35/39 (90%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               7,415.41  0.1124  2.4716  0.1349  0.1238  0.2540  1.6385  2.3430  ±3.45%     3708
   · verter AcornLoose   13,664.67  0.0679  0.4405  0.0732  0.0714  0.1112  0.1542  0.4049  ±0.68%     6833
   · Volar              231,820.01  0.0014  2.5026  0.0043  0.0022  0.0053  0.0127  1.2085  ±6.94%   116094
```

**Result:** Volar is **31.26x faster** than Verter

### single index.story.vue [##################--] 36/39 (92%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               1,408.32  0.6124  2.6938  0.7101  0.6443  2.3060  2.4274  2.6938  ±3.42%      707
   · verter AcornLoose    2,377.74  0.3905  1.0158  0.4206  0.4122  0.7904  0.8729  0.9952  ±0.92%     1189
   · Volar              221,907.50  0.0014  3.4041  0.0045  0.0022  0.0055  0.0149  1.2365  ±7.20%   111137
```

**Result:** Volar is **157.57x faster** than Verter

### single medium.vue [###################-] 37/39 (95%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter              58,317.81  0.0142  7.0681  0.0171  0.0157  0.0256  0.0341  0.8196  ±3.49%    29159
   · verter AcornLoose   65,132.39  0.0114  0.9283  0.0154  0.0165  0.0370  0.0421  0.4339  ±1.38%    32568
   · Volar              179,298.52  0.0015  3.8586  0.0056  0.0025  0.0078  0.0289  1.6258  ±8.55%    89651
```

**Result:** Volar is **3.07x faster** than Verter

### single small.vue [###################-] 38/39 (97%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter              59,890.11  0.0134  0.8960  0.0167  0.0148  0.0449  0.0530  0.6222  ±1.70%    29946
   · verter AcornLoose   72,092.62  0.0108  0.8478  0.0139  0.0121  0.0389  0.0442  0.4049  ±1.37%    36047
   · Volar              159,275.27  0.0015  4.0088  0.0063  0.0032  0.0084  0.0182  1.8680  ±9.14%    79638
```

**Result:** Volar is **2.66x faster** than Verter

### single table.vue [####################] 39/39 (100%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               1,034.90  0.6852  3.7574  0.9663  1.0256  3.3637  3.5654  3.7574  ±4.20%      518
   · verter AcornLoose    1,488.73  0.5241  1.6936  0.6717  0.7326  1.3418  1.4735  1.6936  ±2.01%      745
   · Volar              182,825.32  0.0015  2.4918  0.0055  0.0026  0.0072  0.0132  1.6408  ±8.46%    91603
```

**Result:** Volar is **176.66x faster** than Verter


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
| Script setup completions | 1,241.63 ops/sec | 186.41 ops/sec | ✅ 6.66x faster |
| TypeScript completions in script | 1,960.12 ops/sec | 653.89 ops/sec | ✅ 3.00x faster |
| Auto import component | 183.08 ops/sec | 147.25 ops/sec | ✅ 1.24x faster |
| template completion | 1,557.52 ops/sec | 195.65 ops/sec | ✅ 7.96x faster |
| Complex TypeScript inference | 6,395.98 ops/sec | 726.48 ops/sec | ✅ 8.80x faster |
| Multiple file operations | 397.19 ops/sec | 21.38 ops/sec | ✅ 18.58x faster |
| button.vue - Props in computed (fresh document) | 1,005.05 ops/sec | 563.25 ops/sec | ✅ 1.78x faster |
| button.vue - Action object properties (fresh) | 1,361.35 ops/sec | 847.52 ops/sec | ✅ 1.61x faster |
| avatar.vue - Props in computed (fresh) | 1,129.29 ops/sec | 699.35 ops/sec | ✅ 1.61x faster |
| avatar.vue - Ref properties (fresh) | 811.31 ops/sec | 539.91 ops/sec | ✅ 1.50x faster |
| avatar.vue - HTMLElement properties (fresh) | 507.94 ops/sec | 422.95 ops/sec | ✅ 1.20x faster |
| icon.vue - Ref element (fresh) | 1,079.38 ops/sec | 807.72 ops/sec | ✅ 1.34x faster |
| Real world editing workflow | 459.84 ops/sec | 33.48 ops/sec | ✅ 13.73x faster |
| single ContactInformation.vue | 6,474.94 ops/sec | 2,174,690.7 ops/sec | ⚠️ 335.86x slower |
| single avatar.vue | 15,139.25 ops/sec | 1,753,263.55 ops/sec | ⚠️ 115.81x slower |
| single button.vue | 15,802.48 ops/sec | 2,133,402 ops/sec | ⚠️ 135.00x slower |
| single card.vue | 23,177.59 ops/sec | 2,197,774.24 ops/sec | ⚠️ 94.82x slower |
| single icon.vue | 19,815.79 ops/sec | 1,882,832.9 ops/sec | ⚠️ 95.02x slower |
| single index.story.vue | 6,007.93 ops/sec | 2,138,888.72 ops/sec | ⚠️ 356.01x slower |
| single medium.vue | 25,827.54 ops/sec | 2,198,137.56 ops/sec | ⚠️ 85.11x slower |
| single small.vue | 27,691.78 ops/sec | 2,204,093.56 ops/sec | ⚠️ 79.59x slower |
| single ContactInformation.vue | 1,265.29 ops/sec | 170.81 ops/sec | ✅ 7.41x faster |
| single avatar.vue | 1,729.4 ops/sec | 705.01 ops/sec | ✅ 2.45x faster |
| single button.vue | 2,231.53 ops/sec | 669.74 ops/sec | ✅ 3.33x faster |
| single card.vue | 8,791.05 ops/sec | 2,627.64 ops/sec | ✅ 3.35x faster |
| single icon.vue | 3,540.68 ops/sec | 2,067.71 ops/sec | ✅ 1.71x faster |
| single index.story.vue | 582.1 ops/sec | 240.79 ops/sec | ✅ 2.42x faster |
| single medium.vue | 13,267.7 ops/sec | 4,461.28 ops/sec | ✅ 2.97x faster |
| single small.vue | 13,669.43 ops/sec | 5,083.19 ops/sec | ✅ 2.69x faster |
| single ContactInformation.vue | 5,330.53 ops/sec | 187,671.32 ops/sec | ⚠️ 35.21x slower |
| single avatar.vue | 4,557.46 ops/sec | 195,073.15 ops/sec | ⚠️ 42.80x slower |
| single button.vue | 5,250.96 ops/sec | 156,060.22 ops/sec | ⚠️ 29.72x slower |
| single card.vue | 29,899.42 ops/sec | 197,659.84 ops/sec | ⚠️ 6.61x slower |
| single dynamicInput.vue | 14,661.9 ops/sec | 227,999.09 ops/sec | ⚠️ 15.55x slower |
| single icon.vue | 7,415.41 ops/sec | 231,820.01 ops/sec | ⚠️ 31.26x slower |
| single index.story.vue | 1,408.32 ops/sec | 221,907.5 ops/sec | ⚠️ 157.57x slower |
| single medium.vue | 58,317.81 ops/sec | 179,298.52 ops/sec | ⚠️ 3.07x slower |
| single small.vue | 59,890.11 ops/sec | 159,275.27 ops/sec | ⚠️ 2.66x slower |
| single table.vue | 1,034.9 ops/sec | 182,825.32 ops/sec | ⚠️ 176.66x slower |
