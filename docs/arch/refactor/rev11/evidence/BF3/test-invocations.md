# How to run this evidence suite — and how to tell a real green from a vacuous one

Several of this block's suites are feature-gated, and `cargo test`'s filter is a
plain substring. Both facts have already produced runs that **exited 0 while
executing nothing**. A reader comparing results must be able to tell those apart
from a genuine pass, so the vacuous invocations are recorded here by name.

**The rule: read `running N tests`, never the exit code.** A `running 0 tests`
line followed by `test result: ok` is a filter that matched nothing. It is not
evidence of anything.

That rule asks a reader to notice something, so the three suites below no longer
depend on the reader noticing it. Each of their filters also selects one census
test from `crates/verter_session/src/framework/suite_census.rs`, which re-execs
the test binary with `--list --format=terse`, requires the listing to contain the
census test itself, and counts the tests reported under its suite's module path.
A suite that is commented out, renamed, gated off, or emptied therefore FAILS its
own documented invocation instead of reporting `running 0 tests` / `test result:
ok` / exit 0. The census lives outside all three suites on purpose: a check
placed inside a suite is deleted by the same edit that empties it. This is why
each expected count below is one higher than the suite's own test count.

Living outside them left the reverse hole — one adjacent edit removing a suite's
`mod` line AND the census's own — so the registration is now MUTUAL and
compile-enforced, not conventional. `suite_census.rs` reads each suite's
`pub(crate) const CENSUS_WITNESS_PATH` instead of repeating the path as a string,
and each suite carries a `this_suite_is_registered_with_the_census` test that
calls the census's `covers()`. Measured, each with the edit applied and then
restored: removing `pub(crate) mod framework_product_surface_tests;`, `mod
svelte_batch_route_tests;`, or `mod transport_route_equivalence_tests;` fails to
build with `error[E0432]: unresolved import`, and removing `mod suite_census;`
fails to build with `error[E0433]: cannot find suite_census in super` (two errors
without the transport feature, three with it). Any ONE of the four is a compile
error rather than a vacuous green.

Each census row is bound to a WITNESS TEST, not merely to a location.
Direct-sibling validation proves only a location class, so emptying a suite and
repointing its constant at another direct sibling with tests of its own —
`framework::script_facts` (17 tests), `framework::registry` (10) — cleared the
floor on that sibling's tests. Each suite's constant is now
`concat!(module_path!(), "::this_suite_is_registered_with_the_census")`, the full
path of its OWN witness test; the census requires a test of exactly that path to
be present in the listing and DERIVES the counted module prefix from it, so a
retarget names a witness the target module does not define — unless that module
really does define a test of that name, which is the witness-decoy residue
recorded at the end of this file. Plants applied and
restored, each failing with *no test named `…::this_suite_is_registered_with_the_census`
exists in this binary*: retarget to `framework::script_facts`, retarget to
`framework::registry`, and emptying the suite with the constant left honest
(which removes the witness along with everything else). The floor remains live
and is separately shown: emptying every test EXCEPT the witness fails with *1
test(s) under `framework::svelte_batch_route_tests::`, below the recorded floor
of 8*.

The suite path each census test counts by is also no longer a string a suite can
choose: it is `module_path!()`-derived, the compiler's own answer, and
the census validates the paths before counting with them — pairwise
non-prefixing, neither a prefix of nor prefixed by the census's own module path,
and no census test may fall inside a suite's total. Each is a separately named
assertion. Those checks only relate the four paths to EACH OTHER, so one more
binds them to the census's position: every suite must be the census's DIRECT
SIBLING — same parent module, exactly one further non-empty segment. Without it
a suite retargeted at a disjoint module elsewhere in the crate is non-prefixing
with all of them and clears its floor on that module's tests. Plant applied and
restored: emptying the product suite AND pointing its constant at
`verter_session::lib_tests` (116 tests) now fails with ``verter_session::lib_tests
is not a child of verter_session::framework, the module this census lives in``.
Discrimination, plant applied and restored: emptying every `#[test]`
in the product suite with `#[cfg(any())]` AND widening its path constant to the
parent module made the documented invocation report `running 1 test` → ok →
exit 0 before this change; it now executes `running 1 test` and FAILS with
``framework:: is a prefix of the census's own module path
framework::suite_census::``. With the path left honest, the same emptying fails
at the floor instead (`0 test(s) under framework::framework_product_surface_tests::,
below the recorded floor of 23`).

Removing all four at once is NOT decidable from inside this binary — nothing that
compiles and runs there can observe its own absence — and no scanner is added to
pretend otherwise. That residual is the general execution-attestation problem
(the binary cannot attest to a universe it was never given), and it belongs to
whatever owns gate integrity, not to an in-binary check.

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
| Product/route inventory | `cargo test -p verter_session --lib framework_product_surface -- --test-threads=1` | `running 24 tests` → 22 passed, 2 ignored (23 suite tests plus the census test) |
| Batch route | `cargo test -p verter_session --lib svelte_batch_route -- --test-threads=1` | `running 9 tests` → 8 passed, 1 ignored (8 suite tests plus the census test) |
| Transport equivalence (NAPI / WASM / bundler) | `cargo test -p verter_session --lib --features transport-authoritative transport_route_equivalence -- --test-threads=1` | `running 16 tests` → 15 passed, 1 ignored (the BND-2 Rollup acceptance target); 15 suite tests plus the census test |
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
| Rollup/non-Vite inline requested map | `cargo test -p verter_session --lib --features transport-authoritative the_bundler_rollup_inline_transform_preserves_requested_source_maps -- --ignored --test-threads=1 --nocapture` | `running 1 test` → fails at the final map-parity assertion, which reads the ARTIFACT (`publicTransformMap`) and reports `no source-map artifact was published (the map itself is null)` alongside `hostHasMap=true`, `publicTransformIsInline=true`, `publicTransformHasMap=false`; the earlier freshness, public-factory, include, inline-product, Svelte Rollup classification, and host-map assertions pass |
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
- Export-to-observation tie: the probe's per-export case map is produced by one
  generic driver invoked once per enumerated export, with no per-export body, so
  a case cannot be cloned from a sibling's. Planted both ways and restored.
  Cloning the `VerterVue` case into the `unpluginFactory` slot — so that
  spelling is never called — failed
  `every_exported_bundler_spelling_is_executed_or_classified_out_of_scope`:
  *the case filed under this export was produced by reading `"VerterVue"`
  instead*. Cloning it AND relabelling the record so the name matches passed
  that test but failed
  `the_bundler_public_spellings_are_distinguished_by_what_they_accept`, because
  the probe measures each export's own SHAPE: *the probe read a
  `"unplugin-object"` where this spelling is a `raw-factory`*. The four drivable
  spellings are pairwise distinct on (kind, accepts `.vue`, accepts `.svelte`),
  and that distinctness is itself asserted before it is relied on.
- Probe-assigned labels removed: the probe records only what it READ off the
  value (`typeof`; an object's sorted own keys with each key's type; a
  callable's arity and name) plus what DRIVING it returned (the plugin object's
  own keys), and the consumer derives the classification. The two observations
  are cross-checked: `createUnplugin` is what flattens an adapter's Vite-only
  hooks onto the plugin it returns, so a wrapped entry's plugin carries a
  top-level `configResolved` and a raw factory's does not. Plant applied and
  restored — cloning `VerterVue`'s case into the `unpluginFactory` slot,
  relabelling `exportName` AND the whole evidence block (`valueType: "function"`,
  arity, name), with the driver rigged to throw if the real factory were ever
  driven (it completed, proving it was not) — failed BOTH guards: *read as a raw
  factory, but the plugin it returned carries a FLATTENED `configResolved`,
  which only `createUnplugin` produces*. **Floor, stated:** this proves the
  recorded evidence is value-derived and internally consistent; it does NOT
  prove the probe read honestly, since a probe can print any `typeof` it likes.
  Cross-checking catches a copied case because the copy carries the sibling's
  drive result; it cannot catch a probe that forges both. That is the trust
  floor of every out-of-process probe and is closed only by moving the
  observation in-process.
- Expectations stated by the test, not echoed from the record: the Vue map tie
  compared `sources` against the probe's own reported `id`, so forging BOTH
  passed. Carrier ids are now constants owned by the test, every case's reported
  `id`/`oppositeId` is ASSERTED equal to one of them before anything about that
  case is read, and the map tie compares against the constant. Plant applied and
  restored — retargeting the Vue map's `sources` AND the case's `id` together to
  `/forged/Other.vue` — now fails with *the case this test reads as
  `/probe/Plug.vue` reports a different requested carrier*.
- A test-owned second observation: the guards' two readings both came from the
  probe's own JSON. The Rust test now runs its own minimal observer — script
  text in the Rust source, executed with `node`, importing the same entry the
  probe proved fresh (no second fingerprint), reading SHAPE ONLY and driving
  nothing — and requires the probe's record to agree with it export by export.
  A missing or failing `node` FAILS; it never skips. Plants applied and
  restored: hiding `unpluginFactory` from the probe's enumeration fails with
  *the probe's export enumeration disagrees with this test's own reading of the
  same built entry*; misreporting `VerterSvelte` as a function fails with *the
  probe reports a "function" where this test reads a "object"*.

  **Open residue — invocation attribution.** The full-forgery plant — clone
  `VerterVue`'s case into the `unpluginFactory` slot, forge every probe-written
  field including `pluginKeys`, with the driver rigged to throw if the real
  factory were driven (it completed, proving it was not) — still PASSES both
  guards. Run and confirmed, not assumed.

  The reason is narrow, and an earlier revision of this note stated it wrongly.
  The two spellings are NOT indistinguishable out of process: `typeof` (object
  vs function), `.vite` callability (function vs undefined) and plugin-key
  flattening (`configResolved` / `handleHotUpdate` appear only on the
  `createUnplugin` wrapper) all separate them, and the guards already use the
  first two. What is missing is only that nothing REQUIRES a driven export to
  have been APPLIED: a probe can print an export's TRUE readings while sourcing
  the drive results from its sibling, and each individual statement is then true
  of the real value. The readings themselves are cross-checked against a
  test-owned re-read, so a hidden or mistyped export is caught; only the
  attribution of an invocation is not.

  Nor does closing it need the driving moved in-process — a second wrong claim
  in that earlier revision. Invocation is observable from outside: wrapping the
  built entry's bindings at import (a module hook / `--import` wrapper
  installing an apply-counting `Proxy`) attributes an apply to a spelling, and a
  `Proxy` measurement counted 0 applies from `VerterVue.vite({})` against 1 from
  calling the wrapped factory. The named closure is to require a non-zero apply
  count per driven export. It is not built.
- **Open residue — witness decoy.** A census row is bound to "a test of this
  path exists in the listing", not to the module that DECLARED the constant.
  Measured, plant applied and restored: adding a real `#[test] fn
  this_suite_is_registered_with_the_census` directly in `framework::registry`,
  emptying `svelte_batch_route_tests`, and pointing its constant at that decoy
  path makes the documented invocation report `running 1 test` → ok. The bar
  this raises is still real — an attacker must now ADD a genuine same-named test
  to a sibling module rather than edit one string, and the sibling, pairwise,
  census-overlap and floor checks all still apply — but the row is bound to a
  free `&str`. Closing it means binding the row to the declaring module's own
  identity rather than to a path it hands over. Not built.
- Whole-artifact map parity: the parity comparisons compare the ENTIRE
  normalized map — `version`, `file`, `sourceRoot`, `sources`, every
  `sourcesContent` VALUE, `names`, `mappings` — normalizing only key order and
  absent-vs-`null`, both non-semantic. A subset comparison accepted forged
  `sourcesContent`, which is the text a debugger shows the user as the authored
  source. Plant applied and restored: flipping ONE byte of the Svelte map's
  `sourcesContent[0]` failed the green Vite parity target AND the ignored Rollup
  target, both with *the published virtual-script map is not the map the host
  published for the same requested profile*.
- Map tied to its request: the Vue Vite virtual-script product has no host
  counterpart, so its structural oracle is its whole acceptance and says nothing
  about WHAT is mapped. Every `sources` entry must now name the requested
  carrier. Plant applied and restored: retargeting that map's `sources` at
  `/elsewhere/Unrelated.vue` failed the green target — *the map published for
  `/probe/Plug.vue` names 2 source(s) that are not that carrier*.
- Empty-but-valid map: where the envelope oracle carries the whole acceptance —
  the Vue Vite virtual-script product, which has no host counterpart, and the
  Rollup inline product — it now also requires at least one segment naming an
  authored position. Forcing the Vue product's `mappings` to the single unmapped
  segment `"A"` while leaving a valid `version: 3` envelope failed the green
  Vite target: *the published source map is a valid envelope that maps NOTHING
  — 1 segment(s), none naming an authored position*. The plant was restored. The
  public SVELTE virtual-script product is deliberately exempt: its map is that
  single unmapped segment today, which is the recorded observation in
  [`dispositions.md`](dispositions.md), and its acceptance is host parity rather
  than the envelope oracle.

The Rollup split starts from revision
`77a5429b73eda1746bd431592599033bc4bfb088`. The unmodified public entry itself
is the discriminator: the named ignored target executes `running 1 test`,
passes every setup and host-publication assertion, then fails only because the
inline public transform publishes NO map artifact while the matching requested
host product reports `hostHasMap:true`. At that revision the unignored
`the_bundler` control executed `running 4 tests`: the new target was ignored and
all three Vite/public-entry tests were green. On the current tree that same
filter executes `running 5 tests` → 4 passed, 1 ignored, because the empty-map
characterization was added beside them.

## The same hazard in the other direction

A test can also run against the WRONG THING and pass. The client runtime smoke
mounted compiled modules through a bare `svelte/internal/client` specifier whose
resolution depended on where the scratch directory happened to sit; one directory
further up sits a different `svelte` version. On one tree it bound the pinned
runtime and passed; on another it bound the other copy and died inside the
official runtime. The executor now terminates that walk at its first step and
reports the runtime it bound, and the test pins it to the derived pinned version
— so a mount measured against anything else FAILS rather than deciding nothing.
