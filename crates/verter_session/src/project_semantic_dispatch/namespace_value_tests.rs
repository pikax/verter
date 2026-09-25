//! Values a namespace exports, read in type position. A namespace member
//! registers under its qualified name (`N.plain`, `N.M.deep`), so `typeof
//! N.M.deep` reads that value — a function's body-derived return included
//! — and an instantiation expression (`typeof N.plain<string>`) applies its
//! type arguments to it. An ambient namespace with no export declaration
//! exports every member (the checker's export context).
//!
//! Every expected answer is TypeScript 7.0.2's, measured on this exact
//! fixture with `declare const v: <probe>; export const s: null = v;` read
//! off the TS2322 message (`tsc --noEmit --strict --ignoreConfig`).

use super::checker_probe_lane_tests::mismatches;

const FIXTURE: &str = "\
export namespace N {
  export function make<T>() { return class { v!: T }; }
  export function plain<T>(x: T) { return x; }
  export declare function decl<T>(x: T): T[];
  export const arrow = <T,>(x: T) => [x];
  export namespace M { export function deep<T>(x: T) { return x; } }
}
export declare namespace D { function decl<T>(x: T): T[]; const k: number; namespace Inner { function deep(): string; } }
export declare namespace E { function hidden(): number; export function shown(): string; export {}; }
";

/// A namespace function's value — its body-derived return included — is
/// read through its qualified name, at any nesting depth, and instantiated
/// by the expression's type arguments.
///
/// Measured on TypeScript 7.0.2: `ReturnType<typeof N.plain<string>>` is
/// `string` and `ReturnType<typeof N.plain>` `unknown`; `ReturnType<typeof
/// N.decl<number>>` and `ReturnType<typeof N.arrow<number>>` are
/// `number[]`; `ReturnType<typeof N.M.deep<boolean>>` is `boolean`;
/// `InstanceType<ReturnType<typeof N.make<string>>>['v']` is `string`.
#[test]
fn a_namespace_function_is_read_through_its_qualified_name() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("ReturnType<typeof N.plain<string>>", "string"),
            ("ReturnType<typeof N.plain>", "unknown"),
            ("ReturnType<typeof N.decl<number>>", "number[]"),
            ("ReturnType<typeof N.arrow<number>>", "number[]"),
            ("ReturnType<typeof N.M.deep<boolean>>", "boolean"),
            (
                "InstanceType<ReturnType<typeof N.make<string>>>['v']",
                "string",
            ),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An ambient namespace without an export declaration exports every
/// member; one with an export declaration exports only what it marks.
///
/// Measured on TypeScript 7.0.2: `ReturnType<typeof D.decl<number>>` is
/// `number[]`, `typeof D.k` is `number`, `ReturnType<typeof D.Inner.deep>`
/// is `string` and `ReturnType<typeof E.shown>` is `string`.
#[test]
fn an_ambient_namespace_exports_every_member() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("ReturnType<typeof D.decl<number>>", "number[]"),
            ("typeof D.k", "number"),
            ("ReturnType<typeof D.Inner.deep>", "string"),
            ("ReturnType<typeof E.shown>", "string"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
