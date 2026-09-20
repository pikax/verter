use super::script_setup::*;
use crate::cursor::ScriptLanguage;

fn block(content: &str, start: u32, lang: Option<ScriptLanguage>) -> ScriptBlockInput<'_> {
    ScriptBlockInput {
        content,
        content_start: start,
        lang,
    }
}

fn setup_of(content: &str) -> TsSetupProjection {
    project_script_pair(None, Some(block(content, 0, None)), None)
        .expect("projects")
        .setup
        .expect("setup")
}

#[test]
fn stp11_universal_binder_keeps_constraints_and_carries_no_arguments() {
    let generic = "T extends { id: number }, U = string";
    let facts = project_script_pair(None, Some(block("const a = 1", 0, None)), Some(generic))
        .expect("projects");
    let params = &facts.binder.params;
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].name, "T");
    let constraint = params[0].constraint.expect("constraint");
    assert_eq!(
        &generic[constraint.start as usize..constraint.end as usize],
        "{ id: number }"
    );
    assert!(params[0].default.is_none());
    let default = params[1].default.expect("default");
    assert_eq!(
        &generic[default.start as usize..default.end as usize],
        "string"
    );
    assert_eq!(
        project_script_pair(None, None, Some("T extends")).unwrap_err(),
        SetupProjectionRefusal::InvalidGeneric
    );
}

#[test]
fn stp11_scope_module_imports_exports_and_setup_locals_keep_scope() {
    let normal =
        "import { a } from './a'\nexport const shared = 1\nexport default {}\nfunction helper() {}";
    let setup = "import type { B } from './b'\ninterface Local { x: B }\nconst { p, q } = a\nlet n = shared\nfoo()";
    let facts = project_script_pair(
        Some(block(normal, 10, None)),
        Some(block(setup, 500, None)),
        None,
    )
    .expect("projects");
    assert_eq!(facts.module.named_exports, ["shared", "default"]);
    assert_eq!(
        facts.module.normal_script_bindings,
        ["a", "shared", "helper"]
    );
    let sources: Vec<_> = facts
        .module
        .imports
        .iter()
        .map(|i| (i.source.as_str(), i.from_setup))
        .collect();
    assert_eq!(sources, [("./a", false), ("./b", true)]);
    assert_eq!(facts.module.imports[0].span.start, 10);
    assert_eq!(facts.module.imports[1].span.start, 500);
    let kinds: Vec<_> = facts
        .setup
        .expect("setup")
        .statements
        .into_iter()
        .map(|s| s.kind)
        .collect();
    assert_eq!(
        kinds,
        [
            SetupStatementKind::Import,
            SetupStatementKind::TypeDeclaration {
                name: "Local".into()
            },
            SetupStatementKind::Declaration {
                names: vec!["p".into(), "q".into()]
            },
            SetupStatementKind::Declaration {
                names: vec!["n".into()]
            },
            SetupStatementKind::Other,
        ]
    );
}

#[test]
fn stp11_await_top_level_only_and_nested_functions_excluded() {
    let top = setup_of("const data = await load()");
    assert!(top.checking_wrapper_is_async());
    assert_eq!(top.top_level_await.expect("await").start, 13);

    let loop_await = setup_of("for await (const x of gen()) {}");
    assert!(loop_await.checking_wrapper_is_async());

    let nested = setup_of(
        "const f = async () => { await g() }\nasync function h() { await g() }\nclass C { async m() { await g() } }",
    );
    assert!(!nested.checking_wrapper_is_async());
    assert_eq!(nested.top_level_await, None);
}

#[test]
fn stp11_assertion_ts_angle_assertion_and_tsx_element_parse_once_under_own_grammar() {
    let ts = project_script_pair(None, Some(block("const n = <number>raw", 0, None)), None)
        .expect("angle assertion parses under the TypeScript grammar");
    assert_eq!(ts.setup.expect("setup").grammar, ScriptGrammar::TypeScript);

    let tsx = project_script_pair(
        None,
        Some(block(
            "const el = <div>{a}</div>",
            0,
            Some(ScriptLanguage::TSX),
        )),
        None,
    )
    .expect("JSX parses under the TSX grammar");
    assert_eq!(tsx.setup.expect("setup").grammar, ScriptGrammar::Tsx);

    // The same bytes are not valid under the other grammar: nothing repairs
    // one dialect by re-parsing it as the other.
    assert_eq!(
        project_script_pair(
            None,
            Some(block("const el = <div>{a}</div>", 0, None)),
            None
        )
        .unwrap_err(),
        SetupProjectionRefusal::SyntaxErrors { setup: true }
    );
    assert_eq!(
        project_script_pair(
            None,
            Some(block("const n = <number>raw", 0, Some(ScriptLanguage::TSX))),
            None
        )
        .unwrap_err(),
        SetupProjectionRefusal::SyntaxErrors { setup: true }
    );
    assert_eq!(
        project_script_pair(
            None,
            Some(block("const n = 1", 0, Some(ScriptLanguage::JavaScript))),
            None
        )
        .unwrap_err(),
        SetupProjectionRefusal::NotTypeScript
    );
}

#[test]
fn stp11_one_body_rejects_duplicate_setup_body_placement() {
    assert_eq!(require_single_body(&[BodyProduct::Checking]), Ok(()));
    assert_eq!(require_single_body(&[]), Ok(()));
    assert_eq!(
        require_single_body(&[BodyProduct::Public, BodyProduct::Checking]),
        Err(SetupProjectionRefusal::DuplicateBody {
            products: vec![BodyProduct::Public, BodyProduct::Checking]
        })
    );
}

#[test]
fn stp11_macro_syntax_is_distinct_from_same_named_lexical_functions() {
    let macros = |normal: Option<&str>, setup: &str| -> Vec<&'static str> {
        project_script_pair(
            normal.map(|n| block(n, 0, None)),
            Some(block(setup, 0, None)),
            None,
        )
        .expect("projects")
        .setup
        .expect("setup")
        .macros
        .iter()
        .map(|m| m.name)
        .collect()
    };
    assert_eq!(
        macros(
            None,
            "const props = defineProps<{ a: 1 }>()\ndefineEmits<{ (e: 'x'): void }>()"
        ),
        ["defineProps", "defineEmits"]
    );
    assert!(macros(None, "function defineProps() {}\ndefineProps()").is_empty());
    assert!(macros(None, "import { defineProps } from './mine'\ndefineProps()").is_empty());
    assert!(macros(Some("export function defineProps() {}"), "defineProps()").is_empty());
    assert!(macros(None, "const f = (defineProps: () => void) => defineProps()").is_empty());
}
