# Phase 11c report

## Summary

Phase 11c splits `crates/verter_session/src/host_manage.rs` (9194 LOC at HEAD `65a1bd77` post-11b) into a slim parent shell plus 8 sibling files under `crates/verter_session/src/host_manage/`, following the binding 9-commit plan in §11c.4 of `D:/tmp/verter-architecture-cutover-phase-11c.md`. Public surface (per §11c.1.a–e) was preserved verbatim — zero renames, zero signature changes, zero new `pub` symbols. Only one targeted re-export block was added at the parent shell (per §11c.5) to keep the `crate::host_manage::populate_*` / `crate::host_manage::extract_*` paths used by `meta.rs`, `component_meta_host.rs`, and `component_meta_resolution_policy.rs` resolving after the Domain K free functions moved to a sibling.

The 9-commit plan (1 scaffold + 8 ownership-domain moves; final commit also writes the `phase-11c-complete` R7 marker manifest) was preceded by one prefix commit (`bbff15d9 style(session): …`) absorbing pre-existing fmt drift in 8 `meta_resolve` files inherited from the 11b chain. Without that prefix, the project's husky / lint-staged pre-commit hook (which runs `cargo fmt --check --` workspace-wide, not per-staged-file) would have rejected every Phase 11c commit. The fix is purely mechanical (import reordering + line-wrap normalisation), zero behaviour change. The orchestrator may choose to revert this prefix commit independently of the 11c chain if a different remediation is preferred.

Workspace tests stay green throughout the entire chain at the post-11b baseline of **10283 passed, 0 failed, 4 ignored**. The architecture guard `cargo test -p verter_session --test architecture_guards no_unbounded_recursion_in_resolver_core` remains `#[ignore]` per its existing `phase-05l pending` annotation; Phase 11c does not flip it.

## Files

### Modified
- `crates/verter_session/src/host_manage.rs` — shrunk from 9194 LOC to 1583 LOC; now a slim shell holding Domains A (module helpers + telemetry), B (resolver-host adapters), C (ID/extension helpers + small `impl VerterHost` block), L (test-mod include), plus the §11c.5 re-export block.

### New (8 sibling files under `crates/verter_session/src/host_manage/`)
- `crates/verter_session/src/host_manage/eval_program.rs` — 627 LOC (Domain D: eval-source / parsed-program / external-type-analysis bridge)
- `crates/verter_session/src/host_manage/intrinsic_projection.rs` — 601 LOC (Domain E: project-scoped HTML intrinsic projection)
- `crates/verter_session/src/host_manage/prepared_decl.rs` — 1483 LOC (Domain F: prepared-decl + shallow-state + import-route resolution)
- `crates/verter_session/src/host_manage/component_meta_entry.rs` — 424 LOC (Domain H: public component-meta entry points + audit dispatch)
- `crates/verter_session/src/host_manage/eval_env.rs` — 1017 LOC (Domain G: eval-env + snapshot constructors + evaluated-type compute)
- `crates/verter_session/src/host_manage/fallthrough.rs` — 1003 LOC (Domain I: fallthrough resolution pipeline + runtime node ↔ resolution conversions)
- `crates/verter_session/src/host_manage/analysis_io.rs` — 1605 LOC (Domain J: file analysis / source / template / diagnostics / files / set-import-deps / css-var-flow / export-graph)
- `crates/verter_session/src/host_manage/component_meta_extract.rs` — 1100 LOC (Domain K: component-meta extraction free functions + JSDoc enrichment + SFC sidecar population)

### Visibility bumps in parent shell (host_manage.rs)
Several private items in `host_manage.rs` were promoted from default-private to `pub(in crate::host_manage)` so the new sibling files can reference them via `super::*`. The visibility scope is the `host_manage` module subtree only — these are NOT new public-API or pub(crate) symbols. The bumped items:

- `read_analysis_source_result_detail` (private fn → `pub(in crate::host_manage)`)
- `ParsedEvalProgramCacheKey` (private struct → `pub(in crate::host_manage)`, fields included)
- `ParsedEvalProgramCacheEntry` (private struct → `pub(in crate::host_manage)`, fields included)
- `ParsedTypeResolutionContextCacheEntry` (private struct → `pub(in crate::host_manage)`, fields included)
- `HostNamedTypeCacheAdapter` (private struct → `pub(in crate::host_manage)`, fields included)
- `HOST_PARSED_EVAL_PROGRAM_CACHE` (private thread_local → `pub(in crate::host_manage)`)
- `HOST_PARSED_TYPE_CONTEXT_CACHE` (private thread_local → `pub(in crate::host_manage)`)
- `is_runtime_script_target` (private fn → `pub(in crate::host_manage)`)
- `is_builtin_type_symbol` (private fn → `pub(in crate::host_manage)`)
- `dep_edges_from_resolutions` (private fn → `pub(in crate::host_manage)`)
- `HostShallowImportResolver` (private struct → `pub(in crate::host_manage)`, fields included)
- `log_snapshot_debug` (private fn → `pub(in crate::host_manage)`)
- `HostFallthroughResolver` (private struct → `pub(in crate::host_manage)`, fields included)
- `HostRuntimeValueResolver` (private struct → `pub(in crate::host_manage)`, fields included)
- `STORE_VIEW_STABILITY_MAX_ATTEMPTS` (private const → `pub(in crate::host_manage)`)
- `HostExportGraphResolver` (private struct → `pub(in crate::host_manage)`, fields included)
- `exact_resolution_uses_type_preferred_target` (private fn → `pub(in crate::host_manage)`)

### Re-export block (§11c.5)
A single `pub(crate) use self::component_meta_extract::{populate_sfc_blocks_sidecar, populate_public_instance_sidecar, extract_component_meta_from_resolved, extract_component_meta_from_resolved_with_facts};` block was added in the parent shell so the four pub(crate) free functions in Domain K continue to resolve at `crate::host_manage::*` for the external callers identified in §11c.1.e.

## Final LOC breakdown (per §11c.3 budget = 2000 LOC)

| File | Final LOC | Brief target | Status |
|---|---|---|---|
| host_manage.rs (shell) | 1583 | ≈1555 | within tolerance |
| host_manage/eval_program.rs | 627 | ≈605 | within tolerance |
| host_manage/intrinsic_projection.rs | 601 | ≈585 | within tolerance |
| host_manage/prepared_decl.rs | 1483 | ≈1455 | within tolerance |
| host_manage/component_meta_entry.rs | 424 | ≈400 | within tolerance |
| host_manage/eval_env.rs | 1017 | ≈990 | within tolerance |
| host_manage/fallthrough.rs | 1003 | ≈965 | within tolerance |
| host_manage/analysis_io.rs | 1605 | ≈1565 | within tolerance |
| host_manage/component_meta_extract.rs | 1100 | ≈1070 | within tolerance |
| **total** | **9443** | **≈9190** | parent + siblings |

Largest sibling: `analysis_io.rs` at 1605 LOC, well under the 2000-LOC budget. The 4000-LOC `god_module_size_budget` guard remains `#[ignore]` (Phase 11e flips per parent §11.4 — not touched here).

## Verification commands run + results

| Command | Result |
|---|---|
| `cargo test --workspace --tests --verbose` | **10283 passed, 0 failed, 4 ignored** (matches post-11b baseline; saved to `/tmp/p11c-marker-verify.txt`) |
| `cargo test -p verter_session --test correctness` | **18 passed, 0 failed, 1 ignored** (snapshot drift = `none`) |
| `cargo fmt --all --check` | Exit code 0 (clean) |
| `pnpm install --frozen-lockfile` | Done in 2s, no lockfile drift |
| `cargo test -p verter_session --test architecture_guards no_unbounded_recursion_in_resolver_core` | 1 ignored (`phase-05l pending` — pre-existing, not flipped by 11c) |

## Discoveries

### Pre-existing fmt drift (predecessor 11b)
At HEAD `65a1bd77` (post-11b marker commit), `cargo fmt --check` reports drift in 8 `meta_resolve` files inherited from the 11b refactor chain (commits `2f1a97c2`, `87e801c8`, `96dfcd0f`, `ce2c1cab` in particular). The drift is mechanical (~150 LOC of import reordering and rustfmt line-wrap normalisation, zero behaviour change). It pre-dates Phase 11c. The project's husky pre-commit hook runs `cargo fmt --check --` workspace-wide on every commit that has staged Rust files; this drift caused that hook to fail on the very first 11c commit attempt.

I absorbed the fmt fix as a separate, clearly-attributed prefix commit (`bbff15d9 style(session): apply cargo fmt --all to absorb pre-11c baseline drift`) ahead of the §11c.4 9-commit chain. This is a deviation from the strict REORG-ONLY scope of Phase 11c, justified by:

1. The brief's §0.6.3 verification gate requires `cargo fmt --all --check` to pass at end of phase.
2. CLAUDE.md (global) and the brief's R5 forbid `--no-verify`.
3. Touching only meta_resolve files (which are outside Phase 11c's scope) but only for mechanical fmt fixes (zero semantic change).
4. The prefix commit is a single, isolated diff from the rest of the 11c chain.

The orchestrator may choose to revert `bbff15d9` and absorb the fmt fix into the integration branch independently if that is preferred. The 11c chain stands on its own without `bbff15d9` as far as semantic correctness goes.

### Line-number drift relative to §11c.2 anchors
The brief's §11c.2 anchors are derived from HEAD `97919667` (an earlier verification HEAD). At my actual base (`65a1bd77`, file size 9194), the impl-block anchor is at line 1439 (matches brief's expected `±10` tolerance). Per-domain start lines drifted within the brief's stated `±50` tolerance. No re-derivation was required.

### Mojibake in pre-existing doc comments
Lines like `compile_from_parsed() â€" bypassing` (4 instances at HEAD `65a1bd77`) carry pre-existing UTF-8 → CP1252 mojibake (the â€" sequence is a misencoded em-dash). These were carried verbatim during the moves; I did not touch them. They remain in `analysis_io.rs` after the move. Cleanup is out of scope for Phase 11c.

### Visibility scope choice
For all visibility bumps from `private` to a wider scope, I chose `pub(in crate::host_manage)` (visible only within the host_manage module subtree) rather than `pub(crate)`. This keeps the visibility narrowest while still letting the new sibling files compile, and avoids creating any new `pub(crate)` symbols that would weaken §11c.1's "no new pub or pub(crate) items" rule.

### Test attachment (`#[cfg(test)] #[path = "host_manage_tests.rs"] mod tests;`)
Stays in the parent shell `host_manage.rs` per brief §11c (constraint 3). I did NOT move `host_manage_tests.rs`. Tests reference parent items via `super::*`; they continue to reach symbols in sibling modules through the parent's `pub(crate) use` re-exports plus the `pub(in crate::host_manage)` visibility bumps.

## Anything deferred

None. Phase 11c is an atomic-gate phase per §0.3 ATOMIC_GATE_PHASES allowlist (post-r17 atomic-gate phases must have `status: "success"` AND `deferred: []`). The 9-commit chain landed in its entirety. No sub-step was scoped out by design.

## Commit list

| # | Commit | Subject |
|---|---|---|
| prefix | `bbff15d9` | `style(session): apply cargo fmt --all to absorb pre-11c baseline drift` |
| 11c.1 | `e9d4a010` | `refactor(session): create host_manage submodule scaffold` |
| 11c.2 | `f781c556` | `refactor(session): move eval-program / external-type-analysis bridge` |
| 11c.3 | `e7b805c0` | `refactor(session): move intrinsic-projection pipeline` |
| 11c.4 | `99db4ead` | `refactor(session): move prepared-decl + shallow-state pipeline` |
| 11c.5 | `89ce3bf0` | `refactor(session): move public component-meta entry points` |
| 11c.6 | `9e6cb42d` | `refactor(session): move eval-env + snapshot constructors` |
| 11c.7 | `1a567c9a` | `refactor(session): move fallthrough resolution pipeline` |
| 11c.8 | `884a5673` | `refactor(session): move analysis / files / diagnostics / export-graph methods` |
| 11c.9 | (next commit) | `refactor(session): move component-meta extraction free fns + write phase-11c marker` |
