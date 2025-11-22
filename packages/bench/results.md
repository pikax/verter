# Verter Benchmark Results

**Generated:** 2025-11-22 15:21 UTC

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
   · Verter               5,998.71  0.1411  1.0083  0.1667  0.1634  0.5963  0.6464  0.7412  ±1.33%     3000
   · verter AcornLoose    7,143.52  0.1231  0.5509  0.1400  0.1404  0.3906  0.4469  0.5237  ±0.86%     3572
   · Volar              222,833.51  0.0015  2.4733  0.0045  0.0023  0.0057  0.0127  1.3340  ±7.34%   111417
```

**Result:** Volar is **37.15x faster** than Verter

### single avatar.vue [#-------------------] 2/39 (5%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               4,623.46  0.1808  3.5632  0.2163  0.2008  1.4742  1.8805  2.2979  ±3.43%     2312
   · verter AcornLoose    7,695.61  0.1147  0.6784  0.1299  0.1307  0.2313  0.4695  0.5389  ±0.89%     3848
   · Volar              224,300.72  0.0014  2.9505  0.0045  0.0022  0.0062  0.0120  1.2762  ±7.26%   112349
```

**Result:** Volar is **48.51x faster** than Verter

### single button.vue [##------------------] 3/39 (8%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               5,369.18  0.1244  2.6897  0.1862  0.2026  0.4406  1.8206  2.5620  ±3.44%     2685
   · verter AcornLoose    8,792.70  0.0840  3.1419  0.1137  0.1207  0.3443  0.4423  0.7272  ±1.82%     4397
   · Volar              171,653.42  0.0014  3.5934  0.0058  0.0029  0.0087  0.0147  1.6385  ±8.68%    85827
```

**Result:** Volar is **31.97x faster** than Verter

### single card.vue [##------------------] 4/39 (10%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter              28,049.53  0.0238  1.5813  0.0357  0.0372  0.1032  0.1280  0.9366  ±2.14%    14025
   · verter AcornLoose   42,976.10  0.0183  0.7048  0.0233  0.0204  0.0603  0.0692  0.4564  ±1.25%    21490
   · Volar              201,283.56  0.0014  2.2929  0.0050  0.0022  0.0063  0.0096  1.5825  ±8.19%   100642
```

**Result:** Volar is **7.18x faster** than Verter

### single dynamicInput.vue [###-----------------] 5/39 (13%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter              14,010.95  0.0547  2.5952  0.0714  0.0637  0.1544  0.2037  1.4897  ±2.83%     7006
   · verter AcornLoose   21,249.91  0.0394  0.6920  0.0471  0.0427  0.1059  0.1299  0.4595  ±1.07%    10625
   · Volar              202,039.92  0.0014  2.5961  0.0049  0.0022  0.0064  0.0096  1.5518  ±8.15%   101020
```

**Result:** Volar is **14.42x faster** than Verter

### single icon.vue [###-----------------] 6/39 (15%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               6,670.95  0.1111  2.5360  0.1499  0.1393  0.3396  1.8168  2.2929  ±3.49%     3336
   · verter AcornLoose   13,623.37  0.0638  0.5828  0.0734  0.0683  0.1599  0.2267  0.4786  ±0.96%     6812
   · Volar              187,381.63  0.0014  2.2950  0.0053  0.0029  0.0067  0.0105  1.6353  ±8.29%    93795
```

**Result:** Volar is **28.09x faster** than Verter

### single index.story.vue [####----------------] 7/39 (18%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               1,306.71  0.6179  5.4178  0.7653  0.6811  2.6766  2.8293  5.4178  ±4.30%      654
   · verter AcornLoose    2,265.36  0.3700  1.3856  0.4414  0.4253  0.8148  0.9204  1.3493  ±1.53%     1133
   · Volar              189,458.42  0.0014  3.0559  0.0053  0.0023  0.0067  0.0112  1.6143  ±8.53%    94940
```

**Result:** Volar is **144.99x faster** than Verter

### single medium.vue [####----------------] 8/39 (21%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter              56,374.49  0.0136  2.8213  0.0177  0.0155  0.0435  0.0543  0.6977  ±2.19%    28188
   · verter AcornLoose   75,957.54  0.0112  0.5892  0.0132  0.0123  0.0237  0.0322  0.3779  ±1.16%    37979
   · Volar              203,377.81  0.0014  3.9547  0.0049  0.0021  0.0060  0.0089  1.5699  ±8.31%   101726
```

**Result:** Volar is **3.61x faster** than Verter

### single small.vue [#####---------------] 9/39 (23%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter              60,993.77  0.0133  2.6968  0.0164  0.0146  0.0409  0.0547  0.5931  ±1.98%    30497
   · verter AcornLoose   79,814.56  0.0108  0.6827  0.0125  0.0117  0.0227  0.0279  0.3816  ±1.18%    39908
   · Volar              197,235.96  0.0014  2.8907  0.0051  0.0023  0.0063  0.0098  1.5810  ±8.34%    98768
```

**Result:** Volar is **3.23x faster** than Verter

### single table.vue [#####---------------] 10/39 (26%)

```
     name                       hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter               1,187.35  0.6873  3.3889  0.8422  0.7561  2.7688  3.1434  3.3889  ±3.75%      594
   · verter AcornLoose    1,601.48  0.4940  1.3788  0.6244  0.6275  1.1540  1.2583  1.3788  ±1.91%      801
   · Volar              199,055.56  0.0014  2.9334  0.0050  0.0022  0.0066  0.0102  1.5507  ±8.29%    99528
```

**Result:** Volar is **167.65x faster** than Verter

## parser_process.bench.ts

**Description:** Benchmark comparison between Volar and Verter.

Suites: 8

### single ContactInformation.vue [######--------------] 11/39 (28%)

```
     name                     hz     min      max    mean     p75      p99     p995     p999     rme  samples
   · Verter             1,267.27  0.5676   1.8492  0.7891  0.9808   1.3908   1.4850   1.8492  ±2.05%      634
   · verter AcornLoose  1,433.65  0.5332   1.5971  0.6975  0.8546   1.3098   1.3928   1.5971  ±1.97%      717
   · Volar                165.10  3.2657  17.2809  6.0568  7.3463  17.2809  17.2809  17.2809  ±8.71%       83
```

**Result:** Verter is **7.68x faster** than Volar

### single avatar.vue [######--------------] 12/39 (31%)

```
     name                     hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter             1,713.36  0.4142  3.1390  0.5836  0.7173  1.8800  2.0199  3.1390  ±2.76%      857
   · verter AcornLoose  2,289.57  0.3330  1.3223  0.4368  0.4870  0.8353  1.0173  1.1231  ±1.68%     1145
   · Volar                688.76  0.9164  4.9773  1.4519  1.5076  4.0517  4.9200  4.9773  ±4.98%      345
```

**Result:** Verter is **2.49x faster** than Volar

### single button.vue [#######-------------] 13/39 (33%)

```
     name                     hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter             2,250.93  0.3347  1.7890  0.4443  0.4493  1.3050  1.4458  1.7400  ±2.20%     1126
   · verter AcornLoose  2,855.32  0.2792  1.1495  0.3502  0.3457  0.7993  0.8940  1.1147  ±1.56%     1428
   · Volar                636.94  1.0594  5.5904  1.5700  1.7088  4.1989  4.3093  5.5904  ±4.47%      319
```

**Result:** Verter is **3.53x faster** than Volar

### single card.vue [#######-------------] 14/39 (36%)

```
     name                      hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter              9,137.63  0.0862  0.9537  0.1094  0.1047  0.2136  0.6782  0.8625  ±1.57%     4569
   · verter AcornLoose  11,534.59  0.0709  0.7140  0.0867  0.0805  0.1733  0.4887  0.6337  ±1.29%     5768
   · Volar               2,585.21  0.2670  3.8834  0.3868  0.4007  2.4466  2.7658  3.3055  ±4.68%     1293
```

**Result:** Verter is **3.53x faster** than Volar

### single icon.vue [########------------] 15/39 (38%)

```
     name                     hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter             3,563.11  0.2147  2.2901  0.2807  0.2705  1.4778  1.6292  1.9826  ±2.73%     1782
   · verter AcornLoose  5,773.89  0.1486  0.7526  0.1732  0.1651  0.5246  0.5970  0.7477  ±1.18%     2887
   · Volar              2,062.12  0.3461  3.7246  0.4849  0.5119  2.7246  2.9436  3.3555  ±4.59%     1032
```

**Result:** Verter is **1.73x faster** than Volar

### single index.story.vue [########------------] 16/39 (41%)

```
     name                   hz     min      max    mean     p75     p99     p995     p999     rme  samples
   · Verter             593.04  1.3765   4.3278  1.6862  1.6957  3.2731   4.0243   4.3278  ±3.12%      298
   · verter AcornLoose  783.40  1.0910   2.6633  1.2765  1.2381  2.1568   2.5035   2.6633  ±2.06%      392
   · Volar              237.28  2.8712  14.2938  4.2144  4.9101  7.3642  14.2938  14.2938  ±6.15%      119
```

**Result:** Verter is **2.50x faster** than Volar

### single medium.vue [#########-----------] 17/39 (44%)

```
     name                      hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter             13,466.78  0.0532  0.9910  0.0743  0.0703  0.1495  0.5764  0.7907  ±1.65%     6734
   · verter AcornLoose  18,247.27  0.0432  3.8723  0.0548  0.0490  0.1447  0.4572  0.6716  ±2.18%     9124
   · Volar               4,995.64  0.1327  3.8832  0.2002  0.1719  2.3903  2.6665  3.2155  ±5.70%     2505
```

**Result:** Verter is **2.70x faster** than Volar

### single small.vue [#########-----------] 18/39 (46%)

```
     name                      hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter             15,133.53  0.0518  1.3484  0.0661  0.0597  0.1311  0.5386  0.7364  ±1.63%     7567
   · verter AcornLoose  19,176.68  0.0430  0.7201  0.0521  0.0472  0.1096  0.5045  0.6467  ±1.55%     9589
   · Volar               5,250.83  0.1347  3.8323  0.1904  0.1575  2.3846  2.5162  2.8457  ±5.54%     2626
```

**Result:** Verter is **2.88x faster** than Volar

## completions.bench.ts

**Description:** Tests Vue.js template completion performance in a simple component. Both use LSP+IPC architecture.

Suites: 6

### Script setup completions [##########----------] 19/39 (49%)

```
     name                                     hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - Script setup completions  1,241.03  0.6108  2.4442  0.8058  0.8773  1.5593  1.6885  2.4442  ±1.96%      621
   · Volar - Script setup completions     194.54  4.0965  7.8299  5.1403  5.7528  7.8299  7.8299  7.8299  ±3.35%       98
```

**Result:** Verter is **6.38x faster** than Volar

### TypeScript completions in script [##########----------] 20/39 (51%)

```
     name                                   hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - TypeScript completions  1,841.34  0.3778  1.4523  0.5431  0.5947  1.0642  1.1557  1.4523  ±1.52%      921
   · Volar - TypeScript completions     772.89  1.0892  2.4766  1.2938  1.3292  2.0825  2.1539  2.4766  ±1.45%      387
```

**Result:** Verter is **2.38x faster** than Volar

### Auto import component [###########---------] 21/39 (54%)

```
     name                                hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - Auto import component  201.83  4.6914  5.9436  4.9546  5.0730  5.4892  5.9436  5.9436  ±0.87%      101
   · Volar - Auto import component   164.50  5.4639  7.3782  6.0791  6.4340  7.3782  7.3782  7.3782  ±1.69%       83
```

**Result:** Verter is **1.23x faster** than Volar

### template completion [###########---------] 22/39 (56%)

```
     name          hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter  1,785.44  0.4878  1.3422  0.5601  0.5652  0.9596  1.0445  1.3422  ±1.08%      893
   · Volar     174.82  4.2680  9.0504  5.7202  6.2128  9.0504  9.0504  9.0504  ±3.58%       88
```

**Result:** Verter is **10.21x faster** than Volar

### Complex TypeScript inference [############--------] 23/39 (59%)

```
     name                                         hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - Complex TypeScript inference  6,596.54  0.1043  1.1205  0.1516  0.1488  0.2831  0.3208  0.9883  ±1.02%     3299
   · Volar - Complex TypeScript inference     766.24  1.1059  3.0841  1.3051  1.3840  2.4204  2.6150  3.0841  ±1.74%      384
```

**Result:** Verter is **8.61x faster** than Volar

### Multiple file operations [############--------] 24/39 (62%)

```
     name                                     hz      min      max     mean      p75      p99     p995     p999     rme  samples
   · Verter - Open and complete 5 files   396.35   2.1521   3.6276   2.5230   2.7477   3.5883   3.6276   3.6276  ±1.92%      199
   · Volar - Open and complete 5 files   23.6770  38.6982  51.3575  42.2351  42.0457  51.3575  51.3575  51.3575  ±5.84%       12
```

**Result:** Verter is **16.74x faster** than Volar

## process.bench.ts

**Description:** Benchmark comparison between Volar and Verter.

Suites: 8

### single ContactInformation.vue [#############-------] 25/39 (64%)

```
     name              hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter      6,513.46  0.0817  2.1466  0.1535  0.1757  0.4678  1.3131  1.6423  ±2.80%     3257
   · Volar   2,163,880.27  0.0003  0.6762  0.0005  0.0005  0.0009  0.0009  0.0014  ±0.47%  1081941
```

**Result:** Volar is **332.22x faster** than Verter

### single avatar.vue [#############-------] 26/39 (67%)

```
     name              hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter     15,177.48  0.0455  2.5484  0.0659  0.0553  0.1429  1.3037  1.5631  ±3.75%     7589
   · Volar   1,913,016.00  0.0004  0.4255  0.0005  0.0005  0.0009  0.0010  0.0011  ±0.42%   956508
```

**Result:** Volar is **126.04x faster** than Verter

### single button.vue [##############------] 27/39 (69%)

```
     name              hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter     16,206.54  0.0451  2.0008  0.0617  0.0507  0.1373  1.4183  1.6398  ±4.03%     8104
   · Volar   2,027,064.78  0.0003  0.7946  0.0005  0.0005  0.0008  0.0009  0.0013  ±0.71%  1013533
```

**Result:** Volar is **125.08x faster** than Verter

### single card.vue [##############------] 28/39 (72%)

```
     name              hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter     24,193.77  0.0297  1.8873  0.0413  0.0330  0.0807  0.1445  1.7002  ±4.66%    12097
   · Volar   2,242,667.76  0.0003  0.6115  0.0004  0.0005  0.0008  0.0008  0.0010  ±0.51%  1121335
```

**Result:** Volar is **92.70x faster** than Verter

### single icon.vue [###############-----] 29/39 (74%)

```
     name              hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter     19,732.89  0.0365  2.3409  0.0507  0.0412  0.0847  0.2213  1.9225  ±4.93%     9867
   · Volar   1,911,295.08  0.0004  0.5349  0.0005  0.0005  0.0009  0.0009  0.0011  ±0.77%   956172
```

**Result:** Volar is **96.86x faster** than Verter

### single index.story.vue [###############-----] 30/39 (77%)

```
     name              hz     min      max    mean     p75     p99    p995    p999      rme  samples
   · Verter      6,233.34  0.1127  53.3084  0.1604  0.1291  0.9641  1.0975  1.4115  ±20.97%     3117
   · Volar   2,115,159.88  0.0003   1.2099  0.0005  0.0005  0.0008  0.0009  0.0012   ±0.87%  1057581
```

**Result:** Volar is **339.33x faster** than Verter

### single medium.vue [################----] 31/39 (79%)

```
     name              hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter     25,975.28  0.0256  2.8910  0.0385  0.0297  0.0662  0.0924  1.9427  ±5.27%    12988
   · Volar   2,209,571.12  0.0003  0.5255  0.0005  0.0005  0.0008  0.0008  0.0010  ±0.80%  1104786
```

**Result:** Volar is **85.06x faster** than Verter

### single small.vue [################----] 32/39 (82%)

```
     name              hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter     27,187.24  0.0246  3.0695  0.0368  0.0274  0.0671  0.1054  2.2111  ±6.08%    13594
   · Volar   2,202,272.68  0.0003  0.8147  0.0005  0.0005  0.0007  0.0008  0.0010  ±0.98%  1101137
```

**Result:** Volar is **81.00x faster** than Verter

## real-world-components.bench.ts

**Description:** Measures completion performance in real-world Vue components with multiple edits and completion requests, simulating actual development workflows. Both use LSP+IPC architecture.

Suites: 7

### button.vue - Props in computed (fresh document) [#################---] 33/39 (85%)

```
     name                                                 hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - button.vue props completions (fresh)  1,012.71  0.7117  3.3289  0.9874  1.1284  1.9938  2.0456  3.3289  ±2.42%      507
   · Volar - button.vue props completions (fresh)     556.19  1.3980  4.4044  1.7980  1.9249  3.4692  3.5517  4.4044  ±2.51%      279
```

**Result:** Verter is **1.82x faster** than Volar

### button.vue - Action object properties (fresh) [#################---] 34/39 (87%)

```
     name                                                 hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - button.vue action properties (fresh)  1,334.84  0.6092  1.9452  0.7492  0.7572  1.6871  1.7525  1.9452  ±1.96%      668
   · Volar - button.vue action properties (fresh)     877.40  0.9868  2.1027  1.1397  1.1861  1.7922  1.8019  2.1027  ±1.27%      439
```

**Result:** Verter is **1.52x faster** than Volar

### avatar.vue - Props in computed (fresh) [##################--] 35/39 (90%)

```
     name                                                 hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - avatar.vue props completions (fresh)  1,092.93  0.7067  2.3570  0.9150  1.0484  2.0428  2.2604  2.3570  ±2.35%      547
   · Volar - avatar.vue props completions (fresh)     716.59  1.2014  2.5697  1.3955  1.4536  2.0348  2.1954  2.5697  ±1.40%      359
```

**Result:** Verter is **1.53x faster** than Volar

### avatar.vue - Ref properties (fresh) [##################--] 36/39 (92%)

```
     name                                            hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - avatar.vue ref properties (fresh)  836.33  0.9504  8.0870  1.1957  1.2130  2.6710  2.9749  8.0870  ±3.88%      419
   · Volar - avatar.vue ref properties (fresh)   581.24  1.5071  2.5083  1.7204  1.7938  2.4429  2.4871  2.5083  ±1.45%      291
```

**Result:** Verter is **1.44x faster** than Volar

### avatar.vue - HTMLElement properties (fresh) [###################-] 37/39 (95%)

```
     name                                                    hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - avatar.vue HTMLElement properties (fresh)  518.87  1.7307  3.3838  1.9273  1.9094  2.9231  3.0358  3.3838  ±1.73%      260
   · Volar - avatar.vue HTMLElement properties (fresh)   436.23  1.9561  4.5138  2.2924  2.3360  3.4472  3.4782  4.5138  ±2.31%      219
```

**Result:** Verter is **1.19x faster** than Volar

### icon.vue - Ref element (fresh) [###################-] 38/39 (97%)

```
     name                                         hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · Verter - icon.vue ref element (fresh)  1,082.82  0.7589  2.1647  0.9235  0.9887  1.6711  1.8708  2.1647  ±1.87%      542
   · Volar - icon.vue ref element (fresh)     840.33  1.0460  2.2289  1.1900  1.2267  1.8700  2.0172  2.2289  ±1.29%      422
```

**Result:** Verter is **1.29x faster** than Volar

### Real world editing workflow [####################] 39/39 (100%)

```
     name                                     hz      min      max     mean      p75      p99     p995     p999     rme  samples
   · Verter - complete editing workflow   470.43   1.8495   3.4495   2.1257   2.2274   3.3423   3.3435   3.4495  ±2.14%      236
   · Volar - complete editing workflow   34.9997  24.2717  34.9406  28.5717  30.3923  34.9406  34.9406  34.9406  ±4.75%       18
```

**Result:** Verter is **13.44x faster** than Volar


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
| parser.bench.ts | single ContactInformation.vue | 5,998.71 ops/sec | 222,833.51 ops/sec | ⚠️ 37.15x slower |
| parser.bench.ts | single avatar.vue | 4,623.46 ops/sec | 224,300.72 ops/sec | ⚠️ 48.51x slower |
| parser.bench.ts | single button.vue | 5,369.18 ops/sec | 171,653.42 ops/sec | ⚠️ 31.97x slower |
| parser.bench.ts | single card.vue | 28,049.53 ops/sec | 201,283.56 ops/sec | ⚠️ 7.18x slower |
| parser.bench.ts | single dynamicInput.vue | 14,010.95 ops/sec | 202,039.92 ops/sec | ⚠️ 14.42x slower |
| parser.bench.ts | single icon.vue | 6,670.95 ops/sec | 187,381.63 ops/sec | ⚠️ 28.09x slower |
| parser.bench.ts | single index.story.vue | 1,306.71 ops/sec | 189,458.42 ops/sec | ⚠️ 144.99x slower |
| parser.bench.ts | single medium.vue | 56,374.49 ops/sec | 203,377.81 ops/sec | ⚠️ 3.61x slower |
| parser.bench.ts | single small.vue | 60,993.77 ops/sec | 197,235.96 ops/sec | ⚠️ 3.23x slower |
| parser.bench.ts | single table.vue | 1,187.35 ops/sec | 199,055.56 ops/sec | ⚠️ 167.65x slower |
| parser_process.bench.ts | single ContactInformation.vue | 1,267.27 ops/sec | 165.1 ops/sec | ✅ 7.68x faster |
| parser_process.bench.ts | single avatar.vue | 1,713.36 ops/sec | 688.76 ops/sec | ✅ 2.49x faster |
| parser_process.bench.ts | single button.vue | 2,250.93 ops/sec | 636.94 ops/sec | ✅ 3.53x faster |
| parser_process.bench.ts | single card.vue | 9,137.63 ops/sec | 2,585.21 ops/sec | ✅ 3.53x faster |
| parser_process.bench.ts | single icon.vue | 3,563.11 ops/sec | 2,062.12 ops/sec | ✅ 1.73x faster |
| parser_process.bench.ts | single index.story.vue | 593.04 ops/sec | 237.28 ops/sec | ✅ 2.50x faster |
| parser_process.bench.ts | single medium.vue | 13,466.78 ops/sec | 4,995.64 ops/sec | ✅ 2.70x faster |
| parser_process.bench.ts | single small.vue | 15,133.53 ops/sec | 5,250.83 ops/sec | ✅ 2.88x faster |
| completions.bench.ts | Script setup completions | 1,241.03 ops/sec | 194.54 ops/sec | ✅ 6.38x faster |
| completions.bench.ts | TypeScript completions in script | 1,841.34 ops/sec | 772.89 ops/sec | ✅ 2.38x faster |
| completions.bench.ts | Auto import component | 201.83 ops/sec | 164.5 ops/sec | ✅ 1.23x faster |
| completions.bench.ts | template completion | 1,785.44 ops/sec | 174.82 ops/sec | ✅ 10.21x faster |
| completions.bench.ts | Complex TypeScript inference | 6,596.54 ops/sec | 766.24 ops/sec | ✅ 8.61x faster |
| completions.bench.ts | Multiple file operations | 396.35 ops/sec | 23.68 ops/sec | ✅ 16.74x faster |
| process.bench.ts | single ContactInformation.vue | 6,513.46 ops/sec | 2,163,880.27 ops/sec | ⚠️ 332.22x slower |
| process.bench.ts | single avatar.vue | 15,177.48 ops/sec | 1,913,016 ops/sec | ⚠️ 126.04x slower |
| process.bench.ts | single button.vue | 16,206.54 ops/sec | 2,027,064.78 ops/sec | ⚠️ 125.08x slower |
| process.bench.ts | single card.vue | 24,193.77 ops/sec | 2,242,667.76 ops/sec | ⚠️ 92.70x slower |
| process.bench.ts | single icon.vue | 19,732.89 ops/sec | 1,911,295.08 ops/sec | ⚠️ 96.86x slower |
| process.bench.ts | single index.story.vue | 6,233.34 ops/sec | 2,115,159.88 ops/sec | ⚠️ 339.33x slower |
| process.bench.ts | single medium.vue | 25,975.28 ops/sec | 2,209,571.12 ops/sec | ⚠️ 85.06x slower |
| process.bench.ts | single small.vue | 27,187.24 ops/sec | 2,202,272.68 ops/sec | ⚠️ 81.00x slower |
| real-world-components.bench.ts | button.vue - Props in computed (fresh document) | 1,012.71 ops/sec | 556.19 ops/sec | ✅ 1.82x faster |
| real-world-components.bench.ts | button.vue - Action object properties (fresh) | 1,334.84 ops/sec | 877.4 ops/sec | ✅ 1.52x faster |
| real-world-components.bench.ts | avatar.vue - Props in computed (fresh) | 1,092.93 ops/sec | 716.59 ops/sec | ✅ 1.53x faster |
| real-world-components.bench.ts | avatar.vue - Ref properties (fresh) | 836.33 ops/sec | 581.24 ops/sec | ✅ 1.44x faster |
| real-world-components.bench.ts | avatar.vue - HTMLElement properties (fresh) | 518.87 ops/sec | 436.23 ops/sec | ✅ 1.19x faster |
| real-world-components.bench.ts | icon.vue - Ref element (fresh) | 1,082.82 ops/sec | 840.33 ops/sec | ✅ 1.29x faster |
| real-world-components.bench.ts | Real world editing workflow | 470.43 ops/sec | 35 ops/sec | ✅ 13.44x faster |
