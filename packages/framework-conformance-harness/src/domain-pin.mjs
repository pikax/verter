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
  tag: "v3.6.0-rc.5",
  commit: "f11c8f2639ce15559d64ea054e409081bd8a0ce1",
  tree: "980693b602cff54d492a1d6ada18470596cbf978",
  packageVersion: "3.6.0-rc.5",
  // Direct-package identities, exactly as recorded in version-domain.md.
  directPackages: Object.freeze({
    vue: "sha512-yM+CHEWSTc9FjJGIeViI86VVheHvJ3YaZrrXqlD7wX3S+8tNPR/vDMviGOv4ULIMTkzWWKWVRvylsytXbHBbNA==",
    "@vue/compiler-core":
      "sha512-OSOzR/4Mk8TMStNxFLFwcVjgFvvMGvlKEpboxv9W4ikQhsVLKEMTtzBVY5A11qwb6zGuwWJdCOeME5npmpURiQ==",
    "@vue/compiler-dom":
      "sha512-QBONzGYH7o448rwz+8FUWW4Gm4Zw0EtNhtRooOw/KDFF+/hWz1VlGIpvU9Hjv5MXDMMCu+UsLXEYFtXTSHgIwg==",
    "@vue/compiler-sfc":
      "sha512-o/IH60kRS8C06ek3tullJhm4sK3T6aDXQa8Dgq7qLxRCa5gXrIZMDO9+mZYy0THxAiTZs2tc/XwnKu0JqmSKRw==",
    "@vue/compiler-ssr":
      "sha512-KBsxaO538LZeNARcaYeEwOE0Fl/gw2mEYB9+hK/Hrk7yUCq4WeS9V32HL84SiTY4S6WXrcKP0pXB6zW6zvjB6w==",
    "@vue/compiler-vapor":
      "sha512-UXnYH+4NhPmEmlWcHuiR+KjfpZCuG1CBkWXTSH5720p2jRGzuEGiMUAx7CtMXyIJ0QZwdzDV09xFs35zsgJeYA==",
    "@vue/runtime-core":
      "sha512-NiT9xl/ndkTHASfQ9AjxDjTiClIZRmsIWb0orlKNnjHn6C09PNZX4V/c0Aewtlg8bouarpnV6JLbpg16gYMBJA==",
    "@vue/runtime-dom":
      "sha512-E5A1z7UEoPvAmIpZopSJ5ji8A1wuP2cFHVc41ZN2w32FWV3CxFQVJG3VNSHwFs8lBQ8Ji5SDnkMCfJXHVzt0iQ==",
    "@vue/runtime-vapor":
      "sha512-OmBf4R/SJ11h9ZXrpPThddh2SqTmyc9eCBLFSmH1rhfr/sVFMrrcxMXjFOX1Rn+Nlqiuf9pUx/hYwG2gY2uJHA==",
    "@vue/server-renderer":
      "sha512-esb8yrZjymuMO7Wqjp62B2cFCGvL1AkmlIp8KBsKowG+BOqzemOJHz1yhK7Tf3KE0LIEatDP/Gb4FZo+S/LwyQ==",
    "@vue/reactivity":
      "sha512-FcTNjZwCU4VPAv7W/EJD/ckatgxFJ20jU6S2dGmJC9RS08HAvKB/IjtCQaE7HBuIC4oXQnnahkNuilrDFt0BWA==",
    "@vue/shared":
      "sha512-2dQ2+xAv7USEKgM5ckB2PrNc4pBcqYNCmkk8/RQtbpxpNDK0RvH0c9vG4rgqsvFS4wy3RXyj2ZfoAhldkgZ2dw==",
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
  vuePackageLockSha256: "4c3cc2fb175c4cba390e319aeae04dce6252ac818a2045f8383a040b488430a2",
  vueClosureSha256: "6af174230488ff2d6d054550d81f3a96218c137046abf37a9dc9d27639d9ea07",
  sveltePackageLockSha256: "110dbc95cb501f60177dec712df81a85e5fc8b3dda7a2592dfe5bd26b21d2053",
  svelteClosureSha256: "01e810a3c8ea5a286915071ebb04af86c1b735b1f9ef7b5db5f7605312c7a3e2",
});
