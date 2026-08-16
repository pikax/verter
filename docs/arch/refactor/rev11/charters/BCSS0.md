# BCSS0 — Standalone CSS source-map product correction

**Status:** PROPOSED / **RATIFIED (AMD-009)**; not accepted. **Class:** Framework subsystem.
**Predecessor:** BF3. **Downstream:** acceptance is a mandatory predecessor of B2
and B3.

## Objective

Make the standalone CSS `sourcemap: true` option a real requested product for both
passthrough and transformed CSS branches before B2/B3.

## Owned scope

BCSS0 owns CSS-1 in `verter_compiler::css` and the standalone NAPI product boundary.
Both processing branches currently hard-code `source_map: None`; this block owns a
correct source map through the Rust result and the NAPI spelling. It does not belong
to BS0, BS1, or a host-route owner.

| finding | acceptance ID | correct-behavior target and characterization |
|---|---|---|
| CSS-1 | `BF3-CSS-1-STANDALONE-SOURCEMAP` → `FC-OPTIONS-001` | A separately named correct-behavior target must be added by BCSS0 implementation and is currently absent. `the_standalone_css_spelling_publishes_css_and_ignores_its_source_map_axis` remains the green inert-axis characterization until the axis is live; it is not the acceptance target. |

## Required procedure

First add a separately named public-boundary correct-behavior target that requires a
valid requested map on both processing branches and prove that new target RED. Do
not invert or use the green inert-axis characterization as the RED or acceptance
target; keep it unchanged until the axis is live. Implement the minimum shared
CSS-owner correction, carry the product through NAPI, and rerun map validity,
authored-source anchoring, option-off absence, passthrough/transformed, and
unrequested-product controls. No retraction or typed refusal may replace the product.

## Required exits

`BF3-CSS-1-STANDALONE-SOURCEMAP`, `FC-OPTIONS-001`, and the separately named
correct-behavior target pass: `sourcemap: true` publishes correct CSS plus a valid
authored-source map on both branches; `sourcemap: false` publishes no map; and the
Rust and standalone NAPI product sets agree. The green inert-axis characterization
is retained unchanged until the axis becomes live and is never counted as the
correction's acceptance target. Only BCSS0 acceptance satisfies this block's B2/B3
predecessor edge.

## What it must NOT do

BCSS0 must not implement Svelte, batch, transport, host-route, B3, B4, or later CSS
reconciliation work. It must not add production retraction, a defect-selected
refusal, a fixture-identity branch, generated-output string scanning, or another
source-map authority outside `verter_compiler::css` and the standalone NAPI boundary.

## Abort/rescope

Stop with `RESCOPE_REQUIRED` if a correct standalone product requires changing a
ratified public contract or taking B3/B4 authority. Do not silently ignore the
option, publish a placeholder map, or convert the request to non-success.
