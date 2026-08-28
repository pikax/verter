---
ruling_id: "BUILD-LANE-SEPARATION"
type: "maintainer-directive"
date: "2026-08-21"
date_source: "stated"
binds: ["build architecture", "release pipeline", "developer workflow"]
source_file: "MAINTAINER-DIRECTIVE-BUILD-LANE-SEPARATION.md"
summary: "`pnpm build` must stop meaning 'produce every possible release artifact'. It becomes the normal host-development build; a separate `pnpm dist` becomes the only command producing publication-ready native, LSP, WASM and TypeScript artifacts. The measured problem is not slow Rust compilation but an ambiguous distribution pipeline bundling four different products, targets, profiles and lifecycle stages into the prerequisite for ordinary development and testing — including an unconditional `wasm-opt -Os`. A content-addressed wasm-opt cache alone takes the stable warm build from ~175s to ~23s while preserving byte-identical release output. Tests and the gate must never invoke the distribution build; the gate in particular owns its own target directory, so a prior root build cannot even warm it."
supersedes: []
superseded_by: []
contradicts: []
notes: "Companion to the gate-performance directives: that work removes replayed test execution, this removes replayed BUILD work. Both share one principle — never recompute a deterministic expensive transformation whose inputs have not changed. The release profile (opt-level 3, fat LTO, codegen-units 1) is NOT to be weakened on suspicion: introduce artifact-dev and release-fast profiles, measure, and only move fat LTO into an explicit dist-max profile if it proves unmeasurable. Acceptance is explicit: a second identical `pnpm dist` must not spawn wasm-opt, and cached and uncached optimized WASM must hash identically."
---

# Maintainer Directive — separate build lanes from the distribution pipeline

**Status:** RATIFIED by the maintainer, 2026-08-21.

## The decision

> `pnpm build` must stop meaning "produce every possible release artifact."

It becomes the normal host-development build. A separate **`pnpm dist`** is the
only command that produces publication-ready native, LSP, WASM and TypeScript
artifacts.

## Why — the problem is not Rust being slow

The root build serially combines four different products, targets, profiles and
lifecycle stages: native NAPI in release, LSP in dev, WASM in release plus
binding generation plus Binaryen plus packaging plus a playground copy, and the
TypeScript packages. Those were never a coherent prerequisite for ordinary
development or testing.

Note the incoherence directly: native builds release while the LSP builds dev.
That is neither a developer build nor a release build.

`wasm-opt -Os` runs unconditionally. A content-addressed cache hit alone takes
the stable warm build from ~175s to ~23s — **preserving the exact release
output**, merely declining to recompute it from identical input.

## Lane contract

| Command | Purpose | Must NOT |
|---|---|---|
| `pnpm check` | fast Rust + TS compile validation | link NAPI, generate bindings, run wasm-opt, package |
| `pnpm test` | ordinary unit tests | run the root distribution build |
| `pnpm build` | host developer build: native, LSP, required TS | build release WASM |
| `pnpm build:wasm` | runnable developer WASM | run `wasm-opt` |
| `pnpm dist` | complete publication-ready artifacts | mix dev and release profiles |
| `pnpm gate` | canonical repository validation | run `pnpm build` first |

The gate owns its own target directory and strips target-dir configuration to
preserve isolation, so a prior root build cannot warm it. Prebuilding before the
gate is not merely unnecessary — it cannot work.

## The wasm-opt cache must be content-addressed

Key on the actual optimization inputs — post-wasm-bindgen `.wasm` bytes, the
wasm-opt version, its exact arguments, relevant Binaryen options, and a cache
schema version — never on timestamps or commit ids. Hash the post-BINDGEN wasm,
not the raw Cargo output: binding generation changes the binary, and its output
is what Binaryen receives.

Write to a temporary file and rename atomically, so an interrupted build cannot
leave an entry that looks valid.

## Release profile: measure before weakening

Do NOT weaken the shipped release profile on suspicion. Introduce `artifact-dev`
for local artifacts and `release-fast` (thin LTO, 16 codegen units) for internal
release testing, benchmark against the current profile on build time, artifact
size, compile throughput, LSP and NAPI latency, memory and end-to-end tests — and
only then decide whether fat LTO plus one codegen unit earns its cost or belongs
in an explicit `dist-max`.

## Acceptance

- `pnpm test` never invokes the distribution build.
- `pnpm gate` never invokes the root or distribution build.
- Neither `pnpm build` nor `pnpm build:wasm` ever runs `wasm-opt`.
- Only `pnpm dist` produces publication-ready output.
- **A second identical `pnpm dist` does not spawn `wasm-opt`.**
- **Cached and uncached optimized WASM hash identically.**
- NAPI-dependent tests build the native developer artifact exactly once.
- No package-level build modifies another package.
- Release artifacts never accidentally use a dev profile.
- Current release performance and size remain the baseline until `release-fast`
  proves equal or better.

## Order

**P0** — rename the aggregate to `dist`; make `build` the host developer build;
add `build:wasm` without wasm-opt; remove every `pnpm build` prerequisite from
test and gate paths; add the content-addressed wasm-opt cache; remove the WASM
package's implicit playground copy; stop deleting generated WASM output before
knowing whether inputs changed. This attacks ~86% of the stable warm build.

**P1** — one explicit host target and profile shared by NAPI and LSP; resolve the
`session_metrics` feature divergence (preferably by isolating LSP-only
instrumentation rather than fragmenting the shared core); one Cargo host
invocation; separate NAPI packaging from its compilation; benchmark
`release-fast`.

**P2** — explicit sccache commands with cache-stat validation; diagnose the
transitional NAPI rebuild via Cargo fingerprint logs rather than inference;
Cargo timing reports; Binaryen thread-count and optimization-mode benchmarks;
gate build-job benchmarks; move `onlyBuiltDependencies` into
`pnpm-workspace.yaml`.
