# Semantic Benchmark: Verter vs tsc 7.0.2

`scripts/benchmark/semantic-perf.mjs` compares Verter's type engine with
TypeScript 7.0.2 on **identical demands**: the same module, the same library,
the same compiler options, the same requested answer. It is built to be fair
to both tools. Future performance work is baselined on it, so every run is
validated before any number in it is read.

This page records the harness's structure and contracts, not numbers. Results
are machine-bound and belong to a run's own `results.md`. What a run may
claim on which machine is fixed by the [measurement rule](#measurement-rule);
named, repeatable runs will go through [evidence runs](#evidence-runs), a
planned layer.

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
| `verter-observe` | the same probe built with `--features semantic-observe` (optional semantic capture compiled in; production configuration), in its own target directory. The `verter` probe has that capture physically compiled out, and the validator requires each binary's `identity` to say so | no: shows what compiled-in capture costs, and checks both builds retain the same REQUIRED state |
| `verter-counted` | the probe's twin with a counting global allocator | no: allocation counts only |
| `tsc-cli` | `tsc -p --extendedDiagnostics`, default (parallel) checkers | no: whole-program reference |
| `tsc-cli-1` | `tsc -p --extendedDiagnostics --singleThreaded` | no: whole-program reference |

Observability is compared outside the head-to-head, in two arms. The
`verter-obs` arm turns the host's runtime bookkeeping on in the production
build. The `verter-observe` arm is the build-level comparison: the
production-default probe — built with the default-off `semantic-observe`
Cargo feature (see `docs/arch/semantic-observe.md`) physically compiled out —
paired with the same probe built with it compiled in, both builds retaining
identical required state so the pair isolates what the optional observation
layer costs (see [Running it](#running-it)).

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
millisecond for short requests); the verdict's resolution is calibrated
rather than correcting either side: after its measurement every tsc probe
times 20 trivial warm requests, and the smallest positive time among them is
the server clock's quantum (see Verdicts).
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
as the **engine** exhausting a resource only with evidence. A whole-program
arm (`tsc -p`) is one process that is the engine. A probe arm must have been
in one of its **engine phases** when it was killed — Verter: host
construction, setup, init, cold, warm; tsc: server spawn, initialize, setup,
init, cold, warm — never statistics, observation, calibration or teardown,
which are the harness's work. A supervisor error (containment lost, a tree
that would not empty) invalidates the invocation whatever else happened.

- **Memory.** Only a tree that is the engine alone — a Verter probe, a
  `tsc -p` process — proves it, and only when the supervisor recorded its
  actual kill threshold (`killTriggerBytes`: on a sampled backend such as
  macOS it is below the cap; the configured cap is never used in its place)
  and that threshold is at least the engine budget, on a backend that
  accounts the tree's own memory (Windows job object, macOS physical
  footprint; a Linux cgroup also counts page cache and kernel memory, and an
  unknown backend proves nothing). A `tsc-api` tree also holds the
  node driver, whose memory during a request is not bounded by anything the
  harness can prove, so a memory kill of that tree is **never** attributed:
  tsc exhausting memory on a demand is established by the measuring
  `tsc -p` run (see Correctness), not by the API arm.
- **Deadline.** Only a process that is the engine from its start — a
  `tsc -p` run, in the benchmark and in the reference measurement — can
  prove it: its wall time up to the moment termination began (the
  supervisor's wall time minus its recorded termination latency) must be at
  least the deadline (`--timeout-ms`). A probe arm's deadline kill is
  **never** attributed: no engine-owned clock ends at the kill, and the time
  since the engine's first phase also holds client work, IPC and marker I/O.
  It is reported (with that elapsed time) as `unverified`. The supervisor's
  deadline is the engine's deadline plus a startup allowance
  (`--startup-allowance-ms`: 10 s in quick and standard, 30 s in stress).
- The phase marker is published before each phase begins; a probe that
  cannot publish it stops before the phase, so a stale marker never
  misplaces a kill.
- Anything killed after the probe wrote its measured record (observing,
  calibrating, tearing down) is never the engine's demand.

Any other kill is **unattributed**: the arm is `unverified`, never "exhausts
resources", and never makes a Verter answer `beyond-tsc`. After a warmup
whose engine exhausted the cap, the rest of that arm is recorded as skipped
(see Schedule).

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
9. **Same shipped defaults.** Every environment the harness gives a child —
   the build, the supervisor and through it every probe and `tsc` process —
   is **constructed**, not inherited: only the variables a process needs to
   start and find its files (`PATH`, `HOME`, temporary directories, and on
   Windows the system and profile locations; the build adds `CARGO_HOME`
   and `RUSTUP_HOME`), compared case-insensitively. So no allocator, runtime
   or compiler setting of the caller's shell (`GOGC`, `GOMAXPROCS`,
   `NODE_OPTIONS`, `MALLOC_*`, `GLIBC_TUNABLES`, `LD_PRELOAD`, `DYLD_*`,
   `RUSTFLAGS`, `CARGO_PROFILE_*`, `VERTER_*`, …) reaches a measurement;
   those present in the caller's environment are recorded as ignored. The
   build additionally sets `CARGO_INCREMENTAL=0`, names the repository's
   pinned toolchain (`rust-toolchain.toml`) as `RUSTUP_TOOLCHAIN`, and binds
   `RUSTC` to that toolchain's compiler, discovered in the same constructed
   environment (so no override of the caller's selects another); the
   validator requires the recorded pin to be the repository's and the
   compiler's release to be that pin. Both environments are
   recorded (names, and a digest of the values) and the validator requires
   them to be the constructed ones. A Cargo configuration file outside the
   repository (in an ancestor directory or `CARGO_HOME`), which cargo reads
   whatever the environment, makes the harness refuse to run. `--allow-tuning`
   instead passes the caller's whole environment through and labels the run
   tuned, with the tuning it found. The probe, node and
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
retried with more. Every measured tsc process is contained, and its receipt
keeps the supervisor's termination evidence (backend, containment, cap,
actual kill threshold, deadline, wall time, termination latency): a kill is
read as tsc exhausting the resource by the same rule as a benchmark arm
(`reference.mjs`: memoryKillEvidence, deadlineKillEvidence), and otherwise
as unmeasurable. A supervisor error fails the measurement.

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
their order; tuple labels are dropped. Binders are positions in a lexical
environment: a signature binds its type parameters and parameters (a
`typeof` of a parameter and a type predicate's target refer to it), a
conditional's `infer` declarations are numbered by their first appearance
in its NORMALISED extends clause (so a property reordering the canonical form
permits does not renumber them; members that differ only in their
declarations cannot be ordered and fail closed) and are types only in their
own constraint and the true branch (as TypeScript resolves them: a plain
reference to the same name elsewhere in the extends clause, or in the false
branch, resolves outside), an index signature binds its parameter in its
value type, a mapped type binds its key; a reference — including a computed
member key's leading name — resolves to the innermost binding of its name. So consistent renaming is one type, while swapped binders, or a
bound name against the free name it shadows, are two. Intersection order,
tuple order, argument order and modifiers are kept.

Each Verter answer is classified against the reference:

| Class | Meaning |
|---|---|
| `matched` | Verter's answer equals tsc's (tsc's answer is not an error-any) |
| `beyond-tsc` | tsc stops at an established resource limit — TS2589 / TS2590 / TS2799 / TS2859 beside its answer, or both the measuring program and the demand itself exhaust the cap — and Verter's answer equals the answer the scenario constructs; reported separately, **never counted as a speed win** |
| `mismatch` | a different answer |
| `partial` | the answer is incomplete: an unmaterialised leaf in the production wire form (the terminal projection's `unknown`), an unevaluated top-level conditional, the probe expression handed back unevaluated, or no printable answer |
| `unverified` | the demand completed but its answer was not observed (stopped while observing), the observation lacks its completeness evidence, or the invocation was killed without evidence that its engine exhausted the resource. An observation is either successful (the answer and all its tool's evidence: tsc's error-type flag; Verter's completeness counts) or failed (an error); a failed observation never supplies a comparable answer, whatever text it carries |
| `refusal` | a typed budget fault (Verter's own budget) |
| `error` | any other fault, a miss, or a warm repeat that failed or answered differently |
| `killed` | the engine exhausted the containment cap during the demand, on the evidence above (Verter's tree is its engine alone), or its own peak exceeded the budget; a probe's deadline kill is `unverified` |
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
  not one native architecture (the probes' own `nativeArch` reading must
  equal their target), the build does not record its controlled environment
  (`CARGO_INCREMENTAL=0`, `RUSTC` bound to a fingerprinted compiler), or
  the probe's build inputs (`crates/`, the Cargo manifests, the toolchain pin)
  changed while it was built;
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
**overlap**. The resolution is calibrated: after its measurement, every tsc
probe times 20 trivial warm requests (`getTypeAtPosition` on
`__BenchInit`). If none reads 0 ms the clock is **fine** and the quantum is
at most the smallest calibration time (basis: calibration). If any reads
0 ms the clock is **coarse** (on Windows, readings are 0 or roughly half a
millisecond and up), and the calibration cannot bound its granularity: the
quantum is then estimated as the smallest positive tsc server time the run
produced, and the report labels that basis a **workload heuristic** — it
depends on which requests the run timed and is not a measured property of
the clock. The
resolution is at least 1 ms and two quanta (a difference of two readings)
for one request, and at least 2 ms and four quanta for first type handle (a
sum of three separately timed requests). The ratio shown is tsc's median over Verter's (above 1 favours
Verter), omitted when a median is below the resolution. Absolute numbers of
both arms are always shown; nothing is baseline-subtracted.
`baseline-empty` (a trivial module and probe) is its own row, reported alone.

## Measurement rule

Workers differ and share their machine with other work, so what a run may
claim depends on where it ran:

- **On any worker**, a run establishes **answer classes** (see Correctness)
  and **work counts and growth ratios** (relation proofs, semantic nodes,
  memo entries, hops, expansions, allocation counts, and how they grow with a
  scenario's size). These are machine-independent: the same commit gives the
  same classes and counts everywhere. They are what a change's performance
  acceptance rests on.
- **Time and memory** cells (phase and request times, first type handle,
  peaks, retained memory) are
  **measured only on the benchmark machine**: a worker tagged `bench-m3`.
  On any other worker they are reported as `not measured`, never as zero,
  never as a pass and never as a failure. A timing or memory gate exists only
  on `bench-m3`, owned by the performance work that runs there. The Capacity
  report (each arm's outcome at the engine budget, its engine peak and time,
  and for killed invocations the tree's peak at the kill and the time to it)
  is measured under this same rule.
- A comparison (a baseline commit against a candidate) measures both on the
  same worker in the same session, interleaved — never against a stored
  number from another host or another session.
- A metric, control or containment backend the worker does not provide is
  reported `unavailable`, with the reason. Unavailable or inconclusive
  performance evidence is reported, never a failure of the run.
- No test or gate asserts a timing, sleeps, or compares a wall-clock reading.
  The self-tests prove validation and classification on synthetic records
  only.

The direct harness (`semantic-perf.mjs`) still records the times and memory
it observes on any machine, labelled with that machine's provenance, for
local investigation; those figures are evidence about that machine only.
An evidence run will apply the rule to its summary's cells (see below).

## Tiers

`--tier` chooses what a run covers; every tier keeps the fairness
properties below (fresh processes for cold measurements, warm repeats in one
live process per arm, counterbalanced order, one supervisor and budget for
every arm, the same validation).

| Tier | Scenarios | Arms | Deadline | Indicative duration (one developer machine, Ryzen 9 7950X on Windows; a planning figure, not a measurement) |
|---|---|---|---|---|
| `quick` (default) | one representative normal size per scenario series (21), and the three session workloads | Verter, tsc API, and the labelled observability-on, observe-build and counting Verter arms | 60 s | not yet measured with the session workloads and the observe arm (62 s and 336 invocations before them) |
| `standard` | + the other normal sizes and the sizes at tsc's own limits (48) — the baseline | + `tsc -p` in both thread modes | 120 s | 564 s (1152 invocations) |
| `stress` (opt-in) | + Verter's limit and pathological sizes (55): tsc exhausting 8 GiB, multi-second Verter requests | all seven | 600 s | hours |

Every tier runs 1 warmup and 3 measured invocations per (scenario, arm) and
3 warm repeats; any option given explicitly (`--repeat`, `--arms`,
`--timeout-ms`, …) overrides the tier's default, and `--only` picks
scenarios from the whole catalog. The tier of each scenario is listed in
`scenarios.mjs` (`SCENARIO_TIERS`); the validator checks that a run's cells
are exactly its tier's (or `--only`'s).

## Session workloads

`sessions.mjs` holds the editor workloads. A session is a multi-file project
and an ordered script of steps, run in ONE live engine per invocation:

| Session | Family | Script |
|---|---|---|
| `incremental-edits` | INCREMENTAL | demand an alias that depends on a leaf through an intermediate generic; edit the leaf, the intermediate type and an unrelated file, re-requesting after each |
| `editor-session` | EDITOR SESSION | an InputMenu-equivalent Vue component (props composed from a picked base, events from a tuple map, both imported from a types module): hover-like demands on its types and its component metadata, cold, then across an edit to the types and an edit to the file being hovered |
| `concurrent-demands` | CONCURRENT | eight demands in eight files issued at once (one thread each for Verter; all in flight together through tsc's asynchronous API), cold, then again |

Every demand carries the answer the type system defines at that point of the
script, derived from the construction: tsc's answer must equal it (a run
where it does not fails validation) and Verter's is classified against it
exactly as a probe answer is. A `meta` step (props with their required flags,
and events) has no tsc counterpart: it is Verter-only and never compared.

The arms are `verter`, `verter-observe` and `tsc-api`, each invocation a fresh
process on its own copy of the project, under the same supervisor, budget,
deadline and counterbalanced schedule as the probes. Verter applies an edit
through its workspace and host. tsc uses its own incremental facility: the
driver (`tsc-session-probe.mjs`) writes the file and calls `updateSnapshot`
with `fileChanges.changed` naming it, so the server derives the next snapshot
from the previous one (API program reuse); the validator requires every edit
to name its file and advance the snapshot. A step is compared only when all
its answers matched in Verter and reproduced the constructed answer in tsc.
Answers are observed right after each step, outside every timer, so a
session's memory figures include observation for both tools.

Each arm's provenance is bound as the probes' is: a Verter session must have
run the pinned probe's `session` runner, a tsc session the harness's own node
(recorded with the run's host) running `tsc-session-probe.mjs` on the
invocation's own job and record paths, with the verified tsc executable and
the pinned probe that read the server's counters recorded in the record; the
statistics reading must be of the session's own process (the record's pid),
and every recorded input digest must be the catalog's — the benchmark
library's included. A session killed after its complete record was written
keeps its measurement (the kill took nothing from it); a kill inside a step
counts as the engine's only on the probes' memory-kill evidence. The observe
arm's REQUIRED-state identity is enforced: a build that retains differently
fails validation, and a session where an arm has no completed measured
invocation reports `n/a` with a warning.

## Capacity

Every cell has a Capacity row: each arm's outcome at the engine budget (its
class or status; for a completed arm its engine peak and time), and for the
invocations the supervisor killed, the tree's peak at the kill, the time to
it and how many kills are attributed to the engine. The memory cap stays the
machine's protection: the row reports, it never raises the cap.

## Schedule

Each (scenario, setting, arm) runs `--warmup` unmeasured invocations and
`--repeat` measured ones, every one a fresh process that measures the cold
path. Warm requests are measured in ONE live process per (scenario, arm) —
its first measured invocation, which answers all `--warm-repeats` requests
in-process — the same for both tools; warm verdicts compare those in-process
samples. Warmup rounds run first. The scenario order reverses on alternate
rounds; each cell's arm order alternates from round to round (by the cell's
own index, so the two reversals never cancel), so over the measured rounds
every pair of arms runs in each order equally often in every cell — exactly
for an even count; for an odd count, within one per cell, and since cells
alternate which arm leads the extra round, within one over the whole run.
The validator recomputes the schedule and checks this. Warmups are run, recorded and validated like any other
invocation, then excluded from the statistics. Fresh processes mean cold
semantic caches, not cold filesystem caches. For a baseline, keep the machine
otherwise idle and on stable power (on a laptop: plugged in, not in a
low-power mode); the report records the power state (`pmset` on macOS,
`powercfg` on Windows) at the start and at the end of the run, but does not
enforce it.

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

macOS (Apple silicon) — the baseline is the standard tier; quick is a
one-minute check:

```bash
node scripts/benchmark/semantic-perf.mjs --tier standard --allow-sampled
node scripts/benchmark/semantic-perf.mjs --allow-sampled            # quick
```

Windows (or Linux):

```bash
node scripts/benchmark/semantic-perf.mjs --tier standard
node scripts/benchmark/semantic-perf.mjs                            # quick
```

`--tier stress` adds the Verter-limit and pathological sizes (hours).

Results land in `target/semantic-perf/<timestamp>/`:

- `results.md` — the readable report (the path is printed at the end);
- `results.json` — every invocation's record, provenance, summary and
  validation;
- `runs/<scenario>/<setting>/<arm>/` — each invocation's job, supervisor
  record, stdout/stderr, phase marker and probe record;
- `scenarios/<scenario>/<setting>/` — the exact files both arms read
  (`cli/` holds the whole-program arms' program);
- `bin/` — the pinned binaries that ran.

CLI invocations must use the verified tsc executable, the cell's project, and
the arm's exact thread mode. Runtime and build environment receipts require
64-character hexadecimal SHA-256 digests. Whole-program OS memory peaks are
attributed to the engine only for the Windows job commit-charge or macOS
physical-footprint metric paired with its owning backend. Cgroup peaks remain
raw containment telemetry; the summary and report mark engine memory unavailable.

The command exits 0 only when validation passes. Standalone validation
re-reads every raw record — the supervisor's, and for every probe invocation
its own probe record and phase marker at the path derived from the
invocation, whether or not `results.json` embeds them, and for every
whole-program run the complete stdout file the supervisor recorded — including
middle diagnostics above 1 MiB — and fails on any that differs from
`results.json` (a record on disk that `results.json` omits included). Re-validate any
run with

```bash
node scripts/benchmark/semantic-perf/validate.mjs target/semantic-perf/<timestamp>/results.json
```

Useful options (`--help` lists all, with each tier's time): `--tier quick|standard|stress`,
`--only relation,spread` (scenario ids or prefixes, from the whole catalog), `--repeat 4`, `--warmup 1`, `--settings all` (all four
`strictNullChecks` × `noImplicitAny` settings), `--arms verter,tsc-api`,
`--mem-mb 8192`, `--infra-mb 1024`, `--timeout-ms 60000` (tier default),
`--startup-allowance-ms 10000` (tier default), `--out <dir>`.

`--oss [tools]` and `--biome` add opt-in comparisons with pinned open-source
tools, each in its own section: see
[Open-Source Comparisons](./semantic-benchmark-oss.md).
`--no-demand` skips this demand section (Verter vs the tsc API) and runs only
the other selected sections; `--only-oss [tools]` and `--only-biome` are
`--oss` / `--biome` with it. `--no-tsc` runs no tsc process in any section (the Verter arms stay, classified
against the measured reference, with nothing compared); an explicit
`--arms` naming a tsc arm beside it is refused.

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

## Evidence runs

An evidence run is a **named, frozen** benchmark invocation: a manifest fixes
what is run and which cells must come back, so the same run can be repeated
on the benchmark machine and its answer attached to the work that asked for
it.

The evidence-run layer **is planned and not yet in this repository**: no
runner, self-tests, fixtures or shipped manifests exist yet, and the
harness's entry point today is `semantic-perf.mjs` (see [Running
it](#running-it)). This section is the contract that layer will implement
when it lands: a thin wrapper that validates the manifest, drives
`semantic-perf.mjs` with the manifest's options, validates the result with
the same `validate.mjs`, and writes one machine-readable summary. It adds no
second harness, classifier or validator. Its CLI will be:

```bash
node scripts/benchmark/evidence-run.mjs --run <name> --dry-run   # validate the manifest, emit a summary skeleton
node scripts/benchmark/evidence-run.mjs --run <name>             # execute it
```

`--dry-run` will run nothing: it loads and validates the manifest and writes
the summary skeleton — every required cell present, none measured — on any
worker. Its self-tests (`scripts/benchmark/evidence-run.test.mjs`, with
planted manifests under `scripts/benchmark/evidence-run-fixtures/`) will
join `pnpm test:scripts`.

### Manifests

A manifest will be `scripts/benchmark/evidence-runs/<name>.json`. Each run
name has exactly one owner: the change that needs the run adds its manifest,
and no other change edits it. The first shipped manifest will be
`skr-perf0-structural`, the structural baseline run.

| Field | Meaning |
|---|---|
| `name` | the run's name; must equal the file name without `.json`. Two manifests with one name are rejected |
| `description` | what the run is for, in one sentence |
| `tier` | `quick`, `standard` or `stress` (see Tiers): the default scenario set, deadline and repetition counts |
| `scenarios` | scenario ids or prefixes from the catalog (`scenarios.mjs`), as `--only` takes them; omitted means the tier's set |
| `settings` | `strictNullChecks` × `noImplicitAny` setting ids (`strict`, `snc-off`, `nia-off`, `both-off`); omitted means the harness default |
| `arms` | arm ids from the harness's arm registry (`ARMS` in `semantic-perf/analyze.mjs`); an id the registry does not define is rejected |
| `modes` | the workload modes measured (the harness's cold and warm demands); a mode the harness does not define is rejected |
| `threads` | the thread counts each threaded arm runs at |
| `repeat`, `warmup`, `warmRepeats` | measured invocations, unmeasured warmups and in-process warm repeats per cell; omitted means the tier's defaults. `repeat` takes any count from 2 up, like `--repeat`: even counts balance arm order exactly, odd counts — the tier default is 3 — within one (see Schedule) |
| `noise` | a reference to the noise methodology the run's figures are read under: a link to a section of this page (normally [Verdicts](#verdicts) and [Schedule](#schedule)) |
| `requiredCells` | the cells the summary must contain: each names its scenario, setting, arm, mode, thread count and the metrics it must carry |

The loader will reject a manifest that is not valid JSON, carries an unknown
field, misses a required one, names anything the harness does not define, or
lists a required cell outside the run's own scenarios, settings, arms, modes
and threads. A run whose result lacks a required cell, or whose validation
fails (an invalid answer never counts as a win), fails.

### Summary

Each run (and each dry run) will write one JSON summary into its output
directory, beside the harness's `results.json`. It will hold:

- `run`: the manifest name and the digest of the manifest's bytes;
- `dryRun`: whether anything executed;
- `worker`: the machine identity the Tama evidence job recorded for the
  worker that ran it, its tags, and whether it is `bench-m3`. A run without
  that record is treated as not `bench-m3`;
- `provenance`: the harness's provenance (binaries, toolchain, environment
  digests, TypeScript version, containment backend), as in `results.json`;
- `prerequisites`: every prerequisite the run needs (supervisor, containment
  backend, toolchain, the tsc package), each `met` or `unavailable` with its
  reason;
- `cells`: one entry per required cell, with its answer class (see
  Correctness), its work counts, and each metric as `measured` (with its
  values), `not measured` (time and memory off `bench-m3`, and every metric of
  a dry run) or `unavailable` (with the reason);
- `validation`: the harness validation's verdict and problems.

A Tama evidence job will run the manifest and attach this summary to the task
that requested it. The summary is the run's evidence; the raw records stay in
the output directory for re-validation.

### The bench-m3 rule

Time and memory cells are evaluated only when the run executes on a worker
tagged `bench-m3`. On any other worker a real run still executes every arm,
classifies every answer and records the work counts, and reports its time and
memory cells `not measured` — never zero and never a pass. Answer classes and
work-growth ratios from any worker are valid evidence (see
[Measurement rule](#measurement-rule)).

## Not covered here

These perf-suite families need their own harness and are listed in every
report:

- **workspace concurrency at batch scale** — the concurrent session measures
  demands across files in one engine; sibling batches of 12 / 50 components
  with fs-read, restart and single-flight counts are a workspace lifecycle
  workload;
- **a real component library project** — the editor session uses an
  InputMenu-equivalent component built from local sources; a real library's
  dependency graph is an external corpus, outside the hermetic catalog;
- **frame-runtime overhead on shallow paths** — a Verter-against-Verter
  regression: `scripts/benchmark/signature-kernel-perf.mjs`;
- **recursion-detection time per shape** — a Verter budget property with no
  equivalent tsc demand.
