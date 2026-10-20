//! A chain of instantiations evaluates as a chain of continuation frames,
//! not of native calls: an alias whose body applies the next alias needs
//! that instantiation from the runtime, which parks the frame on the heap
//! while the next one builds. The answer no longer depends on how deep the
//! chain is — neither the stack the caller runs on nor the nested-query
//! depth the connected demand counts for native re-entry decides it.
//!
//! Past the checker's own instantiation depth Verter keeps evaluating and
//! answers; it reports the checker's TS2589 — the same code and message —
//! only where its own instantiation budget runs out, far past the
//! checker's.
//!
//! Every checker answer quoted is TypeScript 7.0.2's, measured through the
//! capped `tsc` with `--ignoreConfig`.

use super::checker_probe_lane_tests::{
    evaluated_mismatches, in_a_fresh_process, test_path, with_probe_in, ProbeProject,
};
use verter_type_engine::semantic_query::{CheckerDiagnosticCode, QueryError, SemanticNodeData};

/// A chain of `length` aliases down to `T | undefined`, a generic function
/// declared to return its end, and the call's result kept as a `const` and
/// joined with a pinned `"ok"` in a return.
fn alias_chain(length: usize) -> String {
    let mut source = String::from("type A0<T> = T | undefined;\n");
    for level in 1..=length {
        source.push_str(&format!("type A{level}<T> = A{}<T>;\n", level - 1));
    }
    source.push_str(&format!(
        "declare function f<T>(x: T): A{length}<T>;\n\
         export const c = f(\"ok\");\n\
         export function gj(b: boolean) {{ if (b) {{ return f(\"ok\"); }} return \"ok\" as const; }}\n"
    ));
    source
}

/// A chain of `length` aliases each applying the one below to a conditional
/// over its own parameter: `E<i><X> = E<i-1><E0<X>>`.
fn conditional_argument_chain(length: usize) -> String {
    let mut source = String::from("type E0<X> = X extends string ? X : never;\n");
    for level in 1..=length {
        source.push_str(&format!("type E{level}<X> = E{}<E0<X>>;\n", level - 1));
    }
    source
}

/// `f` over the evaluated rows, read on a thread of
/// [`super::deep_input_tests::SMALL_STACK`] — 640 KiB, where a probe's
/// fixed work takes about 416 to 480 KiB unoptimized — so a chain
/// evaluated one native level per link has at most 224 KiB to overflow
/// within: under half a KiB a link at 500 links. The tests that read on it
/// run in a fresh process ([`in_a_fresh_process`]), so the thread has
/// exactly that stack and an overflow fails the one test.
fn on_a_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(super::deep_input_tests::SMALL_STACK)
        .spawn(f)
        .expect("spawn the probing thread")
        .join()
        .expect("the probe answers")
}

/// `f` on the calling thread: the tests whose claim is a budget or the
/// memo, not the stack, read where the runner put them.
fn on_this_thread<T>(f: impl FnOnce() -> T) -> T {
    f()
}

const ALIAS_CHAIN_ROWS: &[(&str, &str)] = &[
    ("typeof c", "\"ok\" | undefined"),
    ("ReturnType<typeof gj>", "\"ok\" | undefined"),
];

fn named_chain_answers(
    host: &crate::VerterHost,
    source: &str,
    probe: &str,
    check: impl Fn(&verter_type_expr::TypeExpr),
) {
    let canonical = super::checker_probe_lane_tests::PROBE_FILE;
    let _update = host
        .upsert(crate::UpsertRequest {
            canonical_id: Some(canonical.into()),
            input_id: canonical.into(),
            source: std::sync::Arc::from(format!(
                "{source}\ntype __Probe = {probe};\nexport {{}};\n"
            )),
            file_language: crate::FileLanguage::script_ts(),
            aliases: vec![],
        })
        .expect("the named fixture indexes");
    let _init = host
        .resolve_named_symbol(canonical, "__BenchInit", None)
        .expect("the initialization alias exists");
    let wires = [(); 2].map(|()| {
        let node = host
            .resolve_named_symbol(canonical, "__Probe", None)
            .expect("the probe exists");
        let wire = host
            .project_node_to_type_expr_json_bytes(&node)
            .expect("the answer projects");
        let answer = serde_json::from_slice(&wire).expect("a wire TypeExpr");
        check(&answer);
        wire
    });
    assert_eq!(wires[0], wires[1], "cold and warm named answers agree");
}

#[test]
fn indexed_paths_past_native_query_depth_answer_cold_and_warm() {
    indexed_path_answers(23);
}

#[test]
fn indexed_paths_past_native_query_depth_answer_cold_and_warm_control_22() {
    indexed_path_answers(22);
}

#[test]
fn indexed_paths_past_native_query_depth_answer_cold_and_warm_50() {
    indexed_path_answers(50);
}

#[test]
fn indexed_paths_past_native_query_depth_answer_cold_and_warm_200() {
    indexed_path_answers(200);
}

fn indexed_path_answers(depth: usize) {
    fn assert_leaf(answer: &verter_type_expr::TypeExpr) {
        assert!(
            matches!(answer, verter_type_expr::TypeExpr::Literal(verter_type_expr::LiteralValue::String(value)) if value == "leaf"),
            "{answer:?}"
        );
    }
    let mut source = String::from("type __BenchInit = 0;\n");
    for level in 0..depth {
        let value = if level + 1 == depth {
            "'leaf'".to_string()
        } else {
            format!("L{}", level + 1)
        };
        source.push_str(&format!("interface L{level} {{ k0: {value} }}\n"));
    }
    let probe = format!("L0{}", "['k0']".repeat(depth));
    let displays = cold_and_warm_displays(&source, &probe);
    assert_eq!(displays, ["\"leaf\"", "\"leaf\""], "depth {depth}");
    named_chain_answers(
        &super::checker_probe_lane_tests::default_probe_host(),
        &source,
        &probe,
        assert_leaf,
    );
    if depth == 23 {
        let computed_source = source.replace("{ k0:", "{ k1: never; k0:");
        let computed_probe = format!("L0{}", "['k0' | 'k1']".repeat(depth));
        named_chain_answers(
            &super::checker_probe_lane_tests::default_probe_host(),
            &computed_source,
            &computed_probe,
            assert_leaf,
        );
    }
}

#[test]
fn typeof_import_chains_past_native_query_depth_answer_cold_and_warm() {
    import_chain_displays(50);
    import_chain_named_answers(50, ImportChainKind::Direct);
}

#[test]
fn typeof_import_chains_past_native_query_depth_answer_cold_and_warm_control_22() {
    import_chain_displays(22);
    import_chain_named_answers(22, ImportChainKind::Direct);
}

#[test]
fn typeof_import_chains_past_native_query_depth_answer_cold_and_warm_200_display() {
    import_chain_displays(200);
}

#[test]
fn typeof_import_chains_past_native_query_depth_answer_cold_and_warm_200_named() {
    import_chain_named_answers(200, ImportChainKind::Direct);
}

#[test]
fn namespace_import_chains_answer_cold_and_warm_control_1() {
    import_chain_named_answers(1, ImportChainKind::NamespaceReexport);
}

#[test]
fn namespace_import_chains_answer_cold_and_warm_23() {
    import_chain_named_answers(23, ImportChainKind::NamespaceReexport);
}

#[test]
fn namespace_import_chains_answer_cold_and_warm_50() {
    import_chain_named_answers(50, ImportChainKind::NamespaceReexport);
}

#[test]
fn declared_namespace_import_chains_answer_cold_and_warm_control_1() {
    import_chain_named_answers(1, ImportChainKind::DeclaredNamespace);
}

#[test]
fn declared_namespace_import_chains_answer_cold_and_warm_23() {
    import_chain_named_answers(23, ImportChainKind::DeclaredNamespace);
}

#[test]
fn indexed_namespace_import_chains_answer_cold_and_warm_control_1() {
    import_chain_named_answers(1, ImportChainKind::IndexedNamespaceReexport);
}

#[test]
fn indexed_namespace_import_chains_answer_cold_and_warm_23() {
    import_chain_named_answers(23, ImportChainKind::IndexedNamespaceReexport);
}

#[test]
fn declared_namespace_named_import_chains_answer_cold_and_warm_23() {
    import_chain_named_answers(23, ImportChainKind::NamedDeclaredNamespace);
}

#[test]
fn indexed_declared_namespace_import_chains_answer_cold_and_warm_23() {
    import_chain_named_answers(23, ImportChainKind::IndexedDeclaredNamespace);
}

#[test]
fn computed_indexed_namespace_import_chains_answer_cold_and_warm_23() {
    import_chain_named_answers(23, ImportChainKind::ComputedIndexedNamespaceReexport);
}

#[derive(Clone, Copy)]
enum ImportChainKind {
    Direct,
    NamespaceReexport,
    DeclaredNamespace,
    IndexedNamespaceReexport,
    NamedDeclaredNamespace,
    IndexedDeclaredNamespace,
    ComputedIndexedNamespaceReexport,
}

fn import_chain_files(depth: usize, kind: ImportChainKind) -> Vec<(String, String)> {
    let mut files = vec![(
        "m0.ts".to_string(),
        "export declare const value: { deep: 1 };".to_string(),
    )];
    for level in 1..=depth {
        let import = match kind {
            ImportChainKind::Direct => format!("typeof import('./m{}').value", level - 1),
            ImportChainKind::NamespaceReexport
            | ImportChainKind::IndexedNamespaceReexport
            | ImportChainKind::ComputedIndexedNamespaceReexport => {
                files.push((
                    format!("ns{level}.ts"),
                    format!("export * as ns from './m{}';", level - 1),
                ));
                if matches!(kind, ImportChainKind::ComputedIndexedNamespaceReexport) {
                    format!("(typeof import('./ns{level}').ns)[SelectedKey]")
                } else if matches!(kind, ImportChainKind::IndexedNamespaceReexport) {
                    format!("(typeof import('./ns{level}').ns)['value']")
                } else {
                    format!("typeof import('./ns{level}').ns.value")
                }
            }
            ImportChainKind::DeclaredNamespace
            | ImportChainKind::NamedDeclaredNamespace
            | ImportChainKind::IndexedDeclaredNamespace => {
                files.push((
                    format!("ns{level}.ts"),
                    format!(
                        "export declare namespace ns {{ export const value: typeof import('./m{}').value; }}",
                        level - 1
                    ),
                ));
                match kind {
                    ImportChainKind::NamedDeclaredNamespace => "typeof ns.value".to_string(),
                    ImportChainKind::IndexedDeclaredNamespace => {
                        format!("(typeof import('./ns{level}').ns)['value']")
                    }
                    _ => format!("typeof import('./ns{level}').ns.value"),
                }
            }
        };
        let prelude = match kind {
            ImportChainKind::NamedDeclaredNamespace => {
                format!("import {{ ns }} from './ns{level}';")
            }
            ImportChainKind::ComputedIndexedNamespaceReexport => {
                "type SelectedKey = 'value';".to_string()
            }
            _ => String::new(),
        };
        files.push((
            format!("m{level}.ts"),
            format!("{prelude} export declare const value: {import};"),
        ));
    }
    files
}

fn import_chain_displays(depth: usize) {
    use super::checker_probe_lane_tests::{probe_host, with_probe_on_host};
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node;
    let files = import_chain_files(depth, ImportChainKind::Direct);
    let files: Vec<_> = files
        .iter()
        .map(|(name, text)| (name.as_str(), text.as_str()))
        .collect();
    let project = ProbeProject {
        files: &files,
        ..Default::default()
    };
    let host = probe_host(project);
    let probe = format!("typeof import('./m{depth}').value");
    let displays = [(); 2].map(|()| {
        with_probe_on_host(
            &host,
            project,
            "type __BenchInit = 0;",
            &probe,
            |dispatch, node| render_node(dispatch, node, 0),
        )
    });
    assert_eq!(displays, ["{ deep: 1 }", "{ deep: 1 }"], "depth {depth}");
}

fn import_chain_named_answers(depth: usize, kind: ImportChainKind) {
    use super::checker_probe_lane_tests::probe_host;
    let files = import_chain_files(depth, kind);
    let files: Vec<_> = files
        .iter()
        .map(|(name, text)| (name.as_str(), text.as_str()))
        .collect();
    let project = ProbeProject {
        files: &files,
        ..Default::default()
    };
    let probe = format!("typeof import('./m{depth}').value");
    named_chain_answers(
        &probe_host(project),
        "type __BenchInit = 0;",
        &probe,
        |answer| {
            let verter_type_expr::TypeExpr::Object(object) = answer else {
                panic!("{answer:?}")
            };
            let [verter_type_expr::ObjectMember::Property(property)] = object.properties.as_slice()
            else {
                panic!("{object:?}")
            };
            assert!(
                matches!(&property.key, verter_type_expr::AuthoredPropertyKey::String(name) if name.as_ref() == "deep")
            );
            assert!(
                matches!(&property.ty, verter_type_expr::TypeExpr::Literal(verter_type_expr::LiteralValue::Number(value)) if *value == 1.0)
            );
            assert!(!property.optional && !property.readonly);
        },
    );
}

#[test]
fn applied_imported_typeof_keeps_type_arguments_in_the_callers_scope() {
    use super::checker_probe_lane_tests::probe_host;
    let project = ProbeProject {
        files: &[(
            "generic.ts",
            "type Local = never; export declare function make<T>(): T;",
        )],
        ..Default::default()
    };
    named_chain_answers(
        &probe_host(project),
        "type __BenchInit = 0; type Local = { deep: 1 };",
        "ReturnType<typeof import('./generic').make<Local>>['deep']",
        |answer| {
            assert!(
                matches!(answer, verter_type_expr::TypeExpr::Literal(verter_type_expr::LiteralValue::Number(value)) if *value == 1.0),
                "{answer:?}"
            );
        },
    );
}

/// Alias chains of 200 and 500 aliases evaluate cold, to the checker's
/// answer, on the small stack. Each alias's instantiation needs the next
/// one from the runtime instead of opening it one native query deeper, so
/// the chain is bounded neither by the stack nor by the nested-query depth
/// that bounds native re-entry (a chain of 23 used to exceed it).
///
/// TypeScript 7.0.2, all four `strictNullChecks` × `noImplicitAny` settings,
/// at 50, 200 and 500 aliases: no TS2589; under `strictNullChecks`
/// `typeof c` is `"ok" | undefined` and `gj` returns `"ok" | undefined`.
#[test]
fn alias_chains_up_to_500_long_answer_cold_on_a_small_stack() {
    in_a_fresh_process(test_path!(), || {
        for (length, rows) in [(200, ALIAS_CHAIN_ROWS), (500, &ALIAS_CHAIN_ROWS[..1])] {
            let mismatches =
                on_a_small_stack(move || evaluated_mismatches(&alias_chain(length), rows));
            assert_eq!(
                mismatches,
                Vec::<String>::new(),
                "a chain of {length} aliases"
            );
        }
    });
}

/// `f` over the evaluated rows, read on a 1 MiB thread — the least a host
/// asks from.
fn on_a_one_mebibyte_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(f)
        .expect("spawn the probing thread")
        .join()
        .expect("the probe answers")
}

/// `probe` over `source` displayed twice on one host: cold, then warm from
/// what the cold evaluation kept.
fn cold_and_warm_displays(source: &str, probe: &str) -> [String; 2] {
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node;
    use verter_type_engine::semantic_query::{ProjectionMode, ProjectionReductionContext};
    let host = super::checker_probe_lane_tests::default_probe_host();
    [(); 2].map(|()| {
        super::checker_probe_lane_tests::with_probe_on_host(
            &host,
            Default::default(),
            source,
            probe,
            |dispatch, node| {
                let value = dispatch
                    .normalize_node_for_structural_fact_demand(
                        node,
                        ProjectionReductionContext::published(ProjectionMode::Expanded),
                    )
                    .into_usable_node()
                    .expect("the probe answers");
                render_node(dispatch, value, 0)
            },
        )
    })
}

/// The call's return through 500 aliases answers as the shorter chains do,
/// every row, on a 1 MiB thread, in a fresh process — and the same again
/// warm on the host the cold evaluation filled.
///
/// TypeScript 7.0.2 (all four settings at 500): no TS2589; under
/// `strictNullChecks` `typeof c` and `gj`'s return are `"ok" | undefined`.
#[test]
fn alias_chains_500_long_answer_every_row_cold_and_warm_on_a_one_mebibyte_stack() {
    in_a_fresh_process(test_path!(), || {
        let (mismatches, displays) = on_a_one_mebibyte_stack(|| {
            let source = alias_chain(500);
            (
                evaluated_mismatches(&source, ALIAS_CHAIN_ROWS),
                cold_and_warm_displays(&source, "ReturnType<typeof gj>"),
            )
        });
        assert_eq!(mismatches, Vec::<String>::new(), "a chain of 500 aliases");
        assert_eq!(displays[0], displays[1], "cold and warm answer alike");
    });
}

/// A chain of 500 values each declared `typeof` the one before evaluates
/// on a 1 MiB thread, cold and warm alike: each value's `typeof` builds as
/// a frame whose annotation needs the `typeof` it names from the runtime,
/// instead of evaluating it one native level deeper.
///
/// TypeScript 7.0.2: `typeof v500` is `"ok"`.
#[test]
fn typeof_chains_500_deep_answer_cold_and_warm_on_a_one_mebibyte_stack() {
    in_a_fresh_process(test_path!(), || {
        let mut source = String::from("declare const v0: \"ok\";\n");
        for level in 1..=500 {
            source.push_str(&format!("declare const v{level}: typeof v{};\n", level - 1));
        }
        let displays = on_a_one_mebibyte_stack(move || {
            assert_eq!(
                evaluated_mismatches(&source, &[("typeof v500", "\"ok\"")]),
                Vec::<String>::new()
            );
            cold_and_warm_displays(&source, "typeof v500")
        });
        assert_eq!(displays[0], displays[1], "cold and warm answer alike");
    });
}

/// A chain of 500 constants each initialized from the one before evaluates
/// on a 1 MiB thread, cold and warm alike: each constant's `typeof` lowers
/// its initializer's inferred type as a frame that needs the `typeof` it
/// names from the runtime.
///
/// TypeScript 7.0.2: `typeof x500` is `"ok"`.
#[test]
fn initializer_chains_500_deep_answer_cold_and_warm_on_a_one_mebibyte_stack() {
    in_a_fresh_process(test_path!(), || {
        let mut source = String::from("const x0 = \"ok\" as const;\n");
        for level in 1..=500 {
            source.push_str(&format!("const x{level} = x{};\n", level - 1));
        }
        let displays = on_a_one_mebibyte_stack(move || {
            assert_eq!(
                evaluated_mismatches(&source, &[("typeof x500", "\"ok\"")]),
                Vec::<String>::new()
            );
            cold_and_warm_displays(&source, "typeof x500")
        });
        assert_eq!(displays[0], displays[1], "cold and warm answer alike");
    });
}

/// A chain of 500 constants each initialized by a call over the one before
/// evaluates on a 1 MiB thread, cold and warm alike: each constant's
/// initializer evaluates as its `typeof`'s frame, which needs the `typeof`
/// of the value its argument names from the runtime instead of evaluating
/// it in place.
///
/// TypeScript 7.0.2, `--strict` and with `strictNullChecks` and
/// `noImplicitAny` off: `typeof y500` is `"ok"`.
#[test]
fn call_initializer_chains_500_deep_answer_cold_and_warm_on_a_one_mebibyte_stack() {
    in_a_fresh_process(test_path!(), || {
        let mut source =
            String::from("declare function id<T>(x: T): T;\nconst y0 = \"ok\" as const;\n");
        for level in 1..=500 {
            source.push_str(&format!("const y{level} = id(y{});\n", level - 1));
        }
        let displays = on_a_one_mebibyte_stack(move || {
            assert_eq!(
                evaluated_mismatches(&source, &[("typeof y500", "\"ok\"")]),
                Vec::<String>::new()
            );
            cold_and_warm_displays(&source, "typeof y500")
        });
        assert_eq!(displays[0], displays[1], "cold and warm answer alike");
    });
}

/// Chains of 500 values whose member reads the same member of the one
/// before — object literals, and classes' statics — evaluate on a 1 MiB
/// thread, cold and warm alike: a member path's `typeof` lowers the member
/// where the value declares it as its frame's projection, which needs the
/// member-path `typeof` its value names from the runtime instead of
/// evaluating it in place.
///
/// TypeScript 7.0.2, `--strict` and with `strictNullChecks` and
/// `noImplicitAny` off: `typeof y500.v` and `typeof C500.v` are `"ok"`.
#[test]
fn member_path_typeof_chains_500_deep_answer_cold_and_warm_on_a_one_mebibyte_stack() {
    in_a_fresh_process(test_path!(), || {
        let mut literals = String::from("const y0 = { v: \"ok\" as const };\n");
        let mut statics = String::from("class C0 { static v = \"ok\" as const; }\n");
        for level in 1..=500 {
            literals.push_str(&format!("const y{level} = {{ v: y{}.v }};\n", level - 1));
            statics.push_str(&format!(
                "class C{level} {{ static v = C{}.v; }}\n",
                level - 1
            ));
        }
        let displays = on_a_one_mebibyte_stack(move || {
            [(literals, "typeof y500.v"), (statics, "typeof C500.v")].map(|(source, probe)| {
                assert_eq!(
                    evaluated_mismatches(&source, &[(probe, "\"ok\"")]),
                    Vec::<String>::new(),
                    "{probe}"
                );
                (probe, cold_and_warm_displays(&source, probe))
            })
        });
        for (probe, [cold, warm]) in displays {
            assert_eq!(cold, warm, "{probe}: cold and warm answer alike");
        }
    });
}

/// A chain of 500 functions each returning the type the one before returns
/// evaluates on a 1 MiB thread, cold and warm alike: a function's `typeof`
/// lowers its signatures one at a time as its frame's projections, which
/// need the instantiations and `typeof`s they reach from the runtime.
///
/// TypeScript 7.0.2, `--strict` and with `strictNullChecks` and
/// `noImplicitAny` off: `ReturnType<typeof h500>` is `"ok"`.
#[test]
fn signature_chains_500_deep_answer_cold_and_warm_on_a_one_mebibyte_stack() {
    in_a_fresh_process(test_path!(), || {
        let mut source = String::from("function h0(): \"ok\" { return \"ok\"; }\n");
        for level in 1..=500 {
            source.push_str(&format!(
                "declare function h{level}(): ReturnType<typeof h{}>;\n",
                level - 1
            ));
        }
        let displays = on_a_one_mebibyte_stack(move || {
            assert_eq!(
                evaluated_mismatches(&source, &[("ReturnType<typeof h500>", "\"ok\"")]),
                Vec::<String>::new()
            );
            cold_and_warm_displays(&source, "ReturnType<typeof h500>")
        });
        assert_eq!(displays[0], displays[1], "cold and warm answer alike");
    });
}

/// A chain of `length` non-generic aliases, each a conditional over the one
/// before whose true branch is `branch` of the one before:
/// `C<i> = C<i-1> extends string ? branch(i-1) : never`.
fn conditional_chain(length: usize, branch: impl Fn(usize) -> String) -> String {
    let mut source = String::from("type C0 = \"ok\";\n");
    for level in 1..=length {
        let p = level - 1;
        source.push_str(&format!(
            "type C{level} = C{p} extends string ? {} : never;\n",
            branch(p)
        ));
    }
    source
}

/// Chains of non-generic conditional aliases, each deciding over the one
/// before, evaluate on a 1 MiB thread, cold and warm alike: deciding a
/// conditional needs its operands' instantiations from the runtime instead
/// of normalizing them in place one native level deeper. Five hundred
/// levels whose branch is a literal; a hundred whose branch selects the
/// alias before, which a decided branch keeps as a carrier, so each level's
/// decision walks the carriers below it — that walk, not the runtime, bounds
/// its length here.
///
/// TypeScript 7.0.2, `--strict` and with `strictNullChecks` and
/// `noImplicitAny` off: `C500` over the literal branches and `C100` over the
/// alias branches are `"ok"`.
#[test]
fn conditional_chains_decide_cold_and_warm_on_a_one_mebibyte_stack() {
    in_a_fresh_process(test_path!(), || {
        let displays = on_a_one_mebibyte_stack(|| {
            [
                (conditional_chain(500, |_| "\"ok\"".to_owned()), "C500"),
                (conditional_chain(100, |p| format!("C{p}")), "C100"),
            ]
            .map(|(source, probe)| (probe, cold_and_warm_displays(&source, probe)))
        });
        for (probe, [cold, warm]) in displays {
            assert_eq!(cold, "\"ok\"", "{probe} decides cold");
            assert_eq!(warm, cold, "{probe} decides warm as it does cold");
        }
    });
}

/// A chain alternating a member projection, an instantiation, a flow return
/// and a call at every level — `g<i>` returns `id(b["v"])` over
/// `b: B<i-1>`, and `B<i>` projects `ReturnType<typeof g<i>>` — answers on
/// a 1 MiB thread, cold and warm alike.
///
/// TypeScript 7.0.2: `ReturnType<typeof g150>` is `"ok"`.
#[test]
fn alternating_projection_instantiation_flow_and_call_chains_answer_on_a_one_mebibyte_stack() {
    in_a_fresh_process(test_path!(), || {
        const LENGTH: usize = 150;
        let mut source = String::from(
            "declare function id<T>(x: T): T;\n\
             function g0() { return \"ok\" as const; }\n\
             type B0 = { v: ReturnType<typeof g0> };\n",
        );
        for level in 1..=LENGTH {
            source.push_str(&format!(
                "function g{level}(b: B{p}) {{ return id(b[\"v\"]); }}\n\
                 type B{level} = {{ v: ReturnType<typeof g{level}> }};\n",
                p = level - 1
            ));
        }
        let probe = format!("ReturnType<typeof g{LENGTH}>");
        let displays = on_a_one_mebibyte_stack(move || {
            assert_eq!(
                evaluated_mismatches(&source, &[(probe.as_str(), "\"ok\"")]),
                Vec::<String>::new()
            );
            cold_and_warm_displays(&source, &probe)
        });
        assert_eq!(displays[0], displays[1], "cold and warm answer alike");
    });
}

/// The four `strictNullChecks` × `noImplicitAny` settings.
const SETTINGS: [&str; 4] = [
    r#"{ "strictNullChecks": true, "noImplicitAny": true }"#,
    r#"{ "strictNullChecks": true, "noImplicitAny": false }"#,
    r#"{ "strictNullChecks": false, "noImplicitAny": true }"#,
    r#"{ "strictNullChecks": false, "noImplicitAny": false }"#,
];

/// `probe` over `source`, checked under `options` on the calling thread —
/// under an instantiation budget of `budget` when set — read by `read`
/// with the probe's evaluated node. A test whose claim is the stack calls
/// it from [`on_a_small_stack`].
fn evaluated_in<R>(
    budget: Option<u32>,
    options: &'static str,
    source: String,
    probe: String,
    read: impl FnOnce(
        &super::ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
        verter_type_engine::semantic_query::SemanticNodeId,
    ) -> R,
) -> R {
    use verter_type_engine::semantic_query::{ProjectionMode, ProjectionReductionContext};
    let _budget = budget.map(super::connected_demand::InstantiationBudgetForTests::install);
    with_probe_in(
        ProbeProject {
            compiler_options: Some(options),
            ..Default::default()
        },
        &source,
        &probe,
        |dispatch, node| {
            let value = dispatch
                .normalize_node_for_structural_fact_demand(
                    node,
                    ProjectionReductionContext::published(ProjectionMode::Expanded),
                )
                .into_usable_node()
                .expect("the probe answers");
            read(dispatch, value)
        },
    )
}

/// `probe`'s evaluated type as displayed, `Ok` when it is `checker`.
fn displayed_in(
    budget: Option<u32>,
    options: &'static str,
    source: String,
    probe: &str,
    checker: &'static str,
) -> Result<String, String> {
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::{checker_syntax, render_node};
    evaluated_in(
        budget,
        options,
        source,
        probe.to_owned(),
        move |dispatch, value| {
            let expected = checker_syntax::parse(checker).expect("the checker type parses");
            let shown = render_node(dispatch, value, 0);
            if checker_syntax::matches_node(dispatch, value, &expected, 0) {
                Ok(shown)
            } else {
                Err(shown)
            }
        },
    )
}

/// The TS2589 diagnostic `probe` evaluates to, if it is a checker
/// recovery.
fn recovery_in(
    budget: Option<u32>,
    options: &'static str,
    source: String,
    probe: &str,
) -> Option<verter_type_engine::semantic_query::CheckerDiagnostic> {
    evaluated_in(
        budget,
        options,
        source,
        probe.to_owned(),
        |dispatch, value| match dispatch.graph().node_data(value).as_deref() {
            Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery { diagnostic, .. })) => {
                Some(*diagnostic)
            }
            _ => None,
        },
    )
}

/// `E<length><"ok">` answers `"ok"` under each of `settings`.
fn check_conditional_argument_chains_answer(lengths: &[usize], settings: &[&'static str]) {
    for &length in lengths {
        for &options in settings {
            let answer = displayed_in(
                None,
                options,
                conditional_argument_chain(length),
                &format!("E{length}<\"ok\">"),
                "\"ok\"",
            );
            assert!(
                answer.is_ok(),
                "E{length}<\"ok\"> under {options} is \"ok\": {answer:?}"
            );
        }
    }
}

/// Conditional arguments nested through a chain of aliases evaluate on the
/// small stack up to the checker's instantiation depth.
///
/// TypeScript 7.0.2, all four `strictNullChecks` × `noImplicitAny`
/// settings: `E97<"ok">` is `"ok"`; `E98<"ok">` is TS2589.
#[test]
fn conditional_arguments_nested_97_deep_answer_on_a_small_stack() {
    in_a_fresh_process(test_path!(), || {
        on_a_small_stack(|| check_conditional_argument_chains_answer(&[97], &SETTINGS));
    });
}

/// Past the checker's instantiation depth Verter keeps evaluating: the
/// declared alias type — a conditional nested once per alias — is
/// evaluated to its answer, where the checker gives up.
///
/// TypeScript 7.0.2, all four settings: `E98<"ok">` and `E200<"ok">` are
/// TS2589, the checker's limit; Verter's full answer is `"ok"`, as at 97.
#[test]
fn conditional_arguments_nested_past_the_checker_depth_answer() {
    check_conditional_argument_chains_answer(&[98, 200], &SETTINGS);
}

/// Five hundred levels deep, the same full answer, on the small stack —
/// well inside Verter's instantiation budget, and far past the depth at
/// which an instantiation opened one native level per alias would have
/// overflowed it (224 KiB past a probe's fixed work: under half a KiB a
/// level). The settings do not change what the frames hold, so the strict
/// one stands for the four.
///
/// TypeScript 7.0.2, all four settings: `E500<"ok">` is TS2589; Verter's
/// full answer is `"ok"`.
#[test]
fn conditional_arguments_nested_500_deep_answer_on_a_small_stack() {
    in_a_fresh_process(test_path!(), || {
        on_a_small_stack(|| check_conditional_argument_chains_answer(&[500], &SETTINGS[..1]));
    });
}

/// Verter's instantiation budget, pinned at three points on one chain:
/// the checker's limit, where Verter answers; just inside Verter's budget,
/// where it still answers; and one level past it, where it reports the
/// checker's TS2589 — the checker's code and message — as a typed
/// recovery, having held at most the budget's frames. A lower budget than
/// production's stands in for it on the same path, so the chain stays
/// quick to evaluate: an `E<n>` chain nests `n` instantiations.
///
/// TypeScript 7.0.2, all four settings: `E98<"ok">` is TS2589 with
/// "Type instantiation is excessively deep and possibly infinite."
#[test]
fn verters_instantiation_budget_reports_the_checkers_ts2589() {
    use verter_type_engine::semantic_query::CheckerDiagnosticOperation;
    const BUDGET: u32 = 150;
    const TS2589: &str = "Type instantiation is excessively deep and possibly infinite.";
    let answers = |options: &'static str, length: usize| {
        displayed_in(
            Some(BUDGET),
            options,
            conditional_argument_chain(length),
            &format!("E{length}<\"ok\">"),
            "\"ok\"",
        )
        .is_ok()
    };
    let reports_ts2589 = |options: &'static str, length: usize| {
        recovery_in(
            Some(BUDGET),
            options,
            conditional_argument_chain(length),
            &format!("E{length}<\"ok\">"),
        )
        .is_some_and(|diagnostic| {
            diagnostic.code.code() == 2589
                && diagnostic.code.message() == TS2589
                && diagnostic.operation == CheckerDiagnosticOperation::InstantiationBudget
        })
    };
    for options in SETTINGS {
        assert!(
            answers(options, 98),
            "E98 under {options} answers past the checker's limit"
        );
        let inside = BUDGET as usize;
        assert!(
            answers(options, inside),
            "E{inside} under {options} answers inside Verter's budget"
        );
        assert!(
            reports_ts2589(options, inside + 1),
            "E{} under {options} is the checker's TS2589 at Verter's budget",
            inside + 1
        );
    }
}

/// An instantiation that never terminates — each one needing the next of
/// the same alias over a growing argument — stops at Verter's
/// instantiation budget with the checker's TS2589, in bounded memory.
///
/// TypeScript 7.0.2, all four settings: `Grow<"ok">` over `type Pair<X> =
/// X extends unknown ? [X, X] : never` and `type Grow<T> = T extends never
/// ? never : Pair<Grow<[T]>>` is TS2589.
#[test]
fn an_unbounded_instantiation_stops_at_verters_budget_with_ts2589() {
    const GROW: &str = "type Pair<X> = X extends unknown ? [X, X] : never;\n\
                        type Grow<T> = T extends never ? never : Pair<Grow<[T]>>;\n";
    for options in SETTINGS {
        let recovered = evaluated_in(
            Some(150),
            options,
            GROW.to_owned(),
            "Grow<\"ok\">".to_owned(),
            |dispatch, value| {
                let mut seen = rustc_hash::FxHashSet::default();
                let mut stack = vec![value];
                while let Some(node) = stack.pop() {
                    if !seen.insert(node) {
                        continue;
                    }
                    let Some(data) = dispatch.graph().node_data(node) else {
                        continue;
                    };
                    if matches!(
                        data.as_ref(),
                        SemanticNodeData::Opaque(QueryError::CheckerRecovery { diagnostic, .. })
                            if diagnostic.code == CheckerDiagnosticCode::ExcessivelyDeepInstantiation
                    ) {
                        return true;
                    }
                    data.for_each_retained_child(|child| stack.push(child));
                }
                false
            },
        );
        assert!(
            recovered,
            "Grow<\"ok\"> under {options} reaches the checker's TS2589 at Verter's budget"
        );
    }
}

/// Each frame's result carries what the frames below it read: the carrier
/// of every instantiation a frame needed is replayed into the frame's own
/// tracer when it resumes, so an edit to the declaration at the bottom of a
/// chain reaches every warm instantiation above it.
///
/// TypeScript 7.0.2, `strictNullChecks`: `typeof c` is `"ok" | undefined`
/// over `type A0<T> = T | undefined`, and `"ok" | null` over
/// `type A0<T> = T | null`.
#[test]
fn an_edit_below_a_chain_of_frames_reaches_every_instantiation_above_it() {
    use super::checker_probe_lane_tests::{default_probe_host, with_probe_on_host};
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::{checker_syntax, render_node};
    use verter_type_engine::semantic_query::{ProjectionMode, ProjectionReductionContext};

    let mut source = String::from("import type { A0 } from \"./base\";\n");
    for level in 1..=30 {
        source.push_str(&format!("type A{level}<T> = A{}<T>;\n", level - 1));
    }
    source.push_str("declare function f<T>(x: T): A30<T>;\nexport const c = f(\"ok\");\n");
    let host = default_probe_host();
    let evaluate = |base: &str, checker: &str| -> Result<(), String> {
        let expected = checker_syntax::parse(checker).expect("the checker type parses");
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            "/wb/base.ts",
            base,
            crate::FileLanguage::script_ts(),
        );
        with_probe_on_host(
            &host,
            Default::default(),
            &source,
            "typeof c",
            |dispatch, node| {
                let value = dispatch
                    .normalize_node_for_structural_fact_demand(
                        node,
                        ProjectionReductionContext::published(ProjectionMode::Expanded),
                    )
                    .into_complete_node()
                    .expect("the chain evaluates completely");
                if checker_syntax::matches_node(dispatch, value, &expected, 0) {
                    Ok(())
                } else {
                    Err(render_node(dispatch, value, 0))
                }
            },
        )
    };
    assert_eq!(
        evaluate("export type A0<T> = T | undefined;\n", "\"ok\" | undefined"),
        Ok(())
    );
    assert_eq!(
        evaluate("export type A0<T> = T | null;\n", "\"ok\" | null"),
        Ok(())
    );
}

/// Printing an application of a long alias chain reads each alias's body a
/// bounded number of times: whether the chain declares a homomorphic
/// mapping is walked once for the whole chain, not once per alias, and the
/// printing walk itself runs in a loop.
///
/// TypeScript 7.0.2: `E200<"ok">` is TS2589 (Verter's full answer is
/// `"ok"`); the bound is on Verter's own work.
#[test]
fn printing_an_alias_chain_reads_each_alias_a_bounded_number_of_times() {
    const LENGTH: usize = 200;
    let lowerings = on_this_thread(|| {
        let _trace = super::raise::enable_dispatch_trace_for_test();
        let host = super::checker_probe_lane_tests::default_probe_host();
        super::checker_probe_lane_tests::with_probe_outcome_on_host(
            &host,
            Default::default(),
            &conditional_argument_chain(LENGTH),
            &format!("E{LENGTH}<\"ok\">"),
            |_, _| (),
        );
        super::raise::DISPATCH_TRACE.with(|trace| {
            trace
                .borrow()
                .iter()
                .filter(|kind| **kind == "LowerLocator")
                .count()
        })
    });
    assert!(
        lowerings <= 4 * LENGTH,
        "{lowerings} body lowerings for a chain of {LENGTH} aliases"
    );
}

/// The `typeof` of a call whose declared return is a long alias chain
/// dispatches a bounded number of instantiation reads per alias: the
/// call's binder-reach walk reads the chain's links one after another, and
/// each link's read is one step of the `typeof`'s frame, never a re-run of
/// its whole projection that reads every earlier link again.
///
/// TypeScript 7.0.2, `strict`: `typeof c` is `"ok" | undefined`; the bound
/// is on Verter's own work.
#[test]
fn a_typeof_over_an_alias_chain_reads_each_link_a_bounded_number_of_times() {
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::{checker_syntax, render_node};
    use verter_type_engine::semantic_query::{ProjectionMode, ProjectionReductionContext};
    const LENGTH: usize = 200;
    let instantiations = on_this_thread(|| {
        let _trace = super::raise::enable_dispatch_trace_for_test();
        let host = super::checker_probe_lane_tests::default_probe_host();
        super::checker_probe_lane_tests::with_probe_outcome_on_host(
            &host,
            Default::default(),
            &alias_chain(LENGTH),
            "typeof c",
            |dispatch, outcome| {
                // The reads the `typeof` made, counted before its answer is
                // evaluated for the check below.
                let instantiations = super::raise::DISPATCH_TRACE.with(|trace| {
                    trace
                        .borrow()
                        .iter()
                        .filter(|kind| **kind == "Instantiate")
                        .count()
                });
                let node = outcome
                    .into_complete_node()
                    .expect("`typeof c` answers completely");
                let value = dispatch
                    .normalize_node_for_structural_fact_demand(
                        node,
                        ProjectionReductionContext::published(ProjectionMode::Expanded),
                    )
                    .into_complete_node()
                    .expect("`typeof c` evaluates completely");
                let expected =
                    checker_syntax::parse(ALIAS_CHAIN_ROWS[0].1).expect("the checker type parses");
                assert!(
                    checker_syntax::matches_node(dispatch, value, &expected, 0),
                    "`typeof c` is {}, got {}",
                    ALIAS_CHAIN_ROWS[0].1,
                    render_node(dispatch, value, 0)
                );
                instantiations
            },
        )
    });
    assert!(
        instantiations <= 4 * LENGTH,
        "{instantiations} instantiation reads for a chain of {LENGTH} aliases"
    );
}

/// In a process of its own — nothing evaluated before it, on any stack —
/// one small-stack thread evaluates a 200-alias chain, then conditional
/// arguments nested 97 deep and 500 deep, displays each answer, and drops
/// every host and dispatcher it built there: the three chains one after
/// another on the thread the earlier ones left behind.
///
/// TypeScript 7.0.2, `strict`: `typeof c` and `ReturnType<typeof gj>` over
/// the 200-alias chain are `"ok" | undefined`; `E97<"ok">` is `"ok"`;
/// `E500<"ok">` is TS2589, where Verter's full answer is `"ok"`.
#[test]
fn a_process_of_its_own_evaluates_deep_chains_on_one_small_stack_thread() {
    in_a_fresh_process(test_path!(), || {
        on_a_small_stack(|| {
            let mismatches = evaluated_mismatches(&alias_chain(200), ALIAS_CHAIN_ROWS);
            assert_eq!(mismatches, Vec::<String>::new(), "a chain of 200 aliases");
            for length in [97, 500] {
                let shown = displayed_in(
                    None,
                    SETTINGS[0],
                    conditional_argument_chain(length),
                    &format!("E{length}<\"ok\">"),
                    "\"ok\"",
                )
                .unwrap_or_else(|shown| panic!("E{length}<\"ok\"> displays as {shown}"));
                assert!(
                    shown.contains("ok"),
                    "E{length}<\"ok\"> displays as {shown}"
                );
            }
        });
    });
}

/// Whether Verter's instantiation budget is reached depends on the chain
/// that needed an instantiation, so its recovery is never kept: a demand
/// on the same host under the production budget evaluates the chain again
/// and answers.
///
/// TypeScript 7.0.2, `strict`: `E151<"ok">` is TS2589; Verter's full
/// answer is `"ok"`.
#[test]
fn a_budget_recovery_is_never_kept_in_the_memo() {
    use verter_type_engine::semantic_query::{ProjectionMode, ProjectionReductionContext};
    let recovered_then_answered = on_this_thread(|| {
        let host = super::checker_probe_lane_tests::default_probe_host();
        let source = conditional_argument_chain(151);
        let evaluate = || {
            super::checker_probe_lane_tests::with_probe_on_host(
                &host,
                Default::default(),
                &source,
                "E151<\"ok\">",
                |dispatch, node| {
                    let value = dispatch
                        .normalize_node_for_structural_fact_demand(
                            node,
                            ProjectionReductionContext::published(ProjectionMode::Expanded),
                        )
                        .into_usable_node()
                        .expect("the probe answers");
                    match dispatch.graph().node_data(value).as_deref() {
                        Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery { .. })) => None,
                        _ => Some(
                            crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node(
                                dispatch, value, 0,
                            ),
                        ),
                    }
                },
            )
        };
        let limited = {
            let _budget = super::connected_demand::InstantiationBudgetForTests::install(150);
            evaluate()
        };
        (limited, evaluate())
    });
    assert_eq!(recovered_then_answered, (None, Some("\"ok\"".to_owned())));
}

/// A complete answer kept under the production budget is never served to
/// a demand whose smaller instantiation budget its cold evaluation would
/// have exhausted: its receipt names the instantiation depth the answer
/// took, and that demand evaluates for itself and recovers exactly as it
/// does cold.
///
/// TypeScript 7.0.2, `strict`: `E151<"ok">` is TS2589; Verter's full
/// answer is `"ok"`.
#[test]
fn a_kept_answer_never_passes_a_smaller_instantiation_budget() {
    use verter_type_engine::semantic_query::{ProjectionMode, ProjectionReductionContext};
    let answered_then_recovered = on_this_thread(|| {
        let host = super::checker_probe_lane_tests::default_probe_host();
        let source = conditional_argument_chain(151);
        let evaluate = || {
            super::checker_probe_lane_tests::with_probe_on_host(
                &host,
                Default::default(),
                &source,
                "E151<\"ok\">",
                |dispatch, node| {
                    let value = dispatch
                        .normalize_node_for_structural_fact_demand(
                            node,
                            ProjectionReductionContext::published(ProjectionMode::Expanded),
                        )
                        .into_usable_node()
                        .expect("the probe answers");
                    match dispatch.graph().node_data(value).as_deref() {
                        Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery { .. })) => None,
                        _ => Some(
                            crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node(
                                dispatch, value, 0,
                            ),
                        ),
                    }
                },
            )
        };
        let answered = evaluate();
        let limited = {
            let _budget = super::connected_demand::InstantiationBudgetForTests::install(150);
            evaluate()
        };
        (answered, limited)
    });
    assert_eq!(answered_then_recovered, (Some("\"ok\"".to_owned()), None));
}

/// Whether `probe`'s evaluated type, under `options` and an instantiation
/// budget of `budget`, holds the checker's TS2589 recovery anywhere in it.
fn holds_a_ts2589(options: &'static str, budget: u32, source: String, probe: &str) -> bool {
    evaluated_in(
        Some(budget),
        options,
        source,
        probe.to_owned(),
        |dispatch, value| {
            let mut seen = rustc_hash::FxHashSet::default();
            let mut stack = vec![value];
            while let Some(node) = stack.pop() {
                if !seen.insert(node) {
                    continue;
                }
                let Some(data) = dispatch.graph().node_data(node) else {
                    continue;
                };
                if matches!(
                    data.as_ref(),
                    SemanticNodeData::Opaque(QueryError::CheckerRecovery { diagnostic, .. })
                        if diagnostic.code == CheckerDiagnosticCode::ExcessivelyDeepInstantiation
                ) {
                    return true;
                }
                data.for_each_retained_child(|child| stack.push(child));
            }
            false
        },
    )
}

/// A recursive application over new arguments, in a position the checker
/// defers — an object member, an array or tuple element, a signature, an
/// interface's type argument, or an alias application inside a member — is
/// the lazy recursive reference: evaluating the application enters no
/// instantiation of it, so it never reaches a budget.
///
/// TypeScript 7.0.2, all four settings: `X<"ok">` over each declaration
/// below reports nothing.
#[test]
fn recursive_applications_in_deferred_positions_stay_lazy() {
    const DEFERRED: [&str; 9] = [
        "type X<T> = { v: T; kids: X<[T]>[] };",
        "type X<T> = [T, X<[T]>];",
        "type X<T> = (x: T) => X<[T]>;",
        "type X<T> = T extends any ? { a: X<[T]> } : never;",
        "type X<T> = T extends any ? X<[T]>[] : never;",
        "type X<T> = T extends any ? [X<[T]>] : never;",
        "type W<Y> = { w: Y }; type X<T> = { v: T; next: W<X<[T]>> };",
        "type X<T> = Promise<X<[T]>>;",
        "interface I<Y> { y: Y } type X<T> = T extends any ? I<X<[T]>> : never;",
    ];
    for options in SETTINGS {
        for declaration in DEFERRED {
            assert!(
                !holds_a_ts2589(
                    options,
                    150,
                    format!(
                        "{declaration}
"
                    ),
                    "X<\"ok\">"
                ),
                "`{declaration}` under {options} stays lazy"
            );
        }
    }
}

/// A recursive application over new arguments, as a type argument of
/// another alias in a conditional's branch, is instantiated as the checker
/// instantiates it — each one the next, without end — and stops at
/// Verter's instantiation budget with the checker's TS2589. A lower budget
/// than production's stands in for it on the same path.
///
/// TypeScript 7.0.2, all four settings: `X<"ok">` over each declaration
/// below is TS2589.
#[test]
fn recursive_applications_in_eager_positions_reach_verters_budget() {
    const EAGER: [&str; 4] = [
        "type Id<Y> = Y; type X<T> = T extends any ? Id<X<[T]>> : never;",
        "type X<T> = T extends any ? Exclude<X<[T]>, never> : never;",
        "type W<Y> = { w: Y }; type X<T> = T extends any ? W<X<[T]>> : never;",
        "type W<Y> = Y[]; type X<T> = T extends any ? W<X<[T]>> : never;",
    ];
    for options in SETTINGS {
        for declaration in EAGER {
            assert!(
                holds_a_ts2589(
                    options,
                    150,
                    format!(
                        "{declaration}
"
                    ),
                    "X<\"ok\">"
                ),
                "`{declaration}` under {options} reaches TS2589"
            );
        }
    }
}

/// A homomorphic mapped type whose template applies the declaration over
/// new arguments is TS2589 in the checker, even over a primitive.
///
/// TypeScript 7.0.2, all four settings: `X<"ok">` over `type X<T> = { [K
/// in keyof T]: X<[T]> }` is TS2589.
#[test]
#[ignore = "a homomorphic mapped type over a primitive is the primitive, without instantiating its template"]
fn a_mapped_template_applying_its_declaration_reaches_ts2589() {
    for options in SETTINGS {
        assert!(
            holds_a_ts2589(
                options,
                150,
                "type X<T> = { [K in keyof T]: X<[T]> };
"
                .to_owned(),
                "X<\"ok\">"
            ),
            "the mapped template under {options} reaches TS2589"
        );
    }
}

/// The checker diagnostic `G<string>` over `source` evaluates to, under
/// `options` and a tail budget of `tail` when set: its numeric code, when
/// the answer is a checker recovery anywhere in it.
fn diagnostic_of_g_string(
    options: &'static str,
    source: &'static str,
    tail: Option<u32>,
) -> Option<u32> {
    use verter_type_engine::semantic_query::{ProjectionMode, ProjectionReductionContext};
    on_this_thread(move || {
        let _tail = tail.map(super::connected_demand::TailBudgetForTests::install);
        with_probe_in(
            ProbeProject {
                compiler_options: Some(options),
                ..Default::default()
            },
            source,
            "G<string>",
            |dispatch, node| {
                let value = dispatch
                    .normalize_node_for_structural_fact_demand(
                        node,
                        ProjectionReductionContext::published(ProjectionMode::Expanded),
                    )
                    .into_complete_node()
                    .expect("the probe evaluates completely");
                let mut seen = rustc_hash::FxHashSet::default();
                let mut stack = vec![value];
                while let Some(node) = stack.pop() {
                    if !seen.insert(node) {
                        continue;
                    }
                    let Some(data) = dispatch.graph().node_data(node) else {
                        continue;
                    };
                    if let SemanticNodeData::Opaque(QueryError::CheckerRecovery {
                        diagnostic,
                        ..
                    }) = data.as_ref()
                    {
                        return Some(diagnostic.code.code());
                    }
                    data.for_each_retained_child(|child| stack.push(child));
                }
                None
            },
        )
    })
}

/// A generic alias whose declared type requires itself — its body applies
/// it, directly or through other aliases, with no conditional type of its
/// own deciding — is the checker's TS2456 (`any`), however large the
/// budgets; through its own conditional type's branch the application is
/// the tail loop, TS2589 at the tail budget (a lower one than production's
/// standing in); in a position the checker defers it is a lazy reference.
///
/// TypeScript 7.0.2, all four settings, `G<string>` over: `type G<T> =
/// G<[T]>`, `type G<T> = G<T>`, `type G<T> = H<T>; type H<T> = G<[T]>` and
/// `type G<T> = H<[T]>; type H<T> = T extends any ? G<T> : never` — TS2456,
/// `any`; `type G<T> = T extends never ? never : G<[T]>` — TS2589, `any`;
/// `type G<T> = { a: G<[T]> }` and `type G<T> = G<[T]>[]` — no diagnostic.
#[test]
fn circular_type_aliases_are_the_checkers_ts2456_and_tail_runs_its_ts2589() {
    const CIRCULAR: [&str; 4] = [
        "type G<T> = G<[T]>;\n",
        "type G<T> = G<T>;\n",
        "type G<T> = H<T>; type H<T> = G<[T]>;\n",
        "type G<T> = H<[T]>; type H<T> = T extends any ? G<T> : never;\n",
    ];
    const DEFERRED: [&str; 2] = ["type G<T> = { a: G<[T]> };\n", "type G<T> = G<[T]>[];\n"];
    const TAIL: &str = "type G<T> = T extends never ? never : G<[T]>;\n";
    for options in SETTINGS {
        for source in CIRCULAR {
            assert_eq!(
                diagnostic_of_g_string(options, source, None),
                Some(2456),
                "`{source}` under {options}"
            );
        }
        for source in DEFERRED {
            assert_eq!(
                diagnostic_of_g_string(options, source, None),
                None,
                "`{source}` under {options}"
            );
        }
        assert_eq!(
            diagnostic_of_g_string(options, TAIL, Some(200)),
            Some(2589),
            "`{TAIL}` under {options}"
        );
    }
}
