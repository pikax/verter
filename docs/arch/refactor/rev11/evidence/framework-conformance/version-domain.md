# Exact framework version-domain manifest

Resolved 2026-08-12, Vue domain re-resolved 2026-08-21 (v3.6.0-rc.3 → v3.6.0-rc.5,
one pinned domain workspace-wide). The package locks are the complete
machine-readable dependency closures; this document records their source and
direct-package identity.

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
- exact tag: `v3.6.0-rc.5`
- tag kind: lightweight
- commit: `f11c8f2639ce15559d64ea054e409081bd8a0ce1`
- tree: `980693b602cff54d492a1d6ada18470596cbf978`
- package lock: `oracles/vue/package-lock.json`
- lock SHA-256: `4c3cc2fb175c4cba390e319aeae04dce6252ac818a2045f8383a040b488430a2`
- exact resolved closure: `oracles/vue/closure.tsv`
- closure SHA-256: `6af174230488ff2d6d054550d81f3a96218c137046abf37a9dc9d27639d9ea07`
- package manifest SHA-256: `ffd153afc3d7f814c727b960698a8202237120fe970d0f7249148d8001d31f5f`
- resolver: npm `10.8.2`, lockfile v3, Node `v20.20.2`, exact direct versions,
  `--package-lock-only --ignore-scripts --legacy-peer-deps --no-audit --no-fund`
- closure: 25 non-root packages, fully named/versioned/integrity-bound in the lock

| direct package | version | npm integrity |
|---|---|---|
| `vue` | `3.6.0-rc.5` | `sha512-yM+CHEWSTc9FjJGIeViI86VVheHvJ3YaZrrXqlD7wX3S+8tNPR/vDMviGOv4ULIMTkzWWKWVRvylsytXbHBbNA==` |
| `@vue/compiler-core` | `3.6.0-rc.5` | `sha512-OSOzR/4Mk8TMStNxFLFwcVjgFvvMGvlKEpboxv9W4ikQhsVLKEMTtzBVY5A11qwb6zGuwWJdCOeME5npmpURiQ==` |
| `@vue/compiler-dom` | `3.6.0-rc.5` | `sha512-QBONzGYH7o448rwz+8FUWW4Gm4Zw0EtNhtRooOw/KDFF+/hWz1VlGIpvU9Hjv5MXDMMCu+UsLXEYFtXTSHgIwg==` |
| `@vue/compiler-sfc` | `3.6.0-rc.5` | `sha512-o/IH60kRS8C06ek3tullJhm4sK3T6aDXQa8Dgq7qLxRCa5gXrIZMDO9+mZYy0THxAiTZs2tc/XwnKu0JqmSKRw==` |
| `@vue/compiler-ssr` | `3.6.0-rc.5` | `sha512-KBsxaO538LZeNARcaYeEwOE0Fl/gw2mEYB9+hK/Hrk7yUCq4WeS9V32HL84SiTY4S6WXrcKP0pXB6zW6zvjB6w==` |
| `@vue/compiler-vapor` | `3.6.0-rc.5` | `sha512-UXnYH+4NhPmEmlWcHuiR+KjfpZCuG1CBkWXTSH5720p2jRGzuEGiMUAx7CtMXyIJ0QZwdzDV09xFs35zsgJeYA==` |
| `@vue/runtime-core` | `3.6.0-rc.5` | `sha512-NiT9xl/ndkTHASfQ9AjxDjTiClIZRmsIWb0orlKNnjHn6C09PNZX4V/c0Aewtlg8bouarpnV6JLbpg16gYMBJA==` |
| `@vue/runtime-dom` | `3.6.0-rc.5` | `sha512-E5A1z7UEoPvAmIpZopSJ5ji8A1wuP2cFHVc41ZN2w32FWV3CxFQVJG3VNSHwFs8lBQ8Ji5SDnkMCfJXHVzt0iQ==` |
| `@vue/runtime-vapor` | `3.6.0-rc.5` | `sha512-OmBf4R/SJ11h9ZXrpPThddh2SqTmyc9eCBLFSmH1rhfr/sVFMrrcxMXjFOX1Rn+Nlqiuf9pUx/hYwG2gY2uJHA==` |
| `@vue/server-renderer` | `3.6.0-rc.5` | `sha512-esb8yrZjymuMO7Wqjp62B2cFCGvL1AkmlIp8KBsKowG+BOqzemOJHz1yhK7Tf3KE0LIEatDP/Gb4FZo+S/LwyQ==` |
| `@vue/reactivity` | `3.6.0-rc.5` | `sha512-FcTNjZwCU4VPAv7W/EJD/ckatgxFJ20jU6S2dGmJC9RS08HAvKB/IjtCQaE7HBuIC4oXQnnahkNuilrDFt0BWA==` |
| `@vue/shared` | `3.6.0-rc.5` | `sha512-2dQ2+xAv7USEKgM5ckB2PrNc4pBcqYNCmkk8/RQtbpxpNDK0RvH0c9vG4rgqsvFS4wy3RXyj2ZfoAhldkgZ2dw==` |

The remaining 13 transitive packages and every regular/optional/peer dependency edge
are exact in `closure.tsv`. Upstream range text retained as package metadata in npm's
lock is never used as a selector: installation is lock-bound, and the closure records
only resolved exact versions or an explicit omitted optional peer. Vue VDOM and Vapor
remain distinct capability families. The RC domain can never establish Stable
maturity.

## Svelte

- upstream: `https://github.com/sveltejs/svelte`
- exact tag: `svelte@5.56.8`
- annotated tag object: `a49603bbb50f948fd0c2bf5c55582a8f89b4d91c`
- commit: `44a7813730579b94004e182e5a67aab27aa9d2a6`
- tree: `63390158bfe8f997c474e35215a4fa627194c229`
- package: `svelte@5.56.8`
- integrity: `sha512-PY8LOw7xP6c8IOiVqdo0sbbZVYhXRSfklOQLAUyGBKqjTX0wx/z4l/9J+PmBpmlLnxzEb1NqltxQ5/wZme/Cmg==`
- package lock: `oracles/svelte/package-lock.json`
- lock SHA-256: `0c27c9fc7bed24be3fd7a546b55b6ee5858b244a57613390a213fdb454b92ce2`
- exact resolved closure: `oracles/svelte/closure.tsv`
- closure SHA-256: `3dc4209c2911700de92858e350ddda2e6f5f333874a2eb330125ee808910dbce`
- package manifest SHA-256: `ac1b539596a6ea3e1151b00720edaf73c42a4aab4aac5caafb1079e858a6578a`
- resolver: same npm/Node command as Vue
- closure: 20 non-root packages, fully named/versioned/integrity-bound in the lock

Every regular/optional/peer dependency edge is materialized with its exact resolved
version, or an explicit omitted optional-peer marker, in `closure.tsv`. No range or
dist-tag is consulted during conformance execution.

## Enforcement

BF1 verifies source commit/tree against local immutable mirrors and verifies every
lock entry's version/integrity. BF2 installs from committed locks into disposable
stores, forbids lifecycle scripts and network during tests, and records resolved tree
digests. Any mismatch is a domain failure, not an upgrade opportunity.
