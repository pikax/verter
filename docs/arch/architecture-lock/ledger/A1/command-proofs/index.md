# A1 Command Proofs — Index (`contracts/baseline-lock.md` §4 + `verification.md` §2) — round 2

All commands were run against the FINAL A1 candidate
`13cedd6fc1315bfb6fec0c4cacb0eacdb02c6c83` / tree
`a992bb87382e58d6ec846c7be37cbb941ee0b1b2` (single commit on branch
`block/a1-command-truth`, parented on block base
`b7ea2dc88bda86473de81de3438b7f88ef30adc7`), in the dedicated A1 worktree
(`<WORKTREE>`; the primary checkout was never touched). The candidate was
committed BEFORE any run below; the worktree was CLEAN (0 modified/untracked)
for every run; NO tracked change followed the runs. Shared environment for every
row: Windows 11 Pro 10.0.26200 (26200.8875), AMD Ryzen 9 7950X (32 logical),
127 GB RAM, Git Bash (POSIX sh), no extra environment variables beyond those
stated per-row, default cargo features unless stated. Toolchain (pinned by the
repo's `rust-toolchain.toml`): rustc 1.97.1, cargo 1.97.1, clippy 0.1.97,
rustfmt 1.9.0-stable, cargo-nextest 0.9.137, node v26.5.0, pnpm 10.22.0. Full
provenance: `../environment.md`. Raw output digests are SHA-256 over the raw
captured bytes (stdout+stderr interleaved, with a CMD/CWD/ENV/DATE header and
EXIT/END-DATE trailer written by the capture wrapper).

**A1 framing:** these rows prove NON-VACUITY (intended target + non-zero
executed work), not greenness. `main` is persistently CI-red at the entry
lineage; failing rows are recorded as-is and nothing was changed to make
anything pass.

**Count convention (uniform):** `discovered / executed / passed / failed /
skipped` — discovered = every case the selector enumerated; executed =
discovered minus skipped/ignored; skipped includes libtest `ignored`. Rows
whose tool emits no counts say so explicitly and name their non-vacuity proof.

## Rust

| # | exact command | cwd | env/features | exit | discovered / executed | passed / failed / skipped | binaries/packages/fixtures | non-vacuity | raw file |
|---|---|---|---|---|---|---|---|---|---|
| 01 | `node scripts/gate.mjs --timeout 420m` | `<WORKTREE>` | default; no `VERTER_REQUIRE_TSGO`/`NEXTEST_PROFILE`; `--timeout` is a documented gate flag (wallclock budget only — selection unchanged); warm `target/gate-runner` (tree differs from the prior round by one docs file) | 0 (VERDICT: PASS — all three surfaces green) | S1: 24626 / 24044 across 78 binaries; S2: 3 verter_session libtest suites in-process; S3: 9040 / 8477 across 6 binaries (72 binaries outside the surface-3 filterset) | S1: 24044 / 0 / 582 skipped; S2: 3 suites clean, 0 non-tolerated failures; S3: 8477 / 0 / 563 skipped | two `cargo nextest archive --workspace` universes (dev + no-debug-assertions); build-prerequisite preflight SATISFIED (typescript-plugin dist loaded from the vue-vscode probe location); freshness-tooling preflight already-present ⇒ `typeinfo_proto_ts_freshness` byte-pin tolerance DISABLED (it ran genuinely and passed inside S1/S2 — and row 01b converts that to a direct targeted receipt); gate-internal Vue macro oracle checks passed | PROVEN — gate self-attested per-surface counts; sentinel A proves the same selector fails on a planted break. NOTE: the round-1 red (`verter_language::main cases::compile_fail::registered_authority_capabilities_are_not_mintable_outside_their_authorities`) did NOT occur in this run — see `../sentinel-verification.md` "Nondeterministic findings" | `01-gate-mjs.txt` |
| 01b | `cargo nextest run -p verter_protocol -E 'test(typeinfo_proto_ts_freshness)'` | `<WORKTREE>` | targeted receipt — converts "ran inside the unfiltered gate" inference into direct proof; buf 1.70.0 + oxfmt 0.52.0 present via `node_modules/.bin` | 0 | 79 / 5 (74 non-matching tests skipped by the filter) | 5 / 0 / 74 skipped | verter_protocol test binaries; the `typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output` byte-pin among them | PROVEN — nextest per-run counts; executed > 0 with the exact test name in the run log | `01b-typeinfo-freshness-targeted.txt` |
| 01c | `cargo nextest run -p verter_css_syntax` | `<WORKTREE>` | targeted receipt — converts the CSS row's gate-coverage inference into direct proof | 0 | 94 / 94 across 2 binaries | 94 / 0 / 0 | verter_css_syntax (the custom CSS parser crate) full test suite | PROVEN — nextest per-run counts | `01c-css-syntax-targeted.txt` |
| 01d | `cargo nextest list --archive-file <dev archive> --run-ignored ignored-only` | `<WORKTREE>` | listed from the gate's own dev-profile archive (the surfaces-1/2 universe) — zero rebuild, same feature unification | 0 | 582 ignored-test identities enumerated | n/a (inventory, not a run) | the exact `nextest.tar.zst` the gate ran | PROVEN inventory — the enumeration count EQUALS surface 1's 582 skipped, so the gate's skip figure is fully accounted for by named `#[ignore]` identities (verification.md §2 "a test surface that silently does not run" check) | `01d-ignored-tests-dev-archive.txt` |
| 01e | `cargo nextest list --archive-file <no-debug-assertions archive> --run-ignored ignored-only -E 'package(verter_session) + package(verter_scheduler)'` | `<WORKTREE>` | listed from the gate's own shipped-cfg archive, surface-3 filterset | 0 | 563 ignored-test identities enumerated | n/a (inventory) | the exact `nextest-no-debug-assertions.tar.zst` the gate ran | PROVEN inventory — EQUALS surface 3's 563 skipped | `01e-ignored-tests-shipped-archive.txt` |
| 02 | `cargo clippy --workspace --all-targets -- -D warnings` | `<WORKTREE>` | default; toolchain-pinned clippy 0.1.97 | 101 (FAIL — pre-existing) | full workspace lint compile to the failing crate | 7 denied lints, all in `verter_session` lib (same seven as the prior round) | full workspace incl. test/bench targets | PROVEN — span-anchored diagnostics (`error: could not compile verter_session (lib) due to 7 previous errors`); pre-existing red recorded, not fixed | `02-cargo-clippy.txt` |
| 03 | `cargo check --workspace --release` | `<WORKTREE>` | default; real release profile (opt-level 3 + fat LTO); fully warm (`Finished release profile in 1.03s`) | 0 | full workspace release-profile check (cargo emits no test counts for `check`) | no errors | full workspace | PROVEN by isolated negative control — sentinel E (`../sentinel-verification.md`): a planted E0308 in `verter_mcp_server` flips this exact command to exit 101 naming the crate and the planted line; restore returns exit 0 | `03-cargo-check-release.txt` |
| 04 | `cargo clippy --target wasm32-unknown-unknown -p verter_wasm -- -D warnings` | `<WORKTREE>` | wasm32-unknown-unknown target | 101 (FAIL — pre-existing) | wasm32 lint compile of verter_wasm + dependency graph to the failing crate | 5 denied lints in `verter_session` lib under the wasm target (target-gated subset of row 02's 7) | verter_wasm + deps for wasm32-unknown-unknown | PROVEN — span-anchored diagnostics from the wasm32 build graph; pre-existing red recorded, not fixed | `04-cargo-clippy-wasm.txt` |
| 05 | `cargo fmt --all --check` | `<WORKTREE>` | default | 0 | all workspace `.rs` sources (rustfmt reports no counts) | no diffs | rustfmt 1.9.0-stable over every workspace crate | PROVEN by isolated negative control (`../sentinel-runs/fmt-negative-*.txt`): a planted misformat in `verter_span` flips the same command to exit 1 naming the file; restore returns exit 0 | `05-cargo-fmt-check.txt` |
| 06 | `cargo test --workspace --doc` | `<WORKTREE>` | default | 0 | 32 / 4 doctests across 35 per-crate suites | 4 / 0 / 28 ignored | workspace rustdoc examples (most crates carry zero doctests — an honest small universe, not a filtering artifact) | PROVEN — libtest's own per-suite `test result:` lines; executed 4 > 0 | `06-cargo-test-doc.txt` |
| 21 | `cargo test --locked -p verter_language --test main -- compile_fail` ×4 (consecutive) | `<WORKTREE>` | warm target; the exact command whose single failure the prior round recorded; runs 1–4 | 0,0,0,0 | 6 / 2 per run (4 non-matching tests filtered out) | 2 / 0 / 0 each run (both trybuild compile_fail tests; 10.87s, 1.44s, 1.24s, 1.31s) | verter_language trybuild suite | PROVEN — 4/4 green; own data points for the nondeterminism restatement in `../sentinel-verification.md` | `21-trybuild-rerun-{1..4}.txt` |

## JS / TS

| # | exact command | cwd | env/features | exit | discovered / executed | passed / failed / skipped | binaries/packages/fixtures | non-vacuity | raw file |
|---|---|---|---|---|---|---|---|---|---|
| 07 | `pnpm install --frozen-lockfile` | `<WORKTREE>` | pnpm 10.22.0 | 0 | full workspace dependency materialization, lockfile in sync | n/a | `pnpm-lock.yaml` sha256 `3f789a2a…`; `node_modules/.bin/{buf,oxfmt}` resolve and run (buf 1.70.0, oxfmt 0.52.0) | PROVEN — frozen-lockfile mode fails on any lockfile drift; the gate's freshness preflight independently attested the installed tooling | `07-pnpm-install-frozen-lockfile.txt` |
| 08 | `pnpm test` (root script = `pnpm -r --parallel run test`) | `<WORKTREE>` | after build:ts + build:native + build:wasm | 1 (FAIL — pre-existing) | 24 packages carry a test script; the selector is PARALLEL + BAIL-ON-FIRST-FAILURE: when `@verter/typeinfo` failed (3 tests), pnpm terminated the remaining suites — only 5 packages ran to a summary this round (type-ir 6p, proto 16p, svelte-runtime-tests 35p, binary-launcher 47p, typeinfo 3f/28p); the rest were started-then-killed | see row 08c for the complete per-package truth | 24 workspace package test scripts | **A1 FINDING (recorded, not fixed): the canonical root JS selector is structurally bail-fast — on any red package it kills most of the JS test surface mid-run.** Executed work was non-zero and the failure is genuine, but this row alone cannot attest the full JS surface — row 08c does. The sentinel for THIS selector is honestly UNMET — see `../sentinel-verification.md` sentinel B | `08-pnpm-test.txt` |
| 08c | `pnpm -r --no-bail --parallel run test` (SUPPLEMENT: identical selection to row 08, `--no-bail` so every suite completes) | `<WORKTREE>` | after build:ts + build:native + build:wasm | 1 (FAIL — pre-existing) | 4448 / 4416 vitest tests across the 24 package test scripts (echo-only/no-op scripts in example, nuxt, svelte-jsx, verter-tsc; vue-conformance-oracle runs its own checks) | 4411 / 5 / 32 skipped — failures: `@verter/typeinfo` 3 (extra-imports-structured, vue-instance-props, resolve-symbol; red in both rounds), `@verter/unplugin` 1 (red in both rounds), `@verter/playground` 1 (`wasmInContextLs.spec.ts` carrier-membership case, 5.6s — green in the prior round's no-bail run and green in row 11b's dedicated config this round; consistent with load-widened nondeterminism under the 24-package parallel run; recorded, not fixed) | all 24 package test scripts incl. `@verter/native` (119 pass / 5 skip against the freshly built `.node`) and `@verter/wasm` (16 pass) | PROVEN — per-package vitest counts for every package; failures recorded, not fixed | `08c-pnpm-test-no-bail.txt` |
| 09 | `pnpm run test:scripts` | `<WORKTREE>` | none | 0 | 52 / 52 (vitest 14 + node --test 5 + 7 + 26) | 52 / 0 / 0 | `scripts/sccache-env.test.mjs`, `scripts/lib/publish-set.spec.mjs`, `scripts/editor-contracts/plenary-outcome-guard.test.mjs`, `scripts/validate-program-state.test.mjs` | PROVEN — native per-suite counts in output | `09-pnpm-test-scripts.txt` |

## Native / WASM

| # | exact command | cwd | env/features | exit | discovered / executed | passed / failed / skipped | binaries/packages/fixtures | non-vacuity | raw file |
|---|---|---|---|---|---|---|---|---|---|
| 10 | `pnpm run build:native` (= `@verter/native`: `napi build -o dist --platform --release …` + type emit + Windows artefact copy) | `<WORKTREE>` | napi-rs CLI 3.x; release profile (fat LTO); warm target (docs-only tree delta ⇒ near-no-op recompile) | 0 | full release compile/link of verter_napi; artifact `packages/native/dist/verter-native.win32-x64-msvc.node` = 33,612,800 bytes | build success | crates/verter_napi cdylib; TS type emit | PROVEN — non-trivial platform artifact on disk AND subsequently LOADED: row 08c's `@verter/native` suite ran 119 tests against this exact `.node` | `10-build-native.txt` |
| 11 | `pnpm run build:wasm` (= cargo release wasm32 + wasm-bindgen 0.2.122 + wasm-opt -Os + tsdown + playground copy) | `<WORKTREE>` | wasm32-unknown-unknown; `getrandom_backend="wasm_js"` | 0 | full release wasm pipeline; artifact `packages/wasm/wasm/verter_wasm_bg.wasm` = 15,699,519 bytes (playground copy byte-identical) | build success | verter_wasm crate; wasm-bindgen 0.2.122; binaryen ^130 | PROVEN — non-trivial artifact produced AND actually INSTANTIATED by row 11b (the `@verter/wasm` unit suite in 08c mocks the wasm module and does NOT load the binary — recorded so nobody mistakes it for wasm-runtime proof) | `11-build-wasm.txt` |
| 11b | `pnpm --filter @verter/playground test:wasm` (ci.yml "Playground WASM compiler spec" — the selector that actually instantiates the built wasm) | `<WORKTREE>` | after row 11 | 0 | 43 / 41 | 41 / 0 / 2 skipped | the freshly built `verter_wasm_bg.wasm` loaded through the playground compiler wrapper | PROVEN — real wasm instantiation + compile assertions with native counts | `11b-playground-wasm-spec.txt` |

## Corpus / provider / conformance

Selector provenance: derived from the repository itself — root `package.json`
scripts, `.github/workflows/ci.yml`, and `.github/workflows/corpus-gate.yml`.
No invented command names.

| # | exact command | cwd | env/features | exit | discovered / executed | passed / failed / skipped | oracle/provider provenance | non-vacuity | raw file |
|---|---|---|---|---|---|---|---|---|---|
| 12 | `pnpm run gen:vue-goldens:check` | `<WORKTREE>` | none | 0 | 286 committed oracle artifacts regenerated in-memory and compared (three backend trees: vdom, vapor, vdom-inline) | 286 match / 0 drift | official pinned `vue@3.6.0-rc.1` compiler family over `crates/verter_vue_conformance/corpus` | PROVEN — artifact count in output; sentinel C proves one mutated golden byte (this round planted in the vdom-inline tree) flips this exact selector to exit 1 naming the artifact | `12-vue-goldens-check.txt` |
| 13 | `node scripts/gen-svelte-goldens.mjs --check` | `<WORKTREE>` | none | 0 | 1066 goldens | 1066 in sync | official pinned `svelte@5.56.3` over the vendored `.svelte` corpus | PROVEN — golden count in output | `13-svelte-goldens-check.txt` |
| 14 | `node scripts/gen-svelte-goldens.mjs --conformance --check` | `<WORKTREE>` | none | 0 | 1218 goldens | 1218 in sync | `svelte@5.56.3` + emit-plan manifest `fnv1a64-8faa9143878b8848` | PROVEN — golden count + manifest fingerprint in output | `14-svelte-conformance-goldens-check.txt` |
| 15 | `pnpm run gen:vue-macro-oracle:check` + `pnpm run test:vue-macro-oracle` | `<WORKTREE>` | none | 0 + 0 | 11 oracle cases + 4 node --test tests | 11 in sync; 4 / 0 / 0 | official pinned `@vue/compiler-sfc@3.5.34` runtime-macro differential | PROVEN — case/test counts in output (also re-executed inside gate row 01) | `15-vue-macro-oracle.txt` |
| 16 | `pnpm --filter @verter/dx-harness test:corpus-gate` | `<WORKTREE>` | ARMED this round: `VERTER_CORPUS_GATE_DIR=<REDACTED — classified external corpus root>`; `VERTER_CORPUS_GATE_LABEL="Corpus A"`; `VERTER_CORPUS_GATE_RECEIPT=16r-corpus-gate-receipt.json`; `VERTER_CORPUS_GATE_FILE_DETAIL` unset (sampled paths NOT embedded); all other knobs default (routes tsserver,tsgo,shared-tsgo; sample 40; serial topology; executor unattested). Corpus identity is bound by content fingerprint, not name: SHA-256 over the sorted `sha256(content)  relpath` manifest of the corpus's 290 non-node_modules `.vue` files = `7e9a65dd26b4cd1f17158aa26dc658e8a10768668a44a7aae74067e171f6dec5` | 1 (FAIL — pre-existing product issues) | 290 `.vue` files discovered, 40 sampled; all three routes started and ran bounded sessions; 333 requests sent (tsserver 115, tsgo 109, shared-tsgo 109), 326 answered, 7 timed out; wall clock 454s within the 1200s budget | receipt `pass:false` — every route ended WEDGED with `completed=false` (liveness went dark after a definition timeout), hover/definition p95 bars breached, unexpected-empty results on hover/definition/completion/references, plus unsampled-process and provider-attribution findings on tsgo/shared-tsgo (full list in the receipt) — ALL recorded as-is, nothing fixed | real verter-lsp debug binary over all three provider routes against the classified corpus; machine receipt `16r-corpus-gate-receipt.json` (corpusLabel "Corpus A", vueFileCount 290, sampledCount 40, structural config echo only — no paths) | PROVEN — the selector executed its intended target with substantial non-zero work and failed on REAL acceptance bars (the known wedge-class product issue), not on configuration; sentinel D proves the same selector fails loudly on a planted spawner break and recovers on restore; the corpus name appears NOWHERE in this bundle (zero-hit grep recorded in the exact-candidate record) | `16-corpus-gate-armed.txt` + `16r-corpus-gate-receipt.json` |
| 17 | `node scripts/gen-svelte-name-parity-corpus.mjs --check` | `<WORKTREE>` | none | 0 | 80 rows | 80 in sync | `svelte@5.56.3` name-parity corpus | PROVEN — row count in output | `17-svelte-name-parity-check.txt` |
| 18 | provider matrix `pnpm run test:lsp:neutral` (root composite → dx-harness editor-neutral vitest lane, the ci.yml "Editor-neutral LSP contract" job) | `<WORKTREE>` | debug `verter-lsp.exe`/`verter-relay-shim.exe` (row 18b); tsgo = TS 7.0.2 native `tsc.exe`; tsserver SDK = typescript-plugin's nested TS 6.0.3; receipt redirected into this bundle via `VERTER_EDITOR_NEUTRAL_RECEIPT` | 1 (FAIL — pre-existing product issues) | inventory 92 cases; expectedExecutions 274; attempted 274 (assertion-enforced zero-skip; per-route applicability tsserver 91 / tsgo 91 / shared-tsgo 92); vitest 277 / 277 (274 executions + authority controls) | 241 / 36 / 0 (vitest); per route: tsserver 83/8, tsgo 78/13, shared-tsgo 77/15; setupFailures []. tsserver and tsgo match the prior round exactly; shared-tsgo shows 2 more failures than the prior round on the same tree — run-to-run variance in the flaky relay class, recorded not fixed | REAL providers: tsserver + managed tsgo + relay shared-tsgo; TS CLI authority control ran; machine receipt `18r-editor-neutral-receipt.json` carries `sourceSha` = `13cedd6fc1315bfb6fec0c4cacb0eacdb02c6c83` — the FINAL candidate SHA (bound, not aspirational) | PROVEN — harness asserts attempted == expectedExecutions == 274, every route started, receipts unique per route:case; failures are pre-existing hover/rename/diagnostics product issues recorded, not fixed | `18-provider-matrix-lsp-neutral.txt` + `18r-editor-neutral-receipt.json` |
| 19 | `cargo run -p verter_svelte_conformance -- check` | `<WORKTREE>` | default | 0 | conformance corpus reconciled against the manifest; the binary emits a VERDICT, not a count ("the conformance corpus matches the manifest") | verdict green | corpus + manifest (fingerprint `fnv1a64-8faa9143878b8848`, corroborated by row 14's independent 1218-golden check of the same manifest identity); fixture inventory counted independently: 609 committed `.svelte` fixtures | asserted via the binary's own reconciliation verdict + row-14 corroboration — explicitly NOT count-attested by the selector itself | `19-svelte-conformance-check.txt` |
| 20 | `cargo test -p verter_compiler --features svelte-oracle` | `<WORKTREE>` | `--features svelte-oracle` (live feature-gated oracle harness; needs node_modules svelte@5.56.3 — present) | 0 | 6453 / 6441 across 4 suites (5939-test lib suite + 507-test live oracle harness + two aux suites) | 6441 / 0 / 12 ignored | pinned `svelte@5.56.3` live oracle | PROVEN — libtest per-suite counts; the 507-test suite is the feature-gated live oracle harness a feature-off run does not execute | `20-svelte-oracle-live.txt` |

## Prerequisite-build evidence (not canonical commands; recorded because CI runs them before the canonical selectors)

| # | exact command | mirrors | exit | raw file |
|---|---|---|---|---|
| 00b | `pnpm --filter @verter/language-shared --filter @verter/typescript-plugin build` | ci.yml "Build the Rust gate's TypeScript prerequisites" (gate fails closed with exit 127 without it) | 0 | `00b-gate-ts-prereq-build.txt` |
| 08b | `pnpm run build:ts` | ci.yml "Build TS packages" (JS Build & Test job) | 0 | `08b-build-ts-prereq.txt` |
| 18b | `cargo build -p verter_lsp -p verter_relay_shim` | the debug binaries rows 18 and 16 spawn | 0 | `18b-lsp-debug-builds.txt` |

## Final raw-output digest table (SHA-256 over raw captured bytes)

Computed after every run completed and every evidence file was final. The
sentinel clone and the candidate worktree were both verified clean
(`git status --porcelain` = 0) at `13cedd6fc…` before this table was written;
the block's single documentation-only commit predates ALL runs.

| file | SHA-256 |
|---|---|
| `00b-gate-ts-prereq-build.txt` | `022e9a56ba5b02935737a8411038ef245a8443afc3d4c9e0c744728335e11f4e` |
| `01-gate-mjs.txt` | `8c59894b5f463efeeb7ee57f1e1231a3730dfa5bde41682d592eba1d6e3c8ab5` |
| `01b-typeinfo-freshness-targeted.txt` | `b40b3383fb8324c265352c2c729fe0465adfbc919a340b74d3f72c5c1f8e3f6d` |
| `01c-css-syntax-targeted.txt` | `186cf0ed68f1a46949a7c236eb8508b1f22db93d13a3864351e2c4d741c618b6` |
| `01d-ignored-tests-dev-archive.txt` | `821d6a2aa4c7a0213e60319a788da50d1a03ef6c5a7fd2100041568895801dd0` |
| `01e-ignored-tests-shipped-archive.txt` | `ebcec5bdce9c8b9db40352cca27567f70959662a8a1298a801fbf23a3ac96dfa` |
| `02-cargo-clippy.txt` | `26bbe713d1c57f9827629cddc44adb66314f125039135a9907a3f514e681ad3e` |
| `03-cargo-check-release.txt` | `d7b9a756b2c351f374186ad00c77b9acd9e74487c4f67d3106bdf52408999ca3` |
| `04-cargo-clippy-wasm.txt` | `29aec1af0dea671ba890eee6d4ac52f936cd14959d4241fdb38e0fece0d34bd5` |
| `05-cargo-fmt-check.txt` | `81b0d7ae4e3a46b61fdb02173d070ca53e487dc4b777194f8d57fc7097e55b5e` |
| `06-cargo-test-doc.txt` | `ebf6e8e2224b48f5917e029c8ee70716194908600c01c6ed340b88729aeb6420` |
| `07-pnpm-install-frozen-lockfile.txt` | `917e9ff280c12d3d48ab539621a7629ba821ab937ff977532c86160f01deceeb` |
| `08-pnpm-test.txt` | `6e7528c679eff96b28014a0a560944a6ee240e77ad51fe76b16c32f25b41300c` |
| `08b-build-ts-prereq.txt` | `0787977aadef6b156f1c01b75d8f9f216494d15bacc8667bd9c27c3172c41e2a` |
| `08c-pnpm-test-no-bail.txt` | `7b8bec5cfe6a1f1ce176d492117ee287f00ed4658b0b5f1669e8e42b86b286ca` |
| `09-pnpm-test-scripts.txt` | `2a7d7858620fe54706a04499aed569f544e08c788a8953ca1954f471654b724f` |
| `10-build-native.txt` | `c0797c4a1f3fbe753e3e1e44a471dcb27acc7ba4957ef8154a883280d96ac450` |
| `11-build-wasm.txt` | `7856b054a561578cde0a76a8c4c7bd33cee260a8eacf163134af436dbd124a00` |
| `11b-playground-wasm-spec.txt` | `08383643e529bd43a9a40b3624a18c80069acd0efc9834fca6a3c91f45ed4ff5` |
| `12-vue-goldens-check.txt` | `c8328e182b68ee5b41c8f9088b076d323a6ea299b94109185bf3be4f5974f6ed` |
| `13-svelte-goldens-check.txt` | `47cabfebe19ae74569f64f174b3e260c460a7145897907e4cb5d76873a2816c6` |
| `14-svelte-conformance-goldens-check.txt` | `fed608bfc6c2c6034b66b575ec7a711cc767ae7963228630067e8d6b9dcdf491` |
| `15-vue-macro-oracle.txt` | `43c42ab1782a1d8f44cdb7f46490a00eb5c80886639ffa827bc90fb24b3dcf1e` |
| `16-corpus-gate-armed.txt` | `62a1fe07fe4f623ec86f8cfe29e68bc39bc6c7ceec46c24b1de01b562a4ac030` |
| `16r-corpus-gate-receipt.json` | `1a7d5d8ed443076ec380195686198b26ee6fe17d68a07d10f112661a6f63de47` |
| `17-svelte-name-parity-check.txt` | `fb1bd04514e2ff702333b7b0eb553625fe2d482f2a3ebbe9e5bd23d5f285792c` |
| `18-provider-matrix-lsp-neutral.txt` | `8e26ff51b7ca17b9d726dd4db194afaf86c4c42947023160827d66e3452115f6` |
| `18b-lsp-debug-builds.txt` | `73d582d5542bbe52ace364d541a250ef63d8eda5f7787677f6ae340b21e10529` |
| `18r-editor-neutral-receipt.json` | `1838fcc9f34e776f219f471d23f48be79437d2d9056508206cae18c7670f959b` |
| `19-svelte-conformance-check.txt` | `0f1a560b8148dbed5e58c33a775bc064838bf8ddb56fd271c37933cb024e0dfd` |
| `20-svelte-oracle-live.txt` | `ea9c6883f1937371c30e3b0521a08fbf0ac1209d1419c4a79da42b0e34e733eb` |
| `21-trybuild-rerun-1.txt` | `f6b7a22dd1b924e0386c840bd0cbeea1238e1bba4d0e7e6a111d362d6fbf16b5` |
| `21-trybuild-rerun-2.txt` | `0208e511d2d30652981a2826325ac8384afb77f65d194dfc73c3f5448208904b` |
| `21-trybuild-rerun-3.txt` | `f5d81e9dcb93b61067c67dafaf0f633ccca4432ac39572c5a9de78ac51e73f6b` |
| `21-trybuild-rerun-4.txt` | `fae08889a077e99812bd5984b4d8130d53414a2e5be53a8b84a53fe03847c6a9` |
| `../sentinel-runs/fmt-negative-control.txt` | `764d6335e31dd68693a830cc169c08ec6cc89d5cfd7f874707a2d2276dfee2e7` |
| `../sentinel-runs/fmt-negative-planted-raw.txt` | `4e162e17f0101ce3a0c63c7baa2dba1b31db428b8c39022df35118a946e37e86` |
| `../sentinel-runs/fmt-negative-restored-raw.txt` | `28e30a171b932e529ffa31e7fe877a920f31a06fa8dad9010e761338a0b5ec24` |
| `../sentinel-runs/sentinel-A-gate.txt` | `51e78f5cd1624d3754611ed3a1b501783130581636c355e4d91f50e8223acf3e` |
| `../sentinel-runs/sentinel-A-gate-planted-raw.txt` | `9487b74e190ffd2f0964fd549a473bcf4293e432e1d1c373a20691055e76cbf7` |
| `../sentinel-runs/sentinel-A-gate-restored-raw.txt` | `befc88ac39ea9c3cd3500270f77462e59d2c416db63d93b1b409a72f16944921` |
| `../sentinel-runs/sentinel-B-vitest.txt` | `c13d1c5399305095fd5ba95ce0d6377bc30375b2edc237385b88dcb7a27a3bf0` |
| `../sentinel-runs/sentinel-B-canonical-planted-raw.txt` | `d672ba88c84d3da0f47a9fe6c97a5a9f040a0f361d514029fe5160fbb8ff15e3` |
| `../sentinel-runs/sentinel-B-nobail-planted-raw.txt` | `6d0c0f965c47be59f4a6a88e745967f94efa3c9996778779e6a0434900f8b2e8` |
| `../sentinel-runs/sentinel-B-nobail-restored-raw.txt` | `45cf01e106ac7480596c8b0267ff2e6245c85601a2bf80edec6a12b4cc59524d` |
| `../sentinel-runs/sentinel-C-vue-goldens.txt` | `95a40581d8d2918a886db66d3cc897764e8f927e5335887f170f05aebd8dde23` |
| `../sentinel-runs/sentinel-C-planted-raw.txt` | `574aa5ca4652853c95452b63df05bb6c5522c1ce88d88610b07830e3ed372b0c` |
| `../sentinel-runs/sentinel-C-restored-raw.txt` | `045915be177268ae8f1ffd74cde31a3c54a71092ad22312a68dabb50db25cf81` |
| `../sentinel-runs/sentinel-D-corpus-gate.txt` | `2a05847becfd9b55c5c5790b0bf518719ad707dd759f2064c12f8b58e2399954` |
| `../sentinel-runs/sentinel-D-lsp-build.txt` | `9eb6fdd624ac9ab11e932e2119f05741a417dde1fc1505568fddb84a6297ee5f` |
| `../sentinel-runs/sentinel-D-planted-raw.txt` | `963b28a8d2774d309fe36f369939e3e0699379bc71b00af192ce0a5e69e7de20` |
| `../sentinel-runs/sentinel-D-restored-raw.txt` | `7680dec1f0304a25a08edc88236f05c94cd2845adbbe4dab79f4c887c7cf5edb` |
| `../sentinel-runs/sentinel-E-check-release.txt` | `45f5612ee52c65725715528d80263ae4304b903c7615f6a4cab630ca5e91d5cb` |
| `../sentinel-runs/sentinel-E-planted-raw.txt` | `cdbedf4219f075ea246c0738b286cba1887bf9a60fa5e63ce0f7daadcc7e78e9` |
| `../sentinel-runs/sentinel-E-restored-raw.txt` | `a1f08910eaa47b895c240da7d5758d4d03a65fdcc82024ecd6ee11bf28dc4ee2` |
| `../environment.md` | `be905789c3dc3163094558518d65fe52b8929c37c61244d7d12b501b0cc1f9e7` |
| `../context-packet.md` | `244d9d14b6123b2ef7c084cd7ecda33ef8182ef57e1c04ed005b14f9a1c21f67` |
| `../sentinel-verification.md` | `d1d3bf75dec121a1a134aa51d4d3d512d334c991a839bb5bba25e0b45daa999a` |
