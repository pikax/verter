# J1 §2 Bounds — Latency perf baseline (pre-cutover)

Captured per the charter's Latency bound (`docs/arch/refactor/rev11/charters/J1.md` §2): no committed
wall-clock baseline existed before this capture. This is the pre-cutover baseline, run against the
**legacy lightningcss pipeline** (`verter_compiler::css::{process_style, prepass, scoped::apply_scoped,
modules::apply_css_modules}`, row 3 in J1 §1.1 — untouched by Slice 1) via `crates/verter_bench/benches/css_bench.rs`,
which is itself untouched by this slice's work. It gates deletion of that legacy pipeline (A1/A2, Slice 2):
the converged `style_planner` pipeline's own wall-clock/allocation numbers must land within the ratified
1.2x (20%) ceiling of these numbers, per generator category, before the legacy route is removed.

This capture is informational/baseline-only — it is not interpreted or acted on here.

## Environment

- Command: `cargo bench -p verter_bench --bench css_bench -- --quick` (criterion 0.8.2, `--quick` sampling
  mode — fewer iterations than the criterion default, still representative for a baseline capture)
- `rustc 1.97.1 (8bab26f4f 2026-07-14)`
- `Darwin 25.6.0 arm64` (Apple M3, 24 GiB RAM)
- Git commit: `7bb392521d4453c958e65ad9d9b69673eedc3d32` (branch `block/css-engine-ownership`)
- Bench profile: `release` (criterion's `bench` cargo profile — optimized)

## Raw output

```
Benchmarking process_style/scoped/classes/5
Benchmarking process_style/scoped/classes/5: Analyzing
process_style/scoped/classes/5
                        time:   [5.4709 µs 5.4893 µs 5.5628 µs]
                        thrpt:  [33.259 MiB/s 33.704 MiB/s 33.818 MiB/s]
Benchmarking process_style/scoped/classes/20
Benchmarking process_style/scoped/classes/20: Analyzing
process_style/scoped/classes/20
                        time:   [21.660 µs 21.672 µs 21.718 µs]
                        thrpt:  [35.086 MiB/s 35.160 MiB/s 35.179 MiB/s]
Benchmarking process_style/scoped/classes/50
Benchmarking process_style/scoped/classes/50: Analyzing
process_style/scoped/classes/50
                        time:   [52.800 µs 53.046 µs 54.030 µs]
                        thrpt:  [35.814 MiB/s 36.478 MiB/s 36.648 MiB/s]
Benchmarking process_style/scoped/pseudo/20
Benchmarking process_style/scoped/pseudo/20: Analyzing
process_style/scoped/pseudo/20
                        time:   [17.731 µs 17.742 µs 17.788 µs]
                        thrpt:  [34.153 MiB/s 34.240 MiB/s 34.262 MiB/s]
Benchmarking process_style/modules/classes/5
Benchmarking process_style/modules/classes/5: Analyzing
process_style/modules/classes/5
                        time:   [6.6012 µs 6.6302 µs 6.6374 µs]
                        thrpt:  [27.874 MiB/s 27.905 MiB/s 28.027 MiB/s]
Benchmarking process_style/modules/classes/20
Benchmarking process_style/modules/classes/20: Analyzing
process_style/modules/classes/20
                        time:   [26.407 µs 26.426 µs 26.431 µs]
                        thrpt:  [28.830 MiB/s 28.835 MiB/s 28.855 MiB/s]
Benchmarking process_style/modules/classes/50
Benchmarking process_style/modules/classes/50: Analyzing
process_style/modules/classes/50
                        time:   [64.229 µs 64.246 µs 64.250 µs]
                        thrpt:  [30.117 MiB/s 30.119 MiB/s 30.127 MiB/s]
Benchmarking process_style/scoped+modules/20
Benchmarking process_style/scoped+modules/20: Analyzing
process_style/scoped+modules/20
                        time:   [33.937 µs 33.982 µs 34.164 µs]
                        thrpt:  [22.304 MiB/s 22.423 MiB/s 22.453 MiB/s]
Benchmarking process_style/v-bind/simple/1
Benchmarking process_style/v-bind/simple/1: Analyzing
process_style/v-bind/simple/1
                        time:   [1.4436 µs 1.4446 µs 1.4488 µs]
                        thrpt:  [22.380 MiB/s 22.445 MiB/s 22.462 MiB/s]
Benchmarking process_style/v-bind/simple/5
Benchmarking process_style/v-bind/simple/5: Analyzing
process_style/v-bind/simple/5
                        time:   [6.0849 µs 6.0920 µs 6.1205 µs]
                        thrpt:  [27.112 MiB/s 27.239 MiB/s 27.271 MiB/s]
Benchmarking process_style/v-bind/simple/20
Benchmarking process_style/v-bind/simple/20: Analyzing
process_style/v-bind/simple/20
                        time:   [23.153 µs 23.540 µs 23.636 µs]
                        thrpt:  [29.010 MiB/s 29.129 MiB/s 29.616 MiB/s]
Benchmarking process_style/passthrough/20
Benchmarking process_style/passthrough/20: Analyzing
process_style/passthrough/20
                        time:   [2.0066 µs 2.0082 µs 2.0143 µs]
                        thrpt:  [378.29 MiB/s 379.44 MiB/s 379.73 MiB/s]

Benchmarking prepass/passthrough/5
Benchmarking prepass/passthrough/5: Analyzing
prepass/passthrough/5   time:   [503.50 ns 504.50 ns 508.49 ns]
                        thrpt:  [363.85 MiB/s 366.73 MiB/s 367.45 MiB/s]
Benchmarking prepass/passthrough/20
Benchmarking prepass/passthrough/20: Analyzing
prepass/passthrough/20  time:   [2.0265 µs 2.0287 µs 2.0376 µs]
                        thrpt:  [373.97 MiB/s 375.61 MiB/s 376.02 MiB/s]
Benchmarking prepass/passthrough/50
Benchmarking prepass/passthrough/50: Analyzing
prepass/passthrough/50  time:   [5.0381 µs 5.2101 µs 5.2532 µs]
                        thrpt:  [368.35 MiB/s 371.39 MiB/s 384.07 MiB/s]
Benchmarking prepass/v-bind/simple/1
Benchmarking prepass/v-bind/simple/1: Analyzing
prepass/v-bind/simple/1 time:   [320.05 ns 326.13 ns 327.65 ns]
                        thrpt:  [98.963 MiB/s 99.424 MiB/s 101.31 MiB/s]
Benchmarking prepass/v-bind/simple/5
Benchmarking prepass/v-bind/simple/5: Analyzing
prepass/v-bind/simple/5 time:   [1.5832 µs 1.5834 µs 1.5843 µs]
                        thrpt:  [104.74 MiB/s 104.80 MiB/s 104.81 MiB/s]
Benchmarking prepass/v-bind/simple/20
Benchmarking prepass/v-bind/simple/20: Analyzing
prepass/v-bind/simple/20
                        time:   [6.1157 µs 6.1466 µs 6.2702 µs]
                        thrpt:  [109.36 MiB/s 111.56 MiB/s 112.12 MiB/s]
Benchmarking prepass/v-bind/dotted/1
Benchmarking prepass/v-bind/dotted/1: Analyzing
prepass/v-bind/dotted/1 time:   [359.35 ns 360.03 ns 362.71 ns]
                        thrpt:  [134.09 MiB/s 135.09 MiB/s 135.35 MiB/s]
Benchmarking prepass/v-bind/dotted/5
Benchmarking prepass/v-bind/dotted/5: Analyzing
prepass/v-bind/dotted/5 time:   [1.7353 µs 1.7386 µs 1.7520 µs]
                        thrpt:  [140.98 MiB/s 142.07 MiB/s 142.34 MiB/s]
Benchmarking prepass/v-bind/dotted/20
Benchmarking prepass/v-bind/dotted/20: Analyzing
prepass/v-bind/dotted/20
                        time:   [7.0942 µs 7.1358 µs 7.1462 µs]
                        thrpt:  [141.32 MiB/s 141.53 MiB/s 142.36 MiB/s]
Benchmarking prepass/deep/5
Benchmarking prepass/deep/5: Analyzing
prepass/deep/5          time:   [825.68 ns 828.64 ns 840.51 ns]
                        thrpt:  [180.41 MiB/s 182.99 MiB/s 183.65 MiB/s]
Benchmarking prepass/deep/20
Benchmarking prepass/deep/20: Analyzing
prepass/deep/20         time:   [3.3769 µs 3.3974 µs 3.4792 µs]
                        thrpt:  [177.89 MiB/s 182.18 MiB/s 183.28 MiB/s]
Benchmarking prepass/slotted/5
Benchmarking prepass/slotted/5: Analyzing
prepass/slotted/5       time:   [699.83 ns 699.93 ns 700.35 ns]
                        thrpt:  [230.13 MiB/s 230.27 MiB/s 230.30 MiB/s]
Benchmarking prepass/slotted/20
Benchmarking prepass/slotted/20: Analyzing
prepass/slotted/20      time:   [2.8466 µs 2.8568 µs 2.8593 µs]
                        thrpt:  [229.80 MiB/s 230.01 MiB/s 230.83 MiB/s]
Benchmarking prepass/mixed/6
Benchmarking prepass/mixed/6: Analyzing
prepass/mixed/6         time:   [1.2449 µs 1.2527 µs 1.2546 µs]
                        thrpt:  [157.34 MiB/s 157.59 MiB/s 158.57 MiB/s]
Benchmarking prepass/mixed/30
Benchmarking prepass/mixed/30: Analyzing
prepass/mixed/30        time:   [6.6930 µs 6.7164 µs 6.8102 µs]
                        thrpt:  [151.10 MiB/s 153.21 MiB/s 153.75 MiB/s]

Benchmarking scoped/single_class
Benchmarking scoped/single_class: Analyzing
scoped/single_class     time:   [849.26 ns 851.48 ns 860.36 ns]
                        thrpt:  [22.169 MiB/s 22.400 MiB/s 22.459 MiB/s]
Benchmarking scoped/descendant/5
Benchmarking scoped/descendant/5: Analyzing
scoped/descendant/5     time:   [4.6607 µs 4.6753 µs 4.7337 µs]
                        thrpt:  [36.062 MiB/s 36.513 MiB/s 36.627 MiB/s]
Benchmarking scoped/descendant/20
Benchmarking scoped/descendant/20: Analyzing
scoped/descendant/20    time:   [18.762 µs 18.950 µs 18.998 µs]
                        thrpt:  [37.097 MiB/s 37.190 MiB/s 37.564 MiB/s]
Benchmarking scoped/selector_list/5
Benchmarking scoped/selector_list/5: Analyzing
scoped/selector_list/5  time:   [6.7241 µs 6.7518 µs 6.8622 µs]
                        thrpt:  [31.825 MiB/s 32.346 MiB/s 32.479 MiB/s]
Benchmarking scoped/selector_list/20
Benchmarking scoped/selector_list/20: Analyzing
scoped/selector_list/20 time:   [27.374 µs 27.394 µs 27.478 µs]
                        thrpt:  [33.284 MiB/s 33.385 MiB/s 33.411 MiB/s]
Benchmarking scoped/pseudo/5
Benchmarking scoped/pseudo/5: Analyzing
scoped/pseudo/5         time:   [4.1623 µs 4.1685 µs 4.1932 µs]
                        thrpt:  [35.480 MiB/s 35.690 MiB/s 35.743 MiB/s]
Benchmarking scoped/pseudo/20
Benchmarking scoped/pseudo/20: Analyzing
scoped/pseudo/20        time:   [16.704 µs 16.741 µs 16.750 µs]
                        thrpt:  [36.267 MiB/s 36.287 MiB/s 36.367 MiB/s]
Benchmarking scoped/global/5
Benchmarking scoped/global/5: Analyzing
scoped/global/5         time:   [4.7882 µs 4.7993 µs 4.8436 µs]
                        thrpt:  [32.291 MiB/s 32.589 MiB/s 32.664 MiB/s]
Benchmarking scoped/global/20
Benchmarking scoped/global/20: Analyzing
scoped/global/20        time:   [17.899 µs 17.925 µs 18.031 µs]
                        thrpt:  [35.383 MiB/s 35.592 MiB/s 35.645 MiB/s]

Benchmarking modules/unique_classes/3
Benchmarking modules/unique_classes/3: Analyzing
modules/unique_classes/3
                        time:   [3.8384 µs 3.9258 µs 3.9477 µs]
                        thrpt:  [28.023 MiB/s 28.179 MiB/s 28.821 MiB/s]
Benchmarking modules/unique_classes/10
Benchmarking modules/unique_classes/10: Analyzing
modules/unique_classes/10
                        time:   [12.216 µs 12.286 µs 12.566 µs]
                        thrpt:  [29.521 MiB/s 30.195 MiB/s 30.369 MiB/s]
Benchmarking modules/unique_classes/30
Benchmarking modules/unique_classes/30: Analyzing
modules/unique_classes/30
                        time:   [36.637 µs 36.972 µs 38.311 µs]
                        thrpt:  [30.096 MiB/s 31.186 MiB/s 31.471 MiB/s]
Benchmarking modules/repeated_5x/2
Benchmarking modules/repeated_5x/2: Analyzing
modules/repeated_5x/2   time:   [9.2074 µs 9.3109 µs 9.7248 µs]
                        thrpt:  [24.418 MiB/s 25.504 MiB/s 25.791 MiB/s]
Benchmarking modules/repeated_5x/5
Benchmarking modules/repeated_5x/5: Analyzing
modules/repeated_5x/5   time:   [19.404 µs 19.441 µs 19.589 µs]
                        thrpt:  [30.379 MiB/s 30.611 MiB/s 30.669 MiB/s]
Benchmarking modules/repeated_5x/10
Benchmarking modules/repeated_5x/10: Analyzing
modules/repeated_5x/10  time:   [37.612 µs 37.775 µs 38.431 µs]
                        thrpt:  [30.994 MiB/s 31.532 MiB/s 31.669 MiB/s]
```

## Allocation baseline

Deferred: the counting-allocator test binary (`crates/verter_compiler/tests/allocator_canaries.rs` addition,
mirroring `verter_session`'s `allocator_canaries.rs`) is out of Slice 1's scope (named explicitly deferred in
the implementer brief for this row). The wall-clock capture above is committed now per the charter's
requirement that it block legacy-pipeline deletion (A1/A2); the allocation-count baseline is captured
alongside it only once that separate test binary lands.
