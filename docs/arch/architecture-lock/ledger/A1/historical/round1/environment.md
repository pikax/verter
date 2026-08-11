# A1 environment / provenance bundle (verification.md §2)

DATE: 2026-08-10T04:57:39Z
candidate worktree: (A1 worktree beside main checkout)
candidate SHA: b7ea2dc88bda86473de81de3438b7f88ef30adc7
candidate tree: 47645406a9246e600af995c62608b709347e13a4
branch: block/a1-command-truth
dirty at capture: 0 modified/untracked entries

## Toolchain
rustc: rustc 1.97.1 (8bab26f4f 2026-07-14)
cargo: cargo 1.97.1 (c980f4866 2026-06-30)
rustup active toolchain: 1.97.1-x86_64-pc-windows-msvc (overridden by '<REPO>-wt-a1\rust-toolchain.toml')
cargo-nextest: cargo-nextest 0.9.137 (75ddba7e9 2026-05-26)
clippy: clippy 0.1.97 (8bab26f4f6 2026-07-14)
rustfmt: rustfmt 1.9.0-stable (8bab26f4f6 2026-07-14)
node: v26.5.0
[WARN] The "pnpm" field in package.json is no longer read by pnpm. The following keys were ignored: "pnpm.onlyBuiltDependencies". See https://pnpm.io/settings for the new home of each setting.
pnpm: 10.22.0
git: git version 2.54.0.windows.1

## Pinned versions (package.json devDependencies / dependencies)
typescript: 7.0.2
vue: 3.5.34
svelte: 5.56.3
vitest: ^4.1.7
@vue/compiler-sfc: 3.5.34
packageManager: pnpm@10.22.0+sha512.bf049efe995b28f527fd2b41ae0474ce29186f7edcb3bf545087bd61fbbebb2bf75362d1307fda09c2d288e1e499787ac12d4fcb617a974718a6051f2eee741c

## Lockfiles
Cargo.lock sha256: b4fb9825718c60ca8439744953a82958380dfdb18daabc2ed686e4918e838b27
pnpm-lock.yaml sha256: 3f789a2ade9617b68dc75b2734b36ab331c5aa0518f44e0d04a33dec7cda1cfb

## rust-toolchain.toml
# EXACT-PINNED, deliberately. Not a floating `stable`.
#
# The repo byte-pins rustc's own diagnostic text in 27 trybuild `.stderr`
# fixtures (`crates/*/tests/**/*.stderr`). Those fixtures are a function of the
# compiler version: a rustc release that rewords one note or renumbers one span
# turns every affected fixture red, and re-blessing them against whichever rustc
# the contributor happens to have installed makes CI fail instead. Nothing else
# in the tree binds the fixtures to a compiler, so this file is that binding.
#
# rustup's toolchain-file override outranks `rustup default`, so this pin wins
# over whatever a CI action installed as the default toolchain.
#
# KEEP IN SYNC: `.github/workflows/*.yml` reference `dtolnay/rust-toolchain@<ver>`
# with this same version. That action does NOT read this file — it installs the
# toolchain named by its `@ref` and then `rustup default`s it. The `targets:` and
# `components:` it installs land on THAT toolchain, not on the one this file
# selects, so a drifted ref means `cargo build --target <t>` fails with a missing
# `std` for the pinned toolchain (the release cross-compile matrix in
# `release.yml` installs `${{ matrix.target }}` this way). Bump both together.
#
# ONE-TIME CI COST, on this pin and on every future bump. `Swatinem/rust-cache@v2`
# hashes `rustc -vV` for the ACTIVE toolchain PLUS one per line of `rustup
# toolchain list --quiet` — not just the active one. GitHub runners preinstall
# `stable`, so installing a named version makes that list two entries where it was
# one. The versions collect into a `Set<RustVersion>` of freshly-parsed objects,
# which dedupes by REFERENCE, not by value: `stable` and `1.97.1` resolving to the
# identical compiler does NOT collapse them, and the same version string is folded
# into the digest twice. That digest is part of `restoreKey`, the prefix the cache
# falls back to, so the exact key and the fallback miss together.
#
# Consequence: the first run after this lands cold-builds cargo in EVERY Rust job.
# It self-heals from the second run — the new key is stable. A green-but-slow CI
# run right after a toolchain bump is this, not a regression to investigate.
[toolchain]
channel = "1.97.1"
components = ["rustfmt", "clippy"]
# Targets are PER-TOOLCHAIN, and rustup treats `stable` and `1.97.1` as two
# different toolchain identities even while they are the same compiler. Anyone
# who had `wasm32-unknown-unknown` on `stable` does NOT have it on the pin, and
# `pnpm build:wasm` (part of `pnpm build`) then dies with `E0463: can't find
# crate for std`. Listing it here makes rustup install it with the toolchain.
#
# `wasm32-wasip1` / `wasm32-wasip2` are deliberately NOT listed: they build the
# separate `extensions/lapce` and `extensions/zed` manifests, which are not
# workspace members and not part of `pnpm build`. CI installs those two through
# the action, onto this same pinned toolchain.
targets = ["wasm32-unknown-unknown"]

## Platform
OS: Microsoft Windows [Version 10.0.26200.8875]
CPU: AMD Ryzen 9 7950X 16-Core Processor             x 32 logical
MEM: 127 GB total
arch: x64

## Supplement (captured at close of block)
wasm-bindgen CLI: wasm-bindgen 0.2.122
napi CLI: 3.6.2
binaryen (wasm-opt): packages/wasm devDependency binaryen ^130.0.0 (package-local bin)
buf: 1.70.0
oxfmt: Version: 0.52.0
tsserver SDK (typescript-plugin nested): 6.0.3
tsgo: TypeScript 7.0.2 native tsc.exe (@typescript/typescript-win32-x64)
allocator: MSVC default (no custom global allocator in gate surfaces; verter_session allocator_canaries uses its own counting allocator in its dedicated binary)
panic mode: unwind (workspace default; dev + no-debug-assertions + release profiles)
profiles exercised: dev (gate S1/S2), no-debug-assertions (gate S3: debug_assertions OFF, dev codegen), release (check --release, build:native, build:wasm — opt-level 3 + fat LTO)
CPU governor/background-load policy: developer workstation, no governor pinning; background load minimized but not isolated (recorded per verification.md §2 — this is evidence-collection hardware, not a locked benchmark runner; A1 records no performance claims)

## Dirty-state disclosure
Every candidate command ran with the worktree CLEAN (0 entries). The block's only tracked change (capability-matrix completion) was applied AFTER all candidate command runs and sentinel baselines; it changes documentation only.
