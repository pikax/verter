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
