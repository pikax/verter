# Semantic Benchmark: Verter vs tsc 7.0.2

`scripts/benchmark/semantic-perf.mjs` compares Verter's type engine with
TypeScript 7.0.2 on **identical demands**: the same module, the same library,
the same compiler options, the same requested answer. It is built to be fair
to both tools. Future performance work is baselined on it, so every run is
validated before any number in it is read.

This page records the harness's structure and contracts, not numbers. Results
are machine-bound and belong to a run's own `results.md`.

## What it measures

Each scenario is one TypeScript module that declares two aliases:

```ts
type __BenchInit = 0;          // absorbs one-time lazy initialisation
// … the scenario's declarations …
type __Probe = <expression>;   // the demanded answer
export {};
```

The demand is **the declared type of `__Probe`**. Both arms answer it from the
same files on disk, byte for byte.

| Arm | What runs | Enters the head-to-head |
|---|---|---|
| `verter` | `semantic_perf_probe`: a release executable linking the production `verter_session` library, host built with the shipped `HostConfig::default()` (no audit, trace, footprint or metrics capture) | yes |
| `tsc-api` | TypeScript 7.0.2's native API (`typescript/unstable/sync`, driving the native `tsc --api` server; the verified executable is passed to it explicitly) | yes |
| `verter-obs` | the same probe with the host's observability bookkeeping on (audit records with timing and footprint capture, metrics) | no: shows what the bookkeeping costs |
| `verter-counted` | the probe's twin with a counting global allocator | no: allocation counts only |
| `tsc-cli` | `tsc -p --extendedDiagnostics`, default (parallel) checkers | no: whole-program reference |
| `tsc-cli-1` | `tsc -p --extendedDiagnostics --singleThreaded` | no: whole-program reference |

Verter exposes no whole-program diagnostic pass, so the `tsc -p` arms have no
Verter counterpart. They show what tsc's full check costs, in both thread
modes, beside the demanded-probe numbers, for the scenario plus
`declare const __bench_use: __Probe;`: tsc resolves an unused alias lazily, so
without a use its full check would never compute the answer the probe arms
demand (on the reversed 2,100-member relation it finishes in milliseconds and
reports nothing). They are never used as tsc's time for the demanded probe.

### Phases

Both probe arms run the same phases, in the same order, timed separately and
never overlapping:

| Phase | Verter | tsc |
|---|---|---|
| spawn | (process start is outside the probe; the supervisor's wall time covers it) | `new API()`: start the server, which constructs its sessions, and connect (client-side; reported, never compared) |
| engine start | host construction: an empty engine, ready | the API's `initialize` handshake (server time) — not comparable work: tsc constructs its engine at process start |
| setup | open the project: read the files, configure it from its tsconfig, add the library and the scenario | `updateSnapshot` (server time) |
| init | `resolve_named_symbol_with_audit(scenario, "__BenchInit")` | `getTypeAtPosition` on `__BenchInit`'s name |
| cold | the same call for `__Probe` | the same request for `__Probe` |
| warm | the cold request repeated in the same process | the same |
| engine statistics | OS memory and the host's retention counters, with the host alive and nothing observed yet | the server process's OS memory, likewise |
| observe | materialise and render every answer (outside every timer); memory again | `typeToString` (a union member by member), the error-type flag; memory again |
| teardown | drop the host | close the API (terminates the server): reported, not compared |

**first type handle** = setup + init + cold: from opening the project to
holding the demanded type's handle. Each tool splits its work between opening
a project and answering its first request in its own way (what is parsed,
bound or indexed eagerly and what lazily), so the split is not comparable
across tools, but the sum is. Engine start is reported apart and in no
headline. Both arms end the timed region holding the type's handle
(Verter's interned node in its default projection, tsc's type); printing it
is **observe**. The figure claims no fully printed answer, and equal
structural completeness before observation is not proven, only that the
observed answers are equal.

tsc's request times are its **server-side processing time** as it reports it
(the API's own `collectTiming` measurement, a timer around request handling
that excludes transport), which excludes the IPC an in-process engine does not
pay. On Windows the server's clock is coarse (it reports 0 or about half a
millisecond for short requests); the verdict's resolution is derived from the
run's own smallest observed server time rather than correcting either side.
The round trip is recorded beside every figure.

Every warm request must return the cold request's answer (the same interned
node or type, or, compared after the measurement, the same printed answer).

Each probe writes its record (atomically) as soon as the engine statistics
are read, and a phase marker naming the phase running, so an invocation
stopped during observation keeps its measured demand (classified
`unverified`, never compared).

### Memory

One reader serves both tools: `semantic_perf_probe stats --pid <pid>` reads a
process's operating-system accounting (Windows: peak and current **private
commit**, plus working set; macOS: lifetime-maximum and current **physical
footprint**, plus resident size; Linux: `VmHWM` / `VmRSS`). The Verter probe
reads its own process; the tsc driver reads the tsc server's process. The
engine figures are read with the process alive after the requests and before
anything is observed, so **peak** is the high-water mark over engine start,
setup and requests and **retained** is what the process holds after
answering, and neither includes the benchmark's observation machinery; the
figures after observation are reported apart. These are kernel-maintained
high-water marks, not polled samples. The node client that drives tsc's API
is excluded from tsc's figure, and nothing is subtracted from anything. The
validator requires both arms' figures to be the same metric.

**Engine budget and containment.** Both engines get the same memory budget
(`--mem-mb`, 8192 by default): an engine whose own peak exceeds it counts as
exhausting it, whether or not it survived. Each process tree is contained at
the budget plus an allowance (`--infra-mb`, 1024 by default) for the tree's
other members — tsc's node client and the statistics reader. A kill counts
as the **engine** exhausting a resource only with evidence: Verter's tree is
its engine alone; for tsc, the kill must fall in a phase where the node driver
is blocked in one synchronous request (so it holds what it held when the
phase began — the phase marker records that, read by the same reader) and
that memory must fit the allowance with a 64 MiB margin, so the server alone
passed the budget. Any other kill is **unattributed** (the tsc arm is then
`unverified`, never "tsc exhausts resources"), and a kill while observing is
never the engine's. After a warmup whose engine exhausted the cap, the rest of
that arm is recorded as skipped (see Schedule).

The supervisor independently reports each invocation's whole-process-tree
peak and wall time (for `tsc-api` that tree includes the node client); those
are recorded, never used for the head-to-head. tsc's own `Memory used`
counter (from `--extendedDiagnostics`) appears only in the whole-program
table, apart from OS figures.

### Counts

Relation, work and allocation counts come only from production surfaces or
from separately labelled runs:

- `VerterHost::retention_snapshot()` (production API) after the requests,
  outside every timer: interned relation proofs, semantic nodes, memo
  entries, union views and the retention account's charged bytes and
  refusals;
- the `verter-obs` arm: the production audit record of the cold request
  (hops, expansions, projection operations);
- the `verter-counted` arm: cold-request allocation count and bytes from a
  counting global allocator in a separate binary; its times are never
  compared.

No `cfg(test)` instrumentation is compiled into either probe binary.

## Workload equivalence

Why each probe demands the same work of both tools:

1. **Same source.** One `scenario.ts` per (scenario, setting) is written
   once; both arms read that file. The validator checks the tsc program's
   root files are exactly that directory's library and scenario, and that
   every input is the catalog's.
2. **Same library.** Both read `lib.bench.d.ts`, a copy of TypeScript 7.0.2's
   own declarations for the members the scenarios read. tsc reads it as a
   root file under `noLib`, so no other library is loaded; Verter upserts it
   as an ordinary `.d.ts` file of the project (`--lib-mode root-file`, the
   default), the same channel. (`--lib-mode ambient` registers it through
   Verter's ambient-library registry instead; answers are identical.)
3. **Same options.** Both read the same `tsconfig.json`: `strict` with the
   setting's `strictNullChecks` / `noImplicitAny`, `noLib`, `target es2022`.
   Verter loads it with `verter_workspace::load_compiler_options`, the loader
   production uses.
4. **Same demand.** Verter: `resolve_named_symbol_with_audit(scenario,
   "__Probe", default mode)` — the production typeinfo entry the NAPI
   `resolveSymbolWithAudit` binding calls — returns the alias's resolved
   type. tsc: `getTypeAtPosition` on the alias's name, which for a type
   declaration name is the checker's `getDeclaredTypeOfSymbol` — the
   alias's declared type. Neither request checks the rest of the file, emits
   diagnostics or relates the answer to anything else, and no whole-file
   work runs in either probe process.
5. **One demand per program.** A scenario declares exactly one probe. tsc's
   answers are order-dependent (a failed deep instantiation poisons a later,
   shallower request for the same alias family, measured on the conditional
   chain), so two probes in one program would not be two independent
   demands.
6. **Same boundaries.** Both timed regions start at an opened-project request
   on a ready engine and end holding the evaluated type; engine start,
   observation and teardown are outside them; memory is read at the same
   point (after the requests, before observation).
7. **Same answer, compared by structure.** Each tool prints its answer
   (Verter: the production TypeExpr wire bytes, rendered; tsc: `typeToString`
   with no truncation, a union member by member — each member parenthesised
   — because tsc's printer elides very large types). A parser reads both
   prints into trees, normalises them by identities of the type system, and
   compares the normalised trees themselves (see Correctness). tsc's error
   type is detected with `isErrorType()`, so tsc's error-`any` is never
   mistaken for an `any` answer.
8. **Same containment, budget and schedule.** Every invocation, of every arm,
   runs in a fresh process under the same supervisor with the same engine
   budget, allowance and deadline, in counterbalanced order (see Schedule).
9. **Same shipped defaults.** The harness refuses to run while runtime or
   build tuning is present — variables such as `GOGC`, `GOMEMLIMIT`,
   `GOMAXPROCS`, `GODEBUG`, `NODE_OPTIONS`, `RUSTFLAGS`, `RUSTC`,
   `RUSTC_WRAPPER`, `CARGO_PROFILE_*` / `CARGO_TARGET_*` / `CARGO_BUILD_*`,
   `CC` / `CFLAGS`, `RAYON_NUM_THREADS`, `VERTER_*`, or a Cargo configuration
   file outside the repository (in an ancestor directory or `CARGO_HOME`) —
   unless `--allow-tuning` labels the run tuned. The compiler is the
   toolchain's own (`rustup which rustc`, fingerprinted). The probe, node and
   the tsc package must be one architecture and that must be the hardware's
   native one (read by the probe independently of node: `IsWow64Process2` on
   Windows, `hw.optional.arm64` on macOS), so nothing runs translated.

What is deliberately **not** equal, and how it is reported:

- **Process architecture.** tsc's API is a native server driven over IPC by
  a node client; Verter's probe is one native process. Request times use
  tsc's server-side time (IPC excluded) and memory uses the tsc server
  process alone, so neither the client nor the IPC is charged to tsc; the
  containment allowance keeps the client from spending the engine budget.
- **Threads.** Neither arm is restricted. tsc's API server and Verter's host
  use the threads their shipped defaults use; each process's CPU time is
  recorded next to its wall time. The whole-program arms run tsc in both
  thread modes; the parallel mode is tsc as shipped and is never handicapped.
- **Where lazy work lands.** See Phases: the first-type sum is the comparable
  figure.
- **Teardown.** tsc's teardown terminates its server; Verter's drops a host.
  Reported, not compared.

## Correctness first

Expected answers are **measured** on tsc 7.0.2, independently of the
benchmark's tsc arm, by `scripts/benchmark/semantic-perf/measure-expected.mjs`
in all four `strictNullChecks` × `noImplicitAny` settings, and committed as
`scripts/benchmark/semantic-perf/expected.json`. Every cell carries its own
immutable provenance (the method — measuring suffix, library, verified tsc
executable, platform, limits, launcher — plus the tsconfig and source digests
and a digest of tsc's output); a cell measured by any other method is
re-measured, never relabelled, and the validator rejects a cell whose
provenance disagrees with the file's method. The measurement uses the CLI: the scenario plus

```ts
type __BenchExpand<T> = T extends unknown ? [T] : never;
declare const __bench_v: [__BenchExpand<__Probe>];
const __bench_s: [never] = __bench_v;
type __BenchIsNever = [__Probe] extends [never] ? "yes" : "no";
const __bench_n: "never-check" = null! as __BenchIsNever;
```

The distributive conditional rebuilds the probe's type as a fresh union of
one-element tuples, one per member, filtering none (every type extends
`unknown`), so the head line of the first TS2322 prints the members rather
than an alias or union origin naming them; the second assignment prints
whether the probe is `never`, the one answer the first cannot print. The
measurement records tsc's raw print; `reference.mjs` interprets it (so the
interpretation can be audited and improved without measuring again). An
output with neither verdict is an error, never an answer; a print tsc elided
(bare `any` members among the tuples) is recorded as elided; an `any` beside
TS2589 / TS2590 is tsc's error-any; a measurement tsc cannot finish inside the
cap or the deadline is recorded as `killed` ("tsc exhausts resources"), never
retried with more. Every measured tsc process is contained.

The benchmark's tsc arm must then reproduce the reference exactly — same
canonical answer, same error-type flag — or the run fails validation: either
the reference or the arm is wrong. When the measurement holds no answer
(killed, elided or unmeasurable) but the API answers the demand with the
answer the scenario constructs, that answer is the reference (labelled "by
construction"); any other API answer is `unverified` and its row is never
compared.

The canonical form parses each print with a recursive-descent parser for the
type syntax either tool prints (failing closed on anything else) and compares
the normalised trees by their serialisation — never by re-parsing rendered
text. It normalises only identities of the type system: whitespace,
separators and redundant parentheses; string literals by value and numbers by
value; union members as a set (with `true | false` as `boolean`); `T[]` as
`Array<T>`; object properties and methods keyed by their tagged name (a
written name — `0` and `"0"` are one property — or a computed key, never
equal to a string); same-named overloads, call and construct signatures keep
their order; parameter names and tuple labels are dropped, with a type
predicate's target bound to its parameter's position. Intersection order,
tuple order, argument order and modifiers are kept.

Each Verter answer is classified against the reference:

| Class | Meaning |
|---|---|
| `matched` | Verter's answer equals tsc's (tsc's answer is not an error-any) |
| `beyond-tsc` | tsc stops at an established resource limit — TS2589 / TS2590 / TS2799 / TS2859 beside its answer, or both the measuring program and the demand itself exhaust the cap — and Verter's answer equals the answer the scenario constructs; reported separately, **never counted as a speed win** |
| `mismatch` | a different answer |
| `partial` | the answer is incomplete: an unmaterialised leaf in the production wire form (the terminal projection's `unknown`), an unevaluated top-level conditional, the probe expression handed back unevaluated, or no printable answer |
| `unverified` | the demand completed but its answer was not observed (stopped while observing) |
| `refusal` | a typed budget fault (Verter's own budget) |
| `error` | any other fault, a miss, or a warm repeat that failed or answered differently |
| `killed` | the engine exhausted the containment cap or the deadline during the demand (Verter's tree is its engine alone), or its own peak exceeded the budget |
| `no-reference` | tsc gives no answer to compare with |

Only `matched` rows enter the head-to-head. A fast wrong, partial or refused
answer is never a win.

## Validation

`validate.mjs` fails a run when:

- a binary is not the one named: TypeScript is not 7.0.2 (package, platform
  package and `tsc -v`), a Verter probe was not built at `opt-level 3` without
  debug assertions, or with a non-production feature (`test-support`,
  `attribution`, …), a pinned binary or the tsc API client changed during
  the run, the tsc arm ran another executable, the probe, node and tsc are
  not one native architecture, or the probe's build inputs (`crates/`, the
  Cargo manifests, the toolchain pin) changed while it was built;
- tuning variables were set without `--allow-tuning`;
- the harness itself (`scripts/benchmark/semantic-perf*`) changed during the
  run (the tsc driver is re-read by every invocation);
- the recorded plan is not the counterbalanced schedule, is unbalanced in any
  cell, or `--repeat` is odd;
- any record is missing, duplicated, out of plan order, or a planned
  (scenario, setting, arm) has zero records;
- a child failed (non-zero exit, a supervisor error, sampled containment
  without consent, the wrong containment cap), or a completed probe record
  lacks any required field: every phase and request time present, finite and
  non-negative, one authoritative init time, a successful init request, every
  warm repeat answering the cold answer, statistics present, free of errors,
  of the engine's own process (by pid), naming a metric, current within peak,
  and peaks never decreasing;
- any measurement of either headline arm reads another memory metric, or a
  compared metric has fewer values than measured invocations;
- an arm was skipped without a warmup whose engine exhausted the cap;
- repetitions of one arm disagree on the answer or its class;
- the tsc arm's answer or error-type flag differs from the measured
  reference, or a reference cell was measured on another source, tsconfig,
  library or method, or not by a verified tsc 7.0.2;
- the stored summary differs from the summary recomputed from the raw
  records (a claimed match or verdict the raw answers do not support), or a
  raw record on disk differs from its copy in `results.json`.

A wrong, partial, refused, unverified or killed Verter answer is a
**finding**, not a validation failure; `--require-all-matched` makes anything
but `matched` one, for a run meant as a baseline.

The self-tests (`node --test scripts/benchmark/semantic-perf/semantic-perf.test.mjs`)
prove each failure condition on synthetic runs, including a deliberate wrong
answer, zero records and a failed child, plus the canonicaliser's known
counterexamples and the schedule's balance for odd and even cell counts.

## Verdicts

A verdict is a descriptive rule, not a statistical test. A metric names a
winner only when every measured repetition of one arm beats every repetition
of the other by more than the timer resolution; otherwise the verdict is
**overlap**. The resolution comes from the run's own evidence: tsc's server
clock quantum is the smallest positive server time the run observed, and the
resolution is at least 1 ms and that quantum for one request, and at least
2 ms and three quanta for first type handle (a sum of three separately timed
requests). The ratio shown is tsc's median over Verter's (above 1 favours
Verter), omitted when a median is below the resolution. Absolute numbers of
both arms are always shown; nothing is baseline-subtracted.
`baseline-empty` (a trivial module and probe) is its own row, reported alone.

## Schedule

Each (scenario, setting, arm) runs `--warmup` unmeasured invocations and
`--repeat` measured ones (even), every one a fresh process. Warmup rounds run
first. The scenario order reverses on alternate rounds; each cell's arm order
alternates from round to round (by the cell's own index, so the two reversals
never cancel), so over the measured rounds every pair of arms runs in each
order equally often in every cell — the validator recomputes the schedule and
checks this. Warmups are run, recorded and validated like any other
invocation, then excluded from the statistics. Fresh processes mean cold
semantic caches, not cold filesystem caches. For a baseline, keep the machine
otherwise idle and on stable power (on a laptop: plugged in, not in a
low-power mode); the report records the power state (`pmset` on macOS,
`powercfg` on Windows) but does not enforce it.

When a warmup's engine exhausted the memory cap (an attributed kill), the
remaining invocations of that (scenario, setting, arm) are recorded as
`skipped` rather than driving the machine to the cap again (a memory kill is
deterministic; a timeout is not, and is always re-run). The validator accepts
a skipped record only after such a kill, and the row is classified `killed`.
`--no-skip-after-kill` runs every invocation.

## Process supervision

Every invocation runs under `verter-supervise` (`crates/verter_supervise`),
which establishes the memory cap and the deadline before the child runs,
tears down the whole process tree on every exit path and fails closed
(exit 125) when containment or telemetry is lost. There is no fallback: with
no supervisor the harness refuses to run. Windows and Linux contain the tree
in the kernel (job object, cgroup v2). **macOS has no kernel-enforced
process-tree cap**: its backend samples the tree's physical footprint and
kills on a breach, so it runs only with `--allow-sampled`, and the report
labels every record `containment: sampled`.

## Running it

### Prerequisites (macOS arm64 and Windows)

1. The Rust toolchain: `rustup` (the repository's `rust-toolchain.toml` pins
   the compiler; the first `cargo` call installs it).
2. Node.js 20 or newer and pnpm 9 or newer, native to the machine (on an
   Apple-silicon Mac, an arm64 node, not one running under Rosetta).
3. From the repository root: `pnpm install`. This installs the root
   devDependency `typescript@7.0.2` and, through its optional dependencies,
   the native tsc for the platform (`@typescript/typescript-darwin-arm64` on
   an Apple-silicon Mac, `@typescript/typescript-win32-x64` on Windows). The
   harness resolves both through ordinary dependency resolution and refuses
   any version other than 7.0.2.
4. The supervisor: the harness builds `crates/verter_supervise` itself; on a
   checkout without it, pass `--supervisor <path-to-verter-supervise>`.
5. No tuning variables in the environment and no Cargo configuration outside
   the repository (see Workload equivalence, 9); `--allow-tuning` runs anyway,
   labelled tuned.

### Build (the harness also does this itself)

```bash
CARGO_INCREMENTAL=0 cargo build --release -p verter_bench \
  --bin semantic_perf_probe --bin semantic_perf_probe_counted
```

The harness always runs this build (cargo skips it when nothing changed),
checks cargo's report of the profile and features, and copies the binaries
into the output directory under content-addressed names before running them.
A first release build (fat LTO) takes several minutes.

### Run

macOS (Apple silicon):

```bash
node scripts/benchmark/semantic-perf.mjs --allow-sampled
```

Windows (or Linux):

```bash
node scripts/benchmark/semantic-perf.mjs
```

Results land in `target/semantic-perf/<timestamp>/`:

- `results.md` — the readable report (the path is printed at the end);
- `results.json` — every invocation's record, provenance, summary and
  validation;
- `runs/<scenario>/<setting>/<arm>/` — each invocation's job, supervisor
  record, stdout/stderr, phase marker and probe record;
- `scenarios/<scenario>/<setting>/` — the exact files both arms read
  (`cli/` holds the whole-program arms' program);
- `bin/` — the pinned binaries that ran.

The command exits 0 only when validation passes. Re-validate any run with

```bash
node scripts/benchmark/semantic-perf/validate.mjs target/semantic-perf/<timestamp>/results.json
```

Useful options (`--help` lists all): `--only relation,spread` (scenario id
prefixes), `--repeat 6`, `--warmup 1`, `--settings all` (all four
`strictNullChecks` × `noImplicitAny` settings), `--arms verter,tsc-api`,
`--mem-mb 8192`, `--infra-mb 1024`, `--timeout-ms 300000`, `--out <dir>`.

### Re-measuring the reference

After changing a scenario, the library or the measuring method, re-measure the
affected references (every tsc process contained; `--resume` keeps cells
already measured on the same source by the same method):

```bash
node scripts/benchmark/semantic-perf/measure-expected.mjs --only <ids>                  # under verter-supervise
node scripts/benchmark/semantic-perf/measure-expected.mjs --only <ids> --allow-sampled   # macOS
```

The run fails validation until the reference matches the scenario sources,
the library and the method.

## Not covered here

These perf-suite families need their own harness and are listed in every
report:

- **workspace concurrency** — sibling batches of 12 / 50 components importing
  `./types`: resolution operations, fs reads, restarts and single-flight
  across a workspace; a lifecycle workload with no single demanded probe;
- **whole project (`InputMenu.vue`)** — an equivalent demand for a Vue SFC
  needs matched project dependencies, libraries and projection boundaries on
  both sides; Verter's SFC projection has no tsc counterpart request;
- **frame-runtime overhead on shallow paths** — a Verter-against-Verter
  regression: `scripts/benchmark/signature-kernel-perf.mjs`;
- **recursion-detection time per shape** — a Verter budget property with no
  equivalent tsc demand.
