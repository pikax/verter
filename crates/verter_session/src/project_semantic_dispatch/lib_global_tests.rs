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
