# rsvelte runtime integration

This decision records the ownership boundary for Svelte runtime compilation in
Verter. It is deliberately narrower than a wholesale Svelte adapter rewrite:
runtime compilation moves to rsvelte, while Verter keeps its existing IDE
projection until rsvelte and Verter can prove mapping parity independently.

## Ownership

rsvelte owns Svelte source parsing, component analysis, and client/server
runtime emission. Verter owns carrier registration, compile profiles, cache
admission, scheduling, virtual files, diagnostics transport, IDE projection,
template facts, and cross-file type resolution.

The only production dependency edge is
`verter_compiler::svelte::rsvelte_bridge`. Its input and output are Verter's
framework-neutral `RuntimeCompileOptions` and `RuntimeCompileOutput`; no rsvelte
AST, allocator, diagnostic, or mapping type escapes the module. The bridge:

- pins the rsvelte crate version, Git revision, toolchain ABI, runtime ABI, and
  Svelte compatibility version;
- translates the resolved Verter profile into policy-free rsvelte compile
  options;
- emits either client or server ESM verbatim as the carrier `Main` module;
- translates external CSS, source maps, scope hashes, warnings, and failures
  into neutral carrier DTOs;
- fails closed on compiler or fingerprint mismatch; there is no production
  fallback to Verter's former runtime backend.

Verter's IDE projection remains independent in this cutover. rsvelte's
projection has a different exact-mapping contract, so replacing it belongs in
a separate parity change rather than being hidden inside a runtime migration.

## Version and publication contract

The initial boundary targets:

| Axis | Pin |
| --- | --- |
| `rsvelte_core` | `=0.9.4` plus an exact Git revision during integration |
| rsvelte toolchain ABI | `1` |
| rsvelte runtime ABI | `1` |
| rsvelte facts ABI | `2` |
| Svelte compatibility target/runtime | `5.56.8` |

The Git revision makes review builds reproducible, but it fetches rsvelte's
repository and submodules and ties routine builds to GitHub availability. It is
therefore a review dependency, not the normal release/CI contract. Publish
`rsvelte_esrap` and then `rsvelte_core` to the registry before replacing the
exact Git pin with the same exact crate version. That source-only change must
not require changes to bridge code.

## Merge gates

The runtime cutover is mergeable only when all of these are true:

1. `rsvelte_core`'s `default-features = false` graph excludes formatter, CLI,
   watcher, and module-resolver tooling.
2. `rsvelte_esrap` and `rsvelte_core` are packageable and published in
   dependency order, and Verter resolves the exact registry version without a
   Git source.
3. Verter has only one production Svelte runtime route and no fallback.
4. Client and server carrier tests cover module output, source maps, external
   CSS, default and configured scope hashes, warnings, and fail-closed errors.
5. Verter's canonical workspace, shared-process session, clippy, release,
   WASM, formatting, and TypeScript gates pass.
6. The application runtime is pinned to the same Svelte version rsvelte
   targets, and CSR/SSR behavior is exercised through Verter's public carrier
   path.

Verter's native runtime module is a conformance surface, not a registered host
backend. It may keep its independently pinned Svelte 5.56.3 oracle corpus while
the application runtime follows rsvelte's 5.56.8 target. It must not be
selected by a feature flag or error fallback; the carrier has one production
runtime route.
