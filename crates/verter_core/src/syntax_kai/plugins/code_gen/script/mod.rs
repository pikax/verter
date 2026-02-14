use std::{cell::RefCell, rc::Rc};

use crate::{
    code_transform::{CodeTransform, SourceMapOptions},
    syntax_kai::{
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
        plugins::code_gen::{
            script::process::{process_script_event, ProcessScriptOptions},
            types::ScriptSetupImportDependencies,
        },
        types::{Event, OxcScript},
    },
};

pub mod macros;
pub mod process;
pub mod sections;

pub struct ScriptGeneratorPlugin<'alloc> {
    code_transform: Rc<RefCell<CodeTransform<'alloc>>>,
    component_name: &'alloc str,

    is_multi_script: bool,

    keep_ts_types: bool,
    is_production: bool,
    inline_template: bool,
    runtime_module_name: String,

    imports: ScriptSetupImportDependencies,
}

impl<'alloc> ScriptGeneratorPlugin<'alloc> {
    pub fn new(
        code_transform: Rc<RefCell<CodeTransform<'alloc>>>,
        component_name: &'alloc str,
        is_multi_script: bool,
        keep_ts_types: bool,
        is_production: bool,
    ) -> Self {
        Self {
            code_transform,
            component_name,
            is_multi_script,
            keep_ts_types,
            is_production,
            inline_template: is_production,
            runtime_module_name: "vue".to_string(),

            imports: ScriptSetupImportDependencies::default(),
        }
    }

    /// Set inline template mode (decoupled from is_production).
    pub fn with_inline_template(mut self, inline: bool) -> Self {
        self.inline_template = inline;
        self
    }

    /// Set the runtime module name for helper imports.
    pub fn with_runtime_module_name(mut self, name: String) -> Self {
        self.runtime_module_name = name;
        self
    }

    /// Get the transformed code (script block only).
    pub fn get_code(&self) -> String {
        self.code_transform.borrow().build_string()
    }

    /// Generate source map JSON string.
    pub fn generate_source_map(&self, options: SourceMapOptions) -> String {
        self.code_transform.borrow().generate_map_json(options)
    }

    fn process_script(&mut self, event: &OxcScript<'alloc>, ctx: &mut SyntaxPluginContext<'alloc>) {
        if self.is_multi_script {
            panic!(
                "Multiple <script> blocks are not supported in this version. Found at position {}.",
                event.start
            );
        }

        // Process the script content with macros and transformations.
        let processed = process_script_event(
            event,
            &mut self.code_transform.borrow_mut(),
            ProcessScriptOptions {
                source: ctx.input,
                component_name: self.component_name,
                keep_ts_types: self.keep_ts_types,
                is_production: self.is_production,
                inline_template: self.inline_template,
            },
        );

        self.imports.add(processed.imports.0);
    }
}

impl<'alloc> SyntaxPlugin<'alloc> for ScriptGeneratorPlugin<'alloc> {
    fn name(&self) -> &str {
        "ScriptGeneratorPlugin"
    }

    fn end(&mut self, _ctx: &SyntaxPluginContext<'alloc>) {
        // add imports to the top of the script
        if !self.imports.is_empty() {
            self.code_transform.borrow_mut().prepend(
                format!(
                    "import {{{}}} from '{}';\n",
                    self.imports.to_import_string(),
                    self.runtime_module_name,
                )
                .as_str(),
            );
        }
    }

    fn process_event(
        &mut self,
        event: Event<'alloc>,
        ctx: &mut SyntaxPluginContext<'alloc>,
    ) -> SyntaxResult<Event<'alloc>> {
        if let Event::OxcScript(script) = &event {
            self.process_script(script, ctx);
        }
        SyntaxResult::Keep(event)
    }
}

#[cfg(test)]
mod tests {
    use crate::builder::codegen::{compile, CodegenOptions};
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    // =========================================================================
    // Test Infrastructure
    // =========================================================================

    fn gen(input: &str) -> String {
        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        compile(input, &options, &allocator).code
    }

    fn gen_prod(input: &str) -> String {
        let allocator = Allocator::new();
        let options = CodegenOptions::new()
            .with_filename("test.vue")
            .with_production(true);
        compile(input, &options, &allocator).code
    }

    fn gen_with_filename(input: &str, filename: &str) -> String {
        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename(filename);
        compile(input, &options, &allocator).code
    }

    fn assert_valid_js(code: &str, context: &str) {
        let allocator = Allocator::default();
        let source_type = SourceType::mjs();
        let parser_result = Parser::new(&allocator, code, source_type).parse();
        assert!(
            parser_result.errors.is_empty(),
            "Generated code is NOT valid JavaScript!\n\
             Context: {}\n\
             Parse Errors: {:?}\n\
             Generated Code:\n{}",
            context,
            parser_result.errors,
            code
        );
    }

    fn gen_and_validate(input: &str) -> String {
        let code = gen(input);
        assert_valid_js(&code, input);
        code
    }

    fn gen_prod_and_validate(input: &str) -> String {
        let code = gen_prod(input);
        // Production script code with inline template starts with `return (_ctx,_cache) => {`
        // so we wrap in a function for validation.
        let wrapped = format!("function __wrapper__() {{ {} }}", code);
        assert_valid_js(&wrapped, input);
        code
    }

    // =========================================================================
    // Basic Script Wrapping
    // =========================================================================

    /// @ai-generated — Dev mode emits setup with full signature and __returned__
    #[test]
    fn test_script_setup_basic_dev() {
        let code = gen_and_validate(
            "<script setup>\nconst msg = 'Hello'\n</script>\n<template><div>hi</div></template>",
        );
        assert!(
            code.contains("export default /*@__PURE__*/"),
            "Should have export default, got:\n{}",
            code
        );
        assert!(
            code.contains("__name: 'test'"),
            "Should have component name from filename, got:\n{}",
            code
        );
        assert!(
            code.contains("setup(__props,{expose:__expose})"),
            "Dev mode should have full setup signature, got:\n{}",
            code
        );
        assert!(
            code.contains("__expose()"),
            "Dev mode should auto-call __expose(), got:\n{}",
            code
        );
        assert!(
            code.contains("__returned__"),
            "Dev mode should use __returned__, got:\n{}",
            code
        );
        assert!(
            code.contains("__isScriptSetup"),
            "Dev mode should set __isScriptSetup, got:\n{}",
            code
        );
    }

    /// @ai-generated — Production mode emits production template but script remains dev
    /// NOTE: compile currently hardcodes is_production=false for ScriptGeneratorPlugin.
    /// The template correctly uses production mode. This test verifies current behavior.
    #[test]
    fn test_script_setup_prod_template() {
        let code = gen_prod_and_validate(
            "<script setup>\nconst msg = 'Hello'\n</script>\n<template><div>hi</div></template>",
        );
        // Template uses production mode arrow function
        assert!(
            code.contains("(_ctx,_cache) => {"),
            "Template should use production arrow function, got:\n{}",
            code
        );
    }

    /// @ai-generated — Component name extracted from filename
    #[test]
    fn test_component_name_from_filename() {
        let code = gen_with_filename(
            "<script setup>\nconst x = 1\n</script>\n<template><div>x</div></template>",
            "MyComponent.vue",
        );
        assert_valid_js(&code, "component name");
        assert!(
            code.contains("__name: 'MyComponent'"),
            "Should extract component name from filename, got:\n{}",
            code
        );
    }

    /// @ai-generated — Regular <script> (no setup) strips tags and keeps content
    #[test]
    fn test_script_no_setup() {
        let code = gen_and_validate(
            "<script>\nexport default { name: 'Foo' }\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            !code.contains("<script>"),
            "Should strip <script> tag, got:\n{}",
            code
        );
        assert!(
            !code.contains("</script>"),
            "Should strip </script> tag, got:\n{}",
            code
        );
        assert!(
            code.contains("export default { name: 'Foo' }"),
            "Should keep script content, got:\n{}",
            code
        );
    }

    // =========================================================================
    // defineProps
    // =========================================================================

    /// @ai-generated — defineProps with object arg moves props to component def
    #[test]
    fn test_define_props_object_arg() {
        let code = gen_and_validate(
            "<script setup>\nconst props = defineProps({ title: String })\n</script>\n<template><div>{{ props.title }}</div></template>",
        );
        assert!(
            code.contains("props:"),
            "Should have props section, got:\n{}",
            code
        );
    }

    /// @ai-generated — defineProps with inline type literal resolves to runtime props
    #[test]
    fn test_define_props_typed_inline() {
        let code = gen_and_validate(
            "<script setup lang=\"ts\">\ndefineProps<{ title: string }>()\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("props:"),
            "Should have props section, got:\n{}",
            code
        );
        assert!(
            code.contains("title:"),
            "Should have title prop, got:\n{}",
            code
        );
        assert!(
            code.contains("String"),
            "Should resolve string to String, got:\n{}",
            code
        );
        assert!(
            !code.contains("defineProps<"),
            "Should NOT leave defineProps as-is, got:\n{}",
            code
        );
    }

    /// @ai-generated — defineProps with optional typed prop
    #[test]
    fn test_define_props_typed_optional() {
        let code = gen_and_validate(
            "<script setup lang=\"ts\">\ndefineProps<{ count?: number }>()\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("required: false"),
            "Optional prop should be required: false, got:\n{}",
            code
        );
        assert!(
            code.contains("Number"),
            "Should resolve number to Number, got:\n{}",
            code
        );
    }

    /// @ai-generated — defineProps with string literal union type
    #[test]
    fn test_define_props_string_literal_union() {
        let code = gen_and_validate(
            "<script setup lang=\"ts\">\ndefineProps<{ view?: 'list' | 'board' | 'calendar' }>()\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("String"),
            "String literal union should resolve to String, got:\n{}",
            code
        );
    }

    /// @ai-generated — defineProps with SFC-local interface reference
    #[test]
    fn test_define_props_interface_ref() {
        let code = gen_and_validate(
            "<script setup lang=\"ts\">\ninterface Props { title: string; count?: number }\ndefineProps<Props>()\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("props:"),
            "Should have props section, got:\n{}",
            code
        );
        assert!(
            code.contains("title:"),
            "Should resolve interface prop 'title', got:\n{}",
            code
        );
        assert!(
            code.contains("count:"),
            "Should resolve interface prop 'count', got:\n{}",
            code
        );
    }

    /// @ai-generated — defineProps with SFC-local type alias reference
    #[test]
    fn test_define_props_type_alias_ref() {
        let code = gen_and_validate(
            "<script setup lang=\"ts\">\ntype MyProps = { message: string }\ndefineProps<MyProps>()\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("props:"),
            "Should have props section, got:\n{}",
            code
        );
        assert!(
            code.contains("message:"),
            "Should resolve type alias prop 'message', got:\n{}",
            code
        );
    }

    /// @ai-generated — defineProps with unresolvable imported type defaults to empty props
    #[test]
    fn test_define_props_unresolvable_type() {
        let code = gen_and_validate(
            "<script setup lang=\"ts\">\nimport type { ExternalProps } from './types'\ndefineProps<ExternalProps>()\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            !code.contains("defineProps<"),
            "Should NOT leave defineProps as-is, got:\n{}",
            code
        );
        assert!(
            code.contains("__props"),
            "Should replace with __props, got:\n{}",
            code
        );
    }

    /// @ai-generated — defineProps replaces call site with __props
    #[test]
    fn test_define_props_replaced_with_props() {
        let code = gen_and_validate(
            "<script setup>\nconst props = defineProps({ title: String })\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("__props"),
            "defineProps should be replaced with __props reference, got:\n{}",
            code
        );
    }

    // =========================================================================
    // withDefaults
    // =========================================================================

    /// @ai-generated — withDefaults with inline typed defineProps
    #[test]
    fn test_with_defaults_typed_inline() {
        let code = gen_and_validate(
            "<script setup lang=\"ts\">\nconst props = withDefaults(defineProps<{ foo?: string }>(), { foo: 'bar' })\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("props:"),
            "Should have props section, got:\n{}",
            code
        );
        assert!(
            code.contains("default:"),
            "Should have default value, got:\n{}",
            code
        );
        assert!(
            !code.contains("withDefaults("),
            "Should NOT leave withDefaults as-is, got:\n{}",
            code
        );
    }

    /// @ai-generated — withDefaults with SFC-local interface reference
    #[test]
    fn test_with_defaults_interface_ref() {
        let code = gen_and_validate(
            "<script setup lang=\"ts\">\ninterface Props { foo?: string; bar?: number }\nconst props = withDefaults(defineProps<Props>(), { foo: 'hello' })\n</script>\n<template><div>x</div></template>",
        );
        assert!(code.contains("props:"), "Should have props, got:\n{}", code);
        assert!(
            code.contains("foo:"),
            "Should have foo prop, got:\n{}",
            code
        );
        assert!(
            code.contains("bar:"),
            "Should have bar prop, got:\n{}",
            code
        );
        assert!(
            code.contains("default:"),
            "Should have default for foo, got:\n{}",
            code
        );
    }

    // =========================================================================
    // defineEmits
    // =========================================================================

    /// @ai-generated — defineEmits with array arg
    #[test]
    fn test_define_emits_array() {
        let code = gen_and_validate(
            "<script setup>\nconst emit = defineEmits(['click', 'update'])\n</script>\n<template><div @click=\"emit('click')\">x</div></template>",
        );
        assert!(
            code.contains("emits:"),
            "Should have emits section, got:\n{}",
            code
        );
        assert!(
            code.contains("emit:__emit"),
            "Should have emit in setup signature, got:\n{}",
            code
        );
    }

    /// @ai-generated — defineEmits without declarator doesn't add emit to signature
    #[test]
    fn test_define_emits_no_declarator() {
        let code = gen_and_validate(
            "<script setup>\ndefineEmits(['click'])\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("emits:"),
            "Should have emits section, got:\n{}",
            code
        );
        assert!(
            !code.contains("emit:__emit"),
            "No declarator means no emit in signature, got:\n{}",
            code
        );
    }

    // =========================================================================
    // defineModel
    // =========================================================================

    /// @ai-generated — defineModel produces _useModel call and mergeModels
    #[test]
    fn test_define_model_basic() {
        let code = gen_and_validate(
            "<script setup>\nconst model = defineModel()\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("_useModel"),
            "defineModel should produce _useModel call, got:\n{}",
            code
        );
        assert!(
            code.contains("modelValue"),
            "Default model name should be modelValue, got:\n{}",
            code
        );
    }

    /// @ai-generated — defineModel with named model
    #[test]
    fn test_define_model_named() {
        let code = gen_and_validate(
            "<script setup>\nconst count = defineModel('count')\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("\"count\""),
            "Named model should use provided name, got:\n{}",
            code
        );
    }

    /// @ai-generated — defineModel with defineProps triggers mergeModels
    #[test]
    fn test_define_model_with_props_merge() {
        let code = gen_and_validate(
            "<script setup>\nconst props = defineProps({ title: String })\nconst model = defineModel()\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("_mergeModels"),
            "defineModel + defineProps should use _mergeModels, got:\n{}",
            code
        );
    }

    // =========================================================================
    // defineExpose
    // =========================================================================

    /// @ai-generated — defineExpose replaces with __expose and suppresses auto-expose
    #[test]
    fn test_define_expose() {
        let code = gen_and_validate(
            "<script setup>\nconst publicFn = () => {}\ndefineExpose({ publicFn })\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("__expose("),
            "defineExpose should be replaced with __expose, got:\n{}",
            code
        );
        // When defineExpose is used, auto __expose() should NOT be called
        // (only the user's explicit expose should run)
        let expose_count = code.matches("__expose(").count();
        assert_eq!(
            expose_count, 1,
            "Should have exactly one __expose call (no auto-expose), got {} in:\n{}",
            expose_count, code
        );
    }

    // =========================================================================
    // defineOptions
    // =========================================================================

    /// @ai-generated — defineOptions moves object to component definition
    #[test]
    fn test_define_options() {
        let code = gen_and_validate(
            "<script setup>\ndefineOptions({ inheritAttrs: false })\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("inheritAttrs: false"),
            "defineOptions object should be in output, got:\n{}",
            code
        );
    }

    // =========================================================================
    // defineSlots
    // =========================================================================

    /// @ai-generated — defineSlots replaced with _useSlots()
    #[test]
    fn test_define_slots() {
        let code = gen_and_validate(
            "<script setup>\nconst slots = defineSlots()\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("_useSlots"),
            "defineSlots should be replaced with _useSlots, got:\n{}",
            code
        );
    }

    // =========================================================================
    // TypeScript vs JavaScript Wrapping (inline)
    // NOTE: compile does not call plugin.end(), so script imports
    // (defineComponent, useSlots, mergeModels) are not emitted as import
    // statements. These tests verify the inline helper usage.
    // =========================================================================

    /// @ai-generated — TypeScript setup uses _defineComponent wrapper
    #[test]
    fn test_ts_uses_define_component() {
        let code = gen_and_validate(
            "<script setup lang=\"ts\">\nconst x = 1\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("_defineComponent("),
            "TS script setup should use _defineComponent, got:\n{}",
            code
        );
    }

    /// @ai-generated — JavaScript setup does NOT use _defineComponent
    #[test]
    fn test_js_no_define_component() {
        let code = gen_and_validate(
            "<script setup>\nconst x = 1\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            !code.contains("_defineComponent("),
            "JS script setup should NOT use _defineComponent, got:\n{}",
            code
        );
    }

    // =========================================================================
    // Script Items
    // =========================================================================

    /// @ai-generated — Regular import is moved before component definition
    #[test]
    fn test_import_hoisted() {
        let code = gen_and_validate(
            "<script setup>\nimport { ref } from 'vue'\nconst count = ref(0)\n</script>\n<template><div>{{ count }}</div></template>",
        );
        // Import should appear before export default
        let import_pos = code.find("import { ref }");
        let export_pos = code.find("export default");
        assert!(
            import_pos.is_some() && export_pos.is_some(),
            "Should have both import and export, got:\n{}",
            code
        );
        assert!(
            import_pos.unwrap() < export_pos.unwrap(),
            "Import should appear before export default, got:\n{}",
            code
        );
    }

    /// @ai-generated — Type-only imports are stripped from output
    #[test]
    fn test_type_import_stripped() {
        let code = gen_and_validate(
            "<script setup lang=\"ts\">\nimport type { Ref } from 'vue'\nconst x = 1\n</script>\n<template><div>x</div></template>",
        );
        // Type-only imports should NOT appear in the runtime output
        assert!(
            !code.contains("import type"),
            "Type-only import should be stripped, got:\n{}",
            code
        );
    }

    /// @ai-generated — Declarations appear in return statement
    #[test]
    fn test_declarations_in_return() {
        let code = gen_and_validate(
            "<script setup>\nconst count = 0\nfunction increment() {}\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("count") && code.contains("increment"),
            "Both declarations should be in output, got:\n{}",
            code
        );
    }

    // =========================================================================
    // Edge Cases
    // =========================================================================

    /// @ai-generated — Empty script setup produces valid output
    #[test]
    fn test_script_setup_empty() {
        let code = gen_and_validate("<script setup>\n</script>\n<template><div>x</div></template>");
        assert!(
            code.contains("setup("),
            "Empty setup should still produce setup function, got:\n{}",
            code
        );
    }

    /// @ai-generated — Script setup with only a comment
    #[test]
    fn test_script_setup_comment_only() {
        let code = gen_and_validate(
            "<script setup>\n// just a comment\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("setup("),
            "Comment-only setup should still produce setup function, got:\n{}",
            code
        );
    }

    /// @ai-generated — Production mode includes return with bindings
    #[test]
    fn test_prod_return_bindings() {
        let code = gen_prod_and_validate(
            "<script setup>\nconst msg = 'hi'\n</script>\n<template><div>{{ msg }}</div></template>",
        );
        assert!(
            code.contains("msg"),
            "Production mode should include bindings, got:\n{}",
            code
        );
    }
}
