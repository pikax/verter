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

## Containment in Verter

Every production parse goes through `verter_parser::oxc_parse::Parser`, a
drop-in for `oxc_parser::Parser` that parses exactly what oxc parses. It
runs the parse on a stack the source cannot exhaust: a linear scan
(`oxc_parse/nesting.rs`) bounds the syntax tree's depth from above, and the
parse gets 9 KiB per level of that bound (twice the costliest measured
level, `clone_in`'s) plus 512 KiB. It runs in place when the thread has
that much left (every source of ordinary depth, and every source short
enough that each byte could be a level) and on a stack segment allocated
with `stacker` otherwise. No source is refused and no depth is imposed. The
scan costs about a third of the parse (8.1 ms against 6.1 ms for
`lib.dom.d.ts`'s 2.3 MB; 57 µs against 39 µs for a 16 KB source,
optimized, measured on 0.126). `no_crate_parses_around_the_guard` fails on
any direct `oxc_parser::Parser` in a crate's sources.

Every walk of oxc's over a parsed tree runs under the same containment
(`with_program_stack`, `ProgramWalkStack`, `with_node_stack`,
`with_span_stack`, `with_source_stack`, `with_nesting_stack`), and
`no_crate_walks_oxc_syntax_around_the_containment` fails on a walk entry
outside one.

`stacker` cannot grow the stack on `wasm32-unknown-unknown`: there the
parse and the walks run on the module's stack (1 MiB, wasm32's linker
default), so the release 1 MiB depths above bound what the WebAssembly host
parses and walks. wasm32's stack has no guard page, so past them the
overflow is not a clean abort.

## Canary

`oxc_parse/tests.rs` → `oxc_still_overflows_on_10000_nested_type_arguments`
runs the oxc-only parse of 10,000 nested type arguments on an 8 MiB thread
in a child process and expects it to abort; the ignored
`oxc_parses_deep_nesting_on_an_ordinary_stack` is the same parse expected
to succeed, to un-ignore when oxc stops overflowing.
