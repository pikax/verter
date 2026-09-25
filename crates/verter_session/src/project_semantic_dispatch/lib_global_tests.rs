//! An authored reference to a global the project's library declares — the
//! `String` interface in type position, the `String` value in a `typeof`
//! or a read — resolves through the library, the environment a checker
//! reads from its `lib` files, by the same global lookup a program's own
//! global declarations take.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured on this
//! exact fixture with `tsc --ignoreConfig --noLib --declaration
//! --emitDeclarationOnly --strict` over the library as a root file, read
//! off the emitted `.d.ts`; the four `strictNullChecks` × `noImplicitAny`
//! settings answer alike.

use std::sync::Arc;

use super::checker_probe_lane_tests::{mismatches_in, ProbeProject};
use super::*;
use crate::semantic_query::{
    FlowReturnDegradation, FlowReturnKey, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
};
use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;
use verter_type_expr::facts::FunctionPartIdentity;
use verter_type_expr::{PrimitiveName, TopLevelOwnerId, TypeExpr};

const STRING_LIB: &str = "\
interface Array<T> { length: number; [n: number]: T; }
interface Boolean {}
interface Function {}
interface IArguments {}
interface Number { toFixed(digits?: number): string; }
interface Object {}
interface RegExp {}
interface CallableFunction {}
interface NewableFunction {}
interface String { readonly length: number; readonly [index: number]: string; charAt(pos: number): string; }
interface StringConstructor { new (value?: any): String; (value?: any): string; readonly prototype: String; fromCharCode(...codes: number[]): string; }
declare var String: StringConstructor;
";

const STRING_USE: &str = "\
export function len(s: String) { return s.length; }
export function from() { return String.fromCharCode(65); }
export function call() { return String(1); }
export function protoLen() { return String.prototype.length; }
export function gtFrom() { return globalThis.String.fromCharCode(65); }
";

/// The global `String` interface and `String` value a library declares
/// resolve from a program file, in type position and through a `typeof`.
///
/// Measured on TypeScript 7.0.2: `String['length']` is `number`,
/// `ReturnType<String['charAt']>` `string`,
/// `(typeof String)['prototype']['length']` `number`,
/// `ReturnType<typeof String.fromCharCode>` `string`,
/// `ReturnType<typeof String>` (the constructor's call signature)
/// `string`, `ReturnType<typeof globalThis.String.fromCharCode>` `string`;
/// `len` (`s.length` over `s: String`) is `number`, `from` and `call`
/// `string`, `protoLen` `number`, `gtFrom` `string`.
///
/// Mutation: consulting no library in the global type lookup leaves every
/// type-position row a typed miss; consulting none in the global value
/// lookup leaves every `typeof String` row and the `from`, `call`,
/// `protoLen` and `gtFrom` reads unresolved.
#[test]
fn a_library_global_resolves_in_type_and_value_position() {
    let failures = mismatches_in(
        ProbeProject {
            files: &[],
            compiler_options: None,
            ambient_lib: Some(STRING_LIB),
        },
        STRING_USE,
        &[
            ("String['length']", "number"),
            ("ReturnType<String['charAt']>", "string"),
            ("(typeof String)['prototype']['length']", "number"),
            ("ReturnType<typeof String.fromCharCode>", "string"),
            ("ReturnType<typeof String>", "string"),
            (
                "ReturnType<typeof globalThis.String.fromCharCode>",
                "string",
            ),
            ("ReturnType<typeof len>", "number"),
            ("ReturnType<typeof from>", "string"),
            ("ReturnType<typeof call>", "string"),
            ("ReturnType<typeof protoLen>", "number"),
            ("ReturnType<typeof gtFrom>", "string"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const AUGMENTING_MODULE: &str =
    "export {};\ndeclare global { interface String { extra: \"x\"; } }\n";

/// A program's `declare global` augmentation of a library interface
/// merges with the library's declaration, which comes first.
///
/// Measured on TypeScript 7.0.2: `String['extra']` is `"x"` and
/// `String['length']` `number`; `extra` (`s.extra` over `s: String`) is
/// `"x"`.
///
/// Mutation: reading the program's declaration alone answers
/// `String['length']` with a typed miss.
#[test]
fn a_program_augmentation_merges_with_the_library_declaration() {
    let files = [("aug.ts", AUGMENTING_MODULE)];
    let failures = mismatches_in(
        ProbeProject {
            files: &files,
            compiler_options: None,
            ambient_lib: Some(STRING_LIB),
        },
        "export function extra(s: String) { return s.extra; }\n",
        &[
            ("String['extra']", "\"x\""),
            ("String['length']", "number"),
            ("ReturnType<typeof extra>", "\"x\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const PROJECT_ROOT: &str = "/lg";
const LIB_CANONICAL: &str = "lib.probe.d.ts";

fn library_host(lib: &str) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone_with_tsconfig_projects(
        HostConfig::default(),
        &[(PROJECT_ROOT, r#"{ "compilerOptions": { "strict": true } }"#)],
    ));
    register_library(&host, lib);
    host
}

fn register_library(host: &VerterHost, lib: &str) {
    host.workspace()
        .register_ambient_lib(verter_workspace::AmbientLibSpec {
            project_id: None,
            canonical_id: Arc::from(LIB_CANONICAL),
            source: Arc::from(lib),
        })
        .expect("the library registers against its project");
}

fn upsert(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical.to_string()),
        input_id: canonical.to_string(),
        source: Arc::from(source),
        file_language: crate::LanguageRegistry::global()
            .classify_static(canonical)
            .static_resolution(),
        aliases: Vec::new(),
    });
}

/// One evaluated function's return type and degradation.
fn eval(
    host: &Arc<VerterHost>,
    canonical: &str,
    name: &str,
) -> (TypeExpr, Option<FlowReturnDegradation>) {
    with_dispatch(host, |dispatch| {
        let key = FlowReturnKey {
            function: dispatch.flow_function_slot_for(
                Arc::from(canonical),
                TopLevelOwnerId::ordinary_file(),
                Arc::from(name),
                FunctionPartIdentity::DeclarationBody,
                0,
            ),
            normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
            context: dispatch.flow_return_context_for(canonical),
            demand: crate::semantic_query::ReturnProjectionDemand::whole_return(),
            input: crate::semantic_query::FlowInputContext::empty(),
            result_contract: super::flow_solve::flow_return_result_contract_id(),
        };
        match dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key))) {
            QueryResult::Value(SemanticQueryOutput {
                value: SemanticQueryValue::FlowReturn(result),
                ..
            }) => (
                host.project_node_to_type_expr_for_test(result.return_type())
                    .unwrap_or_else(|| panic!("{name}: the value did not project")),
                result.degradation(),
            ),
            other => panic!("{name} must produce a value, got {other:?}"),
        }
    })
}

fn with_dispatch<R>(
    host: &Arc<VerterHost>,
    read: impl FnOnce(&ProjectSemanticDispatch<'_>) -> R,
) -> R {
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    read(&ProjectSemanticDispatch::new(&host_ctx))
}

/// A library global resolves through the project's symbol index and the
/// library's header index: a read materializes the declarations it names
/// and no other, however many the library holds.
///
/// Measured on TypeScript 7.0.2: `len` is `number` and `from` `string`.
///
/// Mutation: a lookup that finds the declaration by lowering the
/// library's declarations in turn materializes every `Unrelated*`
/// interface and `unrelated*` value before `String`.
#[test]
fn a_library_global_read_materializes_only_the_declarations_it_names() {
    let mut lib = String::new();
    for index in 0..24 {
        lib.push_str(&format!(
            "interface Unrelated{index} {{ u: {index}; }}\ndeclare var unrelated{index}: Unrelated{index};\n"
        ));
    }
    lib.push_str(STRING_LIB);
    let host = library_host(&lib);
    let reader = format!("{PROJECT_ROOT}/reader.ts");
    upsert(
        &host,
        &reader,
        "export function len(s: String) { return s.length; }\n\
         export function from() { return String.fromCharCode(65); }\n",
    );
    assert_eq!(
        eval(&host, &reader, "len"),
        (TypeExpr::Primitive(PrimitiveName::Number), None)
    );
    assert_eq!(
        eval(&host, &reader, "from"),
        (TypeExpr::Primitive(PrimitiveName::String), None)
    );
    let library = with_dispatch(&host, |dispatch| {
        let project = dispatch
            .project_stable_key_for_canonical(&reader)
            .expect("the reader belongs to the configured project");
        verter_workspace::ambient_virtual_canonical_id(project, LIB_CANONICAL)
    });
    let indexed = host
        .ensure_indexed_ready_serve(library.as_ref())
        .expect("the library is served")
        .indexed;
    let memo = indexed.shallow_state.decl_bodies();
    assert!(
        memo.type_entry_materialized("String"),
        "the read names `String`"
    );
    assert!(
        memo.value_entry_materialized("String"),
        "the read names `String`"
    );
    for index in 0..24 {
        assert!(
            !memo.type_entry_materialized(&format!("Unrelated{index}")),
            "`Unrelated{index}` is not read"
        );
        assert!(
            !memo.value_entry_materialized(&format!("unrelated{index}")),
            "`unrelated{index}` is not read"
        );
    }
    assert!(
        !memo.whole_env_materialized(),
        "no whole-library environment"
    );
}

const CALLABLE_STRING_AUGMENTATION: &str =
    "export {};\ndeclare global { interface String { (n: number): \"called\"; } }\n";

const PRIMITIVE_CALLS: &str = "\
declare const s: string;
export function called() { return s(1); }
export function litCalled() { return \"lit\"(2); }
";

/// A primitive's apparent call signatures are those of the global wrapper
/// its project declares — the library's merged with the program's
/// `declare global` augmentation of it, or, with no library declaring it,
/// the program's own declaration — read through the one global lookup.
///
/// Measured on TypeScript 7.0.2 (`--noLib`, the library a root file, as
/// the module doc says): `called` and `litCalled` are `"called"`, with the
/// library's declarations in a library or in a program script alike.
///
/// Mutation: reading the wrapper from the library alone leaves both rows a
/// typed miss when a program script declares it.
#[test]
fn a_primitive_call_reads_the_wrapper_the_project_declares() {
    let augmentation = [("aug.ts", CALLABLE_STRING_AUGMENTATION)];
    let script = [
        ("globals.d.ts", STRING_LIB),
        ("aug.ts", CALLABLE_STRING_AUGMENTATION),
    ];
    for (files, ambient_lib) in [(&augmentation[..], Some(STRING_LIB)), (&script[..], None)] {
        let failures = mismatches_in(
            ProbeProject {
                files,
                compiler_options: Some(r#"{ "strict": true }"#),
                ambient_lib,
            },
            PRIMITIVE_CALLS,
            &[
                ("ReturnType<typeof called>", "\"called\""),
                ("ReturnType<typeof litCalled>", "\"called\""),
            ],
        );
        assert!(
            failures.is_empty(),
            "library {}:\n{}",
            ambient_lib.is_some(),
            failures.join("\n")
        );
    }
}

/// A function's return as the host's request entry answers it: the read
/// runs under the function file's request, as every production read does.
fn request_return(host: &Arc<VerterHost>, canonical: &str, name: &str) -> TypeExpr {
    let identity = verter_type_expr::facts::FlowFunctionReturnIdentity {
        anchor: verter_type_expr::locators::AuthoredAnchor {
            canonical_id: Arc::from(canonical),
            owner: TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from(name),
            space: verter_type_expr::locators::LocatorSymbolSpace::Value,
        },
        function_part: FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    };
    let carrier = host.get_flow_return_type_with_audit(
        &identity,
        crate::semantic_query::ReturnProjectionDemand::whole_return(),
    );
    let result = carrier
        .as_result()
        .unwrap_or_else(|_| panic!("{name} produced no flow-return result"));
    host.project_node_to_type_expr_for_test(result.return_type())
        .unwrap_or_else(|| panic!("{name}: the value did not project"))
}

/// A global lookup that finds no declaration depends on the program's
/// global contributors: a declaration that appears later misses the warm
/// read. (A production host registers its libraries once, when it is
/// constructed, so a library registered after a read is not a live case;
/// a library's re-registration invalidates through the dependency its hit
/// records.)
///
/// Measured on TypeScript 7.0.2 with `strictBindCallApply` off (the
/// callable apparent interface is then `Function`), on the four
/// `strictNullChecks` × `noImplicitAny` settings: once a program script
/// declares `interface Function { extra: "x" }` and
/// `interface String { (n: number): "called"; }`, `ge` (`g.extra` over
/// `function g() {}`) is `"x"` and `called` (`s(1)` over `s: string`)
/// `"called"`. Before it the program declares neither global (the
/// checker's TS2318) and answers `any` for both, the checker's recovery,
/// which the lane answers as a typed miss.
///
/// Mutation: observing no contributor population for a global lookup keeps
/// serving both warm misses after the declarations appear.
#[test]
fn a_global_declared_after_a_miss_misses_the_warm_read() {
    let host = Arc::new(VerterHost::new_standalone_with_tsconfig_projects(
        HostConfig::default(),
        &[(
            PROJECT_ROOT,
            r#"{ "compilerOptions": { "strict": true, "strictBindCallApply": false } }"#,
        )],
    ));
    let reader = format!("{PROJECT_ROOT}/reader.ts");
    upsert(
        &host,
        &reader,
        "function g() {}\nexport function ge() { return g.extra; }\n\
         declare const s: string;\nexport function called() { return s(1); }\n",
    );
    let extra = TypeExpr::Literal(verter_type_expr::LiteralValue::String("x".into()));
    let called = TypeExpr::Literal(verter_type_expr::LiteralValue::String("called".into()));
    assert_ne!(
        request_return(&host, &reader, "ge"),
        extra,
        "no `Function` is declared yet"
    );
    assert_ne!(
        request_return(&host, &reader, "called"),
        called,
        "no `String` is declared yet"
    );
    upsert(
        &host,
        &format!("{PROJECT_ROOT}/globals.d.ts"),
        "interface Function { extra: \"x\"; }\ninterface String { (n: number): \"called\"; }\n",
    );
    assert_eq!(request_return(&host, &reader, "ge"), extra);
    assert_eq!(request_return(&host, &reader, "called"), called);
}

/// Two configured projects, each registering its own library.
fn two_library_host(lib_a: &str, lib_b: &str) -> Arc<VerterHost> {
    let config = |root: &str| verter_workspace::VfsProjectConfig {
        root: root.to_string(),
        rank: verter_workspace::ProjectRank::Explicit,
        tsconfig_path: Some(format!("{root}/tsconfig.json")),
        root_files: vec![],
        extensions: vec![".ts".into(), ".d.ts".into()],
        workspace_root: root.to_string(),
        workspace_aliases: vec![],
        compiler_options: verter_semantic::resolver_core::IdeProjectCompilerOptions::default(),
        references: vec![],
        membership: verter_workspace::configured_membership_match_all_under_root(
            &verter_workspace::CanonicalPath::new(root),
        ),
    };
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    workspace.set_project_graph(verter_workspace::ProjectGraph::from_configs(vec![
        config("/a"),
        config("/b"),
    ]));
    let mut libraries = Vec::new();
    for (ordinal, (lib_id, lib)) in [("lib.a.d.ts", lib_a), ("lib.b.d.ts", lib_b)]
        .into_iter()
        .enumerate()
    {
        let project = verter_workspace::workspace_snapshot::ProjectId(ordinal as u32);
        verter_workspace::WorkspaceAccess::register_ambient_lib(
            workspace.as_ref(),
            verter_workspace::AmbientLibSpec {
                project_id: Some(project),
                canonical_id: Arc::from(lib_id),
                source: Arc::from(lib),
            },
        )
        .expect("the library registers against its project");
        let key = verter_workspace::WorkspaceRead::project_stable_key(workspace.as_ref(), project)
            .expect("the project has a key");
        libraries.push((
            verter_workspace::ambient_virtual_canonical_id(key, lib_id),
            lib,
        ));
    }
    let access: Arc<dyn verter_workspace::WorkspaceAccess> = workspace;
    let host = Arc::new(VerterHost::new(HostConfig::default(), access));
    for (virtual_id, lib) in libraries {
        let _ = host.upsert(UpsertRequest {
            canonical_id: None,
            input_id: virtual_id.to_string(),
            source: Arc::from(lib),
            file_language: crate::FileLanguage::script_ts(),
            aliases: Vec::new(),
        });
    }
    upsert(&host, "/a/main.ts", "export const a = 1;\n");
    upsert(&host, "/b/main.ts", "export const b = 2;\n");
    host
}

/// A primitive is a node every project shares, so the call signatures its
/// wrapper gives it depend on the project that asks. A read of the SAME
/// interned `string` from two projects whose `String` declarations differ
/// answers each project's own signature; its key is rewritten to the
/// demanding project's wrapper before admission, so each answer is one memo
/// candidate keyed by that project's `String` (the shared `string` subject
/// is never admitted), and a warm repeat is served from it.
///
/// Measured on TypeScript 7.0.2: a `string` called in a program whose
/// `String` declares `(): "a"` returns `"a"`, and `"b"` under `(): "b"`.
///
/// Mutation: rewriting no primitive subject to the demand's wrapper admits
/// no scoped candidate (the reads still answer, out of the memo); with the
/// out-of-memo rail also dropped, project B is served A's `"a"`.
#[test]
fn a_primitive_wrapper_read_is_the_demanding_projects() {
    let host = two_library_host(
        "interface String { (): \"a\"; }\n",
        "interface String { (): \"b\"; }\n",
    );
    let graph = host.project_type_store().semantic_graph();
    let string = graph.intern_node(crate::semantic_query::SemanticNodeData::Primitive(
        crate::semantic_query::PrimitiveKind::String,
    ));
    let key = |subject| SemanticQueryKey::SignaturesOfType {
        subject,
        kind: crate::semantic_query::SignatureKind::Call,
        context: crate::semantic_query::SemanticContextId::production(),
    };
    // One read under `canonical`'s demand: the rendered signatures and the
    // wrapper surface the demand's project declares.
    let read = |canonical: &str| {
        with_dispatch(&host, |dispatch| {
            let _scope =
                LexicalDemandScopeGuard::push(&dispatch.lexical_demand_scope, Arc::from(canonical));
            let read = dispatch.execute_via_cold_build_helper(key(string));
            let signatures: Vec<String> = match &read.value {
                QueryResult::Value(SemanticQueryValue::SignatureSet(set)) => set
                    .nodes
                    .iter()
                    .map(|nodes| {
                        let authored = nodes.authored.expect("an authored signature");
                        crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node(
                            dispatch, authored, 0,
                        )
                    })
                    .collect(),
                other => panic!("{canonical}: the read must settle, got {other:?}"),
            };
            let surface = match dispatch.global_wrapper_surface("String", &[], canonical) {
                super::apparent_type::GlobalWrapper::Surface(surface) => surface,
                _ => panic!("{canonical}'s project declares `String`"),
            };
            (signatures, surface)
        })
    };
    let reads: Vec<(&str, Vec<String>, SemanticNodeId)> =
        ["/a/main.ts", "/b/main.ts", "/a/main.ts", "/b/main.ts"]
            .into_iter()
            .map(|canonical| {
                let (signatures, surface) = read(canonical);
                (canonical, signatures, surface)
            })
            .collect();
    for (canonical, signatures, _) in &reads {
        let own = if canonical.starts_with("/a") {
            "() => \"a\""
        } else {
            "() => \"b\""
        };
        assert_eq!(
            signatures,
            &vec![own.to_string()],
            "{canonical} reads its own project's `String`"
        );
    }
    for (canonical, _, surface) in &reads {
        assert_eq!(
            graph.slot_candidate_count_for_tests(&key(*surface)),
            1,
            "{canonical}: the read is admitted under its project's `String`"
        );
    }
    assert_eq!(
        graph.slot_candidate_count_for_tests(&key(string)),
        0,
        "the shared `string` subject is never admitted"
    );
}
