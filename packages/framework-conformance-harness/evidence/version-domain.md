# Exact framework version-domain manifest

Resolved 2026-08-12; Vue domain re-resolved 2026-08-21 (v3.6.0-rc.3 → v3.6.0-rc.5)
and again 2026-11-19 (v3.6.0-rc.5 → v3.6.0-rc.15), one pinned domain
workspace-wide. The package locks are the complete machine-readable dependency
closures; this document records their source and direct-package identity.

**Scope of "re-resolved workspace-wide."** This re-resolution covers the
runtime-output oracle domain this document and `domain-pin.mjs` govern: the
package closures above and everything the `packages/framework-conformance-harness`
goldens/corpus are generated against. It does NOT cover the separate, already-frozen
BF1/BF2 official-case-manifest evidence package (`vue-official-cases.tsv`,
`vue-options.tsv`, `option-inventories.md`, `generate-official-case-manifests.mjs`
and `verify-b2-parse-facets.mjs`'s `EXPECTED_VUE`, and the corresponding
`performance-gates.toml` BF2 cell), which is pinned to its own separate,
already-measured source commit as part of a not-yet-ratified AMD-005 proposal
(`capability-matrix.md`, `ssr-hydration.md`) and is not re-derived on every
runtime-oracle version bump. Moving that package to track a new Vue commit needs a
genuine BF1/BF2 re-certification (a fresh pinned checkout, regenerated manifests,
and re-measured performance cells under BF2's own methodology), not a label edit —
see `capability-matrix.tsv`'s `compatibility_domain` column, which stays
`core@3.6.0-rc.3` deliberately, matching that frozen package and its governing
(unratified) contract prose.

## Vue

- upstream: `https://github.com/vuejs/core`
- exact tag: `v3.6.0-rc.15`
- tag kind: lightweight
- commit: `1595469ef66e4126351609f7df8ef334560aa3fa`
- tree: `a417748239111b19c9f78380531fda9ef13f7c6c`
- package lock: `oracles/vue/package-lock.json`
- lock SHA-256: `418ffca6ed806b30f93c82b8358720a3b13618ab35f5703cac9b310f3a5ea44a`
- exact resolved closure: `oracles/vue/closure.tsv`
- closure SHA-256: `db40b4813c2abd9e10a37595b0c5ada635a52c770079c3fd01ca360cda380da5`
- package manifest SHA-256: `dfcc97ba5106eb4467c07256b0a83482ec0d5f72942c76c5c67c6c2019b08654`
- resolver: npm `10.8.2` (invoked as `npx npm@10.8.2`), lockfile v3, Node `v26.5.0`,
  exact direct versions, `--package-lock-only --ignore-scripts --legacy-peer-deps --no-audit --no-fund`
- closure: 25 non-root packages, fully named/versioned/integrity-bound in the lock

| direct package | version | npm integrity |
|---|---|---|
| `vue` | `3.6.0-rc.15` | `sha512-A1XZEGZqt3xozb/LcWROmRc15CvjcWpPc0zie4eAXnYrEuZde2yeZINUA8U54LlvcUZOCZGR1OdLI1yoa0X6VA==` |
| `@vue/compiler-core` | `3.6.0-rc.15` | `sha512-VMgoQYzsmwxn6t9iPY3XpQYypOUkHdGIp1JaTah9WPrFY9jLXiYcYD7c2pK8MIOLeqlTpqpSnc1VZoXeWAmjew==` |
| `@vue/compiler-dom` | `3.6.0-rc.15` | `sha512-TLIO+IR53l2vMNu/16jMLJ9JgjhCuxYE9gywiUr5UqoNYCQnqM0G0XLw+Rtu4vMXENeXDrHp/Np1r6TQ3YP6Sw==` |
| `@vue/compiler-sfc` | `3.6.0-rc.15` | `sha512-MfOVqyvO28j2AZCt51ccvEbrtJM/VWGic19gtMO9Zr6hGQ7bvCxx2U4WtYFWiNaq8DXtGTqGpUc72MjjxUm5aA==` |
| `@vue/compiler-ssr` | `3.6.0-rc.15` | `sha512-+jVVmJb24EV+ad7KnO6bjX9iQW1vQ4LhZbIMHK5+NQ5dTn04SGuWEbQ7iGnAzFpRiKqpqPXuZg80BN/+cK5VOA==` |
| `@vue/compiler-vapor` | `3.6.0-rc.15` | `sha512-VetAFW1F0LpMAt+JJMFMeAgaePyRRzQjElXwpl19/EfIlC8/OVScunKSlfpZs8jK+x3qLNNFvJHe47Ho4k2Ygw==` |
| `@vue/runtime-core` | `3.6.0-rc.15` | `sha512-Ub3yACqYogmG4oTiCGlw3syKHPanZM9USDc5DzzZrTfCRfzy3vYnyABvRS412RMivL9VDNED/FtMo5ZirXvHow==` |
| `@vue/runtime-dom` | `3.6.0-rc.15` | `sha512-ABXTEhclPU2IRw6T7U1c2vo2Z9l/1EaVA5OmqKyuxl8hiXPwpLyZxgDjOJNg28SGwukYRpUJBRSJxwvgTdJ7WQ==` |
| `@vue/runtime-vapor` | `3.6.0-rc.15` | `sha512-J778jb0bvqvJTsLMUTMKqzX/3prgXqgrmmDBDf2jvodoRW7SfDhWCkKsbUhIPUpxnhAzDlRf6/fvGWPix8x8xQ==` |
| `@vue/server-renderer` | `3.6.0-rc.15` | `sha512-KAhQFtrwOW0V7MYaqS7eWBh7fCjD7IJybZ6Qigiws0XgirxEPG4Oo7NFZGcCji+j/2otlq1J9g1QryEjsHHQ7A==` |
| `@vue/reactivity` | `3.6.0-rc.15` | `sha512-qiDqzBA3ntWab6PYOS8MIWMdd276/rYB4zzeeh8iVUvgU/6L8VaEN5arCK0eGp7j0+Q3+mSIOQwdL9opQuvO9A==` |
| `@vue/shared` | `3.6.0-rc.15` | `sha512-yvK8qzcF0RFAd/QTUcVzNhBpRsY9JJFwRy9AnSwQV5juyymCr4BewPSO3sd6OMqJepqwqS0Xk0yaebIEyDmftA==` |

The remaining 13 transitive packages and every regular/optional/peer dependency edge
are exact in `closure.tsv`. Upstream range text retained as package metadata in npm's
lock is never used as a selector: installation is lock-bound, and the closure records
only resolved exact versions or an explicit omitted optional peer. Vue VDOM and Vapor
remain distinct capability families. The RC domain can never establish Stable
maturity.

## Svelte

- upstream: `https://github.com/sveltejs/svelte`
- exact tag: `svelte@5.56.10`
- annotated tag object: `75870e2a3e643af19fb7baabf754875942464510`
- commit: `56a036f4ce873a24ee6631a06d03d372523d7a9b`
- tree: `b7ced8028848a6c63bd8ff04177a43223ec89518`
- package: `svelte@5.56.10`
- integrity: `sha512-Lcxbj8I/KAbpY+VjtY4ENQBV0dDCipfGAhqb51XQZ67CIQqXgsv/8dPkbILaj4Fb6/b6JAEM/PIVbILXgDQy2g==`
- package lock: `oracles/svelte/package-lock.json`
- lock SHA-256: `110dbc95cb501f60177dec712df81a85e5fc8b3dda7a2592dfe5bd26b21d2053`
- exact resolved closure: `oracles/svelte/closure.tsv`
- closure SHA-256: `01e810a3c8ea5a286915071ebb04af86c1b735b1f9ef7b5db5f7605312c7a3e2`
- package manifest SHA-256: `9877a609a49bb008a4f529cb92a6fed03ef77c3e6cb7221a864dc0832bc39939`
- resolver: npm `10.8.2`, lockfile v3, Node `v20.20.2`, with the same flags as Vue
- closure: 20 non-root packages, fully named/versioned/integrity-bound in the lock

Every regular/optional/peer dependency edge is materialized with its exact resolved
version, or an explicit omitted optional-peer marker, in `closure.tsv`. No range or
dist-tag is consulted during conformance execution.

## Enforcement

BF1 verifies source commit/tree against local immutable mirrors and verifies every
lock entry's version/integrity. BF2 installs from committed locks into disposable
stores, forbids lifecycle scripts and network during tests, and records resolved tree
digests. Any mismatch is a domain failure, not an upgrade opportunity.
