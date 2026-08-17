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
| AT-2 | `BF3-AT-2-BATCH-REFUSAL-ATOMICITY` → `FC-ATOMIC-001` | `a_genuinely_failing_batch_entry_publishes_no_partial_product` is the green characterization of the currently reachable genuine-conflict class, not a RED target. After RT-1 is corrected, BA0 must prove the newly reachable Svelte-refusal batch class and, when the green test already covers the reachable genuine-failure class as recorded, add a new separately named `#[ignore]`d correct-behavior target (currently absent). |

## Required procedure

For AT-1, enable the existing ignored public-boundary target over the canonical
request token, requested-product set, typed terminal result, and every publication
under that token, and prove its RED result discriminates partial publication. Keep
that target as the live `FC-ATOMIC-001` target.

For AT-2, do not require a RED cycle against
`a_genuinely_failing_batch_entry_publishes_no_partial_product`; it is already green
for the reachable genuine-conflict failure described in `dispositions.md`. Once
RT-1 is corrected, prove the Svelte-refusal batch class. If the current green test
already covers the reachable genuine-failure class, add a new separately named
ignored correct-behavior target for the Svelte-refusal class and prove that new
target RED. Then make the minimum common-layer correction, with no retraction
mechanism, and rerun the live targets, characterizations, batch-neighbour control,
and independent-product success controls.

## Required exits

Both acceptance IDs satisfy `FC-ATOMIC-001`: success publishes all and only the
products requested by that identity, refusal publishes none, and a failing batch
entry cannot publish a partial product or contaminate a neighbour. Separate IDE-only
or PublicApi-only requests may succeed under separate identities; a combined request
cannot publish those products beside its runtime refusal. AT-1's existing ignored
test is the live acceptance target. AT-2 proves the Svelte-refusal batch class after
RT-1 is corrected, using a new ignored correct-behavior target when the current green
test already covers the reachable genuine-failure class; the current green test
remains a characterization/control, not a RED target. All affected public routes
observe the same transaction boundary.

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
