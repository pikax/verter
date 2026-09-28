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
bisection, on the optimized wasm32-unknown-unknown module under Node.js
26 (the shadow stack grown per parse, so the engine stack binds), at V8's
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
enough that each byte could be a level) and on a stack segment reserved
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

A segment reserves its size and commits only what the work on it
touches: on Windows a fiber made with `CreateFiberEx`, committing 64 KiB
and reserving the rest (`CreateFiber`, which `stacker` used, commits the
whole size: a 1,000,000-level bound committed 9,234,591,744 bytes for a
walk a few frames deep, 86–94 KB now,
`a_grown_stack_commits_what_the_walk_touches_not_what_it_reserves`); on
Unix an anonymous `MAP_NORESERVE` mapping; on wasm32 a heap allocation for
the module's shadow stack. A segment that cannot be reserved is
`StackUnavailable`, never a panic: the parse returns an empty program and
the `verter(stack-unavailable)` diagnostic
(`stack_unavailable_diagnostic`, `is_stack_unavailable`), operational
incompleteness rather than a syntax error. A walk runs over a program
whose parse had the same stack or more, so a walk that cannot reserve its
segment meets an exhausted address space and ends the process as any
failed allocation does.

Every walk of oxc's over a parsed tree runs under the same containment
(`with_program_stack`, `ProgramWalkStack`, `with_node_stack`,
`with_span_stack`, `with_source_stack`, `with_nesting_stack`), and
`no_crate_walks_oxc_syntax_around_the_containment` fails on a walk entry
outside one.

On wasm32 the shadow stack grows like any other segment, but the
engine's call stack does not, and nothing in the module can read how much
of it is left. There the parse is bounded by the stack the host provides:
`WASM_ENGINE_NESTING` = (984 KiB, V8's default stack, less 256 KiB kept
for the host's frames and the module's callers) / 2,376 bytes (twice the
costliest measured level, an object literal's 1,188) = 313 levels of the
scan's bound. A deeper source returns `StackUnavailable` before oxc runs,
where it trapped the instance before; oxc itself reaches 782 to 9,568
levels there, so the bound is the engine stack's, not a language limit.
The bound holds for oxc's parse and walks; the module's own analysis
over the parsed program is contained by its explicit stacks
(`performance-gates.md`).

## Canary

`oxc_parse/tests.rs` → `oxc_still_overflows_on_10000_nested_type_arguments`
runs the oxc-only parse of 10,000 nested type arguments on an 8 MiB thread
in a child process and expects it to abort; the ignored
`oxc_parses_deep_nesting_on_an_ordinary_stack` is the same parse expected
to succeed, to un-ignore when oxc stops overflowing.
