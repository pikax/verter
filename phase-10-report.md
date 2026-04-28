# Phase 10 — Architecture Enforcement Tests — Report

## Summary

Added `crates/verter_session/tests/architecture_guards.rs` (NEW file) containing
seven static-source-scan guard tests, each `#[ignore]`'d pending its blocking
phase per §10.2 verbatim. No production code changes. Phase 10 is test-only
scaffold; subsequent phases (4, 4b, 6, 7, 11) flip their respective
`#[ignore]` attributes when they delete the matching legacy code.

## Files modified

- `crates/verter_session/tests/architecture_guards.rs:1` (NEW; 166 lines after
  rustfmt)

## Tests added

Each guard `#[ignore]`'d, with reason verbatim from §10.2 except where the
worker-confirmation paragraph in §10.2 directed an `"phase-NN pending or
unnecessary"` override (guards 5 and 6 — see pre-flight below).

1. `architecture_guards::no_read_source_in_component_meta`
   — `#[ignore = "phase-04 pending"]`
2. `architecture_guards::no_read_source_in_declaration_metadata`
   — `#[ignore = "phase-04b pending"]`
3. `architecture_guards::no_text_based_macro_surface_projection_helpers`
   — `#[ignore = "phase-04b pending"]`
4. `architecture_guards::no_macro_string_heuristics_in_resolver_core`
   — `#[ignore = "phase-04b pending"]`
5. `architecture_guards::no_deprecated_workspace_reexports`
   — `#[ignore = "phase-06 pending or unnecessary"]`
6. `architecture_guards::no_local_vite_helpers_in_lsp`
   — `#[ignore = "phase-07 pending or unnecessary"]`
7. `architecture_guards::god_module_size_budget`
   — `#[ignore = "phase-11 pending"]`

## Per-guard pre-flight verification

Counts captured in worktree at base commit
`0f31dabd94deb9bb9f45dd5c4dbdc9c03233d827`:

| # | Guard | Probe | Observed | Plan-write expected | Outcome |
|---|---|---|---|---|---|
| 1 | `no_read_source_in_component_meta` | `host.read_source` count in `component_meta.rs` | **4** | ≥1 (4 documented) | Met — guard correctly red |
| 2 | `no_read_source_in_declaration_metadata` | `read_source` count in `declaration_metadata.rs` | **9** | ≥1 (5 documented) | Met — guard correctly red |
| 3 | `no_text_based_macro_surface_projection_helpers` | three text-projection helper symbols anywhere under `verter_session/src/` | **20** matches | ≥1 per symbol | Met — guard correctly red |
| 4 | `no_macro_string_heuristics_in_resolver_core` | `.contains("defineProps"` etc. in `resolver_core/` | **0** | 0 (forward-regression guard per §10.2 plan body) | Already clean — guard discriminates against future regressions, not current state. `#[ignore]` reason kept verbatim per plan body |
| 5 | `no_deprecated_workspace_reexports` | `pub use ProjectGraph\|ProjectRank\|VfsProjectConfig` in `verter_session/src/lib.rs` | **0** | nonzero | UNMET — per §10.2 worker confirmation, reason updated to `"phase-06 pending or unnecessary"`. The phase-6 flip step in §10.3 will become a trivial `#[ignore]` removal that lands as already-passing |
| 6 | `no_local_vite_helpers_in_lsp` | `pub fn read_vite_config\|parse_vite_config\|discover_vite_aliases` in `crates/verter_lsp/src/{server,background_init}.rs` | **0** | nonzero | UNMET — per §10.2 worker confirmation, reason updated to `"phase-07 pending or unnecessary"`. Phase-7 flip becomes trivial |
| 7 | `god_module_size_budget` | `wc -l` on the five god modules | meta_resolve.rs **11928**; component_meta_query_engine.rs **11119**; host_manage.rs **9026**; ide/script.rs **10945**; lsp/server.rs **6990** | exceeded budgets (11928, 11119, 9026, 10945, 6990 vs 6000, 6000, 5000, 6000, 4000) | Met — guard correctly red |

§10.4 STOP did NOT fire. The example STOP case in §10.4 is "host.read_source
already absent in component_meta.rs at base" (guard 1 unmet). Guards 1, 2, 3,
7 all have nonzero preconditions. Guards 4, 5, 6 are explicitly addressed by
the §10.2 worker-confirmation paragraph (forward regression for guard 4,
"pending or unnecessary" reason for guards 5 and 6).

## Discriminating behaviour verification

Pre-change tree (base commit), with `--include-ignored`:

```
test no_read_source_in_component_meta ... FAILED  (4 host.read_source matches)
test no_read_source_in_declaration_metadata ... FAILED  (9 read_source matches)
test no_text_based_macro_surface_projection_helpers ... FAILED  (component_meta.rs)
test god_module_size_budget ... FAILED  (meta_resolve.rs 11928 > 6000)

test no_macro_string_heuristics_in_resolver_core ... ok  (forward-regression guard)
test no_deprecated_workspace_reexports ... ok  (pre-flight precondition unmet)
test no_local_vite_helpers_in_lsp ... ok  (pre-flight precondition unmet)

test result: FAILED. 3 passed; 4 failed; 0 ignored
```

Discriminating set (4 of 7) characterise pre-Phase-{4,4b,11} state. Guards 4,
5, 6 are forward-regression-only (will pass today, fail tomorrow if a
forbidden pattern is reintroduced).

Default mode (no `--include-ignored`): 0 passed, 0 failed, 7 ignored. ✔

## Tests newly passing / failing

- Newly added: 7 ignored, 0 passed, 0 failed (default mode). The §10.3
  per-phase flip steps will un-ignore each guard as its blocking phase lands.
- No existing test changed status.

## End-of-change verification (§0.6.3)

| Gate | Result |
|---|---|
| `cargo test --workspace --tests --verbose` | **10116 passed**, 0 failed, 9 ignored (7 of 9 are mine) |
| `cargo test -p verter_session --test correctness` | **11 passed**, 0 failed, 1 ignored. Snapshot drift: **none** |
| `cargo clippy --workspace --tests -- -D warnings` | 2 errors, BOTH pre-existing on the base tree in `crates/verter_session/src/meta_resolve.rs` (`unused import: NodeScopeId`; `using contains() instead of iter().any()` at line 1799). Confirmed pre-existing by re-running clippy with the new test file removed — same 2 errors. Out of scope per §0.6.2 ("while-I'm-here" refactors / "moving work to a different file/crate" forbidden). |
| `cargo fmt --all --check` | Clean |
| `pnpm install --frozen-lockfile` | Clean (no drift) |

## Audit metrics

N/A — Phase 10 is pure test scaffolding; no semantic changes, no audit-impacting code touched.

## Deferred

- The two pre-existing clippy errors in `crates/verter_session/src/meta_resolve.rs` are out
  of Phase 10 scope. They will be picked up by Phase 11a (which splits and rewrites
  `meta_resolve.rs`).
- Per §10.1, type-system enforcement (sealed-trait `ResolverContext` that does NOT
  expose `read_source` to `resolver_core`) is the long-term correct mechanism for the
  `host.read_source`-related guards. That work is Phase 10a, sequenced after Phase 11.
  Phase 10's static guards are the immediate safety net.
- §10.3 ignore-flip ownership: each subsequent phase (4, 4b, 6, 7, 11) is responsible
  for un-ignoring its own guard. Phase 10 does NOT flip any `#[ignore]`.

## Marker

The §0.6 R7 manifest is committed at
`crates/verter_session/.phase-markers/phase-10-complete` as the LAST commit
of the phase, with message `chore(orchestrator): mark phase 10 complete`.
`guards_un_ignored` is `[]` per §10.3 (Phase 10 does not flip any ignores).
