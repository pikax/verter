# BRT0 — Immediate route and transport parity

**Status:** PROPOSED / **RATIFIED (AMD-009 §7)**; not accepted. The §7 DIRECTION is
ratified by a direct maintainer act; these exact bytes are recorded and independently
reviewed, not maintainer-inspected — see
[`amd009-ratification-packet.md`](../evidence/BF3/amd009-ratification-packet.md).
**Class:** Framework subsystem. **Predecessor:** BF3. **Downstream:** acceptance is a
mandatory predecessor of B2 and B3.

## Objective

Correct the immediate public-route carrier selection and portable
transport-result divergences exposed by BF3, without changing Svelte compiler
semantics.

## Owned scope

BRT0 owns RT-1, the distinct TR-1, and the Rollup/non-Vite inline portion of
BND-2. BND-1 remains outside scope after public-contract remeasurement rejected
it as a defect. The Vite virtual-script portion of BND-2 is also green and
outside correction scope; see `../evidence/BF3/dispositions.md`.

| finding | acceptance ID | existing discriminating test |
|---|---|---|
| RT-1 | `BF3-RT-1-BATCH-CARRIER-PARITY` → `FC-ROUTES-001` | `a_svelte_batch_matches_the_single_file_route_item_for_item` (`#[ignore]`d target); characterizations `a_svelte_batch_input_is_currently_compiled_by_the_vue_carrier`, `the_svelte_runtime_refusals_do_not_fire_on_the_batch_route`, and `the_host_backed_batch_lane_shows_the_same_svelte_language_divergence` |
| TR-1 | `BF3-TR-1-MISSING-PRODUCT-PARITY` → `FC-ROUTES-001` | `the_transports_serialize_a_missing_node_differently` (green characterization; it fails if either current shape moves) |
| BND-2 (Rollup/non-Vite inline product only) | `BF3-BND-2-SOURCEMAP-PARITY` → `FC-ROUTES-001` | `the_bundler_rollup_inline_transform_preserves_requested_source_maps` (`#[ignore]`d target); green Vite control `the_bundler_virtual_script_loads_publish_requested_source_maps` |

## Required procedure

For each owned item above — RT-1 and TR-1 from the ratified table, and the
Rollup/non-Vite half of BND-2 from the re-measured rows that post-date it, both
carried by [`../evidence/BF3/dispositions.md`](../evidence/BF3/dispositions.md) —
first enable or add the correct public-boundary parity
assertion and prove it RED, then make the minimum route/transport-owner correction
and rerun the target, all named characterizations, representative success and
genuine-refusal cases, ordering, neighbour isolation, and option-conversion
controls. No production retraction may substitute for parity.

## Required exits

RT-1 passes item-for-item between Svelte batch and single-file routes with stable
ordering, correct carrier selection, refusal parity, and no cross-item contamination.
TR-1 gives NAPI and WASM one ratified missing-product contract.
The public Rollup/non-Vite inline transform retains a requested map when the
matching host product publishes one; Vite virtual-script map behavior remains
green.
`FC-ROUTES-001` stays non-vacuous across those public boundaries. Only BRT0
acceptance satisfies this block's B2/B3 predecessor edge.

## Re-measured exclusions and split

These two rows were `AWAITING CONFIRMATION` when this charter was first written. The
audit then executed both public pinned Vite entries and both public Rollup entries and
recorded the settled result in
[`../evidence/BF3/dispositions.md`](../evidence/BF3/dispositions.md), which is the
authority for every classification below; this charter follows that table and
re-classes nothing itself.

BND-1 is not an include-identity defect: `VerterVue.vite({})` is Vue-pinned and
`VerterSvelte.vite({})` is Svelte-pinned, and each public entry accepts its own
documented extension. BND-2 is not a defect on Vite: the transform wrapper is
routing glue with `map: null`, while the `?vue&type=script` and
`?verter&type=script` loads consumed by Vite both publish the requested map.
It is confirmed only on `VerterVue.rollup()`'s non-Vite inline product, which
drops the map despite requesting one and receiving one from the host.
`VerterSvelte.rollup()` retains a virtual-script wrapper and is therefore not
part of that inline-product correction.

## What it must NOT do

BRT0 must not implement Svelte planner, semantic-lowering, emitter, map, or projector
corrections; standalone CSS product generation; BA0's request/result architecture;
or B3/B4 authority. It must not add production retraction, defect-selected refusal,
fixture-identity routing, a version-specific divergence list, or source/generated
string scanning as a second routing authority.

## Abort/rescope

Stop with `RESCOPE_REQUIRED` if a mismatch is actually a Svelte semantic defect or
requires a ratified product-contract change. Route it to the owning correction block;
do not hide it with transport normalization, a fixture branch, or a retraction path.
