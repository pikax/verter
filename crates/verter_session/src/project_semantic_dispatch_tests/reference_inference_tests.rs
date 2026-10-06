//! Inference between two references to one generic declaration reads their
//! type arguments before any structure (`inferFromTypes` over one alias or
//! one reference target: `inferFromTypeArguments`).
//!
//! Every expected answer is TypeScript 7.0.2's, measured with `tsc
//! --ignoreConfig --noEmit --strict --noErrorTruncation` under all four
//! `strictNullChecks` x `noImplicitAny` settings (alike in every setting):
//! the type is read off TS2322 for `const s: never = x` with `declare const
//! x: <probe>`.

use super::differential_harness_tests::{Matrix, ALL};

const SOURCE: &str = r##"
type Box<X> = { v: X };
interface IB<X> { v: X }
type Fn<X> = (x: X) => void;
type Two<A, B> = { a: A; b: B };
type Ph<T> = { k: 1 };
interface IPh<T> { k: 1 }
type CoFn<T> = { f: (x: T) => void };
type PE<M, L> = { type: "ParsingError"; message: M; lineNumber: L };
type F<X> = [X] extends [PE<infer M, any>] ? M : "ok";
"##;

/// `Box<"a">` against `Box<infer P>` infers `"a"` from the type argument,
/// and so does a parameter the declaration never reads (`Ph<"a">` against
/// `Ph<infer P>`), which no structure infers (`{ k: 1 }` against
/// `Ph<infer P>` infers nothing, `unknown`). A contravariant parameter's
/// argument infers contravariantly, a repeated `infer` unions its
/// candidates, and a reference nested in a tuple or another reference is a
/// site of the same pattern.
#[test]
fn references_to_one_declaration_infer_from_their_type_arguments() {
    let failures = Matrix::new(SOURCE).settings(&ALL).types(&[
        (r#"Box<"a"> extends Box<infer P> ? P : 0"#, r#""a""#),
        (r#"IB<"a"> extends IB<infer P> ? P : 0"#, r#""a""#),
        (r#"{ v: "a" } extends Box<infer P> ? P : 0"#, r#""a""#),
        ("Box<Box<1>> extends Box<Box<infer Q>> ? Q : 0", "1"),
        ("Fn<string> extends Fn<infer P> ? P : 0", "string"),
        (
            r#"Two<1, "x"> extends Two<infer A, infer B> ? [A, B] : 0"#,
            r#"[1, "x"]"#,
        ),
        ("Two<1, 2> extends Two<infer A, infer A> ? A : 0", "1 | 2"),
        (r#"Ph<"a"> extends Ph<infer P> ? P : 0"#, r#""a""#),
        (r#"IPh<"a"> extends IPh<infer P> ? P : 0"#, r#""a""#),
        ("{ k: 1 } extends Ph<infer P> ? P : 0", "unknown"),
        (r#"CoFn<"a"> extends CoFn<infer P> ? P : 0"#, r#""a""#),
        (r#"F<PE<"m", 1>>"#, r#""m""#),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
