# DOC7 evidence index

Selected case IDs: `DOC7-AC1`, `DOC7-AC2`, `DOC7-AC3`, `DOC7-AC4`, `DOC7-AC5`.

Commands (run on the candidate; do not treat this file as an execution transcript):

- `node --test tests/documentation/DOC7/doc7.test.mjs`
- `node --test tests/documentation/DOC1/doc1.test.mjs`
- `node docs/scripts/reference-harness.mjs`
- `pnpm --filter docs check`

Deletion population this node: empty. The SDK guide structure home, the
`sdkGuides` manifest section and the SDK documentation gate are additive;
no previous SDK/extension guide route existed to displace. Shipped editor
pages under `docs/editor` remain the official-extension truth.

AC1: the DOC1 harness stays the sole owning interface. The guide model is
declared in `examples/reference/manifest.json` (`sdkGuides`) and validated
only through `validate()`; the docs package check invokes the same script.

AC2: a supplied slot bound to a static JSON-only sample fails with
`static-sdk-example`; a supplied example without a shipped command or
public import fails with `sdk-example-without-entry`; `--sdk-gate` rejects
any pending topic with `sdk-gate-unsatisfied` and passes only when all six
topics bind executable examples.

AC3: sdk pages join the source digest, so fresh versus incremental digest
equality, edit/revert and perturbed-discovery canonical equality are proven
on the extended boundary; a missing topic page is a partial result;
cancellation reuses the harness abort paths.

AC4: `examples/reference/sdk` is the DOC1-tested public guide structure;
host/profile, permissions, uncertainty and migration notes are in
`sdk-docs-model.v1.json`. All six slots are recorded pending — no page
claims a working SDK example.

AC5: no performance budget is bound; empty deletion population.

This file does not claim the cases executed. Copying the charter here is
not evidence.
