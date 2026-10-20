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
