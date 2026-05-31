# Rust build & test time measurements

Empirical, cross-platform measurements of Verter's Rust build/test loop, taken on
the `perf/build-test-times` branch. The goal is to find the durable levers for
faster build + test cycles and to separate **platform quirks** (debug-info
format, linker choice) from the **structural** win (fewer / parallel compilation
units).

> Methodology: all numbers are wall-clock, taken **warm** (the dependency graph
> compiled once before each incremental measurement). We compare *orders of
> magnitude*, so the minor rustc-version spread across hosts (1.92 / 1.94 / 1.96)
> is not significant. Where a result was surprising it was re-measured under a
> controlled back-to-back A/B to rule out disk-cache / thermal confounds.

## Hosts

| Host | CPU | Cores | RAM | rustc | Linker (default) | dev `split-debuginfo` default |
|---|---|---|---|---|---|---|
| Windows | (x86_64 MSVC) | 32 | — | 1.92 MSVC | `link.exe` | `packed` (PDB) |
| Linux (WSL) | (x86_64) | 8 | — | 1.94 | `ld` (GNU) | `off`/embedded (ELF) |
| macOS | Apple M2 (aarch64) | 8 | 8 GB | 1.96 | `ld-prime` (ld-1053.12) | **`unpacked`** (DWARF in `.o`) |

The macOS default linker (`ld-prime`, shipped with the Xcode 15+ toolchain,
reporting `PROJECT:ld-1053.12`) is already the modern fast Mach-O linker. macOS
dev builds use `unpacked` split-debuginfo: DWARF stays in the per-CU `.o` files
and `dsymutil` only runs on demand — so the debug **level** affects rustc
*codegen* time and artifact size, but barely affects *link* time.

## Results

### M1 — `verter_session` rlib incremental recompile (`touch lib.rs; cargo build -p verter_session --lib`)

| Measurement | Windows (32c) | Linux (8c WSL) | macOS (M2 8c) |
|---|---|---|---|
| recompile @ `debug=2` | 76 s | ~17 s | **2.59 s** |
| recompile @ `line-tables-only` | **5 s** | ~22 s (= noise) | **2.57 s** |
| rlib size `debug=2` | 433 MB | (re-measure) | 251 MB |
| rlib size `line-tables-only` | 72 MB | (re-measure) | 84 MB |

- **macOS recompile time is debug-level-neutral** (2.57 vs 2.59 s — within noise),
  exactly as the `unpacked` split-debuginfo hypothesis predicts: the DWARF is
  emitted into `.o`s either way and there is no expensive debug-info packing at
  rlib time.
- The artifact, however, is **~3× larger** at `debug=2` (251 MB vs 84 MB) — a
  pure disk/IO cost with no compile-time benefit on this platform.

### M2 — aggregate test build, `verter_session` (302 test binaries)

| Measurement | Linux (8c WSL) | macOS (M2 8c) |
|---|---|---|
| Cold build all test bins @ `line-tables-only` | 78 s | 108.3 s |
| Cold build all test bins @ `debug=2` | — | 127.7 s |
| Relink-all (edit lib → rebuild suite) @ `line-tables` | 57–59 s (ld) / 74–77 s (mold) | 54.2 s |
| Relink-all @ `debug=2` | — | 56.6 s |

- On the **cold** aggregate, `line-tables-only` is **~18 % faster** on macOS
  (108 vs 128 s) — here the codegen/IO savings of less debug info *do* show up,
  multiplied across 302 compilation units.
- On the **relink-all** loop the difference shrinks to ~5 % (54.2 vs 56.6 s,
  near noise): that loop is dominated by recompiling the test crates themselves,
  and linking stays cheap thanks to `unpacked` split-debuginfo.
- macOS relink-all (54.2 s) is comparable to Linux `ld` (57–59 s).

### M3 — linker swap (`verter_lsp` incremental relink, warm steady-state)

| Platform | Default | Alternative | Verdict |
|---|---|---|---|
| Windows | `link.exe` | `lld-link` ≈ same | no help |
| Linux | `ld` 57–59 s | `mold` 74–77 s | **slower**; bundled `rust-lld` unstable on stable |
| macOS | `ld-prime` **2.74 s** | `lld` (brew) **2.99 s** (~9 % slower) | **no help** |

- macOS controlled A/B (3 warm samples each): `ld-prime` 2.76/2.74/2.73 s vs
  `lld` 3.30/3.00/2.96 s. The default Apple linker is **slightly faster** than
  `lld`. An initial run that appeared to favour `lld` was a confound — it
  compared lld-warm samples against default-ld samples still settling cold disk
  cache; under identical warm conditions the default wins.
- `mold` is **ELF-only → not applicable** on Mach-O.

### M4 — full workspace (macOS M2, all 23 crates / 351 test bins / 12,365 tests)

`verter_session` is the worst crate but not the whole picture. Measured across the
entire workspace, before (`debug=2`, the `refactor/semantic-db-overhaul` default)
vs after (`line-tables-only`, this branch):

| Measurement | Before (`debug=2`) | After (`line-tables`) | Delta |
|---|---|---|---|
| Cold build **all 351 test bins** | 206.9 s (3:27) | 172.3 s (2:52) | **−17 % (−35 s)** |
| **Test execution** (`cargo nextest run --workspace`, 12,365 tests) | 131.6 s | ~131–153 s | **neutral** |
| Peak swap during build | **1023 MB (swap file maxed)** | 807 MB | debug=2 worse |
| Min free RAM during build | 31 % | 31 % | both tight |
| `target/debug` size | **66 GB** | ~22–25 GB (3× less debuginfo) | much smaller |

- **Test *execution* is debug-flag-neutral** (~131 s either way): debug info does
  not change test runtime. `line-tables-only` is a build/disk lever, never a
  test-speed lever.
- Full suite result on this host: **12,339 / 12,365 pass; 26 fail; 586 skipped.**
  All 26 failures are **environmental / pre-existing and debug-independent** — no
  Node.js installed (`verter_type_runtime` node-discovery), `ts-rs` 12.0.1
  binding-format assertions (`verter_audit`), and corpus-regeneration parity
  (`verter_session::corpus_generator_parity`). None relate to build flags.

### Memory: 8 GB host

The macOS host has only 8 GB RAM, so memory pressure was measured directly
(`vm.swapusage` + `memory_pressure` sampled every 2–3 s):

| Phase | New swap growth | Min free RAM |
|---|---|---|
| `verter_session` relink-all (302 bins) | **0** (swap flat) | 43 % |
| Full workspace build @ `line-tables` | ~574 MB | 31 % |
| Full workspace build @ `debug=2` | **swap file maxed (1024 MB)** | 31 % |
| Full workspace **test run** (execution) | none beyond build | 54 % |

- 8 GB is **fine for single-crate builds and for test execution** (no swap, never
  tight).
- 8 GB **is a real constraint for the full-workspace *build*** — it swaps at
  `line-tables` and saturates the swap file at `debug=2`. This is an additional
  argument for `line-tables-only` (less memory pressure + 3× less disk) and for
  the structural consolidation (fewer parallel compile/link jobs in flight).

## Verdict

1. **Debug-info win is platform-shaped, as hypothesised.** It is enormous on
   Windows (PDB packing: 76 s → 5 s, ~15×), neutral on Linux, and on macOS it
   sits *between*: **neutral on incremental recompile**, but a real **~18 % win
   on cold aggregate builds** plus a **3× smaller rlib**. The mac win is a
   codegen/IO win on bulk builds, not a link win.

2. **Linker swaps don't help anywhere.** Windows `lld-link` ≈ `link.exe`; Linux
   `mold` is slower; macOS `lld` is ~9 % slower than the default `ld-prime`.
   `mold` is N/A on Mach-O. The default modern linker is the right choice on all
   three OSes.

3. **`debug = "line-tables-only"` is neutral-or-better on macOS** — neutral on
   incremental recompile, faster on cold aggregate builds, 3× smaller artifacts,
   and it keeps panic/backtrace `file:line`. **No regression: the committed
   `[profile.dev] debug = "line-tables-only"` change is safe to keep
   cross-platform** (no per-OS scoping needed).

## The durable, cross-platform lever is structural

Flags and linkers are platform quirks with small or zero payoff. The real lever
is **fewer / more-parallel compilation units**:

- `verter_session` produced **301 separate test binaries**; touching `lib.rs`
  forced all of them to recompile + relink. **LANDED** (see
  [`test-binary-consolidation.md`](./test-binary-consolidation.md)): consolidated
  to **23 binaries** via `#[path]` mod-root groups, in three gated phases.
  Measured relink-all (Windows 32-core): **~98 s @ 301 → ~70 s @ 200 → ~19 s @ 23
  (~5×)**. The win is larger on the 8-core Linux/macOS machines where link
  parallelism can't hide the binary count.
- Longer term, splitting the 355-file `verter_session` crate increases
  cross-crate parallelism and shrinks the incremental blast radius.
