//! A namespace merged with a function or a class, a namespace declared in
//! several dotted blocks, and a namespace alias (`import X = N.M`), read
//! through the named-type demand a module's type alias takes.
//!
//! A namespace merged with a function or a class adds its value exports to
//! that value: `typeof f` and the class's static side carry them. A dotted
//! block (`namespace A.B { … }`) names a member another block of the same
//! namespace contributes (`A.B.C`) by its short name. An alias declared by
//! `import X = N.M` names the namespace `N.M`, in type and value position.
//!
//! Every expected answer is TypeScript 7.0.2's, measured on these exact
//! modules in all four `strictNullChecks` × `noImplicitAny` settings.

use std::sync::Arc;

use super::differential_harness_tests::{Matrix, ALL};
use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::{checker_syntax, render_node};

/// Every `(alias, checker print)` pair whose named demand, in any setting,
/// does not resolve to the checker's answer.
fn named_mismatches(source: &str, rows: &[(&str, &str)]) -> Vec<String> {
    let host = Matrix::new(source).settings(&ALL).host(&[]);
    let mut failures = Vec::new();
    for setting in ALL {
        let canonical = format!("{}/probe.ts", setting.root);
        for (name, answer) in rows {
            let result = host
                .resolve_named_symbol_with_audit(&canonical, name, None)
                .into_result()
                .expect("the named demand does not fault");
            let Some(node) = result else {
                failures.push(format!(
                    "`{name}` [{}]: the checker answers `{answer}`, the demand resolved nothing",
                    setting.label
                ));
                continue;
            };
            let store_view = host.resolver_store_view_read().into_owned_view();
            let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
            let host_ctx =
                crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
            let dispatch = super::ProjectSemanticDispatch::new(&host_ctx);
            let expected = checker_syntax::parse(answer)
                .unwrap_or_else(|err| panic!("the checker print `{answer}` must parse: {err}"));
            if !checker_syntax::matches_node(&dispatch, node.id(), &expected, 0) {
                failures.push(format!(
                    "`{name}` [{}]: the checker answers `{answer}`, the demand measured `{}`",
                    setting.label,
                    render_node(&dispatch, node.id(), 0)
                ));
            }
        }
    }
    failures
}

/// A namespace merged with a function adds its value exports to `typeof f`;
/// its type exports and the function's own call signature add no key.
///
/// Mutation: reading `typeof f` from the function declaration alone leaves
/// `keyof typeof f` `never`.
#[test]
fn a_namespace_merged_with_a_function_adds_its_values_to_typeof() {
    let source = r#"
type __BenchInit = 0;
type __D<T> = T extends unknown ? T : never;
function f(x: number) { return x > 0 ? "pos" as const : "neg" as const; }
namespace f { export const tag = "T" as const; export type Out = ReturnType<typeof f>; }
type __Probe = __D<keyof typeof f>;
type Tag = typeof f.tag;
"#;
    let failures = named_mismatches(source, &[("__Probe", "\"tag\""), ("Tag", "\"T\"")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A namespace merged with a class adds its value exports to the class's
/// static side, beside `prototype` and the class's own statics; its
/// interface adds no key.
///
/// Mutation: reading the static side from the class declaration alone
/// drops `make`.
#[test]
fn a_namespace_merged_with_a_class_adds_its_values_to_the_static_side() {
    let source = r#"
type __BenchInit = 0;
type __D<T> = T extends unknown ? T : never;
class K { a = 1 as const; static s = "s" as const; }
namespace K { export const make = () => new K(); export interface Opts { o: 1 } }
type __Probe = __D<keyof typeof K>;
"#;
    let failures = named_mismatches(source, &[("__Probe", "\"make\" | \"prototype\" | \"s\"")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A dotted namespace block names a namespace another block contributes to
/// the same merged namespace by its short name, in type and `typeof`
/// position.
///
/// Mutation: binding only a block's single-segment siblings leaves `C.T`
/// unresolved inside `namespace A.B`.
#[test]
fn a_dotted_namespace_block_reads_a_sibling_block_by_its_short_name() {
    let source = r#"
type __BenchInit = 0;
namespace A.B.C { export type T = 1; export const v = 2 as const; }
namespace A.B { export type U = [C.T, typeof C.v]; export type V = C.T; }
type __Probe = A.B.V;
type First = A.B.U[0];
type Second = A.B.U[1];
"#;
    let failures = named_mismatches(source, &[("__Probe", "1"), ("First", "1"), ("Second", "2")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `import X = N.M` names the namespace `N.M`: a type and a value read
/// through `X` read `N.M`'s members.
///
/// Mutation: leaving the alias unbound keeps `In.T['deep']` an unresolved
/// indexed access.
#[test]
fn a_namespace_alias_reads_the_namespace_it_names() {
    let source = r#"
type __BenchInit = 0;
namespace Outer { export namespace Inner { export type T = { deep: "yes" }; export const x = 5 as const; } }
import In = Outer.Inner;
type __Probe = In.T['deep'];
type X = typeof In.x;
"#;
    let failures = named_mismatches(source, &[("__Probe", "\"yes\""), ("X", "5")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Merges outside the function, class and alias forms keep their answers:
/// interface merging, an enum merged with a namespace, and a plain
/// namespace read.
#[test]
fn interface_enum_and_plain_namespace_merges_keep_their_answers() {
    let source = r#"
type __BenchInit = 0;
type __D<T> = T extends unknown ? T : never;
interface I { a: 1 }
interface I { b: 2 }
enum E { X = 1 }
namespace E { export const extra = "e" as const; }
namespace P { export type T = "p"; export const v = 3 as const; }
type Keys = __D<keyof I>;
type EnumKeys = __D<keyof typeof E>;
type Plain = P.T;
type PlainValue = typeof P.v;
"#;
    let failures = named_mismatches(
        source,
        &[
            ("Keys", "\"a\" | \"b\""),
            ("EnumKeys", "\"X\" | \"extra\""),
            ("Plain", "\"p\""),
            ("PlainValue", "3"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A script's dotted namespace block names a member another script's block
/// of the same global namespace contributes, by its short name, in type and
/// `typeof` position: global namespaces merge across scripts. A member of
/// the inner namespace shadows the outer namespace's member of that name,
/// whichever file declares it; any enclosing namespace's member shadows a
/// file-scope name of the declaring script; and an edit to the contributing
/// block reaches a warm read.
///
/// Measured on TypeScript 7.0.2 (`tsc --ignoreConfig --strict` over the two
/// scripts and a probe module, each read off a TS2322 against `never`):
/// `A.B.V` is `1`, `A.B.U[0]` `1`,
/// `A.B.U[1]` `2`, `A.B.W` `"outer"`, `A.B.Y` `"inner"`, `A.FP` `"inner"`,
/// `A.FD` `"innerD"`, `A.FV` `"inner"`, `A.B.Q` `"inner"`; with `T = 3` in
/// `b.ts`, `A.B.V` is `3`.
///
/// Mutation: binding only the declaring file's own blocks leaves `C.T`
/// unresolved inside `namespace A.B` and reads `X` as `A.X`; letting a
/// file-scope binding stand reads `A.FP` as `"file"` and `A.FD` as
/// `"fileD"`; letting the declaring file's outer-namespace value stand
/// reads `A.B.Q` as `"outer"`.
#[test]
fn a_dotted_block_reads_a_member_another_scripts_block_contributes() {
    use super::checker_probe_lane_tests::{
        mismatches_in, mismatches_on_host, probe_host, ProbeProject,
    };
    let declaring = "namespace A.B { export type V = C.T; export type U = [C.T, typeof C.v]; \
        export type W = O; }\n\
        namespace A { export type X = \"outer\"; }\n\
        namespace A.B { export type Y = X; }\n\
        type P = \"file\";\n\
        const fv = \"file\" as const;\n\
        namespace D { export type T = \"fileD\"; }\n\
        namespace A { export type FP = P; export type FD = D.T; export type FV = typeof fv; \
        export const w = \"outer\" as const; }\n\
        namespace A.B { export type Q = typeof w; }\n";
    let contributing = |t: &'static str| -> String {
        format!(
            "namespace A.B.C {{ export type T = {t}; export const v = 2 as const; }}\n\
             namespace A {{ export type O = \"outer\"; }}\n\
             namespace A.B {{ export type X = \"inner\"; export const w = \"inner\" as const; }}\n\
             namespace A {{ export type P = \"inner\"; export const fv = \"inner\" as const; \
             export namespace D {{ export type T = \"innerD\"; }} }}\n"
        )
    };
    let before = contributing("1");
    let after = contributing("3");
    let files = [("a.ts", declaring), ("b.ts", before.as_str())];
    let edited_files = [("a.ts", declaring), ("b.ts", after.as_str())];
    let project = |files| ProbeProject {
        files,
        compiler_options: None,
        ambient_lib: None,
    };
    let mut failures = mismatches_in(
        project(&files),
        "export {};\n",
        &[
            ("A.B.V", "1"),
            ("A.B.U[0]", "1"),
            ("A.B.U[1]", "2"),
            ("A.B.W", "\"outer\""),
            ("A.B.Y", "\"inner\""),
            ("A.FP", "\"inner\""),
            ("A.FD", "\"innerD\""),
            ("A.FV", "\"inner\""),
            ("A.B.Q", "\"inner\""),
        ],
    );
    let warm = probe_host(project(&files));
    failures.extend(mismatches_on_host(
        &warm,
        project(&files),
        "export {};\n",
        &[("A.B.V", "1")],
    ));
    crate::u6_flow_shape_corpus_tests::upsert(
        &warm,
        "/wb/b.ts",
        &after,
        crate::FileLanguage::script_ts(),
    );
    failures.extend(
        mismatches_on_host(
            &warm,
            project(&edited_files),
            "export {};\n",
            &[("A.B.V", "3")],
        )
        .into_iter()
        .map(|failure| format!("after the edit, warm: {failure}")),
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A class merged with another script's namespace that exports no value
/// still depends on that namespace: once an edit makes it export a value
/// (a new exported value, or `export` on a declared one), a warm host
/// answers the class's static keys as a cold build does.
///
/// Measured on TypeScript 7.0.2 (`tsc --ignoreConfig --strict` over the
/// two scripts): before each edit `ClassKeys` is `"prototype"`, after it
/// `"make" | "prototype"`.
///
/// Mutation: a warm read that keeps no dependency on the namespace's file
/// when the namespace merges nothing answers `"prototype"` after the edit.
#[test]
fn a_namespace_starting_to_export_a_value_misses_the_warm_class_read() {
    use super::checker_probe_lane_tests::{probe_host, ProbeProject};
    let class_script = "type __D<T> = T extends unknown ? T : never;\n\
        declare class K {}\n\
        type ClassKeys = __D<keyof typeof K>;\n";
    let edits = [
        (
            "declare namespace K { type T = 0 }\n",
            "declare namespace K { const make: 1 }\n",
        ),
        (
            "namespace K { const hidden = 1; }\n",
            "namespace K { const hidden = 1; export const make = 1; }\n",
        ),
        (
            "namespace K { const make = 1; }\n",
            "namespace K { export const make = 1; }\n",
        ),
    ];
    let read = |host: &Arc<crate::VerterHost>, expected: &str| {
        let node = host
            .resolve_named_symbol_with_audit("/wb/a.ts", "ClassKeys", None)
            .into_result()
            .expect("the named demand does not fault")
            .expect("`ClassKeys` resolves");
        let store_view = host.resolver_store_view_read().into_owned_view();
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
        let dispatch = super::ProjectSemanticDispatch::new(&host_ctx);
        let expected = checker_syntax::parse(expected).expect("the checker print parses");
        (
            render_node(&dispatch, node.id(), 0),
            checker_syntax::matches_node(&dispatch, node.id(), &expected, 0),
        )
    };
    let after = "\"make\" | \"prototype\"";
    let mut failures = Vec::new();
    for (namespace, edited) in edits {
        let files = [("a.ts", class_script), ("b.ts", namespace)];
        let edited_files = [("a.ts", class_script), ("b.ts", edited)];
        let project = |files| ProbeProject {
            files,
            compiler_options: None,
            ambient_lib: None,
        };
        let warm = probe_host(project(&files));
        let (before, matched) = read(&warm, "\"prototype\"");
        assert!(matched, "before editing `{namespace}`: {before}");
        crate::u6_flow_shape_corpus_tests::upsert(
            &warm,
            "/wb/b.ts",
            edited,
            crate::FileLanguage::script_ts(),
        );
        let (cold, matched) = read(&probe_host(project(&edited_files)), after);
        assert!(matched, "a cold build of `{edited}`: {cold}");
        let (warm_after, matched) = read(&warm, after);
        if !matched {
            failures.push(format!(
                "after editing `{namespace}` to `{edited}`, the warm host answers `{warm_after}`"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
