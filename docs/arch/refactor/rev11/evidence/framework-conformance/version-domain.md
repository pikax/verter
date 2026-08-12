# Exact framework version-domain manifest

Resolved 2026-08-12. The package locks are the complete machine-readable dependency
closures; this document records their source and direct-package identity.

## Vue

- upstream: `https://github.com/vuejs/core`
- exact tag: `v3.6.0-rc.3`
- tag kind: lightweight
- commit: `3adb225775c9b28223a56e07f7a2f874b6fbb138`
- tree: `36da8dc8841a35d3e1163e4b9bb5752f95ca527a`
- package lock: `oracles/vue/package-lock.json`
- lock SHA-256: `0dd2290c0b7d01f4727953b838610727b18bcb999b634eeb8ab726508a34b951`
- exact resolved closure: `oracles/vue/closure.tsv`
- closure SHA-256: `d5caba234d8545b8b7bc7cc4cca8b8cf63f8ed594140d7cae80f3c7ae64606b2`
- package manifest SHA-256: `df17ad96a1dc2b18783b2040e35bcd1e83239e8b7d4bd3255b5bdf2dbbf3b6e4`
- resolver: npm `10.8.2`, lockfile v3, Node `v20.20.2`, exact direct versions,
  `--package-lock-only --ignore-scripts --legacy-peer-deps --no-audit --no-fund`
- closure: 25 non-root packages, fully named/versioned/integrity-bound in the lock

| direct package | version | npm integrity |
|---|---|---|
| `vue` | `3.6.0-rc.3` | `sha512-SsLCdsc8WoOJC1KHsMxvkVFjKmVpurF2DZJSy5A8sOSBR6ar1cQ370j2TBO80MW7ct80aHh0oQWU9BzMo8H9Qg==` |
| `@vue/compiler-core` | `3.6.0-rc.3` | `sha512-WtpFH7AYGbw7K1AbUKkLxYRfrp0+0kB5RHMlEeTk5sKGcwSV+sNZQbq7R3Ybaq55XLjPCd0QF7TG3AQauGoIiQ==` |
| `@vue/compiler-dom` | `3.6.0-rc.3` | `sha512-n/3HTAcXwNdwrx8eS1JUwCw4wbS+gPi8hIM7WcoTvHqgYJL5xhfChsmJQtzkX24Lweu7strPsNSbNsf/S3D3WQ==` |
| `@vue/compiler-sfc` | `3.6.0-rc.3` | `sha512-+QT0wGQixwrkvG+qGEY2SkzUJJw1M3KlXtJ+xFHeZXZrPmvLWVAt/4B/G/H0gVWa8SiqOZLedI7ADqmjgm7Q6Q==` |
| `@vue/compiler-ssr` | `3.6.0-rc.3` | `sha512-iywY3ipWer9pJ6Xa5vQ1sGd/hT0cGPDn7m5zwJDKnBcflSt4pfktE+xl2t0cSFs4/mTHEevuz5xamdyCJ2L6KQ==` |
| `@vue/compiler-vapor` | `3.6.0-rc.3` | `sha512-wMdb1WpwosxWl3sNOYLPw9DgL+AzSdaJWnBi5GEvR1ajqb7mY3Ivenvs5QIBGXRbNYKrQBqfdkBWH/3xNWIXwQ==` |
| `@vue/runtime-core` | `3.6.0-rc.3` | `sha512-uGD8nlft/+wKALxpSDzItg1ICtNMQkkOjCurmG9evTVgerBmkm0RUmZGHlIaKVECLizKBpf7s+p0NaH9yZJfLA==` |
| `@vue/runtime-dom` | `3.6.0-rc.3` | `sha512-/cB2vZhcGFhl+YYxwsJyFB1KjVFKK29JATuJzSQxhlXbCD+kAwJ1ZJB615RS9Yd5mC9hooM65G9clrbD9LlXHA==` |
| `@vue/runtime-vapor` | `3.6.0-rc.3` | `sha512-4OrYk9KWBz71axcmDTPh1TiGG84dq937Olj3qlGp9rklVwUL0f+7w1dZSfWPzV4Y/d8ye6WgLfNmntqBOX094g==` |
| `@vue/server-renderer` | `3.6.0-rc.3` | `sha512-YCKcCMz7NY92Wp6Ugv7JBFHqgbdteIC6CM3TzMMbJ8uB56sUXrF7qJRh3z6AyH3FESycFdXnUSIwNhkmjL5hfg==` |
| `@vue/reactivity` | `3.6.0-rc.3` | `sha512-+Uvp1i+oozwkyVy2HGUhmA23QDO/YY+QyBm32oddZyG6+FEaEANG7NCQr+asSJzNHWAZmZo97zVNai0tOBdJRw==` |
| `@vue/shared` | `3.6.0-rc.3` | `sha512-EFnGq/OonnFgOtgAhXLIv8owITuFsaGglKXjsAUJQ+2uVuCPxypdW7NIZUlt7ED2raM1Hn/C83eTeK0tZVGCZw==` |

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
