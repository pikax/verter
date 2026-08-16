# BF3 — per-finding dispositions

Facts and dispositions for every finding this block's probe surfaced. The table
below is the **ratified** one; nothing here is re-classed, renamed or invented.
Two rows (`BND-1`, `BND-2`) post-date it and are marked as such.

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
| SV-4 | untyped `$props()` destructure publishes an empty props surface, no diagnostic | Svelte session-projector defect | DEFER | BS0, distinct item | BS0 acceptance, no later than plan close, before any downstream dispatch | `BF3-SV-4-PROPS-SURFACE` → `FC-TS-001` | `an_untyped_svelte_props_destructure_publishes_a_props_surface_typescript_sees_as_empty` (green characterization, read from the checker inside the pinned Svelte closure); no `#[ignore]`d target — the correct surface is the projector's to define |
| RT-1 | the batch route compiles `.svelte` as Vue and drops its refusals | public batch route / carrier-selection defect | DEFER | BRT0 | BRT0 acceptance, no later than plan close, before any downstream dispatch | `BF3-RT-1-BATCH-CARRIER-PARITY` → `FC-ROUTES-001` | `a_svelte_batch_matches_the_single_file_route_item_for_item` (`#[ignore]`d); characterized green by `a_svelte_batch_input_is_currently_compiled_by_the_vue_carrier`, `the_svelte_runtime_refusals_do_not_fire_on_the_batch_route`, `the_host_backed_batch_lane_shows_the_same_svelte_language_divergence` |
| AT-1 | a combined IDE-requesting compile publishes the TSX product after a runtime refusal | atomicity violation | DEFER | BA0 | BA0 acceptance, no later than plan close, before any downstream dispatch | `BF3-AT-1-COMBINED-REFUSAL-ATOMICITY` → `FC-ATOMIC-001` | `a_refused_combined_request_publishes_no_product_at_all` (`#[ignore]`d conformance target, added this round); characterized green by `a_refused_runtime_surface_still_publishes_the_ide_and_public_api_products` |
| AT-2 | a batch entry publishes a product together with a genuine typed refusal | per-entry atomicity violation | DEFER | BA0, distinct item | BA0 acceptance, no later than plan close, before any downstream dispatch | `BF3-AT-2-BATCH-REFUSAL-ATOMICITY` → `FC-ATOMIC-001` | `a_genuinely_failing_batch_entry_publishes_no_partial_product` — see the observation note below |
| CSS-1 | the standalone CSS route accepts and ignores `sourcemap: true` | option/product-contract defect | DEFER | BCSS0 | BCSS0 acceptance, no later than plan close, before any downstream dispatch | `BF3-CSS-1-STANDALONE-SOURCEMAP` → `FC-OPTIONS-001` | `the_standalone_css_spelling_publishes_css_and_ignores_its_source_map_axis` (green characterization; fails if the axis becomes live) |
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

## Rows post-dating the ratified table

Both were recorded while executing the bundler route this round and await
confirmation. Class and disposition are provisional.

| id | finding | class | disposition | owner | resolution gate | acceptance id | gating test |
|---|---|---|---|---|---|---|---|
| BND-1 | the unplugin's `transformInclude` filter rejects a `.svelte` id while its `transform` hook handles the same id | bundler route-identity inconsistency (provisional) | AWAITING CONFIRMATION | BRT0 | BRT0 acceptance, no later than plan close, before any downstream dispatch | pending | `the_bundler_route_matches_the_in_process_host_route` (green characterization of both answers) |
| BND-2 | the unplugin returns no source map for either carrier although the profile it builds requests `sourceMap: true` (`packages/unplugin/src/index.ts:732`), while the host route publishes one for the same request | bundler optional-product contract (provisional) | AWAITING CONFIRMATION | BRT0 | BRT0 acceptance, no later than plan close, before any downstream dispatch | pending | `the_bundler_route_matches_the_in_process_host_route` (green characterization) |

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

## What this block did not do

No production mechanism, no amendment, no compiler correction. Every `#[ignore]`d
conformance target above states the correct behaviour and fails today; each is
the acceptance gate for its owner's correction.
