# Phase 9b — `compileBatch` Host-Backed Cutover — Implementation Report

**Sub-plan:** `<scratch>/verter-architecture-cutover-phase-09.md` (REVISION 3.2)
**Branch:** `wt/phase-09b-host-backed-compile-cutover`
**Base commit:** `4cc68fdafe39ea24921ce220ed80f9e347ae1789` (post-Phase-10a integration tip)
**Worktree:** `<worktree>/phase-09b-host-backed-compile-cutover`
**Merge strategy:** Squash-merge into `refactor/semantic-db-overhaul`.

## TL;DR

The host-bypassing free-fn `compileBatch` (Rayon-direct, stateless, importing
`verter_compiler::compile::compile` directly inside `verter_napi`) is now
**deleted**. Batch SFC compile is the host-backed `host.compileMany` instance
method, routing through scheduler + dispatch + compile_cache with a single
8 MiB-stack Rayon worker pool, per-input panic isolation, byte-identical
upsert short-circuit, read-once dedupe, and Phase-1 semantic-db
pre-invalidation. JS-side surface is migrated atomically (one release).

## Recovery procedure

None required — fresh start at `4cc68fda`. No prior 9b worktree to recover.

## Pre-flight gates (per §7.1)

All four gates verified at the worktree HEAD before commit 1:

1. `<scratch>/verter-architecture-cutover-phase-09.md` exists.
2. `phase-09b.user_approved == true` recorded in `<scratch>/verter-cutover-state.json`.
3. The bypass is intact: `crates/verter_napi/src/lib.rs:2314` matches
   `^use verter_compiler::compile::\{?compile\b`.
4. `crates/verter_napi/src/lib.rs` contains a `^(fn|pub fn) compile_batch`
   anchor (lines 2345 and 2387 in the base tree).

## Pre-flight `cargo fmt --all --check`

Output: `EXIT=0`. No prefix commit needed.

## Per-commit summary

| # | SHA | Subject | LOC delta |
|---|-----|---------|-----------|
| 1 | `48d41dcb` | `test(architecture): add no_napi_direct_verter_compiler_emitters (fails on HEAD)` | +1139 |
| 2 | `41432558` | `feat(session): add host-backed VerterHost::compile_many` | +1140 / -6 |
| 3 | `cea84499` | `refactor(napi): delete compileBatch bypass; wire host.compileMany` | +874 / -149 |
| 4 | `867df320` | `refactor(benchmark,native): migrate to host.compileMany` | +140 / -32 |
| 5 | (this commit) | `chore(orchestrator): mark phase 9b complete` | marker + report |

### Commit 1 — architecture guard RED proof

Body: `crates/verter_session/tests/architecture_guards.rs` appended with
`mod napi_compiler_emitters` (~370 lines: visitor + classifier +
allow/deny lists + walker + run helper) plus the new
`no_napi_direct_verter_compiler_emitters` test.

The guard parses every production `.rs` under `crates/verter_napi/src/` with
`syn` (sibling `*_tests.rs` and `tests.rs` whitelisted), inspects
`ItemUse` / `ExprPath` / `TypePath`, and rejects:

- Any reference to `verter_compiler::compile::compile`
  (`compile_from_parsed` is also explicitly denied).
- Anything under `verter_compiler::compile_parallel::*` (entire namespace
  forward-defense-forbidden).
- Any leaf inside `verter_compiler::compile::*` not on the pure-data
  allow-list (`CodegenOptions`, `VerterCompileOptions`, `VerterCompileResult`,
  `TypesParserConfig`, `ParsedSfc`).

Glob arms (`use ... ::*`) whose prefix matches either compile namespace are
rejected outright. Rename arms (`use compile as compile_sfc`) match on the
ORIGINAL ident.

**RED proof captured at HEAD (`phase-09b-redproof.txt`):**

```
running 1 test
test no_napi_direct_verter_compiler_emitters ... FAILED

failures:

---- no_napi_direct_verter_compiler_emitters stdout ----

thread 'no_napi_direct_verter_compiler_emitters' panicked at
crates\verter_session\tests\architecture_guards.rs:1929:13:
Phase 9b architecture guard violation:
found 1 verter_compiler::compile{,_parallel} reference(s) in 1 file(s):
  .../crates/verter_napi/src/lib.rs -- use `compile`

NAPI production sources MUST NOT reference
`verter_compiler::compile::{compile, compile_from_parsed}` or any
symbol under `verter_compiler::compile_parallel::*`. Batch and single
SFC compile must route through `VerterHost::compile_many` /
`VerterHost::get_virtual_file`. See sub-plan §5 for the full rule set.

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured;
18 filtered out; finished in 0.04s
```

This is the discriminating evidence that commit 1 lands the guard
observably failing on the bypass at `lib.rs:2314`. All other 17 guard
tests in the file continue to pass.

### Commit 2 — `host_compile` + `compile_many` + warm-peek + priority-parameterized upsert helper

Files added/modified (production):

- `Cargo.toml` (workspace root): `rayon = "1.8"` added to
  `[workspace.dependencies]`.
- `crates/verter_session/Cargo.toml`: target-gated
  `[target.'cfg(not(target_arch = "wasm32"))'.dependencies] rayon = { workspace = true }`.
- `crates/verter_session/src/host_compile.rs` (new, ~330 LOC): the entire
  §3.1 module body — `CompileBatchInput` / `CompileBatchEntry` /
  `CompileBatchOptions` / `compile_profile_for_bundler()` /
  `VerterHost::compile_many` (three-phase algorithm) /
  `scheduler_source_differs_from` / `upsert_with_priority_for_batch` /
  `compile_one_in_batch` / `panic_message`.
- `crates/verter_session/src/host_compile_tests.rs` (new, ~500 LOC):
  the 13 tests per §3.6.
- `crates/verter_session/src/host_resolve.rs`: added `pub(crate) fn compile_slot_is_warm`
  near `get_virtual_file`. Body mirrors the writer freshness predicate at
  current-tree `host_resolve.rs:3014` (`slot.semantic_hash == parse.semantic_hash
  && slot.style_override_hash == soh && slot.content_override_hash == coh`)
  — verified to match base 4cc68fda lines 3041-3045. **No `cc.evicted`
  early-return** (writer doesn't either; eviction zeroes
  `compile_slots`, which makes both call sites observe the same `None`).
- `crates/verter_session/src/host_upsert.rs`: refactored. The existing
  public `upsert` is now a one-line forwarder calling
  `self.upsert_with_priority(req, Priority::Interactive)`.
  - New `pub(crate) fn upsert_with_priority(req, priority)` performs the
    `semantic_db.lock().invalidate(id)` pre-invalidation invariant
    (current-tree `host_upsert.rs:67-69` — verified to match base
    4cc68fda `host_upsert.rs:36-39`), records the priority into the
    test-only observable, then delegates.
  - New `pub(crate) fn upsert_via_scheduler_with_priority(req, priority)`
    is the priority-parameterized inner helper. Body is the existing
    `upsert_via_scheduler` flow with the previously hard-coded
    `Priority::Interactive` (current-tree `host_upsert.rs:128`,
    base 4cc68fda `host_upsert.rs:81`) replaced by the parameter.
- `crates/verter_session/src/lib.rs`:
  - Module declarations: `#[cfg(not(target_arch = "wasm32"))] pub mod host_compile;`
    + `#[cfg(all(test, not(target_arch = "wasm32")))] mod host_compile_tests;`.
  - Two `#[cfg(test)]` test-only fields on `VerterHost`:
    - `last_upsert_priority: parking_lot::Mutex<Option<verter_scheduler::stage::Priority>>`
    - `compile_one_call_count: std::sync::atomic::AtomicUsize`
  - Both initialized in the single `new_with_scheduler_config` constructor
    (the other three constructors delegate). Compiled out in production builds.
- `crates/verter_session/tests/architecture_guards.rs`: extended
  `phase_8_allow_list` with a Phase-9b citation for `last_upsert_priority`
  (the field is structurally `Mutex<...>` and the cache-shape detector
  flags it; the citation declares it as a per-host single-cell test
  observable, NOT a cache).

Test counts: 13 / 13 host_compile tests pass.

### Commit 3 — NAPI deletion + `compileMany` method

Files modified:

- `crates/verter_napi/src/lib.rs`:
  - Deleted lines 2304-2405 (the entire `compileBatch` bypass block —
    block comment, `oxc_allocator::Allocator` import, `rayon::prelude::*`
    import, the deny-listed `use verter_compiler::compile::{compile as
    compile_sfc, ...}`, the three `#[napi(object)]` types `BatchFile` /
    `BatchOptions` / `BatchResult`, the `compile_batch_files` private
    helper, and the `#[napi] pub fn compile_batch` public entry point).
  - Deleted three smoke tests in `mod tests` that exercised the deleted
    `compile_batch_files` helper.
  - Added `use verter_session::host_compile;` and the three new
    `#[napi(object)]` types `NapiCompileBatchInput` /
    `NapiCompileBatchOptions` / `NapiCompileBatchEntry`.
  - Added the `#[napi(js_name = "compileMany")]` instance method on
    `impl NapiVerterHost`. JS strings `"interactive"` / `"background"`
    are translated to `Priority::Interactive` / `Priority::Background`;
    invalid strings return an `ffi_err` mentioning the invalid value.
- `crates/verter_napi/Cargo.toml`: removed `rayon = "1.8"`.

**GREEN proof captured (`phase-09b-greenproof.txt`):**

```
running 1 test
test no_napi_direct_verter_compiler_emitters ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured;
18 filtered out; finished in 0.04s
```

Workspace cargo test: 45/45 suites green at this commit.

### Commit 4 — JS surface migration + benchmark + E2E

Files modified:

- `packages/native/index.ts`:
  - Deleted `BatchFile` / `BatchOptions` / `BatchResult` interfaces.
  - Deleted `export declare function compileBatch(...)`.
  - Added `CompileBatchInput` / `CompileBatchOptions` / `CompileBatchEntry`
    interfaces (with full JSDoc).
  - Added `compileMany(files, options)` method declaration on the
    `VerterHost` class.
- `packages/native/index.js`:
  - Removed `compileBatch` from the destructured native binding.
  - Removed `module.exports.compileBatch`.
  - Added a thin Buffer-coercion wrapper around
    `VerterHost.prototype.compileMany` that mirrors the existing
    `upsert` wrapper convention (string → Buffer for `input.source`;
    `canonicalId` is always a string).
- `packages/native/index.spec.ts`:
  - Updated the prototype-method audit test to include `compileMany` in
    the `declaredMethods` array (negative-asserts no native method goes
    unsurfaced in TS).
  - Renamed and re-asserted the top-level-export sanity test:
    `processStyle and compileBatch` → `processStyle and VerterHost`,
    with a negative assertion `expect(native.compileBatch).toBeUndefined()`.
  - Added `describe("VerterHost.compileMany", ...)` E2E suite (5 tests
    per §3.6: single-input, per-file isolation, warm cache,
    `priority="interactive"`, and rejected invalid priority).
- `packages/benchmark/src/apple-to-apple.ts`:
  - Replaced `import { compileBatch as verterCompileBatch } from "@verter/native"`
    with type-only `import type { CompileBatchInput, CompileBatchEntry }`.
  - Migrated the MT stress block to `host.compileMany(inputs, { threads,
    priority: "interactive" })` using `createVerterHost("none")` (already
    imported from `./compilers/verter.js`).
  - Updated the MT loop's success filter from `r => !r.error` to
    `r => r.errors.length === 0` (matching the new array shape).
  - Updated the banner to "Verter MT: host-backed (scheduler +
    compile_cache + dispatch, ${CPU_COUNT} Rayon threads, Interactive
    priority)."
  - Updated the file-header comment to reflect the host-backed path.

Verification:
- `pnpm --filter @verter/native test`: 32/32 passed (1 skipped, unrelated).
- `pnpm --filter @verter/benchmark test`: 39/39 passed.
- `cargo test --workspace --tests`: 10296/10296 passed (45 suites).
- `cargo clippy --workspace --tests`: clean.
- `cargo fmt --all --check`: clean.

### Commit 5 — marker + report

This commit lands `crates/verter_session/.phase-markers/phase-09b-complete`
and `phase-09b-report.md` (this file).

## Architecture decisions log

### Option A pivot (carried forward from r2 → r3 → r3.2)

R1's option of relocating the rayon bypass into a hypothetical
`verter_compiler::compile_parallel` was rejected by all three reviewers
(architectural relocation rather than fix). The user-approved direction
is full migration to graph + dispatch with no defers.

### Three-phase batch algorithm

The algorithm collapses three concerns into one pass:

1. **Phase 0 — empty-input short-circuit.** Returns `Vec::new()` before
   any pool construction. Discriminated by `compile_many_with_zero_inputs`
   (the test passes `threads: Some(8)` and asserts the call returns).
2. **Phase 1 — group by canonical, reject conflicts, selectively upsert.**
   Inputs are bucketed by `canonical_id`. If two inputs share a
   `canonical_id` with different sources, the entire group is rejected
   with a `duplicate canonical_id with conflicting source in batch` error
   (every entry for that canonical receives this error in Phase 3).
   For non-conflicting groups, an upsert is submitted ONLY when the
   scheduler does not already hold byte-identical source — discriminated
   by `crate::hash::hash_16(source.as_bytes()) == hd.parse.whole_hash`.
   Upserts go through `upsert_with_priority` (NOT
   `upsert_via_scheduler_with_priority` directly) to preserve the
   `semantic_db.invalidate(id)` pre-invalidation invariant.
3. **Phase 2 — compile each unique canonical exactly once.** Probes
   `compile_slot_is_warm`, then calls `get_virtual_file(VirtualNodeKind::Main)`
   inside `std::panic::catch_unwind`. This is the read-once / process-once
   invariant: 5 duplicate inputs sharing one canonical_id produce exactly
   ONE compile dispatch, observable via the
   `compile_one_call_count` test counter.
4. **Phase 3 — fan out.** Iterates the original `inputs` list (the
   caller's order) and produces one `CompileBatchEntry` per input
   position. Entries for the same canonical share the same `Arc<str>`
   payloads (refcount-only fan-out, no string copy).

### Caller-configurable priority (default = Background)

`CompileBatchOptions.priority: Option<Priority>` — defaults to
`Priority::Background` (yields to concurrent interactive work).
The benchmark explicitly passes `Interactive` because it has no
concurrent interactive work and wants to measure the production path's
full throughput. Future LSP/MCP cold-start consumers will use the
default. The JS surface accepts `"interactive"` / `"background"`;
invalid strings raise an FFI error.

### Test-only observables on `VerterHost`

Two `#[cfg(test)]` fields, both compiled out of release builds:

- `last_upsert_priority: parking_lot::Mutex<Option<Priority>>` — written
  by `upsert_with_priority`; read by
  `compile_many_propagates_interactive_priority` and
  `compile_many_priority_default_is_background` to discriminate priority
  propagation. `Arc::ptr_eq` / `dispatch_counter` cannot discriminate
  this property.
- `compile_one_call_count: AtomicUsize` — incremented at the very top of
  `compile_one_in_batch` (BEFORE the precomputed-error short-circuit).
  Read by `compile_many_compiles_each_canonical_once` to discriminate
  the read-once invariant. Why this specific observable: `Arc::ptr_eq`,
  `compile_slots.len()`, and `dispatch_counter()` all fail to discriminate
  the bug they would catch (HashMap last-write-wins, cache coalescence,
  wrong key-space respectively). The explicit per-call-site counter is
  the only architecturally-clean discriminator.

## Files-touched consolidated list

Production sources:
- `Cargo.toml` (workspace root) — `rayon = "1.8"` added to
  `[workspace.dependencies]`.
- `Cargo.lock` — verter_session inherits rayon; verter_napi loses it.
- `crates/verter_session/Cargo.toml` — target-gated rayon dev-target.
- `crates/verter_session/src/host_compile.rs` — new (~340 LOC).
- `crates/verter_session/src/host_compile_tests.rs` — new (~500 LOC).
- `crates/verter_session/src/host_resolve.rs` — `compile_slot_is_warm` accessor.
- `crates/verter_session/src/host_upsert.rs` — priority refactor.
- `crates/verter_session/src/lib.rs` — module wiring + 2 test-only fields
  on `VerterHost` + 1 module declaration.
- `crates/verter_napi/src/lib.rs` — bypass deletion + `compile_many`
  instance method + 3 NAPI types.
- `crates/verter_napi/Cargo.toml` — `rayon = "1.8"` removed.
- `packages/native/index.ts` — TS surface migration.
- `packages/native/index.js` — JS wrapper migration.
- `packages/native/index.spec.ts` — 5-test E2E suite + audit-test fix.
- `packages/benchmark/src/apple-to-apple.ts` — host-backed migration.

Tests:
- `crates/verter_session/tests/architecture_guards.rs` — new
  `no_napi_direct_verter_compiler_emitters` guard + 1 allow-list entry.

Markers:
- `crates/verter_session/.phase-markers/phase-09b-complete` — R7 marker.

Reports:
- `phase-09b-redproof.txt` — RED proof of arch guard at HEAD (commit 1).
- `phase-09b-greenproof.txt` — GREEN proof of arch guard after
  bypass-deletion (commit 3).
- `phase-09b-report.md` — this file.

## Performance gate result

The hard performance gate is `compile_many_throughput_smoke` in
`host_compile_tests.rs`. It:

1. Compiles 200 synthetic SFCs cold.
2. Compiles the same 200 SFCs warm.
3. Asserts `cache_hit_ratio_warm == 1.0` (every entry is a warm hit on
   the second call).
4. Asserts `cache_hit_ratio_cold == 0.0` (no entry is warm on the first
   call).
5. Writes JSON metadata with timing + throughput observations.

JSON artifact path:
`target/phase09b/phase09b-bench.json`

Sample contents from the marker-time run:
```json
{
  "cache_hit_ratio_warm": 1.0,
  "cold_ms": 53.16,
  "cold_throughput": 3761.91,
  "n_files": 200,
  "warm_ms": 3.07,
  "warm_throughput": 65206.05
}
```

200 SFCs cold compile in ~53ms; warm replay in ~3ms. Confirms the
warm-cache short-circuit is working: 1.0 hit ratio on the warm pass.

The soft `apple-to-apple` benchmark gate is informational only (per §0
"Performance gate"); requires `VIZE_PATH` to be set. Not run in this
report; will execute on the orchestrator's CI lane if VIZE_PATH is
available there.

## Verification command output

Final marker-time fresh `cargo test --workspace --tests --verbose`:

- 45 / 45 test suites green.
- 10296 / 10296 tests passed.
- 0 failures.
- Test categories: `cargo test --workspace --tests` workspace + integration
  test binaries combined.

Final marker-time fresh `cargo test -p verter_session --test correctness`:

- 18 / 18 correctness fixtures pass.

Final clippy: clean.
Final fmt: clean.

Architecture guard `no_napi_direct_verter_compiler_emitters` PASSES at
HEAD (verified at commit 3 onwards).
