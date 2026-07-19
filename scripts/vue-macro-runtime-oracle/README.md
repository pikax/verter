# Vue macro runtime oracle

This oracle captures normalized runtime facts from the repository-pinned local
`@vue/compiler-sfc@3.5.34`. It never uses the network and does not compare
cosmetic JavaScript formatting.

`oracle-lib.mjs` compiles the sources in `fixtures.mjs`, parses the generated
module, and records schema-v2 facts in
`crates/verter_session/tests/fixtures/vue_macro_runtime_oracle.json`. Provenance
includes the compiler version, compile-profile options, and a SHA-256 fingerprint
of every source/support-file/profile/contract input. Runtime rows retain
constructor order, `required`, `skipCheck`, literal-safe defaults plus
`defaultKind`, and `typePresent` (which distinguishes omitted `type` from
explicit `type: null`).

The profile matrix covers development, production, and production custom
elements. `verter-complete-extension` fixtures establish an official baseline;
Verter may improve that shape only when its canonical result is `Complete`.

Regenerate after intentional fixture or pinned-compiler changes:

```bash
node scripts/gen-vue-macro-runtime-oracle.mjs
```

Verify drift and normalization:

```bash
node scripts/gen-vue-macro-runtime-oracle.mjs --check
node --test scripts/vue-macro-runtime-oracle/oracle.test.mjs
```

Do not edit the generated JSON by hand.
