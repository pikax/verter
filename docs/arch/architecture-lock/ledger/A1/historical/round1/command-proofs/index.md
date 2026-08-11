# A1 Command Proofs — Index (`contracts/baseline-lock.md` §4 + `verification.md` §2)

All commands were run against the A1 block-base tree
`b7ea2dc88bda86473de81de3438b7f88ef30adc7` / tree
`47645406a9246e600af995c62608b709347e13a4` on branch `block/a1-command-truth` in
the dedicated A1 worktree (referred to below as `<WORKTREE>`; the primary checkout
was never touched, and the worktree was CLEAN — 0 modified/untracked — for every
run below; the block's only tracked change, the capability-matrix completion, was
committed AFTER all candidate command runs and changes no source, build input, or
test). Shared environment for every row: Windows 11 Pro 10.0.26200 (26200.8875),
AMD Ryzen 9 7950X (32 logical), 127 GB RAM, Git Bash (POSIX sh), no extra
environment variables beyond those stated per-row, default cargo features unless
stated. Toolchain (pinned by the repo's `rust-toolchain.toml`): rustc 1.97.1,
cargo 1.97.1, clippy 0.1.97, rustfmt 1.9.0-stable, cargo-nextest 0.9.137, node
v26.5.0, pnpm 10.22.0. Full provenance: `../environment.md`. Raw output digests
are SHA-256 over the raw captured bytes (stdout+stderr interleaved, with a CMD/
CWD/ENV/DATE header and EXIT trailer written by the capture wrapper).

**A1 framing:** these rows prove NON-VACUITY (intended target + non-zero executed
work), not greenness. `main` is persistently CI-red at the entry lineage; failing
rows are recorded as-is and nothing was changed to make anything pass.

## Rust

| # | exact command | cwd | env/features | exit | discovered / executed | pass / fail / skip | binaries/packages/fixtures | non-vacuity | raw file | raw SHA-256 |
|---|---|---|---|---|---|---|---|---|---|---|
| 01 | `node scripts/gate.mjs --timeout 420m` | `<WORKTREE>` | default; no `VERTER_REQUIRE_TSGO`/`NEXTEST_PROFILE`; `--timeout` is a documented gate flag (wallclock budget only — selection unchanged); cold `target/gate-runner` | 1 (VERDICT: FAIL — 1 non-tolerated failure) | S1: 24044 discovered+run across 78 binaries (582 skipped); S2: 3 verter_session libtest suites in-process; S3: 8477 run across 6 binaries (563 tests + 72 binaries filtered out) | S1: 24043 pass / 1 fail / 582 skip; S2: 3 suites clean; S3: 8477 pass / 0 fail / 563 skip | two `cargo nextest archive --workspace` universes (dev 206s + no-debug-assertions 197s); build-prereq preflight SATISFIED (typescript-plugin dist loaded); freshness-tooling preflight already-present ⇒ byte-pin tolerance DISABLED; gate-internal Vue macro oracle checks passed | PROVEN — self-attested per-surface counts; both preflights attested; the single failure (`verter_language::main cases::compile_fail::registered_authority_capabilities_are_not_mintable_outside_their_authorities`, trybuild 6/6 sub-cases) is a pre-existing red recorded, not fixed; sentinel A (below) proves the selector fails on a planted break | `01-gate-mjs.txt` | `(final digest in table at end)` |
| 02 | `cargo clippy --workspace --all-targets -- -D warnings` | `<WORKTREE>` | default | 101 (FAIL — pre-existing) | full workspace lint compile executed to the failing crate | 7 denied lints, all in `verter_session` lib: empty-line-after-doc-comment, large-enum-variant, `matches!`-like match, redundant closure, match→`?`, contains_key+insert, very-complex-type | full workspace incl. test/bench targets; toolchain-pinned clippy 0.1.97 | PROVEN — the run compiled real crates and produced 7 concrete span-anchored diagnostics (`error: could not compile verter_session (lib) due to 7 previous errors`); pre-existing red recorded, not fixed | `02-cargo-clippy.txt` | (final table) |
| 03 | `cargo check --workspace --release` | `<WORKTREE>` | default; real release profile | 0 | full workspace release-profile check (`Finished release profile in 58.65s`) | no errors | full workspace | asserted via cargo's own per-crate `Checking` lines over the workspace member set (cargo reports no test counts for `check`); no independent negative control was run for this row | `03-cargo-check-release.txt` | (final table) |
| 04 | `cargo clippy --target wasm32-unknown-unknown -p verter_wasm -- -D warnings` | `<WORKTREE>` | wasm32-unknown-unknown target | 101 (FAIL — pre-existing) | wasm32 lint compile of verter_wasm + its dependency graph executed to the failing crate | 5 denied lints in `verter_session` lib under the wasm target (subset of row 02's 7 — target-gated code differs) | verter_wasm + deps for wasm32-unknown-unknown | PROVEN — concrete span-anchored diagnostics from the wasm32 build graph; pre-existing red recorded, not fixed | `04-cargo-clippy-wasm.txt` | (final table) |
| 05 | `cargo fmt --all --check` | `<WORKTREE>` | default | 0 | all workspace .rs sources (rustfmt reports no counts) | no diffs | rustfmt 1.9.0-stable over every workspace crate | PROVEN by isolated negative control (`../sentinel-runs/fmt-negative-control.txt`): a planted misformat in `verter_span` flips the same command to exit 1 naming the file; restore returns exit 0 | `05-cargo-fmt-check.txt` | `1d77feacf7cd43e4cab3e116fdd07823c01878a94f3b4b1ee05a2cf26a138013` |
| 06 | `cargo test --workspace --doc` | `<WORKTREE>` | default | 0 | 35 per-crate doc-test suites discovered; 32 doctests total (4 executed + 28 ignored) | 4 pass / 0 fail / 28 ignored | workspace rustdoc examples (most crates carry zero doctests — an honest small universe, not a filtering artifact) | PROVEN — libtest's own per-suite `test result:` lines; executed count 4 > 0 | `06-cargo-test-doc.txt` | (final table) |

## JS / TS

| # | exact command | cwd | env/features | exit | discovered / executed | pass / fail / skip | binaries/packages/fixtures | non-vacuity | raw file | raw SHA-256 |
|---|---|---|---|---|---|---|---|---|---|---|
| 07 | `pnpm install --frozen-lockfile` | `<WORKTREE>` | pnpm 10.22.0 | 0 | full workspace dependency materialization, lockfile in sync (29.1s) | n/a | pnpm-lock.yaml sha256 `3f789a2a…`; note: pnpm 10.22 ignores `package.json#pnpm.onlyBuiltDependencies` and skipped build scripts for `@bufbuild/buf`, `@parcel/watcher`, `core-js` — `node_modules/.bin/{buf,oxfmt}` still resolve and run (buf 1.70.0, oxfmt 0.52.0) | PROVEN — frozen-lockfile mode fails on any lockfile drift; the gate's freshness preflight independently attested the installed tooling | `07-pnpm-install-frozen-lockfile.txt` | `628431e3dc5c4211b75b86b09bd7508cda7d5a8a3903157d0832b5e70d52fa79` |
| 08 | `pnpm test` (root script = `pnpm -r --parallel run test`) | `<WORKTREE>` | after build:ts + build:native + build:wasm | 1 (FAIL — pre-existing) | 24 packages carry a test script; the selector is PARALLEL + BAIL-ON-FIRST-FAILURE: when `@verter/typeinfo` failed (3 tests), pnpm terminated the remaining suites — only 6 packages ran to a summary (typeinfo 3f/28p, binary-launcher 47p, proto 16p, type-ir 6p, verter-lsp 47p, verter-mcp 47p), 17 were started-then-killed, `@verter/native` produced no output | see row 08c for the complete per-package truth | 24 workspace package test scripts | **A1 FINDING (recorded, not fixed): the canonical root JS selector is structurally bail-fast — on any red package it silently kills most of the JS test surface mid-run.** CI does not rely on it (ci.yml runs targeted per-package selectors). Executed work was non-zero and the failure is genuine, but this row alone cannot attest the full JS surface — row 08c does | `08-pnpm-test.txt` | (final table) |
| 08c | `pnpm -r --no-bail --parallel run test` (SUPPLEMENT: identical selection to row 08, `--no-bail` so every suite completes) | `<WORKTREE>` | after build:ts + build:native + build:wasm | 1 (FAIL — pre-existing) | 4448 vitest tests executed across 21 vitest-reporting suites in 19 packages (+ echo-only/no-op scripts in example, nuxt, svelte-jsx, verter-tsc; vue-conformance-oracle runs its own checks; vue-vscode chains 3 vitest runs: 150+38+451) | 4412 pass / 4 fail / 32 skip — failures: `@verter/typeinfo` 3 (extra-imports-structured, vue-instance-props, resolve-symbol), `@verter/unplugin` 1 | all 24 package test scripts incl. `@verter/native` (119 pass / 5 skip against the freshly built `.node`) and `@verter/wasm` (16 pass against the freshly built wasm) | PROVEN — per-package vitest counts for every package; the 4 failures are pre-existing reds recorded, not fixed | `08c-pnpm-test-no-bail.txt` | (final table) |
| 09 | `pnpm run test:scripts` | `<WORKTREE>` | none | 0 | 52 discovered+executed (vitest 14 + node --test 5 + 7 + 26) | 52 pass / 0 fail / 0 skip | `scripts/sccache-env.test.mjs`, `scripts/lib/publish-set.spec.mjs`, `scripts/editor-contracts/plenary-outcome-guard.test.mjs`, `scripts/validate-program-state.test.mjs` | PROVEN — native per-suite counts in output | `09-pnpm-test-scripts.txt` | `8e4966db3d713716df1cd0b9292baf5ec7e7b57eb525e3a38b07cffb06ec2b0c` |

## Native / WASM

| # | exact command | cwd | env/features | exit | discovered / executed | pass / fail / skip | binaries/packages/fixtures | non-vacuity | raw file | raw SHA-256 |
|---|---|---|---|---|---|---|---|---|---|---|
| 10 | `pnpm run build:native` (= `@verter/native`: `napi build -o dist --platform --release --manifest-path ../../crates/verter_napi/Cargo.toml` + type emit + Windows artefact copy) | `<WORKTREE>` | napi-rs CLI 3.x; release profile (fat LTO) | 0 | full release compile of verter_napi + dependency graph; artifact `packages/native/dist/verter-native.win32-x64-msvc.node` = 33,612,800 bytes | build success | crates/verter_napi cdylib; TS type emit | PROVEN — non-trivial platform artifact produced on disk AND subsequently LOADED: row 08c's `@verter/native` suite ran 119 tests against this exact `.node` | `10-build-native.txt` | (final table) |
| 11 | `pnpm run build:wasm` (= `@verter/wasm`: cargo release wasm32-unknown-unknown + wasm-bindgen 0.2.122 + binaryen wasm-opt -Os + tsdown + playground copy) | `<WORKTREE>` | wasm32-unknown-unknown; `getrandom_backend="wasm_js"` cfg from .cargo/config.toml | 0 | full release wasm compile; artifact `packages/wasm/wasm/verter_wasm_bg.wasm` = 15,699,519 bytes (copied byte-identical to `packages/playground/public/`) | build success | verter_wasm crate; wasm-bindgen CLI 0.2.122; binaryen from packages/wasm devDeps | PROVEN — non-trivial artifact produced (far above ci.yml's own 1000-byte smoke floor) AND actually INSTANTIATED by row 11b (the `@verter/wasm` unit suite in 08c mocks the wasm module and does NOT load the binary — recorded so nobody mistakes it for wasm-runtime proof) | `11-build-wasm.txt` | (final table) |
| 11b | `pnpm --filter @verter/playground test:wasm` (ci.yml "Playground WASM compiler spec" — the selector that actually instantiates the built wasm) | `<WORKTREE>` | after row 11 | 0 | 43 tests discovered, 41 executed | 41 pass / 0 fail / 2 skip | the freshly built `verter_wasm_bg.wasm` loaded through the playground compiler wrapper | PROVEN — real wasm instantiation + compile assertions with native counts | `11b-playground-wasm-spec.txt` | (final table) |

## Corpus / provider / conformance

Selector provenance: derived from the repository itself — root `package.json`
scripts, `.github/workflows/ci.yml` (jobs: JS Build & Test, Svelte Oracle, Svelte
CSS Conformance, Editor-neutral LSP contract) and
`.github/workflows/corpus-gate.yml`. No invented command names.

| # | exact command | cwd | env/features | exit | discovered / executed | pass / fail / skip | oracle/provider provenance | non-vacuity | raw file | raw SHA-256 |
|---|---|---|---|---|---|---|---|---|---|---|
| 12 | `pnpm run gen:vue-goldens:check` | `<WORKTREE>` | none | 0 | 286 committed oracle artifacts regenerated in-memory and compared | 286 match / 0 drift | official pinned `vue@3.6.0-rc.1` + `@vue/compiler-dom@3.6.0-rc.1` + `@vue/compiler-sfc@3.6.0-rc.1` + `@vue/compiler-vapor@3.6.0-rc.1`, esbuild 0.28.0, over `crates/verter_vue_conformance/corpus` | PROVEN — artifact count in output; sentinel C (below) proves one mutated golden byte flips this exact selector to exit 1 naming the artifact | `12-vue-goldens-check.txt` | `523bc19e1fd55543f5fa1b7df31d1d5644c1ea7323df3a3871b1fb1994bb54a8` |
| 13 | `node scripts/gen-svelte-goldens.mjs --check` | `<WORKTREE>` | none | 0 | 1066 goldens | 1066 in sync | official pinned `svelte@5.56.3` over the vendored `.svelte` corpus | PROVEN — golden count in output | `13-svelte-goldens-check.txt` | `92605a42105ebf2e2e0c2e365fd4788717217b4ad245de6a4cd7efa069b923f1` |
| 14 | `node scripts/gen-svelte-goldens.mjs --conformance --check` | `<WORKTREE>` | none | 0 | 1218 goldens | 1218 in sync | `svelte@5.56.3` + emit-plan manifest `fnv1a64-8faa9143878b8848` | PROVEN — golden count + manifest fingerprint in output | `14-svelte-conformance-goldens-check.txt` | `835a948b30e9039b5303603a307bf60c6b3f51e275fa6c35be7b43ab492db5b7` |
| 15 | `pnpm run gen:vue-macro-oracle:check` + `pnpm run test:vue-macro-oracle` | `<WORKTREE>` | none | 0 + 0 | 11 oracle cases + 4 node --test tests | 11 in sync; 4 pass / 0 fail | official pinned `@vue/compiler-sfc@3.5.34` runtime-macro differential | PROVEN — case/test counts in output (also re-executed inside gate row 01) | `15-vue-macro-oracle.txt` | `82128954607a88b9a35a4576169a27a4e30f8178d646f5c6b47885d2fb9ba9b7` |
| 16 | `pnpm --filter @verter/dx-harness test:corpus-gate` | `<WORKTREE>` | `VERTER_CORPUS_GATE_DIR` unset (private corpus is never committed and is unavailable in this environment) | 0 | 1 lane file discovered; 0 tests executed | 0 pass / 0 fail / 1 EXPLICIT skip | corpus-gate.yml external-corpus benchmark lane (all three provider routes when armed) | **EXPLICIT SKIP, recorded as SKIP not pass** — verbose reporter captured the lane's own skip declaration: "corpus gate skipped: VERTER_CORPUS_GATE_DIR is unset — … this skip is explicit, not a pass." A1 asserts nothing about corpus-gate results; the row proves the skip is honest and visible, which is the charter's requirement for an unavailable suite | `16-corpus-gate-skip.txt` | `99ab34a3401a7c1a52884fe8f2d42ae6c4bd0d066552292501e012c23f4b5cce` |
| 17 | `node scripts/gen-svelte-name-parity-corpus.mjs --check` | `<WORKTREE>` | none | 0 | 80 rows | 80 in sync | `svelte@5.56.3` name-parity corpus | PROVEN — row count in output | `17-svelte-name-parity-check.txt` | (final table) |
| 18 | provider matrix `pnpm run test:lsp:neutral` (root composite → `pnpm --filter @verter/dx-harness test:editor-neutral-lsp`, the ci.yml "Editor-neutral LSP contract" lane) | `<WORKTREE>` | debug `verter-lsp.exe`/`verter-relay-shim.exe`; tsgo = TS 7.0.2 native `tsc.exe` via `typescript/lib/getExePath.js`; tsserver SDK = typescript-plugin's nested TS | 1 (FAIL — pre-existing product issues) | inventory 92 cases; expectedExecutions 274; attempted 274 (assertion-enforced: zero-skip, per-route applicability tsserver 91 / tsgo 91 / shared-tsgo 92); vitest 277 tests (274 executions + authority controls) | 240 pass / 34 fail / 0 skip; per route: tsserver 83/8, tsgo 78/13, shared-tsgo 79/13; setupFailures [] | REAL providers: tsserver + managed tsgo + relay-backed shared-tsgo; TS CLI authority control ran `Version 7.0.2`; machine receipt `18r-editor-neutral-receipt.json` carries `sourceSha` = the candidate SHA | PROVEN — the harness itself asserts attempted == expectedExecutions == 274, every route started, receipts unique per route:case (all these afterAll assertions held — 0 hook errors); the 34 failures are pre-existing hover/rename/diagnostics product issues recorded, not fixed | `18-provider-matrix-lsp-neutral.txt` + `18r-editor-neutral-receipt.json` | (final table) |
| 19 | `cargo run -p verter_svelte_conformance -- check` | `<WORKTREE>` | default | 0 | conformance corpus (609 committed `.svelte` fixtures under `crates/verter_svelte_conformance/corpus`) reconciled against the manifest | verdict: "the conformance corpus matches the manifest" | corpus + manifest (fingerprint `fnv1a64-8faa9143878b8848`, corroborated by row 14's independent 1218-golden check of the same manifest) | asserted via the binary's own reconciliation verdict + row-14 corroboration on the same manifest identity (the binary prints no count; fixture inventory counted independently: 609) | `19-svelte-conformance-check.txt` | (final table) |
| 20 | `cargo test -p verter_compiler --features svelte-oracle` | `<WORKTREE>` | `--features svelte-oracle` (live feature-gated oracle harness; requires node_modules svelte@5.56.3 — present) | 0 | 6453 tests discovered across 4 suites (5939 lib + 0 + 507 oracle harness + 7) | 6441 pass / 0 fail / 12 ignored | pinned `svelte@5.56.3` live oracle | PROVEN — libtest per-suite counts; the 507-test suite is the feature-gated live oracle harness that a feature-off run does not execute | `20-svelte-oracle-live.txt` | (final table) |

## Prerequisite-build evidence (not canonical commands; recorded because CI runs them before the canonical selectors)

| # | exact command | mirrors | exit | raw file | raw SHA-256 |
|---|---|---|---|---|---|
| 00b | `pnpm --filter @verter/language-shared --filter @verter/typescript-plugin build` | ci.yml "Build the Rust gate's TypeScript prerequisites" (gate fails closed with exit 127 without it) | 0 | `00b-gate-ts-prereq-build.txt` | `1837bb3e6efee93f84b523b53cff823fe4047e77683215da4b9480f9c07399d9` |
| 08b | `pnpm run build:ts` | ci.yml "Build TS packages" (JS Build & Test job) | 0 | `08b-build-ts-prereq.txt` | `7edca239d11c6d56006b40a633719d96a8517a11badf49139e81fa050ffb4b6c` |

(Additional prerequisite rows and the final digest table are appended at the end of the block.)

## Final raw-output digest table (SHA-256 over raw captured bytes)

Computed after every run completed and every evidence file was final. The
sentinel copy and the candidate worktree were both verified clean
(`git status --porcelain` = 0) at `b7ea2dc88…` before this table was written;
the block's single documentation-only commit was created after all candidate
runs.

| file | SHA-256 |
|---|---|
| `00b-gate-ts-prereq-build.txt` | `1837bb3e6efee93f84b523b53cff823fe4047e77683215da4b9480f9c07399d9` |
| `01-gate-mjs.txt` | `d6e211153c23a674678280fcd9519ca87ba88715097c20600e2032e267a83e12` |
| `02-cargo-clippy.txt` | `9418cdf188cdab8bafe803aa64c9224176f5a309de4f88bd8aea3be1e266d5d8` |
| `03-cargo-check-release.txt` | `4ec650b1a48ca663f769238c3bd03bb004d8ca18664da9b1467c2dc8f72b772a` |
| `04-cargo-clippy-wasm.txt` | `d7b5ac68a28ab2d4562d815c25853a63c1eae4398585da8415570db034e970c3` |
| `05-cargo-fmt-check.txt` | `1d77feacf7cd43e4cab3e116fdd07823c01878a94f3b4b1ee05a2cf26a138013` |
| `06-cargo-test-doc.txt` | `16a1d23338b1c8fbd21ed1bfddd12a61d814d22c23f825413041152d12fe6f7e` |
| `07-pnpm-install-frozen-lockfile.txt` | `628431e3dc5c4211b75b86b09bd7508cda7d5a8a3903157d0832b5e70d52fa79` |
| `08-pnpm-test.txt` | `69cc801d380febeb2fcbc15cedc2aad46fc44ef4f29928b865c817a45e81a1f1` |
| `08b-build-ts-prereq.txt` | `7edca239d11c6d56006b40a633719d96a8517a11badf49139e81fa050ffb4b6c` |
| `08c-pnpm-test-no-bail.txt` | `3368bc916f4b1ff6257df7bec854933163fee501e00108decef97ffb0bdf67f7` |
| `09-pnpm-test-scripts.txt` | `8e4966db3d713716df1cd0b9292baf5ec7e7b57eb525e3a38b07cffb06ec2b0c` |
| `10-build-native.txt` | `ef571b62f25707eab01f44a70aa8832473933fa276d9e5aa8a9cb58d6996b69a` |
| `11-build-wasm.txt` | `57aea41948fe534ad7e91620a3db56a0b5d389dcf2559dfac3266687ee592d2d` |
| `11b-playground-wasm-spec.txt` | `eb7116912118991957dcd29b840f1048f8aeeaa87d98c1e681908fa2a6f94b23` |
| `12-vue-goldens-check.txt` | `523bc19e1fd55543f5fa1b7df31d1d5644c1ea7323df3a3871b1fb1994bb54a8` |
| `13-svelte-goldens-check.txt` | `92605a42105ebf2e2e0c2e365fd4788717217b4ad245de6a4cd7efa069b923f1` |
| `14-svelte-conformance-goldens-check.txt` | `835a948b30e9039b5303603a307bf60c6b3f51e275fa6c35be7b43ab492db5b7` |
| `15-vue-macro-oracle.txt` | `82128954607a88b9a35a4576169a27a4e30f8178d646f5c6b47885d2fb9ba9b7` |
| `16-corpus-gate-skip.txt` | `99ab34a3401a7c1a52884fe8f2d42ae6c4bd0d066552292501e012c23f4b5cce` |
| `17-svelte-name-parity-check.txt` | `77b27fbdefb6081f9057c36e65368f9f49085dc8bda6e1ea6b6feb63626ac465` |
| `18-provider-matrix-lsp-neutral.txt` | `2e39434c55ea9c63d64afd10428a139fb8e89b075dedb9ffe3746d4ceefa039e` |
| `18b-lsp-debug-builds.txt` | `cca06dd4f36880872261bf14cc17e7066b3af878845307a9fba73713336948d8` |
| `18r-editor-neutral-receipt.json` | `f37945d7ed00b5d27dcc20fdab404eb16c27bbcba6443dc9e3f06d0eed7a9f38` |
| `19-svelte-conformance-check.txt` | `f42b9f2ac95dc49f8ede771cc42eb58b7d1d771a38b57b55feaee3f53d7b5cc6` |
| `20-svelte-oracle-live.txt` | `730a25dc72866472bfaa7363d0e04dc00c09f3ff70367ecd6cc656311f6b5302` |
| `../sentinel-runs/fmt-negative-control.txt` | `d396034da028b9f1be30dfeae49ad9ffdb87d938375c5097424af39920ea40c2` |
| `../sentinel-runs/sentinel-A-gate.txt` | `c50c8770e498363a0c7602bc8eefda155ff29ae1b21450e0311b5a0f9dbe2fa0` |
| `../sentinel-runs/sentinel-A-gate-planted-raw.txt` | `45578bc4bf19c4477bd09ffabc78db6bb9c6db9f4840e8d7fb88309bed59f634` |
| `../sentinel-runs/sentinel-A-gate-restored-raw.txt` | `3fddf01b4a486734ab446ebcab2033fafd54d173a9a7a5abfaba728bfdf6abeb` |
| `../sentinel-runs/sentinel-B-vitest.txt` | `7950a0973974e3cefccfb72cb6b51b994977774c823b75b71992afc5b882b564` |
| `../sentinel-runs/sentinel-B-vitest-planted-raw.txt` | `9bb1bd2b8a67f522ea9dd2f58adc30f42b1c0eb7610e32d8df83caadaa88f55b` |
| `../sentinel-runs/sentinel-B-vitest-restored-raw.txt` | `f070073c414fc128b59badb9adaab8be055d0605bcab65bb4f6cab0d2cc362f0` |
| `../sentinel-runs/sentinel-B2-planted-raw.txt` | `fbb0a2a492f6a2e509c913e2809a9a4cac330da1fd6520b9c756fe0424a11216` |
| `../sentinel-runs/sentinel-B2-restored-raw.txt` | `937b4adb3a7d0657faf00e6ca192c7c4a213a2ac4cab41a9c9efba924108800f` |
| `../sentinel-runs/sentinel-B3-planted-raw.txt` | `d3eab9fbdd39b49f0e9b18a32017db78abacad93d28acb17db548357baac0542` |
| `../sentinel-runs/sentinel-B3-restored-raw.txt` | `fa033e1002796f195a641bcf552a2728e2db56ac8e1717dc064531d576820206` |
| `../sentinel-runs/sentinel-C-vue-goldens.txt` | `6237ae70c719d7224915630691ea2dab4d3676fbf5d50b3767c0fdf117fbbe5a` |
| `../environment.md` | `9a32cf7507de083979d9700baddc8bdcc94e1028b79b19ef1c9356ed44a3ee95` |
| `../context-packet.md` | `0bdab0e8fadef31e40d69fdd337a780d2d7c02f9130cd35974009266ee7cd7a2` |
| `../sentinel-verification.md` | `852e50626e6876e947bdf4283926b6a85d1d84eaab3b959d5eb2b3e571c601ee` |

