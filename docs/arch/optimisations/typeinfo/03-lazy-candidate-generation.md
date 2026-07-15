# T3 — Lazy candidate generation in `resolve_eval_dependency_canonical_with`

**Level:** micro (allocation removal). **Risk:** minimal — pure refactor with a pinned probe contract.
**Reference implementation:** branch `perf/t3-lazy-candidates`, commit `432762b8a` (measurement machine).

## Problem (profiler evidence)

`crates/verter_session/src/host_manage.rs::resolve_eval_dependency_canonical_with` (~line 195) is the
candidate-probing core under `resolve_eval_dependency_canonical` (42.6 % of benchmark CPU inclusive).
It eagerly builds a `Vec<String>` of up to 13 `format!` candidates on EVERY call — **before** the
explicit-extension fast path that usually returns the input untouched. `alloc::fmt::format` under this
function alone is **12.6 % of total pass CPU**, plus the malloc/free traffic of the discarded strings.

## Change

In `resolve_eval_dependency_canonical_with`:

1. Evaluate the explicit-extension fast path (`!prefers_type_companion && has_explicit_extension &&
   has_candidate(dep_canonical)`) BEFORE any candidate construction.
2. Replace the eager `Vec<String>` with two const rule tables probed lazily:
   - `STRIP_RULES` (11 rows): `(".esm-bundler.js", ".d.ts")`, `(".esm-browser.js", ".d.ts")`,
     `(".esm-browser.prod.js", ".d.ts")`, `(".global.js", ".d.ts")`, `(".global.prod.js", ".d.ts")`,
     `(".cjs.js", ".d.ts")`, `(".cjs.prod.js", ".d.ts")`, `(".js", ".d.ts")`, `(".jsx", ".d.ts")`,
     `(".mjs", ".d.mts")`, `(".cjs", ".d.cts")` — each `strip_suffix` match constructs its candidate
     into ONE reused `String` buffer (`clear()` + `push_str`, pre-reserved `dep.len() + 11`).
   - `APPEND_SUFFIXES` (6 rows): `.d.ts`, `.ts`, `.tsx`, `/index.d.ts`, `/index.ts`, `/index.tsx`.
3. Preserve the exact original probe ORDER and the two tail fallbacks (extension-less input probe,
   `prefers_type_companion` input probe) — including the quirky `/x/.js` dot-file edge where
   `Path::extension()` is `None` but `ends_with(".js")` is true, so the input is probed twice at the tail.
4. Signature unchanged (`pub(crate)`, same params) — there is a second caller at
   `crates/verter_session/src/host_executor.rs:295`.

## Test contract (write these BEFORE refactoring)

12 recording-closure tests (module `resolve_eval_dependency_probe_contract_tests` in
`crates/verter_session/src/host_manage_tests.rs` on the reference branch) that pin the probe sequence
via a closure recording every probed candidate, covering: runtime-js input order; each bundler suffix's
companion-first order; jsx/mjs/cjs mappings; extension-less candidate order + index candidates + raw-path
fallback; explicit non-js extension fast path probing ONLY the input; explicit `.d.ts` input order;
empty input probing nothing; the dot-file double-tail-probe edge. Verified discriminating: a temporary
`.d.ts`/`.ts` order permutation must fail ≥half of them.

## Verification

- `cargo test -p verter_session --lib` (all green; 16 tests in the touched area).
- Clippy delta vs base = zero new findings (the WIP base itself has ~83 pre-existing `-D warnings`
  errors in `verter_session` — diff error sets, don't gate on exit code).
- Artifact parity: byte-identical benchmark artifacts (same resolution results by construction).

## Measured result (this machine)

Full pass (post-fix protocol, median of 3 interleaved runs): steady 20 480 → **16 859 ms (−17.7 %)**;
p50 42.7 → 35.2 ms; p95 345 → 291 ms; max 1985 → 1739 ms; peak RSS 720 → 685 MB.
