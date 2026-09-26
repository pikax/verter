use super::*;
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;

#[test]
fn test_parse_setup_with_import() {
    let source = r#"import { ref } from 'vue';
const count = ref(0);"#;
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let ret = Parser::new(&allocator, source, source_type).parse();

    let result = parse_script(&ret.program, ScriptMode::Setup, 0, source);

    // Should have import + declaration
    let imports: Vec<_> = result
        .items
        .iter()
        .filter(|i| matches!(i, ScriptItem::Import(_)))
        .collect();
    let decls: Vec<_> = result
        .items
        .iter()
        .filter(|i| matches!(i, ScriptItem::Declaration(_)))
        .collect();

    assert_eq!(imports.len(), 1);
    assert_eq!(decls.len(), 1);
}

#[test]
fn test_parse_setup_with_macro() {
    let source = r#"const props = defineProps<{ msg: string }>();"#;
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let ret = Parser::new(&allocator, source, source_type).parse();

    let result = parse_script(&ret.program, ScriptMode::Setup, 0, source);

    let macros: Vec<_> = result
        .items
        .iter()
        .filter(|i| matches!(i, ScriptItem::Macro(_)))
        .collect();

    assert_eq!(macros.len(), 1);
    if let ScriptItem::Macro(m) = macros[0] {
        assert_eq!(m.kind(), VueMacroKind::DefineProps);
        assert!(matches!(
            m,
            ScriptMacro::DefineProps {
                type_params: Some(_),
                ..
            }
        ));
    }
}

#[test]
fn define_model_name_is_decoded_while_span_remains_the_authored_literal() {
    let source = r#"const model = defineModel('foo\nbar')"#;
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
    let result = parse_script(&parsed.program, ScriptMode::Setup, 0, source);

    let (name, name_span) = result
        .items
        .iter()
        .find_map(|item| match item {
            ScriptItem::Macro(ScriptMacro::DefineModel {
                name, name_span, ..
            }) => Some((*name, *name_span)),
            _ => None,
        })
        .expect("defineModel macro");

    assert_eq!(name, Some("foo\nbar"));
    let name_span = name_span.expect("authored name span");
    assert_eq!(
        &source[name_span.start as usize..name_span.end as usize],
        r#"'foo\nbar'"#
    );
}

fn with_defaults_object(source: &str, check: impl FnOnce(&str, &MacroObjectArg<'_>)) {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
    let result = parse_script(&parsed.program, ScriptMode::Setup, 0, source);
    let defaults = result
        .items
        .iter()
        .find_map(|item| match item {
            ScriptItem::Macro(ScriptMacro::WithDefaults {
                defaults: Some(defaults),
                ..
            }) => Some(defaults),
            _ => None,
        })
        .expect("withDefaults object");
    check(source, defaults);
}

#[test]
fn macro_object_static_eligibility_and_full_property_spans_are_parser_facts() {
    with_defaults_object(
        "withDefaults(defineProps<{ foo?: number; bar?: number }>(), { foo: 1, ['bar']: 2 })",
        |source, object| {
            assert_eq!(
                object.static_eligibility,
                MacroObjectStaticEligibility::Eligible
            );
            assert!(object.static_eligibility.is_eligible());
            assert_eq!(
                object
                    .properties
                    .iter()
                    .map(|property| property.name)
                    .collect::<Vec<_>>(),
                ["foo", "bar"]
            );
            assert_eq!(
                object
                    .properties
                    .iter()
                    .map(|property| &source[property.property_span.start as usize
                        ..property.property_span.end as usize])
                    .collect::<Vec<_>>(),
                ["foo: 1", "['bar']: 2"]
            );
        },
    );
}

#[test]
fn macro_property_span_includes_method_prefix_and_body() {
    with_defaults_object(
        "withDefaults(defineProps<{ run?: () => number }>(), { async run() { return 1 } })",
        |source, object| {
            let property = object.properties.first().expect("run method");
            assert!(property.is_method);
            assert_eq!(
                &source[property.property_span.start as usize..property.property_span.end as usize],
                "async run() { return 1 }"
            );
        },
    );
}

#[test]
fn macro_object_static_eligibility_distinguishes_every_unsupported_key_or_spread_shape() {
    let cases = [
        (
            "withDefaults(defineProps<{}>(), { [key]: 1 })",
            MacroObjectStaticEligibility::ContainsUnsupportedKey,
        ),
        (
            "withDefaults(defineProps<{}>(), { ...defaults })",
            MacroObjectStaticEligibility::ContainsSpread,
        ),
        (
            "withDefaults(defineProps<{}>(), { 1: 'one', ...defaults })",
            MacroObjectStaticEligibility::ContainsSpreadAndUnsupportedKey,
        ),
    ];

    for (source, expected) in cases {
        with_defaults_object(source, |_, object| {
            assert_eq!(object.static_eligibility, expected, "{source}");
            assert!(!object.static_eligibility.is_eligible(), "{source}");
        });
    }
}

#[test]
fn macro_dependency_paths_come_from_typed_syntax_not_comment_or_literal_text() {
    let source = r#"
const typed = defineProps<{ actual: Réel; literal: 'Phantom' /* Phantom */ }>()
const runtime = defineProps({ actual: Object as PropType<Réel> })
const emit = defineEmits({ change: (value: Réel /* Phantom */) => true })
"#;
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
    let result = parse_script(&parsed.program, ScriptMode::Setup, 0, source);
    let macros = result
        .items
        .iter()
        .filter_map(|item| match item {
            ScriptItem::Macro(mac) => Some(mac),
            _ => None,
        })
        .collect::<Vec<_>>();

    let ScriptMacro::DefineProps {
        type_params: Some(type_params),
        ..
    } = macros[0]
    else {
        panic!("expected typed props macro");
    };
    assert_eq!(
        type_params
            .type_dependency_paths
            .iter()
            .map(|path| path.root())
            .collect::<Vec<_>>(),
        ["Réel"]
    );

    for mac in &macros[1..] {
        let properties = match mac {
            ScriptMacro::DefineProps {
                object_arg: Some(object),
                ..
            }
            | ScriptMacro::DefineEmits {
                object_arg: Some(object),
                ..
            } => &object.properties,
            _ => panic!("expected object macro"),
        };
        assert_eq!(
            properties[0]
                .type_dependency_paths
                .iter()
                .map(|path| path.root())
                .collect::<Vec<_>>(),
            ["Réel"]
        );
    }
}

#[test]
fn test_parse_setup_with_async() {
    let source = r#"const data = await fetch('/api');"#;
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let ret = Parser::new(&allocator, source, source_type).parse();

    let result = parse_script(&ret.program, ScriptMode::Setup, 0, source);

    assert!(result.is_async);
    let async_items: Vec<_> = result
        .items
        .iter()
        .filter(|i| matches!(i, ScriptItem::Async(_)))
        .collect();
    assert!(!async_items.is_empty());
}

#[test]
fn test_parse_setup_with_top_level_await_expression() {
    // Test top-level await as an expression statement
    let source = r#"await Promise.resolve();"#;
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let ret = Parser::new(&allocator, source, source_type).parse();

    let result = parse_script(&ret.program, ScriptMode::Setup, 0, source);

    assert!(
        result.is_async,
        "is_async should be true for top-level await"
    );
    let async_items: Vec<_> = result
        .items
        .iter()
        .filter(|i| matches!(i, ScriptItem::Async(_)))
        .collect();
    assert_eq!(
        async_items.len(),
        1,
        "Should have exactly one ScriptAsync item"
    );
}

#[test]
fn test_parse_setup_with_multiple_awaits() {
    let source = r#"
const a = await fetch('/api/a');
const b = await fetch('/api/b');
await processResults(a, b);
"#;
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let ret = Parser::new(&allocator, source, source_type).parse();

    let result = parse_script(&ret.program, ScriptMode::Setup, 0, source);

    assert!(result.is_async);
    let async_items: Vec<_> = result
        .items
        .iter()
        .filter(|i| matches!(i, ScriptItem::Async(_)))
        .collect();
    assert_eq!(
        async_items.len(),
        3,
        "Should detect all three await expressions"
    );
}

#[test]
fn test_parse_setup_with_await_using() {
    // await using is ES2024 syntax for async disposal
    let source = r#"await using resource = getAsyncResource();"#;
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let ret = Parser::new(&allocator, source, source_type).parse();

    let result = parse_script(&ret.program, ScriptMode::Setup, 0, source);

    assert!(result.is_async, "is_async should be true for await using");
    let async_items: Vec<_> = result
        .items
        .iter()
        .filter(|i| matches!(i, ScriptItem::Async(_)))
        .collect();
    assert!(
        !async_items.is_empty(),
        "Should have ScriptAsync item for await using"
    );
}

#[test]
fn test_parse_setup_with_for_await_of() {
    let source = r#"
for await (const item of asyncIterable) {
    console.log(item);
}
"#;
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let ret = Parser::new(&allocator, source, source_type).parse();

    let result = parse_script(&ret.program, ScriptMode::Setup, 0, source);

    assert!(
        result.is_async,
        "is_async should be true for for await...of"
    );
    let async_items: Vec<_> = result
        .items
        .iter()
        .filter(|i| matches!(i, ScriptItem::Async(_)))
        .collect();
    assert!(
        !async_items.is_empty(),
        "Should have ScriptAsync item for for await...of"
    );
}

#[test]
fn test_parse_setup_with_nested_await() {
    // Await nested in call expression arguments
    let source = r#"process(await fetch('/api'));"#;
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let ret = Parser::new(&allocator, source, source_type).parse();

    let result = parse_script(&ret.program, ScriptMode::Setup, 0, source);

    assert!(result.is_async, "is_async should be true for nested await");
    let async_items: Vec<_> = result
        .items
        .iter()
        .filter(|i| matches!(i, ScriptItem::Async(_)))
        .collect();
    assert!(
        !async_items.is_empty(),
        "Should detect await in call argument"
    );
}

#[test]
fn test_parse_setup_with_await_in_binary_expression() {
    let source = r#"const result = (await fetchA()) + (await fetchB());"#;
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let ret = Parser::new(&allocator, source, source_type).parse();

    let result = parse_script(&ret.program, ScriptMode::Setup, 0, source);

    assert!(result.is_async);
    let async_items: Vec<_> = result
        .items
        .iter()
        .filter(|i| matches!(i, ScriptItem::Async(_)))
        .collect();
    assert_eq!(
        async_items.len(),
        2,
        "Should detect both awaits in binary expression"
    );
}

#[test]
fn test_parse_setup_with_await_in_conditional() {
    let source = r#"const result = condition ? await fetchA() : await fetchB();"#;
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let ret = Parser::new(&allocator, source, source_type).parse();

    let result = parse_script(&ret.program, ScriptMode::Setup, 0, source);

    assert!(result.is_async);
    let async_items: Vec<_> = result
        .items
        .iter()
        .filter(|i| matches!(i, ScriptItem::Async(_)))
        .collect();
    assert_eq!(
        async_items.len(),
        2,
        "Should detect both awaits in conditional expression"
    );
}

#[test]
fn test_parse_setup_await_inside_async_function_not_counted() {
    // Await inside an async function should NOT make the script setup async
    // Only top-level awaits should count
    let source = r#"
async function fetchData() {
    return await fetch('/api');
}
const handler = async () => {
    await something();
};
"#;
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let ret = Parser::new(&allocator, source, source_type).parse();

    let result = parse_script(&ret.program, ScriptMode::Setup, 0, source);

    assert!(
        !result.is_async,
        "Awaits inside async functions should not make script setup async"
    );
    let async_items: Vec<_> = result
        .items
        .iter()
        .filter(|i| matches!(i, ScriptItem::Async(_)))
        .collect();
    assert!(
        async_items.is_empty(),
        "Should not have ScriptAsync items for nested async functions"
    );
}

#[test]
fn test_parse_setup_async_span_is_correct() {
    let source = r#"await fetch('/api');"#;
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let ret = Parser::new(&allocator, source, source_type).parse();

    let result = parse_script(&ret.program, ScriptMode::Setup, 0, source);

    let async_items: Vec<_> = result
        .items
        .iter()
        .filter_map(|i| {
            if let ScriptItem::Async(a) = i {
                Some(a)
            } else {
                None
            }
        })
        .collect();

    assert_eq!(async_items.len(), 1);
    // Spans are in local coordinates (OXC output); adjust_program_spans handles SFC offset in production
    assert_eq!(async_items[0].span.start, 0);
    assert!(async_items[0].span.end > async_items[0].span.start);
}

#[test]
fn test_parse_setup_with_await_in_if_condition() {
    let source = r#"
if (await checkCondition()) {
    doSomething();
}
"#;
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let ret = Parser::new(&allocator, source, source_type).parse();

    let result = parse_script(&ret.program, ScriptMode::Setup, 0, source);

    assert!(
        result.is_async,
        "is_async should be true for await in if condition"
    );
    let async_items: Vec<_> = result
        .items
        .iter()
        .filter(|i| matches!(i, ScriptItem::Async(_)))
        .collect();
    assert!(
        !async_items.is_empty(),
        "Should detect await in if condition"
    );
}

#[test]
fn test_parse_setup_real_world_async_script() {
    // This is the exact script from the user's Vue SFC
    let source = r#"import { ref } from "vue";

const foo = ref("");

await Promise.resolve();

async () => {
  await Promise.resolve();
};

let a = {};"#;
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let ret = Parser::new(&allocator, source, source_type).parse();

    let result = parse_script(&ret.program, ScriptMode::Setup, 0, source);

    // Should detect the top-level await, but NOT the one inside the async arrow function
    assert!(
        result.is_async,
        "is_async should be true - there's a top-level await"
    );

    let async_items: Vec<_> = result
        .items
        .iter()
        .filter_map(|i| {
            if let ScriptItem::Async(a) = i {
                Some(a)
            } else {
                None
            }
        })
        .collect();

    // Should have exactly 1 async item (the top-level await Promise.resolve())
    // The await inside async () => {} should NOT be counted
    assert_eq!(
        async_items.len(),
        1,
        "Should have exactly 1 ScriptAsync item for top-level await"
    );

    // Verify the span points to the correct location
    let await_text = &source[async_items[0].span.start as usize..async_items[0].span.end as usize];
    println!("Detected async span text: '{}'", await_text);
    assert!(await_text.contains("await"), "Span should contain 'await'");
}

#[test]
fn test_parse_setup_with_await_in_ts_non_null_assertion() {
    // `(await foo())!` — the TS non-null assertion wraps the await in TSNonNullExpression
    let source = r#"const item = (await getById(id))!"#;
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let ret = Parser::new(&allocator, source, source_type).parse();

    let result = parse_script(&ret.program, ScriptMode::Setup, 0, source);

    assert!(
        result.is_async,
        "is_async should be true for await inside TS non-null assertion"
    );
}

#[test]
fn test_parse_setup_with_await_in_ts_as_expression() {
    // `(await foo()) as T` — TSAsExpression wraps the await
    let source = r#"const item = (await getById(id)) as string"#;
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let ret = Parser::new(&allocator, source, source_type).parse();

    let result = parse_script(&ret.program, ScriptMode::Setup, 0, source);

    assert!(
        result.is_async,
        "is_async should be true for await inside TS as expression"
    );
}

#[test]
fn test_parse_options_with_define_component() {
    let source = r#"import { defineComponent } from 'vue';
export default defineComponent({
    setup() {
        return {};
    }
});"#;
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let ret = Parser::new(&allocator, source, source_type).parse();

    let result = parse_script(&ret.program, ScriptMode::Options, 0, source);

    let default_exports: Vec<_> = result
        .items
        .iter()
        .filter_map(|i| {
            if let ScriptItem::DefaultExport(e) = i {
                Some(e)
            } else {
                None
            }
        })
        .collect();

    assert_eq!(default_exports.len(), 1);
    assert_eq!(
        default_exports[0].export_type,
        DefaultExportType::DefineComponent
    );
    assert!(default_exports[0].setup_body_span.is_some());
}

#[test]
fn test_parse_options_plain_object() {
    let source = r#"export default {
    setup() {
        return {};
    }
};"#;
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let ret = Parser::new(&allocator, source, source_type).parse();

    let result = parse_script(&ret.program, ScriptMode::Options, 0, source);

    let default_exports: Vec<_> = result
        .items
        .iter()
        .filter_map(|i| {
            if let ScriptItem::DefaultExport(e) = i {
                Some(e)
            } else {
                None
            }
        })
        .collect();

    assert_eq!(default_exports.len(), 1);
    assert_eq!(default_exports[0].export_type, DefaultExportType::Object);
}

#[test]
fn test_detect_macro_kind() {
    assert_eq!(
        detect_macro_kind(b"defineProps"),
        Some(VueMacroKind::DefineProps)
    );
    assert_eq!(
        detect_macro_kind(b"defineEmits"),
        Some(VueMacroKind::DefineEmits)
    );
    assert_eq!(
        detect_macro_kind(b"withDefaults"),
        Some(VueMacroKind::WithDefaults)
    );
    assert_eq!(detect_macro_kind(b"ref"), None);
}

#[test]
fn test_statement_spans_are_local() {
    // Statement/expression spans come directly from OXC (in production, they're
    // pre-adjusted by adjust_program_spans). parse_script does NOT adjust them.
    let source = r#"const x = 1;"#;
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let ret = Parser::new(&allocator, source, source_type).parse();

    let result = parse_script(&ret.program, ScriptMode::Setup, 0, source);

    let decls: Vec<_> = result
        .items
        .iter()
        .filter_map(|i| {
            if let ScriptItem::Declaration(d) = i {
                Some(d)
            } else {
                None
            }
        })
        .collect();

    assert_eq!(decls.len(), 1);
    // 'x' is at position 6 in the source
    assert_eq!(decls[0].span.start, 6);
    assert_eq!(decls[0].span.end, 7);
}

#[test]
fn test_get_current_instance_detection() {
    assert_eq!(
        detect_vue_api_call(b"getCurrentInstance"),
        Some(VueApiKind::GetCurrentInstance)
    );
    // Category should be InstanceAccess
    assert_eq!(
        VueApiKind::GetCurrentInstance.category(),
        VueApiCategory::InstanceAccess
    );
    // Should require sync context
    assert!(VueApiKind::GetCurrentInstance.requires_sync_context());
}

#[test]
fn test_get_current_instance_before_await() {
    use setup::check_expression_for_usage;
    use shared::ScriptParseContext;
    use usage::{CallSiteContext, UsageCollector};

    let source = r#"const instance = getCurrentInstance();"#;
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let ret = Parser::new(&allocator, source, source_type).parse();

    let ctx = ScriptParseContext::new(0, source.as_bytes());
    let mut collector = UsageCollector::new(source.as_bytes());

    // Process the variable declaration
    for stmt in &ret.program.body {
        if let oxc_ast::ast::Statement::VariableDeclaration(var_decl) = stmt {
            for declarator in &var_decl.declarations {
                if let Some(init) = &declarator.init {
                    let binding_span = if let oxc_ast::ast::BindingPattern::BindingIdentifier(id) =
                        &declarator.id
                    {
                        Some(crate::common::Span::new(id.span.start, id.span.end))
                    } else {
                        None
                    };
                    check_expression_for_usage(init, &ctx, &mut collector, binding_span);
                }
            }
        }
    }

    // Should have detected getCurrentInstance
    assert_eq!(collector.sync_context_usages.len(), 1);
    let usage = &collector.sync_context_usages[0];
    assert_eq!(usage.kind, VueApiKind::GetCurrentInstance);
    // Should be before await (no await in this source)
    assert_eq!(usage.context, CallSiteContext::BeforeAwait);
    assert!(usage.is_safe());
}

#[test]
fn test_get_current_instance_after_await() {
    use setup::check_expression_for_usage;
    use shared::ScriptParseContext;
    use usage::{CallSiteContext, UsageCollector};

    // getCurrentInstance after an await
    let source = r#"
const data = await fetch('/api');
const instance = getCurrentInstance();
"#;
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let ret = Parser::new(&allocator, source, source_type).parse();

    let ctx = ScriptParseContext::new(0, source.as_bytes());
    let mut collector = UsageCollector::new(source.as_bytes());

    // Process all statements
    for stmt in &ret.program.body {
        match stmt {
            oxc_ast::ast::Statement::VariableDeclaration(var_decl) => {
                for declarator in &var_decl.declarations {
                    if let Some(init) = &declarator.init {
                        let binding_span =
                            if let oxc_ast::ast::BindingPattern::BindingIdentifier(id) =
                                &declarator.id
                            {
                                Some(crate::common::Span::new(id.span.start, id.span.end))
                            } else {
                                None
                            };
                        check_expression_for_usage(init, &ctx, &mut collector, binding_span);
                    }
                }
            }
            oxc_ast::ast::Statement::ExpressionStatement(expr) => {
                check_expression_for_usage(&expr.expression, &ctx, &mut collector, None);
            }
            _ => {}
        }
    }

    // Should have detected getCurrentInstance
    assert_eq!(
        collector.sync_context_usages.len(),
        1,
        "Should have one getCurrentInstance call"
    );
    let usage = &collector.sync_context_usages[0];
    assert_eq!(usage.kind, VueApiKind::GetCurrentInstance);
    // Should be after await
    assert_eq!(usage.context, CallSiteContext::AfterAwait);
    assert!(usage.is_potentially_unsafe());
    // Should have the preceding await span
    assert!(usage.preceding_await_span.is_some());
}

#[test]
fn array_arg_element_names_come_from_ast_literal() {
    // Prop / event names are read from the string-literal AST value, transparently
    // unwrapping a TS `as` / `satisfies` / non-null wrapper. Dynamic and
    // non-string-literal elements name nothing — never a span-sliced fragment.
    let source = r#"defineProps(['foo' as const, dynamic, `tpl`, 'bar'])"#;
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::tsx()).parse();
    let result = parse_script(&ret.program, ScriptMode::Setup, 0, source);

    let arr = result
        .items
        .iter()
        .find_map(|i| match i {
            ScriptItem::Macro(ScriptMacro::DefineProps {
                array_arg: Some(arr),
                ..
            }) => Some(arr),
            _ => None,
        })
        .expect("defineProps array argument");

    let names: Vec<Option<&str>> = arr.elements.iter().map(|e| e.name).collect();
    assert_eq!(
        names,
        vec![
            Some("foo"), // `'foo' as const` → unwrapped to the literal name
            None,        // `dynamic` identifier → names nothing
            None,        // template literal → not a string-literal prop name
            Some("bar"), // plain string literal
        ],
        "array element names must come from the AST literal, not span slicing"
    );
}
