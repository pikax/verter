# Phase 9 — `compileBatch` Breaking-Migration Sub-Plan Needed

**Status:** STOP — `compile_batch` is on the public NAPI API and bypasses the
shared host path. Per §9.1 (r3 — Codex-9 + §16 update), this requires a
production-grade breaking-migration sub-plan, not a `#[doc]` annotation.
**This file documents the deferral; the actual sub-plan is to be designed by a
Plan-type subagent before the migration executes (per §9.1 directive).**

This is a **deferral per §0.5.1**, not a stuck. Phase 9's marker will record
`status: "partial-deferred"` with `phase-09-breaking-sub-plan-needed` in the
`deferred` list.

---

## 1. Public-API Surface Identified

### 1.1 Rust (NAPI binding layer)

`crates/verter_napi/src/lib.rs`:

| Line | Symbol | Kind |
| ---- | ------ | ---- |
| 2314 | `use verter_compiler::compile::{compile as compile_sfc, CodegenOptions, VerterCompileOptions};` | direct compiler import (bypasses host) |
| 2317 | `pub struct BatchFile { filename, source }` | `#[napi(object)]` exported type |
| 2326 | `pub struct BatchOptions { threads }` | `#[napi(object)]` exported type |
| 2333 | `pub struct BatchResult { filename, code, error, durationMs }` | `#[napi(object)]` exported type |
| 2345 | `fn compile_batch_files(files, skip_source_map) -> Vec<BatchResult>` | internal Rayon helper |
| 2387 | `pub fn compile_batch(files, options) -> Result<Vec<BatchResult>>` | `#[napi]` exported function — **public NAPI** |

The function calls `compile_sfc(source, &codegen_opts, &verter_opts, &allocator)`
directly on each file in parallel via `rayon::par_iter`. There is no
`VerterHost`, no scheduler, no `IndexedReady`, no `ProjectTypeStore`, no
shallow file inventory, no dependency-graph awareness.

### 1.2 TypeScript (typed exports)

`packages/native/index.ts`:

```ts
// Line 101-118 — exported types
export interface BatchFile { filename: string; source: string; }
export interface BatchOptions { threads?: number; }
export interface BatchResult { filename: string; code: string; error?: string; durationMs: number; }

// Line 120-128 — exported function declaration
/** ... benchmark comparison ... */
export declare function compileBatch(files: BatchFile[], options?: BatchOptions): BatchResult[];
```

### 1.3 JavaScript (runtime export)

`packages/native/index.js`:

```js
// Line 123-130 — destructure from native binding
const { processStyle: _processStyle, compileBatch, VerterHost, ... } = nativeBinding;

// Line 175 — re-export at module top level
module.exports.compileBatch = compileBatch;
```

### 1.4 JS-side consumers

| File | Line | Usage |
| ---- | ---- | ----- |
| `packages/benchmark/src/apple-to-apple.ts` | 24 | `import { compileBatch as verterCompileBatch } from "@verter/native";` |
| `packages/benchmark/src/apple-to-apple.ts` | 213 | benchmark Verter MT path |
| `packages/benchmark/src/apple-to-apple.ts` | 266 | console banner |
| `packages/native/index.spec.ts` | 569-572 | `expect(typeof native.compileBatch).toBe("function");` smoke test |

The lone external (non-test) consumer is the benchmark suite, which uses
`compileBatch` for an "apples-to-apples" parallel-compile comparison against
a third-party tool (Vize's `compileSfcBatch`). The Rust source comment at
line 2385 confirms the design intent:

```rust
/// Equivalent to Vize's `compileSfcBatch` for fair benchmark comparison.
```

---

## 2. Architectural Violation Specified

The authority chain (CLAUDE.md, top of file, "Architectural target for the
project-global cache cutover") declares:

> scheduler → IndexedReady → ProjectTypeStore → SemanticGraph/component-meta → thin consumers

`compile_batch` is a **direct bypass** of every step:

| Authority step | Bypass |
| -------------- | ------ |
| Scheduler (sole parser) | `compile_batch` calls `parse_sfc` indirectly via `verter_compiler::compile::compile`, allocating a fresh OXC `Allocator` per file with no scheduler awareness. |
| `IndexedReady` (canonical post-parse artifact) | Not produced. Each parse arena is dropped at the end of one `par_iter` element. |
| `ProjectTypeStore` (single project-global cache) | Not consulted. No cache key is computed; identical inputs across calls re-parse and re-compile. |
| `SemanticGraph` / `verter_session` ownership | No `VerterHost`, no published snapshot, no dependency-graph integration. |
| Single-file ownership lifecycle | Each file is treated in isolation — cross-file imports are not resolved, semantic queries are not available, and macro types like `defineProps<T>()` cannot bind to types from sibling files. |

The Rust source comment at lines 2306-2309 explicitly documents this:

```rust
// compile_batch() is a pure stateless parallel compiler: no VerterHost, no
// caching. Each file gets its own bumpalo Allocator per Rayon thread.
// This matches Vize's compileSfcBatch() API for a fair benchmark comparison.
```

Per **CLAUDE.md → "Shared Optimized Codebase (CRITICAL)"**:

> Improvements should land in the lowest reusable owner crate that can
> correctly serve all consumers.
> Consumer packages such as `@verter/component-meta`, the LSP, MCP, unplugin,
> and playground should consume the shared substrate rather than carrying
> their own semantic forks.

`compile_batch` is precisely such a "consumer-local fork" — it owns its own
parser allocation, its own (absent) caching strategy, and its own narrow
single-file semantics. The fact that it ships a benchmark-defensible result
(equivalent to Vize) does not justify keeping a parallel compile pipeline
outside the host-backed authority chain on a published API surface.

---

## 3. Proposed Breaking Migration Shape

### 3.1 Identify or design the host-backed equivalent

Two design directions exist; the Plan-type subagent must choose one:

**Option A: extend `VerterHost` with a multi-file batch entry point**

Add (e.g.) `VerterHost::compile_batch_via_scheduler(files: &[(canonical_id,
source)], options) -> Vec<CompileBatchResult>`. The host would:

1. Submit each file's `(canonical_id, source)` to the scheduler in one
   admission burst.
2. Wait for all `ParseSnapshot`s to publish through the scheduler's
   completion fence.
3. For each file, hit the per-file compile cache (`compile_cache` DashMap),
   which already keys by `(canonical_id, content_hash, profile_hash)`.
4. Emit `BatchResult { filename, code, error, durationMs }` from cached or
   freshly-computed `CompileCacheEntry` values.

The Rayon parallelism currently in `compile_batch_files` is replaced by the
scheduler's existing parallel admission; the host owns the thread pool and
deduplication.

**Option B: add a stateless "no-host" parallel-compile entry point on the
shared crate**

Add (e.g.) `verter_compiler::batch::compile_batch_parallel(...)` that lives in
the owner crate, takes ownership of the Rayon parallelism, and is consumed
identically by NAPI / WASM / unplugin if desired. This keeps the bypass but
moves it to the shared substrate — the architectural violation softens
(consumer-local fork → owner-crate-local utility) but the authority chain
bypass remains.

The user directive cited in the brief ("production-grade, no quick fix")
points at **Option A**. The Plan-type subagent must confirm.

### 3.2 Sequence NAPI consumer migration

Approximate commit-by-commit sequence (the actual sequence is the
sub-plan's responsibility):

1. **commit N+1 — host-backed batch API lands (Rust)**.
   Owner crate (`verter_session` for Option A, `verter_compiler` for Option
   B) gains the new entry point. Tests: parity against `compile_batch_files`
   on a fixture set; cross-file-resolution test that fails on `compile_batch`
   but passes on the new API.

2. **commit N+2 — NAPI exposes the new path under a new symbol**.
   `verter_napi::lib` adds `pub fn compile_batch_via_host(...)` (or chosen
   name). Old `compile_batch` stays. `packages/native/index.ts` declares
   `export declare function compileBatchViaHost(...)`.

3. **commit N+3 — JS consumer (benchmark) migrates**.
   `packages/benchmark/src/apple-to-apple.ts` switches its import to the
   new symbol. The `index.spec.ts` smoke test is updated to assert both
   exist during the deprecation window.

4. **commit N+4 — deprecation notice on old API**.
   `compile_batch` (Rust) and `compileBatch` (TS) gain doc-block
   `@deprecated` notices pointing at the new symbol.

5. **commit N+5 — release window cuts over**.
   One published release with both APIs available. CHANGELOG entry calls
   out the migration.

6. **commit N+6 — delete the old API**.
   `compile_batch` and `compile_batch_files` are removed from
   `verter_napi::lib`. `compileBatch` is removed from `packages/native`. The
   `verter_compiler::compile::compile` direct import in `verter_napi` is
   deleted. The smoke test in `index.spec.ts` is removed or updated to
   assert `compileBatch === undefined`.

### 3.3 Deprecation window

One release with both APIs (per CLAUDE.md "Legacy Code Deletion" — a
deprecation window is acceptable when the APIs cross a published-package
boundary, but should not extend beyond one release). This matches the user's
directive: production-grade, no shims that linger across multiple releases.

### 3.4 Alignment with user directive

The user's directive (cited in the brief) is "production-grade, no quick
fix". The sub-plan above:

- Routes the function through the shared authority chain (no consumer-local
  fork on a published surface).
- Uses an explicit deprecation window for the breaking JS-side change so
  third-party consumers (if any) have one release to migrate.
- Deletes the legacy direct-compiler path completely at the end of the
  window — no permanent dual path, no compatibility shim.

---

## 4. Plan-type Subagent Spawn (per §9.1)

The brief at lines 3748-3766 specifies the subagent that should design the
final migration:

```text
Agent(
  description: "Design Phase 9 compile_batch breaking migration",
  subagent_type: "Plan",
  prompt: "Read crates/verter_napi/src/lib.rs around the compile_batch
   function. Read packages/native/src/index.ts to find JS consumers.
   Design a breaking-migration plan that:
     1. Identifies the host-backed equivalent (or proposes adding one).
     2. Sequences NAPI consumer migration commit-by-commit.
     3. Specifies the deprecation window (one release with both APIs,
        then deletion).
     4. Aligns with the user's directive: production-grade, no quick fix.
   Output: D:/tmp/verter-architecture-cutover-phase-09.md.
   DO NOT IMPLEMENT — design only."
)
```

Phase 9 worker is NOT spawning that agent; per the brief, the worker
commits this `phase-09-breaking-sub-plan-needed.md` document, surfaces the
deferral, and STOPs. User reviews and triggers the Plan-type subagent
separately.

---

## 5. STOP Reason

§9.3 STOP CONDITIONS:

> §9.1: `compile_batch` is on public NAPI API — STOP.

`compile_batch` is unambiguously on the public NAPI API:

- `pub fn compile_batch` with `#[napi]` → exported to the native binary.
- `export declare function compileBatch` in `packages/native/index.ts` →
  typed export.
- `module.exports.compileBatch = compileBatch;` in `packages/native/index.js`
  → runtime export.
- Smoke-tested at `packages/native/index.spec.ts:572`.

§9.2 (LSP/MCP dependency direction inventory) is not executed in this Phase
9 run — the §9.3 STOP terminates execution after §9.1's deferral commit.
§9.2 will land in a follow-up Phase 9 worker run after the user reviews the
breaking sub-plan, or it can be promoted to a separate dedicated phase.
