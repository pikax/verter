# Vendored Vue shims

`shims/` holds the **committed** `vue` / `@vue/*` TypeScript declaration shims the
differential baseline type-checks generated `.vue.tsx` against. They are vendored
(never installed at runtime) so the baseline stays hermetic — it builds and runs
with no third-party repository or `npm`/`pnpm` install alongside this repo.

The directory is named `shims/`, **not** `node_modules/`, on purpose: the repo-wide
`node_modules` gitignore rule would otherwise exclude these committed files. The
`verter_dx_baseline` materializer copies the contents of this directory into the
baseline workspace's `node_modules/` at materialize time, so the on-disk name here
is irrelevant to resolution.

Every package is pinned to the workspace Vue line (`VENDORED_VUE_VERSION` in
`src/vendorManifest.ts`); the materializer's strict vendored-Vue version sync
refuses any package whose version drifts from the `expectedVueVersion` computed
from `vue/package.json`. `buildVendorManifest()` produces a tamper-evident
checksum inventory of this tree.
