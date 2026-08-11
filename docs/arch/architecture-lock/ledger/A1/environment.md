# A1 environment / provenance bundle (verification.md §2) — round 2

DATE: 2026-08-10T07:25:44Z (toolchain capture; candidate committed 2026-08-10T07:17:47Z, first command run 2026-08-10T07:19:20Z, per-row DATE/END-DATE in each raw file)
candidate worktree: (A1 worktree beside main checkout)
candidate SHA: 13cedd6fc1315bfb6fec0c4cacb0eacdb02c6c83
candidate tree: a992bb87382e58d6ec846c7be37cbb941ee0b1b2
block base SHA: b7ea2dc88bda86473de81de3438b7f88ef30adc7
block base tree: 47645406a9246e600af995c62608b709347e13a4
branch: block/a1-command-truth (single commit parented on the block base)
dirty at capture: 0 modified/untracked entries

**Run ordering (round-2 correction):** the block's single tracked change (the
capability-matrix completion) was committed FIRST, producing the candidate
identity above; EVERY command and sentinel run below then ran against that exact
SHA/tree with a clean worktree, and NO tracked change followed the runs. (The
superseded round-1 bundle under `historical/round1/` had run every command on
the block-base tree instead; it is retained for audit only and is not the
review evidence.)

## Toolchain
rustc: rustc 1.97.1 (8bab26f4f 2026-07-14)
cargo: cargo 1.97.1 (c980f4866 2026-06-30)
rustup active toolchain: 1.97.1-x86_64-pc-windows-msvc (overridden by '<REPO>-wt-a1\rust-toolchain.toml')
cargo-nextest: cargo-nextest 0.9.137 (75ddba7e9 2026-05-26)
clippy: clippy 0.1.97 (8bab26f4f6 2026-07-14)
rustfmt: rustfmt 1.9.0-stable (8bab26f4f6 2026-07-14)
node: v26.5.0
pnpm: 10.22.0
git: git version 2.54.0.windows.1

## Pinned versions (package.json devDependencies / dependencies)
typescript: 7.0.2
vue: 3.5.34
svelte: 5.56.3
@vue/compiler-sfc: 3.5.34
vue conformance-oracle pin: vue@3.6.0-rc.1 (goldens tree `crates/verter_vue_conformance/corpus/goldens/3.6.0-rc.1/`, three backend trees: vdom, vapor, vdom-inline)
packageManager: pnpm@10.22.0+sha512.bf049efe995b28f527fd2b41ae0474ce29186f7edcb3bf545087bd61fbbebb2bf75362d1307fda09c2d288e1e499787ac12d4fcb617a974718a6051f2eee741c

## Lockfiles
Cargo.lock sha256: b4fb9825718c60ca8439744953a82958380dfdb18daabc2ed686e4918e838b27
pnpm-lock.yaml sha256: 3f789a2ade9617b68dc75b2734b36ab331c5aa0518f44e0d04a33dec7cda1cfb

## rust-toolchain.toml
Exact-pinned `channel = "1.97.1"`, components rustfmt + clippy, target
wasm32-unknown-unknown. The full annotated file (byte-identical at base and
candidate) is quoted in `historical/round1/environment.md`; the tree diff
between base and candidate is one documentation file, so every toolchain-
relevant byte is unchanged.

## Platform
OS: Microsoft Windows [Version 10.0.26200.8875] (Windows 11 Pro)
CPU: AMD Ryzen 9 7950X 16-Core Processor x 32 logical
MEM: 127 GB total
arch: x64

## Supplement
wasm-bindgen: 0.2.122 (Cargo.lock)
napi CLI: @napi-rs/cli ^3.6.2
binaryen (wasm-opt): packages/wasm devDependency binaryen ^130.0.0 (package-local bin)
buf: 1.70.0 (node_modules/.bin — present, so the gate's typeinfo byte-pin ran with tolerance DISABLED)
oxfmt: 0.52.0
tsserver SDK (typescript-plugin nested): 6.0.3
tsgo: TypeScript 7.0.2 native tsc.exe (@typescript/typescript-win32-x64)
native artifact: packages/native/dist/verter-native.win32-x64-msvc.node = 33,612,800 bytes
wasm artifact: packages/wasm/wasm/verter_wasm_bg.wasm = 15,699,519 bytes (playground copy byte-identical)
allocator: MSVC default (no custom global allocator in gate surfaces; verter_session allocator_canaries uses its own counting allocator in its dedicated binary)
panic mode: unwind (workspace default; dev + no-debug-assertions + release profiles)
profiles exercised: dev (gate S1/S2), no-debug-assertions (gate S3: debug_assertions OFF, dev codegen), release (check --release, build:native, build:wasm — opt-level 3 + fat LTO)
CPU governor/background-load policy: developer workstation, no governor pinning; the worktree command battery and the clone sentinel battery were run SEQUENTIALLY (never concurrently) to keep load conditions honest for flake attribution; background load minimized but not isolated (this is evidence-collection hardware, not a locked benchmark runner; A1 records no performance claims)

## External corpus (row 16) — CLASSIFIED handling
The corpus-gate row ran against a locally available external corpus whose name
is classified: it appears NOWHERE in this bundle or in the tracked diff. It is
identified only by the anonymous label `Corpus A`
(`VERTER_CORPUS_GATE_LABEL`) and by an immutable content fingerprint:
SHA-256 over the sorted manifest of `sha256(content)<2 spaces>relative-path`
for every non-node_modules `.vue` file under the corpus root (290 files):
`7e9a65dd26b4cd1f17158aa26dc658e8a10768668a44a7aae74067e171f6dec5`.
`VERTER_CORPUS_GATE_FILE_DETAIL` was left unset, so sampled relative paths are
not embedded in the receipt; the raw stdout and receipt were audited (and
redacted where needed) so no corpus-identifying path fragment remains.

## Dirty-state disclosure
Every candidate command and sentinel baseline ran with the worktree CLEAN
(0 entries) at the candidate SHA above; the tracked change predates all runs.
