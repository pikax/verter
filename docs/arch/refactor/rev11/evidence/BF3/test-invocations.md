# How to run this evidence suite — and how to tell a real green from a vacuous one

Several of this block's suites are feature-gated, and `cargo test`'s filter is a
plain substring. Both facts have already produced runs that **exited 0 while
executing nothing**. A reader comparing results must be able to tell those apart
from a genuine pass, so the vacuous invocations are recorded here by name.

**The rule: read `running N tests`, never the exit code.** A `running 0 tests`
line followed by `test result: ok` is a filter that matched nothing. It is not
evidence of anything.

## Invocations that matched ZERO tests and still exited 0

| invocation | why it matched nothing |
|---|---|
| `cargo test -p verter_session --lib "the_tsc_product\|the_diagnostics_route"` | libtest has **no alternation syntax**. The whole string `the_tsc_product\|the_diagnostics_route` is treated as ONE literal substring, and no test name contains it. Run one filter per command. |
| `cargo test -p verter_session --lib svelte_official_conformance` | the module is `#[cfg(feature = "bf2-authoritative")]` (`crates/verter_session/src/compile/map_equality_tests.rs`), so without the feature it is not compiled in |
| `cargo test -p verter_session --lib public_api_typescript_observation` | same gate |
| `cargo test -p verter_session --lib ide_surface_typescript_observation` | same gate |

Each was re-run correctly; the correct forms are below.

## The correct invocation for every suite

All builds bounded (`CARGO_BUILD_JOBS=4`). Never `node scripts/gate.mjs`, never a
bare workspace `cargo build` / `test` / `nextest`.

| suite | invocation | expected |
|---|---|---|
| Svelte official-conformance gate | `cargo test -p verter_session --lib --features bf2-authoritative svelte_official_conformance -- --test-threads=1` | `running 19 tests` → 16 passed, 3 ignored |
| PublicApi / TSC / declaration under the TypeScript oracle | `cargo test -p verter_session --lib --features bf2-authoritative public_api_typescript_observation -- --test-threads=1` | `running 7 tests` → 7 passed |
| IDE/TSX under the TypeScript oracle | `cargo test -p verter_session --lib --features bf2-authoritative ide_surface_typescript_observation -- --test-threads=1` | `running 3 tests` → 3 passed |
| Product/route inventory | `cargo test -p verter_session --lib framework_product_surface -- --test-threads=1` | `running 21 tests` → 20 passed, 1 ignored |
| Batch route | `cargo test -p verter_session --lib svelte_batch_route -- --test-threads=1` | `running 7 tests` → 6 passed, 1 ignored |
| Transport equivalence (NAPI / WASM / bundler) | `cargo test -p verter_session --lib --features transport-authoritative transport_route_equivalence -- --test-threads=1` | `running 8 tests` → 8 passed |
| TypeScript observation domain (harness) | `npx vitest --run --root packages/framework-conformance-harness test/typescript-observation-domain.spec.mjs` | 20 passed (20) |

The three `#[ignore]`d conformance targets and the two batch/atomicity targets are
run with `-- --ignored` and are EXPECTED TO FAIL — each states behaviour the
compiler does not yet meet. See [`dispositions.md`](dispositions.md).

## The same hazard in the other direction

A test can also run against the WRONG THING and pass. The client runtime smoke
mounted compiled modules through a bare `svelte/internal/client` specifier whose
resolution depended on where the scratch directory happened to sit; one directory
further up sits a different `svelte` version. On one tree it bound the pinned
runtime and passed; on another it bound the other copy and died inside the
official runtime. The executor now terminates that walk at its first step and
reports the runtime it bound, and the test pins it to the derived pinned version
— so a mount measured against anything else FAILS rather than deciding nothing.
