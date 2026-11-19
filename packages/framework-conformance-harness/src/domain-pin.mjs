// Exact, immutable official-core compatibility domains this harness may
// invoke. Mirrors
// `packages/framework-conformance-harness/evidence/version-domain.md`
// byte-for-byte. Sole authority other harness modules read pin identities
// from; nothing here is fetched at runtime, and nothing here may be
// widened to a range or dist-tag (official-core-oracles.md).
//
// A newer Vue RC, Vue stable, or Svelte release is a distinct compatibility
// domain requiring its own amendment and regenerated evidence. No upgrade
// path.

export const VUE_DOMAIN = Object.freeze({
  framework: "vue",
  upstream: "https://github.com/vuejs/core",
  tag: "v3.6.0-rc.15",
  commit: "1595469ef66e4126351609f7df8ef334560aa3fa",
  tree: "a417748239111b19c9f78380531fda9ef13f7c6c",
  packageVersion: "3.6.0-rc.15",
  // Direct-package identities, exactly as recorded in version-domain.md.
  directPackages: Object.freeze({
    vue: "sha512-A1XZEGZqt3xozb/LcWROmRc15CvjcWpPc0zie4eAXnYrEuZde2yeZINUA8U54LlvcUZOCZGR1OdLI1yoa0X6VA==",
    "@vue/compiler-core":
      "sha512-VMgoQYzsmwxn6t9iPY3XpQYypOUkHdGIp1JaTah9WPrFY9jLXiYcYD7c2pK8MIOLeqlTpqpSnc1VZoXeWAmjew==",
    "@vue/compiler-dom":
      "sha512-TLIO+IR53l2vMNu/16jMLJ9JgjhCuxYE9gywiUr5UqoNYCQnqM0G0XLw+Rtu4vMXENeXDrHp/Np1r6TQ3YP6Sw==",
    "@vue/compiler-sfc":
      "sha512-MfOVqyvO28j2AZCt51ccvEbrtJM/VWGic19gtMO9Zr6hGQ7bvCxx2U4WtYFWiNaq8DXtGTqGpUc72MjjxUm5aA==",
    "@vue/compiler-ssr":
      "sha512-+jVVmJb24EV+ad7KnO6bjX9iQW1vQ4LhZbIMHK5+NQ5dTn04SGuWEbQ7iGnAzFpRiKqpqPXuZg80BN/+cK5VOA==",
    "@vue/compiler-vapor":
      "sha512-VetAFW1F0LpMAt+JJMFMeAgaePyRRzQjElXwpl19/EfIlC8/OVScunKSlfpZs8jK+x3qLNNFvJHe47Ho4k2Ygw==",
    "@vue/runtime-core":
      "sha512-Ub3yACqYogmG4oTiCGlw3syKHPanZM9USDc5DzzZrTfCRfzy3vYnyABvRS412RMivL9VDNED/FtMo5ZirXvHow==",
    "@vue/runtime-dom":
      "sha512-ABXTEhclPU2IRw6T7U1c2vo2Z9l/1EaVA5OmqKyuxl8hiXPwpLyZxgDjOJNg28SGwukYRpUJBRSJxwvgTdJ7WQ==",
    "@vue/runtime-vapor":
      "sha512-J778jb0bvqvJTsLMUTMKqzX/3prgXqgrmmDBDf2jvodoRW7SfDhWCkKsbUhIPUpxnhAzDlRf6/fvGWPix8x8xQ==",
    "@vue/server-renderer":
      "sha512-KAhQFtrwOW0V7MYaqS7eWBh7fCjD7IJybZ6Qigiws0XgirxEPG4Oo7NFZGcCji+j/2otlq1J9g1QryEjsHHQ7A==",
    "@vue/reactivity":
      "sha512-qiDqzBA3ntWab6PYOS8MIWMdd276/rYB4zzeeh8iVUvgU/6L8VaEN5arCK0eGp7j0+Q3+mSIOQwdL9opQuvO9A==",
    "@vue/shared":
      "sha512-yvK8qzcF0RFAd/QTUcVzNhBpRsY9JJFwRy9AnSwQV5juyymCr4BewPSO3sd6OMqJepqwqS0Xk0yaebIEyDmftA==",
  }),
  // The EXACT oracle module specifiers production code loads from this
  // domain's realized install, with the loader each callsite uses — the
  // authoritative inventory the entry-resolvability gate must prove
  // loadable (oracle-install.mjs). Resolving a package's ROOT entry proves
  // nothing about a divergent subpath/condition target (svelte/compiler's
  // `require` and `default` exports point at DIFFERENT files), so the gate
  // resolves each row under the same loader semantics as its caller.
  // Derived from the real callers; keep in sync when a loader callsite is
  // added or removed:
  //   - invoke-vue-oracle.mjs:       oracleRequire("vue", "@vue/compiler-sfc")
  //   - execute-vue-runtime.mjs:     importOracleModule("vue", "vue"),
  //                                  importOracleModule("vue", "@vue/server-renderer")
  //   - hydration.mjs (hydrateVue):  importOracleModule("vue", "vue")
  oracleLoadSpecifiers: Object.freeze([
    Object.freeze({ specifier: "@vue/compiler-sfc", loader: "require" }),
    Object.freeze({ specifier: "vue", loader: "import" }),
    Object.freeze({ specifier: "@vue/server-renderer", loader: "import" }),
  ]),
});

export const SVELTE_DOMAIN = Object.freeze({
  framework: "svelte",
  upstream: "https://github.com/sveltejs/svelte",
  tag: "svelte@5.56.10",
  tagObject: "75870e2a3e643af19fb7baabf754875942464510",
  commit: "56a036f4ce873a24ee6631a06d03d372523d7a9b",
  tree: "b7ced8028848a6c63bd8ff04177a43223ec89518",
  packageVersion: "5.56.10",
  directPackages: Object.freeze({
    svelte:
      "sha512-Lcxbj8I/KAbpY+VjtY4ENQBV0dDCipfGAhqb51XQZ67CIQqXgsv/8dPkbILaj4Fb6/b6JAEM/PIVbILXgDQy2g==",
  }),
  // See VUE_DOMAIN.oracleLoadSpecifiers for the field contract. Real
  // callers this inventory is derived from:
  //   - invoke-svelte-oracle.mjs:     importOracleModule("svelte", "svelte/compiler")
  //   - execute-svelte-runtime.mjs:   importOracleModule("svelte", "svelte/server")
  //   - hydration.mjs (hydrateSvelteClient): the generated runner imports
  //     "svelte" from inside the install tree in a child launched with
  //     `--conditions=browser`, hence the extra condition on that row.
  oracleLoadSpecifiers: Object.freeze([
    Object.freeze({ specifier: "svelte/compiler", loader: "import" }),
    Object.freeze({ specifier: "svelte/server", loader: "import" }),
    Object.freeze({
      specifier: "svelte",
      loader: "import",
      extraConditions: Object.freeze(["browser"]),
    }),
  ]),
});

/** Committed evidence-lock digests this harness cross-checks itself against. */
export const EVIDENCE_LOCK_DIGESTS = Object.freeze({
  vuePackageLockSha256: "418ffca6ed806b30f93c82b8358720a3b13618ab35f5703cac9b310f3a5ea44a",
  vueClosureSha256: "db40b4813c2abd9e10a37595b0c5ada635a52c770079c3fd01ca360cda380da5",
  sveltePackageLockSha256: "110dbc95cb501f60177dec712df81a85e5fc8b3dda7a2592dfe5bd26b21d2053",
  svelteClosureSha256: "01e810a3c8ea5a286915071ebb04af86c1b735b1f9ef7b5db5f7605312c7a3e2",
});
