// ESM-only fixture for the `importEsm` loader test. The top-level `await` makes
// this module impossible to load with `require()` (even under Node's
// require-of-ESM support) — so a successful load PROVES `importEsm` performed a
// genuine dynamic `import()` and never collapsed to `require`.
await Promise.resolve();

export const marker = "esm-only-via-dynamic-import";

export default { marker };
