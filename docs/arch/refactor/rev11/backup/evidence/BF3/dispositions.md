# BF3 — per-finding dispositions

Facts and dispositions for every finding this block's probe surfaced. The table
below is the **ratified** one; nothing here is re-classed, renamed or invented.
Two rows (`BND-1`, `BND-2`) post-date it and are recorded separately with
their corrected public-contract measurements. BND-1 remains rejected as a
defect. BND-2 is split by public product: the Vite virtual-script path is green,
while the Rollup/non-Vite inline product is confirmed and deferred to BRT0.

Every `DEFER`'s resolution gate is its owner block's acceptance, no later than
plan close, and before any downstream dispatch.

The machine-readable per-cell facts live at
`crates/verter_session/src/svelte_conformance_cell_record.json` (kept there so no
path under `crates/` names the program, its revision, or a block) and are held
against the live suite by
`the_committed_cell_record_matches_what_the_suite_observes`.

## Ratified rows

| id | finding | class | disposition | owner | resolution gate | acceptance id | gating test |
|---|---|---|---|---|---|---|---|
| SV-1 | `{#each}` flags set `EACH_ITEM_REACTIVE` where official does not (21 vs 20) | Svelte compiler defect | DEFER | BS0 | BS0 acceptance, no later than plan close, before any downstream dispatch | `BF3-SV-1-EACH-FLAGS` → `FC-SVELTE-001` | `each_flags_for_a_keyed_runes_each_match_the_official_compiler` (`#[ignore]`d conformance target); characterized green by `each_flags_for_a_keyed_runes_each_currently_add_the_item_reactive_bit` |
| SV-2 | `$props()` non-interpolation instance-script usage refused; official accepts | Svelte compiler gap | DEFER | BS0 | BS0 acceptance, no later than plan close, before any downstream dispatch | `BF3-SV-2-PROPS-INSTANCE` → `FC-SVELTE-001` | `a_runes_props_read_in_the_instance_script_compiles_to_a_runtime_module` (`#[ignore]`d); characterized green by `a_runes_props_read_in_the_instance_script_is_currently_refused_with_its_typed_code` |
| SV-3 | client source map omits authored script-declaration provenance | Svelte compiler mapping defect | DEFER | BS0 | BS0 acceptance, no later than plan close, before any downstream dispatch | `BF3-SV-3-CLIENT-MAP-SCRIPT` → `FC-SVELTE-001` | `the_client_source_map_covers_every_required_authored_anchor` (`#[ignore]`d); characterized green by `the_client_source_map_currently_carries_only_these_authored_coordinates` |
| SV-4 | untyped `$props()` destructure publishes an empty props surface, no diagnostic | Svelte session-projector defect | DEFER | BS0, distinct item | BS0 acceptance, no later than plan close, before any downstream dispatch | `BF3-SV-4-PROPS-SURFACE` → `FC-TS-001` | `an_untyped_svelte_props_destructure_publishes_its_authored_props_to_typescript` (`#[ignore]`d correct-surface target; TypeScript must see required `label` and optional `disabled`); characterized green by `an_untyped_svelte_props_destructure_publishes_a_props_surface_typescript_sees_as_empty` inside the pinned Svelte closure |
| RT-1 | the batch route compiles `.svelte` as Vue and drops its refusals | public batch route / carrier-selection defect | DEFER | BRT0 | BRT0 acceptance, no later than plan close, before any downstream dispatch | `BF3-RT-1-BATCH-CARRIER-PARITY` → `FC-ROUTES-001` | `a_svelte_batch_matches_the_single_file_route_item_for_item` (`#[ignore]`d); characterized green by `a_svelte_batch_input_is_currently_compiled_by_the_vue_carrier`, `the_svelte_runtime_refusals_do_not_fire_on_the_batch_route`, `the_host_backed_batch_lane_shows_the_same_svelte_language_divergence` |
| AT-1 | a combined IDE-requesting compile publishes the TSX product after a runtime refusal | atomicity violation | DEFER | BA0 | BA0 acceptance, no later than plan close, before any downstream dispatch | `BF3-AT-1-COMBINED-REFUSAL-ATOMICITY` → `FC-ATOMIC-001` | `a_refused_combined_request_publishes_no_product_at_all` (`#[ignore]`d conformance target, added this round); characterized green by `a_refused_runtime_surface_still_publishes_the_ide_and_public_api_products` |
| AT-2 | the host-backed batch construction reads a product and an error list from the same response independently, so it COULD express both at once; no reachable input does. **Amended** — the original claim, that a batch entry publishes a product together with a genuine typed refusal, is REJECTED as not demonstrated | latent construction hazard, reachability unproven (not a demonstrated per-entry atomicity violation) | DEFER | BA0, distinct item | BA0 acceptance, no later than plan close, before any downstream dispatch | `BF3-AT-2-BATCH-REFUSAL-ATOMICITY` → `FC-ATOMIC-001` | `the_host_backed_success_construction_is_never_fed_a_response_that_carries_an_error` (`#[ignore]`d characterization of the hazard, NOT a required-RED target); reachable-class control `a_genuinely_failing_batch_entry_publishes_no_partial_product`; search control `searching_for_a_batch_entry_that_serves_a_stale_product_beside_fresh_errors_finds_none` — see the observation note below |
| CSS-1 | the standalone CSS route accepts and ignores `sourcemap: true` | option/product-contract defect | DEFER | BCSS0 | BCSS0 acceptance, no later than plan close, before any downstream dispatch | `BF3-CSS-1-STANDALONE-SOURCEMAP` → `FC-OPTIONS-001` | `the_standalone_css_route_publishes_valid_requested_maps_for_passthrough_and_transformed_css` (`#[ignore]`d correct-behavior target, owned by BCSS0); characterized green by `the_standalone_css_spelling_publishes_css_and_ignores_its_source_map_axis` |
| TR-1 | NAPI returns null where WASM throws for a missing product | portable transport-contract defect | DEFER | BRT0, distinct item | BRT0 acceptance, no later than plan close, before any downstream dispatch | `BF3-TR-1-MISSING-PRODUCT-PARITY` → `FC-ROUTES-001` | `the_transports_serialize_a_missing_node_differently` (green characterization; fails if either shape moves) |
| RA-1 | `list_virtual_files` names `Main` for a component whose runtime surface is refused | parse-derived route-assembly artifact | REJECTED as a defect | — | — | — | `the_node_list_names_main_for_a_component_whose_runtime_surface_is_refused` (green characterization of the parse-derived list) |
| RA-2 | `has_runtime_surface` counts styles, so a refusal publishing CSS would take the wrong arm; no reachable state | latent | REJECTED as a defect | — | — | — | no test: the state is unreachable, so there is nothing to characterize. Recorded here only |

### Observation note on AT-2

**The AT-2 row above is AMENDED, and it is the only ratified row this block
touched.** Read its provenance first, because it is the weakest link in the
chain: the authority is the maintainer's standing ruling of 2026-08-17 on bug
handling and the type waiver
([`maintainer-standing-ruling-bugs-and-types.md`](maintainer-standing-ruling-bugs-and-types.md),
verbatim), which is a GENERAL rule and does NOT name AT-2. Applying it to this
row is the program orchestrator's reading. What the ruling states in its own
words is rule 4 — a bug found during the program is captured as an `#[ignore]`d
test with the fix deferred — together with its consequence that a finding which
is not a demonstrated, reproduced defect must not carry a required-RED target.
The amendment applied is the one
[`at2-deviation-memo.md`](at2-deviation-memo.md) asked for, on the
recommendation of the independent unprimed consult at
[`at2-disposition-ruling.md`](at2-disposition-ruling.md): reject the ratified
claim, reclassify as a latent construction hazard with reachability unproven,
retain the DEFER to `BA0`, and drop the requirement that a Svelte-refusal
atomicity target be RED. The class, finding text and gating column changed; the
disposition, owner, resolution gate and acceptance ID did not. Everything else
in the ratified table is untouched.

**The authority chain, stated without softening, because a review seat calls it
insufficient.** The maintainer's verbatim act is GENERAL and does not name AT-2.
The wording that matches the memo's requested act clause for clause is the
PROGRAM ORCHESTRATOR's direction, not the maintainer's own words; what the
maintainer supplied is the general rule that direction applies. The earlier
independent consult also held that no track-level actor may amend a ratified
row, and that holds here only if the standing act is read as the authorizing
act. One review seat (Codex, this round) holds that it is not, and asks for an
explicit maintainer act naming AT-2 or a revert of this row and the matching
`BA0` lines. That objection is recorded rather than answered: if the chain is
rejected, this row and those lines revert, and item 6 returns to
`NOT-EVIDENCED`.

The measurement that made the amendment necessary is unchanged, and follows.

#### The construction-site enumeration

Every construction of `CompileBatchEntry` in
`crates/verter_session/src/host_compile.rs`, with what each writes into the
product fields:

| # | origin | `errors` | `code` / `lang` / `source_map` | atomic? |
|---|---|---|---|---|
| 1 | Stage-D `group_errors` fan-out (duplicate-canonical conflict, upsert failure) | one string | hardcoded empty | YES |
| 2 | `compile_one_in_batch` precomputed-error short circuit | one string | hardcoded empty | YES |
| 3 | HostBacked `get_virtual_file` → `Ok(response)` | error-severity diagnostics of the response | `response.code` / `source_map` / `lang` | **NO** |
| 4 | HostBacked `Err(CompileError)` | non-empty by construction | hardcoded empty | YES |
| 5 | HostBacked `Err(other)` — including `RuntimeSurfaceRefused` | one formatted string | hardcoded empty | YES |
| 6 | RuntimeRender `Ok(render)` | hardcoded `Vec::new()` | product | YES |
| 7 | RuntimeRender `Err(CompileError)` | non-empty | hardcoded empty | YES |
| 8 | RuntimeRender `Err(other)` | one string | hardcoded empty | YES |
| 9 | `compile_panic_entry` | one string | hardcoded empty | YES |

Eight of the nine are atomic by hardcoded literal. Site 3 is the only
construction that reads a product and an error list independently, so it is the
only shape that can express a product beside a fatal-looking `errors` list.

#### The reachability argument

The typed refusal does **not** reach site 3. `HostError::RuntimeSurfaceRefused`
is returned as `Err` by `get_virtual_file`
(`crates/verter_session/src/host_resolve/virtual_file_pipeline.rs:1087-1108`) and
lands on site 5, which publishes no product. The actual typed-refusal path is
therefore atomic.

A genuine *Svelte* refusal cannot reach the batch at all while RT-1 stands:
`crates/verter_session/src/host_compile.rs:469-478` hardcodes
`file_language: FileLanguage::vue()` for every batch input, and the
`RuntimeRender` lane never reads the runtime-surface-refused flag. Malformed Vue
script is not a substitute — the Vue carrier's error recovery still publishes a
module for it, with no error at all. So the earlier "product beside a refusal"
reading came from a Vue-shaped SUCCESS, not from a refusal.

The only known upstream producer of an `Ok` carrying error-severity diagnostics
is the `DevServeLastKnownGood` stale serve (`virtual_file_pipeline.rs:1755-1766`),
which pairs the PREVIOUS compile's outputs with the NEW compile's error
diagnostics — and whose own source says it is not a fresh refusal. Every
invalidation route that could make an unchanged-profile compile newly fail also
clears the last-good slot (`host_upsert.rs:754-761`, `block_content.rs:1613-1618`,
`host_lifecycle.rs:530`/`:916`, `host_manage/analysis_io.rs:2044`/`:2092`), and a
cross-file edit dies with the warm hit because `peek_last_good` validates the same
fact signature as `lookup` (`cache_runtime/compile_output_node.rs:665` vs `:549`).

#### The residual, stated as UNKNOWN

One residual is **UNKNOWN, not closed**: `lookup` acquires a store view
unconditionally while `peek_last_good` skips its validator when the fact rail is
empty (`cache_runtime/compile_output_node.rs:665` vs `:549`), and a self-contained
SFC records zero facts. Reaching the stale serve through that crack would
additionally require a cold recompile of unchanged bytes to fail, which only the
transient macro-semantic outcomes could do. It was probed and not reproduced. It
is recorded as an open proof gap, not as a closure.

#### What was driven

`a_genuinely_failing_batch_entry_publishes_no_partial_product` is now a
table-driven regression over every failing-entry class the public `compile_many`
API can genuinely reach, on both lanes where the class exists on that lane. Each
row proves the entry entered its intended class before asking whether it
published anything, and each row carries an entry that must publish cleanly:

| class | lanes driven | what proves the entry entered it |
|---|---|---|
| duplicate-canonical conflict | RuntimeRender, HostBacked | the entry's single error is exactly the batch's per-canonical conflict message, at BOTH original input positions |
| compile failure (`Err(CompileError)`) | RuntimeRender, HostBacked | every message is prefixed with the canonical id — the discriminator against site 3, whose diagnostics ride verbatim beside a published product — and at least one is the template-parse diagnostic |
| other typed host error (`Err(other)`) | RuntimeRender | the single error is the typed grammar-mismatch host error, and the same inputs under the lane's ordinary profile publish |
| caught panic | RuntimeRender, HostBacked | the single error carries the coordinator's panic rendering and the injected panic body |
| upsert failure | NOT REACHABLE | it folds into the same per-canonical map as the conflict, but the upsert engine only errors from a scheduler `Failed` / `Superseded` / `Shutdown` completion state or a post-commit generation-fence mismatch (`host_upsert.rs` `map_states` / `finish_upsert_post_commit`); `compile_many` exposes no input that produces any of them, and the only in-tree driver is a test-only completion-state seam that bypasses the batch. Both constructions it would reach (sites 1 and 2) hardcode an empty product |
| typed Svelte runtime refusal | NOT REACHABLE | the batch hardcodes the Vue carrier (RT-1) |
| other host error on HostBacked | NOT REACHABLE | that lane's profile is the fixed bundler preset, which `compile_many` never lets a caller vary, so the grammar axis — the one caller-settable route to `Err(other)` — exists only on the render lane |

Three rows above are NOT REACHABLE, and they are not the same kind of fact: upsert
failure and the typed Svelte runtime refusal are whole classes with no reachable
input on either lane, while `Err(other)` is a class the table DOES drive — on the
render lane — that has no reachable input on the host-backed one. None of the three
is represented by an `#[ignore]`d target: such a target would fail for a reason
other than the property it names, which the ruling calls a stub. The suite states
the same three facts in the same terms
(`a_genuinely_failing_batch_entry_publishes_no_partial_product`).

Two controls run beside the table
(`an_ordinary_success_and_a_warning_only_compile_are_never_read_as_failures`), so
a diagnostic is never equated with a refusal and "no product beside a failure" is
never satisfied by a route that withholds every product: an ordinary SUCCESS, and
a compile that succeeds while carrying a non-error diagnostic (an unresolvable
member-position macro type, which degrades that member to `null` and warns). Both
publish their product and report no error, on both lanes.

The residual above additionally has a committed probe control,
`searching_for_a_batch_entry_that_serves_a_stale_product_beside_fresh_errors_finds_none`.
It drives, through the public API only, the sequences most likely to reach the
stale serve — a zero-fact self-contained component compiled to populate the
last-good slot, then store-view-advancing operations that do not edit that file's
bytes, then a re-request; the same file recompiled into a genuine failure, with
and without unrelated generations in between; and a fail-then-recover cycle — and
asserts every resulting entry is atomic. Its own doc comment states plainly that
it SEARCHES for the shape and does not prove it unreachable.

None of this evidences the ORIGINAL AT-2 claim, and none of it is offered as
that claim's gate. It is the charter's separate atomicity exit, driven green.

#### The amended row's own artifact

The hazard's PRECONDITION is now carried by
`the_host_backed_success_construction_is_never_fed_a_response_that_carries_an_error`
in `svelte_batch_route_tests.rs`, `#[ignore]`d per rule 4 of the standing ruling.
It is deliberately NOT a required-RED target and it passes today. It asserts two
things through the public batch API, on the host-backed lane: a compile that
SUCCEEDS while carrying a diagnostic is served by site 3 from a response that
carries NO error-severity diagnostic (the response's diagnostics list is
non-empty, so that is a reading rather than a vacuous check over an empty list),
and a compile that genuinely FAILS answers `Err`, so its entry never comes from
site 3 at all. The day a successful host-backed response carries an
error-severity diagnostic, the first reading turns RED. That is the artifact
`BA0` re-runs.

**What it does not claim.** It does not prove site 3 READS its error list rather
than writing a hardcoded empty one. Replacing that filter with `Vec::new()`
leaves it green — two review seats proved exactly that — and the test's own doc
comment says so. Deciding that half needs a synthetic response carrying a product
and an error together, which needs a production seam this block does not own.

**Its `#[ignore]` is a deliberate category mismatch, and costs no coverage.**
Every other ignored target in these suites states a correct behaviour the product
does not yet have, and FAILS. This one passes, because the finding it names is
not a defect and therefore has no failing correct-behaviour to state; it is
`#[ignore]`d because the standing ruling's rule 4 puts a deferred finding's
artifact outside the default universe, not because it is expected to fail. Both
review seats flagged the shape; it is recorded rather than disguised. No live
coverage is lost: the same precondition is asserted by the NON-ignored
`an_ordinary_success_and_a_warning_only_compile_are_never_read_as_failures`,
whose host-backed half already requires the response to carry no error-severity
diagnostic and which runs in the default suite.

**Two plants settle what the reachability argument rests on**, each proven
present, unique and absent from `HEAD` before its run and restored
byte-identically:

| plant | mutation | result |
|---|---|---|
| `PLANT_P4_ERROR_SEVERITY` | flip the producer of `XUnresolvedImportedMacroType` from `Diagnostic::warning` to `Diagnostic::error` | the entry reports the error and publishes `code_len=0`, `lang=None` — the `compile_diags.has_errors` gate turned the whole response into `Err`, so it landed on an atomic arm. An error-severity COMPILE diagnostic therefore cannot reach site 3 at all: this is the mechanism behind the reachability argument, measured rather than argued |
| `PLANT_P5_OK_WITH_ERROR_DIAGNOSTIC` | merge an error-severity diagnostic into the response `get_virtual_file` returns on its SUCCESS path, downstream of that gate | the precondition now holds, the test turns RED naming it — and the entry carries `errors=[the planted message]` beside `code_len=480, lang=Some("ts")`. **The hazard is real as a construction property**; what is unproven is only whether any input can produce that response |

## Re-measured rows post-dating the ratified table

The earlier confirmations measured the wrong contracts. BND-1 called the
legacy/default `unpluginFactory`, which is intentionally Vue-pinned, directly
on `/probe/Plug.svelte`; its `transformInclude=false` answer is the documented
Vue include contract, not a Svelte-route mismatch. The public entries were
executed independently: `VerterVue.vite({})` returned `transformInclude=true`
for `/probe/Plug.vue` and false for `/probe/Plug.svelte`, while
`VerterSvelte.vite({})` returned true for `/probe/Plug.svelte` and false for
`/probe/Plug.vue`.

BND-2 initially inspected only the synthetic carrier wrapper returned by the
Vite transform. That wrapper intentionally has no map because it imports and
re-exports the compiled virtual script. Executing the public Vite consumer path
resolved and loaded `/probe/Plug.vue?vue&type=script&lang.js` through
`VerterVue.vite({}).load` and
`/probe/Plug.svelte?verter&type=script&lang.js` through
`VerterSvelte.vite({}).load`; both loaded products carried the requested map.
The Svelte loaded code also matched the in-process host's mapped `Main` product.
That path remains green and is not a defect.

The distinct public Rollup/non-Vite product is defective. `VerterVue.rollup()`
requests `sourceMap: true` and returns the compiled module inline because no
Vite virtual-script consumer exists, but the public transform returns
`map: null`. The matching in-process host request publishes a map. The executed
measurement is therefore `hostHasMap:true`, `publicTransformIsInline:true`,
`publicTransformHasMap:false`. `VerterSvelte.rollup()` was executed too and is
not the same product shape: it retains a `?verter&type=script` wrapper, and that
loaded script carries its map.

The probe no longer treats loadability as freshness. Before import it hashes the
current production `src/` inputs and the complete ignored `dist/` tree against
`packages/unplugin/scripts/probe-bundler-route.freshness.json`; missing or stale
dist exits non-zero. The record was generated from
`pnpm --filter @verter/unplugin build` at this HEAD.

| id | finding | class | disposition | owner | resolution gate | acceptance id | gating test |
|---|---|---|---|---|---|---|---|
| BND-1 | the Vue-pinned legacy/default factory rejects `.svelte`, while the public Svelte-pinned entry accepts it | documented entry-specific include contract | REJECTED as a defect | — | — | — | `the_bundler_public_entries_apply_their_documented_include_contract` (green; executes both public pinned entries) |
| BND-2 | `VerterVue.rollup()` requests a source map and returns the host product inline, but its public transform drops the host-published map | Rollup/non-Vite inline source-map parity defect; Vite virtual-script products remain green | DEFER | BRT0, distinct item | BRT0 acceptance, no later than plan close, before any downstream dispatch | `BF3-BND-2-SOURCEMAP-PARITY` → `FC-ROUTES-001` | `the_bundler_rollup_inline_transform_preserves_requested_source_maps` (`#[ignore]`d correct-product target); Vite control `the_bundler_virtual_script_loads_publish_requested_source_maps` remains green |

## Observations recorded, not classed

Measured while closing the route inventory. Neither is proposed as a finding;
both are pinned green so a change in either direction is visible.

- **The audited-compile transport spelling returns the audit record, not the
  product.** `crates/verter_napi/src/lib.rs:2525-2540` and
  `crates/verter_wasm/src/lib.rs:874` encode `.audit()` and drop the result;
  with audit disabled it projects to `null`
  (`crates/verter_napi/src/audit.rs:60-65`). On an audit-ENABLED host the Vue
  carrier yields a stored record and the Svelte carrier yields `null`, for a
  component whose module serves normally. Both transports agree, so this is the
  shared host's capture behaviour, not a transport divergence. Pinned by
  `the_audited_compile_spelling_captures_for_vue_and_not_for_svelte_on_both_transports`.
- **Carrier classification follows the canonical's extension, not the upsert's
  `fileKind`.** Observed while planting against the probe: a Vue source upserted
  at a `.svelte` canonical with `fileKind: "vue"` still took the Svelte lane.
  Recorded because it is the reason a naive plant on that axis does not
  discriminate.
- **The published Svelte virtual-script map is structurally valid and
  semantically empty.** Measured on the built bundler entry: the Vue public
  virtual-script product carries an 84-character `mappings` — 16 segments across
  18 generated lines, 12 of them naming an authored position — while BOTH public
  Svelte routes, the Vite virtual-script load and the Rollup one, carry the
  single segment `"A"`: one generated column, no authored position, nothing a
  consumer can navigate to. The green acceptance target asks only for a v3 map
  with a non-empty `mappings` string, so it passes on both. Pinned by
  `the_public_svelte_virtual_script_map_currently_maps_nothing_where_vue_maps_most_of_its_output`,
  which fixes the Svelte structure exactly and the Vue structure as a floor, so
  a correction to the Svelte map builder flips it — that flip is the signal to
  re-measure, not a regression. This is NOT a new finding row: Svelte client
  map provenance is already owned as SV-3 by BS0, whose correct-behavior target
  is `the_client_source_map_covers_every_required_authored_anchor`. Recorded
  here only because the measurement was taken at the bundler boundary, where
  the acceptance target cannot see the difference.

## What this block did not do

No production mechanism, no amendment, no compiler correction. The remaining
`#[ignore]`d conformance targets above state correct behaviour and fail today;
each is the acceptance gate for its owner's correction. BND-1 stays pinned by
its green public-entry assertion. BND-2 keeps the green Vite public-product
assertion and adds the failing Rollup inline-product acceptance target.
