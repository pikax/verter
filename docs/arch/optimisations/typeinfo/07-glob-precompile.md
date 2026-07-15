# T8 — Precompile workspace membership globs at snapshot build

**Level:** micro. **Risk:** low — match semantics pinned by parity tests.
**Reference implementation:** branch `perf/t8-glob-precompile`, commit `93c452021` (measurement machine).

## Problem (profiler evidence)

`crates/verter_workspace/src/normalized_glob.rs::NormalizedGlob::matches` runs
`glob::Pattern::new(&self.0)` on EVERY call. The hot caller is
`WorkspaceSnapshot::owners_for_file` → membership `contains` loops
(`crates/verter_workspace/src/membership.rs`) — ≈ **2 % of pass CPU** in glob compilation + matching.

## Change (compile at the snapshot/membership construction boundary — NOT lazily inside NormalizedGlob)

- New `CompiledGlob { raw: NormalizedGlob, pattern: Option<glob::Pattern> }` in `normalized_glob.rs`;
  compile once at construction; `pattern: None` on invalid glob → `matches() == false`, preserving the
  historical `unwrap_or(false)`.
- Shared `const fn match_options()` used by BOTH `NormalizedGlob::matches` (kept for cold callers) and
  `CompiledGlob::matches`, so the two can never diverge on MatchOptions
  (`case_sensitive: !cfg!(windows)`, `require_literal_separator: true`,
  `require_literal_leading_dot: false`).
- `membership.rs`: `StaticMembershipSpec.include/.exclude` and `FallbackMembership.exclude` become
  `Vec<CompiledGlob>`; `typescript_default_excludes` returns precompiled globs.
- `snapshot_builder.rs::membership_to_spec` compiles once per pattern; the `materialize_from_spec`
  walk gets precompiled matching for free. `NormalizedGlob` value semantics (Clone/Eq/Hash/Display)
  untouched. Export `CompiledGlob` from `lib.rs`.
- NOT migrated (out of scope, noted debt): `resolver.rs::matches_any_pattern_for_root` — a separate
  legacy `Vec<String>` glob path using DEFAULT MatchOptions, not a `NormalizedGlob` caller.
- `owners_for_file` per-snapshot memo evaluated and SKIPPED: `WorkspaceSnapshot` is built via bare
  struct literals at 12+ sites across 3 crates — retrofit cost exceeds the residual win.

## Test contract

- Characterization pins (pre-change, kept green): invalid include pattern → `contains == false`;
  invalid exclude never excludes (spec + fallback); `owners_for_file` three-class pin (member file /
  excluded file / non-member) under glob-bearing memberships.
- `CompiledGlob` unit tests: parity table vs `NormalizedGlob::matches`, invalid-pattern behavior,
  raw-string preservation, `From` conversions.
- `cargo test -p verter_workspace` (497+4 green on reference; baseline 489+4) and the
  `verter_session --lib workspace` filter (71 green).

## Measured result (this machine)

Full pass (post-fix protocol, median of 3 interleaved runs): steady 20 480 → **20 076 ms (−2.0 %)** —
small but consistent (every t8 run ≤ every baseline run); p95 345 → 332 ms; peak RSS ~flat. Keep: the cost
is trivial and the win is real on workspace-heavy operations beyond this benchmark.

## Follow-up: per-root memo for the default-exclude set

Compiling per membership construction was still per-CALL for the TS default excludes: allocation
sampling over a 179-component pass attributed ~100 MB of transient allocations (6 distinct stacks)
plus ~1.4% self CPU in `glob::Pattern::new` to `typescript_default_excludes`, driven by hot callers
that reconstruct memberships (`IdeProjectConfig::new` via the workspace-default env-hash/identity
helpers in `engine.rs`, invoked per store-view/no-owner env read). Now: a process-wide per-ROOT memo
(`LazyLock<RwLock<FxHashMap<CanonicalPath, Arc<[CompiledGlob]>>>>`, bounded, clear-on-overflow) and
`StaticMembershipSpec.exclude` / `FallbackMembership.exclude` are `Arc<[CompiledGlob]>` — each root
compiles its three default-exclude globs once per process; membership clones stop deep-cloning them.
The include default (`{root}/**/*`) still compiles per construction — falls out when the
workspace-default env-hash reconstruction itself is cached (recorded as the remaining owner-layer fix).
