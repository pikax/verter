//! A type's syntax nests without bound, and lowering it must not spend
//! native stack per nesting level: a named reference's arguments, an
//! indexed access's object, union and intersection arms, array and tuple
//! elements, a `keyof` operand and an object type's property values lower
//! from an explicit stack.
//!
//! Every expected answer is TypeScript 7.0.2's, measured on the exact
//! fixture with `tsc --ignoreConfig --noEmit --strict` under each of
//! `strictNullChecks` and `noImplicitAny` on and off (the four settings
//! agree on every probe) as `const s: "q" = null! as <probe>;`, read off
//! the TS2322 message.

use super::checker_probe_lane_tests::mismatches;

/// `interface Box<T> { v: T }`, `interface R { v: R }` and
/// `type D = Box<Box<…<1>…>>`, `Box` applied `depth` times around `1`.
fn nested_box(depth: usize) -> String {
    format!(
        "interface Box<T> {{ v: T }}\ninterface R {{ v: R }}\ntype D = {}1{};\n",
        "Box<".repeat(depth),
        ">".repeat(depth)
    )
}

/// `Box` applied `depth` times around `inner`.
fn boxed(depth: usize, inner: &str) -> String {
    format!("{}{inner}{}", "Box<".repeat(depth), ">".repeat(depth))
}

/// An 80-deep nested generic application reads on the default test
/// stack: through 80 member reads, and as either side of a relation.
///
/// Measured on TypeScript 7.0.2 (all four settings): over `D` with `Box`
/// applied 80 times, `D['v']…['v']` (80 reads) is `1`, and `[D] extends
/// [Box<…80…<number>>] ? 1 : 2` and `[D] extends [Box<…80…<1>>] ? 1 : 2`
/// are `1`.
#[test]
fn an_80_deep_nested_generic_application_reads_on_the_default_stack() {
    let reads = format!("D{}", "['v']".repeat(80));
    let to_number = format!("[D] extends [{}] ? 1 : 2", boxed(80, "number"));
    let to_itself = format!("[D] extends [{}] ? 1 : 2", boxed(80, "1"));
    let failures = mismatches(
        &nested_box(80),
        &[
            (reads.as_str(), "1"),
            (to_number.as_str(), "1"),
            (to_itself.as_str(), "1"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A 700-deep nested generic application parses and lowers on the
/// scheduler's workers: the parser and the syntax-tree clone recurse once
/// per argument level on the worker that loads the file.
///
/// Measured on TypeScript 7.0.2 (all four settings): over `D` with `Box`
/// applied 700 times, `keyof D` is `"v"` and `D extends Box<unknown> ? 1 :
/// 2` is `1` (the same at 1,500 and 2,100 levels).
#[test]
fn a_700_deep_nested_generic_application_parses_on_a_scheduler_worker() {
    let failures = mismatches(
        &nested_box(700),
        &[
            ("keyof D", "\"v\""),
            ("D extends Box<unknown> ? 1 : 2", "1"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A chain of 1,000 indexed accesses lowers on the default test stack.
///
/// Measured on TypeScript 7.0.2 (all four settings): over `interface R {
/// v: R }`, `R['v']…['v']` (1,000 reads) is `R`.
#[test]
fn a_1000_long_indexed_access_chain_lowers_on_the_default_stack() {
    let reads = format!("R{}", "['v']".repeat(1000));
    let failures = mismatches(&nested_box(1), &[(reads.as_str(), "R")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `type Pick<T> = T extends 0 ? "c0" : T extends 1 ? "c1" : … : "none"`,
/// `links` conditionals chained through their false branches.
fn conditional_chain(links: usize) -> String {
    let mut body = String::new();
    for link in 0..links {
        body.push_str(&format!("T extends {link} ? \"c{link}\" : "));
    }
    format!("type Pick<T> = {body}\"none\";\n")
}

/// A conditional chained 320 deep through its false branches resolves on
/// the default test stack.
///
/// Measured on TypeScript 7.0.2 (all four settings): over the 320-link
/// chain, `Pick<319>` is `"c319"` and `Pick<-1>` is `"none"`; over a
/// 160-link chain, `Pick<159>` is `"c159"`.
#[test]
fn a_320_deep_conditional_chain_resolves_on_the_default_stack() {
    let failures = mismatches(
        &conditional_chain(320),
        &[("Pick<319>", "\"c319\""), ("Pick<-1>", "\"none\"")],
    );
    let shorter = mismatches(&conditional_chain(160), &[("Pick<159>", "\"c159\"")]);
    assert!(
        failures.is_empty() && shorter.is_empty(),
        "{}",
        failures
            .into_iter()
            .chain(shorter)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// A 400-deep nested object type reads through 400 member accesses on the
/// default test stack: its declaration's shape lowers from an explicit
/// stack.
///
/// Measured on TypeScript 7.0.2 (all four settings): over `type O = { v:
/// { v: … 1 } }` nested 400 times, `O['v']…['v']` (400 reads) is `1`.
#[test]
fn a_400_deep_nested_object_type_reads_on_the_default_stack() {
    let source = format!("type O = {}1{};\n", "{ v: ".repeat(400), " }".repeat(400));
    let reads = format!("O{}", "['v']".repeat(400));
    let failures = mismatches(&source, &[(reads.as_str(), "1")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
