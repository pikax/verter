# oxc's parser on deeply nested input

A dependency limitation, recorded with a standalone reproducer. Nothing
here is filed upstream.

## What fails

`oxc_parser` 0.151.0 (`oxc_parser::Parser::parse`) is a recursive descent
with no depth limit of its own. It spends native stack per level of
syntactic nesting, and when the thread's stack runs out the process aborts
(`thread '…' has overflowed its stack`, exit code 0xC00000FD on Windows)
before any AST is returned. Nothing of Verter's runs: the reproducer calls
`oxc_parser::Parser::new(…).parse()` and nothing else. oxc's walks over the
tree it returns (`CloneIn::clone_in`, `oxc_semantic::SemanticBuilder`, the
`oxc_ast_visit` walkers) recurse per level the same way.

## Reproducer

`crates/verter_parser/examples/oxc_deep_parse.rs` builds one nested
TypeScript source and parses it on a thread of the given stack:

```text
cargo run -p verter_parser --example oxc_deep_parse [--release] -- <form> <depth> <stack-MiB>
```

For example `oxc_deep_parse parentheses 6000 8` aborts with
`thread 'oxc-parse' has overflowed its stack` on an optimized build;
`oxc_deep_parse parentheses 200000 2048` parses (1 statement, 0 errors).

A stack of 0 parses on the calling thread, which is how the same example
runs as a WebAssembly module (no threads) under Node.js's WASI host:

```text
cargo build -p verter_parser --example oxc_deep_parse --release --target wasm32-wasip1
node [--stack-size=<KiB>] crates/verter_parser/examples/oxc_deep_parse_wasi.mjs <form> <depth>
```

`oxc_deep_parse_wasi.mjs parentheses 900` parses; at 1,000 the host throws
`RangeError: Maximum call stack size exceeded` (Node.js 26, default stack).

`crates/verter_semantic/examples/oxc_deep_walk.rs` parses the same sources
on a stack the parse cannot exhaust, then runs one of oxc's walks over the
program on a thread of the given stack:

```text
cargo run -p verter_semantic --example oxc_deep_walk [--release] -- <clone|visit|semantic> <form> <depth> <stack-MiB>
```

## Measured

Platform: Windows 11 Pro 10.0.26200, x86_64-pc-windows-msvc,
rustc 1.97.1, oxc 0.151.0. The deepest depth that parses, by bisection
(within 1%):

| form (per level) | release, 1 MiB | release, 8 MiB | debug, 1 MiB | debug, 8 MiB |
|---|---|---|---|---|
| parentheses `(…)` | 496 | 4,093 | 343 | 2,859 |
| logical not `!` | 9,000 | 74,500 | 4,812 | 40,000 |
| type argument `Box<…>` | 577 | 4,781 | 319 | 2,671 |
| conditional `b ? 1 : …` | 968 | 8,000 | 1,890 | 15,750 |
| array `[…]` | 496 | 4,093 | 378 | 3,156 |
| object `{ v: … }` | 425 | 3,500 | 298 | 2,484 |
| arrow `() => …` | 882 | 7,312 | 1,273 | 10,625 |
| template hole `` `${…}` `` | 433 | 3,593 | 378 | 3,156 |
| `keyof …` | 1,843 | 15,250 | 2,968 | 24,750 |
| block `{ … }` | 1,577 | 13,000 | 1,250 | 10,375 |

So a level costs oxc's parser up to 2.4 KiB optimized and 3.4 KiB
unoptimized (an object literal; 0.126 spent 1.9 and 3.7 KiB). With a 2 GiB
stack the parentheses, type-argument and object forms parse at 10,000 and
200,000 levels. TypeScript 7.0.2 checks each of these forms at 10,000
levels, and the parentheses, type-argument and `!` forms at 50,000 and
200,000.

oxc's walks, the deepest depth each finishes on a 1 MiB thread:

| form | clone, release | clone, debug | semantic, release | semantic, debug | visit, debug |
|---|---|---|---|---|---|
| parentheses | 1,376 | 684 | 9,024 | 2,176 | 4,512 |
| `!` | 1,376 | 684 | 9,024 | 2,416 | 4,512 |
| `Box<…>` | 1,920 | 238 | 5,280 | 1,128 | 2,112 |
| conditional | 1,376 | 552 | 9,024 | 2,416 | 4,512 |
| array | 1,376 | 372 | 9,024 | 1,432 | 2,176 |
| object | 1,192 | 236 | 9,024 | 952 | 1,760 |
| arrow | 1,376 | 504 | 9,024 | 1,792 | 2,752 |
| template hole | 1,376 | 348 | 9,024 | 1,696 | 2,880 |
| `keyof` | 2,336 | 680 | 7,936 | 2,160 | 4,512 |
| block | 2,256 | 388 | 4,224 | 1,376 | 2,880 |

`clone_in` is the costliest walk, 4.4 KiB a level unoptimized (an object
literal or a type argument); the semantic builder spends up to 1.1 KiB and
a `Visit` walk 0.6 KiB. An optimized `Visit` walk of most forms reaches
millions of levels (its frames fold into loops); its array, object,
template and block forms stop between 9,024 and 12,672.

### WebAssembly

A WebAssembly module recurses on two stacks: its shadow stack in linear
memory (1 MiB, wasm32's linker default, placed first in memory so an
overflow traps with `memory access out of bounds`), and the engine's own
call stack, which the module cannot see or grow: past it V8 throws
`RangeError: Maximum call stack size exceeded`. Either unwinds out of the
module without restoring its shadow stack pointer, so the instance is not
fit for another call. The deepest depth that parses and walks, by
bisection, on the optimized wasm32-unknown-unknown module (oxc_parser 0.151.0,
release profile) under Node.js 26.5.0 (V8 14.6.202.34-node.24)
(the shadow stack grown per parse, so the engine stack binds), at V8's
default engine stack (984 KiB) and at `--stack-size=4000`:

| form (per level) | parse, 984 | parse, 4000 | clone, 984 | visit, 984 | semantic, 984 | bytes a level |
|---|---|---|---|---|---|---|
| parentheses `(…)` | 964 | 3,952 | 964 | 964 | 964 | 1,034 |
| logical not `!` | 9,568 | 39,168 | 5,424 | 9,568 | 9,568 | 104 |
| type argument `Box<…>` | 964 | 3,952 | 964 | 964 | 964 | 1,034 |
| conditional `b ? 1 : …` | 3,360 | 13,760 | 3,360 | 3,360 | 3,360 | 297 |
| array `[…]` | 856 | 3,520 | 856 | 860 | 860 | 1,159 |
| object `{ v: … }` | 840 | 3,440 | 840 | 840 | 840 | 1,188 |
| arrow `() => …` | 2,488 | 10,208 | 2,488 | 2,488 | 2,488 | 400 |
| template hole `` `${…}` `` | 782 | 3,200 | 782 | 782 | 782 | 1,277 (two scan levels) |
| `keyof …` | 4,976 | 20,416 | 4,976 | 4,976 | 4,976 | 200 |
| block `{ … }` | 2,600 | 10,624 | 2,600 | 2,592 | 2,600 | 385 |
| member `1..a.a…` | parses in a loop | | 3,360 | 10,400 | 5,664 | 297 (clone) |

"Bytes a level" is the engine stack a level adds, from the two stack
sizes. Through the module Verter ships (`verter_wasm`, `VerterHost.upsert`
then `getAnalysis` on a `<script setup lang="ts">` holding the form) the
limits at 984 KiB are 1,016 parentheses, 964 type arguments, 880 object
literals, 808 template holes and 2,184 blocks: an object literal's level
takes 1,177 bytes there, and the module's callers of the parse take under
16 KiB.

## Containment in Verter

Every production parse goes through `verter_parser::oxc_parse::Parser`, a
drop-in for `oxc_parser::Parser` that parses exactly what oxc parses. It
runs the parse on a stack the source cannot exhaust: a linear scan
(`oxc_parse/nesting.rs`) bounds the syntax tree's depth from above, and the
parse gets 9 KiB per level of that bound (twice the costliest measured
level, `clone_in`'s) plus 512 KiB. It runs in place when the thread has
that much left (every source of ordinary depth, and every source short
enough that each byte could be a level) and on a stack region reserved
for it otherwise (`oxc_parse/stack.rs`). Where a stack can grow no depth
is imposed. The scan costs about a third of the parse (1.8 ms against
5.8 ms for `lib.dom.d.ts`'s 2.3 MB, 22 ms against 60 ms for
`typescript.js`'s 9.1 MB, optimized). `no_crate_parses_around_the_guard`
fails on any direct `oxc_parser::Parser` in a crate's sources.

The scan is an upper bound only if it reads every token where oxc does. An
audit found syntax it counted as no level and text it skipped past where
the lexical grammar ends it: a numeric literal swallowing the member
accesses after it (`1..a.a…`), braceless statement bodies (`if (a) if (a)
…`, labels, `do`, an `else` after a `;`), a function's body read as the
type syntax of its return annotation, commas between nested type
arguments in an expression, JSX member names, type arguments and
attribute elements, a `/` in a regular expression's class, a CRLF line
continuation, line comments ended by CR or U+2028, HTML-like comments,
non-ASCII spaces, and divisions read as regular expressions (after a
keyword used as a property name, an identifier spelled like a type
operator, an object literal). Each, 10,000 deep, overflowed a 1 MiB
thread; `oxc_parse/tests.rs` →
`every_form_nested_past_the_brackets_parses_and_walks_on_a_small_stack`
parses and walks each in a child process. Where the previous token cannot
decide between a division and a regular expression (`await`, `yield` and
`of` may be identifiers; a function or class body the scan reads as an
expression's may be a declaration's), the rest of the source is bounded
by its length, a level per byte.

A region reserves its size and commits only what the work on it
touches: on Windows a fiber made with `CreateFiberEx`, committing 64 KiB
and reserving the rest (`CreateFiber`, which `stacker` used, commits the
whole size: a 1,000,000-level bound committed 9,234,591,744 bytes for a
walk a few frames deep, 86–94 KB now,
`a_grown_stack_commits_what_the_walk_touches_not_what_it_reserves`); on
Unix an anonymous `MAP_NORESERVE` mapping; on wasm32 a heap allocation for
the module's shadow stack (`oxc_parse/stack.rs`).

Reserving is the only step that can fail, and it fails as
`StackUnavailable`, never a panic. A parse whose region cannot be
reserved returns an empty program marked `fatal_error` whose one
diagnostic is `verter(stack-unavailable)` (`stack_unavailable_diagnostic`,
`is_stack_unavailable`): typed operational incompleteness, not a syntax
error and not a program to read. An operation pays for its walks' stack
once, at its boundary: `with_program_walk_stack_lease` reserves the region
the program's walks can need before any begins and returns
`StackUnavailable` without running the operation when it cannot; every
walk inside the lease that needs no more runs on the lease's region,
switched to without reserving (a Windows fiber serves each walk in turn),
or in place when it already runs there. Leases, regions and the thread's
stack limit are restored on return and on unwind. `oxc_parse::faults`
(`cfg(test)` or the `stack-fault-injection` feature) fails and counts
reservations, and `oxc_parse/tests.rs` proves on it: a refused parse is
typed and a retry parses; a refused lease starts no walk and a retry
walks; a lease covers a whole-program walk, a statement walk, a walk by
text and nested walks with one reservation (five without it); a panic
on a lease region restores the stack and the lease; a covered walk reached
on a region reserved past the lease does not re-enter the lease's region
under its own suspended work.

#### How a refusal is tested

A reservation only happens when the thread's stack is too small for the
work, so a test that injects the fault has to make one happen — and the
thread's real stack is not the test's to decide. glibc serves a thread
that asked for 1 MiB a cached stack up to four times the size, and a
runner's own thread is larger still, so a fixture sized from the size
asked for parses in place, reserves nothing, and the fault never fires:
the refusal tests flaked in Compiler Contracts on 2026-09-29 and
2026-10-02, and sizing the fixtures from the stack the thread turned out
to have is the same dependence wearing a different hat.

`faults::force_reservations(&[purpose], needed)` removes it. The forcing is
test-only and restores itself on drop (and on unwind), like the cross-thread
fault it sits beside; the work of a forced purpose skips the parse length
shortcut, the thread's own stack, and the walk-stack lease — one holding a
region included, for that lease's region would run the work with no
reservation of its own for a fault to refuse — so a parse or a walk of that
purpose reserves a region wherever it runs, a scheduler worker's included.

Reaching a scheduler worker is why the forcing is not scoped to a thread: no
thread-scoped forcing reaches work the test did not start. It is scoped to
the *reservation* instead — the purposes it names, and the bytes it names
beside them. A forced reservation is sized from the syntax scan, so `needed`
is the size of the test's own source's scan
(`parse_stack_bytes`), and the forcing leaves every other size to the
thread's own stack, as in production: a test holding a forcing changes no
other test's walk or parse, whatever stack either runs on. The tests in one
binary that force a purpose therefore give it sizes no other of them parses
— a distinct nesting depth is a distinct size. Work that runs on the thread
the test itself established takes `faults::force_reservations_here(&[purpose])`
instead, which reaches that thread's reservations of any size and no other
thread's: it is the scoping for a test that enumerates every reservation its
work makes, over sources of several sizes.

Nothing any of it does is reachable outside `cfg(test)` and the dev-only
`stack-fault-injection` feature, so a shipped build carries none of it and
the refusal semantics, `stack_bytes`, the wasm32 profile and the census are
untouched.

Every refusal test then takes its proof from the fault, not from a
source too deep for the machine: a small source, the forcing, the
injected reservation refused, nothing published, and a retry without the
fault succeeding. A forced purpose sizes its reservation from the syntax
scan and never from the length, so the size a test computes with
`parse_stack_bytes` is exactly the size the operation reserves whatever
stack the thread has. A test that keys its fault on a size takes it on the
work of its own thread where the work runs on one — the Vue and Svelte
execution refusals enumerate this thread's parses of the script and refuse
each in turn — because a size-keyed fault is process-wide and matches
whichever thread makes the reservation, so two tests keying the same size
would take each other's faults.
`a_forced_refusal_holds_twenty_times_on_a_large_stack` is the standing
proof: twenty refused parses and twenty refused leases, on a thread whose
stack is explicitly 64 MiB.

A walk no lease covers reserves a region of its own and, when that
reservation fails, ends the process as a failed allocation does. That
stays so until each operation that starts oxc walks holds a lease, and it
cannot yet: see "Operations without a typed incompleteness" below.

Every walk of oxc's over a parsed tree runs under the same containment
(`with_program_stack`, `ProgramWalkStack`, `with_node_stack`,
`with_span_stack`, `with_source_stack`, `with_nesting_stack`), and
`no_crate_walks_oxc_syntax_around_the_containment` fails on a walk entry
outside one.

### The WebAssembly engine-stack profile

On wasm32 the shadow stack grows like any other region, but the engine's
call stack does not, and nothing in the module can read how much of it is
left. There the parse runs under a measured runtime safety profile of the
engine it runs on, `EngineStackProfile`, and a source whose nesting bound
passes it is not parsed: `StackUnavailable` before oxc runs, where the
instance trapped before. The profile is a property of the engine, the
oxc version and the build, not of the language: every host whose stack
can grow carries no profile (`HOST_ENGINE_STACK_PROFILE` is `None`) and
parses any depth (`a_host_whose_stack_grows_refuses_no_depth` parses
200,000 nested brackets on a 1 MiB thread), and oxc itself parses 782 to
9,568 levels on V8's default stack.

`V8_DEFAULT_STACK_PROFILE` is the only profile, and it is scoped to the
runtime contract of V8 at its default stack: Node.js and Chromium. It
records its measurement: V8 14.6 (Node.js 26.5.0) at `--stack-size` 984
KiB, `oxc_parser` 0.151.0, `wasm32-unknown-unknown` release profile
(opt-level 3, lto, one codegen unit); 256 KiB kept for the host's frames
and the module's callers of the parse (under 16 KiB measured from
`VerterHost.upsert` down), and 2,376 bytes a level, twice the costliest
measured level (an object literal's 1,188, table above). Its nesting,
(984 KiB − 256 KiB) / 2,376 bytes, is 313 levels of the scan's bound.
SpiderMonkey, JavaScriptCore and wasmtime are not measured and carry no
profile; another host adds its own profile from its own measurement,
never from probing the stack inside the module.

A stale profile fails a test rather than holding silently:
`the_engine_stack_profile_was_measured_against_the_locked_oxc` fails when
the locked `oxc_parser` differs from the version the profile records, and
`packages/wasm/src/deep-source.spec.ts` analyzes a script at the profile's
nesting on the engine the suite runs on, so an engine whose frames cost
more fails there. `the_engine_stack_profile_admits_its_nesting_and_refuses_one_more`
holds the boundary (313 admitted, 314 and 31,300 refused, oxc never run
for them); the spec holds it through the shipped module (a script at the
profile's nesting yields its binding; one level past it and 100 times
past it neither throw nor trap), and serves a shallow script, and the
refused file made shallow, on the same instance afterwards. With the
refusal removed the module throws `RangeError: Maximum call stack size
exceeded` at 31,300 levels, and the same instance then fails the shallow
script too.

### Typed incompleteness of a refused parse

A parse or a walk-stack lease refused its stack returns `StackUnavailable`,
and the refused parse's program is empty and marked fatal. Nothing is
read off that program as the file's: every operation that parses or
leases records its refusals (`oxc_parse::refusals_within`, scoped to the
operation on its thread, restored on return and on unwind, an inner
operation's refusal carried to the one enclosing it), and a refusal makes
the operation's product typed incompleteness, never an empty file's.

- The scheduler's source stage (`host_executor::execute_source`) fails
  with `StageErrorKind::StackUnavailable`, which the upsert reports as
  `SchedulerError::StackUnavailable { file_id, needed }`; it publishes no
  snapshot, and the same upsert once the stack can be had publishes the
  file. A refused script parse sets `ParseSnapshot::refused`; a refused
  carrier projection is `SyntaxReject::StackUnavailable`, which the
  publication store never retains, and a stage sharing another stage's
  projection maps that reject to the same typed failure.
- The analysis lanes that parse on their own serve no analysis and
  publish no artifact from a refused parse or lease: the rebuild of a
  snapshot from source (`host_manage/analysis_io.rs`,
  `host_manage/eval_env.rs`), the overlay materializer and the
  evaluation program (`parsed_eval_program`), whose flights publish
  nothing from it.
- Template facts whose expression parse was refused are absent, not
  empty, and the result that would have read them is partial, so a cache
  keyed on the source never retains it.
- A compile whose parse or lease was refused is refused whole
  (`VueHostCompileRefusal` / `SvelteHostCompileRefusal::StackUnavailable`)
  and publishes no product: the host reports the fatal diagnostic
  `HOST_STACK_UNAVAILABLE`, a failure blocked on an input outside the
  bytes.
- A public-API projection with a refused parse fails with
  `TscGenerationError::StackUnavailable` (detail code `stack-unavailable`),
  whose subject is the whole source (`TscFailureSubject::Source`, on the
  wire `{ "kind": "source" }` through napi and wasm), and caches no extract
  of the script read off the refused parse.

Through the shipped module a script past the profile throws the typed
refusal on upsert, serves no analysis, and raises no `no-undef-properties`
warning for the binding it declares.

The compiler's entries outside a host record their own refusals: the
standalone compile (`StandaloneCompiler::compile`, `compile_prepared`,
`compile_batch`) fails with `DirectCompileError::StackUnavailable`, a
prepared carrier keeps its preparation's refusal and refuses every compile
from it; the `tsc` generation (`generate_tsc_output*`,
`extract_tsc_state`, `generate_tsc_from_state`) fails with
`TscGenerationError::StackUnavailable`; the specifier inventory
(`collect_module_specifier_spans`) answers the refusal, and the
standalone checker reports it as the file's typed failure.

A walk's stack is taken by a walk-stack lease, the one fallible step, at
an operation's boundary: the operation holds a lease sized for the
program it walks (the analysis lanes, the evaluation program's function
index and the walks over its functions), or a walk takes a lease of its
own (`oxc_parse::leased_program_walk`, `leased_span_walk`,
`leased_ast_walk`) inside an operation that records its refusals: the
source stage, the indexed and overlay materialisations (each on its
calling thread and in the cold-index job on the declaration-lowering
worker), the compile and render entries, the compile request, and the
public-API projection. A refused lease is `StackUnavailable`, recorded
for the operation, and the walk does not run; nothing ends the process.
The remaining containment calls reserve a region of their own when no
lease covers them, which fails only by ending the process; test builds
report each such walk by its call site, and each leased walk that could
reserve while no operation records its refusals.
`every_walk_of_the_host_operations_runs_under_a_lease` runs the host's
operations (upsert, analysis, a flow return, the runtime compile, a
compile request, the public-API projection) and the compiler's standalone
entries (the direct, prepared and batched compile, the `tsc` generation,
the specifier inventory) over TypeScript, Vue and Svelte sources nesting
201 levels, in a child process, and fails on any reported walk.

### Hand-written recursions over oxc syntax

The containment guard finds walks of oxc's (`Visit`, `walk_*`,
`clone_in`, the semantic builder); it does not find Verter's own
functions that recurse over oxc syntax, which spend native stack per
level just the same and belong on explicit work stacks, not stack
regions: a stack region contains oxc's own recursion only.

The script analysis is one such consumer, and it runs on the calling
thread's stack, outside any region. Every pass of it that input nesting
reaches walks from an explicit stack: the module-reference collector, the
await scan, the string evaluator, the nested-macro scan, the binding
leaves of a destructuring pattern, the macro type references, the macro
call walk and the macro type-argument role walk.
`build_tests.rs` →
`every_deeply_nested_script_is_analyzed_on_a_small_stack` analyzes 34
forms 10,000 deep on a 1 MiB thread (calls, parentheses, arrays,
objects, conditionals, operator and member chains, awaits, templates,
arrows, `ref` / `computed`, `defineProps` / `defineEmits` values and
types, `withDefaults`, destructuring, returns, blocks, `if`s, classes),
each in a child process; restoring any pass's recursion overflows the
forms that reach it. The dynamic event-name walks of the Vue projection
walk from explicit stacks too.

A census of the functions over oxc AST types that lie on a cycle of
calls within their file (calling themselves, or calling or passing by
name a function that leads back to them) counts 251 left (67 in
`verter_parser`, 66 in `verter_session`, 65 in `verter_semantic`, 51 in
`verter_compiler`, 2 in `verter_lsp`); a cycle through another file is
not counted. Some are bounded by a depth of their own, and some run
inside a walk's containment. They are tracked, not converted:
`hand_written_recursions_over_oxc_syntax_do_not_grow` fails on a new
recursive function over oxc syntax (listed in
`oxc_parse/hand_written_recursions.txt`), pointing it to an explicit
stack, and on a listed one that no longer recurses. The analysis
flagging an `AppConfig` interface in a namespace nest, a cycle of three
functions the census missed while it counted only direct calls,
overflowed the scheduler's I/O thread on namespaces nested 10,000 deep
once analysis ran off the parse's stack region; it walks the nest from
an explicit stack.

## Canary

`oxc_parse/tests.rs` → `oxc_still_overflows_on_10000_nested_type_arguments`
runs the oxc-only parse of 10,000 nested type arguments on an 8 MiB thread
in a child process and expects it to abort; the ignored
`oxc_parses_deep_nesting_on_an_ordinary_stack` is the same parse expected
to succeed, to un-ignore when oxc stops overflowing.
