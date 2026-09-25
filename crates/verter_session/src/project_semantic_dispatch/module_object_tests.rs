//! A module read as a value is its module object: one object whose
//! properties are the module's exported values — through a namespace
//! import, an import assignment of a module without `export =`, and
//! `typeof import("…")`. A namespace declaration read as a value is the
//! same object over the namespace's exported values, and an ambient
//! `declare module "…"` block is the module a file would be. A module
//! assigned with `export =` is its value, which a namespace import wraps
//! with `default` when it can be called or constructed.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured on this
//! exact fixture with `tsc --ignoreConfig --declaration
//! --emitDeclarationOnly --strict --module commonjs`, each row asserted as
//! an exact type equality beside it; the four `strictNullChecks` ×
//! `noImplicitAny` settings answer alike.

use super::checker_probe_lane_tests::{mismatches_in, ProbeProject};

const DEP: &str = "\
export const a = 1;
export let b = \"s\";
export function f(): \"f\" { return \"f\"; }
export class K { m(): \"km\" { return \"km\"; } static s = 1; }
export interface I { i: 1 }
export type T = { t: 1 };
export enum E { X = 1 }
export namespace NN { export const q = 2; }
export default 5;
";

const DEP2: &str = "\
export * from './dep';
export const extra = \"e\";
export { a as renamed } from './dep';
";

const NS_EQ: &str =
    "declare namespace N { function f(): \"n-f\"; const c: \"n-c\"; }\nexport = N;\n";
const FN_EQ: &str = "declare function F(): \"f-call\";\nexport = F;\n";
const CLS_EQ: &str = "declare class C { m(): \"c-m\"; static s: \"c-s\"; }\nexport = C;\n";
const MERGED_EQ: &str = "\
declare function M(): \"m-call\";
declare namespace M { const c: \"m-c\"; function g(): \"m-g\"; }
export = M;
";
const VAL_EQ: &str = "declare const V: { a: \"v-a\"; b(): \"v-b\" };\nexport = V;\n";

const FILES: [(&str, &str); 7] = [
    ("dep.ts", DEP),
    ("dep2.ts", DEP2),
    ("ns.d.ts", NS_EQ),
    ("fn.d.ts", FN_EQ),
    ("cls.d.ts", CLS_EQ),
    ("merged.d.ts", MERGED_EQ),
    ("val.d.ts", VAL_EQ),
];

const USE: &str = "\
import * as D from './dep';
import * as D2 from './dep2';
import * as N from './ns';
import * as F from './fn';
import * as C from './cls';
import * as M from './merged';
import * as V from './val';
import NR = require('./ns');
import DR = require('./dep');
export function retD() { return D; }
export function obj() { return { d: D }; }
export function assign(): { a: number } { return D; }
namespace LN {
  export const c = 1;
  export function f(): \"lf\" { return \"lf\"; }
  const hidden = 2;
  export namespace Inner { export const i = 3; }
  export interface T { t: 1 }
  export namespace TypesOnly { export interface U {} }
}
export function retLN() { return LN; }
";

/// A namespace import of a module without `export =` is the module
/// object: its value exports (no interface or type alias), `default`
/// among them, and every name an `export * from` re-exports except
/// `default`. `typeof import("…")` and an import assignment of the module
/// are the same object.
///
/// Measured on TypeScript 7.0.2: `keyof typeof D` is
/// `"E" | "K" | "NN" | "a" | "b" | "default" | "f"`, `keyof typeof D2`
/// `"E" | "K" | "NN" | "a" | "b" | "extra" | "f" | "renamed"`,
/// `keyof typeof import('./dep')` and `keyof typeof DR` the same as `D`;
/// `(typeof D)['a']` is `1`, `(typeof D)['b']` `string`,
/// `(typeof D)['default']` `5`, `(typeof D)['NN']['q']` `2`,
/// `InstanceType<(typeof D)['K']>['m']` `() => "km"`,
/// `(typeof D2)['renamed']` `1`; `typeof D` relates to `{ a: number }`
/// (`"y"`) and not to `{ zz: number }` (`"n"`); `retD` is `typeof D`, `obj`
/// `{ d: typeof D; }`.
///
/// Mutation: reading no module object for a namespace import leaves every
/// `typeof D` row a typed miss; collecting no `export *` names answers
/// `keyof typeof D2` `"extra" | "renamed"`.
#[test]
fn a_namespace_import_is_the_module_object() {
    let failures = mismatches_in(
        ProbeProject {
            files: &FILES,
            compiler_options: None,
            ambient_lib: None,
        },
        USE,
        &[
            (
                "keyof typeof D",
                "\"E\" | \"K\" | \"NN\" | \"a\" | \"b\" | \"default\" | \"f\"",
            ),
            (
                "keyof typeof D2",
                "\"E\" | \"K\" | \"NN\" | \"a\" | \"b\" | \"extra\" | \"f\" | \"renamed\"",
            ),
            (
                "keyof typeof import('./dep')",
                "\"E\" | \"K\" | \"NN\" | \"a\" | \"b\" | \"default\" | \"f\"",
            ),
            (
                "keyof typeof DR",
                "\"E\" | \"K\" | \"NN\" | \"a\" | \"b\" | \"default\" | \"f\"",
            ),
            ("(typeof D)['a']", "1"),
            ("(typeof D)['b']", "string"),
            ("(typeof D)['default']", "5"),
            ("(typeof D)['NN']['q']", "2"),
            ("InstanceType<(typeof D)['K']>['m']", "() => \"km\""),
            ("(typeof D2)['renamed']", "1"),
            ("(typeof D) extends { a: number } ? \"y\" : \"n\"", "\"y\""),
            ("(typeof D) extends { zz: number } ? \"y\" : \"n\"", "\"n\""),
            ("ReturnType<typeof retD>['f']", "() => \"f\""),
            ("ReturnType<typeof obj>['d']['a']", "1"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A module assigned with `export =` is its value: a namespace, a
/// function, a class or a `const`. A namespace import of it is the value
/// itself when it cannot be called or constructed, and otherwise the
/// object of its own properties and `default`, the value (TypeScript 7's
/// interop); `typeof import("…")` and an import assignment apply no
/// interop.
///
/// Measured on TypeScript 7.0.2: `keyof typeof N` and `keyof typeof NR`
/// are `"c" | "f"`, `keyof typeof F` `"default"`, `keyof typeof M`
/// `"c" | "default" | "g"`, `keyof typeof V` `"a" | "b"`,
/// `keyof typeof import('./fn')` `never`; `(typeof F)['default']` is
/// `typeof import("./fn")` (`() => "f-call"`), `(typeof C)['s']` `"c-s"`,
/// `InstanceType<(typeof C)['default']>['m']` `() => "c-m"`,
/// `(typeof M)['c']` `"m-c"`, `(typeof V)['a']` `"v-a"`. (`keyof typeof
/// C` is `"default" | "prototype" | "s"`; a class's `prototype` property is
/// not asked here.)
///
/// Mutation: applying the interop to no namespace import answers
/// `keyof typeof F` `never` and misses `(typeof F)['default']`; applying it
/// to every one answers `keyof typeof V` with `"default"` too.
#[test]
fn a_module_assigned_with_export_equals_is_its_value() {
    let failures = mismatches_in(
        ProbeProject {
            files: &FILES,
            compiler_options: None,
            ambient_lib: None,
        },
        USE,
        &[
            ("keyof typeof N", "\"c\" | \"f\""),
            ("keyof typeof NR", "\"c\" | \"f\""),
            ("keyof typeof F", "\"default\""),
            ("keyof typeof M", "\"c\" | \"default\" | \"g\""),
            ("keyof typeof V", "\"a\" | \"b\""),
            ("keyof typeof import('./fn')", "never"),
            ("(typeof F)['default']", "() => \"f-call\""),
            ("(typeof C)['s']", "\"c-s\""),
            ("InstanceType<(typeof C)['default']>['m']", "() => \"c-m\""),
            ("(typeof M)['c']", "\"m-c\""),
            ("(typeof V)['a']", "\"v-a\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A namespace declaration read as a value is the object of its exported
/// values and of the nested namespaces that declare one; a member it does
/// not export, and a namespace holding only types, is no property.
///
/// Measured on TypeScript 7.0.2: `keyof typeof LN` is
/// `"Inner" | "c" | "f"`, `(typeof LN)['Inner']['i']` `3`, `retLN`
/// `typeof LN` (its `c` `1`).
///
/// Mutation: reading no object for a namespace name leaves every row a
/// typed miss.
#[test]
fn a_namespace_declaration_is_its_object() {
    let failures = mismatches_in(
        ProbeProject {
            files: &FILES,
            compiler_options: None,
            ambient_lib: None,
        },
        USE,
        &[
            ("keyof typeof LN", "\"Inner\" | \"c\" | \"f\""),
            ("(typeof LN)['Inner']['i']", "3"),
            ("ReturnType<typeof retLN>['c']", "1"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const AMBIENT: &str = "\
declare module \"amb\" {
  export function x(): \"x\";
  export const k: \"k\";
  export let l: number;
  export class Cls { m(): \"cm\"; static s: \"cs\"; }
  export namespace AN { function f(): \"an-f\"; interface U { u: \"u\" } const c: \"an-c\"; }
  const hidden: \"h\";
  export interface T { t: \"t\" }
  export default function dflt(): \"d\";
}
declare module \"amb2\" {
  function y(): \"y\";
  const z: \"z\";
}
declare module \"eqmod\" {
  function Q(): \"q\";
  namespace Q { const w: \"w\"; }
  export = Q;
}
";

/// A namespace import of a module only `declare module` blocks declare is
/// the object of the blocks' values — every declaration of a block without
/// an export declaration is exported, and `export default function f`
/// exports `f` as `default` only — and a block assigning `export =` is its
/// value, wrapped by the interop when it can be called.
///
/// Measured on TypeScript 7.0.2: `keyof typeof A` is
/// `"AN" | "Cls" | "default" | "hidden" | "k" | "l" | "x"`,
/// `keyof typeof A2` `"y" | "z"`, `keyof typeof Q` `"default" | "w"`,
/// `keyof typeof A.AN` `"c" | "f"`, `(typeof A)['k']` `"k"`,
/// `ReturnType<(typeof A)['default']>` `"d"`,
/// `ReturnType<(typeof Q)['default']>` `"q"`, `(typeof Q)['w']` `"w"`.
///
/// Mutation: reading no object for an ambient module leaves every
/// `typeof A` and `typeof A2` row a typed miss; listing the `export
/// default` declaration by its own name answers `keyof typeof A` with
/// `"dflt"` too; recording no namespace a block declares drops `"AN"` from
/// `keyof typeof A` and misses `keyof typeof A.AN`.
#[test]
fn an_ambient_module_is_its_blocks_object() {
    let files = [("amb.d.ts", AMBIENT)];
    let failures = mismatches_in(
        ProbeProject {
            files: &files,
            compiler_options: None,
            ambient_lib: None,
        },
        "import * as A from 'amb';\nimport * as A2 from 'amb2';\nimport * as Q from 'eqmod';\n",
        &[
            (
                "keyof typeof A",
                "\"AN\" | \"Cls\" | \"default\" | \"hidden\" | \"k\" | \"l\" | \"x\"",
            ),
            ("keyof typeof A2", "\"y\" | \"z\""),
            ("keyof typeof Q", "\"default\" | \"w\""),
            ("keyof typeof A.AN", "\"c\" | \"f\""),
            ("(typeof A)['k']", "\"k\""),
            ("ReturnType<(typeof A)['default']>", "\"d\""),
            ("ReturnType<(typeof Q)['default']>", "\"q\""),
            ("(typeof Q)['w']", "\"w\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
