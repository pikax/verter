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
| AT-2 | a batch entry publishes a product together with a genuine typed refusal | per-entry atomicity violation | DEFER | BA0, distinct item | BA0 acceptance, no later than plan close, before any downstream dispatch | `BF3-AT-2-BATCH-REFUSAL-ATOMICITY` → `FC-ATOMIC-001` | `a_genuinely_failing_batch_entry_publishes_no_partial_product` — see the observation note below |
| CSS-1 | the standalone CSS route accepts and ignores `sourcemap: true` | option/product-contract defect | DEFER | BCSS0 | BCSS0 acceptance, no later than plan close, before any downstream dispatch | `BF3-CSS-1-STANDALONE-SOURCEMAP` → `FC-OPTIONS-001` | `the_standalone_css_route_publishes_valid_requested_maps_for_passthrough_and_transformed_css` (`#[ignore]`d correct-behavior target, owned by BCSS0); characterized green by `the_standalone_css_spelling_publishes_css_and_ignores_its_source_map_axis` |
| TR-1 | NAPI returns null where WASM throws for a missing product | portable transport-contract defect | DEFER | BRT0, distinct item | BRT0 acceptance, no later than plan close, before any downstream dispatch | `BF3-TR-1-MISSING-PRODUCT-PARITY` → `FC-ROUTES-001` | `the_transports_serialize_a_missing_node_differently` (green characterization; fails if either shape moves) |
| RA-1 | `list_virtual_files` names `Main` for a component whose runtime surface is refused | parse-derived route-assembly artifact | REJECTED as a defect | — | — | — | `the_node_list_names_main_for_a_component_whose_runtime_surface_is_refused` (green characterization of the parse-derived list) |
| RA-2 | `has_runtime_surface` counts styles, so a refusal publishing CSS would take the wrong arm; no reachable state | latent | REJECTED as a defect | — | — | — | no test: the state is unreachable, so there is nothing to characterize. Recorded here only |

### Observation note on AT-2

AT-2 is recorded exactly as ratified. The driven evidence for the genuine-failure
class this block could reach does **not** reproduce it: with two batch inputs
naming the same canonical under different sources — the batch's own typed
conflict failure, `crates/verter_session/src/host_compile.rs:479-485` — every
failing entry publishes no code, no source map and no output language, and its
neighbour is unaffected. That is what `a_genuinely_failing_batch_entry_publishes_no_partial_product`
asserts, green.

The failure class that would exercise a genuine *Svelte* refusal inside a batch
is not reachable while RT-1 stands: the batch selects the Vue carrier for every
input, so no Svelte refusal fires there. Malformed Vue script is not a substitute
— the Vue carrier's error recovery still publishes a module for it. So the
earlier "product beside a refusal" reading came from a Vue-shaped SUCCESS, not
from a refusal. This note records the measurement; the row's class and owner are
unchanged.

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
