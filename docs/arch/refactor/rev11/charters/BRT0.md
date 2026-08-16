# BRT0 — Immediate route and transport parity

**Status:** PROPOSED / **RATIFIED (AMD-009)**; not accepted. **Class:** Framework subsystem.
**Predecessor:** BF3. **Downstream:** acceptance is a mandatory predecessor of B2
and B3.

## Objective

Correct the immediate public-route carrier selection and portable transport-result
divergences exposed by BF3, and adjudicate the two provisional bundler rows, without
changing Svelte compiler semantics.

## Owned scope

BRT0 owns RT-1 and the distinct TR-1. It also carries BND-1 and BND-2 exactly as
`AWAITING CONFIRMATION` / provisional; it may not promote, rename, or reclassify
either without the required confirmation.

| finding | acceptance ID | existing discriminating test |
|---|---|---|
| RT-1 | `BF3-RT-1-BATCH-CARRIER-PARITY` → `FC-ROUTES-001` | `a_svelte_batch_matches_the_single_file_route_item_for_item` (`#[ignore]`d target); characterizations `a_svelte_batch_input_is_currently_compiled_by_the_vue_carrier`, `the_svelte_runtime_refusals_do_not_fire_on_the_batch_route`, and `the_host_backed_batch_lane_shows_the_same_svelte_language_divergence` |
| TR-1 | `BF3-TR-1-MISSING-PRODUCT-PARITY` → `FC-ROUTES-001` | `the_transports_serialize_a_missing_node_differently` (green characterization; it fails if either current shape moves) |
| BND-1 | pending — `AWAITING CONFIRMATION` / provisional | `the_bundler_route_matches_the_in_process_host_route` (green characterization of both route answers) |
| BND-2 | pending — `AWAITING CONFIRMATION` / provisional | `the_bundler_route_matches_the_in_process_host_route` (green characterization of both source-map answers) |

## Required procedure

For each ratified item, first enable or add the correct public-boundary parity
assertion and prove it RED, then make the minimum route/transport-owner correction
and rerun the target, all named characterizations, representative success and
genuine-refusal cases, ordering, neighbour isolation, and option-conversion controls.
For BND-1/BND-2, execute the bundler and in-process boundaries, record discriminating
evidence, and obtain confirmation before assigning any non-pending acceptance ID.
No production retraction may substitute for parity.

## Required exits

RT-1 passes item-for-item between Svelte batch and single-file routes with stable
ordering, correct carrier selection, refusal parity, and no cross-item contamination.
TR-1 gives NAPI and WASM one ratified missing-product contract. BND-1/BND-2 are each
confirmed and corrected under an accepted ID or rejected with authority and evidence;
`AWAITING CONFIRMATION` cannot satisfy acceptance. `FC-ROUTES-001` stays
non-vacuous across the public boundaries. Only BRT0 acceptance satisfies this
block's B2/B3 predecessor edge.

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
