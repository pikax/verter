# Disabled-overhead measurement

The claim under test: with the `attribution` feature off — the configuration
every production build and the canonical gate use — the instrumentation costs
nothing.

Three independent proofs, because no single one is sufficient.

## 1. Structural: the OFF arm emits no code

Each recording macro has two `#[cfg]` arms. The disabled arm expands to a
single `const` item naming the site:

```rust
const _: WorkSite = WorkSite::ContentHash;
```

A `const` with no reads produces no instructions: no atomics, no clock reads,
no branch. The amount/digest expression is **not mentioned at all**, so an
expensive quantity (`allocator.used_bytes()`, which walks the arena chunk
list) costs nothing when the feature is off.

Naming the site in the disabled arm is deliberate: a renamed or deleted
variant fails a DEFAULT build, so instrumentation cannot rot behind a feature
nobody enables. The trade-off is that the disabled arm does not type-check the
amount expression — `cargo check -p <crate> --features verter_audit/attribution`
is what does. That gap is real and was observed during this block: a wrong
field name in the compiled-output digest compiled clean in the default arm and
failed only under the feature.

## 2. Behavioural: the argument is never evaluated

`crates/verter_audit/src/attribution/disabled_tests.rs` →
`disabled_macros_never_evaluate_their_argument`.

A tattling closure increments a thread-local when called. It is passed to
`attribute_n!`, `attribute_max!` and `attribute_digest!` with the feature off;
the counter must read 0. The test then calls the tattler directly and asserts
it reads 1 — a control proving a zero means "not evaluated", not "broken
probe". Compiled ONLY when the feature is off, so it runs in the default gate.

## 3. Measured: wall-clock A/B against the uninstrumented tree

Three arms over the identical workload (40 synthesised Vue components + a
shared TS module; upsert → `ensure_loaded` → `get_component_meta` per
component → `compile_many` over the corpus), release profile, 7 measured runs
per invocation after a warmup run.

- **control** — the pre-instrumentation tree at `839645e3e`, harness copied in
  verbatim with the attribution blocks stripped (they do not exist there).
- **off** — this tree, default features.
- **on** — this tree, `--features attribution`, which also installs
  `AttributingAllocator` as the global allocator.

| arm     | median ms (rep 1) | median ms (rep 2) |
| ------- | ----------------- | ----------------- |
| control | 74.19             | 74.96             |
| off     | 74.60             | 70.60             |
| on      | —                 | 79.44             |

**Disabled overhead is not measurable.** The control-vs-off delta straddles
zero (+0.6% in rep 1, −5.8% in rep 2): run-to-run noise on this workload
exceeds the effect in both directions, which is what "no code emitted" should
look like. No claim stronger than "below noise" is supported by this data, and
none is made.

**The enabled arm costs roughly +7–13%** over the disabled arm on the same
workload. Most of that is the attributing global allocator, which intercepts
every allocation in the process; the counter increments themselves are single
uncontended relaxed `fetch_add`s. The enabled arm is a measurement
configuration, not a shipping one.

## Reproducing

```bash
# control
git worktree add /tmp/a4-base 839645e3e
# copy crates/verter_bench/examples/attribution_baseline.rs in, strip the
# `#[cfg(feature = "attribution")]` blocks and the global allocator
cargo run --manifest-path /tmp/a4-base/Cargo.toml -p verter_bench --release \
    --example attribution_baseline -- --files 40 --runs 7

# off
cargo run -p verter_bench --release --example attribution_baseline -- --files 40 --runs 7

# on
cargo run -p verter_bench --release --features attribution \
    --example attribution_baseline -- --files 40 --runs 7
```

Machine: darwin 25.6.0, arm64. Absolute numbers are machine-specific; the
arm-to-arm comparison is the result.
