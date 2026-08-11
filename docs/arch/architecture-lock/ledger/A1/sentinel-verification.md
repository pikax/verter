# A1 Sentinel Verification (`verification.md` §2) — round 2

Requirement: "Sentinel verification is required for critical harnesses: in an
isolated non-candidate run, deliberately break one known assertion or test fixture
and prove the canonical selector fails."

**Isolated copy:** a full `git clone` of the A1 candidate worktree, updated to the
FINAL candidate `13cedd6fc1315bfb6fec0c4cacb0eacdb02c6c83` (branch
`block/a1-command-truth`, clean, tree `a992bb87382e58d6ec846c7be37cbb941ee0b1b2`),
placed OUTSIDE both the primary checkout and the candidate worktree (referred to
below as `<SENTINEL>`), with its own `pnpm install --frozen-lockfile`, its own TS
builds, and its own cargo target directory. No candidate file was modified at any
point; the candidate worktree remained clean throughout. Per the A1 charter the
sentinel clone is evidence-only scaffolding and was DELETED after the battery
(deletion verified in the exact-candidate record).

**Sequencing:** the candidate worktree's command battery ran first; the clone
battery ran strictly AFTER it (never concurrently), so no sentinel run distorted a
candidate measurement and load conditions stay attributable.

**Plant-proof discipline (CLAUDE.md "Verification Must Prove Execution"):** every
plant carries a globally unique round-2 token. Before each planted run we prove the
token is (a) PRESENT in the planted file, (b) UNIQUE in the sentinel tree (exactly
the expected occurrence count, `git diff --stat` = exactly the plant), and (c) NEW —
zero occurrences in the unmodified candidate tree. A green planted run is treated
as a failure to prove, never as a pass. Where a baseline is RED (the JS suite, the
armed corpus gate), overall exit codes cannot discriminate a plant; the
pre-registered discriminator is the planted failure's OWN identity (present in the
planted run, absent in the restored run), recorded verbatim below.

Raw outputs live in `sentinel-runs/` beside this file; digests in
`command-proofs/index.md`'s final table.

---

# Sentinel A — the Rust gate (`node scripts/gate.mjs --timeout 420m`)

Same invocation as candidate command-proof row 01; run twice in `<SENTINEL>`.

- **Plant:** `panic!("A1_SENTINEL_PLANT_GATE_R2_4b7e1d");` inserted as the first
  statement of `#[test] fn tracked_paths_are_portable_across_platforms()` in
  `crates/verter_session/tests/cases/tracked_paths_are_portable.rs` — a test that
  executes on ALL THREE gate surfaces.
- **Plant-proof:** token 0 before (sentinel AND candidate trees); exactly 1 file
  after; `git diff --stat` = 1 file, 1 insertion — the plant is the only change.
- **Planted run:** exit **1**, `VERDICT: FAIL — 3 non-tolerated failure(s)`, and
  ALL THREE are the planted test, named per surface: `[nextest]`,
  `[libtest:verter_session::main]`, and `[shipped-cfg/nextest]`
  `cases::tracked_paths_are_portable::tracked_paths_are_portable_across_platforms`,
  with the panic backtrace citing the planted line
  (`tracked_paths_are_portable.rs:124:5`). Surface counts: S1 24626 discovered /
  24044 executed / 24043 passed / 1 failed / 582 skipped — the single failure IS
  the plant; S2 2 suites clean + 1 suite failing on the plant; S3 9040 / 8477 /
  8476 / 1 / 563 — the single failure IS the plant.
- **Restore:** `git checkout --` the file; token 0; sentinel tree clean.
- **Restored run:** exit **0**, `VERDICT: PASS (all three surfaces green)` — S1
  24044 executed / 24044 passed / 582 skipped; S2 3 suites clean; S3 8477 / 8477 /
  563 skipped. The planted test name appears only as PASSING; the plant identity is
  gone and the full green profile matches the candidate run (row 01).
- **Verdict: DISCRIMINATING.** The canonical gate cannot stay green over a broken
  known assertion (named on every surface), and restoration returns the candidate
  result exactly.
- Raw: `sentinel-runs/sentinel-A-gate.txt` (driver log + plant proof),
  `sentinel-runs/sentinel-A-gate-{planted,restored}-raw.txt`.

# Sentinel B — the JS suite

- **Plant site (both selectors):** appended `describe("a1 sentinel", …)` with a
  TYPE-LEVEL failure (`assertType<number>("…" as string)`) carrying the unique
  token `A1_SENTINEL_PLANT_VITEST_R2_9e51c3` to
  `packages/types/src/exports.spec.ts` (`@verter/types` is a typecheck-only
  suite — the type-error class is the failure class it owns, a round-1 finding).
  Plant-proof: token 0 before (both trees); 1 file / 6 insertions after; 0 after
  restore.

- **B-canonical — `pnpm test` (the root selector): UNMET, with cause.** The
  planted canonical run exited 1 but delivered NO verdict for the planted suite:
  the raw output contains ZERO occurrences of the token outside this battery's own
  header — `@verter/native`'s pretest failed first in the clone (no built `.node`)
  and the selector's PARALLEL + BAIL-ON-FIRST-FAILURE semantics killed the planted
  suite before it reported (only 3 of 24 packages reached a summary). Per the
  pre-registered discipline that is a failure to prove, not a pass. This
  reproduces the round-1 result on a second independent run: **the canonical root
  JS selector's sentinel is UNMET — the selector is structurally bail-fast and
  cannot reliably deliver a planted suite's verdict on a red baseline.** The
  finding is recorded (not fixed) in command-proof row 08; the discrimination
  below belongs to the `--no-bail` VARIANT, not to the canonical selector.
- **B-no-bail — `pnpm -r --no-bail --parallel run test` (identical package
  selection, bail semantics only): DISCRIMINATING.** Planted run: exit 1 with
  `× A1_SENTINEL_PLANT_VITEST_R2_9e51c3` and `@verter/types`
  `Tests 1 failed | 614 passed (615)`. Restored run: `@verter/types`
  `Tests 614 passed (614)`, token absent from the output. Overall exit stays 1 in
  both directions from the PRE-EXISTING typeinfo/unplugin reds, so discrimination
  is per-suite identity, as pre-registered.
- Raw: `sentinel-runs/sentinel-B-vitest.txt` (driver log),
  `sentinel-runs/sentinel-B-canonical-planted-raw.txt`,
  `sentinel-runs/sentinel-B-nobail-{planted,restored}-raw.txt`.

# Sentinel C — conformance selector (`pnpm run gen:vue-goldens:check`)

- **Plant:** appended `/*A1_SENTINEL_PLANT_GOLDEN_R2_77aa02*/` to the committed
  official-Vue golden artifact
  `crates/verter_vue_conformance/corpus/goldens/3.6.0-rc.1/vdom-inline/built-ins/keep-alive.js`
  — deliberately in the `vdom-inline` tree this round, so all three golden trees
  (vdom, vapor, vdom-inline) have now been sentinel-exercised across rounds.
- **Plant-proof:** 0 before (both trees); exactly 1 after; 1 file / 1 insertion.
- **Planted run:** exit **1**, naming the exact planted artifact:
  `DRIFTED (bytes differ): …/vdom-inline/built-ins/keep-alive.js` and
  `goldens check failed: 0 missing, 1 drifted, 0 stale`.
- **Restored run:** exit **0**, `goldens check OK: 286 committed artifacts match a
  fresh run` — identical verdict to candidate row 12.
- **Verdict: DISCRIMINATING.**
- Raw: `sentinel-runs/sentinel-C-vue-goldens.txt`,
  `sentinel-runs/sentinel-C-{planted,restored}-raw.txt`.

# Sentinel D — the corpus gate (`pnpm --filter @verter/dx-harness test:corpus-gate`) — NEW this round

Run in `<SENTINEL>` against the SAME classified corpus as candidate row 16
(identified only by label `Corpus A` and the content fingerprint in
`command-proofs/index.md`), with `VERTER_CORPUS_GATE_SAMPLE=8` and
`VERTER_CORPUS_GATE_ROUTES=tsgo` to bound the sentinel's wall clock (the selector
itself is byte-identical to the canonical one; only env scale differs, and the
plant acts before any scale knob matters). Clone-local debug `verter-lsp` /
`verter-relay-shim` were built first (`sentinel-runs/sentinel-D-lsp-build.txt`).

- **Plant:** in `packages/dx-harness/src/corpus-gate/spawn.ts`, the spawner's
  binary resolution `platformBinary(repoRoot, "verter-lsp")` was renamed to
  `platformBinary(repoRoot, "verter-lsp-A1_SENTINEL_PLANT_CORPUS_R2_d41c8f")`.
- **Plant-proof:** token 0 before (both trees); exactly 1 file after; 1 file /
  1 insertion + 1 deletion; 0 after restore.
- **Planted run:** exit **1** with the plant NAMED as the failure:
  `[tsgo] fatal route error: verter-lsp binary is required and must be a file:
  …\target\debug\verter-lsp-A1_SENTINEL_PLANT_CORPUS_R2_d41c8f.exe`; the route
  recorded `sent=0` — no probe work could run.
- **Restored run:** exit **1** — but with a COMPLETELY DIFFERENT failure identity
  and real execution: 290 `.vue` files discovered, 8 sampled, a real bounded tsgo
  session ran (49 requests sent, 46 answered, 3 timed out) and failed on the SAME
  pre-existing acceptance-bar class as candidate row 16 (route wedged); the
  planted token appears ZERO times. Because the armed corpus gate is a RED
  baseline (pre-existing product issues), discrimination is per-failure-identity,
  as pre-registered: plant present ⇒ spawn-fatal naming the token with zero
  requests; plant removed ⇒ real corpus execution with the candidate's failure
  class.
- **Verdict: DISCRIMINATING.**
- Raw: `sentinel-runs/sentinel-D-corpus-gate.txt`,
  `sentinel-runs/sentinel-D-{planted,restored}-raw.txt`,
  `sentinel-runs/sentinel-D-lsp-build.txt`.

# Sentinel E — `cargo check --workspace --release` negative control — NEW this round

Command-proof row 03 is the only build-type row whose tool emits no counts and had
no negative control in round 1.

- **Plant:** `const A1_SENTINEL_PLANT_CHECK_RELEASE_R2_66f0: u32 = "type error";`
  appended to `crates/verter_mcp_server/src/main.rs` in `<SENTINEL>`.
- **Plant-proof:** 0 before (both trees); exactly 1 after; 1 file / 2 insertions.
- **Planted run:** exit **101**, `error[E0308]: mismatched types` on the planted
  line (token visible in the diagnostic) and
  `error: could not compile verter_mcp_server (bin "verter-mcp-server")`.
- **Restored run:** exit **0**, `Finished release profile [optimized]`.
- **Verdict: DISCRIMINATING.**
- Raw: `sentinel-runs/sentinel-E-check-release.txt`,
  `sentinel-runs/sentinel-E-{planted,restored}-raw.txt`.

# Supplementary negative control — `cargo fmt --all --check`

- **Plant:** misformatted `fn  a1_sentinel_plant_fmt_r2_31d9( ) ->i32{  1 }`
  appended to `crates/verter_span/src/lib.rs`.
- **Plant-proof:** 0 before (both trees); exactly 1 after; 1 file / 2 insertions.
- **Planted run:** exit **1**, `Diff in …\crates\verter_span\src\lib.rs`.
- **Restored run:** exit **0**, empty output — the candidate result.
- **Verdict: DISCRIMINATING.**
- Raw: `sentinel-runs/fmt-negative-control.txt`,
  `sentinel-runs/fmt-negative-{planted,restored}-raw.txt`.

# Nondeterministic findings (restated — supersedes the round-1 claims)

1. **`verter_language::main cases::compile_fail::registered_authority_capabilities_are_not_mintable_outside_their_authorities`
   (trybuild): NONDETERMINISTIC; NOT REPRODUCED UNDER ISOLATION.** The round-1
   record called this "checkout-environment-sensitive (linked worktree red /
   fresh clone green at the same commit)". That causal claim is WITHDRAWN as
   refuted: the architecture reviewer ran the exact command
   (`cargo test --locked -p verter_language --test main -- compile_fail`) six
   times in the SAME linked worktree at the round-1 candidate — three warm
   nextest runs and two with a cold scratch `CARGO_TARGET_DIR` — and got 6/6
   GREEN (both trybuild tests), while the one observed red ran 23.2s versus 0.9s
   warm. This round adds independent data on the FINAL candidate in the SAME
   linked worktree: the full gate (row 01) ran the test GREEN on its surfaces,
   and four consecutive direct re-runs (row 21) were 4/4 GREEN (10.87s, 1.44s,
   1.24s, 1.31s). Total: 10+ green isolated executions, one red ever observed —
   inside a fully loaded gate run. Classification: nondeterministic (consistent
   with load-widened timing, e.g. a trybuild child-rustc slowdown under full
   parallel load); no worktree-versus-clone cause is asserted anywhere.
2. **`verter_tsc::main cases::fallthrough::fallthrough_attrs_accepted_only_where_they_reach_the_dom`:
   same classification, uniformly applied.** Observed red exactly once in round 1
   (the restored sentinel gate run) and green in every other execution including
   this round's full gate. Nondeterministic; not reproduced under isolation.
3. **`@verter/playground` `wasmInContextLs.spec.ts` carrier-membership case:**
   failed once this round inside the 24-package parallel `--no-bail` run (5.6s),
   green in the prior round's identical run and green in this round's dedicated
   row 11b config. Same classification; recorded, not fixed.
4. **shared-tsgo provider-matrix variance:** 77 passed / 15 failed this round vs
   79 / 13 in round 1 on unchanged provider code (docs-only tree delta) —
   run-to-run variance in the flaky relay class; recorded, not fixed.

These four are the same evidence shape and receive the same label; round 1's
asymmetric treatment (one "checkout-topology defect", one "flake") is corrected.

# Result summary

| Harness | Canonical selector | Plant | Plant proven applied | Planted result | Restored result | Verdict |
|---|---|---|---|---|---|---|
| Rust gate | `node scripts/gate.mjs --timeout 420m` | `panic!` in a 3-surface verter_session test | yes (0→1, 1-file diff, both-tree negative) | exit 1; VERDICT FAIL names the plant on ALL 3 surfaces | exit 0; VERDICT PASS, plant absent | DISCRIMINATING |
| JS suite — CANONICAL `pnpm test` | `pnpm test` (parallel + bail-fast) | type error in `@verter/types` spec | yes | exit 1 but the planted suite's verdict was NEVER DELIVERED (bail-fast killed it; token absent from output) | n/a | **UNMET, with cause** — the canonical selector cannot reliably discriminate on a red baseline; finding recorded in row 08, not fixed |
| JS suite — `--no-bail` VARIANT | `pnpm -r --no-bail --parallel run test` (identical selection) | same plant | yes | `× A1_SENTINEL_PLANT_VITEST_R2_9e51c3`, types 1 failed / 614 passed | types 614/614, token absent | DISCRIMINATING (credit belongs to the variant, not the canonical selector) |
| conformance | `pnpm run gen:vue-goldens:check` | 1 mutated byte in a committed official golden (vdom-inline tree) | yes | exit 1 naming the exact drifted artifact | exit 0, 286 artifacts in sync | DISCRIMINATING |
| corpus gate | `pnpm --filter @verter/dx-harness test:corpus-gate` (armed) | spawner binary-resolution break | yes | exit 1, fatal route error naming the token'd path, sent=0 | exit 1 on the candidate's REAL pre-existing acceptance-bar class, token absent, 49 requests executed | DISCRIMINATING (per-failure-identity on a red baseline, as pre-registered) |
| release check | `cargo check --workspace --release` | E0308 const in verter_mcp_server | yes | exit 101 naming crate + planted line | exit 0 | DISCRIMINATING |
| (supplementary) rustfmt | `cargo fmt --all --check` | misformatted fn in verter_span | yes | exit 1 naming the file | exit 0 | DISCRIMINATING |
