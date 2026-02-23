//! Vue SFC script parsing module.
//!
//! This module provides parsing for Vue Single File Component script blocks,
//! supporting both Options API (`<script>`) and Script Setup (`<script setup>`).
//!
//! # Example
//!
//! ```ignore
//! use verter_core::syntax::plugins::analysis::plugins::script::{
//!     parse_script, ScriptMode, ScriptParseResult,
//! };
//!
//! let result = parse_script(program, ScriptMode::Setup, base_offset, source);
//! ```

// This module is WIP - allow dead code for now
#![allow(dead_code)]
#![allow(unused_imports)]

pub mod bindings;
pub mod macros;
pub mod options;
pub mod resolve_type;
pub mod setup;
pub mod shared;
pub mod types;
pub mod usage;

use oxc_ast::ast::Program;

pub use macros::{
    detect_macro_kind, is_define_component, MacroArrayArg, MacroDeclarator, MacroObjectArg,
    MacroProperty, MacroTypeParams, ScriptMacro, VueMacroKind,
};
pub use resolve_type::{
    build_type_context, extract_companion_types, format_runtime_types, resolve_type_elements,
    resolve_type_elements_with_ctx, resolve_type_elements_with_ctx_ref, DiagnosticLocation,
    ResolutionDiagnostic, ResolutionDiagnosticKind, ResolvedElements, ResolvedEmit, RuntimeType,
    TypeResolutionContext,
};
pub use shared::ScriptParseContext;
pub use types::*;
pub use usage::{
    detect_vue_api_call,
    // Template usage types
    BindingRefContext,
    BindingRefInfo,
    // Sync context tracking (getCurrentInstance safety)
    CallSiteContext,
    ComponentUsageInfo,
    // Loop & render performance tracking
    ConditionLikelihood,
    EmitCallUsage,
    EmitEventName,
    FileUsageFlags,
    InjectUsage,
    IterableType,
    LifecycleHook,
    LifecycleUsage,
    LoopChildren,
    LoopInfo,
    ProvideKey,
    ProvideKeyKind,
    ProvideUsage,
    ReactiveKind,
    ReactiveStateUsage,
    RenderPatternWarning,
    SlotDefinitionInfo,
    SlotName,
    SlotUsageInfo,
    StaticConditionValue,
    SyncContextUsage,
    TemplateMetrics,
    TemplateRefAttrUsage,
    TemplateUsageCollector,
    TemplateUtilUsage,
    UsageCollector,
    VueApiCategory,
    VueApiKind,
    WarningSeverity,
    WatcherUsage,
};

use options::{process_options_statements, OptionsContext};
use setup::{process_setup_statements, SetupContext};
use shared::{try_process_export, try_process_import};

/// Parse a Vue SFC script block.
///
/// This is the main entry point for script parsing. It walks the already-parsed
/// OXC Program AST and extracts relevant information based on the script mode.
///
/// # Arguments
///
/// * `program` - The OXC-parsed program AST
/// * `mode` - The script mode (Options or Setup)
/// * `content_offset` - The byte offset for unadjusted TypeScript type annotation spans.
///   In the `syntax` pipeline this is `content_start` (where script content begins in the SFC).
///   In direct parsing (tests), this is 0 since spans are already local.
/// * `source` - The source text of the script content
///
/// # Returns
///
/// A `ScriptParseResult` containing all extracted items, async status, and errors.
pub fn parse_script<'a>(
    program: &Program<'a>,
    mode: ScriptMode,
    content_offset: u32,
    source: &'a str,
) -> ScriptParseResult<'a> {
    parse_script_with_companion(program, mode, content_offset, source, None)
}

/// Parse a script program with optional companion type information.
///
/// When `companion_types` is `Some`, type references in `defineProps<T>()`
/// that can't be resolved from the setup script's own declarations will
/// fall back to these pre-resolved types from the companion `<script>` block.
pub fn parse_script_with_companion<'a>(
    program: &Program<'a>,
    mode: ScriptMode,
    content_offset: u32,
    source: &'a str,
    companion_types: Option<rustc_hash::FxHashMap<String, resolve_type::ResolvedElements>>,
) -> ScriptParseResult<'a> {
    let ctx = ScriptParseContext::new(content_offset, source.as_bytes());
    let mut items = Vec::new();
    let mut errors = Vec::new();
    let mut is_async = false;

    // First pass: collect imports and exports (shared across modes)
    for stmt in &program.body {
        if let Some(import_item) = try_process_import(stmt, &ctx) {
            items.push(import_item);
        }
        if let Some(export_item) = try_process_export(stmt, &ctx) {
            items.push(export_item);
        }
    }

    // Second pass: mode-specific processing
    match mode {
        ScriptMode::Setup => {
            let mut type_ctx = build_type_context(program, source.as_bytes(), content_offset);
            if let Some(companion) = companion_types {
                type_ctx.companion_types = companion;
            }
            let mut setup_ctx = SetupContext::new();
            process_setup_statements(
                &program.body,
                &ctx,
                &type_ctx,
                &mut setup_ctx,
                &mut items,
                &mut errors,
            );
            is_async = setup_ctx.is_async;
        }
        ScriptMode::Options => {
            let mut options_ctx = OptionsContext::new();
            process_options_statements(
                &program.body,
                &ctx,
                &mut options_ctx,
                &mut items,
                &mut errors,
                &mut is_async,
            );
        }
    }

    // Extract binding metadata (only for script setup)
    let bindings = match mode {
        ScriptMode::Setup => bindings::extract_bindings(program, &ctx),
        ScriptMode::Options => Vec::new(),
    };

    ScriptParseResult {
        is_async,
        items,
        errors,
        bindings,
    }
}

#[cfg(test)]
mod tests {
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
        let await_text =
            &source[async_items[0].span.start as usize..async_items[0].span.end as usize];
        println!("Detected async span text: '{}'", await_text);
        assert!(await_text.contains("await"), "Span should contain 'await'");
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
}
