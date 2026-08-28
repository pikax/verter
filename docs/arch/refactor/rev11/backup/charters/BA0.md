# BA0 — Immediate request and result atomicity

**Status:** PROPOSED / **RATIFIED (AMD-009 §7)**; not accepted. The §7 DIRECTION is
ratified by a direct maintainer act; these exact bytes are recorded and independently
reviewed, not maintainer-inspected — see
[`amd009-ratification-packet.md`](../evidence/BF3/amd009-ratification-packet.md).
**Class:** Foundational atomic. **Predecessor:** BF3. **Downstream:** acceptance is a
mandatory predecessor of B2 and B3.

## Objective

Correct the legacy mixed-outcome request/result boundary before B2/B3. IDE and
PublicApi availability use independent typed product-request identities and
independently committed results; an intentionally combined multi-product request is
all-or-nothing. Shared preparation may be reused, but it does not merge publication
identity.

## Owned scope

BA0 owns AT-1 and the distinct per-entry item AT-2 at the common compiler/session
request, result, and publication boundary. It establishes the minimum immediate
separation required by `FC-ATOMIC-001`; B3 and B4 retain final authority for the
canonical request model and atomic publication architecture.

| finding | acceptance ID | target and characterization obligation |
|---|---|---|
| AT-1 | `BF3-AT-1-COMBINED-REFUSAL-ATOMICITY` → `FC-ATOMIC-001` | `a_refused_combined_request_publishes_no_product_at_all` (`#[ignore]`d target); characterization `a_refused_runtime_surface_still_publishes_the_ide_and_public_api_products` |
| AT-2 | `BF3-AT-2-BATCH-REFUSAL-ATOMICITY` → `FC-ATOMIC-001` | AT-2 is an AMENDED row: a latent construction hazard with reachability unproven, not a demonstrated per-entry atomicity violation (see the maintainer standing ruling of 2026-08-17 and the amended row in `dispositions.md`). Its artifact is the `#[ignore]`d characterization `the_host_backed_success_construction_is_never_fed_a_response_that_carries_an_error`, which is NOT a required-RED target. `a_genuinely_failing_batch_entry_publishes_no_partial_product` remains the green regression over the reachable failure classes, and `searching_for_a_batch_entry_that_serves_a_stale_product_beside_fresh_errors_finds_none` the green search control. |

## Required procedure

For AT-1, enable the existing ignored public-boundary target over the canonical
request token, requested-product set, typed terminal result, and every publication
under that token, and prove its RED result discriminates partial publication. Keep
that target as the live `FC-ATOMIC-001` target.

For AT-2, require no RED cycle at all. Its ratified claim is rejected as not
demonstrated and the row is amended to a latent construction hazard with
reachability unproven, so there is no reproduced defect for a RED target to
assert. In particular, do NOT require a Svelte-refusal atomicity target to be
RED: such a target would fail only because the separate ratified row RT-1
prevents Svelte classification on the batch route at all, which the standing
ruling calls a stub rather than evidence. Make the minimum common-layer
correction that removes the hazard — the host-backed successful-response
construction reading a product and an error list independently — with no
retraction mechanism, and rerun the live targets, the `#[ignore]`d
characterization named above, the batch-neighbour control, and the
independent-product success controls. If the hazard is demonstrated reachable
before or during BA0, that reproduction is a new finding with its own RED target;
it is not assumed here.

## Required exits

Both acceptance IDs satisfy `FC-ATOMIC-001`: success publishes all and only the
products requested by that identity, refusal publishes none, and a failing batch
entry cannot publish a partial product or contaminate a neighbour. Separate IDE-only
or PublicApi-only requests may succeed under separate identities; a combined request
cannot publish those products beside its runtime refusal. AT-1's existing ignored
test is the live acceptance target. AT-2 is satisfied by removing the latent
construction hazard and by its `#[ignore]`d characterization turning from
"passes, hazard unreachable" into a statement the corrected construction makes
structurally true; it carries no RED target and no Svelte-refusal obligation. All
affected public routes observe the same transaction boundary.

Only BA0 acceptance satisfies this block's B2/B3 predecessor edge.

## What it must NOT do

BA0 must not implement Svelte emitter, map, or projector fixes; standalone CSS or
route/transport corrections; B3's final canonical request model; or B4's final
publication substrate. It must not add production retraction, a defect-selected
refusal, artifact-withholding shim, fixture-identity branch, known-divergence list,
or string-scanning second authority.

## Abort/rescope

Stop with `RESCOPE_REQUIRED` if the immediate correction cannot preserve the ratified
request/product contract without taking B3/B4 authority. Do not blur independent
request identities, treat same-component products as one transaction, or hide a
mixed result behind a guard.
