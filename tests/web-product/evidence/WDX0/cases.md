# WDX0 evidence index

Selected case IDs: `WDX0-AC1`, `WDX0-AC2`, `WDX0-AC3`, `WDX0-AC4`, `WDX0-AC5`, `WDX0-ratification`.

Commands (run on the candidate; do not treat this file as an execution transcript):

- `node tests/web-product/WDX0/verify.mjs`
- `node --test tests/web-product/WDX0/wdx0.test.mjs`
- `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
- the remaining `docs-domain` final profile commands (see `catalogs/gate-profiles.toml`)

Deletion population this node: empty. The boundary is additive; no route is displaced or retired by WDX0.

AC1 grounding: the 41-row disposition population and the 39-edge qualification fan-in were extracted from the ratified program DAG export (direct successors of WDX0; WDX2 predecessor set) and pinned in the machine product; the harness re-checks them as closed populations.

AC2 grounding: the two-final-owners, train-owns-two-recommendations, proof-claimed-as-support, runtime-claim-at-ratification, exclusion-without-reason and catalog-family/count-drift twins are planted negatives; the catalog check grounds itself in the live `catalogs/product-surface-catalog.toml` (24 `vue.*`/`svelte.*` surfaces at ratification), so an added-portfolio family advertised before its terminal is a failing case, not a stale pass.

AC3 is bound to WDX1/WDX2/WDX3 as downstream runtime-test owners (docs-only node, 0 production LOC); AC5 records no budget and the empty deletion population; AC4 specifies the producers' obligations (per-head VIM/DX capability and host/profile evidence, DOC1-tested `examples/reference`, WDX3 checks completed artifacts).

This file does not claim the cases executed. Copying the charter here is not evidence.
