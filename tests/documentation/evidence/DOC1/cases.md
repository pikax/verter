# DOC1 evidence index

Selected case IDs: `DOC1-AC1`, `DOC1-AC2`, `DOC1-AC3`, `DOC1-AC4`, `DOC1-AC5`.

Commands (run on the candidate; do not treat this file as an execution transcript):

- `node --test tests/documentation/DOC1/doc1.test.mjs`
- `node docs/scripts/reference-harness.mjs`
- `pnpm --filter docs check`
- `pnpm docs:build`

Deletion population this node: empty. The public example home and docs-build checks are additive. `examples/` and `packages/example` stay contributor estates.

AC1: `docs/scripts/reference-harness.mjs` is the sole owning interface. The docs package `check`/`build` scripts invoke it. Published examples live under `examples/reference` and are resolved through that interface against live `package.json` exports and shipped bins.

AC2: an example importing `packages/unplugin/src` fails; an example running `cargo run -p verter_tsc` fails. Shipped `@verter/unplugin/vite` and `verter-tsc` succeed.

AC3: fresh versus incremental digest equality, edit/revert, AbortSignal cancellation, missing-source partial, stale generated-page rejection, canonical equality under reversed discovery.

AC4: `examples/reference` is the tested public home. Pins, static-proof class, host/profile and uncertainty notes are in `reference-harness.v1.json`. No catalog family is added.

AC5: no performance budget is bound; empty deletion population.

This file does not claim the cases executed. Copying the charter here is not evidence.
