// Exact, immutable official-core compatibility domains this harness may
// invoke. Mirrors
// `docs/arch/refactor/rev11/evidence/framework-conformance/version-domain.md`
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
  tag: "v3.6.0-rc.3",
  commit: "3adb225775c9b28223a56e07f7a2f874b6fbb138",
  tree: "36da8dc8841a35d3e1163e4b9bb5752f95ca527a",
  packageVersion: "3.6.0-rc.3",
  // Direct-package identities, exactly as recorded in version-domain.md.
  directPackages: Object.freeze({
    vue: "sha512-SsLCdsc8WoOJC1KHsMxvkVFjKmVpurF2DZJSy5A8sOSBR6ar1cQ370j2TBO80MW7ct80aHh0oQWU9BzMo8H9Qg==",
    "@vue/compiler-core":
      "sha512-WtpFH7AYGbw7K1AbUKkLxYRfrp0+0kB5RHMlEeTk5sKGcwSV+sNZQbq7R3Ybaq55XLjPCd0QF7TG3AQauGoIiQ==",
    "@vue/compiler-dom":
      "sha512-n/3HTAcXwNdwrx8eS1JUwCw4wbS+gPi8hIM7WcoTvHqgYJL5xhfChsmJQtzkX24Lweu7strPsNSbNsf/S3D3WQ==",
    "@vue/compiler-sfc":
      "sha512-+QT0wGQixwrkvG+qGEY2SkzUJJw1M3KlXtJ+xFHeZXZrPmvLWVAt/4B/G/H0gVWa8SiqOZLedI7ADqmjgm7Q6Q==",
    "@vue/compiler-ssr":
      "sha512-iywY3ipWer9pJ6Xa5vQ1sGd/hT0cGPDn7m5zwJDKnBcflSt4pfktE+xl2t0cSFs4/mTHEevuz5xamdyCJ2L6KQ==",
    "@vue/compiler-vapor":
      "sha512-wMdb1WpwosxWl3sNOYLPw9DgL+AzSdaJWnBi5GEvR1ajqb7mY3Ivenvs5QIBGXRbNYKrQBqfdkBWH/3xNWIXwQ==",
    "@vue/runtime-core":
      "sha512-uGD8nlft/+wKALxpSDzItg1ICtNMQkkOjCurmG9evTVgerBmkm0RUmZGHlIaKVECLizKBpf7s+p0NaH9yZJfLA==",
    "@vue/runtime-dom":
      "sha512-/cB2vZhcGFhl+YYxwsJyFB1KjVFKK29JATuJzSQxhlXbCD+kAwJ1ZJB615RS9Yd5mC9hooM65G9clrbD9LlXHA==",
    "@vue/runtime-vapor":
      "sha512-4OrYk9KWBz71axcmDTPh1TiGG84dq937Olj3qlGp9rklVwUL0f+7w1dZSfWPzV4Y/d8ye6WgLfNmntqBOX094g==",
    "@vue/server-renderer":
      "sha512-YCKcCMz7NY92Wp6Ugv7JBFHqgbdteIC6CM3TzMMbJ8uB56sUXrF7qJRh3z6AyH3FESycFdXnUSIwNhkmjL5hfg==",
    "@vue/reactivity":
      "sha512-+Uvp1i+oozwkyVy2HGUhmA23QDO/YY+QyBm32oddZyG6+FEaEANG7NCQr+asSJzNHWAZmZo97zVNai0tOBdJRw==",
    "@vue/shared":
      "sha512-EFnGq/OonnFgOtgAhXLIv8owITuFsaGglKXjsAUJQ+2uVuCPxypdW7NIZUlt7ED2raM1Hn/C83eTeK0tZVGCZw==",
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
  tag: "svelte@5.56.8",
  tagObject: "a49603bbb50f948fd0c2bf5c55582a8f89b4d91c",
  commit: "44a7813730579b94004e182e5a67aab27aa9d2a6",
  tree: "63390158bfe8f997c474e35215a4fa627194c229",
  packageVersion: "5.56.8",
  directPackages: Object.freeze({
    svelte:
      "sha512-PY8LOw7xP6c8IOiVqdo0sbbZVYhXRSfklOQLAUyGBKqjTX0wx/z4l/9J+PmBpmlLnxzEb1NqltxQ5/wZme/Cmg==",
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
  vuePackageLockSha256: "0dd2290c0b7d01f4727953b838610727b18bcb999b634eeb8ab726508a34b951",
  vueClosureSha256: "d5caba234d8545b8b7bc7cc4cca8b8cf63f8ed594140d7cae80f3c7ae64606b2",
  sveltePackageLockSha256: "0c27c9fc7bed24be3fd7a546b55b6ee5858b244a57613390a213fdb454b92ce2",
  svelteClosureSha256: "3dc4209c2911700de92858e350ddda2e6f5f333874a2eb330125ee808910dbce",
});
