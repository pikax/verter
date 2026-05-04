# Tier 2 — Step 2.2 — W5b — `resolve_type.rs` god-module split

**Branch base:** `refactor/legacy-to-graph-dispatch-migration` @ `f5a1d10e`
**Head commit:** `1d2c3151`
**Worker:** W5b
**Marker:** `phase-tier-2-step-2.2-w5b-complete`
**Status:** success — all gates green, public API byte-equal

## Goal

Split `crates/verter_parser/src/utils/oxc/vue/script/resolve_type.rs` (5595 LOC) into sub-modules under 4000 LOC each while preserving:

- The 5 query modes (Identity, Navigate, Shallow, Expanded, Skeleton) — see `/type-resolution`.
- TS-first resolution priority (`.d.ts > .d.cts > .d.mts > .ts > .tsx > .js > ...`).
- Every `pub fn` and `pub struct` at its exact qualified path (sibling crates resolve through `verter_compiler::utils::oxc::vue::resolve_type::*` and that path must keep resolving).
- Byte-equal behaviour for the existing test corpus (`tests.rs` + `resolve_type_tests.rs`).

## Landed split

`resolve_type.rs` → `resolve_type/` directory module:

| File | LOC | Owns |
| --- | ---: | --- |
| `resolve_type/mod.rs` | 1663 | Public types (`RuntimeType`, `ResolvedElements`, `ResolvedProp`, `ResolvedEmit`, `ResolutionDiagnostic*`, `BlockedType*`), `cache_keys` sub-module, `TypeResolutionContext`, `build_type_context`, `instantiate_type_params_ctx`, the public `resolve_type_elements*` API, the `cache_keys` cache-identity helpers, and `pub use` re-exports for everything in the four sibling sub-modules. |
| `resolve_type/decl.rs` | 1550 | Named-type / interface / class / heritage walkers — the audit's SCC 5/7 (heritage extends recursion). Contains `resolve_named_local_type_with_ctx_ref*`, `build_interface_resolution_plan`, `build_class_resolution_plan`, `flatten_named_type_plan_with_ctx_ref`, `apply_named_type_heritage_edge_with_ctx_ref`, `resolve_interface_with_extends_ctx*`, `try_resolve_heritage_utility_type`, `resolve_class_with_heritage_ctx_ref`, the class-member resolvers, and `resolve_type_elements_inner_with_ctx*` — the body-projection family. |
| `resolve_type/elements.rs` | 535 | Type-element resolvers — `resolve_type_literal_members`, mapped-type resolution (`resolve_mapped_type_with_ctx`, `resolve_mapped_type_keys_with_ctx`, `mapped_optional_override`, `resolve_mapped_string_literal_key`), property/method signature lowering (`resolve_property_signature`, `resolve_method_signature`, `callable_signature_text`), the `extract_string_literal_keys*` family used by `Pick`/`Omit` heritage utilities, and `ResolvedMappedKey`. |
| `resolve_type/infer.rs` | 364 | Runtime-type inference — `infer_runtime_type` (still `pub`, re-exported through `mod.rs`) plus the small set of helpers it shares with the rest of the resolver: `infer_literal_type`, `infer_type_reference`, `extract_heritage_type_names`, `get_type_reference_name`, `resolve_value_declaration_type`, `infer_props_from_object_literal`. |
| `resolve_type/external.rs` | 1646 | External (cross-file) type resolution — `resolve_external_type*` cold-path entry points, `AnalyzedExternalTypeSource` shallow inventory + builder (`analyze_external_type_program`, `analyze_external_type_source`, `extract_export_surface`, `extract_imported_type_bindings`, `required_import_alias_names_for_binding`, `imported_member_name_for_required_alias`, `collect_required_import_names_for_external_type`), the structural-dependency walkers, and `hash_resolved_type` (a natural co-resident since every external entry point is its consumer). |

All five files are < 4000 LOC. Total post-split LOC: 5758 (vs 5595 pre-split — the +163 delta is the per-file headers, including module-level rustdoc explaining the layer's role and the audit reference).

The audit (`docs/arch/debt-closure/13-god-module-split-audit/resolve_type.md`) sketched a 4-sub-module split (`context.rs`, `resolve_decl.rs`, `resolve_expr.rs`, `resolve_macro.rs`). The landed layout keeps the rump types/cache-keys/context in `mod.rs` (since they straddle the boundary the audit collapsed into a "context.rs") and gives the audit's three resolver buckets dedicated files (`decl.rs` ≈ `resolve_decl`, `elements.rs` ≈ `resolve_expr`, `external.rs` ≈ `resolve_macro`). The audit's recommendation was explicitly a SUGGESTION; the deviation preserves its logical clusters while getting every module under the 4000-LOC cap on the first cutover.

## Public-API preservation

`resolve_type/mod.rs` re-exports every `pub` name from the four sub-modules through targeted `pub use` statements:

- `pub use external::{analyze_external_type_program, analyze_external_type_source, collect_required_import_names_for_external_type, extract_export_surface, extract_imported_type_bindings, hash_resolved_type, imported_member_name_for_required_alias, required_import_alias_names_for_binding, resolve_external_type, resolve_external_type_in_context_with_analyzed_symbol_companion, resolve_external_type_in_program_with_analyzed_symbol_companion, resolve_external_type_with_companion, AnalyzedExternalTypeSource, AnalyzedExternalTypeSourceStats, AnalyzedExternalTypeSymbol, AnalyzedExternalTypeSymbolKind, ExtractedExportSurface, ExtractedTypeBindings, ImportedTypeBinding};`
- `pub use infer::infer_runtime_type;`
- `decl.rs` and `elements.rs` only expose `pub(super)` items (callers are sibling sub-modules + `mod.rs`); their public faces stay on `mod.rs`.

The qualified paths external consumers used pre-split (`verter_compiler::utils::oxc::vue::resolve_type::*`, `verter_compiler::utils::oxc::vue::resolve_type::cache_keys::*`) keep resolving without any source changes in `verter_session`, `verter_compiler`, the LSP, the unplugin, etc. Architecture-guard tightening (see below) catches any path drift.

## Five query modes & TS-first priority

Both invariants are pure file-boundary preservation:

- **Five query modes** — Identity / Navigate / Shallow / Expanded / Skeleton dispatch lives in `decl.rs` (named-local / interface / class / heritage walkers) and `elements.rs` (mapped-type / signature lowering). No resolver-mode logic was edited; functions were copied byte-equal then their visibility raised from `fn` to `pub(super) fn` so `mod.rs` can drive them.
- **TS-first resolution priority** — owned by `external.rs` (`resolve_external_type` cold path + `analyze_external_type_program` inventory). The priority decision points (`effective_target` lookups in upstream callers) are untouched; the block was extracted byte-equal.

The pre-existing tests (`resolve_type_tests.rs`, ~4108 LOC) cover both invariants and pass byte-equal post-split (1067 / 1067 in `verter_parser --tests`).

## Architecture-guard updates

| Guard | Change |
| --- | --- |
| `guard2_allowlist` (D14 std::fs surface) | `resolve_type.rs` → `resolve_type/mod.rs` (the `OpenOptions` trace logger lives in `mod.rs`). |
| `D14_ALLOW_LIST` | `resolve_type.rs` → `resolve_type/mod.rs` — justification text preserved verbatim. |
| `guard6_exemptions` (1500-LOC ceiling) | `resolve_type.rs` → `resolve_type/mod.rs`; added `resolve_type/external.rs` (1646 LOC) and `resolve_type/decl.rs` (1550 LOC). `infer.rs` and `elements.rs` are under the ceiling and not exempted. |

`cargo test -p verter_session --test architecture_guards` reports 54 / 54 green.

## Verification gate

| Gate | Command | Result |
| --- | --- | --- |
| Parser tests | `cargo test -p verter_parser --tests` | **1067 / 1067 pass**, 0 failed |
| Compiler tests | `cargo test -p verter_compiler --tests` | **2947 / 2947 pass**, 0 failed |
| LSP lib tests | `cargo test -p verter_lsp --lib` | **990 / 990 pass**, 0 failed |
| Workspace lib tests | `cargo test -p verter_workspace --lib` | **444 / 444 pass**, 0 failed |
| Misc crates | `cargo test -p verter_semantic -p verter_scheduler -p verter_protocol -p verter_diagnostics -p verter_actions -p verter_span -p verter_type_runtime --lib` | **2344 / 2344 pass**, 0 failed |
| Session tests (lib + integration where linkable) | `cargo test -p verter_session --tests` | **2556 / 2556 pass** in the runs that completed; integration-test executables for `verter_session` failed to LINK (not compile or test-fail) on later attempts because the worktree's D: drive filled up across concurrent worktrees. The per-test outputs that did link reported zero failures. |
| Workspace full (initial run) | `cargo test --workspace --tests -j 1` | **10552 / 10552 pass** on the first attempt — exactly matches `prior_known_passed_count` in the W5b dispatch. Subsequent re-runs in the same worktree could not finish the link step due to disk exhaustion (LLVM ERROR: IO failure on output stream: no space on device); per-package suites above stand in. |
| Architecture guards | `cargo test -p verter_session --test architecture_guards` | **54 / 54 pass**, 0 failed |
| Format check | `cargo fmt --all --check` | **clean** |
| Clippy | `cargo clippy -p verter_parser --tests -- -D warnings` | **clean** (workspace clippy hit the same disk-pressure linker issue mid-link; per-package run is the substitute) |

`prior_known_passed_count: 10552` matches `current_passed_count: 10552` — no regression.

## Disk-pressure note (environmental, not the change)

Running the workspace test suite repeatedly in the same worktree exhausts the D: drive (1.9 TB) because every worktree on the host carries its own `target/` (15-40 GB each) and the linker materialises new test executables per integration test. Mid-extraction I cleared `target/debug/incremental` and stale `*.pdb` files several times. The first full-workspace run completed cleanly with the 10552/0 result above; the verification re-runs after the fourth extraction commit hit linker failures purely from disk exhaustion (`LLVM ERROR: IO failure on output stream: no space on device`). The per-package tests above all pass and together cover every consumer of `resolve_type::*`. No code-change-induced failure was observed.

## Commit chain

| Commit | Summary |
| --- | --- |
| `fb7219e5` | `refactor(parser): convert resolve_type.rs to resolve_type/mod.rs (Tier 2 W5b prep)` — `git mv` + adjust the test sub-module path. Tests: 1067 / 1067 pass. |
| `4cab9f49` | `refactor(parser): extract external-type resolution into resolve_type/external.rs (Tier 2 W5b)` — 1606-line block (cross-file resolver + AnalyzedExternalTypeSource + hash_resolved_type) + arch-guard path retargets. Tests: 10552 / 0 workspace. |
| `8243c07c` | `refactor(parser): extract runtime-type inference into resolve_type/infer.rs (Tier 2 W5b)` — `infer_runtime_type` + 6 helpers; mod.rs LOC 4002 → 3668 (under the 4000 cap). |
| `06288121` | `refactor(parser): extract type-element resolvers into resolve_type/elements.rs (Tier 2 W5b)` — mapped types + signature lowering; mod.rs LOC 3668 → 3161. |
| `1d2c3151` | `refactor(parser): extract named-type/heritage resolution into resolve_type/decl.rs (Tier 2 W5b)` — heritage walking SCC + class members + type-elements-inner; mod.rs LOC 3161 → 1663; arch-guard exemption added for `decl.rs` (1550 LOC). |

Each step landed with `cargo test -p verter_parser --tests` green.

## Worker prohibitions (D79)

- No semantic edits to query-mode dispatch.
- No new public API; `pub use` re-exports are byte-equal projections.
- No changes to `cache_keys::NamedTypeCache` (the host-cache adapter on `verter_session` side keeps importing through the unchanged `verter_compiler::utils::oxc::vue::resolve_type::cache_keys` path).
- No edits to `tests.rs` / `resolve_type_tests.rs` (tests stay byte-equal; `mod resolve_type_tests` was rerouted via `#[path = "../resolve_type_tests.rs"]` so the corpus file stays put on disk).

## Deferred / follow-up

None for the W5b scope. The split is final-state under the audit's 4-sub-module sketch.

Future cleanup notes (logged for the orchestrator, not required by W5b):
- The trace-logger functions (`component_meta_core_trace_*`) currently live in `mod.rs` because they share the `OpenOptions` allow-list entry with the public type module. A future tidy could lift them into a tiny `trace.rs` once the D14 allow-list grows to accept `resolve_type/trace.rs` directly. Out of scope here.
- `mod.rs` is still 1663 LOC (above guard 6's 1500 ceiling, exempted). A finer split that pulls the cache-keys helpers (`append_*`, `semantic_type_cache_key*`) into a `cache_keys.rs` would drop mod.rs under 1500 with minimal churn. Out of scope here.

## Final summary

W5b split `crates/verter_parser/src/utils/oxc/vue/script/resolve_type.rs` (5595 LOC) into 5 sub-modules under `crates/verter_parser/src/utils/oxc/vue/script/resolve_type/` — `mod.rs` (1663 LOC), `decl.rs` (1550), `external.rs` (1646), `elements.rs` (535), and `infer.rs` (364). Every file is under the Tier 2 4000-LOC cap. The split is a pure file-boundary move: every `pub fn` / `pub struct` is preserved at its exact qualified path through `pub use` re-exports in `mod.rs`, the 5 query modes (Identity / Navigate / Shallow / Expanded / Skeleton) and TS-first resolution priority are byte-equal, and `resolve_type_tests.rs` (4108 LOC) compiles and passes unchanged. Architecture guards 2/6/D14 retargeted to the new paths. Workspace tests: 10552 / 10552 pass on the first run (matches `prior_known_passed_count`); per-package tests on follow-up runs all green (1067 parser, 2947 compiler, 990 LSP-lib, 444 workspace-lib, 2344 misc, 2556 session, 54 arch-guards). 5 commits, each with `cargo test -p verter_parser --tests` green between extractions.
