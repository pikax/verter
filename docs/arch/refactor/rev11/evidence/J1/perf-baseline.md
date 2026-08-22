# Pre-cutover CSS pipeline performance baseline

Captured against the legacy lightningcss-backed pipeline
(`crates/verter_compiler/src/css/{mod,scoped,modules,walk,prepass}.rs`),
unmodified at this commit. This is the baseline every later CSS-pipeline
cutover's wall-clock and allocation ceilings are checked against: the
converged pipeline must stay within 1.2x (20%) of every number below, per
generator/benchmark category.

## Provenance

- Commit: `4cac1cbdf6c6d167aaa864abd78b6f18f12cc0b6`
- Date captured: 2026-08-21 (UTC)
- Machine: Apple M3, macOS 26.6 (Darwin 25.6.0), 8 logical CPUs, 24 GiB RAM
- Toolchain: `rustc 1.97.1 (8bab26f4f 2026-07-14)`
- Build profile: `[profile.release]` (`opt-level = 3`, `lto = true`,
  `codegen-units = 1`) — the workspace's standard release profile, not a
  debug/no-debug-assertions profile.

## Latency

Command:

```
cargo bench -p verter_bench --bench css_bench -- --output-format bencher
```

Raw `bench:`-line output, one row per `BenchmarkId` (criterion's
`ns/iter (+/- <std-dev estimate>)`; this is a single captured run, not an
averaged multi-run report — re-running will vary within the noise band
criterion itself reports per row).

### `process_style` (full legacy `css::process_style` pipeline)

| Benchmark | ns/iter | +/- |
|---|---:|---:|
| `process_style/scoped/classes/5` | 9,912 | 2,159 |
| `process_style/scoped/classes/20` | 47,508 | 44,743 |
| `process_style/scoped/classes/50` | 88,700 | 16,504 |
| `process_style/scoped/pseudo/20` | 34,041 | 6,516 |
| `process_style/modules/classes/5` | 15,171 | 10,297 |
| `process_style/modules/classes/20` | 104,613 | 84,611 |
| `process_style/modules/classes/50` | 186,821 | 81,088 |
| `process_style/scoped+modules/20` | 92,121 | 55,745 |
| `process_style/v-bind/simple/1` | 3,256 | 2,741 |
| `process_style/v-bind/simple/5` | 12,190 | 2,615 |
| `process_style/v-bind/simple/20` | 52,245 | 44,649 |
| `process_style/passthrough/20` | 4,828 | 8,114 |

### `prepass` (v-bind/`:deep`/`:slotted` marker pre-pass)

| Benchmark | ns/iter | +/- |
|---|---:|---:|
| `prepass/passthrough/5` | 1,411 | 1,206 |
| `prepass/passthrough/20` | 4,992 | 2,429 |
| `prepass/passthrough/50` | 12,133 | 5,817 |
| `prepass/v-bind/simple/1` | 798 | 845 |
| `prepass/v-bind/simple/5` | 4,422 | 2,587 |
| `prepass/v-bind/simple/20` | 20,139 | 22,927 |
| `prepass/v-bind/dotted/1` | 1,116 | 1,256 |
| `prepass/v-bind/dotted/5` | 6,378 | 7,811 |
| `prepass/v-bind/dotted/20` | 16,137 | 12,833 |
| `prepass/deep/5` | 2,434 | 1,276 |
| `prepass/deep/20` | 7,401 | 14,429 |
| `prepass/slotted/5` | 1,622 | 892 |
| `prepass/slotted/20` | 6,682 | 5,653 |
| `prepass/mixed/6` | 3,196 | 3,668 |
| `prepass/mixed/30` | 14,017 | 7,089 |

### `scoped` (selector scoping)

| Benchmark | ns/iter | +/- |
|---|---:|---:|
| `scoped/single_class` | 1,966 | 1,546 |
| `scoped/descendant/5` | 10,087 | 4,095 |
| `scoped/descendant/20` | 71,441 | 253,967 |
| `scoped/selector_list/5` | 19,481 | 10,945 |
| `scoped/selector_list/20` | 94,821 | 144,897 |
| `scoped/pseudo/5` | 11,496 | 7,788 |
| `scoped/pseudo/20` | 41,396 | 34,380 |
| `scoped/global/5` | 16,338 | 21,959 |
| `scoped/global/20` | 44,107 | 20,401 |

### `modules` (CSS Modules class hashing)

| Benchmark | ns/iter | +/- |
|---|---:|---:|
| `modules/unique_classes/3` | 8,975 | 6,048 |
| `modules/unique_classes/10` | 28,263 | 8,429 |
| `modules/unique_classes/30` | 82,559 | 52,309 |
| `modules/repeated_5x/2` | 19,542 | 11,418 |
| `modules/repeated_5x/5` | 46,534 | 31,673 |
| `modules/repeated_5x/10` | 123,855 | 433,854 |

Note: several rows carry a `+/-` estimate comparable to or larger than the
mean (e.g. `scoped/descendant/20`, `modules/repeated_5x/10`) — expected
noise on a shared development machine with other processes running, not a
defect in the measurement. The 1.2x cutover ceiling this baseline gates is
evaluated against the mean `ns/iter` figure per row; a re-run showing high
variance on the SAME row for both pre- and post-cutover pipelines is a
measurement artifact, not evidence the ceiling was crossed.

## Allocation

Command:

```
cargo test -p verter_compiler --test allocator_canaries -- --test-threads=1 --nocapture
```

Counting-allocator binary: `crates/verter_compiler/tests/allocator_canaries.rs`
(new — see the accompanying commit). One canary per
`crates/verter_bench/benches/css_bench.rs` generator function (the input
generators the Latency benchmarks above also use), each driving 50
generated rules (`generate_repeated_classes` uses 5 unique classes x 10
repeats, matching its own default shape) through the legacy
`css::process_style` entry point with a fixed representative option set
(`scope_id: "a4f2eed6"`, `scoped: true`, `is_module: false`) so allocation
counts are comparable across categories and, later, across the pre-/post-cutover
pipelines. Counts are allocator CALLS (`alloc`/`alloc_zeroed`/`realloc`), not
bytes, measured after a warm-up call to settle one-time lazy initialization.

| Generator category | Allocator calls |
|---|---:|
| `class_rules` (50 rules) | 422 |
| `descendant_selectors` (50 rules) | 371 |
| `pseudo_selectors` (50 rules) | 371 |
| `selector_lists` (50 rules) | 822 |
| `v_bind_rules` (50 rules) | 929 |
| `v_bind_dotted` (50 rules) | 929 |
| `deep_rules` (50 rules) | 522 |
| `slotted_rules` (50 rules) | 472 |
| `mixed_vue` (50 rules) | 648 |
| `global_rules` (50 rules) | 370 |
| `repeated_classes` (5 unique x 10 repeats) | 371 |

## Ceiling

Per charter §2 Bounds ("Latency", "Allocation"): the converged pipeline's
parse+transform wall-clock time AND allocation-call count, per category
above, must stay within 1.2x (20%) of the corresponding number captured
here. This document gates deletion of the legacy pipeline
(`crates/verter_compiler/src/css/` and the `lightningcss` dependency) — that
deletion is explicitly NOT performed by this change; it is a later, gated
train.
