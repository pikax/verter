//! An indexed access whose terminal is a named declaration is that
//! declaration, printed by its name as the checker prints it: a member
//! typed by a class, an interface or a generic application, and a class
//! constructor's `prototype` (its class, over `any` for each parameter).
//!
//! Every expected answer is TypeScript 7.0.2's, measured on this exact
//! fixture with `export const a: null = null! as <probe>;` read off the
//! TS2322 message (`tsc --noEmit --strict --ignoreConfig`).

use super::checker_probe_lane_tests::mismatches;

const FIXTURE: &str = "\
export class Decl { x = 1; static s = ''; }
export declare class DDecl { y: number; }
export class GDecl<T> { t!: T; }
export type W = { d: Decl; g: GDecl<string> };
export interface IF { d: Decl }
interface QA { qa: 1 }
interface QB { qb: 2 }
interface H { both?: QA; req: QA }
type Div = H & { both?: QB; req?: QB };
";

/// Measured on TypeScript 7.0.2: `W['d']`, `IF['d']` and `(typeof
/// Decl)['prototype']` are `Decl`, `(typeof DDecl)['prototype']` is
/// `DDecl`, `W['g']` is `GDecl<string>` and `(typeof GDecl)['prototype']`
/// is `GDecl<any>`; `H['req']` is `QA`, `Div['req']` is `QA & QB`,
/// `H['both']` is `QA | undefined` and `Div['both']` is `(QA & QB) |
/// undefined`.
#[test]
fn an_indexed_access_of_a_named_declaration_prints_its_name() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("W['d']", "Decl"),
            ("IF['d']", "Decl"),
            ("(typeof Decl)['prototype']", "Decl"),
            ("(typeof DDecl)['prototype']", "DDecl"),
            ("W['g']", "GDecl<string>"),
            ("(typeof GDecl)['prototype']", "GDecl<any>"),
            ("H['req']", "QA"),
            ("Div['req']", "QA & QB"),
            ("H['both']", "QA | undefined"),
            ("Div['both']", "(QA & QB) | undefined"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
