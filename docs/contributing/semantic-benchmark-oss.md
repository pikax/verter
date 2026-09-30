# Semantic Benchmark: Open-Source Comparisons

The [semantic benchmark](./semantic-benchmark.md) compares Verter with
TypeScript 7.0.2 on equivalent demands. `--oss` adds pinned open-source
tools to the same run — all of them, or a comma-separated subset
(`--oss tsz,biome`) — each in its own section:

| Tool | Section |
|---|---|
| `tsz`, `bamtiscript`, `ezno` | the type checkers, whole program, on the program tsc's reference answers were measured on, beside `tsc -p` and a one-shot Verter process answering the demand |
| `biome` | semantic only: Biome has no type query, so Biome, Verter and tsc answer whether the declared type of `__Probe` is Promise-like, on a paired catalog (`--biome` alone selects just this section) |

Biome cannot join the checkers' section: that section reads each tool's
answer from the type it prints in an assignability error on the measuring
program, and Biome, a linter, reports no assignability errors and prints no
types. Its inference answers only yes/no questions through its type-aware
rules, so it gets the one demand it can answer, on its own catalog.

Without `--oss` or `--biome` nothing changes: the same arms, schedule, validation and
report as before. With a flag, the run gains its own section in
`results.json` (`oss`, `biome`) and in `results.md` (after every default
section), each with its own schedule and validation; the command exits 0
only when every section validates. `validate.mjs` re-validates the sections
of a stored run, re-reading every raw output.

This page records the method, not numbers: results are machine-bound and
belong to a run's own `results.md`.

## Fairness

The rules of the main benchmark hold for every third-party tool:

- **Correctness before speed.** A row counts in a comparison only when the
  tool's answer matches tsc 7.0.2's measured answer. A wrong, partial,
  unreadable, erroring or killed answer is a finding, reported, never a win.
- **Equivalent work.** Every arm reads the same library (`lib.bench.d.ts`,
  as a root file under `noLib`), the same module and the same tsconfig. Where
  a tool needs its own channel to state its answer, only that one line
  differs, and the page says so.
- **Same containment.** Every third-party process — each benchmark
  invocation, each build of a tool from source, each smoke check — runs under
  `verter-supervise` with a memory cap and a deadline; benchmark invocations
  get exactly the cap and deadline tsc gets (`--mem-mb`, `--infra-mb`,
  `--timeout-ms`).
- **Same schedule.** Fresh processes, warmups, counterbalanced order: the
  main benchmark's `schedule`, over each section's own arms.
- **Provenance.** Every tool is pinned by release and commit in
  `scripts/benchmark/semantic-perf/oss/tools.json`; a release file is
  verified by its pinned sha256, a source build checks out the pinned
  commit. The run records the source, the build (command and `rustc -vV`),
  the license and the binary's sha256, and validation fails when a binary
  changed during the run.
- **No silent skips.** A tool that does not build, or does not run on this
  platform, is reported as `unavailable on <platform>: <reason>`.

## The tools

| Tool | Pin | License | Windows x64 | macOS arm64 | How it answers |
|---|---|---|---|---|---|
| [tsz](https://github.com/tsz-org/tsz) | v0.1.75 | Apache-2.0 | release archive | release archive | a tsc-compatible CLI: the measuring program's diagnostics, in tsc's format |
| [bamTiScript](https://github.com/gosuda/bamTiScript) (`bamts`) | commit `60003d0f` (no releases) | MIT | built from source; **does not run**: its file system rejects every Windows drive path (`TS5083: platform path prefix is unsupported`) | built from source | a tsc-compatible CLI (`--noEmit`), as tsz |
| [Ezno](https://github.com/kaleidawave/ezno) | 0.0.23 | MIT | release binary | built from source at the release commit (no macOS release) | diagnostics only, in its own format (no TypeScript codes); it cannot read the benchmark library (its definition files use their own format), so its rows are findings |
| [Biome](https://biomejs.dev) | 2.5.15 | MIT OR Apache-2.0 | release binary | release binary | no type query; its type inference answers only through type-aware lint rules (`--biome`) |

A checker's language server (tsz and bamTiScript have one) could answer a
per-alias hover, but hover text elides large types and each server prints
its own form, so the benchmark uses the one channel every CLI shares with
tsc's reference: diagnostics on the measuring program.

## The type checkers

Each cell of the run (scenario × setting) gets a `measure/` directory: the
library, the tsconfig, and the scenario plus the reference's measuring
suffix (see Correctness first in the main page). Its arms:

| Arm | Command | Role |
|---|---|---|
| `tsc-measure` | `tsc -p <tsconfig>` | the reference's own method, default (parallel) checkers; must reproduce the reference or the run fails |
| `tsc-measure-1` | `tsc -p <tsconfig> --singleThreaded` | the same, one checker thread |
| `verter` | the Verter probe's `run` job on the same files (no warm repeat) | the declared type of `__Probe`: the demand, not a whole-program check |
| `oss-<tool>` | the tool's own command on the same program (`tools.json` `argv`) | the finding |

A tool's output is read with the reference's own reader
(`parseMeasurement`, `interpretMeasurement`): the head line of the TS2322 on
the measuring assignment prints the probe's type, the second assignment
whether it is `never`, and every other diagnostic's code is kept. Classes:

| Class | Meaning |
|---|---|
| `matched` | the canonical answer and the set of other diagnostic codes equal tsc's (tsc's error-any beside a resource code matches only the same error-any beside the same code) |
| `beyond-tsc` | tsc stops at a resource limit and the tool's answer is the scenario's constructed one |
| `mismatch` | another answer, or the same answer with other diagnostics |
| `unreadable` | the output states no answer in tsc's form (another format, a missing or contradictory verdict, a print that is not type syntax) |
| `error` / `killed` / `unverified` | an abnormal exit; the engine exhausted the cap or the deadline (a whole-program process is its engine from its start, so a kill is attributed by the `tsc -p` rule); a kill without that evidence |
| `no-reference` | tsc gives no answer to compare with |

The `verter` arm is classified by the demand section's own Verter classifier
(`matched`, `mismatch`, `partial`, `refusal`, `beyond-tsc`, …): its type
must equal tsc's, and no diagnostic codes are compared because Verter
reports none for the whole program.

**Timing units.** Every OSS arm is a whole-program, one-shot process, so its
units are the supervisor's: wall time from process start to exit (process
start, parse, bind, check and print included, for every arm alike), the
process tree's peak memory, and CPU time. A matched row shows the arm against
`tsc-measure` (tsc as shipped) and `tsc-measure-1` beside it, and each
matched tool against Verter where Verter matched too. **Against tsc -p,
over the run** tallies each arm: how many rows it matched, and on those
rows how often it or tsc won each metric, or neither.

**Verter here does less work.** tsc and the checkers check the whole
program and report every diagnostic; Verter answers the one demanded type
(it exposes no whole-program pass). A Verter verdict in this section is
therefore a demanded answer against a whole-program check on the same
bytes, as a whole process — not like for like. Its process also reads its
own OS statistics and prints the full answer, and its in-engine first type
handle is shown apart.

**Not comparable, so never shown side by side:** these whole-process
figures and the in-engine figures of the demand section (`verter`, `tsc-api`
there). A warm figure (a one-shot process has no in-process repeat). A
tool's self-reported timings.

## Biome: semantic only

Biome is a linter with its own type inference, not a type checker: it has no
type query (no CLI, and the daemon's type dump is an unstable debug format
of a whole module). Its inference answers only through the decisions of its
type-aware rules, and one of them projects a declared type:
`noFloatingPromises` reports an unhandled call whose result is Promise-like.
So every tool answers the same **boolean projection** of the demand — is
the declared type of `__Probe` Promise-like? — on the **thenable catalog**
(`oss/thenable.mjs`):

- Each feature of the main catalog's families (relations, template literals,
  alias and conditional chains, a tail-recursive parse, generic-call,
  overload, `infer` and callback inference, library shapes, mapped types)
  comes as a **pair** of programs differing in one place, one whose `__Probe`
  is Promise-like and one whose is not. A tool that does not evaluate the
  feature answers both alike and cannot decide the pair: neither "always
  yes" nor "always no" scores.
- The expected answer of each program is tsc 7.0.2's, measured by the
  reference's method (`oss/measure-thenable.mjs` writes
  `oss/thenable-expected.json`, with the measuring method, the library and
  the tsc executable recorded). The run's tsc arm must reproduce it.

| Arm | Program | Answer |
|---|---|---|
| `tsc-thenable` | the module plus the measuring suffix, `tsc -p` | the reference's reading, projected |
| `verter-thenable` | the module, the Verter probe's `run` job (no warm repeats) | the declared type of `__Probe`, projected; it must also equal tsc's answer in full |
| `biome-types` | the module plus `declare function __bench_probe(): __Probe; __bench_probe();`, `biome lint` with only `noFloatingPromises` | Promise-like exactly when the rule reports that call's line; any other diagnostic makes the output unreadable |

**Timing unit:** a cold process to a decided answer — start, read the library
and the module, answer, print, exit — for every arm, under the same
supervisor, cap and deadline. A row is compared (wall, peak, CPU) only when
all three arms answered as tsc answers. Verter's process also reads its own
OS statistics and prints the full answer; Biome's builds its project scan;
Verter's in-engine first type handle is shown apart.

**Not measured:** lint throughput and rule sets (a different question from
the semantic one), formatting (Verter has no formatter), and editor (LSP)
latency.

## Provisioning

```bash
node scripts/benchmark/semantic-perf/oss/provision.mjs [--tools tsz,biome] [--force] \
  [--supervisor <exe>] [--build-mem-mb 12288] [--build-timeout-ms 3600000] [--jobs <n>]
```

A run with `--oss` (or `--biome`) provisions a missing tool itself. Each tool
lands in `target/oss-tools/<tool>/<pin>/` (outside the tracked tree) and is
never updated: a new pin is a reviewed edit of `tools.json`, and lands in a
new directory. A release file or archive is downloaded and checked against
its pinned sha256; a source build clones the pinned commit, verifies
`HEAD`, and runs the pinned build under the supervisor (build memory cap
and deadline as above, a constructed environment, `CARGO_INCREMENTAL=0`,
the tool's own `rust-toolchain` file). Every provisioned tool then passes a
**smoke check** under the supervisor: one one-line program with one type
error, which it must report. The directory ends with `provenance.json` (what
was fetched or built, the smoke result and the binary's sha256) or
`unavailable.json` (the platform and the reason), and a run refuses a binary
whose hash no longer matches its record.

## Running

```bash
node scripts/benchmark/semantic-perf.mjs --oss                        # every tool; Windows (and Linux)
node scripts/benchmark/semantic-perf.mjs --oss --allow-sampled        # macOS
node scripts/benchmark/semantic-perf.mjs --tier standard --oss tsz    # one checker
node scripts/benchmark/semantic-perf.mjs --biome                      # the Biome section alone
node scripts/benchmark/semantic-perf.mjs --oss --allow-sampled --no-tsc  # every tool, no tsc arm anywhere
node scripts/benchmark/semantic-perf.mjs --only-oss --allow-sampled   # every tool, without the demand section
node scripts/benchmark/semantic-perf.mjs --only-biome                 # the Biome section, without the demand section
```

`--no-demand` skips the demand section (Verter vs the tsc API on each
scenario's demanded type): its report says so, and every other selected
section still runs, tsc included (the Verter probes are still built, since
the sections run them). `--only-oss [tools]` is `--oss --no-demand`;
`--only-biome` is `--biome --no-demand`.

`--oss` alone already selects Biome's section as well as the checkers'.
`--no-tsc` runs no tsc process in any section: the demand section keeps only its
Verter arms, the checkers' section drops `tsc-measure`/`tsc-measure-1`, and
the Biome section drops `tsc-thenable`. Every answer is still classified
against the measured reference files, so `matched` keeps its meaning, but no
row is timed against tsc: the checkers' head-to-head becomes a table of
each arm's own figures with each matched tool set against Verter, and a Biome row is compared (Verter with
Biome) when both of those arms matched. What needs a live tsc answer is lost:
the run cannot confirm that tsc still reproduces the reference, and the main
run cannot class an answer `beyond-tsc` or take an API answer as the
reference by construction.

The OSS arms run on the tier's cells (quick, standard, stress, or `--only`);
the Biome catalog is small and runs whole in every tier.

## Tests

`node --test scripts/benchmark/semantic-perf/oss/oss.test.mjs` needs no tool:
it checks the manifest's pins, provisioning's bookkeeping, the flag selection, the reading and
classification of every answer form, the catalog's pairing against its
measured reference, and each section's validation on synthetic runs (a wrong
tsc reference, a changed binary, the wrong cap, a missing record and a
tampered summary fail; a tool's wrong answer is a finding). No test asserts
a timing.

After changing the thenable catalog or the library, re-measure its reference:

```bash
node scripts/benchmark/semantic-perf/oss/measure-thenable.mjs [--allow-sampled]
```
