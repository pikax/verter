//! Differential probes of modules and globals: namespaces (merged,
//! nested, ambient, and merged with functions, classes and enums), global
//! scripts, named, default, namespace and `export =` imports, re-exports,
//! `import type`, module and global augmentations, and `import()` types.
//! A row is a function of the fixture answered as its body-derived return,
//! or a type in TYPE position.
//!
//! Every expected answer is TypeScript 7.0.2's, measured on the exact
//! fixture with `tsc --ignoreConfig --declaration --emitDeclarationOnly
//! --strict --noErrorTruncation` (with `--module commonjs` for the
//! multi-file fixture, which imports an `export =` module) under each
//! `strictNullChecks` × `noImplicitAny` setting: `declare const p:
//! <probe>; const s: never = p;` (a function row reads `ReturnType<typeof
//! f>`) read off the TS2322 message; the four settings agree on every row.
//! An ignored test asserts the measured answer for rows the lane does not
//! yet answer as the checker does.

use super::differential_harness_tests::{Matrix, Read};

/// Namespaces merged with each other, a function, a class and an enum.
const NAMESPACES: &str = r##"
namespace NS { export const v = 1; export function f() { return "f" as const; } export interface I { i: 1 } export namespace Inner { export const deep = true; } const hidden = 2; }
namespace NS { export const w = "w"; }
namespace NSType { export type T = { t: 1 }; }
declare namespace Amb { const a: 1; function af(): 2; interface AI { ai: 3 } }
function Merge() { return "call" as const; }
namespace Merge { export const m = 1; }
class Cls { c = 2; }
namespace Cls { export const s = 1; }
enum En { A }
namespace En { export const extra = "e"; }
export function nsV() { return NS.v; }
export function nsF() { return NS.f(); }
export function nsW() { return NS.w; }
export function nsDeep() { return NS.Inner.deep; }
export function ambA() { return Amb.a; }
export function ambF() { return Amb.af(); }
export function mergeCall() { return Merge(); }
export function mergeProp() { return Merge.m; }
export function clsStatic() { return Cls.s; }
export function clsInst() { return new Cls().c; }
export function enExtra() { return En.extra; }
export function enMember() { return En.A; }
"##;

/// A namespace's exported values, functions, nested namespaces and types read
/// through the namespace (across merged blocks and from an ambient namespace),
/// and a namespace merged with a function, a class or an enum adds its members
/// to the value.
#[test]
fn namespace_members_read_as_the_checker_reads_them() {
    let matrix = Matrix::new(NAMESPACES);
    let failures = matrix.same(&[
        (Read::Return("nsV"), "number"),
        (Read::Return("nsF"), "\"f\""),
        (Read::Return("nsW"), "string"),
        (Read::Return("nsDeep"), "boolean"),
        (Read::Return("ambA"), "1"),
        (Read::Return("ambF"), "2"),
        (Read::Return("mergeCall"), "\"call\""),
        (Read::Return("mergeProp"), "number"),
        (Read::Return("clsStatic"), "number"),
        (Read::Return("clsInst"), "number"),
        (Read::Return("enExtra"), "string"),
        (Read::Return("enMember"), "En"),
        (Read::Type("NS.I['i']"), "1"),
        (Read::Type("NSType.T['t']"), "1"),
        (Read::Type("Amb.AI['ai']"), "3"),
        (Read::Type("typeof NS.v"), "1"),
        (Read::Type("keyof typeof NS.Inner"), "\"deep\""),
        (Read::Type("(typeof NS)['w']"), "\"w\""),
        (Read::Type("ReturnType<typeof NS.f>"), "\"f\""),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A global script declaring variables, overloads, merged interfaces and
/// functions.
const GLOBAL_SCRIPT: &str = r##"
declare var gVar: string;
declare let gLet: number;
declare function gFn(x: number): boolean;
declare function gFn(x: string): string;
interface GI { g: 1 }
interface GI { h: 2 }
type GT = [GI];
declare const gConst: "c";
function gDecl() { return 1 as const; }
var gInferred = { k: "v" };
function readVar() { return gVar; }
function readLet() { return gLet; }
function callFnNum() { return gFn(1); }
function callFnStr() { return gFn("s"); }
function readConst() { return gConst; }
function callDecl() { return gDecl(); }
function readInferred() { return gInferred; }
function readThroughGlobalThis() { return globalThis.gVar; }
"##;

/// A global script's `declare var` / `let` / `const`, overloaded declared
/// functions, merged interfaces, functions and inferred variables read from the
/// script's own functions (a global `var` through `globalThis` too) and in TYPE
/// position.
#[test]
fn global_script_declarations_read_as_the_checker_reads_them() {
    let matrix = Matrix::new(GLOBAL_SCRIPT).script();
    let failures = matrix.same(&[
        (Read::Return("readVar"), "string"),
        (Read::Return("readLet"), "number"),
        (Read::Return("callFnNum"), "boolean"),
        (Read::Return("callFnStr"), "string"),
        (Read::Return("readConst"), "\"c\""),
        (Read::Return("callDecl"), "1"),
        (Read::Return("readInferred"), "{ k: string; }"),
        (Read::Return("readThroughGlobalThis"), "string"),
        (Read::Type("GI['h']"), "2"),
        (Read::Type("keyof GI"), "keyof GI"),
        (Read::Type("GT[0]['g']"), "1"),
        (Read::Type("typeof gConst"), "\"c\""),
        (
            Read::Type("typeof gFn"),
            "{ (x: number): boolean; (x: string): string; }",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A module importing named, default, namespace, `export =` and type-only
/// bindings, augmenting its dependency and the global scope.
const IMPORTS: &str = r##"
import { exported, ExportedI, def as renamedDefault } from "./dep";
import defaultValue from "./dep";
import * as depNs from "./dep";
import Cjs = require("./cjs");
import type { OnlyType, Plain2 } from "./dep";
declare module "./dep" { interface ExportedI { augmented: "aug" } }
declare global { interface GlobalAug { ga: 1 } }
export function readExported() { return exported; }
export function readDefault() { return defaultValue; }
export function readRenamed() { return renamedDefault; }
export function readNs() { return depNs.exported; }
export function readNsFn() { return depNs.fn(); }
export function readCjs() { return Cjs.cjsValue; }
export function callCjs() { return Cjs.cjsFn(); }
export function readReexport() { return depNs.reexported; }
"##;

/// The modules the probe module imports: a module with named, default and
/// re-exported bindings, its re-export source, and an `export =` declaration
/// file.
const IMPORTS_FILES: &[(&str, &str)] = &[
    (
        "dep.ts",
        r##"export const exported = 42 as const;
export interface ExportedI { base: "b" }
export type OnlyType = { only: true };
export interface Plain2 { p2: "p" }
const d = { dv: 1 };
export default d;
export const def = "named-def" as const;
export function fn() { return [1] as const; }
export { other as reexported } from "./other";
"##,
    ),
    (
        "other.ts",
        r##"export const other = { o: "o" } as const;
"##,
    ),
    (
        "cjs.d.ts",
        r##"declare namespace CjsNs { const cjsValue: "cv"; function cjsFn(): 7; interface CjsI { ci: 9 } }
export = CjsNs;
"##,
    ),
];

/// Named, renamed, default, namespace and `export =` imports, a re-export
/// through the namespace, a type-only import, a global augmentation, `typeof`
/// over an imported value and `import()` types read as the checker reads them.
#[test]
fn imported_bindings_read_as_the_checker_reads_them() {
    let matrix = Matrix::new(IMPORTS).files(IMPORTS_FILES);
    let failures = matrix.same(&[
        (Read::Return("readExported"), "42"),
        (Read::Return("readDefault"), "{ dv: number; }"),
        (Read::Return("readRenamed"), "\"named-def\""),
        (Read::Return("readNs"), "42"),
        (Read::Return("readNsFn"), "readonly [1]"),
        (Read::Return("readCjs"), "\"cv\""),
        (Read::Return("callCjs"), "7"),
        (Read::Return("readReexport"), "{ readonly o: \"o\"; }"),
        (Read::Type("Plain2['p2']"), "\"p\""),
        (Read::Type("OnlyType"), "OnlyType"),
        (Read::Type("GlobalAug['ga']"), "1"),
        (Read::Type("typeof depNs.exported"), "42"),
        (Read::Type("Cjs.CjsI['ci']"), "9"),
        (Read::Type("typeof import('./dep').exported"), "42"),
        (Read::Type("import('./dep').ExportedI['base']"), "\"b\""),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `declare module "./dep" { interface ExportedI { augmented: "aug" } }` merges
/// into the imported `ExportedI`: `ExportedI['base']` is `"b"` and
/// `ExportedI['augmented']` is `"aug"` (an unaugmented import beside it reads).
///
/// What the lane gives:
/// - `ExportedI['base']`: the checker answers `"b"`; the lane measured `<opaque
///   Miss>`.
/// - `ExportedI['augmented']`: the checker answers `"aug"`; the lane measured
///   `<opaque Miss>`.
#[test]
#[ignore = "an imported interface merged with a module augmentation has both members"]
fn a_module_augmented_interface_keeps_its_own_and_added_members() {
    let matrix = Matrix::new(IMPORTS).files(IMPORTS_FILES);
    let failures = matrix.types(&[
        ("ExportedI['base']", "\"b\""),
        ("ExportedI['augmented']", "\"aug\""),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
