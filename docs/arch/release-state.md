# Release architecture and supported scope

This document records the durable contract of the current Verter beta release
tree. It describes shipped architecture and support boundaries; it is not a
substitute for the release gate or approval of a particular Git revision.

## Compiler and semantic ownership

Verter's compiler is Rust-owned. `verter_compiler` parses framework carriers
and emits runtime or IDE output, while `verter_session` owns workspace state,
dependency invalidation, semantic queries, and cache admission. Editor and
JavaScript packages consume those native surfaces; they do not contain a
second semantic compiler.

The obsolete TypeScript package `@verter/core` has been removed. It was not a
dependency of the production compiler or editor path, and retaining it made
the documented architecture materially misleading. The retirement decision
and fixture disposition are recorded in
[Retiring `@verter/core`](./last/verter-core-retirement.md).

## Supported product surfaces

- **Vue compilation and IDE support:** Rust runtime and IDE compilation,
  source mapping, component metadata, the Rust LSP, and editor-owned
  TypeScript serving are release surfaces.
- **TypeScript serving:** interactive editor features and rich diagnostics use
  one editor-owned TypeScript route per epoch. TSGO and tsserver remain
  provider choices, with fallback occurring only after a connected demand
  observes a bounded failure. The owning implementation contract lives in
  `.claude/skills/host-session/SKILL.md`.
- **Editors:** VS Code is the primary packaged integration. Neovim, Helix,
  Zed, and Lapce adapters use the shared native client and LSP contracts, with
  each adapter's packaging limitations documented alongside its implementation.
- **Svelte:** native client compilation is experimental and pinned to
  `svelte@5.56.3`. Covered behavior is protected by runtime, conformance, and
  official-oracle tests. Unsupported runtime features and unavailable server
  output fail closed with typed diagnostics; they do not return successful
  placeholder modules. See the [unplugin API](../api/unplugin.md) and
  [Svelte compiler architecture](./svelte-native-compiler-plan.md).

Verter remains beta software. The public [guide](../guide/index.md) is the
authority for user-facing maturity and installation expectations.

## Projection safety and diagnostics

Type projection is stack-safe without imposing a structural-depth limit.
These cases are intentionally distinct:

1. Legitimate recursive types terminate as valid recursive references or
   carriers and do not emit an operational diagnostic.
2. An exact in-flight identity cycle keeps the existing cycle/sentinel
   semantics and is not reported as budget exhaustion.
3. Runaway expansion whose identity continually changes is stopped by the
   connected-demand work budget.
4. Deep but finite structural types resolve completely; depth alone is not a
   failure condition.
5. Genuine host re-entry across queries is bounded independently by the
   connected-query-depth limit.

When an operational limit trips, the typed partial result and the best safe
carrier are preserved. The public diagnostic path emits one diagnostic per
root demand and reason at the most relevant authored span:

| Condition | Typed outcome | Diagnostic code | Message |
| --- | --- | --- | --- |
| Connected-demand work exhausted | Partial: work budget | `verter/type-expansion-budget` | `Type expansion exceeded Verter's safe evaluation budget.` |
| Connected-query depth exhausted | Partial: query depth | `verter/type-query-depth-limit` | `Type evaluation exceeded Verter's safe connected-query depth limit.` |

Neither outcome is converted into `Complete`, a normal miss, or a generic
unknown value. Logs and telemetry are supplementary only. A partial result is
never admitted to a warm memo, projection cache, component-meta result cache,
or persistent artifact, so a repeated demand recomputes. Stable unresolved
references remain complete carriers and receive no budget diagnostic.

The detailed type-resolution rules live in the owning
`.claude/skills/type-resolution/SKILL.md`; the editor-facing diagnostic split is
also recorded in [the two-mode TypeScript model](./ts-compat-two-mode-model.md).

## Cache and engine invariants

- Read-side facts and the current workspace view determine cache validity.
- Cancellation, supersession, incomplete provenance, recursion sentinels, and
  operationally limited results are return-only and cannot warm authoritative
  caches.
- Request-local projection and substitution state dies with the request.
- The semantic dispatch is the single type-resolution authority. Compatibility
  packages may project its results but may not recreate a parallel resolver.
- Framework selection changes syntax and code generation, not cache ownership
  or semantic authority.

The full cache contract is documented in
[Fact-based cache architecture](./fact-based-cache.md).

## Benchmark interpretation

The Svelte compiler fence compares Verter with the pinned official compiler on
an explicit corpus. Both backends generate source maps; timed workers validate
clean mapped output and attest fresh stateless compilation plus an immutable
clean revision; memory is measured as isolated-process peak RSS. Separate
conformance and official-oracle tests protect the fixture behavior. Together,
these rails make the fence suitable for regression detection on that corpus.

It is not evidence that Verter is universally faster or uses less memory than
the official Svelte compiler. Corpus coverage, platform, compiler version, and
supported feature scope bound every result. No broad performance claim should
be published from that fence.

## Deliberate follow-up work

The following are improvements, not silently claimed release capabilities:

- broaden Svelte real-world and preprocessing coverage before considering it a
  general replacement for the official compiler;
- implement and validate additional Svelte server/runtime surfaces rather than
  returning placeholders;
- evaluate Vue Vapor and additional framework adapters on their own behavioral
  and performance evidence;
- continue typeinfo performance work without weakening typed completeness,
  cache admission, or public diagnostics.

Release verification remains the non-vacuous gate described in
[Testing](../contributing/testing.md), followed by independent review of one
unchanged Git revision.
