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
`mod` line AND the census's own — so the registration is MUTUAL and
compile-enforced, not conventional. `suite_census.rs` NAMES each suite's witness
test as an ITEM instead of repeating its path as a string, and each suite carries
a `this_suite_is_registered_with_the_census` test that calls the census's
`covers()`. Measured with the transport feature on, each edit applied and then
restored: removing `pub(crate) mod framework_product_surface_tests;`, `mod
svelte_batch_route_tests;`, or `mod transport_route_equivalence_tests;` fails to
build with `error[E0433]: cannot find <suite> in super`, pointed at the line of
`suite_census.rs` that names that suite's witness; removing `mod suite_census;`
fails to build with seven `error[E0433]` sites across FOUR modules — all three
suites plus `script_facts_tests.rs`, the outside anchor described below. Any ONE
of the four is a compile error rather than a vacuous green.

Each census row is bound to a WITNESS TEST by the COMPILER's identity for it, not
by a string the suite hands over. Direct-sibling validation proves only a
location class, so emptying a suite and repointing it at another direct sibling
with tests of its own — `framework::script_facts` (18 tests),
`framework::registry` (10) — cleared the floor on that sibling's tests. A row is
now a reference to the witness FUNCTION ITEM
(`witness_identity(&super::<suite>::this_suite_is_registered_with_the_census)`),
and the path comes back from `std::any::type_name` on that item's zero-sized
function-item type. Measured in this crate, the three rows answer
`verter_session::framework::{framework_product_surface_tests,
svelte_batch_route_tests, transport_route_equivalence_tests}::this_suite_is_registered_with_the_census`
— the same spelling `module_path!()` produces, so the existing crate-segment
stripping and prefix derivation are unchanged. Measured in the same probe: the
reference must NOT be coerced to a `fn()` pointer, which erases every definition
to the string `fn()`; the helper therefore takes `&F`. The census still requires a
test of exactly that path to be present in the listing and still DERIVES the
counted module prefix from it, so a row can only ever count a module that really
declares the item `suite_census.rs` names.

What that changes, measured. The decoy plant recorded at the end of this file —
add a genuine `#[test] fn this_suite_is_registered_with_the_census` at the
`framework::registry` module root, empty `svelte_batch_route_tests`, point the
batch row at the decoy — now fails to BUILD at the second step: emptying the
suite removes the item the census names, `error[E0425]: cannot find value
this_suite_is_registered_with_the_census in module super::svelte_batch_route_tests`
(rustc's own `help:` offers `use crate::framework::registry::…`, which is exactly
the edit a retarget now requires). Plant applied and restored.

**The residual bar, stated exactly.** The remaining retarget requires editing
`suite_census.rs` ITSELF. Measured, plant applied and restored: with the registry
decoy in place, the batch suite emptied, AND the batch row inside
`suite_census.rs` repointed at
`super::registry::this_suite_is_registered_with_the_census`, the documented
invocation reports `running 1 test` → ok. That is a strictly narrower bar than
before — a free `&str` in any suite no longer suffices, the retarget is confined
to the one file whose whole subject is the census, and the sibling, pairwise,
census-overlap and floor checks still apply to whatever it names — but it is not
closed, and nothing here pretends it is.

The floor is separately shown to still be live: disabling every `#[test]` in the
batch suite EXCEPT the witness (7 × `#[cfg(any())]`, plant applied and restored)
failed with *1 test(s) under `framework::svelte_batch_route_tests::`, below the
recorded floor of 8* — 8 being the floor recorded at the time of that
measurement. The batch suite has since grown to 10 tests and its floor with it,
and the current floor is proven BOUNDARY-EXACT: at 10 the census passes, and a
plant raising it to 11 — proven present-and-new before the run, and the file
restored to a byte-identical SHA-256 afterwards — fails with *svelte_batch_route:
this binary carries 10 test(s) under `framework::svelte_batch_route_tests::`,
below the recorded floor of 11*.

The suite path each census test counts by is the compiler's own answer rather
than a string a suite can choose, and the census validates the paths before
counting with them — pairwise non-prefixing, neither a prefix of nor prefixed by
the census's own module path, and no census test may fall inside a suite's total.
Each is a separately named assertion. Those checks only relate the paths to EACH
OTHER, so one more binds them to the census's position: every suite must be the
census's DIRECT SIBLING — same parent module, exactly one further non-empty
segment. Without it a row aimed at a disjoint module elsewhere in the crate is
non-prefixing with all of them and clears its floor on that module's tests.
Re-measured after the change, plant applied and restored: pointing the batch row
at a genuine same-named witness planted in `framework::script_facts`'s test
module fails with ``verter_session::framework::script_facts::tests … is not a
DIRECT sibling of the census — a suite identity is exactly one segment beyond the
shared parent``, and the batch suite's own registration test fails alongside it
because `covers()` no longer carries it. (The earlier
`verter_session::lib_tests` and parent-module-widening plants were expressed by
editing a suite's own path constant. That constant no longer exists; the
assertions they exercised are the ones re-measured here.)

Removing all four `mod` declarations in ONE edit used to be undecidable from
inside this binary, because every party to the mutual argument went with them. It
is now a BUILD error, because the anchor sits OUTSIDE the set:
`framework::script_facts`'s own test module — 18 tests, none of them this
evidence suite's — carries `no_suite_census_row_counts_this_module`, which
asserts through `suite_census::counts_tests_in` that no census row's counting
prefix covers it. Measured, plants applied and restored: removing all four `mod`
lines fails with `error[E0433]: cannot find suite_census in framework` pointed at
`script_facts_tests.rs:32`; removing all four AND that anchor test compiles
cleanly, and `cargo test -p verter_session --lib svelte_batch_route` then reports
`running 0 tests` → `test result: ok` → exit 0, which is the vacuous green the
anchor now costs. The anchor is load-bearing rather than a `use` for its own
sake: with a census row pointed at that module it FAILS, *`verter_session::framework::script_facts::tests`
is counted by a suite census row* (the same plant as the sibling measurement
above).

The general execution-attestation problem is NOT closed by this. A binary still
cannot attest to a universe it was never given, and a landed guard in this
repository must be structural rather than a name-keyed source-tree scanner, so no
out-of-process "diff the `mod` lines against `--list`" check is added to pretend
otherwise. That residual is recorded as row **GI-21** in
[`gate-integrity-ledger.md`](../../../../gate-integrity-ledger.md) and is owned
by the gate-integrity block.

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
| Svelte official-conformance gate | `cargo test -p verter_session --lib --features bf2-authoritative svelte_official_conformance -- --test-threads=1` | `running 20 tests` → 17 passed, 3 ignored |
| PublicApi / TSC / declaration under the TypeScript oracle | `cargo test -p verter_session --lib --features bf2-authoritative public_api_typescript_observation -- --test-threads=1` | `running 8 tests` → 7 passed, 1 ignored |
| IDE/TSX under the TypeScript oracle | `cargo test -p verter_session --lib --features bf2-authoritative ide_surface_typescript_observation -- --test-threads=1` | `running 3 tests` → 3 passed |
| Product/route inventory | `cargo test -p verter_session --lib framework_product_surface -- --test-threads=1` | `running 24 tests` → 22 passed, 2 ignored (23 suite tests plus the census test) |
| Batch route | `cargo test -p verter_session --lib svelte_batch_route -- --test-threads=1` | `running 12 tests` → 10 passed, 2 ignored (11 suite tests plus the census test) |
| Transport equivalence (NAPI / WASM / bundler) | `cargo test -p verter_session --lib --features transport-authoritative transport_route_equivalence -- --test-threads=1` | `running 23 tests` → 21 passed, 2 ignored (the BND-2 Rollup acceptance target and the TR-1 missing-node parity target); 22 suite tests plus the census test |
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
| TR-1 missing-node transport parity | `cargo test -p verter_session --lib --features transport-authoritative the_transports_report_a_missing_node_the_same_way -- --ignored --test-threads=1 --nocapture` | `running 1 test` → fails at the parity assertion: `the transports still spell a missing node differently: napi {"outcome":"missing"}, wasm {"message":"HostError::MissingVirtualNode: /probe/Server.svelte","outcome":"error"}`; the earlier staleness guard and the two no-product assertions pass |

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
- Opposite-carrier pin, no longer skippable. The rejection half of each pinned
  entry's include contract is taken against the case's reported `oppositeId`, so
  a `null` there once left that assertion describing nothing while the test
  stayed green. It is now REQUIRED and equal to the test's own carrier constant.
  Plant applied and restored: forcing `oppositeId: null` on both public Vite
  cases fails the include-contract target with *the case's opposite-carrier
  decision was taken against a carrier this test did not ask about (expected
  `/probe/Plug.svelte`)*.
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

  **Invocation attribution — CLOSED.** The residue was that nothing required a
  driven export to have been APPLIED: the probe could print an export's TRUE
  readings while sourcing its drive results from a sibling, and every individual
  statement stayed true of the real value. The test-owned observation no longer
  reads shape only. Per enumerated export it now wraps the callable it is about
  to invoke — an unplugin object's `.vite`, or a raw factory itself — in an
  apply-counting `Proxy`, invokes it the same way the probe does, and records
  the apply count, the plugin THAT invocation returned, and that plugin's
  `transformInclude` answer for both carriers. It also evaluates
  `default === VerterVue` itself and compares the previously-gathered-but-unused
  `rollup` callability. A spelling the test classifies as executed must carry a
  non-zero apply count from this side; its reported `pluginKeys`, per-carrier
  include decisions, alias identity and default-alias identity must equal the
  ones this side produced. Both halves of the `(kind, accepts_vue,
  accepts_svelte)` triple the contract rows are matched on are therefore
  measured by the test.

  Measured, three plants, each proven present-and-new before the run and each
  restored to a byte-identical file (SHA-256 compared):

  - The recorded FULL FORGERY — skip `driveExport("unpluginFactory")` and file a
    hand-written case carrying the raw factory's TRUE evidence and a
    raw-factory-plausible `pluginKeys` (no flattened `configResolved`), with the
    carrier results taken from the wrapped sibling — PASSED the pre-change tree
    (`running 16 tests` → 15 passed, measured with the Rust file reverted and
    the plant still applied) and now FAILS both bundler partition guards at
    *the plugin the PROBE reports driving is not the plugin this test's own
    invocation of the same spelling returned*. That assertion is the one that
    catches it, and it fires before `assert_evidence_matches_the_driven_plugin`,
    which the forged keys were built to satisfy.
  - The literal clone — the same skip, filing a copy of `VerterVue`'s case
    (its flattened `pluginKeys` included) with the evidence relabelled as the
    raw factory's — fails at the same assertion, naming both key lists.
  - Flipping the reported `svelte` include decision for `unpluginFactory` fails
    the partition guard at *the svelte include decision the probe reports
    differs from the one this test's own invocation of the same spelling
    produced*. Pre-change that test only required the value to be a boolean.

  The apply count is proven to FAIL rather than skip: making the observation's
  own drive branch unreachable for `VerterVue` fails with *this test classifies
  the spelling as executed, but this test's own apply-counting invocation of it
  ran 0 time(s)*. Plant applied and restored.

  **What remains.** The observation drives the FACTORY and the include decision,
  never a carrier transform, so the per-carrier PRODUCT bytes in the probe's
  record are still the probe's word. Where they carry weight they are judged
  anyway — the Svelte products against the host route, and the raw factory's
  against the wrapped entry's, which the suite requires to be equal because one
  wraps the other. A forged case for `unpluginFactory` can therefore now consist
  only of statements this test independently established true, or of a product
  the suite separately requires to equal its sibling's; it can no longer carry a
  wrong result.
- **Witness decoy — NARROWED, and the remainder named.** A census row used to be
  bound to "a test of this path exists in the listing", where the path was a
  `&str` the suite handed over; the recorded plant (a genuine `#[test] fn
  this_suite_is_registered_with_the_census` at the `framework::registry` module
  root, `svelte_batch_route_tests` emptied, its constant repointed at that decoy)
  reported `running 1 test` → ok. A row is now a REFERENCE to the witness
  function ITEM, and the path is `std::any::type_name`'s answer for that item, so
  the same plant fails to BUILD: `error[E0425]: cannot find value
  this_suite_is_registered_with_the_census in module
  super::svelte_batch_route_tests`. What REMAINS is narrower and is stated in
  full above: the retarget now has to be made in `suite_census.rs` itself, and
  measured with that edit applied — the registry decoy, the emptied suite, and
  the batch row repointed at `super::registry::…` — the invocation again reports
  `running 1 test` → ok. Both plants applied and restored. Closing that remainder
  is not an in-binary problem: it is a file whose entire subject is the census
  being edited, which the review of `suite_census.rs` owns. No further mechanism
  is claimed.
- **Open residue — whole-set removal / execution attestation.** Removing every
  `mod` declaration this census depends on used to be a vacuous green; it is now
  a build error, raised from `framework::script_facts`'s test module, which sits
  outside the set (measured above, both directions). The GENERAL problem is
  untouched: a test binary cannot attest to a universe it was never given, and a
  landed guard here must be structural rather than a name-keyed source-tree
  scanner, so no out-of-process `mod`-line scanner is added. Owned by the
  gate-integrity block as row **GI-21** in
  [`gate-integrity-ledger.md`](../../../../gate-integrity-ledger.md); the
  acceptance bar there is tree-derived inventory compared against libtest
  discovery, exercised through the canonical entry point.
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
filter executes `running 9 tests` → 8 passed, 1 ignored, because the empty-map
characterization and the four bundler product lanes below were added beside
them.

## The bundler product lanes, driven

Five bundler lanes were recorded in
`crates/verter_session/src/framework/framework_product_surface_inventory.json`
as read-verified citations. Each is an ALIAS of a host route already proven
live, so each is now DRIVEN and held to route identity plus publication — the
bundler's product must BE the in-process host's product for the same typed
request — rather than to a semantic case of its own. Every plant below was
proven present-and-new before its run (the marker occurs zero times in the
pristine bytes, exactly once afterwards, and the bytes changed) and every file
was restored to its pre-plant SHA-256.

The profiles the comparisons are made against are RESTATED in the test, never
read out of the probe's record, including the eight-hex-digit component id the
bundler derives (SHA-256 of the root-relative carrier path, plus the source in a
production profile). A wrong restatement fails the comparison; it cannot make it
vacuous. `NODE_ENV` is pinned to `development` for the probe spawn, because the
non-Vite lanes read it to select production codegen and a production profile
changes both the emitted module and the id it is scoped by.

- **Style lane** (`packages/unplugin/src/index.ts:68`) and the **virtual-node
  inventory listing** beside it (`:63`), reached through the Svelte carrier
  transform, driven by
  `the_bundler_style_lane_publishes_the_hosts_style_products_and_none_without_a_style_block`.
  The probe parses the published wrapper's `?verter&type=style` import lines and
  `load()`s each; the test pins exactly one request at index 0 with `lang.css`,
  compares it against the host's `Style{0}` node through the suite's own
  `assert_case_matches_host`, and additionally compares the WHOLE map artifact
  against the host's. The count rests on a NEGATIVE CONTROL asserted first: the
  Vue carrier this suite drives has no `<style>` block, so its wrapper must
  publish zero style requests. Plants applied and restored — appending a phantom
  style request to the Vue wrapper's list fails with *the Vue carrier this suite
  drives has no `<style>` block, but its wrapper published 1 style request(s)*;
  rewriting one byte of the loaded style code fails the parity comparison; and
  flipping one byte of that product's map `sourcesContent` fails with *the
  bundler route published a different map than the host published for the same
  requested profile*.
- **Load lane** (`:668`), driven by
  `the_bundler_load_lane_serves_the_hosts_node_and_nothing_for_an_unregistered_carrier`.
  Every request the wrapper points at is answered from a cache the carrier
  transform filled; a `?vue&type=template` request is never cached, so answering
  it means the lane fell through to the host. The product is compared against
  the host's `Template` node, code and whole map. NEGATIVE CONTROL: the same
  request for `/probe/NotRegistered.vue`, which must publish nothing. Plants
  applied and restored — serving the registered carrier's product under the
  unregistered request's label fails with *the load lane published a product for
  a carrier it never transformed*; substituting a plausible render function for
  the loaded bytes fails the parity comparison.
- **Runtime-render batch lane** (`:101`), driven by
  `the_bundler_inline_transform_publishes_the_hosts_runtime_render_batch_product`.
  The non-Vite inline transform's CODE (previously captured only as a boolean)
  is compared against `compile_many` on the `RuntimeRender` lane for the same
  canonical and an equivalent profile. **What the identity proves, stated
  exactly:** that lane is documented to publish the same `Main` bytes as the
  host-backed lane through the same substrate, and the test MEASURES that
  equality rather than assuming it — so byte identity with the render lane does
  not by itself discriminate WHICH host lane produced the bundler's bytes. It
  proves the bundler publishes the host's runtime-render product rather than
  something of its own, and a divergence between the two host lanes turns the
  measurement red and makes the test able to tell them apart. Plant applied and
  restored: renaming the component binding in the published inline product fails
  with *the published inline product is not the host's render-only batch product
  for the same canonical and profile*.
- **Non-Vite CSS scoping lane** (`:863`) and the **`processStyle` wrapper**
  beneath it (`packages/unplugin/src/core/compiler.ts:64`), driven by
  `the_non_vite_style_lane_scopes_through_the_shared_css_processor`. A
  Rollup-shaped plugin (no resolved Vite config) is driven twice: the carrier
  transform, which caches the profile, then the style sub-request. The request
  carries `&scoped` deliberately — an UNSCOPED request returns its input
  byte-for-byte, so an unscoped product is indistinguishable from a lane that
  never ran — and the CSS carries a `v-bind()` payload, whose rewrite names the
  component id of the cached profile the lane read, so the product identifies
  WHICH carrier's profile produced it. The include gate is also pinned: with no
  Vite config the lane requires a non-`css` lang, hence `lang.scss`. The product
  is compared against `verter_compiler::css::process_style` on the same bytes
  and scope id, and the published map is asserted `null`. Plants applied and
  restored — driving the UNSCOPED request while still recording the scoped id
  fails with *published CSS with no `[data-v-82f3abaf]` scoping attribute*
  (and shows the `v-bind()` rewrite alone survives an unscoped request, which is
  why the scoping assertion is separate); echoing the scoped product as the
  negative control fails with *transformed CSS for a carrier it never
  transformed*.
- **Recompile lane** (`:803`) — DRIVEN, by
  `the_bundler_pre_compile_lane_publishes_the_hosts_products_for_a_real_project`
  and `the_bundler_cross_file_recompile_write_is_attributed_to_the_recompile_call`.
  The probe writes a two-file project INSIDE the repository (a parent passing a
  literal prop to a child, the shape the cross-file pass records constness for)
  into a directory it allocates PER INVOCATION with `mkdtemp` under the stable,
  ignored parent `.verter-probe-fixtures/`, and removes only that directory in a
  `finally`. It then builds
  `VerterVue.vite({ preCompile: true, crossFileOptimize: true })`, resolves a
  production config rooted at the fixture, and runs `buildStart()`. What the
  hook published is read back through the plugin's own `load` hook and compared
  against the host's `Main` nodes for that production profile, per file. Both
  sources are pinned to the test's own values, as is the fixture's stable parent,
  its leaf prefix, and that the leaf is a single directory level — the random
  suffix is the only part that cannot be pinned, and pinning a fixed path instead
  would make two concurrent probes delete each other's files. Plants applied and
  restored — building the plugin with `preCompile: false` fails with *the host
  route published a product but the bundler route returned … "outcome":"missing"*;
  substituting `export default {}` for the child's published module fails the
  parity comparison.

  **What the first test claims, exactly.** Two things: `buildStart` completed
  over a real two-file project on disk, and both modules it published are
  byte-identical to the in-process host's products for the same profile. It
  asserts NOTHING about the cross-file recompile block — not that it was
  entered, and not that it iterated. An earlier version of this test called
  `compute_cross_file_optimizations()` on a FRESH IN-PROCESS host and read the
  non-empty result as evidence that the plugin's block iterated. That inference
  does not hold: it is a different host in a different process. The claim was
  dropped rather than weakened, and the measurement it rested on with it.

  Its products cannot separate the two writes, and it does not pretend to: the
  cross-file result never reaches codegen —
  `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs` passes
  `prop_constness_overrides: None` — so a recompiled module is byte-identical to
  the pre-compiled one.

  **How the write IS attributed, against the shipped artifact.** An earlier
  record named a `session_metrics`-enabled native build as the closure
  condition, on the reading that the host metrics channel was the only thing
  that could count the call. That is false: the metrics channel is *one* way to
  count it, not the only one. An observation of `host.getVirtualFile` taken
  WHILE `buildStart` runs is what names the recompile call — with one precision
  a review seat was right to insist on. The hook reaches that call at TWO
  places, not one: the recompile block itself (`:803`), and the compiled-style
  read the SVELTE pre-compile branch performs (`readCompiledStyleArtifacts`,
  called at `:785`, calling `getVirtualFile` at `:68`). The plugin's other two
  call sites — the load lane at `:668` and the transform at `:1041` — are not
  reachable from `buildStart`.

  Three things keep that from weakening the attribution, and none of them is
  prose. This lane's fixture is TWO `.vue` files, so the Svelte branch cannot
  fire at all. The two reads are distinguishable in the record regardless: the
  style read asks for a `?verter&type=style&index=…` request while the recompile
  asks for a BARE canonical, and the test asserts EQUALITY against the bare child
  canonical, so a style read fails that assertion rather than passing as a
  recompile. And reading 2 below turns the cross-file flag off on the same
  fixture and observes ZERO — which ties the observation to the cross-file block
  specifically, not merely to "something in `buildStart`".

  `the_bundler_cross_file_recompile_write_is_attributed_to_the_recompile_call`
  takes that observation at the NATIVE MODULE BOUNDARY: the probe resolves the
  same `@verter/native` the plugin's own `createRequire(dist/index.mjs)`
  resolves (recorded as `nativeEntry` and pinned by the test to this
  repository's `packages/native/index.js`) and wraps
  `VerterHost.prototype.getVirtualFile` so the call delegates and hands back the
  real value. The plugin is not modified, the lane under observation is the
  shipped code path, the wrapper is installed only around this lane group and
  removed afterwards, and the observation is armed only for the duration of
  `buildStart` — so a read taken by the later `load` calls cannot be recorded as
  a recompile.

  Three readings, each needing the other two:

  1. **The call.** The ordinary lane observes exactly one read during
     `buildStart`, and its `rawId` is the CHILD — the file whose constness hints
     the cross-file pass changed.
  2. **The negative control** (`vueRecompileLaneWithoutCrossFile`). The same
     drive with `crossFileOptimize` off observes NONE, while still publishing
     both modules and matching the host's products for both. Zero is therefore
     an absent recompile rather than an absent lane, and the observation channel
     is not a constant.
  3. **The write** (`vueRecompileWriteAttribution`). In a third drive the
     boundary substitutes, for that one call's return only, the real value with
     `\n/* verter-probe: recompile-return */\n` appended. The published child
     module is then asserted EQUAL to the host's own product followed by exactly
     those bytes — an equality, never a search for the marker inside generated
     output — with its map still equal to the host's, while the PARENT publishes
     the host's product unchanged. The value the recompile call returned is
     therefore what the route cached and served, which is the write.

  **Discrimination.** Each reading was proven discriminating by a plant in the
  PLUGIN SOURCE — the code under test, never the test — each proven present and
  unique in the working tree and absent from `HEAD` before its run, rebuilt
  through `pnpm --filter @verter/unplugin build` with the committed freshness
  record regenerated, then restored and rebuilt back to a freshness record
  byte-identical to the committed one (the build is deterministic; the restored
  digests reproduce the committed pair exactly).

  | plant | mutation | result |
  |---|---|---|
  | `PLANT_P1_NO_RECOMPILE_CALL` | replace the `getVirtualFile` call with a literal | reading 1 RED — `buildStartVirtualFileCalls` is `[]`; the built `dist` drops from 5 `getVirtualFile` occurrences to 4, and the child publishes `""` |
  | `PLANT_P2_DROP_RECOMPILE_WRITE` | keep the call, drop the `scriptCache.set` | reading 3 RED — the published child is the host product WITHOUT the marker, while reading 1 still passes; this is what separates "the write happened" from "the call happened" |
  | `PLANT_P3_IGNORE_CROSSFILE_FLAG` | enter the block regardless of `opts.crossFileOptimize` | reading 2 RED — the control observes one read with `crossFileOptimize: false` |

## The batch route's per-entry atomicity table, driven

`a_genuinely_failing_batch_entry_publishes_no_partial_product`
(`crates/verter_session/src/framework/svelte_batch_route_tests.rs`) is a
table-driven regression over every failing-entry class the public `compile_many`
API can genuinely reach, on both lanes where the class exists on that lane. It is
reached by the batch-route invocation above; the whole suite now executes
`running 12 tests` → 10 passed, 2 ignored.

The second ignored target is the amended AT-2 row's own artifact,
`the_host_backed_success_construction_is_never_fed_a_response_that_carries_an_error`
(`cargo test -p verter_session --lib the_host_backed_success_construction_is_never_fed_a_response_that_carries_an_error -- --ignored --test-threads=1` → `running 1 test` → PASSES).
It is `#[ignore]`d under the maintainer standing ruling of 2026-08-17 and is
deliberately not a required-RED target: it characterizes a latent construction
hazard whose reachability is unproven, and it turns RED the day a successful
host-backed response carries an error-severity diagnostic. See
[`dispositions.md`](dispositions.md) and
[`maintainer-standing-ruling-bugs-and-types.md`](maintainer-standing-ruling-bugs-and-types.md).

For each row the entry is FIRST proven to have entered the class the row names —
never merely that `errors` is non-empty, which would let a row pass on a failure
from a class it is not measuring — and only then asked whether it published code,
a source map, or an output language. Every row also carries an entry that must
publish cleanly, so "no product beside a failure" cannot be satisfied by a route
that withholds every product.

| class | lanes | what proves the entry entered it |
|---|---|---|
| duplicate-canonical conflict | RuntimeRender, HostBacked | the single error is exactly the batch's per-canonical conflict message, at BOTH original input positions |
| compile failure (`Err(CompileError)`) | RuntimeRender, HostBacked | every message is prefixed with the canonical id — the discriminator against the successful-response construction, whose diagnostics ride verbatim beside a published product — and at least one is the template-parse diagnostic |
| other typed host error (`Err(other)`) | RuntimeRender | the single error is the typed grammar-mismatch host error, and the SAME inputs under the lane's ordinary profile publish, so the failure is the axis and not the input |
| caught panic | RuntimeRender, HostBacked | the single error carries the coordinator's panic rendering and the injected panic body |

Recorded NOT REACHABLE through this API, with the source reason, and deliberately
NOT represented by an `#[ignore]`d target (such a target would fail for a reason
other than the property it names):

- **Upsert failure.** It folds into the same per-canonical error map as the
  conflict, but the upsert engine only errors from a scheduler `Failed` /
  `Superseded` / `Shutdown` completion state or a post-commit generation-fence
  mismatch (`crates/verter_session/src/host_upsert.rs`, `map_states` /
  `finish_upsert_post_commit`). `compile_many` exposes no input that produces any
  of them — it deduplicates by canonical before submitting — and the only in-tree
  driver of those states is a test-only completion-state seam that bypasses the
  batch entirely. Both constructions the class would reach hardcode an empty
  code, map and language, exactly like the conflict row.
- **A typed Svelte runtime refusal.** `crates/verter_session/src/host_compile.rs:469-478`
  hardcodes `file_language: FileLanguage::vue()` for every batch input, and the
  render lane never reads the runtime-surface-refused flag. That is RT-1.
- **`Err(other)` on the HostBacked lane.** That lane's profile is the fixed
  bundler preset, which `compile_many` never lets a caller vary, so the grammar
  axis — the one caller-settable route into the generic host-error arm — exists
  only on the render lane.

Two controls run beside the table, in
`an_ordinary_success_and_a_warning_only_compile_are_never_read_as_failures`, so a
diagnostic is never equated with a refusal: an ordinary SUCCESS, and a compile
that SUCCEEDS while carrying a non-error diagnostic (an unresolvable
member-position macro type, which degrades that member to `null` and warns). Both
publish and report no error on both lanes. The warning is measured differently
per lane because the lanes surface it differently by construction: the render
lane carries it on the entry, while the host-backed entry's success-warning list
is empty by construction, so that half reads the warning off the response for the
same canonical the batch just compiled.

`searching_for_a_batch_entry_that_serves_a_stale_product_beside_fresh_errors_finds_none`
is a committed probe control for the one residual the enumeration in
[`dispositions.md`](dispositions.md) records as UNKNOWN. It drives, through the
public API only, a zero-fact self-contained component compiled to populate the
last-good slot, then store-view-advancing operations that do not edit that file's
bytes, then a re-request; the same file recompiled into a genuine failure, with
and without unrelated generations in between; and a fail-then-recover cycle. It
asserts every resulting entry is atomic. Its own doc comment states plainly that
it SEARCHES for the shape and does not prove it unreachable.

### Atomicity discrimination evidence

Every assertion above was proven discriminating by a mutation of the code under
test — `crates/verter_session/src/host_compile.rs`, never the test — each proven
present-and-new before its run (the marker occurs zero times in the pristine
bytes and exactly once afterwards, and the bytes changed) and each restored to a
byte-identical SHA-256 (`13f2cd52dfa8b87daa856fc553636a70785df0c014db1f5c0ff6eda974153730`
for `host_compile.rs`). A green planted run was treated as a failed plant until
proven otherwise; every plant below went RED.

| plant | RED at |
|---|---|
| the Stage-D group-error fan-out publishes code and a language | `DuplicateCanonicalConflict/RuntimeRender … published 12 bytes of code alongside its failure` |
| the HostBacked `CompileError` arm publishes code and a language | `CompileFailure/HostBacked …` **and** the probe control at `HostBacked/fail-after-last-good … served a product alongside 6 error(s)` |
| the RuntimeRender `CompileError` arm publishes code and a language | `CompileFailure/RuntimeRender …` **and** the probe control at `RuntimeRender/fail-after-last-good …` |
| the RuntimeRender generic-host-error arm publishes code and a language | `OtherHostError/RuntimeRender …` |
| the caught-panic entry publishes code and a language | `Panic/RuntimeRender …` |
| the per-canonical conflict message changes | class proof: *did not fail with the batch's own per-canonical conflict error, so it entered some other class* |
| the HostBacked compile-failure messages lose their canonical prefix | class proof: *carries an error that is not prefixed with its canonical id, so it came from the successful-response construction rather than the failing-compile one* |
| the RuntimeRender compile-failure messages lose their canonical prefix | the same class proof on the render lane |
| the generic host-error rendering changes | class proof: *did not fail with the typed grammar-mismatch host error this row drives* |
| the coordinator's panic rendering changes | class proof: *failed, but not through the coordinator's panic conversion* |
| a Stage-B group error is applied to every position in the batch | the clean-publish half: *`…Neighbour.svelte` should have published cleanly but reported ["duplicate canonical_id with conflicting source in batch"]* |
| the Stage-D group error is attributed to a different canonical | the per-class entry count: *expected 2 entr(ies) in this class, drove 0* |
| the successful-response arm folds warnings into errors | the warning-only control on HostBacked: *should have published cleanly but reported* the degraded-prop warning (*Authoritative runtime prop `foo` for macro syntax index 0 is unresolved (missing-dependency); Vue runtime validation degrades this row to null.*) |
| the render lane drops a successful compile's warning list | the warning-only control on RuntimeRender: *the warning-only input carried no unresolved-macro-type diagnostic, so this control is not measuring a warning: []* |
| the render lane withholds a successful product | the ordinary-success control: *reported no failure but published no code either* |

**What this evidence does NOT establish.** It establishes that the table
discriminates, not that AT-2 AS ORIGINALLY RATIFIED is evidenced. The only
construction that can express a product beside a fatal-looking error list — the
successful-response arm — is reached with error diagnostics in these runs only by
a PLANT; no public input was found that reaches it that way, and the residual
stays UNKNOWN. That measurement is precisely what the AT-2 amendment rests on:
the row is now a latent construction hazard with reachability unproven, carried
by an `#[ignore]`d characterization rather than a required-RED target, so item 6
no longer turns on it. See [`dispositions.md`](dispositions.md),
[`at2-deviation-memo.md`](at2-deviation-memo.md) (discharged) and
[`maintainer-standing-ruling-bugs-and-types.md`](maintainer-standing-ruling-bugs-and-types.md),
and the act that names the row and closes the authority question,
[`maintainer-act-at2-amendment.md`](maintainer-act-at2-amendment.md). The review seat
that disputed the amendment's authority was upheld and answered by that act; its report
is preserved verbatim in
[`exhaustion-closure-reviews.md`](exhaustion-closure-reviews.md).

## The same hazard in the other direction

A test can also run against the WRONG THING and pass. The client runtime smoke
mounted compiled modules through a bare `svelte/internal/client` specifier whose
resolution depended on where the scratch directory happened to sit; one directory
further up sits a different `svelte` version. On one tree it bound the pinned
runtime and passed; on another it bound the other copy and died inside the
official runtime. The executor now terminates that walk at its first step and
reports the runtime it bound, and the test pins it to the derived pinned version
— so a mount measured against anything else FAILS rather than deciding nothing.

## Every Svelte conformance axis has its own planted defect

The Svelte official-conformance gate judges a candidate on six axis families:
**parse**, **real-package link**, **structural**, **diagnostics**, **mapping**
(content integrity and anchor coverage) and **runtime**. Each family now has an
independently planted defect that is proven to turn the gate RED, and each plant
is bracketed by a green UNPLANTED control run over the same baseline — the
`basic-runes` client golden's own recorded artifact, the candidate proven to
reach `verdict: "pass"`. A plant is only trusted once the control passes; every
plant additionally proves it APPLIED (the marker occurs zero times in the
pristine bytes and exactly once afterwards, and the bytes differ), because an
exit code is never evidence that a mutation landed and a search that hits a
pre-existing occurrence is a false positive.

The first five families are planted in
`the_gate_detects_a_planted_defect_on_every_applicable_axis_family`
(`crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_gate.rs`).
The runtime family is planted separately, in
`the_runtime_comparison_detects_a_planted_wrong_render`, because the oracle CLI
reports the runtime axis `not-applicable` for a Svelte client golden and the
runtime comparison is the separate mount performed by `compare_mounted_render`.
That helper is the single comparison the live gate
(`every_emitting_client_request_mounts_and_renders_what_the_golden_renders`) also
drives, so the plant is judged by exactly the code that judges the shipped
route's output rather than by a second copy of it.

The runtime plant retemplates the golden module's `<p>zero</p>` template — the
one the `alternate` branch instantiates, and the markup the control run actually
renders — into `<p>GATE-RUNTIME-PLANT</p>`. What it proves is a MOUNTED-BUT-WRONG
render, not a crash: the test asserts the planted candidate still mounts (`ok`),
that the comparison reports a divergence anyway, that the planted marker appears
in the candidate's rendered markup and not in the golden's, that the two rendered
strings differ, and that the planted markup is also unequal to the line this
suite pins for that golden in `svelte_client_rendered_markup.txt` — so the pin is
a second independent catch of the same defect. A plant that merely crashed the
mount would prove only that the comparison notices a broken module.
