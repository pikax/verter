# oxc's parser on deeply nested input

A dependency limitation, recorded with a standalone reproducer. Nothing
here is filed upstream.

## What fails

`oxc_parser` 0.126.0 (`oxc_parser::Parser::parse`) is a recursive descent
with no depth limit of its own. It spends native stack per level of
syntactic nesting, and when the thread's stack runs out the process aborts
(`thread '…' has overflowed its stack`, exit code 0xC00000FD on Windows)
before any AST is returned. Nothing of Verter's runs: the reproducer calls
`oxc_parser::Parser::new(…).parse()` and nothing else.

## Reproducer

`crates/verter_parser/examples/oxc_deep_parse.rs` builds one nested
TypeScript source and parses it on a thread of the given stack:

```text
cargo run -p verter_parser --example oxc_deep_parse [--release] -- <form> <depth> <stack-MiB>
```

For example `oxc_deep_parse parentheses 6000 8` aborts with
`thread 'oxc-parse' has overflowed its stack` on an optimized build;
`oxc_deep_parse parentheses 200000 2048` parses (1 statement, 0 errors).

## Measured

Platform: Windows 11 Pro 10.0.26200, x86_64-pc-windows-msvc,
rustc 1.97.1, oxc_parser 0.126.0. The deepest depth that parses, by
bisection (within 1%):

| form (per level) | release, 1 MiB | release, 8 MiB | debug, 1 MiB | debug, 8 MiB |
|---|---|---|---|---|
| parentheses `(…)` | 684 | 5,664 | 412 | 3,424 |
| logical not `!` | 3,712 | 30,464 | 1,680 | 14,080 |
| type argument `Box<…>` | 548 | 4,512 | 274 | 2,272 |
| conditional `b ? 1 : …` | 1,968 | 16,256 | 1,952 | 16,256 |
| array `[…]` | 636 | 5,248 | 404 | 3,360 |
| object `{ v: … }` | 548 | 4,512 | 304 | 2,544 |
| arrow `() => …` | 1,968 | 16,256 | 1,160 | 9,600 |
| template hole `` `${…}` `` | 572 | 4,736 | 404 | 3,360 |
| `keyof …` | 1,968 | 16,256 | 3,680 | 30,464 |
| block `{ … }` | 1,088 | 8,960 | 1,336 | 11,072 |

So a level costs oxc 1.5–1.9 KiB (optimized) and up to 3.7 KiB
(unoptimized) for the costliest forms. With a 2 GiB stack the parentheses,
type-argument and object forms parse at 10,000 and 200,000 levels.
TypeScript 7.0.2 checks each of these forms at 10,000 levels, and the
parentheses, type-argument and `!` forms at 50,000 and 200,000.

## Containment in Verter

Every production parse goes through `verter_parser::oxc_parse::Parser`, a
drop-in for `oxc_parser::Parser` that parses exactly what oxc parses. It
runs the parse on a stack the source cannot exhaust: a linear scan
(`oxc_parse/nesting.rs`) bounds the syntax tree's depth from above, and the
parse gets 8 KiB per level of that bound (twice oxc's costliest measured
level) plus 512 KiB. It runs in place when the thread has that much left
(every source of ordinary depth, and every source short enough that each
byte could be a level) and on a stack segment allocated with `stacker`
otherwise. No source is refused and no depth is imposed. The scan costs
about a third of the parse (8.1 ms against 6.1 ms for `lib.dom.d.ts`'s
2.3 MB; 57 µs against 39 µs for a 16 KB source, optimized).
`no_crate_parses_around_the_guard` fails on any direct
`oxc_parser::Parser` in a crate's sources.

`stacker` cannot grow the stack on `wasm32-unknown-unknown`: there the
parse runs on the module's 8 MiB stack (`crates/verter_wasm/build.rs`), so
the release column's 8 MiB depths above bound what the WebAssembly host
parses. wasm32's stack has no guard page, so past them the overflow is not
a clean abort.
