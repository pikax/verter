# DOC2 evidence index

Selected case IDs: `DOC2-AC1`, `DOC2-AC2`, `DOC2-AC3`, `DOC2-AC4`, `DOC2-AC5`.

Commands (run on the candidate; do not treat this file as an execution transcript):

- `node --test tests/documentation/DOC2/doc2.test.mjs`
- `node --test tests/documentation/DOC1/doc1.test.mjs`
- `node tests/architecture-health/ARH1/verify.mjs`
- `node docs/scripts/reference-harness.mjs`
- `pnpm --filter docs check`
- `pnpm docs:build`

Predecessor ARH1 is ratified by `verify.mjs` (live counts). The
`arh1.test.mjs` `--provenance` case pins an ARH1 feature-branch merge
that is not an ancestor of the squash on main; that pin is ARH1-owned
and is not re-ratified here.

Deletion population this node: empty. The four contributor pages, their
model and the harness `contributorDocs` section are additive; no existing
page is superseded and no second capability matrix or semantic authority
is introduced.

AC1: `docs/scripts/reference-harness.mjs` (contributorDocs section) is the
sole owning interface; the model file
`tests/documentation/DOC2/products/contributor-docs-model.v1.json` is the
single machine declaration. Pages, citations, hotspot coverage, listing
and taught interfaces are all validated through it by the existing
`pnpm --filter docs check` command.

AC2: a taught interface under `docs/` fails `unowned-taught-path` (local
resolver); a second taught path for one capability fails `second-authority`
(duplicate cache, including a same-crate second file); substituting the
declared `semantic-query-memo` authority fails `authority-mismatch`; a
`test_`-prefixed symbol fails `test-only-api` (unsupported test-only API);
a symbol absent from its source fails `taught-symbol-missing`; a cited
repo path that vanished fails `broken-citation`.

AC3: N/A for production state/query/map boundaries (none touched; their
proofs stay with rev11.scheduler-runtime, rev11.flow, B4R0, LSO0). The
docs-boundary equivalents are proven: canonical-receipt equality under
reversed model arrays, incremental digest match, taught-source identity,
catalog-only identity (capabilityCatalog participates in the receipt
digest), edit/revert, pre-aborted and mid-page cancellation, missing-page
partial.

AC4: pages are public delivery under the DOC0 information architecture
(inventory rows appended, owner DOC2, audience contributor), linked from
`docs/contributing/index.md`. Each retained hotspot surface is mapped to
DOC0 catalog ids and listed on the architecture page; host/profile basis
names the ARH1 `candidate` SHA, records the original measurement host as
unrecorded in the contract products, and names the live `ubuntu-latest` /
`.nvmrc` architecture-health re-derivation. `examples/reference` is
consumed, not duplicated. Permissions N/A (no capability surface created).

AC5: no performance budget is bound; empty deletion population; no
required check dropped.

This file does not claim the cases executed. Copying the charter here is
not evidence.
