use super::script_setup::*;
use crate::cursor::ScriptLanguage;

fn block(content: &str, start: u32, lang: Option<ScriptLanguage>) -> ScriptBlockInput<'_> {
    ScriptBlockInput {
        content,
        content_start: start,
        lang,
    }
}

fn ts(content: &str, start: u32) -> ScriptBlockInput<'_> {
    block(content, start, Some(ScriptLanguage::TypeScript))
}

fn setup_of(content: &str) -> TsSetupProjection {
    project_script_pair(None, Some(ts(content, 0)), None)
        .expect("projects")
        .setup
        .expect("setup")
}

#[test]
fn stp11_universal_binder_keeps_constraints_and_carries_no_arguments() {
    let generic = "T extends { id: number }, U = string";
    let facts =
        project_script_pair(None, Some(ts("const a = 1", 0)), Some(generic)).expect("projects");
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
    let facts =
        project_script_pair(Some(ts(normal, 10)), Some(ts(setup, 500)), None).expect("projects");
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
fn stp11_normal_script_exports_distinguish_type_only_and_wildcard_re_exports() {
    let normal = concat!(
        "export type A = 1\n",
        "export const shared = 1\n",
        "export * from './z'\n",
        "export * as ns from './w'\n",
    );
    let facts = project_script_pair(Some(ts(normal, 0)), None, None).expect("projects");
    assert_eq!(facts.module.named_exports, ["shared", "ns"]);
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
fn stp11_class_computed_key_await_is_top_level() {
    let computed = setup_of("class C { [await k]() {} }");
    assert!(computed.checking_wrapper_is_async());

    // A method BODY await stays nested even when the key is computed.
    let body_only = setup_of("class C { [k]() { return async () => { await g() } } }");
    assert!(!body_only.checking_wrapper_is_async());
}

#[test]
fn stp11_assertion_ts_angle_assertion_and_tsx_element_parse_once_under_own_grammar() {
    let result = project_script_pair(None, Some(ts("const n = <number>raw", 0)), None)
        .expect("angle assertion parses under the TypeScript grammar");
    assert_eq!(
        result.setup.expect("setup").grammar,
        ScriptGrammar::TypeScript
    );

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
        project_script_pair(None, Some(ts("const el = <div>{a}</div>", 0)), None).unwrap_err(),
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
fn stp11_absent_lang_is_javascript_not_typescript() {
    // Vue's own default for an unlabeled `<script setup>` is JavaScript
    // (`sfc_script_dialect`); this projection only owns TypeScript/TSX.
    assert_eq!(
        project_script_pair(None, Some(block("const n = 1", 0, None)), None).unwrap_err(),
        SetupProjectionRefusal::NotTypeScript
    );
}

#[test]
fn stp11_mismatched_script_lang_pair_is_refused() {
    let normal = ts("export const shared = 1", 0);
    let setup = block("const n = 1", 0, Some(ScriptLanguage::TSX));
    assert_eq!(
        project_script_pair(Some(normal), Some(setup), None).unwrap_err(),
        SetupProjectionRefusal::ScriptLangConflict
    );
}

#[test]
fn stp11_one_body_setup_statements_appear_once() {
    let facts = project_script_pair(
        None,
        Some(ts(
            "const a = 1
const b = 2",
            0,
        )),
        None,
    )
    .unwrap();
    let setup = facts.setup.expect("setup");
    assert_eq!(setup.statements.len(), 2);
}

#[test]
fn stp11_macro_requires_macro_position() {
    let macros = |setup: &str| -> Vec<&'static str> {
        project_script_pair(None, Some(ts(setup, 0)), None)
            .unwrap()
            .setup
            .unwrap()
            .macros
            .iter()
            .map(|m| m.name)
            .collect()
    };
    assert_eq!(macros("const p = defineProps<{a: 1}>()"), ["defineProps"]);
    assert_eq!(
        macros("const p = withDefaults(defineProps<{a: 1}>(), {})"),
        ["withDefaults", "defineProps"]
    );
    assert!(macros("foo(defineProps())").is_empty());
    assert!(macros("const p = cond ? defineProps() : 1").is_empty());
}

#[test]
fn stp11_await_in_class_heritage_is_top_level() {
    let facts =
        project_script_pair(None, Some(ts("class A extends (await base()) {}", 0)), None).unwrap();
    assert!(facts.setup.unwrap().checking_wrapper_is_async());
}

#[test]
fn stp11_macro_syntax_is_distinct_from_same_named_lexical_functions() {
    let macros = |normal: Option<&str>, setup: &str| -> Vec<&'static str> {
        project_script_pair(normal.map(|n| ts(n, 0)), Some(ts(setup, 0)), None)
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

#[test]
fn stp11_vue_imported_macro_is_still_recognised() {
    let macros = |normal: Option<&str>, setup: &str| -> Vec<&'static str> {
        project_script_pair(normal.map(|n| ts(n, 0)), Some(ts(setup, 0)), None)
            .expect("projects")
            .setup
            .expect("setup")
            .macros
            .iter()
            .map(|m| m.name)
            .collect()
    };
    // Vue's `compileScript` strips a macro specifier imported from `'vue'`
    // with a warning and still processes the call as the macro.
    assert_eq!(
        macros(
            None,
            "import { defineProps } from 'vue'\nconst p = defineProps<{ a: number }>()"
        ),
        ["defineProps"]
    );
    // A type-only normal-script declaration of the same name occupies only
    // the type space and must not suppress the macro.
    assert_eq!(
        macros(Some("type defineProps = 1"), "defineProps()"),
        ["defineProps"]
    );
    // A macro name imported from elsewhere is still an ordinary call.
    assert!(macros(
        None,
        "import { defineProps } from 'vue-router'\ndefineProps()"
    )
    .is_empty());
}

#[test]
fn stp11_nested_macro_call_is_not_reported() {
    let macros = setup_of("function f() { defineProps() }\nif (x) { defineEmits() }").macros;
    assert!(macros.is_empty());
}
