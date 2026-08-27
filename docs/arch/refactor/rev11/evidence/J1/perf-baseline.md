> Historical recapture of the deleted lightningcss pipeline. This file is the
> frozen A31 baseline, not a live capture recipe. Lightningcss is gone; do not
> recapture this pipeline. The live A29 named canary
> `converged_style_pipeline_allocation_within_ratified_ceiling` is the 1.2x
> allocation gate and must stay GREEN. The table below that says every category
> misses 1.2x is this pre-arena recapture, not the live canary.

# J1 §2 Bounds — recaptured latency and allocation baseline

Recapture of the pre-deletion CSS style-pipeline baseline on this tree, through
the H013 runner (`crates/verter_bench/src/bin/css_latency_gate.rs`) against the
legacy lightningcss pipeline (`verter_compiler::css::{process_style, prepass,
scoped::apply_scoped, modules::apply_css_modules}`). The identity universe is
derived from `verter_bench::css_identities` (the same module `css_bench.rs`
registers). This artifact is the committed pre-convergence record A31's
produce-then-gate command consumes. Historical `--quick` criterion numbers and
donor JSON are not this recapture.

Machine-bound raw capture: `docs/arch/refactor/rev11/evidence/J1/css-baseline-legacy.json`.

## Environment

- Command: `cargo run -p verter_bench --release --bin css_latency_gate -- capture --out docs/arch/refactor/rev11/evidence/J1/css-baseline-legacy.json --pipeline legacy-lightningcss`
- Sampling: `css_gate::SAMPLING_MODE` (warmup ≥30 iters and ≥100 ms, calibrate
  iters-per-sample so one sample is ≥2 ms, 30 samples, statistic = median of
  per-sample means). Not criterion `--quick`.
- `rustc 1.97.1 (8bab26f4f 2026-07-14)`
- `aarch64-apple-darwin`, `Apple M3`, 8 logical CPUs, 25769803776 RAM bytes,
  `macos 25.6.0`
- Git commit at capture: `1548e5b23d199fd9c761d952f50b4ecb4d5888bb`
- Tree object: `23c24944286825cbae660469b0b132de9bce0f1f`
- `css_bench.rs` blob: `e127383985e853c72082ee572a2d1d8c39463f3a`
- `css_identities.rs` blob: `7acd265b6fddb353ac1896c080f99a99a19e1d08`
- Cargo profile: `release`; enabled features: none
- Load average before/after: `{ 5.22 7.79 10.06 }` / `{ 5.28 7.76 10.03 }`
- Captured at: `2026-08-27T14:03:01Z`
- Record integrity (sha256 of canonical JSON with empty `integrity`): see JSON

## Latency (42 identities)

Wall-clock median nanoseconds, 30 samples each.

| identity | median ns | alloc count |
|---|---:|---:|
| process_style/scoped/classes/5 | 6153 | 54 |
| process_style/scoped/classes/20 | 24151 | 179 |
| process_style/scoped/classes/50 | 56458 | 422 |
| process_style/scoped/pseudo/20 | 18394 | 159 |
| process_style/modules/classes/5 | 7235 | 82 |
| process_style/modules/classes/20 | 29018 | 293 |
| process_style/modules/classes/50 | 71214 | 718 |
| process_style/scoped+modules/20 | 37449 | 377 |
| process_style/v-bind/simple/1 | 1493 | 31 |
| process_style/v-bind/simple/5 | 6367 | 107 |
| process_style/v-bind/simple/20 | 24496 | 385 |
| process_style/passthrough/20 | 2130 | 0 |
| prepass/passthrough/5 | 530 | 0 |
| prepass/passthrough/20 | 2160 | 0 |
| prepass/passthrough/50 | 5445 | 0 |
| prepass/v-bind/simple/1 | 345 | 11 |
| prepass/v-bind/simple/5 | 1581 | 48 |
| prepass/v-bind/simple/20 | 6395 | 186 |
| prepass/v-bind/dotted/1 | 367 | 11 |
| prepass/v-bind/dotted/5 | 1931 | 48 |
| prepass/v-bind/dotted/20 | 7684 | 185 |
| prepass/deep/5 | 926 | 16 |
| prepass/deep/20 | 3663 | 61 |
| prepass/slotted/5 | 722 | 11 |
| prepass/slotted/20 | 2947 | 41 |
| prepass/mixed/6 | 1302 | 30 |
| prepass/mixed/30 | 7056 | 145 |
| scoped/single_class | 917 | 17 |
| scoped/descendant/5 | 5140 | 49 |
| scoped/descendant/20 | 19602 | 159 |
| scoped/selector_list/5 | 7163 | 94 |
| scoped/selector_list/20 | 28050 | 339 |
| scoped/pseudo/5 | 4335 | 49 |
| scoped/pseudo/20 | 17754 | 159 |
| scoped/global/5 | 5154 | 49 |
| scoped/global/20 | 19666 | 158 |
| modules/unique_classes/3 | 4067 | 53 |
| modules/unique_classes/10 | 12324 | 149 |
| modules/unique_classes/30 | 37394 | 434 |
| modules/repeated_5x/2 | 8935 | 123 |
| modules/repeated_5x/5 | 21676 | 260 |
| modules/repeated_5x/10 | 41700 | 488 |

A31 ceiling: every candidate identity's wall-clock median must be ≤ 1.2× the
value in this table. Exact-set: compiled-in universe = this record = fresh
candidate, else refuse.

## Allocation (11 generator categories)

Canary protocol: `css::process_style` with `scoped=true`, `is_module=false`,
`scope_id="a4f2eed6"`, `N=50` (or `generate_repeated_classes(5, 10)`). Live
debug-profile recapture via
`dual_pipeline_allocation_instrument::each_category_observes_both_pipelines`
(`--test-threads=1`) on the same tree. Release-profile
`allocation_by_category` in the JSON matched these counts exactly.

| category | legacy count | style_planner count | ratio |
|---|---:|---:|---:|
| class_rules | 422 | 621 | 1.472× |
| descendant_selectors | 371 | 670 | 1.806× |
| pseudo_selectors | 371 | 570 | 1.536× |
| selector_lists | 822 | 1274 | 1.550× |
| v_bind_rules | 929 | 1442 | 1.552× |
| v_bind_dotted | 929 | 1442 | 1.552× |
| deep_rules | 522 | 1020 | 1.954× |
| slotted_rules | 472 | 974 | 2.064× |
| mixed_vue | 648 | 1656 | 2.556× |
| global_rules | 370 | 920 | 2.486× |
| repeated_classes | 371 | 572 | 1.542× |

These legacy counts are the retained values in
`crates/verter_compiler/tests/allocator_canaries.rs::allocation_ceiling::RETAINED_LEGACY_ALLOC`.
`retained_legacy_allocation_matches_live_legacy_pipeline` requires the live
legacy pipeline still to produce them. The named A29 gate
`converged_style_pipeline_allocation_within_ratified_ceiling` asserts each
converged/legacy ratio ≤ 1.2×; on this recapture every category misses (1.472×–2.556×).
The assertion is not ignored and is not rebased.
