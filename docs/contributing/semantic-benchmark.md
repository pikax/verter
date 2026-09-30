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
| `tsc-api` | TypeScript 7.0.2's native API (`typescript/unstable/sync`, driving the native `tsc --api` server) | yes |
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

Both probe arms time the same phases, separately, never overlapping:

| Phase | Verter | tsc |
|---|---|---|
| spawn | (process start is outside the probe; the supervisor's wall time covers it) | `new API()`: start the server, connect |
| setup | read the files, build workspace and host, configure the project, upsert library and scenario | `updateSnapshot` (open the project), server-side time; the round trip is recorded beside it |
| init | `resolve_named_symbol_with_audit(scenario, "__BenchInit")` | `getTypeAtPosition` on `__BenchInit`'s name |
| cold | the same call for `__Probe` | the same request for `__Probe` |
| observe | materialise and render the answer (outside every timer) | `typeToString`, error-type flag, union members (outside every timer) |
| warm | the cold request repeated in the same process | the same |
| teardown | drop the host | close the API (terminates the server): reported, not compared |

**first answer** = setup + init + cold. Each tool splits its work between
opening a project and answering its first request in its own way (what is
parsed, bound or indexed eagerly and what lazily), so the split between setup,
init and cold is not comparable across tools, but their sum is: it is the
time from opening the project to the delivered answer.
The report shows the split and the sum, and the verdict on the sum is the
robust one.

tsc's request times are its **server-side processing time** (the API's own
`collectTiming` measurement), which excludes the IPC round trip an in-process
engine does not pay; the round trip is reported beside it. On Windows the
server's clock is coarse (about half a millisecond), so sub-millisecond tsc
figures are not claimed as differences (see Verdicts).

### Memory

One reader serves both tools: `semantic_perf_probe stats --pid <pid>` reads a
process's operating-system accounting (Windows: peak and current **private
commit**, plus working set; macOS: lifetime-maximum and current **physical
footprint**, plus resident size; Linux: `VmHWM` / `VmRSS`). The Verter probe
reads its own process; the tsc driver reads the tsc server's process. Both
readings are taken with the process alive after the requests, so **peak** is
the high-water mark over setup and requests and **retained** is what the
process still holds after answering. These are kernel-maintained high-water
marks, not polled samples. The node client that drives tsc's API is excluded
from tsc's figure (its own peak is recorded separately), and nothing is
subtracted from anything.

The supervisor independently reports each invocation's whole-process-tree
peak and wall time (for `tsc-api` that tree includes the node client); those
are recorded and shown, never used for the head-to-head. tsc's own
`Memory used` counter (from `--extendedDiagnostics`) appears only in the
whole-program table, apart from OS figures.

### Counts

Relation, work and allocation counts come only from production surfaces or
from separately labelled runs:

- `VerterHost::retention_snapshot()` (production API) after the requests,
  outside every timer: interned relation proofs, semantic nodes, memo
  entries, union views and the retention account's charged bytes and
  refusals;
- the `verter-counted` arm: cold-request allocation count and bytes from a
  counting global allocator in a separate binary; its times are never
  compared.

No `cfg(test)` instrumentation is compiled into either probe binary.

## Workload equivalence

Why each probe demands the same work of both tools:

1. **Same source.** One `scenario.ts` per (scenario, setting) is written
   once; both arms read that file. The validator checks the tsc program's
   root files are exactly that directory's library and scenario.
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
   diagnostics or relates the answer to anything else.
5. **One demand per program.** A scenario declares exactly one probe. tsc's
   answers are order-dependent (a failed deep instantiation poisons a later,
   shallower request for the same alias family, measured on the conditional
   chain), so two probes in one program would not be two independent
   demands.
6. **Same answer, observed outside the timers.** Each tool prints its answer
   (Verter: the production TypeExpr wire bytes, rendered;
   tsc: `typeToString` with no truncation, without the alias name). A
   canonicaliser makes the prints comparable (quoting, union and object
   member order, `T[]` vs `Array<T>`, `boolean` vs `true | false`) without
   evaluating either. tsc's error type is detected with `isErrorType()`, so
   tsc's error-`any` is never mistaken for an `any` answer.
7. **Same containment and schedule.** Every invocation, of every arm, runs in
   a fresh process under the same supervisor with the same memory cap and
   deadline; the order is counterbalanced (see Schedule).

What is deliberately **not** equal, and how it is reported:

- **Process architecture.** tsc's API is a native server driven over IPC by
  a node client; Verter's probe is one native process. Request times use
  tsc's server-side time (IPC excluded) and memory uses the tsc server
  process alone, so neither the client nor the IPC is charged to tsc. The
  invocation's whole-tree peak and wall time, which do include the node
  client, are shown separately and never compared.
- **Threads.** Neither arm is restricted. tsc's API server and Verter's host
  use the threads their shipped defaults use; each process's CPU time is
  recorded next to its wall time. The whole-program arms run tsc in both
  thread modes; the parallel mode is tsc as shipped and is never handicapped.
- **Where lazy work lands.** See Phases: the first-answer sum is the
  comparable figure.
- **Teardown.** tsc's teardown terminates its server; Verter's drops a host.
  Reported, not compared.

## Correctness first

Expected answers are **measured** on tsc 7.0.2, independently of the
benchmark's tsc arm, by `scripts/benchmark/semantic-perf/measure-expected.mjs`
in all four `strictNullChecks` × `noImplicitAny` settings, and committed as
`scripts/benchmark/semantic-perf/expected.json`. The measurement uses the CLI:
the scenario plus

```ts
type __BenchExpand<T> = T extends __BenchNothing ? never : [T];
interface __BenchNothing { readonly __benchNothing: 1 }
declare const __bench_v: [__BenchExpand<__Probe>];
const __bench_s: [never] = __bench_v;
type __BenchIsNever = [__Probe] extends [never] ? "yes" : "no";
const __bench_n: "never-check" = null! as __BenchIsNever;
```

The head line of the first TS2322 prints the probe's members (rebuilt as a
fresh union so no alias or union origin names them); the second assignment
prints whether the probe is `never`, the one answer the first cannot print. An
output with neither verdict is an error, never an answer. An `any` beside
TS2589 / TS2590 is tsc's error-any. A measurement tsc cannot finish inside
the cap or the deadline is recorded as `killed` ("tsc exhausts resources on
this input"), never retried with more.

Every measured tsc process is contained (the supervisor, or a capped tsc
wrapper). The benchmark's tsc arm must then reproduce the reference exactly —
same canonical answer, same error-type flag — or the run fails validation:
either the reference or the arm is wrong.

The measuring `tsc -p` checks the whole file, so it can exhaust the cap on a
program whose single demanded probe the API still answers. In that case the
API's answer becomes the reference only when it equals the answer the
scenario constructs (labelled "by construction" in the report); any other API
answer is `unverified` and its row is never compared. An API arm killed at
the cap is a valid observation ("tsc exhausts resources").

Each Verter answer is classified against the reference:

| Class | Meaning |
|---|---|
| `matched` | Verter's answer equals tsc's (tsc's answer is not an error-any) |
| `beyond-tsc` | tsc stops at a resource limit (TS2589 / TS2590 / TS2859, or it exhausts the cap) and Verter's answer equals the answer the scenario constructs; reported separately, **never counted as a speed win** |
| `mismatch` | a different answer |
| `partial` | the answer is incomplete: an unmaterialised leaf in the production wire form (the terminal projection's `unknown`), an unevaluated top-level conditional, the probe expression handed back unevaluated, or no printable answer |
| `refusal` | a typed budget fault (Verter's own budget) |
| `error` | any other fault, a miss, or a failed warm repeat |
| `killed` | the supervisor killed the process at the cap or the deadline |
| `no-reference` | tsc gives no answer and the scenario constructs none |

Only `matched` rows enter the head-to-head. A fast wrong, partial or refused
answer is never a win.

## Validation

`validate.mjs` fails a run when:

- a binary is not the one named: TypeScript is not 7.0.2 (package, platform
  package and `tsc -v`), a Verter probe was not built at `opt-level 3` without
  debug assertions, or with a non-production feature (`test-support`,
  `attribution`, …), a pinned binary's sha256 changed during the run, or the
  probe's build inputs (`crates/`, `Cargo.toml`, `Cargo.lock`, the toolchain
  pin) changed while it was built;
- the harness itself (`scripts/benchmark/semantic-perf*`) changed during the
  run (the tsc driver is re-read by every invocation);
- any record is missing, duplicated, out of plan order, or a planned
  (scenario, setting, arm) has zero records;
- a child failed: non-zero exit, no or malformed probe record, missing
  process statistics, a supervisor error, or sampled containment without
  consent;
- repetitions of one arm disagree on the answer or its class;
- the tsc arm's answer or error-type flag differs from the measured reference;
- the stored summary differs from the summary recomputed from the raw
  records (a claimed match or verdict the raw answers do not support), or a
  raw record on disk differs from its copy in `results.json`.

A wrong, partial, refused or killed Verter answer is a **finding**, not a
validation failure; `--require-all-matched` makes it one, for a run meant as
a baseline.

The self-tests (`node --test scripts/benchmark/semantic-perf/semantic-perf.test.mjs`)
prove each failure condition on synthetic runs, including a deliberate wrong
answer, zero records and a failed child.

## Verdicts

A metric names a winner only when every measured repetition of one arm beats
every repetition of the other, by more than 1 ms for times; otherwise the
verdict is **overlap**. The ratio shown is tsc's median over Verter's (above 1
favours Verter). Absolute numbers of both arms are always shown; nothing is
baseline-subtracted. `baseline-empty` (a trivial module and probe) is its own
row, reported alone.

## Schedule

Each (scenario, setting, arm) runs `--warmup` unmeasured invocations and
`--repeat` measured ones, every one a fresh process. Warmup rounds run first.
Rounds alternate direction: the scenario order reverses on every other round,
and the arm order reverses on every other (round, scenario) pair, so with an
even `--repeat` each arm runs first in exactly half of its measured
invocations. Warmups are run, recorded and validated like any other
invocation, then excluded from the statistics. Fresh processes mean cold
semantic caches, not cold filesystem caches.

When a warmup is killed at the memory cap, the remaining invocations of that
(scenario, setting, arm) are recorded as `skipped` rather than driving the
machine to the cap again (a memory kill is deterministic; a timeout is not,
and is always re-run). The validator accepts a skipped record only after such
a kill, and the row is classified `killed`. `--no-skip-after-kill` runs every
invocation.

## Process supervision

Every invocation runs under `verter-supervise` (`crates/verter_supervise`),
which establishes the memory cap and the deadline before the child runs,
tears down the whole process tree on every exit path and fails closed
(exit 125) when containment or telemetry is lost. There is no fallback: with
no supervisor the harness refuses to run. Windows and Linux contain the tree
in the kernel (job object, cgroup v2). **macOS has no kernel-enforced
process-tree cap**: its backend samples the tree's physical footprint and
kills on a breach, so it runs only with `--allow-sampled`, and every record
says `containment: sampled`.

## Running it

### Prerequisites (macOS arm64 and Windows)

1. The Rust toolchain: `rustup` (the repository's `rust-toolchain.toml` pins
   the compiler; the first `cargo` call installs it).
2. Node.js 20 or newer and pnpm 9 or newer.
3. From the repository root: `pnpm install`. This installs the root
   devDependency `typescript@7.0.2` and, through its optional dependencies,
   the native tsc for the platform (`@typescript/typescript-darwin-arm64` on
   an Apple-silicon Mac, `@typescript/typescript-win32-x64` on Windows). The
   harness resolves both through ordinary dependency resolution and refuses
   any version other than 7.0.2.
4. The supervisor: the harness builds `crates/verter_supervise` itself when
   the checkout has it; otherwise pass `--supervisor <path-to-verter-supervise>`.

### Build (the harness also does this itself)

```bash
CARGO_INCREMENTAL=0 cargo build --release -p verter_bench \
  --bin semantic_perf_probe --bin semantic_perf_probe_counted
```

The harness always runs this build (cargo skips it when nothing changed),
checks cargo's report of the profile and features, and copies the binaries
into the output directory under content-addressed names before running them.

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
  record, stdout/stderr and probe record;
- `scenarios/<scenario>/<setting>/` — the exact files both arms read;
- `bin/` — the pinned binaries that ran.

The command exits 0 only when validation passes. Re-validate any run with

```bash
node scripts/benchmark/semantic-perf/validate.mjs target/semantic-perf/<timestamp>/results.json
```

Useful options (`--help` lists all): `--only relation,spread` (scenario id
prefixes), `--repeat 6`, `--warmup 1`, `--settings all` (all four
`strictNullChecks` × `noImplicitAny` settings), `--arms verter,tsc-api`,
`--mem-mb 8192`, `--timeout-ms 300000`, `--out <dir>`.

### Re-measuring the reference

After changing a scenario or the library, re-measure its reference (every tsc
process contained):

```bash
node scripts/benchmark/semantic-perf/measure-expected.mjs --only <ids>          # under verter-supervise
node scripts/benchmark/semantic-perf/measure-expected.mjs --only <ids> --allow-sampled   # macOS
```

The run fails validation until the reference matches the scenario sources.

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
