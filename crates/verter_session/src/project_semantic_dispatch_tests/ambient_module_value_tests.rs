//! A value imported from a module that only `declare module "…"` blocks
//! declare resolves to the block's declaration, as its types do: a named,
//! default or namespace import, an import assignment of a block assigning
//! `export =`, and the members a block's namespace, class or merged
//! function declares.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured on this
//! exact fixture with `tsc --ignoreConfig --declaration
//! --emitDeclarationOnly --strict --module commonjs`, each row asserted as
//! an exact type equality beside it; the four `strictNullChecks` ×
//! `noImplicitAny` settings answer alike.

use super::checker_probe_lane_tests::{mismatches_in, ProbeProject};

const ASSIGNED_NAMESPACE: &str = r#"
declare module "pkg" {
  namespace NS {
    interface Opt { k: 99 }
    type Box<T> = { value: T };
    namespace Inner { interface Item { k: 7 } }
  }
  export = NS;
}
declare module "other-pkg" {
  namespace NS { interface Opt { k: -1 } }
  export = NS;
}
"#;

#[test]
fn an_ambient_assigned_namespace_exports_its_types() {
    let files = [("assigned.d.ts", ASSIGNED_NAMESPACE)];
    let failures = mismatches_in(
        ProbeProject {
            files: &files,
            ..ProbeProject::default()
        },
        r#"
import type { Opt, Opt as Renamed, Box, Inner } from "pkg";
import * as Pkg from "pkg";
import type { Opt as OtherOpt } from "other-pkg";
"#,
        &[
            ("Opt[\"k\"]", "99"),
            ("Renamed[\"k\"]", "99"),
            ("Pkg.Opt[\"k\"]", "99"),
            ("Box<42>[\"value\"]", "42"),
            ("Pkg.Box<43>[\"value\"]", "43"),
            ("Inner.Item[\"k\"]", "7"),
            ("Pkg.Inner.Item[\"k\"]", "7"),
            ("OtherOpt[\"k\"]", "-1"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn an_ambient_assigned_namespace_type_tracks_contributor_edits() {
    use super::checker_probe_lane_tests::{probe_host, upsert_probe_files, with_probe_on_host};
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node;

    let files = [("assigned.d.ts", ASSIGNED_NAMESPACE)];
    let project = ProbeProject {
        files: &files,
        ..ProbeProject::default()
    };
    let host = probe_host(project);
    let source = r#"import type { Opt } from "pkg"; import * as Pkg from "pkg";"#;
    let dependent = ASSIGNED_NAMESPACE.replace(
        "interface Opt { k: 99 }",
        "interface Base { k: 99 } type Opt = Base;",
    );
    let edited = dependent.replace("k: 99", "k: 100");
    let retargeted = edited.replace(
        "export = NS",
        "namespace Other { interface Opt { k: 101 } } export = Other",
    );
    for probe in ["Opt[\"k\"]", "Pkg.Opt[\"k\"]"] {
        for (ambient, expected) in [
            (ASSIGNED_NAMESPACE, "99"),
            (dependent.as_str(), "99"),
            (edited.as_str(), "100"),
            (retargeted.as_str(), "101"),
            (ASSIGNED_NAMESPACE, "99"),
        ] {
            upsert_probe_files(&host, &[("assigned.d.ts", ambient)]);
            for _ in 0..2 {
                with_probe_on_host(&host, project, source, probe, |dispatch, node| {
                    assert_eq!(render_node(dispatch, node, 0), expected, "{probe}");
                });
            }
        }
    }
}

#[test]
fn an_ambient_assigned_namespace_resolves_sibling_types() {
    let files = [(
        "assigned.d.ts",
        r#"
declare module "pkg" {
  namespace NS {
    interface Base { k: 99 }
    type Opt = Base;
    interface Holder { value: Base }
    type Box<T> = { value: T };
    type Wrapped<T> = Box<T>;
    namespace Inner { type Opt = Base; }
  }
  export = NS;
}
declare module "other-pkg" {
  namespace NS { interface Base { k: -1 } type Opt = Base; }
  export = NS;
}
interface Base { k: 0 }
"#,
    )];
    let failures = mismatches_in(
        ProbeProject {
            files: &files,
            ..ProbeProject::default()
        },
        r#"import type { Opt, Holder, Wrapped, Inner } from "pkg";
import * as Pkg from "pkg";
import type { Opt as OtherOpt } from "other-pkg";"#,
        &[
            ("Opt[\"k\"]", "99"),
            ("Pkg.Opt[\"k\"]", "99"),
            ("Holder[\"value\"][\"k\"]", "99"),
            ("Pkg.Holder[\"value\"][\"k\"]", "99"),
            ("Wrapped<42>[\"value\"]", "42"),
            ("Pkg.Wrapped<43>[\"value\"]", "43"),
            ("Inner.Opt[\"k\"]", "99"),
            ("OtherOpt[\"k\"]", "-1"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn an_ambient_namespace_type_applies_generic_defaults() {
    let files = [(
        "defaults.d.ts",
        r#"
declare module "pkg" {
  namespace NS { type Box<T = 41, U = T> = { value: U }; }
  export = NS;
}
declare module "plain" { type Box<T = 41, U = T> = { value: U }; }
"#,
    )];
    let failures = mismatches_in(
        ProbeProject {
            files: &files,
            ..ProbeProject::default()
        },
        r#"import type { Box } from "pkg";
import * as Pkg from "pkg";
import type { Box as PlainBox } from "plain";"#,
        &[
            ("Box[\"value\"]", "41"),
            ("Pkg.Box[\"value\"]", "41"),
            ("Box<42>[\"value\"]", "42"),
            ("Pkg.Box<42, 43>[\"value\"]", "43"),
            ("PlainBox[\"value\"]", "41"),
            ("PlainBox<42>[\"value\"]", "42"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn an_ambient_namespace_recursive_sibling_remains_projectable() {
    let files = [(
        "recursive.d.ts",
        r#"
declare module "pkg" {
  namespace NS { interface Link { k: 99; next: Link } }
  export = NS;
}
"#,
    )];
    let failures = mismatches_in(
        ProbeProject {
            files: &files,
            ..ProbeProject::default()
        },
        r#"import type { Link } from "pkg"; import * as Pkg from "pkg";"#,
        &[
            ("Link[\"next\"][\"k\"]", "99"),
            ("Pkg.Link[\"next\"][\"k\"]", "99"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn an_ambient_assigned_namespace_resolves_alias_consumers() {
    let files = [(
        "assigned.d.ts",
        r#"
declare module "pkg" {
  namespace NS {
    interface Base { k: 99 }
    type Opt = Base;
    type Scalar = 99;
    type Alias = Scalar;
  }
  export = NS;
}
"#,
    )];
    let failures = mismatches_in(
        ProbeProject {
            files: &files,
            ..ProbeProject::default()
        },
        r#"import type { Base, Opt, Alias } from "pkg"; import * as Pkg from "pkg";"#,
        &[
            ("Opt extends { k: 99 } ? true : false", "true"),
            ("Pkg.Opt extends { k: 99 } ? true : false", "true"),
            ("Alias", "99"),
            ("Pkg.Alias", "99"),
            ("Base extends { k: 99 } ? true : false", "true"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn an_ambient_assigned_namespace_typeof_reads_its_lexical_value() {
    for global in ["", "declare const value: 0;"] {
        let ambient = format!(
            r#"
declare module "pkg" {{
  namespace NS {{ const value: 99; type Opt = {{ k: typeof value }}; }}
  export = NS;
}}
declare module "other-pkg" {{
  namespace NS {{ const value: 7; type Opt = {{ k: typeof value }}; }}
  export = NS;
}}
{global}
"#
        );
        let files = [("assigned.d.ts", ambient.as_str())];
        let failures = mismatches_in(
            ProbeProject {
                files: &files,
                ..ProbeProject::default()
            },
            r#"import type { Opt } from "pkg"; import * as Pkg from "pkg";
import type { Opt as OtherOpt } from "other-pkg";"#,
            &[
                ("Opt[\"k\"]", "99"),
                ("Pkg.Opt[\"k\"]", "99"),
                ("OtherOpt[\"k\"]", "7"),
            ],
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}

#[test]
fn an_ambient_assigned_namespace_exports_enum_member_types() {
    let files = [(
        "assigned.d.ts",
        r#"
declare module "pkg" { namespace NS { enum E { X = 99 } } export = NS; }
"#,
    )];
    let failures = mismatches_in(
        ProbeProject {
            files: &files,
            ..ProbeProject::default()
        },
        r#"import type { E } from "pkg"; import * as Pkg from "pkg";"#,
        &[
            ("E.X extends 99 ? true : false", "true"),
            ("Pkg.E.X extends 99 ? true : false", "true"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn an_ambient_assigned_namespace_exports_class_instance_types() {
    let files = [(
        "assigned.d.ts",
        r#"
declare module "pkg" {
  namespace NS { class Opt { k: 99 } }
  export = NS;
}
declare module "explicit" {
  namespace NS { export class Box<T = 41> { value: T } }
  export = NS;
}
"#,
    )];
    let failures = mismatches_in(
        ProbeProject {
            files: &files,
            ..ProbeProject::default()
        },
        r#"import type { Opt } from "pkg"; import * as Pkg from "pkg";
import type { Box } from "explicit"; import * as Explicit from "explicit";"#,
        &[
            ("Opt[\"k\"]", "99"),
            ("Pkg.Opt[\"k\"]", "99"),
            ("Box<42>[\"value\"]", "42"),
            ("Explicit.Box[\"value\"]", "41"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const AMBIENT: &str = "\
declare module \"amb\" {
  export function x(): \"x\";
  export function x(n: number): \"xn\";
  export interface T { t: \"t\" }
  export const k: \"k\";
  export let l: number;
  export class Cls { m(): \"cm\"; static s: \"cs\"; }
  export namespace AN { function f(): \"an-f\"; interface U { u: \"u\" } const c: \"an-c\"; }
  const hidden: \"h\";
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

const USE: &str = "\
import { x, k, l, Cls, AN, hidden } from 'amb';
import dflt from 'amb';
import type { T } from 'amb';
import * as A from 'amb';
import { y, z } from 'amb2';
import Q = require('eqmod');
import QD from 'eqmod';
export function cx() { return x(); }
export function cxn() { return x(1); }
export function rk() { return k; }
export function rl() { return l; }
export function cm() { const c = new Cls(); return c.m(); }
export function cs() { return Cls.s; }
export function rt(t: T) { return t.t; }
export function anf() { return AN.f(); }
export function anc() { return AN.c; }
export function df() { return dflt(); }
export function sx() { return A.x(); }
export function sk() { return A.k; }
export function sanf() { return A.AN.f(); }
export function cy() { return y(); }
export function rz() { return z; }
export function q() { return Q(); }
export function qw() { return Q.w; }
export function qd() { return QD(); }
export function h() { return hidden; }
";

/// Every import form of an ambient module's value reads the block's
/// declaration: overloads in order, a `let`'s declared type, a class's
/// instance and static members, a namespace's members, `default` the
/// block's `export default` declaration, and an import assignment (or a
/// default import) the value a block assigns with `export =`, whose merged
/// namespace declares its properties. A block without an export
/// declaration exports every declaration.
///
/// Measured on TypeScript 7.0.2: `cx` is `"x"`, `cxn` `"xn"`, `rk` `"k"`,
/// `rl` `number`, `cm` `"cm"`, `cs` `"cs"`, `rt` `"t"`, `anf` `"an-f"`,
/// `anc` `"an-c"`, `df` `"d"`, `sx` `"x"`, `sk` `"k"`, `sanf` `"an-f"`,
/// `cy` `"y"`, `rz` `"z"`, `q` `"q"`, `qw` `"w"`, `qd` `"q"`, `h` `"h"`;
/// `typeof k` is `"k"` and `ReturnType<typeof x>` (the last overload)
/// `"xn"`.
///
/// Mutation: resolving no import of an ambient module to the block's member
/// leaves every named, default and import-assignment row a typed miss while
/// `rt` (a type) still answers; addressing only a block's `declare global`
/// values by the declaring file's identity leaves every value row a typed
/// miss.
#[test]
fn an_ambient_module_value_resolves_to_its_declaration() {
    let files = [("amb.d.ts", AMBIENT)];
    let failures = mismatches_in(
        ProbeProject {
            files: &files,
            compiler_options: None,
            ambient_lib: None,
        },
        USE,
        &[
            ("ReturnType<typeof cx>", "\"x\""),
            ("ReturnType<typeof cxn>", "\"xn\""),
            ("ReturnType<typeof rk>", "\"k\""),
            ("ReturnType<typeof rl>", "number"),
            ("ReturnType<typeof cm>", "\"cm\""),
            ("ReturnType<typeof cs>", "\"cs\""),
            ("ReturnType<typeof rt>", "\"t\""),
            ("ReturnType<typeof anf>", "\"an-f\""),
            ("ReturnType<typeof anc>", "\"an-c\""),
            ("ReturnType<typeof df>", "\"d\""),
            ("ReturnType<typeof sx>", "\"x\""),
            ("ReturnType<typeof sk>", "\"k\""),
            ("ReturnType<typeof sanf>", "\"an-f\""),
            ("ReturnType<typeof cy>", "\"y\""),
            ("ReturnType<typeof rz>", "\"z\""),
            ("ReturnType<typeof q>", "\"q\""),
            ("ReturnType<typeof qw>", "\"w\""),
            ("ReturnType<typeof qd>", "\"q\""),
            ("ReturnType<typeof h>", "\"h\""),
            ("typeof k", "\"k\""),
            ("ReturnType<typeof x>", "\"xn\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
