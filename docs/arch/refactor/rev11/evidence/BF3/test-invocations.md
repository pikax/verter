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
| PublicApi / TSC / declaration under the TypeScript oracle | `cargo test -p verter_session --lib --features bf2-authoritative public_api_typescript_observation -- --test-threads=1` | `running 8 tests` → 7 passed, 1 ignored |
| IDE/TSX under the TypeScript oracle | `cargo test -p verter_session --lib --features bf2-authoritative ide_surface_typescript_observation -- --test-threads=1` | `running 3 tests` → 3 passed |
| Product/route inventory | `cargo test -p verter_session --lib framework_product_surface -- --test-threads=1` | `running 22 tests` → 20 passed, 2 ignored |
| Batch route | `cargo test -p verter_session --lib svelte_batch_route -- --test-threads=1` | `running 7 tests` → 6 passed, 1 ignored |
| Transport equivalence (NAPI / WASM / bundler) | `cargo test -p verter_session --lib --features transport-authoritative transport_route_equivalence -- --test-threads=1` | `running 11 tests` → 10 passed, 1 ignored (the BND-2 Rollup acceptance target) |
| TypeScript observation domain (harness) | `npx vitest --run --root packages/framework-conformance-harness test/typescript-observation-domain.spec.mjs` | 22 passed (22) |

The corrected public bundler measurements are green and run individually:

| target | invocation | expected current result |
|---|---|---|
| BND-1 public include contract | `cargo test -p verter_session --lib --features transport-authoritative the_bundler_public_entries_apply_their_documented_include_contract -- --test-threads=1 --nocapture` | `running 1 test` → passes: `VerterVue.vite({})` accepts `.vue`/rejects `.svelte`; `VerterSvelte.vite({})` accepts `.svelte`/rejects `.vue` |
| BND-2 Vite mapped product | `cargo test -p verter_session --lib --features transport-authoritative the_bundler_virtual_script_loads_publish_requested_source_maps -- --test-threads=1 --nocapture` | `running 1 test` → passes: `VerterVue.vite({}).load("/probe/Plug.vue?vue&type=script&lang.js")` and `VerterSvelte.vite({}).load("/probe/Plug.svelte?verter&type=script&lang.js")` both publish maps |

The obsolete blanket BND targets were removed: they measured the internal
Vue-pinned raw factory and the Vite wrapper-only `map` field, respectively.
BND-1 remains rejected and green. BND-2 now keeps the green Vite public-product
control and has a separate ignored target for the confirmed Rollup/non-Vite
inline map drop. The probe also fails closed unless its complete ignored `dist/`
tree matches the committed production-source/dist fingerprint produced by
`pnpm --filter @verter/unplugin build`.

The remaining correct-behavior targets are run individually and are EXPECTED TO
FAIL until their correction owner lands the product:

| target | invocation | expected current result |
|---|---|---|
| Rollup/non-Vite inline requested map | `cargo test -p verter_session --lib --features transport-authoritative the_bundler_rollup_inline_transform_preserves_requested_source_maps -- --ignored --test-threads=1 --nocapture` | `running 1 test` → fails at the final map-parity assertion with `hostHasMap=true`, `publicTransformIsInline=true`, `publicTransformMap=null`, and `publicTransformHasMap=false`; the earlier freshness, public-factory, include, inline-product, Svelte Rollup classification, and host-map assertions pass |
| Svelte untyped props surface | `cargo test -p verter_session --lib --features bf2-authoritative an_untyped_svelte_props_destructure_publishes_its_authored_props_to_typescript -- --ignored --test-threads=1 --nocapture` | `running 1 test` → fails because TypeScript observes `{}` rather than required `label` plus optional `disabled` |
| Standalone CSS requested maps | `cargo test -p verter_session --lib --features bf2-authoritative the_standalone_css_route_publishes_valid_requested_maps_for_passthrough_and_transformed_css -- --ignored --test-threads=1 --nocapture` | `running 1 test` → fails because the passthrough branch publishes no requested map |

The other ignored conformance, batch, and atomicity targets remain expected failures
and are run one filter per command. See [`dispositions.md`](dispositions.md).

## BND discrimination evidence

Starting revision for every reversible plant:
`4a659105fc3091f813b8b1d960004591684c0323`.

- Dist freshness: changed the committed expected dist SHA-256 by one nibble,
  then ran `node packages/unplugin/scripts/probe-bundler-route.mjs`. It exited
  `2` with `loaded:false`, `fresh:false`, and the expected/observed hashes.
  Restoring the nibble returned `loaded:true`, `fresh:true`.
- BND-1: temporarily changed only the `sveltePublicEntry` probe from
  `module_.VerterSvelte` to `module_.VerterVue`, then ran the named BND-1
  invocation above. It executed `running 1 test` and failed because the planted
  entry rejected its documented `.svelte` carrier. The scoped edit was restored.
- BND-2: temporarily forced the Svelte case's `loadedScriptHasMap` observation
  to `false`, then ran the named BND-2 invocation above. It executed
  `running 1 test` and failed at the mapped virtual-script product assertion.
  The edit was restored.
- Green control after every restore: the combined required invocation with
  filter `the_bundler` executed `running 3 tests`; all three passed and none was
  ignored at that revision.

The Rollup split starts from revision
`77a5429b73eda1746bd431592599033bc4bfb088`. The unmodified public entry itself
is the discriminator: the named ignored target executes `running 1 test`,
passes every setup and host-publication assertion, then fails only because the
inline public transform reports `map: null` / `publicTransformHasMap:false` while
the matching requested host product reports `hostHasMap:true`. The unignored
`the_bundler` control executes `running 4 tests`: the new target is ignored and
all three Vite/public-entry tests remain green.

## The same hazard in the other direction

A test can also run against the WRONG THING and pass. The client runtime smoke
mounted compiled modules through a bare `svelte/internal/client` specifier whose
resolution depended on where the scratch directory happened to sit; one directory
further up sits a different `svelte` version. On one tree it bound the pinned
runtime and passed; on another it bound the other copy and died inside the
official runtime. The executor now terminates that walk at its first step and
reports the runtime it bound, and the test pins it to the derived pinned version
— so a mount measured against anything else FAILS rather than deciding nothing.
