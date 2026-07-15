# Plan: Consolidate `verter_session` integration-test binaries

Status: **landed** — 301 → 23 top-level binaries (~92% fewer). OS-agnostic build-time fix (helps Windows, Linux, macOS).

## 0. What landed

Three phases, each gated on the full workspace suite (`cargo nextest run --workspace`); test-marker parity held throughout (**1427 == 1427**, no test bodies changed — `git mv` preserves content):

- **Phase 1 — 301 → 200.** The 117 self-contained files (no `#[path]`/`include_str!`/relative-path dep, no `#[global_allocator]`, basename not referenced elsewhere) `git mv`'d into 16 `#[path]` mod-root groups (`tests/g_<group>.rs` + `tests/g_<group>/`).
- **Phase D — 200 → 149.** The 3 meta-guards that resolved test files by their **top-level** path were made **location-independent** (resolve by basename recursively under `tests/**`): `critical_rules_have_guards` (R6 registry), `architecture_guards::wave_3_entry_points`, `walker_parity_baselines`. With that, the 51 clean files previously kept top-level (referenced by name) joined the groups.
- **Phase C — 149 → 23.** The 127 remaining coupled files. File-relative paths (`#[path]`/`include_str!`/`include_bytes!`) get a uniform **+1 `../` depth rewrite** when a file moves into `tests/g_<group>/`; the ~92 `CARGO_MANIFEST_DIR` (manifest-relative) files move untouched. One submodule sibling-ref fixed (`correctness/expected.rs`: `crate::snapshot_view` → `super::`).

**Measured win** (Windows, 32-core, `debug=line-tables-only`): the "edit `verter_session` lib → rebuild the whole test suite" relink-all loop dropped **~98s @ 301 → ~70s @ 200 → ~19s @ 23 binaries (~5×)**. The rlib recompile is a fixed ~5s floor; the rest is link count, which is why the speedup tracks the binary-count reduction. The win is larger on lower-core machines (Linux/macOS 8-core) where link parallelism can't hide the count.

### What stays top-level (cannot consolidate)

- **`#[global_allocator]`** alloc-counting tests (2) — one global allocator per binary.
- **Big stitcher roots** — `corpus_audit_tests.rs` (179 modules), `fact_matrix.rs` (51), `component_meta_audit.rs` (24 `#[path]`) — already one binary each, no win available.
- **Hardcoded-path readers/targets** — `architecture_guards.rs` reads its own source by the literal path `tests/architecture_guards.rs` (as do `project_type_store_tests` and `workspace_bookkeeping_invariants`), so it stays top-level. The general rule: a file is consolidatable only if **every** reader of it resolves location-independently. Most cross-references are in the 3 now-recursive meta-guards, which is why only `architecture_guards.rs` had to stay.

The full workspace suite is green except one **unrelated pre-existing** failure: `mapper_fingerprint::..._without_stack_overflow`, a deterministic Windows-only stack overflow in a file untouched by this work (it fails on the base commit; CI is Linux, where it passes).
- **Binary-unique** (2) — `#[global_allocator]` alloc-counting tests (`baseline_trace_alloc_count`, `canary_warm_hit_zero_alloc`) must stay separate binaries (one global allocator per binary).

### Phase 2 — LANDED as Phase C (see §0)

The ~120 relative-path-coupled files were consolidated via the uniform `../`-depth rewrite (manifest-relative strings like `env!("CARGO_MANIFEST_DIR").join("tests/…")` and trybuild's `"tests/compile-fail/…"` left as-is; `audit_hot_loop_denylist.rs` kept top-level as a cross-file `#[path]` share). Final: **149 → 23 binaries**.

---

## Original plan (for phase 2 reference)

Status: proposed. OS-agnostic build-time fix (helps Windows, Linux, macOS; largest absolute win on Linux).

## 1. Context

Cargo treats every **top-level** `tests/*.rs` file as its own `[[test]]` target. `crates/verter_session/tests/` currently holds **301** such files, so `cargo test -p verter_session` compiles and links **301 separate test binaries**, each statically linking the `verter_session` rlib (≈72 MB at `debug="line-tables-only"`, ≈433 MB at full debug) plus the whole dependency graph (oxc, etc.).

Consequences (measured on Windows, 32 cores, warm target):

- One touched test file relinks in ~2 s (`line-tables-only`) / ~4 s (full debug). Across 301 binaries the **link phase dominates** the suite build.
- Any edit to `verter_session`'s lib forces **all 301 binaries to relink**. Measured on Linux (WSL, 8-core, rustc 1.94): a cold build of all **304** test binaries is ~78 s, and one `lib.rs` edit → relink-all is **~57–59 s**. That per-edit cost is exactly what consolidation removes (~12 links instead of ~300).
- No other crate is affected at scale — the next-highest is `verter_lsp` with 6 test files — so this plan is scoped to `verter_session` only.

The fix is the standard pattern (matklad, "Fast Rust Builds"): collapse many integration-test *targets* into a few by making the individual files **modules of a shared binary**. Cargo only auto-discovers *top-level* `tests/*.rs` as targets; files under `tests/<subdir>/` are plain modules. The repo already uses this shape (`tests/correctness/`, `tests/fixtures/`, `tests/component_meta_audit/`, `tests/fact_matrix/`, …), so the migration follows an existing convention.

## 2. Design decision — group, don't mono-bundle

| Layout | Links | Single-test edit cost | Parallelism |
| --- | --- | --- | --- |
| 301 binaries (today) | 301 | recompile 1 file | high (but 301 links) |
| 1 binary | 1 | recompile **all** test code | none (serial codegen) |
| **~12 group binaries (chosen)** | ~12 | recompile 1 group | groups build concurrently |

Group by the existing filename prefixes. From the current tree the natural groups are: `block_6i_*` (24), `derived_raw_*` (21), `component_meta_*` (13), `slot_binding_*` (9), `type_resolution_*` (8), `route_db_*` (7), `resolved_import_*` (7), `family_bcd_*` (6), `compile_*`/`compile_audit_*` (≈10), `cache_*` (≈8), `audit_*` (≈20 across `*_audit_*`), and a `misc` group for the long tail. Aim for ~20–30 files per group so single-test iteration stays fast.

Result: ~30× fewer links (301 → ~12), groups compile in parallel, and editing one test recompiles only its group.

## 3. Changes

1. Create `crates/verter_session/tests/it/<group>/` and move each `tests/<name>.rs` to `tests/it/<group>/<name>.rs`.
2. For each group `G`, add one top-level root `crates/verter_session/tests/<G>.rs`:
   ```rust
   //! Single linked binary for the `<G>` integration tests.
   #[path = "it/<G>/mod.rs"]
   mod <G>;
   ```
   and generate `tests/it/<G>/mod.rs` with `pub mod <name>;` per moved file.
3. Re-home shared helpers. The 40 files that declare a sibling `mod foo;` keep their helper file, moved into the same group dir. Genuinely shared helpers move to `tests/it/common/` and are pulled in with `#[path = "../common/mod.rs"]`. The existing module/fixture subdirs (`correctness/`, `fixtures/`, `block_2_canary/`, `component_meta_audit/`, `component_meta_audit_corpus/`, `fact_matrix/`, `perf_bounds/`, `compile-fail/`) move under `tests/it/` and are referenced from the relevant group root.
4. Relative-path reads: guard/corpus tests that read via `CARGO_MANIFEST_DIR` and target `src/**` (e.g. `architecture_guards.rs`, `no_legacy_walker.rs`) are unaffected. Any test that enumerates `tests/*.rs` by path, or any guard asserting a `tests/*.rs` naming/location convention, must be updated to the new layout.
5. `trybuild`/`compile-fail` (`tests/compile_fail.rs` + `tests/compile-fail/`) stays its own root (trybuild drives its own harness) — fold it into a `compile_fail` group root rather than the shared modules.

## 4. Legacy deletions

- Delete all 301 top-level `tests/*.rs` files — they become modules under `tests/it/`.
- Single cutover: no transitional dual layout, no top-level files left beside the group roots, no feature flag. (Per repo policy: replace, don't shim.)

## 5. Verification

- Target-count drop: `cargo build -p verter_session --tests -v 2>&1 | grep -c 'Compiling verter_session.*test'` and the count of group-root executables in `target/debug/deps/` ≈ 12, down from 301.
- No test lost: `cargo nextest list -p verter_session | wc -l` is **identical** pre/post-migration (capture the pre count before starting).
- Full gate (per `CLAUDE.md` End-of-change Checks):
  - `cargo nextest run --workspace`
  - `cargo test -p verter_session --tests`
  - `cargo clippy --workspace -- -D warnings`
  - `cargo fmt --all --check`
- Wall-clock proof: time `cargo test -p verter_session --tests --no-run` cold, before vs after, to confirm the link-phase reduction.

## 6. Notes / sequencing

- A group binary recompiles all its members when any member changes — keep groups balanced.
- **Linkers do not help here (measured).** Linux: `mold` was *slower* than GNU `ld` for the 304-tiny-binary relink (74–77 s vs 57–59 s) — many tiny links don't benefit from mold and `mold -run` adds overhead. Windows: `lld-link` ≈ `link.exe`. Bundled `rust-lld` is unusable on stable. Re-evaluate mold only *after* consolidation, when there are ~12 larger links instead of ~300 tiny ones.
- **Complementary structural lever (separate, larger):** the 355-file `verter_session` crate recompiles as a single unit on every edit (~5 s on 32-core Windows, ~22 s on 8-core Linux). Splitting it into cohesive sub-crates (`resolver_core`, `meta_resolve`, audit runtime, cache layers — already conceptual module boundaries) would let cargo recompile and parallelize only the touched part. This is the largest *universal* compile lever and the one that most helps core-bound Linux/macOS machines; track it as its own effort.
- Renaming phase-named test files (`block_6i_round10_*`, `block_2_*`) to invariant-descriptive names is a **separate** follow-up. The no-phase-archaeology guard targets `src/**`, not test filenames, so it does not block this migration; do the rename after consolidation lands to keep the diff reviewable.
- nextest already runs each individual test in its own process, so per-test parallelism is unchanged by consolidation — only the *build* gets cheaper.
- This migration is mechanical but large (301 file moves + ~12 roots + helper re-homing). It is a good candidate for the `/multi-agent-orchestration` flow: one implementer per group, dual review, then a single cutover commit.
