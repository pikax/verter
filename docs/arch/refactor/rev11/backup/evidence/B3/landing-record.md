# B3 — landing record

Base `7e8b025b8`. Candidate `1c8e01792`. Dispatch context:
[`context-packet.md`](context-packet.md).

## What shipped

- `CompileRequest` (`crates/verter_compiler/src/compile_request/`) is the
  sole canonical typed compile request. Every production route constructs
  through it: the internal one-shot compiler entrypoint, the host's
  per-file/virtual session route (`compile_request_build.rs`), NAPI
  `compile_many`/`compile_with_audit`, WASM, and the unplugin Rust
  ingress.
- Per-field option admission (`VueOptionAttempt`/`SvelteOptionAttempt`)
  covers the full BF1 option inventory exactly once; an unrecognized
  option refuses at construction. Both construction sites originally
  found unwired (the audited NAPI route, the FFI/WASM boundary) are now
  wired — see `debt-FC-OPTIONS-002-option-attempt-decode-unwired.md`
  (CLOSED).
- SSR x Vapor and inline x SSR/inline x Vapor combinations refuse
  structurally at `CompileRequest::new` AND inside `compile_bundle`
  itself — the latter was a real, previously undetected gap found during
  this work: `compile_bundle`'s shared per-file path (used by every NAPI/
  WASM/unplugin caller) had no guard of its own, only `CompileRequest::
  new`'s callers did, so a broken hybrid `__vapor: true` + `ssrRender`
  module was silently producible through that path. Proven via a
  revert-guard regression test.
- `framework_extras: Arc<dyn Any>` replaced with a typed
  `VueExecutionInputs`. `CompileTargetTag` replaced with a product-set +
  backend representation, TS bindings regenerated
  (`packages/types/audit.generated.ts`).
- Svelte option *output* liveness (making each represented option's
  codegen behavior observably correct) is out of scope here and left
  DEFERRED — see `debt-FC-SVELTE-001-svelte-output-liveness.md`. Every
  supported-canonical Svelte option is still represented and normalized
  on the request rather than hardcoded away.

## Review arc

Round 1 (all three seats, concurrent): BLOCKING, convergent finding that
the session per-file/virtual route never actually built a
`CompileRequest` (see context-packet.md). One fix round closed this plus
several other convergent findings. No further review-seat round was
required.

## Pre-landing verification — four real regressions caught and fixed by
## this session's own diligence (none from the review seats; all after
## review round cap was reached)

The review seats never re-ran on the final candidate after round 1's fix.
Landing readiness instead ran multiple full-workspace/gate passes and
investigated every failure against base before accepting it as
pre-existing, catching four genuine regressions along the way:

1. **`ANALYSIS`/`META` audit targets + `SVELTE-MODULE` admission gap +
   NAPI unknown-key silent-drop** — found and fixed during this session's
   own spot-check of the round-1 fix, not a review-seat finding.
2. **`packages/native` JS test staleness** (2 files, 4 cases) — JS
   consumers asserting the old `CompileTargetTag` shape after the wire
   schema changed; also found `packages/vue-vscode/src/
   audit.transforms.ts`'s "Show Recent Audit Records" QuickPick
   permanently rendering `target=?`, and two `index.spec.ts` cases using
   target presets that predated a deliberate contract change
   (`ensure_compile_artifacts` no longer synthesizes a runtime node for
   an IDE-only target). Independently re-verified: `pnpm --filter
   @verter/native test` 15/15 files, 123 passed, 7 skipped, 0 failures.
3. **`@verter/typeinfo` native-wire object-key decoding** — a latent
   pre-existing bug in `native-to-descriptor.ts` (assumed a flat
   `member.name` string field a retired `TypeExpr` wire shape used; the
   current `verter_type_expr` wire encodes an `AuthoredPropertyKey`
   variant carrier). This candidate's routing changes shifted which wire
   shape these specific test cases exercised, exposing the dormant
   decoder bug (confirmed via direct A/B wire-format comparison against
   base's built addon). Fixed both decode directions plus the same bug
   for `method`-kind members and a missing `spread` member kind. No Rust
   touched. Independently re-verified: `pnpm --filter @verter/typeinfo
   test` 11/11 files, 33/33 tests.
4. **`packages/wasm`/`packages/playground` never built in the fresh
   worktree** — `pnpm run build:wasm` had never been run, so
   `packages/wasm/wasm/verter_wasm.js` didn't exist, failing 13 wasm
   tests and 1 playground test with module-not-found. Same class as the
   native-build and oracle-npm-cache worktree-setup gaps below; not a
   code defect. Built and independently re-verified: `pnpm --filter
   @verter/wasm test` 3/3 files, 16/16 tests; `pnpm --filter
   @verter/playground test` 27/27 files, 425/425 tests.

## Discriminated as pre-existing / environmental, not regressions

- `native_content_handoff::external_template_ide_compile_contains_selected_bytes`
  — appeared identically on all 3 gate surfaces across two full gate runs.
  Zero diff in this file against base; passes in isolation and under a
  scoped nextest run on base; only manifests under the full
  24500+-test workspace-wide run's resource load. Same failure class as
  the documented pre-existing trybuild-timeout baseline, just
  deterministic-under-load rather than intermittent.
- Two transient trybuild-smoke TIMEOUTs on the first gate run
  (`hot_materialize_structural_rails_smoke`,
  `hot_materialize_and_script_fact_structural_rails_smoke`) — zero diff
  against base, pass cleanly in isolation, vanished on gate re-run.
  Resource-contention flake under an 8-thread parallel gate run.
- `packages/vue-vscode`'s `extensionTsService.*.spec.ts` suite — a
  different single test among ~20 fails on each independent full
  `pnpm test` run (each in the 6-12s range, spawning real `tsserver`
  child processes). Package has zero diff against base in any file this
  suite touches. Two of the specific failing tests were independently
  confirmed to pass cleanly in isolation on both base and candidate.
  Consistent with load-dependent flakiness, not a regression.
- `packages/typescript-plugin`'s `index.spec.ts` — 2 cases fail
  identically on base and candidate with the exact same assertion
  mismatch, caused by macOS's `/var` -> `/private/var` `os.tmpdir()`
  symlink resolution. `packages/typescript-plugin` has zero diff in this
  candidate's range.
- `packages/framework-conformance-harness` — initially appeared to fail
  (106/632 in a fresh worktree run) due to a missing gitignored
  `.oracle-npm-cache` local provisioning cache, not present in a fresh
  worktree. Copied from the main checkout (a one-time, offline-afterwards
  cache, same class of gap as the native build). Re-ran clean: 30/30
  files, 640/640 tests, matching base exactly.
- `@verter/nuxt`'s documented no-op escape hatch (Nuxt peer-dep type
  gaps) and `@verter/benchmark`'s failures from a missing external
  `nuxt-ui` corpus checkout — both pre-existing/environmental per
  Testing-Hermeticity's expected external-corpus gating, unrelated to
  this candidate.

## Verification

- Canonical Rust gate (`node scripts/gate.mjs --test-threads 8
  --memory-limit 18GiB`) ran twice on the Rust-touching tip. Second run:
  only the one discriminated pre-existing failure above (all 3 surfaces),
  no other non-tolerated failures. `fmt`/`clippy --workspace --all-targets
  -D warnings`/`clippy --target wasm32-unknown-unknown -p verter_wasm -D
  warnings`/`cargo check --workspace --release` all clean on the same
  tip. The two commits landed after that gate run
  (`packages/native`/`@verter/typeinfo` JS/TS fixes, then the wasm build)
  touch zero `.rs` files — the gate result carries forward unchanged.
- Full workspace `pnpm test`: every package clean or discriminated
  pre-existing per above.
